#!/bin/sh
# Real Docker build/receiver/storage checks with the explicitly synthetic demo.
# No provider credentials, real model calls, external comments, or public ports.
set -eu

fail() { printf 'container smoke: %s\n' "$*" >&2; exit 1; }
smoke_existing_image=
if [ "$#" -eq 2 ] && [ "$1" = --image ]; then
  smoke_existing_image=$2
elif [ "$#" -ne 0 ]; then
  fail 'usage: container-smoke.sh [--image EXISTING_IMAGE]'
fi
command -v docker >/dev/null 2>&1 || fail 'Docker is required on the test host'
command -v python3 >/dev/null 2>&1 || fail 'Python 3 is required on the test host to check JSON output'
docker info >/dev/null || fail 'Docker daemon is unavailable'

smoke_root=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
smoke_tmp=$(mktemp -d "${TMPDIR:-/tmp}/waking-container-smoke.XXXXXXXX")
smoke_run=${smoke_tmp##*/}
smoke_image=waking-container-smoke:${smoke_run##*.}
[ -z "$smoke_existing_image" ] || smoke_image=$smoke_existing_image
smoke_volume=$smoke_run-data
smoke_setup=$smoke_run-setup
smoke_demo=$smoke_run-demo
smoke_receiver=$smoke_run-receiver
smoke_recreated=$smoke_run-recreated
smoke_label=waking.container-smoke.run=$smoke_run

cleanup() {
  smoke_exit=$?
  trap - 0 HUP INT TERM
  set +e
  # Names are unique, and labels must also match before any Docker deletion.
  # Never prune the daemon, remove unrelated images, or touch a user volume.
  for cleanup_name in "$smoke_setup" "$smoke_demo" "$smoke_receiver" "$smoke_recreated"; do
    cleanup_owner=$(docker container inspect --format '{{ index .Config.Labels "waking.container-smoke.run" }}' "$cleanup_name" 2>/dev/null)
    if [ "$cleanup_owner" = "$smoke_run" ]; then
      if [ "$smoke_exit" -ne 0 ]; then
        printf '\nContainer log: %s\n' "$cleanup_name" >&2
        docker logs --tail 100 "$cleanup_name" >&2
      fi
      docker container rm --force "$cleanup_name" >/dev/null || smoke_exit=1
    fi
  done
  cleanup_owner=$(docker volume inspect --format '{{ index .Labels "waking.container-smoke.run" }}' "$smoke_volume" 2>/dev/null)
  if [ "$cleanup_owner" = "$smoke_run" ]; then
    docker volume rm "$smoke_volume" >/dev/null || smoke_exit=1
  fi
  cleanup_owner=$(docker image inspect --format '{{ index .Config.Labels "waking.container-smoke.run" }}' "$smoke_image" 2>/dev/null)
  if [ -z "$smoke_existing_image" ] && [ "$cleanup_owner" = "$smoke_run" ]; then
    docker image rm "$smoke_image" >/dev/null || smoke_exit=1
  fi
  rm -rf -- "$smoke_tmp"
  exit "$smoke_exit"
}
trap cleanup 0
trap 'exit 129' HUP
trap 'exit 130' INT
trap 'exit 143' TERM

# Python runs only on the test host, reading CLI stdout. The image needs only
# its installed waking binary; no Python, curl, or extra product dependency.
check_output() {
  python3 - "$@" <<'PY'
import json
import re
import sys
from pathlib import Path, PurePosixPath


def data(path):
    envelope = json.loads(Path(path).read_text())
    assert envelope.get("schema_version") == 1, "unsupported JSON envelope"
    assert envelope.get("ok") is True, envelope
    return envelope["data"]


mode, *paths = sys.argv[1:]
if mode == "setup":
    configured = data(paths[0])
    assert configured["data_dir"] == "/data", configured
    assert configured["service_started"] is False, configured
elif mode == "demo":
    demo = data(paths[0])
    assert demo["processed"] == 2, demo
    assert demo["idle_model_calls"] == 0, demo
    assert demo["duplicate_state"] == "duplicate", demo
    assert demo["source_calls"] == 0, demo
    assert demo["live_codex_verified"] is False, demo
    assert demo["external_replies_sent"] == 0, demo
    assert len(demo["drafts"]) == 2, demo
    assert len(set(demo["events"])) == 2, demo
    state_dir = demo["state_dir"]
    state_path = PurePosixPath(state_dir)
    assert state_path.parent == PurePosixPath("/data"), state_dir
    assert re.fullmatch(r"demo-[0-9a-f-]{36}", state_path.name), state_dir
    assert str(state_path) == state_dir, state_dir
    print(state_dir)
elif mode == "ready":
    status = data(paths[0])
    raise SystemExit(0 if status.get("daemon_running") is True else 1)
elif mode == "sqlite":
    doctor = data(paths[0])
    sqlite = doctor["sqlite"]
    assert sqlite["version_number"] >= 3_051_003, sqlite
    assert doctor["model_turns_requested_by_doctor"] == 0, doctor
    print("Container bundled SQLite:", sqlite["version"], sqlite["version_number"])
elif mode == "snapshot":
    demo, status, events, jobs, drafts = map(data, paths[:5])
    assert status["daemon_running"] is True, status
    assert status["events"] == {"drafted": 2}, status
    assert status["jobs"] == {"drafted": 2}, status
    assert status["drafts"] == {"draft": 2}, status
    assert len(events) == len(jobs) == len(drafts) == 2
    assert {entry["event"]["event_id"] for entry in events} == set(demo["events"])
    assert {job["event"]["event_id"] for job in jobs} == set(demo["events"])
    assert {draft["id"] for draft in drafts} == {draft["id"] for draft in demo["drafts"]}
    assert all(job["state"] == "drafted" for job in jobs), jobs
    assert all(draft["state"] == "draft" for draft in drafts), drafts
    # Preserve exact identities, bodies, versions, dispatch/turn IDs, and states.
    # Counting two rows alone would miss replacement or accidental re-execution.
    snapshot = {
        "events": sorted(events, key=lambda item: item["event"]["event_id"]),
        "jobs": sorted(jobs, key=lambda item: item["id"]),
        "drafts": sorted(drafts, key=lambda item: item["id"]),
    }
    baseline = Path(paths[5])
    if baseline.exists():
        assert snapshot == json.loads(baseline.read_text()), "persisted state changed after container lifecycle operation"
    else:
        baseline.write_text(json.dumps(snapshot, sort_keys=True))
else:
    raise SystemExit("unknown output check")
PY
}

container_cli() {
  cli_container=$1
  shift
  docker exec "$cli_container" /usr/local/bin/waking --data-dir "$smoke_state_dir" --json "$@"
}

wait_receiver() {
  wait_container=$1
  wait_phase=$2
  wait_attempt=0
  # A bounded readiness wait belongs to this test harness, not to the product's
  # event processing. It does not ingest an event or request a model turn.
  while [ "$wait_attempt" -lt 30 ]; do
    if container_cli "$wait_container" status > "$smoke_tmp/$wait_phase-status.json" 2> "$smoke_tmp/$wait_phase-readiness.log" &&
       check_output ready "$smoke_tmp/$wait_phase-status.json" 2>> "$smoke_tmp/$wait_phase-readiness.log"; then
      return
    fi
    wait_running=$(docker container inspect --format '{{ .State.Running }}' "$wait_container" 2>/dev/null || :)
    [ "$wait_running" = true ] || fail "$wait_phase receiver exited before becoming ready"
    wait_attempt=$((wait_attempt + 1))
    sleep 1
  done
  cat "$smoke_tmp/$wait_phase-readiness.log" >&2
  fail "$wait_phase receiver did not become ready within 30 attempts"
}

snapshot() {
  snapshot_container=$1
  snapshot_phase=$2
  wait_receiver "$snapshot_container" "$snapshot_phase"
  container_cli "$snapshot_container" events list > "$smoke_tmp/$snapshot_phase-events.json"
  container_cli "$snapshot_container" jobs list > "$smoke_tmp/$snapshot_phase-jobs.json"
  container_cli "$snapshot_container" drafts list > "$smoke_tmp/$snapshot_phase-drafts.json"
  check_output snapshot "$smoke_tmp/demo.json" "$smoke_tmp/$snapshot_phase-status.json" \
    "$smoke_tmp/$snapshot_phase-events.json" "$smoke_tmp/$snapshot_phase-jobs.json" \
    "$smoke_tmp/$snapshot_phase-drafts.json" "$smoke_tmp/baseline.json"
  printf 'Verified %s: receiver active; two original jobs and two unsent drafts persisted.\n' "$snapshot_phase"
}

start_receiver() {
  start_name=$1
  # Override the image's fixed /data argument with the demo's exact state_dir.
  # All runtime containers are offline. No ports are published to the host.
  docker run --detach --name "$start_name" --label "$smoke_label" \
    --network none --mount "type=volume,src=$smoke_volume,dst=/data" \
    --entrypoint /usr/local/bin/waking "$smoke_image" \
    --data-dir "$smoke_state_dir" serve --listen 127.0.0.1:7433 >/dev/null
}

if [ -z "$smoke_existing_image" ] && docker image inspect "$smoke_image" >/dev/null 2>&1; then
  fail 'temporary image tag unexpectedly exists'
fi
if docker volume inspect "$smoke_volume" >/dev/null 2>&1; then
  fail 'temporary volume name unexpectedly exists'
fi
if [ -z "$smoke_existing_image" ]; then
  printf 'Building deploy/Dockerfile into temporary image %s\n' "$smoke_image"
  docker build --file "$smoke_root/deploy/Dockerfile" --tag "$smoke_image" --label "$smoke_label" "$smoke_root"
else
  docker image inspect "$smoke_image" >/dev/null || fail 'supplied image is unavailable'
fi
docker volume create --label "$smoke_label" "$smoke_volume" >/dev/null

printf 'Initializing the private volume and running the explicitly mock-only demo.\n'
docker run --rm --name "$smoke_setup" --label "$smoke_label" \
  --network none --mount "type=volume,src=$smoke_volume,dst=/data" \
  "$smoke_image" --json setup > "$smoke_tmp/setup.json"
check_output setup "$smoke_tmp/setup.json"
docker run --rm --name "$smoke_setup" --label "$smoke_label" \
  --network none --mount "type=volume,src=$smoke_volume,dst=/data" \
  "$smoke_image" --json doctor > "$smoke_tmp/doctor.json"
check_output sqlite "$smoke_tmp/doctor.json"
docker run --rm --name "$smoke_demo" --label "$smoke_label" \
  --network none --mount "type=volume,src=$smoke_volume,dst=/data" \
  "$smoke_image" --json demo > "$smoke_tmp/demo.json"
smoke_state_dir=$(check_output demo "$smoke_tmp/demo.json")

start_receiver "$smoke_receiver"
snapshot "$smoke_receiver" initial
smoke_original_id=$(docker container inspect --format '{{ .Id }}' "$smoke_receiver")
docker restart --time 10 "$smoke_receiver" >/dev/null
[ "$(docker container inspect --format '{{ .Id }}' "$smoke_receiver")" = "$smoke_original_id" ] || fail 'restart changed container identity'
snapshot "$smoke_receiver" restarted

docker stop --time 10 "$smoke_receiver" >/dev/null
docker container rm "$smoke_receiver" >/dev/null
start_receiver "$smoke_recreated"
[ "$(docker container inspect --format '{{ .Id }}' "$smoke_recreated")" != "$smoke_original_id" ] || fail 'recreation reused the removed container identity'
snapshot "$smoke_recreated" recreated

printf 'Container image, receiver startup, restart, and named-volume recreation smoke passed.\n'
printf 'The demo executor is synthetic. No real Codex, provider write, public endpoint, or cloud relay E2E was verified.\n'
