# Installation and lifecycle

## Native release

Waking's ordinary runtime is one Rust binary. It requires no Node.js, Python, Docker, Rust compiler, or elevated installation. The release installer uses system shell tools, `curl`, `tar`, and either `sha256sum` or `shasum`.

| Archive target | Build and smoke runner | Compatibility evidence |
| --- | --- | --- |
| `x86_64-unknown-linux-gnu` | Ubuntu 22.04 x86_64 | v0.1.0: 93 native tests, release build, and installer smoke passed; older glibc and musl are not claimed |
| `aarch64-unknown-linux-gnu` | Ubuntu 22.04 arm64 | v0.1.0: 93 native tests, release build, and installer smoke passed |
| `x86_64-apple-darwin` | macOS 15 Intel | v0.1.0: 93 native tests, release build, and installer smoke passed; older OS versions are not claimed |
| `aarch64-apple-darwin` | macOS 15 arm64 | v0.1.0: 93 native tests, release build, and installer smoke passed |

Artifacts are named `waking-v0.1.0-TARGET.tar.gz`. `SHA256SUMS` covers the four archives. The install script verifies the archive hash, permitted archive members, and runnable version before replacing the destination. These checks detect mismatches; checksums hosted with the release are not an independent publisher signature.

The [README](../README.md#install) gives the pinned download path. [v0.1.0](https://github.com/Nunchakus888/waking/releases/tag/v0.1.0) was published on 2026-11-03 from commit `52dff8a4fd2522c793ef2b6bc709d561922f4790` after the successful [four-platform release run](https://github.com/Nunchakus888/waking/actions/runs/41619988502). Every platform ran all 93 tests, including socket fixtures, with zero failures and zero ignored tests. Source installation remains available with `cargo install --locked --path .`.

Offline installation from a directory containing the matching archive and `SHA256SUMS`:

```sh
sh scripts/install.sh --version v0.1.0 \
  --from-dir /absolute/path/to/release-files \
  --bin-dir "$HOME/.local/bin"
```

The default directory is `~/.local/bin`; use `--bin-dir` or `WAKING_INSTALL_DIR` to change it. The default version is pinned; use `--version` or `WAKING_VERSION` to select another published release. No automatic background upgrade runs.

## Configuration and secrets

```sh
waking setup
waking --json schema
waking --json doctor
```

`setup` is idempotent. The data directory is selected by `--data-dir`, then `WAKING_DATA_DIR`, then the XDG/default path. The default is `~/.local/share/waking` when `XDG_DATA_HOME` is absent. Keep configuration and database state private and back them up before upgrades.

Source configuration stores credential environment-variable names. A foreground process inherits its launching environment. A service does not automatically inherit secrets from a terminal opened later. Use a private `--env-file` when needed; its format is plain `NAME=VALUE`, one per line, without shell expansion or `export` statements. Do not put actual credentials in commands, plugin manifests, issues, or chat.

## User service

```sh
waking --env-file /absolute/path/to/private.env service install
waking service start
waking service status
waking service stop
waking service uninstall
```

The private file must already have appropriate permissions (for example `chmod 600`). The service definition references its path, never embeds secret values. Keep that path available after installation.

Linux uses the current user's `waking.service` under `~/.config/systemd/user` (or `XDG_CONFIG_HOME`). It needs a working systemd user session. Spaces and Unicode in executable paths are supported; systemd's unsupported quoted/backslash executable paths are rejected before writing a definition. macOS uses `~/Library/LaunchAgents/io.github.nunchakus888.waking.plist` in the logged-in GUI user's launchd domain. Installation enables future login startup; `start` explicitly starts now. Repeating installation updates only Waking's owned definition and does not restart an already running process. Stop and start after changing its executable or environment-file path.

`stop` stops current execution but leaves configuration and login registration. `uninstall` stops/disables the owned service and removes its definition; it keeps configuration, database, drafts, and external provider webhooks. One service definition belongs to one data directory. A different profile cannot silently take it over.

Service template tests do not prove launchd/systemd operation on a target machine. Actual install/start/stop/restart/uninstall remains a separate acceptance check.

## Upgrade, rollback, and removal

Stop the receiver, take a complete state backup, and install the next pinned release. The installer preserves the previous recognized binary as `waking.previous`. Run `--version` and `doctor` before restarting. A binary backup alone cannot reverse an incompatible database migration; retain the matching state backup and follow that release's migration notes.

To remove the product, first stop and uninstall its service, then remove the installed `waking` binary and optional `waking.previous`. Uninstall the Codex plugin separately. Keep the data directory for recovery, or delete it only after explicitly deciding to discard its events, bindings, and drafts. Removing Waking does not delete provider-side webhook registrations; remove only registrations you created for this product.

## Release process

Routine CI uses one Linux runner for formatting, Clippy, tests, CLI/installer smoke, plugin metadata, and a pinned `cargo-audit` scan of `Cargo.lock` against the RustSec advisory database. The audit runs on main and pull requests and fails on advisories or warnings; it does not require the optional GitHub Dependency graph setting. GitHub Actions updates remain pinned and reviewable through Dependabot. Dependabot proposes Cargo and Actions updates monthly. Toolchain updates are deliberate changes to `rust-toolchain.toml`; dependency versions are resolved in `Cargo.lock`.

After a successful **push CI run on this repository's `main` branch**, the release workflow checks out that exact CI commit and reads the package version. Pull-request, fork, failed-CI, and unrelated-branch completions cannot enter this automatic publication path. If the version is already publicly released, all four native jobs are skipped. A new version runs native tests (including the isolated socket fixtures), build, and installer smoke on all four targets, assembles checksums, and automatically publishes an experimental prerelease at the tested commit. Release attempts are serialized so repeated pushes do not race into duplicate four-platform builds.

Existing `v*` tags and manual dispatch remain available. A tag run prepares a draft; a manual run publishes only when its `publish` input is selected. The automatic successful-main-CI path publishes without another manual tag step. Existing published assets are immutable: changing code for a published version requires a new package version. An existing tag that points at a different commit blocks publication rather than moving it. Public release status still requires an actual successful GitHub run; local workflow validation is not publication evidence.
