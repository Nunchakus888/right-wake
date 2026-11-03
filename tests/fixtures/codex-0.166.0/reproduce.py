#!/usr/bin/env python3
"""Isolated, credential-free Codex protocol probe. Never submits a model turn."""

import argparse
import json
import os
from pathlib import Path
import selectors
import subprocess
import tempfile
import time


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--codex", type=Path, required=True, help="Absolute Codex executable path")
    parser.add_argument("--out", type=Path, required=True, help="Directory for sanitized evidence")
    args = parser.parse_args()
    binary = args.codex.resolve(strict=True)
    output = args.out.resolve()
    output.mkdir(parents=True, exist_ok=True)
    transcript = []
    report = {
        "transport": "isolated stdio",
        "model_turns_sent": 0,
        "private_session_access": False,
        "checks": [],
        "redactions": ["temporary configuration path", "host name", "installation ID"],
    }

    with tempfile.TemporaryDirectory(prefix="waking-codex-probe-") as temporary:
        workspace = Path(temporary)
        config = workspace / "isolated-config"
        project = workspace / "empty-project"
        schemas = workspace / "schema"
        config.mkdir()
        project.mkdir()
        # This is Codex's documented configuration directory for this child
        # process only, not a change to the caller's environment or profiles.
        # Deliberately do not inherit account/API keys or the existing home.
        child_environment = {
            "PATH": os.defpath,
            "LANG": "C.UTF-8",
            "CODEX_HOME": str(config),
            "RUST_LOG": "error",
        }
        version = subprocess.run(
            [str(binary), "--version"], cwd=project, env=child_environment,
            capture_output=True, text=True, check=True, timeout=15,
        ).stdout.strip()
        report["codex_version"] = version
        subprocess.run(
            [str(binary), "app-server", "generate-json-schema", "--out", str(schemas)],
            cwd=project, env=child_environment, capture_output=True, check=True, timeout=30,
        )
        turn_schema = json.loads((schemas / "v2" / "TurnStartParams.json").read_text())
        report["turn_start_fields"] = sorted(turn_schema["properties"])
        report["tool_output_fields"] = sorted(turn_schema["definitions"]["TurnToolOutput"]["properties"])
        report["checks"].append("installed binary generated its stable protocol schema")

        def scrub(value):
            if isinstance(value, str):
                return value.replace(str(workspace), "<probe-workspace>").replace(str(binary), "<codex-binary>")
            if isinstance(value, list):
                return [scrub(item) for item in value]
            if isinstance(value, dict):
                return {
                    key: "<redacted-probe-metadata>" if key in {"serverName", "installationId"} else scrub(item)
                    for key, item in value.items()
                }
            return value

        process = subprocess.Popen(
            [str(binary), "app-server", "--listen", "stdio://", "-c", "analytics.enabled=false",
             "-c", "check_for_update_on_startup=false"],
            cwd=project, env=child_environment, stdin=subprocess.PIPE,
            stdout=subprocess.PIPE, stderr=subprocess.PIPE,
        )
        selector = selectors.DefaultSelector()
        selector.register(process.stdout, selectors.EVENT_READ, "stdout")
        selector.register(process.stderr, selectors.EVENT_READ, "stderr")
        buffers = {"stdout": b"", "stderr": b""}
        errors = []

        def send(message):
            assert message["method"] not in {"thread/start", "thread/fork", "turn/start", "turn/steer"}
            transcript.append({"direction": "request", "message": message})
            process.stdin.write(json.dumps(message).encode() + b"\n")
            process.stdin.flush()

        def reply(expected, seconds=15):
            deadline = time.monotonic() + seconds
            while time.monotonic() < deadline:
                for key, _ in selector.select(max(0, deadline - time.monotonic())):
                    data = os.read(key.fileobj.fileno(), 65536)
                    if not data:
                        selector.unregister(key.fileobj)
                        continue
                    channel = key.data
                    buffers[channel] += data
                    if len(buffers[channel]) > 4 * 1024 * 1024:
                        raise RuntimeError("probe output exceeded its bounded buffer")
                    while b"\n" in buffers[channel]:
                        line, buffers[channel] = buffers[channel].split(b"\n", 1)
                        if channel == "stderr":
                            errors.append(line.decode(errors="replace"))
                            errors[:] = errors[-8:]
                            continue
                        message = json.loads(line)
                        # A nonempty thread list is unexpected. Do not record
                        # its contents, even if a managed host changed storage.
                        if message.get("id") in {2, 5} and message.get("result", {}).get("data"):
                            raise RuntimeError("isolated thread store is not empty; contents were not retained")
                        transcript.append({"direction": "response", "message": message})
                        if message.get("id") == expected:
                            return message
                if process.poll() is not None:
                    raise RuntimeError(f"app-server exited with status {process.returncode}")
            raise TimeoutError(f"no response for request {expected}")

        try:
            send({"id": 1, "method": "initialize", "params": {
                "clientInfo": {"name": "waking_schema_probe", "version": "0.1.0"},
                "capabilities": {"experimentalApi": False},
            }})
            initialized = reply(1)
            assert "result" in initialized, initialized
            returned_home = initialized["result"].get("codexHome")
            assert returned_home == str(config), "host did not use the requested isolated configuration"
            report["checks"].append("initialize accepted with the expected isolated configuration path")
            send({"method": "initialized", "params": {}})
            query = {"limit": 10, "modelProviders": [], "sourceKinds": ["cli", "vscode", "exec", "appServer"], "archived": False}
            send({"id": 2, "method": "thread/list", "params": query})
            assert reply(2).get("result", {}).get("data") == []
            report["checks"].append("fresh isolated thread list is empty")
            absent = "00000000-0000-4000-8000-000000000001"
            send({"id": 3, "method": "thread/read", "params": {"threadId": absent}})
            assert "error" in reply(3)
            report["checks"].append("missing thread/read rejected")
            send({"id": 4, "method": "thread/resume", "params": {"threadId": absent, "excludeTurns": True}})
            assert "error" in reply(4)
            report["checks"].append("missing thread/resume rejected without fallback")
            send({"id": 5, "method": "thread/list", "params": query})
            assert reply(5).get("result", {}).get("data") == []
            report["checks"].append("no new thread after rejected resume")
            report["status"] = "passed"
        except Exception as error:
            report["status"] = "blocked"
            report["error"] = str(error)
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
            report["process_exit_code"] = process.returncode
            report["stderr_tail"] = errors
            report = scrub(report)
            transcript = scrub(transcript)
            (output / "smoke-report.json").write_text(json.dumps(report, indent=2) + "\n")
            (output / "smoke-transcript.json").write_text(json.dumps(transcript, indent=2) + "\n")
    print(json.dumps(report, indent=2))
    return 0 if report.get("status") == "passed" else 1


if __name__ == "__main__":
    raise SystemExit(main())
