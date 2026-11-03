#!/bin/sh
# Exercise the shipped Compose file with synthetic signed ingress on loopback.
set -eu
fail() { printf 'compose smoke: %s\n' "$*" >&2; exit 1; }
[ "$#" -eq 1 ] || fail 'usage: compose-smoke.sh EXISTING_IMAGE'
image=$1
root=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
tmp=$(mktemp -d "${TMPDIR:-/tmp}/waking-compose-smoke.XXXXXXXX")
project=$(printf '%s' "${tmp##*/}" | tr '[:upper:]' '[:lower:]' | tr '.' '-')
export WAKING_PORT=0
compose() {
  docker compose --project-name "$project" --project-directory "$tmp" \
    -f "$tmp/compose.yaml" "$@"
}
cleanup() {
  result=$?
  trap - 0 HUP INT TERM
  set +e
  if [ -f "$tmp/compose.yaml" ]; then
    [ "$result" -eq 0 ] || compose logs --tail 100 >&2
    # Only this random, pre-checked test project owns these volumes/containers.
    compose down --volumes --timeout 10 >/dev/null || result=1
  fi
  rm -rf -- "$tmp"
  exit "$result"
}
trap cleanup 0
trap 'exit 129' HUP
trap 'exit 130' INT
trap 'exit 143' TERM
[ -z "$(docker container ls -aq --filter "label=com.docker.compose.project=$project")" ] || fail 'test project container collision'
[ -z "$(docker volume ls -q --filter "label=com.docker.compose.project=$project")" ] || fail 'test project volume collision'
cp "$root/deploy/compose.yaml" "$tmp/compose.yaml"
printf 'WAKING_COMPOSE_SMOKE_SECRET=synthetic-compose-secret-never-a-provider-credential\n' > "$tmp/private.env"
chmod 600 "$tmp/private.env"
compose config --format json > "$tmp/resolved.json"
python3 - "$tmp/resolved.json" "$image" <<'PY'
import json, sys
from pathlib import Path
cfg = json.loads(Path(sys.argv[1]).read_text())
service = cfg["services"]["waking"]
assert service["image"] == sys.argv[2], service
assert service["pull_policy"] == "never", service
assert service["restart"] == "unless-stopped", service
assert service["command"] == ["relay", "serve", "--listen", "0.0.0.0:7433"], service
assert len(service["ports"]) == 1 and service["ports"][0]["host_ip"] == "127.0.0.1", service
assert service["ports"][0]["target"] == 7433, service
assert any(v["type"] == "volume" and v["target"] == "/data" for v in service["volumes"]), service
PY
compose run --rm -T waking --json setup > "$tmp/setup.json"
compose run --rm -T waking --json source add --id container --kind linear \
  --tenant compose-smoke --secret-env WAKING_COMPOSE_SMOKE_SECRET > "$tmp/source.json"
compose up --detach

ready() {
  container=$(compose ps --quiet waking)
  [ -n "$container" ] || fail 'Compose service is absent'
  docker container inspect "$container" > "$tmp/container.json"
  port=$(python3 - "$tmp/container.json" <<'PY'
import json, sys
from pathlib import Path
container = json.loads(Path(sys.argv[1]).read_text())[0]
assert container["Config"]["Cmd"] == ["relay", "serve", "--listen", "0.0.0.0:7433"], container["Config"]["Cmd"]
assert container["Config"]["User"] == "10001:10001", container["Config"]["User"]
ports = container["NetworkSettings"]["Ports"]["7433/tcp"]
assert len(ports) == 1 and ports[0]["HostIp"] == "127.0.0.1", ports
print(ports[0]["HostPort"])
PY
  )
  python3 - "$port" <<'PY'
import json, sys, time, urllib.request
url = "http://127.0.0.1:" + sys.argv[1] + "/health"
opener = urllib.request.build_opener(urllib.request.ProxyHandler({}))
for attempt in range(30):
    try:
        with opener.open(url, timeout=1) as response:
            result = json.load(response)
        assert result["ok"] and result["data"]["receiving"], result
        break
    except (OSError, AssertionError):
        if attempt == 29:
            raise
        time.sleep(1)
PY
}
ready
python3 - "$port" "$tmp/receipt.json" <<'PY'
import hashlib, hmac, json, sys, time, urllib.request
from pathlib import Path
body = json.dumps({"type": "Comment", "action": "create", "organizationId": "compose-smoke",
                   "webhookTimestamp": int(time.time() * 1000),
                   "data": {"id": "compose-comment", "issueId": "compose-issue",
                            "body": "@waking synthetic container persistence check", "user": {"id": "fixture"}}}).encode()
signature = hmac.new(b"synthetic-compose-secret-never-a-provider-credential", body, hashlib.sha256).hexdigest()
request = urllib.request.Request("http://127.0.0.1:" + sys.argv[1] + "/hooks/container", body,
                                 {"Content-Type": "application/json", "Linear-Signature": signature,
                                  "Linear-Delivery": "compose-delivery"})
opener = urllib.request.build_opener(urllib.request.ProxyHandler({}))
with opener.open(request, timeout=5) as response:
    receipt = json.load(response)
assert receipt["ok"] and receipt["data"]["state"] == "received", receipt
assert receipt["data"]["model_completed"] is False, receipt
Path(sys.argv[2]).write_text(json.dumps(receipt))
PY

snapshot() {
  compose exec -T waking /usr/local/bin/waking --data-dir /data --json status > "$tmp/status.json"
  compose exec -T waking /usr/local/bin/waking --data-dir /data --json events list > "$tmp/events.json"
  python3 - "$tmp" <<'PY'
import json, sys
from pathlib import Path
root = Path(sys.argv[1])
def data(name):
    envelope = json.loads((root / name).read_text())
    assert envelope["ok"] and envelope["schema_version"] == 1, envelope
    return envelope["data"]
status, events, receipt = data("status.json"), data("events.json"), data("receipt.json")
assert status["daemon_running"] and status["events"] == {"received": 1}, status
assert status["jobs"] == {} and status["drafts"] == {}, status
assert len(events) == 1 and events[0]["event"]["event_id"] == receipt["event_id"], events
baseline = root / "baseline.json"
if baseline.exists():
    assert events == json.loads(baseline.read_text()), "pending relay event changed across container replacement"
else:
    baseline.write_text(json.dumps(events))
PY
}
snapshot
original=$(compose ps --quiet waking)
compose restart waking
ready
[ "$(compose ps --quiet waking)" = "$original" ] || fail 'restart replaced the container'
snapshot
compose down --timeout 10
compose up --detach
ready
[ "$(compose ps --quiet waking)" != "$original" ] || fail 'recreation reused the old container'
snapshot
printf 'Compose relay startup, signed loopback receipt, restart, and pending-event volume persistence passed.\n'
printf 'This fixture made no real provider request, model call, external reply, or public relay connection.\n'
