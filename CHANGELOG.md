# Changelog

## 0.3.0 — personal Linear scope and queued continuation

- Add `linear_assignee_id` to Linear sources: new comments on assigned issues and genuine new assignments become candidates without requiring a mention. An explicit organization scopes each personal source.
- Persist verified Issue ownership, revisions, removals, conflicts, and assignment intervals. Unknown ownership waits; reassignment invalidates unsubmitted work. Late events cannot revive a previous ownership interval. Pending capacity is bounded and advances on events rather than model polling.
- Add strict `source linear-import` and `source linear-state` commands. Partial snapshots seed known ownership without deleting absent issues or treating a saved baseline as an active subscription. Relay and worker preserve and independently apply assignment facts and static denials.
- Add opt-in Codex `--delivery queue` using the original owning App Server's experimental atomic queue. Direct `tool-output` remains the default and requires exclusive writer control. Both modes still require an actual restricted tool/credential boundary.
- Keep queued external content in immutable local storage and expose it through the read-only `right_wake_dispatch_context` MCP tool; accept a completed queued result only after successful context retrieval. Fixed queued notices contain a dispatch UUID, not source text.
- Correlate queue notices with exact client/thread/turn identity. Preserve unknown outcomes after ambiguous submission, do not automatically retry, and do not force a paused queue to start.
- Add read-only `host inspect`, concise personal-scope doctor diagnostics, and Agent-friendly schema descriptions. Update the setup Skill to preserve existing profiles/baselines and prepare concrete administrator handoffs.
- Upgrade current-name 0.2.0 profiles additively to database schema 4; keep bundled SQLite 3.54.1. Native and container installers pin 0.3.0. No registry dependency versions changed.

Published as an [experimental prerelease](https://github.com/Nunchakus888/right-wake/releases/tag/v0.3.0) on **2026-11-04 at 05:02:52 UTC**, from [`0d10c54`](https://github.com/Nunchakus888/right-wake/commit/0d10c547bf0f53d91c0ea3685c7ee4ce408aaf9d). Each of four native platforms passed **129 tests, 0 failures, 0 ignored**; both Linux image/Compose checks and 18 container-installer boundary tests passed. Exact runs and installation evidence are recorded in [v0.3.0 verification](docs/releases/v0.3.0-verification.md). Live YCloud registration, public HTTPS reachability, and real original-thread execution remain deployment-specific checks; the release does not invent those missing prerequisites.

## 0.2.0 — complete RightWake naming

- Use **RightWake** as the product name and **Wake the right Agent thread.** as the slogan. The canonical repository is `Nunchakus888/right-wake`.
- Rename the Rust package and executable to `right-wake`; Rust imports use `right_wake`. Help, schema, diagnostics, package filenames, installation scripts, and release workflows use the same identity.
- Publish the Codex plugin as `right-wake@right-wake`, with `right-wake-setup`, `right-wake-route`, and `right-wake-review` skills, and `right_wake_*` MCP tools.
- Use `RIGHT_WAKE_*` environment variables, `~/.local/share/right-wake` state, `right-wake.service`, the `io.github.nunchakus888.right-wake` launchd identity, and `right-wake` container/service/volume names.
- Set the default text trigger to `@right-wake`, Generic Webhook headers to `X-RightWake-*`, and approval/review/credential domains to `right-wake.*`.
- Remove legacy-name aliases, fallback discovery, historical image-source acceptance, and compatibility/migration advice. Configure 0.2.0 with current names and a fresh profile.
- Keep event-driven execution, explicit existing-thread ownership, durable queues, scoped context routing, and independent approval boundaries intact.

Published as an [experimental prerelease](https://github.com/Nunchakus888/right-wake/releases/tag/v0.2.0) on **2026-11-04 11:44:14 Asia/Shanghai**, from [`533e575`](https://github.com/Nunchakus888/right-wake/commit/533e57560e36780b7a3688a375c7ec07cf9fb8ef). Each of four native platforms passed **96 tests, 0 failures, 0 ignored**; both Linux Docker/Compose checks passed before publication. A fresh Linux x86_64 installation from the public assets, the real GitHub-cached plugin's launch of that binary, and the public amd64 image archive's checksum/metadata inspection also passed. [v0.2.0 verification](docs/releases/v0.2.0-verification.md) records the exact CI/release runs, public assets, and installation evidence. Live provider and original-thread model acceptance remain separately tracked.

## Earlier previews

The immutable [historical archive](docs/archive/README.md) retains the original 0.1.0 and 0.1.1 release notes, verification reports, plugin-installation results, and earlier changelog. Published tags and assets retain their original contents. These records document prior work; they are not installation instructions or compatibility guarantees for 0.2.0.
