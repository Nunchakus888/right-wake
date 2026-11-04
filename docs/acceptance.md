# Acceptance ledger

This ledger preserves the original implementation brief's 32 scenarios. **Implemented**, **tested locally**, **tested with a real integration**, and **released** are separate claims. A prepared workflow, manifest, recipe, or mock result must not be reported as a completed real installation or platform test.

The latest user requirement supersedes automatic new work-thread creation. Criterion 15 and the original new-thread demo are replaced by an explicit existing-thread claim; all other incomplete requirements remain visible obligations.

## Evidence snapshot

This snapshot concerns **v0.2.0**, which introduces the complete `right-wake` namespace. Earlier tests used earlier release identities; their original records are preserved in the [historical archive](archive/README.md). They are not substituted for a new-version run.

| Check | Current evidence |
| --- | --- |
| Namespace and installation contract | Source, package, CLI, plugin/skills, MCP tools, environment variables, profiles, services, containers, triggers, and signature domains use the new names. No legacy aliases, fallback discovery, image-source exceptions, or automatic migration are supplied. Run results belong in [v0.2.0 verification](releases/v0.2.0-verification.md). |
| Native CLI/core/provider/host tests | Rustfmt, all-target Clippy, and locked build passed. Local default tests: 88 passed / 8 ignored; six TCP fixtures then passed, totaling **94 of 96** successful tests. The two Unix-socket fixtures await native runners. |
| Installer and package smoke | Local 0.2.0 archive/install smoke, repeat installation, path-with-spaces handling, backup, and checksum rejection passed. Actual public-download verification follows publication. |
| Plugin and skills | Official Codex CLI 0.166.0 installed the local `right-wake@right-wake` 0.2.0 package in a fresh credential-free profile. All 11 cached files matched source; three cached skills passed validation. The cached launcher then started the actual 0.2.0 binary: MCP initialize, 11 tools, repeated setup, and status passed without a model or provider call. |
| Agent-led Linear onboarding | The workflow discovers authorized state and routes, preserves intended ownership, and reports concrete blockers. Fresh current-package trials and live Linear registration/model continuation require separate evidence. |
| Service manager | Current definition tests and target-machine lifecycle checks are separate. Actual user launchd/systemd install/start/stop/restart/uninstall remains unverified until recorded. |
| Real Codex | On 2026-11-04, the 0.2.0 probe passed six checks against checksum-verified official Codex CLI 0.166.0: schema generation, isolated initialization, empty thread list, missing read/resume rejection, and no fallback thread. [Current records](../tests/fixtures/codex-0.166.0/README.md). This used stdio and no model; real continuation and desktop/cross-client concurrency remain unverified. |
| Real Linear/GitHub/GitLab write E2E | Requires authorized test resources and credentials; do not post in production discussions to create evidence. |
| Native and container distribution | v0.2.0 publication is pending. The release must contain four native archives, two Linux Docker-load archives, checksums, both installers, and Compose configuration under the new names. |
| Independent review preparation | A real 0.2.0 native demo/export produced Python signing bytes identical to Rust under the new domain; exact body and target matched. No private key, signature, or provider send was used. |
| Dependency audit and SQLite | The actual 0.2.0 binary reports SQLite 3.54.1; startup floor 3.51.3 and schema 3 remain. All 261 registry package records/checksums are unchanged. Fresh dependency audit is a CI gate. |
| Container | Pending current Linux amd64/arm64 image build/load, non-root startup, Compose receipt, and persistence results. Public WSS and live Codex/provider integration remain separate. |

## Required scenarios

“Pending evidence” means this file does not yet contain a verified result for the complete scenario. It does not imply the relevant code is absent. The checks below must be rerun for the 0.2.0 identifiers; historical passes remain linked in the archive.

| ID | Required outcome | Evidence needed / current qualification |
| --- | --- | --- |
| 1 | Fresh native install without Node/Python/Docker/sudo | Four native runners must pass archive/install smoke; a public-release download must install the new binary in a fresh directory. End-user-machine installation is a separate check. |
| 2 | Clean Codex marketplace install and binary discovery/install | Install `right-wake@right-wake` in an empty profile; verify the cached launcher and renamed tools. Conversational onboarding remains separate. |
| 3 | Repeated setup creates no duplicates | Repeated native setup must preserve a current-version profile; live service repetition and full onboarding remain separate. |
| 4 | Missing permissions, expired credentials, and network failure explain next action | Provider/host failure cases plus actual setup diagnostics |
| 5 | Stop/upgrade/uninstall touches only owned resources and preserves recovery | Current installer and owned-service/data tests; live managers still require verification. No legacy namespace migration is part of 0.2.0. |
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
| 31 | Single-container cloud keeps state without claiming local-chat inheritance | Current Linux amd64/arm64 image and Compose checks must preserve synthetic jobs, unsent drafts, and pending events across restart/recreation. Live agent execution and public ingress remain separate; no local-chat inheritance is claimed. |
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

[v0.2.0 verification](releases/v0.2.0-verification.md) records the current source/tag, native and container jobs, public assets, checksums, and installation results as they are obtained. Earlier release and plugin records are linked from the [historical archive](archive/README.md). Do not mark the overall original goal complete while required scenarios remain unverified or unimplemented.
