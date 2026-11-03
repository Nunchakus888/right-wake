#!/usr/bin/env python3
"""Installer boundary tests with a fake Docker daemon; never claim an image ran."""
import hashlib
import json
import os
from pathlib import Path
import subprocess
import tempfile
import unittest


INSTALLER = Path(__file__).resolve().with_name("install-container.sh")
DOCKER = r'''#!/usr/bin/env python3
import json, os, sys
from pathlib import Path
args = sys.argv[1:]
with open(os.environ["FAKE_CALLS"], "a") as log:
    log.write(json.dumps(args) + "\n")
state = Path(os.environ["FAKE_STATE"])
platform = os.environ.get("FAKE_PLATFORM", "linux/amd64")
if args == ["version", "--format", "{{.Server.Os}}/{{.Server.Arch}}"]:
    if os.environ.get("FAKE_OFFLINE"):
        sys.exit(1)
    print(platform)
elif args[:2] == ["image", "inspect"]:
    if not state.exists() and not os.environ.get("FAKE_EXISTING"):
        sys.exit(1)
    print(os.environ.get("FAKE_METADATA", platform + " 0.1.1 https://github.com/Nunchakus888/waking"))
elif args[:2] == ["image", "load"]:
    assert args[2] == "--input" and Path(args[3]).is_file(), args
    if os.environ.get("FAKE_LOAD_FAIL"):
        sys.exit(1)
    state.write_text("loaded")
    print("Loaded image: " + os.environ.get("FAKE_LOADED_TAG", "waking:0.1.1"))
else:
    raise SystemExit("unexpected Docker operation: " + repr(args))
'''
CURL = r'''#!/usr/bin/env python3
import json, os, shutil, sys
from pathlib import Path
args = sys.argv[1:]
with open(os.environ["FAKE_URLS"], "a") as log:
    log.write(json.dumps(args) + "\n")
assert args[-1].startswith("https://github.com/Nunchakus888/waking/releases/download/v0.1.1/"), args
if os.environ.get("FAKE_DOWNLOAD_FAIL"):
    raise SystemExit(22)
destination = args[args.index("--output") + 1]
shutil.copyfile(Path(os.environ["FAKE_DOWNLOADS"]) / args[-1].rsplit("/", 1)[1], destination)
'''


class InstallerContract(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory(prefix="waking-installer-test-")
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        self.bin = self.root / "fake-bin"
        self.source = self.root / "release files"
        self.work = self.root / "work"
        for path in (self.bin, self.source, self.work):
            path.mkdir()
        for name, code in (("docker", DOCKER), ("curl", CURL),
                           ("uname", "#!/bin/sh\nexit 99\n")):
            path = self.bin / name
            path.write_text(code)
            path.chmod(0o755)
        self.calls = self.root / "calls.jsonl"
        self.env = dict(os.environ, PATH=str(self.bin) + os.pathsep + os.environ["PATH"],
                        TMPDIR=str(self.work), WAKING_VERSION="v0.1.1",
                        FAKE_CALLS=str(self.calls), FAKE_STATE=str(self.root / "loaded"),
                        FAKE_URLS=str(self.root / "urls.jsonl"), FAKE_DOWNLOADS=str(self.source))
        self.assets = {}
        lines = []
        for arch in ("amd64", "arm64"):
            asset = self.source / f"waking-v0.1.1-container-linux-{arch}.tar.gz"
            asset.write_bytes(b"mock image payload for " + arch.encode())
            self.assets[arch] = asset
            lines.append(hashlib.sha256(asset.read_bytes()).hexdigest() + "  " + asset.name)
        self.manifest = self.source / "SHA256SUMS"
        self.manifest.write_text("\n".join(lines) + "\n")

    def run_install(self, *, success=True, download=False, args=(), **env):
        command = ["sh", str(INSTALLER)]
        if not download:
            command += ["--from-dir", str(self.source)]
        result = subprocess.run(command + list(args), env=dict(self.env, **env),
                                text=True, capture_output=True, timeout=10)
        self.assertEqual(result.returncode == 0, success, result.stdout + result.stderr)
        self.assertEqual(list(self.work.iterdir()), [], "installer leaked its temporary files")
        calls = [json.loads(line) for line in self.calls.read_text().splitlines()] if self.calls.exists() else []
        self.assertTrue(all(call[:2] in (["version", "--format"], ["image", "inspect"], ["image", "load"])
                            for call in calls), calls)
        return result, calls

    def test_amd64_uses_daemon_and_loads_without_starting(self):
        result, calls = self.run_install()
        loads = [call for call in calls if call[:2] == ["image", "load"]]
        self.assertEqual(len(loads), 1)
        self.assertEqual(Path(loads[0][-1]).name, self.assets["amd64"].name)
        self.assertIn("Loaded waking:0.1.1 for linux/amd64", result.stdout)

    def test_arm64_uses_daemon_instead_of_client_uname(self):
        _, calls = self.run_install(FAKE_PLATFORM="linux/arm64")
        load = next(call for call in calls if call[:2] == ["image", "load"])
        self.assertEqual(Path(load[-1]).name, self.assets["arm64"].name)

    def test_checksum_mismatch_never_loads(self):
        self.assets["amd64"].write_bytes(b"corrupt payload")
        result, calls = self.run_install(success=False)
        self.assertIn("checksum mismatch", result.stderr)
        self.assertFalse(any(call[:2] == ["image", "load"] for call in calls))

    def test_ambiguous_checksum_never_loads(self):
        self.manifest.write_text(self.manifest.read_text() * 2)
        result, calls = self.run_install(success=False)
        self.assertIn("ambiguous", result.stderr)
        self.assertFalse(any(call[:2] == ["image", "load"] for call in calls))

    def test_missing_checksum_never_loads(self):
        self.manifest.write_text("")
        _, calls = self.run_install(success=False)
        self.assertFalse(any(call[:2] == ["image", "load"] for call in calls))

    def test_unsupported_daemon_platform_fails_before_load(self):
        result, calls = self.run_install(success=False, FAKE_PLATFORM="windows/amd64")
        self.assertIn("unsupported Docker server platform", result.stderr)
        self.assertEqual(len(calls), 1)

    def test_offline_daemon_fails_before_download(self):
        self.run_install(success=False, download=True, FAKE_OFFLINE="1")
        self.assertFalse((self.root / "urls.jsonl").exists())

    def test_unrelated_existing_tag_is_not_replaced(self):
        result, calls = self.run_install(success=False, FAKE_EXISTING="1", FAKE_METADATA="unrelated")
        self.assertIn("refusing to replace", result.stderr)
        self.assertFalse(any(call[:2] == ["image", "load"] for call in calls))

    def test_loaded_metadata_mismatch_is_not_success(self):
        result, _ = self.run_install(success=False, FAKE_METADATA="linux/arm64 0.1.1 wrong-source")
        self.assertIn("loaded image metadata differs", result.stderr)

    def test_wrong_archive_tag_is_not_hidden_by_existing_image(self):
        result, _ = self.run_install(success=False, FAKE_EXISTING="1", FAKE_LOADED_TAG="unrelated:latest")
        self.assertIn("archive did not report the expected tag", result.stderr)

    def test_failed_docker_load_is_not_success(self):
        result, _ = self.run_install(success=False, FAKE_LOAD_FAIL="1")
        self.assertIn("could not load", result.stderr)

    def test_pinned_https_downloads_then_verifies_and_loads(self):
        self.run_install(download=True)
        calls = [json.loads(line) for line in (self.root / "urls.jsonl").read_text().splitlines()]
        self.assertEqual(len(calls), 2)
        self.assertEqual({call[-1].rsplit("/", 1)[1] for call in calls},
                         {self.assets["amd64"].name, "SHA256SUMS"})
        self.assertTrue(all(call[call.index("--proto-redir") + 1] == "=https" for call in calls))

    def test_failed_download_never_loads(self):
        result, calls = self.run_install(success=False, download=True, FAKE_DOWNLOAD_FAIL="1")
        self.assertIn("download failed", result.stderr)
        self.assertFalse(any(call[:2] == ["image", "load"] for call in calls))

    def test_invalid_version_fails_before_docker(self):
        _, calls = self.run_install(success=False, args=("--version", "v0.1.1/elsewhere"))
        self.assertEqual(calls, [])


if __name__ == "__main__":
    unittest.main()
