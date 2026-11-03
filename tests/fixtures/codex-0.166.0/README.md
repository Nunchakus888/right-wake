# Codex 0.166.0: recorded protocol smoke test

These are sanitized records from an actual official Linux x86-64 Codex CLI
0.166.0 process, recorded on 2026-11-03. The binary's package checksum matched
OpenAI's release metadata. See [the compatibility report](../../../docs/host-compatibility.md)
for checksums and remaining acceptance gates.

The probe generates a schema into a temporary directory, initializes an isolated
stdio App Server, checks an empty thread list, verifies rejection of a nonexistent
thread ID, and checks that rejection did not create a new thread. It sends no
model turn, creates no thread, and uses no account credentials or existing user
session. The configuration path, installation ID, and host name are redacted.
No binary or complete generated schema is included here.

Run against a separately installed trusted binary:

```sh
python3 tests/fixtures/codex-0.166.0/reproduce.py \
  --codex /absolute/path/to/codex \
  --out /absolute/path/to/probe-results
```

Python's standard library is sufficient. The script creates a temporary Codex
configuration directory and an empty project, sets that directory only for its
child process, and does not inherit authentication environment variables.

This is not a real existing-thread context test, a model execution test, a
shared-desktop concurrency test, or a test of the native Unix socket. The
shipped adapter's Unix socket test is separate:

```sh
cargo test --lib host::tests::real_unix_transport_smoke -- --ignored
```
