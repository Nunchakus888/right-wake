# Waking

**Wake the right thread.**

[简体中文](README.zh-CN.md)

Waking is a native Rust bridge that receives work events, identifies their owner, and brings the new information back to an **existing local agent thread**. It is designed for work already spread across Linear, GitHub, GitLab, and several Codex conversations.

The durable binding is a work resource → host → thread relationship. Switching windows does not change it. A busy or unavailable thread keeps its work queued. An unknown resource waits for a justified assignment; it does not create a new work conversation. Replies start as drafts.

## Project status

Version **0.1.0** is an initial implementation under validation. The source, local tests, release preparation, published artifacts, and real platform acceptance are tracked separately in [the acceptance ledger](docs/acceptance.md). A mock executor passing a test does not establish compatibility with a real Codex desktop version.

The four-platform release workflow is supplied, but a workflow file alone is not a published release. Check [Releases](https://github.com/Nunchakus888/waking/releases) before using the download commands below. Codex CLI 0.166.0 passed a no-model host probe and local marketplace installation in an isolated empty profile. Real existing-thread model continuation, desktop concurrency, GitHub-based plugin download, and authorized provider read/write E2E still need their own evidence.

## What it does

- Receives verified work events through webhooks; idle operation does not call a model to ask whether anything changed.
- Resolves explicit discussion and issue bindings before considering context-directory evidence.
- Keeps queue state and context cards locally. Context cards help find the right conversation; the destination retains its own original history.
- Gives each host/thread a serialized work queue and exposes blocked or uncertain delivery states.
- Produces inspectable drafts with an explicit destination, identity, and version. External sending requires the configured independent approval.
- Offers a standalone CLI and a repository-distributed Codex plugin using the same native binary.

Provider, host, semantic routing, and deployment capability limits are listed in [the architecture](docs/architecture.md) and [acceptance ledger](docs/acceptance.md). Waking is an independent project; it is not an official OpenAI, Linear, GitHub, GitLab, or Anthropic product.

## Install

The ordinary native installation needs a shell, `curl`, `tar`, and a system SHA-256 tool. It needs **no Node.js, Python, Docker, Rust, or sudo**. Select macOS or glibc Linux on arm64 or x86_64. The installer downloads the matching pinned release, verifies its archive checksum and executable version, and installs in `~/.local/bin`. It does not start a service or modify Codex configuration.

After `v0.1.0` has been published:

```sh
curl --proto '=https' --tlsv1.2 --fail --location \
  --output waking-install.sh \
  https://github.com/Nunchakus888/waking/releases/download/v0.1.0/install.sh
# Inspect the downloaded installer, then run it.
sh waking-install.sh --version v0.1.0
export PATH="$HOME/.local/bin:$PATH"
waking --version
waking setup
waking --json doctor
```

For development or before the first release is published, build from the checked-out source:

```sh
cargo install --locked --path .
waking --json schema
waking --json doctor
```

Only source builds require Rust. See [installation, upgrade, and removal](docs/installation.md) for offline installation, platform limits, backups, and services.

## Use from Codex

Add this repository as a plugin marketplace:

```sh
codex plugin marketplace add https://github.com/Nunchakus888/waking
codex plugin add waking@waking --json
```

You can also install **Waking** through `/plugins` in a compatible local Codex host. After installation, ask:

> Set up Waking and connect this work resource to its existing Codex conversation. Keep replies as drafts.

The plugin provides setup, routing, and review skills plus local MCP tools. It does not install or start a daemon through hidden lifecycle hooks. Its setup conversation is not automatically the work destination. See [the plugin guide](docs/plugin.md) for local prerequisites and the clean-install verification procedure.

## First work event

The minimum useful setup has one verified source, one supported local host, and one explicit binding to an existing work thread.

```sh
waking --json schema
waking --json doctor
waking --json source list
waking --json host list
waking --json bindings
```

Use the installed schema and each command's `--help` to configure the source and host. Provide environment-variable names for credentials; do not paste secret values into a chat or command line. Select the canonical work resource and actual existing thread, then read the binding back. Start `waking serve` in that environment or explicitly install its user service. Configure the provider's webhook to the reachable receiver, then submit one harmless event in an authorized test resource.

Inspect each stage separately:

```sh
waking --json status
waking --json events list
waking --json jobs list
waking --json drafts list
```

“Received,” “completed in the original thread,” and “sent to the provider” are different outcomes. `doctor` and the event/job record should identify the next action when one stage is blocked. Full setup and recovery behavior is specified in [user journeys](docs/product.md) and [states](docs/states.md).

## Deployment choices

| Mode | Event receiver | Agent execution | Existing local history |
| --- | --- | --- | --- |
| Local | Reachable local webhook endpoint | Local machine | Available only through a supported local host |
| Hybrid | Your own online relay | Local machine over an outbound connection | Remains on the local host |
| Self-hosted cloud | Your server | Your server | Must exist on that server; local chats are not transferred automatically |

There is no assumed public hosted relay. Webhooks require a reachable HTTPS endpoint or an explicitly configured self-hosted relay. Provider credentials and agent login belong on the execution host where possible. A relay that receives webhook plaintext can read that payload; transport encryption is not end-to-end payload secrecy.

## Development and validation

```sh
cargo fmt --all -- --check
cargo clippy --locked --all-targets -- -D warnings
cargo test --locked
cargo build --locked
sh scripts/smoke.sh target/debug/waking
python3 scripts/check-plugin.py
```

Python is used only for repository/release checks, not for the installed product. The pinned Rust toolchain is in [rust-toolchain.toml](rust-toolchain.toml). Routine CI runs on one Linux runner. Successful main-branch CI publishes an unpublished package version through four native build/test jobs; an already published version skips that matrix. Tags and manual release runs are also supported. See [release controls](docs/installation.md#release-process).

## Documentation

- [Product definition and user journeys](docs/product.md)
- [Architecture and context routing](docs/architecture.md)
- [Status and recovery language](docs/states.md)
- [Installation and distribution](docs/installation.md)
- [Codex plugin and agent workflow](docs/plugin.md)
- [Acceptance criteria and evidence](docs/acceptance.md)
- [Official protocol and product references](docs/references.md)
- [Contribution guide](CONTRIBUTING.md) and [security policy](SECURITY.md)

## License and acknowledgements

[Apache License 2.0](LICENSE). Waking is an independent Rust implementation. [WakeWire](https://github.com/glenncalleja/wakewire) informed the event-bridge problem framing and adapter considerations. Referencing that project does not imply compatibility, affiliation, or copied implementation. See [references](docs/references.md) for official host and provider contracts.
