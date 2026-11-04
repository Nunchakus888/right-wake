#!/usr/bin/env python3
"""Probe queue method gates in an empty Codex profile; never execute model work."""

import argparse
from datetime import datetime, timezone
import hashlib
import json
import os
from pathlib import Path
import selectors
import subprocess
import tempfile
import time


SOURCE_COMMIT = "6ff1e1ca3b30fc8b914720681fcf5fe62e95f856"
BINARY_SHA256 = "b8a11fedf24b2dfb59a70c9e52b030f9a508dcd49cd2dd0d41956f4c785156b8"
ARCHIVE_SHA256 = "a0bf572d2f93f056c6712a6da348e96078dc84f3885fd7be67807ef389cb3978"
MISSING_THREAD = "00000000-0000-4000-8000-000000000001"
CLIENT_ID = "00000000-0000-4000-8000-000000000002"
QUEUE_METHODS = ("thread/queue/list", "thread/queue/add", "thread/queue/start")


def sha256(path):
    digest = hashlib.sha256()
    with path.open("rb") as source:
        for block in iter(lambda: source.read(1024 * 1024), b""):
            digest.update(block)
    return digest.hexdigest()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--codex", type=Path, required=True)
    parser.add_argument("--archive", type=Path, help="Optional original verified Linux musl archive")
    parser.add_argument("--out", type=Path, required=True)
    args = parser.parse_args()
    binary = args.codex.resolve(strict=True)
    output = args.out.resolve()
    output.mkdir(parents=True, exist_ok=True)
    transcript = []
    report = {
        "recorded_at_utc": datetime.now(timezone.utc).isoformat(),
        "source_tag": "rust-v0.166.0",
        "source_commit": SOURCE_COMMIT,
        "release_asset": "codex-x86_64-unknown-linux-musl.tar.gz",
        "binary_sha256": sha256(binary),
        "expected_archive_sha256": ARCHIVE_SHA256,
        "archive_reverified_in_this_run": False,
        "transport": "isolated stdio",
        "model_turns_requested": 0,
        "queue_submissions_accepted": 0,
        "threads_created": 0,
        "existing_user_session_access": False,
        "sessions": [],
        "limits": [
            "Method/parser and missing-target rejection evidence only; no real queue execution.",
            "No Desktop connection, existing-session resumption, model, authentication, or tool call.",
            "Queue add/start are sent only for a verified absent ID in a fresh empty isolated store.",
        ],
    }
    assert report["binary_sha256"] == BINARY_SHA256, "Use the pinned official Linux musl binary"
    if args.archive:
        report["archive_sha256"] = sha256(args.archive.resolve(strict=True))
        assert report["archive_sha256"] == ARCHIVE_SHA256, "Archive checksum differs from the verified release asset"
        report["archive_reverified_in_this_run"] = True

    with tempfile.TemporaryDirectory(prefix="right-wake-queue-probe-") as temporary:
        workspace = Path(temporary)

        def scrub(value):
            if isinstance(value, str):
                return value.replace(str(workspace), "<isolated-probe>").replace(str(binary), "<codex-binary>")
            if isinstance(value, list):
                return [scrub(item) for item in value]
            if isinstance(value, dict):
                return {key: "<redacted-probe-metadata>" if key in {"serverName", "installationId"}
                        else scrub(item) for key, item in value.items()}
            return value

        def environment(config):
            # Configure only the child process. Never inherit credentials, a
            # user HOME, provider URLs, proxy tokens, or the caller's Codex profile.
            return {"PATH": os.defpath, "LANG": "C.UTF-8",
                    "CODEX_HOME": str(config), "RUST_LOG": "error"}

        try:
            schema_config = workspace / "schema-config"
            schema_config.mkdir()
            child_env = environment(schema_config)
            report["codex_version"] = subprocess.run(
                [str(binary), "--version"], cwd=workspace, env=child_env,
                capture_output=True, text=True, check=True, timeout=15,
            ).stdout.strip()
            assert report["codex_version"] == "codex-cli 0.166.0"
            for experimental in (False, True):
                schema = workspace / ("experimental-schema" if experimental else "stable-schema")
                command = [str(binary), "app-server", "generate-json-schema", "--out", str(schema)]
                if experimental:
                    command.append("--experimental")
                subprocess.run(command, cwd=workspace, env=child_env, capture_output=True,
                               check=True, timeout=30)
                present = (schema / "v2" / "ThreadQueueAddParams.json").exists()
                assert present == experimental, "Queue/add is expected only in the experimental schema"
                if experimental:
                    add = json.loads((schema / "v2" / "ThreadQueueAddParams.json").read_text())
                    listing = json.loads((schema / "v2" / "ThreadQueueListResponse.json").read_text())
                    report["queue_add_fields"] = sorted(add["properties"])
                    report["queue_list_response_fields"] = sorted(listing["properties"])
            report["queue_schema_experimental_only"] = True

            for experimental in (False, True):
                config = workspace / ("experimental-profile" if experimental else "stable-profile")
                project = workspace / ("experimental-project" if experimental else "stable-project")
                config.mkdir()
                project.mkdir()
                process = subprocess.Popen(
                    [str(binary), "app-server", "--listen", "stdio://", "-c", "analytics.enabled=false",
                     "-c", "check_for_update_on_startup=false"], cwd=project, env=environment(config),
                    stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.PIPE,
                )
                selector = selectors.DefaultSelector()
                selector.register(process.stdout, selectors.EVENT_READ, "stdout")
                selector.register(process.stderr, selectors.EVENT_READ, "stderr")
                buffers = {"stdout": b"", "stderr": b""}
                errors = []
                pending = []
                session = {"experimental_api": experimental, "checks": []}
                report["sessions"].append(session)

                def send(message):
                    method = message["method"]
                    assert method in {"initialize", "initialized", "thread/list", "thread/read", *QUEUE_METHODS}
                    if method in {"thread/read", *QUEUE_METHODS}:
                        assert message["params"]["threadId"] == MISSING_THREAD
                    transcript.append({"experimental_api": experimental, "direction": "request", "message": message})
                    process.stdin.write(json.dumps(message).encode() + b"\n")
                    process.stdin.flush()

                def receive(expected):
                    deadline = time.monotonic() + 15
                    while time.monotonic() < deadline:
                        while pending:
                            message = pending.pop(0)
                            # Never retain unexpected private contents if storage
                            # isolation is not honored by a managed environment.
                            if message.get("id") in {2, 7} and message.get("result", {}).get("data"):
                                raise RuntimeError("isolated thread store is not empty; contents discarded")
                            transcript.append({"experimental_api": experimental, "direction": "response", "message": message})
                            if message.get("id") == expected:
                                return message
                        for key, _ in selector.select(max(0, deadline - time.monotonic())):
                            data = os.read(key.fileobj.fileno(), 65536)
                            if not data:
                                selector.unregister(key.fileobj)
                                continue
                            channel = key.data
                            buffers[channel] += data
                            if len(buffers[channel]) > 4 * 1024 * 1024:
                                raise RuntimeError("probe output exceeds four MiB")
                            while b"\n" in buffers[channel]:
                                line, buffers[channel] = buffers[channel].split(b"\n", 1)
                                if channel == "stderr":
                                    errors.append(line.decode(errors="replace"))
                                    errors[:] = errors[-8:]
                                else:
                                    pending.append(json.loads(line))
                        if process.poll() is not None and not pending:
                            raise RuntimeError(f"app-server exited: {process.returncode}")
                    raise TimeoutError(f"no response for request {expected}")

                try:
                    send({"id": 1, "method": "initialize", "params": {
                        "clientInfo": {"name": "right_wake_queue_probe", "version": "0.3.0"},
                        "capabilities": {"experimentalApi": experimental}}})
                    initialized = receive(1)
                    assert initialized.get("result", {}).get("codexHome") == str(config)
                    send({"method": "initialized", "params": {}})
                    query = {"limit": 10, "modelProviders": [], "sourceKinds": [], "archived": False}
                    send({"id": 2, "method": "thread/list", "params": query})
                    assert receive(2).get("result", {}).get("data") == []
                    send({"id": 3, "method": "thread/read", "params": {"threadId": MISSING_THREAD}})
                    assert "error" in receive(3)
                    session["checks"].append("isolated store empty; synthetic target is absent")
                    for request_id, method in enumerate(QUEUE_METHODS, start=4):
                        params = {"threadId": MISSING_THREAD}
                        if method == "thread/queue/add":
                            params.update({"clientUserMessageId": CLIENT_ID,
                                           "input": [{"type": "text", "text": "Absent-target protocol probe; no work authorized.", "text_elements": []}]})
                        send({"id": request_id, "method": method, "params": params})
                        error = receive(request_id).get("error", {})
                        expected_code = -32603 if experimental else -32600
                        expected_text = "no rollout found for thread id" if experimental else "requires experimentalApi capability"
                        assert error.get("code") == expected_code and expected_text in error.get("message", ""), error
                        session["checks"].append({"method": method, "error_code": expected_code,
                                                  "reason": "missing_target" if experimental else "experimental_capability_required"})
                    send({"id": 7, "method": "thread/list", "params": query})
                    assert receive(7).get("result", {}).get("data") == []
                    session["checks"].append("isolated store remains empty; no fallback thread")
                finally:
                    process.stdin.close()
                    try:
                        process.wait(timeout=3)
                    except subprocess.TimeoutExpired:
                        process.terminate()
                        try:
                            process.wait(timeout=3)
                        except subprocess.TimeoutExpired:
                            process.kill()
                            process.wait()
                    selector.close()
                    session["process_exit_code"] = process.returncode
                    session["stderr_tail"] = errors
            report["status"] = "passed"
        except Exception as error:
            report["status"] = "blocked"
            report["error"] = str(error)
        finally:
            report = scrub(report)
            (output / "probe-report.json").write_text(json.dumps(report, indent=2) + "\n")
            (output / "probe-transcript.json").write_text(json.dumps(scrub(transcript), indent=2) + "\n")
    print(json.dumps(report, indent=2))
    return 0 if report["status"] == "passed" else 1


if __name__ == "__main__":
    raise SystemExit(main())
