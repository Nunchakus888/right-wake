# Codex 0.166.0: experimental queue protocol probe

This is a separate RightWake 0.3.0 probe of the official Linux x86-64 musl
Codex 0.166.0 binary. It preserves the earlier stable-protocol fixture unchanged.
The accompanying JSON files were recorded from the executable, not a mock.
This recorded run reverified the extracted executable digest. It did not
reverify the original archive; `archive_reverified_in_this_run` records that
distinction explicitly. The archive digest below is the expected release digest
preserved from the earlier download verification.

The probe verifies the experimental schema and method gate, rejection of a
synthetic nonexistent thread, and that an isolated thread store stays empty.
It **does not establish execution of a queued model turn, a connection to Codex
Desktop, access to a real user's thread, or working MCP permissions**. Those
deployment prerequisites require separate verification. RightWake's WebSocket
queue protocol fixtures exercise completion/correlation handling without a
provider call in `src/host_queue_tests.rs`.

## Reproduce

Download the official
[`codex-x86_64-unknown-linux-musl.tar.gz`](https://github.com/openai/codex/releases/download/rust-v0.166.0/codex-x86_64-unknown-linux-musl.tar.gz),
verify its release checksum, and extract the binary. This script pins both
digests and rejects another binary or version:

```sh
python3 tests/fixtures/codex-0.166.0-queue/reproduce.py \
  --codex /absolute/path/to/codex \
  --archive /absolute/path/to/codex-x86_64-unknown-linux-musl.tar.gz \
  --out /absolute/path/to/new-probe-output
```

The optional `--archive` verifies the original release archive in addition to
the required executable digest. The script starts isolated stdio processes,
uses empty temporary configuration/project directories, omits credential and
home environment variables, and never logs in or invokes a model. Queue add and
start requests are restricted to one verified absent synthetic thread ID; their
expected result is rejection. No existing profile or database is inspected.

| Evidence | Reference or observed value |
| --- | --- |
| Source tag | `rust-v0.166.0` |
| Source commit | `6ff1e1ca3b30fc8b914720681fcf5fe62e95f856` |
| Expected release archive SHA-256; not reverified in this run | `a0bf572d2f93f056c6712a6da348e96078dc84f3885fd7be67807ef389cb3978` |
| Extracted executable SHA-256 | `b8a11fedf24b2dfb59a70c9e52b030f9a508dcd49cd2dd0d41956f4c785156b8` |
| Without experimental capability | Queue list/add/start reject with `-32600` |
| With experimental capability | Same methods reach target validation, reject the absent thread with `-32603` |
| Queue add fields | `threadId`, `input`, `clientUserMessageId` |
| Queue list result | `data`, `nextCursor`; no paused-state field |

## Primary implementation sources

- [Queue request processor](https://github.com/openai/codex/blob/6ff1e1ca3b30fc8b914720681fcf5fe62e95f856/codex-rs/app-server/src/request_processors/thread_queue_processor.rs)
- [Queue scheduler and interruption behavior](https://github.com/openai/codex/blob/6ff1e1ca3b30fc8b914720681fcf5fe62e95f856/codex-rs/ext/queue/src/service.rs)
- [Atomic `StartIfIdle` admission](https://github.com/openai/codex/blob/6ff1e1ca3b30fc8b914720681fcf5fe62e95f856/codex-rs/core/src/session/turn_input.rs)
- [Queue persistence; client IDs are not a uniqueness key](https://github.com/openai/codex/blob/6ff1e1ca3b30fc8b914720681fcf5fe62e95f856/codex-rs/state/src/runtime/queued_items.rs)
- [Upstream queue behavior tests](https://github.com/openai/codex/blob/6ff1e1ca3b30fc8b914720681fcf5fe62e95f856/codex-rs/app-server/tests/suite/v2/thread_queue.rs)
- [User-message `clientId` mapping](https://github.com/openai/codex/blob/6ff1e1ca3b30fc8b914720681fcf5fe62e95f856/codex-rs/app-server-protocol/src/protocol/v2/item.rs)

Queue admission belongs to the same owning App Server. It inherits that thread's
tools and settings; it does not impose a per-dispatch read-only tool policy.
An uncertain add response must not be retried automatically because enqueue
generates a fresh entry even when the same client ID is reused. The adapter
therefore retains unknown outcomes for reconciliation and never force-starts a
paused queue or falls back to a new thread.
