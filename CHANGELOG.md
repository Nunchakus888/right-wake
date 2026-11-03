# Changelog

## Plugin workflow update after 0.1.1

- Extend the existing `waking-setup` skill with a bundled Linear onboarding reference: discover and reuse authorization/profile/source/host/binding state, configure available delivery, and report exact unresolved prerequisites.
- Preserve existing configuration on repeat runs, require trusted original-thread ownership, and distinguish configuration from actual provider receipt and model continuation.
- Keep the published native binary and installer at v0.1.1. The workflow adds no runtime dependency; refresh the installed plugin package to receive the updated skill. Authorized webhook administration remains a prerequisite for automatic provider registration.

## 0.1.1 — SQLite update and Docker distribution

- Upgrade bundled SQLite from 3.46.0 to 3.54.1 through `rusqlite` 0.41.1 / `libsqlite3-sys` 0.39.1, including the upstream WAL-reset fix. Refuse state access with a linked SQLite older than 3.51.3 and report the actual runtime version in `doctor`.
- Keep database schema version 3. Verify isolated data created by the published v0.1.0 binary can be opened, backed up, and restored with v0.1.1 without changing its stored records.
- Add checksummed Linux amd64/arm64 Docker-load image archives, a load-only installer, and Compose configuration for a persistent VPS relay. Release images reuse the corresponding tested native binaries; no GHCR package is required.
- Pin native/plugin installation to v0.1.1 and document native local execution, Docker relay storage, and the boundary around original local session history. SQLite remains embedded; no separate database service is introduced.
- Raise source-build MSRV to Rust 1.89 for the dependency update; the pinned Rust 1.99.1 toolchain is unchanged.
- Make native package smoke invoke the actual backup CLI and inspect its snapshot, in addition to the existing core backup regression tests.

Published as an [experimental prerelease](https://github.com/Nunchakus888/waking/releases/tag/v0.1.1) on 2026-11-03. All four native platforms passed 94 tests each, and both Linux Docker/Compose release checks passed. Exact runs and publication evidence are recorded in [v0.1.1 verification](docs/releases/v0.1.1-verification.md); real host/provider integration remains separately tracked.

## 0.1.0 — initial preview

- Native Rust CLI, local management interface, and repository Codex plugin.
- Event ingress and normalized identity for scoped provider adapters.
- Durable explicit bindings to existing threads, context cards, and observable job states.
- Draft-first output and independently signed publication review.
- Native installation, user-service definitions, four-target release workflow, and source container recipe.

Implementation, local validation, real integration, and public release results are recorded separately in [docs/acceptance.md](docs/acceptance.md). This entry describes the preview's intended release contents and is not evidence that every original acceptance scenario has passed.
