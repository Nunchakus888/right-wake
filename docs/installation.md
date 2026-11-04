# Installation and lifecycle

**RightWake** uses `right-wake` consistently for its executable, packages, plugin, services, and data paths. v0.3.0 preserves an existing 0.2.0 `right-wake` profile and adds database tables for Linear ownership and queued dispatch context. Back up before upgrading; no legacy-name aliases are provided.

## Native release

RightWake's ordinary runtime is one Rust binary with bundled SQLite. No separate database service or SQLite command-line installation is needed. It requires no Node.js, Python, Docker, Rust compiler, or elevated installation. The release installer uses system shell tools, `curl`, `tar`, and either `sha256sum` or `shasum`.

The installer is pinned to **[v0.3.0](https://github.com/Nunchakus888/right-wake/releases/tag/v0.3.0)**. The [verification report](releases/v0.3.0-verification.md) records actual source, native jobs, image checks, plugin installation, and public downloads. Earlier [v0.2.0 results](releases/v0.2.0-verification.md) remain unchanged and are not counted as new-version results.

| Archive target | Native release runner | Platform scope |
| --- | --- | --- |
| `x86_64-unknown-linux-gnu` | Ubuntu 22.04 x86_64 | glibc Linux; older glibc and musl are not claimed |
| `aarch64-unknown-linux-gnu` | Ubuntu 22.04 arm64 | glibc Linux arm64 |
| `x86_64-apple-darwin` | macOS 15 Intel | Older OS versions are not claimed |
| `aarch64-apple-darwin` | macOS 15 arm64 | Native Apple Silicon |

Native artifacts are named `right-wake-v0.3.0-TARGET.tar.gz`. `SHA256SUMS` covers four native archives and two Linux Docker image archives. The native installer selects only its matching archive, then verifies its hash, permitted members, and runnable version before replacing the destination. These checks detect mismatches; checksums hosted with the release are not an independent publisher signature.

The [README](../README.md#install) gives the pinned download path. Prior release checksums and results are retained in the [historical archive](archive/README.md). Source installation remains available with `cargo install --locked --path .`, followed by `right-wake setup`.

Offline installation from a directory containing the matching archive and `SHA256SUMS`:

```sh
sh scripts/install.sh --version v0.3.0 \
  --from-dir /absolute/path/to/release-files \
  --bin-dir "$HOME/.local/bin"
```

The default directory is `~/.local/bin`; use `--bin-dir` or `RIGHT_WAKE_INSTALL_DIR` to change it. The default version is pinned; use `--version` or `RIGHT_WAKE_VERSION` to select another published release. No automatic background upgrade runs.

## Docker for a VPS relay

Use the native binary beside the local agent whose history you want to continue. For an online VPS relay, the [Docker + Compose guide](../deploy/README.md) uses the separate `install-container.sh` to verify and load one architecture-specific image from GitHub Release assets:

- `right-wake-v0.3.0-container-linux-amd64.tar.gz`
- `right-wake-v0.3.0-container-linux-arm64.tar.gz`

The resulting image is tagged `right-wake:0.3.0`. The installer loads the image only; configuration, secrets, volume initialization, and startup are explicit. The supplied Compose service runs `relay serve`, uses a named volume at `/data`, and binds its published port to host loopback for your TLS proxy. No GHCR login or image-registry publication is required. The release pipeline attaches both image archives and deployment files to the versioned release, after native and Linux image/Compose checks succeed. Actual public download, checksum, archive-metadata, and runtime results are identified separately in [verification](releases/v0.3.0-verification.md).

The image embeds the same RightWake binary and SQLite library used by the corresponding Linux native release. A separate database container is unnecessary. Cloud receipt does not move local Codex history or execute work while its local host is offline; [deployment choices](deployment.md) explains that boundary.

## Configuration and secrets

```sh
right-wake setup
right-wake --json schema
right-wake --json doctor
```

`setup` is idempotent. The data directory is selected by `--data-dir`, then `RIGHT_WAKE_DATA_DIR`, then the XDG/default path. The default is `~/.local/share/right-wake` when `XDG_DATA_HOME` is absent. This directory holds `config.json` and `state.sqlite3`, including any live SQLite WAL/SHM sidecars. Docker uses its persistent `/data` volume instead. Keep configuration and state private and back them up before upgrades. Use local persistent storage rather than a network filesystem for SQLite WAL operation; see [SQLite's WAL requirements](https://www.sqlite.org/wal.html).

Source configuration stores credential environment-variable names. A foreground process inherits its launching environment. A service does not automatically inherit secrets from a terminal opened later. Use a private `--env-file` when needed; its format is plain `NAME=VALUE`, one per line, without shell expansion or `export` statements. Do not put actual credentials in commands, plugin manifests, issues, or chat.

## User service

```sh
right-wake --env-file /absolute/path/to/private.env service install
right-wake service start
right-wake service status
right-wake service stop
right-wake service uninstall
```

The private file must already have appropriate permissions (for example `chmod 600`). The service definition references its path, never embeds secret values. Keep that path available after installation.

Linux uses the current user's `right-wake.service` under `~/.config/systemd/user` (or `XDG_CONFIG_HOME`). It needs a working systemd user session. Spaces and Unicode in executable paths are supported; systemd's unsupported quoted/backslash executable paths are rejected before writing a definition. macOS uses `~/Library/LaunchAgents/io.github.nunchakus888.right-wake.plist` in the logged-in GUI user's launchd domain. Installation enables future login startup; `start` explicitly starts now. Repeating installation updates only RightWake's owned definition and does not restart an already running process. Stop and start after changing its executable or environment-file path.

`stop` stops current execution but leaves configuration and login registration. `uninstall` stops/disables the owned service and removes its definition; it keeps configuration, database, drafts, and external provider webhooks. One service definition belongs to one data directory. A different profile cannot silently take it over.

Service template tests do not prove launchd/systemd operation on a target machine. Actual install/start/stop/restart/uninstall remains a separate acceptance check.

## Upgrade, rollback, and removal

Stop the receiver, take a complete state backup, and install the next pinned release. The installer preserves the previous recognized binary as `right-wake.previous`. Run `--version` and `doctor` before restarting. A binary backup alone cannot reverse an incompatible database migration; retain the matching state backup and follow that release's migration notes.

**0.2.0 → 0.3.0:** keep the same data directory. Schema **4** adds the Linear issue ownership projection and immutable dispatch-context storage; existing sources, bindings, contexts, jobs, drafts, and secret references remain. Existing Codex hosts default to `tool_output`, so an upgrade does not silently opt into the experimental queue. A saved `linear-onboarding.json` is an input for the setup Agent to inspect, not an automatically trusted import. Convert verified records to the [strict Linear snapshot format](linear.md), retaining the original file.

Keep the matching pre-upgrade state backup for rollback: a 0.2.0 binary should not be run against an upgraded schema-4 database. This is an additive upgrade within the current `right-wake` namespace, not migration of older, differently named previews or their signed artifacts.

Bundled SQLite remains **3.54.1**, with a startup floor of 3.51.3. Updating a host's `sqlite3` package does not update the embedded library: replace the RightWake binary or image. `right-wake --json doctor` reports the actual linked version. [v0.3.0 notes](releases/v0.3.0.md) describe the new filtering and delivery modes.

For Docker upgrades, load the new image, use the matching reviewed Compose file, and recreate the service with the **same Compose project and data volume**. Keep `private.env` and operator configuration. A container replacement does not require a new database; changing the project name can select a different empty volume. See [container lifecycle](../deploy/README.md#storage-upgrades-and-removal).

To remove the product, first stop and uninstall its service, then remove the installed `right-wake` binary and optional `right-wake.previous`. Uninstall the Codex plugin separately. Keep the data directory for recovery, or delete it only after explicitly deciding to discard its events, bindings, and drafts. Removing RightWake does not delete provider-side webhook registrations; remove only registrations you created for this product.

## Release process

Routine CI uses one Linux runner for formatting, Clippy, tests, CLI/installer smoke, plugin metadata, and a pinned `cargo-audit` scan of `Cargo.lock` against the RustSec advisory database. The audit runs on main and pull requests and fails on advisories or warnings; it does not require the optional GitHub Dependency graph setting. GitHub Actions updates remain pinned and reviewable through Dependabot. Dependabot proposes Cargo and Actions updates monthly. Toolchain updates are deliberate changes to `rust-toolchain.toml`; dependency versions are resolved in `Cargo.lock`.

After a successful **push CI run on this repository's `main` branch**, the release workflow checks out that exact CI commit and reads the package version. Pull-request, fork, failed-CI, and unrelated-branch completions cannot enter this automatic publication path. If the version is already publicly released, all four native jobs are skipped. A new version runs native tests (including the isolated socket fixtures), build, and installer smoke on all four targets. The Linux jobs reuse their compiled native binaries to build and check Docker-load images; no second Rust compilation is needed for those images. Successful release jobs assemble the archive checksums and publish an experimental prerelease at the tested commit. Release attempts are serialized so repeated pushes do not race into duplicate four-platform builds.

Existing `v*` tags and manual dispatch remain available. A tag run prepares a draft; a manual run publishes only when its `publish` input is selected. The automatic successful-main-CI path publishes without another manual tag step. Existing published assets are immutable: changing code for a published version requires a new package version. An existing tag that points at a different commit blocks publication rather than moving it. Public release status still requires an actual successful GitHub run; local workflow validation is not publication evidence.
