# Changelog

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
