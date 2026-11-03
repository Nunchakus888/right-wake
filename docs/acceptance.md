# Acceptance ledger

This ledger preserves the original implementation brief's 32 scenarios. **Implemented**, **tested locally**, **tested with a real integration**, and **released** are separate claims. A prepared workflow, manifest, recipe, or mock result must not be reported as a completed real installation or platform test.

The latest user requirement supersedes automatic new work-thread creation. Criterion 15 and the original new-thread demo are replaced by an explicit existing-thread claim; all other incomplete requirements remain visible obligations.

## Evidence snapshot

| Check | Current evidence |
| --- | --- |
| Plugin metadata and paths | Local structural validator passes; three repository skills pass the skill validator; official Codex CLI 0.166.0 also installed the local marketplace package in an isolated empty profile |
| Native CLI/core/provider/host tests | v0.1.1 local restricted environment: 86 passed, 8 socket fixtures explicitly ignored. The [release run](https://github.com/Nunchakus888/waking/actions/runs/41634516676) ran **all 94 tests on each of four native targets**, with 0 failures and 0 ignored tests. [Main CI](https://github.com/Nunchakus888/waking/actions/runs/41633868983) passed formatting, Clippy with `-D warnings`, tests, smoke, plugin validation, and dependency audit. These are contract tests, not live provider acceptance |
| Installer/package smoke | All four v0.1.1 native targets passed build and archive/install smoke, including paths with spaces, repeated installation, an actual database backup CLI invocation, and checksum rejection. A fresh public-download Linux x86_64 installation passed version/SQLite checks, repeated setup, doctor/status/schema, backup, and the synthetic demo. The public amd64 image archive and downloaded install/Compose files also matched release metadata. See [v0.1.1 verification](releases/v0.1.1-verification.md); [v0.1.0 evidence](releases/v0.1.0-verification.md) remains historical |
| Service manager | Four targeted Rust tests passed for ownership, escaping, idempotent definition writes, and data preservation. Read-only `systemd-analyze verify` accepted spaces, literal `$`/`%`, and Unicode executable paths; unsupported quote/backslash paths now fail before definition writes. No actual user launchd/systemd service was registered or started |
| Real Codex | Official CLI 0.166.0: real no-model handshake, protocol schema, thread listing, and rejection of missing thread read/resume passed. [Archived probe evidence](../tests/fixtures/codex-0.166.0/README.md). Real continuation of an existing work thread and desktop/cross-client concurrency remain unverified |
| Real Linear/GitHub/GitLab write E2E | Requires authorized test resources and credentials; do not post in production discussions to create evidence |
| Clean Codex plugin profile | Codex CLI 0.166.0: local marketplace add/install/list passed, installed and enabled at 0.1.0, three cached skills present. Cached MCP launcher: initialize, 11 tools, setup/status passed. Real GitHub URL retrieval/install also passed at commit `a9e8f28`; onboarding/model use not tested; see [plugin details](plugin.md#recorded-local-installation-result) |
| Native and container distribution | **[v0.1.1 published](https://github.com/Nunchakus888/waking/releases/tag/v0.1.1)** on 2026-11-03 from `f0ae3d8fc408ef14b8b53409381bd3ebf0856cda`: four macOS/Linux arm64/x86_64 native archives, two Linux Docker-load archives, `SHA256SUMS`, both installers, and Compose configuration. The macOS backup ancestor-symlink regression passed on both Mac targets |
| Independent review preparation | 14 fixture checks passed; a real native demo/export produced Python signing bytes identical to Rust, with exact body/target matching and no credential value exported. No provider send occurred |
| Dependency audit and SQLite | v0.1.1 main CI ran `cargo-audit` 0.22.2 against the fetched RustSec database: 262 locked dependencies, no reported vulnerabilities or audit warnings. The binary actually links SQLite 3.54.1 and enforces a 3.51.3 minimum before state access. A checksum-verified public v0.1.0 binary generated isolated data; v0.1.1 open/backup/restore preserved all rows in 13 tables, both drafts, and schema 3 |
| Container | Both Linux jobs in the [v0.1.1 release run](https://github.com/Nunchakus888/waking/actions/runs/41634516676) passed actual image build/load, non-root startup, SQLite 3.54.1 checks, and original event/job/draft retention across restart/replacement. The shipped Compose relay also retained a signed synthetic pending event across restart/recreation without dispatch. This does not establish live Codex/provider or public WSS integration |

## Required scenarios

“Pending evidence” means this file does not yet contain a verified result for the complete scenario. It does not imply the relevant code is absent.

| ID | Required outcome | Evidence needed / current qualification |
| --- | --- | --- |
| 1 | Fresh native install without Node/Python/Docker/sudo | Four native CI runners passed archive/install smoke. Public release download and install passed in a fresh Linux x86_64 directory without a build or service registration; no claim of a separate end-user-machine test |
| 2 | Clean Codex marketplace install and binary discovery/install | Official CLI 0.166.0 local marketplace install in an empty profile passed; cached MCP binary launch passed. Real GitHub marketplace retrieval and cached-launcher checks passed; conversational onboarding remains separate |
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
| 31 | Single-container cloud keeps state without claiming local-chat inheritance | v0.1.1 Docker and Compose checks passed on Linux amd64/arm64: original synthetic completed jobs, unsent drafts, and relay pending events persist across restart/replacement. Live agent execution and public ingress remain separate checks; no local-chat inheritance is claimed |
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

## Release evidence

[v0.1.1 verification](releases/v0.1.1-verification.md) records the exact source/tag, CI and four-platform results, actual Docker/Compose evidence, public assets, checksums, and download checks. [v0.1.0 verification](releases/v0.1.0-verification.md) retains the earlier release's historical evidence. Do not mark the overall original goal complete while required scenarios remain unverified or unimplemented.
