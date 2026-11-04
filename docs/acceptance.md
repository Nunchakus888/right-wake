# Acceptance ledger

This ledger preserves the original implementation brief's 32 scenarios. **Implemented**, **tested locally**, **tested with a real integration**, and **released** are separate claims. A prepared workflow, manifest, recipe, or mock result must not be reported as a completed real installation or platform test.

The latest user requirement supersedes automatic new work-thread creation. Criterion 15 and the original new-thread demo are replaced by an explicit existing-thread claim; all other incomplete requirements remain visible obligations.

## Evidence snapshot

This snapshot concerns **v0.3.0**, adding personal Linear watching and optional Codex queue delivery within the current `right-wake` namespace. The [v0.3.0 verification report](releases/v0.3.0-verification.md) is the record of actual new-version runs and public assets. The complete [v0.2.0 evidence](releases/v0.2.0-verification.md) and [earlier archive](archive/README.md) remain unchanged.

| Check | Current evidence and boundary |
| --- | --- |
| Personal Linear scope | Implemented provider candidates, durable owner/revision/removal/conflict projection, partial bootstrap, pre-submission checks, bounded event-driven recovery, and relay policy preservation. Signed fixtures and integration suites are listed in the current report; no live user baseline is claimed. |
| Codex queue | Implemented optional atomic queue on the original owning server, fixed dispatch notice, immutable MCP context/read receipt, exact correlation, guarded submission, and unknown outcomes. Mock protocol and real no-model probe evidence do not prove a live Desktop work turn. |
| Native core/provider/host tests | Local and four-platform gate results are recorded with the actual source and job IDs in the current report. Prior-version test counts are not reused. |
| Installer and public packages | Native release jobs, real Linux image/Compose checks, published assets, checksums, and fresh public installation each need their own recorded result. |
| Plugin and skills | Three bundled skills and twelve local MCP tools. Metadata validation, independent setup scenario review, real GitHub marketplace/cache installation, and actual stdio launcher checks are distinguished in the current report. |
| Agent-led Linear onboarding | Preserves existing state, configures personal scope, imports verified snapshots, prepares administrator handoff, and reports desired scope/baseline/receipt/execution separately. Live registration and original-thread execution still require actual local capabilities. |
| Real Codex | The new [0.166.0 Queue probe](../tests/fixtures/codex-0.166.0-queue/README.md) verifies experimental capability and missing-thread behavior without credentials, model calls, or created threads. The [older direct probe](../tests/fixtures/codex-0.166.0/README.md) remains historical evidence. Shared owning-server identity, original-history recall, and tool restrictions need real deployment checks. |
| Services and providers | Native service definitions are testable; actual launchd/systemd lifecycle, public ingress, authorized provider read/write E2E, and live model completion remain separate integration checks. No production comments are posted to manufacture evidence. |
| State and dependencies | Schema 4 adds current-name 0.2.0 tables; preserve a pre-upgrade backup. Bundled SQLite remains 3.54.1 with a 3.51.3 startup floor. The 261 registry dependency versions/checksums are unchanged. The current CI audit result belongs in the report. |

## Required scenarios

“Pending evidence” means this file does not yet contain a verified result for the complete scenario. It does not imply the relevant code is absent. Current 0.3.0 results are recorded in the linked report and qualified per scenario below; historical passes remain linked in the archive.

| ID | Required outcome | Evidence needed / current qualification |
| --- | --- | --- |
| 1 | Fresh native install without Node/Python/Docker/sudo | Four native archive/install/backup gates and a fresh public Linux download must be recorded for this version, including a path with spaces and actual binary/MCP/SQLite checks. End-user-machine installation remains separate. |
| 2 | Clean Codex marketplace install and binary discovery/install | Verify current GitHub marketplace retrieval, installed/enabled cache identity, and the actual cached MCP launcher with the public binary. Conversational onboarding remains separate. |
| 3 | Repeated setup creates no duplicates | CLI and MCP repeat-setup tests; live service repetition and full conversational onboarding remain separate. |
| 4 | Missing permissions, expired credentials, and network failure explain next action | Provider/host failure cases plus actual setup diagnostics |
| 5 | Stop/upgrade/uninstall touches only owned resources and preserves recovery | Current installer and owned-service/data tests; live managers still require verification. The 0.2.0 current-name profile upgrades additively; no old-name namespace migration is supplied. |
| 6 | One valid event from each native platform has the right identity | Linear, GitHub, GitLab signed fixtures plus authorized real ingress |
| 7 | Bad signatures, applicable stale timestamps, and oversized bodies rejected | Provider/HTTP boundary tests |
| 8 | Exact-body verification handles whitespace, newlines, Unicode | Raw-byte signature fixtures |
| 9 | Equal displayed numbers across tenants/repos/instances never collide | Canonical identity isolation tests |
| 10 | PR review comments and MR notes preserve reply location | Provider normalization and exact endpoint tests; real reply-location E2E |
| 11 | Unknown legitimate events skip safely; self-writes do not loop | Provider event/action filtering and actor/receipt tests |
| 12 | Duplicates, ordering, and edits do not duplicate replies or lose valid revisions | Revision/deduplication/late-event tests |
| 13 | Issue A continues Thread A; B continues B with history | Mock correlation plus real prior-history continuation in two original threads |
| 14 | Shared Thread C serializes and coordinates human activity | Queue admission/correlation/guard tests; direct delivery still needs exclusive writing, and live Queue coordination requires the same owning App Server |
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
| 31 | Single-container cloud keeps state without claiming local-chat inheritance | Linux amd64/arm64 image and Compose gates check original synthetic jobs/drafts and pending relay events across restart/recreation. Current run IDs and outcomes belong in the report. Live agent execution and public ingress remain separate; no local-chat inheritance is claimed. |
| 32 | Generic Webhook is configuration-driven; GenericCommand uses safe stdin | Config-only mapping and fixed executable/argv/JSON stdin tests |

## Additional brief obligations

The brief also requires schema validation/migration recovery, explicit source/host/binding/global pause behavior, secrets/key management, meaningful doctor diagnostics, user-level service control, generic extension examples, dependency review, container/TLS relay deployment instructions, bilingual onboarding, and open-source release assets. A missing item is still incomplete even if it is not a separate row in the 32-scenario table.

## Reproducible local checks

```sh
cargo fmt --all -- --check
cargo clippy --locked --all-targets -- -D warnings
cargo test --locked
cargo build --locked
sh scripts/smoke.sh target/debug/right-wake
python3 scripts/check-plugin.py
```

Run `right-wake --data-dir /absolute/path/to/disposable-demo demo` only against a fresh disposable directory. The demo uses signed local events and mock threads; its label must remain visible. For real tests, record the source scope, provider event/delivery ID, exact host version, thread/turn IDs, original-history evidence, draft destination/version, explicit approval, and provider result. Redact tokens and private task content.

## Release evidence

[v0.3.0 verification](releases/v0.3.0-verification.md) records the current source/tag, native and container jobs, public assets, checksums, and installation results as they are obtained. Earlier release and plugin records are linked from the [historical archive](archive/README.md). Do not mark the overall original goal complete while required scenarios remain unverified or unimplemented.
