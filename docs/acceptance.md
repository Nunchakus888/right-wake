# Acceptance ledger

This ledger preserves the original implementation brief's 32 scenarios. **Implemented**, **tested locally**, **tested with a real integration**, and **released** are separate claims. A prepared workflow, manifest, recipe, or mock result must not be reported as a completed real installation or platform test.

The latest user requirement supersedes automatic new work-thread creation. Criterion 15 and the original new-thread demo are replaced by an explicit existing-thread claim; all other incomplete requirements remain visible obligations.

## Evidence snapshot

| Check | Current evidence |
| --- | --- |
| Plugin metadata and paths | Local structural validator passes; three repository skills pass the skill validator; official Codex CLI 0.166.0 also installed the local marketplace package in an isolated empty profile |
| Native CLI/core/provider/host tests | Linux x86_64: `cargo test --locked` passed 78 tests, 0 failed; 7 native socket fixtures are explicitly ignored in this restricted environment and enabled by the release matrix. Clippy with `-D warnings` passed. These are local contract tests, not live provider acceptance |
| Installer/package smoke | `cargo build --locked` and `sh scripts/smoke.sh target/debug/waking` passed on Linux x86_64: native CLI JSON, repeated setup, packaging, installation in a path with spaces, backup, and checksum rejection. Other release platforms/fresh machines remain separate |
| Service manager | Four targeted Rust tests passed for ownership, escaping, idempotent definition writes, and data preservation. Read-only `systemd-analyze verify` accepted spaces, literal `$`/`%`, and Unicode executable paths; unsupported quote/backslash paths now fail before definition writes. No actual user launchd/systemd service was registered or started |
| Real Codex | Official CLI 0.166.0: real no-model handshake, protocol schema, thread listing, and rejection of missing thread read/resume passed. [Archived probe evidence](../tests/fixtures/codex-0.166.0/README.md). Real continuation of an existing work thread and desktop/cross-client concurrency remain unverified |
| Real Linear/GitHub/GitLab write E2E | Requires authorized test resources and credentials; do not post in production discussions to create evidence |
| Clean Codex plugin profile | Codex CLI 0.166.0: local marketplace add/install/list passed, installed and enabled at 0.1.0, three cached skills present. Cached MCP launcher: initialize, 11 tools, setup/status passed. GitHub URL download/onboarding/model use not tested; see [plugin details](plugin.md#recorded-local-installation-result) |
| Four release targets | Prepared workflow; each actual native build/test result and release URL must be recorded |
| Container | Source recipe supplied; image build/run/restart not yet established |

## Required scenarios

“Pending evidence” means this file does not yet contain a verified result for the complete scenario. It does not imply the relevant code is absent.

| ID | Required outcome | Evidence needed / current qualification |
| --- | --- | --- |
| 1 | Fresh native install without Node/Python/Docker/sudo | Linux x86_64 native archive/package/install smoke passed locally; fresh-machine and other-target release checks remain |
| 2 | Clean Codex marketplace install and binary discovery/install | Official CLI 0.166.0 local marketplace install in an empty profile passed; cached MCP binary launch passed. GitHub download and conversational onboarding still need evidence |
| 3 | Repeated setup creates no duplicates | Native CLI repeated setup preserves existing configuration; live service repetition and full onboarding remain separate |
| 4 | Missing permissions, expired credentials, and network failure explain next action | Provider/host failure cases plus actual setup diagnostics |
| 5 | Stop/upgrade/uninstall touches only owned resources and preserves recovery | Installer plus owned service/data tests; live managers still require verification |
| 6 | One valid event from each native platform has the right identity | Linear, GitHub, GitLab signed fixtures plus authorized real ingress |
| 7 | Bad signatures, applicable stale timestamps, and oversized bodies rejected | Provider/HTTP boundary tests |
| 8 | Exact-body verification handles whitespace, newlines, Unicode | Raw-byte signature fixtures |
| 9 | Equal displayed numbers across tenants/repos/instances never collide | Canonical identity isolation tests |
| 10 | PR review comments and MR notes preserve reply location | Provider normalization and exact endpoint tests; real reply-location E2E |
| 11 | Unknown legitimate events skip safely; self-writes do not loop | Provider event/action filtering and actor/receipt tests |
| 12 | Duplicates, ordering, and edits do not duplicate replies or lose valid revisions | Revision/deduplication/late-event tests |
| 13 | Issue A continues Thread A; B continues B with history | Mock correlation plus real prior-history continuation in two original threads |
| 14 | Shared Thread C serializes and coordinates human activity | Queue tests; host exclusive-writer requirement remains explicit for unproven cross-client coordination |
| 15 | Unbound resource is claimed once to an existing thread and reused | **Superseded original create-once rule** by the user's no-new-thread requirement |
| 16 | Window switching does not affect destination | Persisted host/thread IDs plus real desktop observation |
| 17 | Several history candidates require at most one ownership claim; mentions do not bind | Ownership provenance and ambiguity tests; no title-only assignment |
| 18 | Rebinding prevents stale queued delivery | Binding-version invalidation tests |
| 19 | Pause/archive/delete/invalid worktree/offline have explicit outcomes | Host and queue tests plus actually supported pause scopes |
| 20 | Normal increments do not resend all history or run idle summaries | Request payload/coverage checks and model invocation assertions |
| 21 | Storage failure prevents a successful receipt ACK | Transaction/admission failure tests |
| 22 | Crash after possible host acceptance does not blindly replay | Durable-attempt recovery and unknown-state tests |
| 23 | Timed-out external send is reconciled before retry | Provider outbox and readback/reconciliation evidence |
| 24 | Unapproved, changed, and stale drafts cannot send | Signature/action/version/source-revision tests; independent signer separation |
| 25 | Payload cannot change thread/cwd/command/permissions/credentials | Payload-injection and trusted configuration boundary tests |
| 26 | Remote tool mutation is not assumed safe from filesystem read-only mode | Host tool-approval policy and real tool configuration inspection |
| 27 | No event means no model turn | Deterministic invocation-count test, not a long sleep |
| 28 | Relay receives while local device is offline, then replays completely | Durable relay/device ACK integration tests and actual reconnect demonstration |
| 29 | Device source scope and revocation are enforced | Cross-source denial and active revocation tests |
| 30 | Relay restart, disk full, duplicate ACK, reconnect, backlog are bounded | Fault/recovery tests for every case, not just successful delivery |
| 31 | Single-container cloud keeps state without claiming local-chat inheritance | Actual container build, restart, persisted task test |
| 32 | Generic Webhook is configuration-driven; GenericCommand uses safe stdin | Config-only mapping and fixed executable/argv/JSON stdin tests |

## Additional brief obligations

The brief also requires schema validation/migration recovery, explicit source/host/binding/global pause behavior, secrets/key management, meaningful doctor diagnostics, user-level service control, generic extension examples, dependency review, container/TLS relay deployment instructions, bilingual onboarding, and open-source release assets. A missing item is still incomplete even if it is not a separate row in the 32-scenario table.

## Reproducible local checks

```sh
cargo fmt --all -- --check
cargo clippy --locked --all-targets -- -D warnings
cargo test --locked
cargo build --locked
sh scripts/smoke.sh target/debug/waking
python3 scripts/check-plugin.py
```

Run `waking --data-dir /absolute/path/to/disposable-demo demo` only against a fresh disposable directory. The demo uses signed local events and mock threads; its label must remain visible. For real tests, record the source scope, provider event/delivery ID, exact host version, thread/turn IDs, original-history evidence, draft destination/version, explicit approval, and provider result. Redact tokens and private task content.

## Release evidence to fill

Record the repository commit, tag, CI/release run links, archive checksums, per-platform results, public release URL, clean-install result, and remaining real-integration blockers. Do not mark the overall original goal complete while required scenarios remain unverified or unimplemented.
