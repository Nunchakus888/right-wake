# Waking

**Wake the right thread.**

[简体中文](README.zh-CN.md)

Waking wakes the right **existing agent thread** when a work event arrives. It connects discussions in Linear, GitHub, and GitLab to the conversations already handling that work, so follow-up work can continue with its original context.

- **Wake on events.** Webhooks trigger work. While idle with no pending work, Waking submits no model turns; there is no periodic model polling, reducing unnecessary token use.
- **Route to the right conversation.** Reuse discussion and issue bindings first. When ownership is unknown, an optional event-triggered semantic router compares permitted context cards and recommends an existing thread for a one-time claim.
- **Continue the original context.** Address the same host and thread and resume the history held by that host. Context cards help choose the destination; the work continues in the original conversation. Waking does not create a replacement work thread.
- **Run locally or on your VPS.** Execute beside your existing local agent, on a self-hosted cloud server, or locally with an online relay receiving events while your machine is offline.

Waking is a native Rust binary with a CLI and Codex plugin. Persistent bindings record **work resource → host → thread**, so switching windows does not change the destination. Work stays queued while its host is busy, paused, or unavailable; ambiguous ownership waits for a claim. Replies start as inspectable drafts and need independent approval before publication.

Start with [the Codex plugin](#use-from-codex), [Linear setup](docs/linear.md), or [the deployment guide](docs/deployment.md).

## Project status

**[v0.1.0 is available as an experimental prerelease](https://github.com/Nunchakus888/waking/releases/tag/v0.1.0).** All four native targets passed 93 tests each, with no failures or ignored tests, plus build and installer smoke. The container build, receiver restart, and volume persistence checks also passed. Implementation, distribution, and real platform acceptance remain distinct in [the acceptance ledger](docs/acceptance.md).

Official **Codex CLI 0.166.0** passed a no-model host probe and clean-profile plugin installation from both the local repository and the **real GitHub marketplace URL**. The cached plugin's MCP launcher also passed initialization, tool listing, setup, and status checks. Real model continuation of an existing work thread, desktop concurrency, and authorized provider read/write E2E remain separate acceptance work. Automatic Codex dispatch requires a supported host, exclusive writer ownership, and restricted mutation tools; see [host compatibility](docs/host-compatibility.md).

Waking is an independent project; it is not an official OpenAI, Linear, GitHub, GitLab, or Anthropic product.

## Install

The ordinary native installation needs a shell, `curl`, `tar`, and a system SHA-256 tool. It needs **no Node.js, Python, Docker, Rust, or sudo**. Select macOS or glibc Linux on arm64 or x86_64. The installer downloads the matching pinned release, verifies its archive checksum and executable version, and installs in `~/.local/bin`. It does not start a service or modify Codex configuration.

Install the pinned `v0.1.0` release:

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

For development, build from the checked-out source:

```sh
cargo install --locked --path .
waking setup
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

> Set up Waking for this work resource so incoming events continue its existing Codex conversation. Inspect schema, doctor, and existing bindings first; confirm the exact destination and keep replies as drafts.

The plugin provides setup, routing, and review skills plus local MCP tools. It does not install or start a daemon through hidden lifecycle hooks. Its setup conversation is not automatically the work destination. See [the plugin guide](docs/plugin.md) for local prerequisites and the clean-install verification procedure.

## First work event

To try the signed-event, binding, existing-context, and local-draft flow without credentials:

```sh
waking --json demo
```

This explicitly uses two pre-existing **mock** threads. It makes no model or provider call; real Codex continuation has its own acceptance check.

The minimum useful setup has one verified source, one reachable event receiver, one supported execution host, and one explicit binding to an existing work thread on that host.

**Already connected to Linear MCP?** That provides tool access to Linear. Waking still needs a separately configured signed webhook, reachable receiver, source, and thread binding; an MCP connection alone does not establish event delivery. Follow [the Linear setup guide](docs/linear.md).

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

| Mode | Event receiver | Agent execution | Conversation history |
| --- | --- | --- | --- |
| Local | Local receiver behind a reachable HTTPS endpoint | Supported host on your local machine | Continues an existing thread on that machine |
| VPS / self-hosted cloud | Receiver on your server | Supported host on that server | Continues a thread already available on the server |
| Cloud relay + local execution | Your own online relay | Local host connects outward to the relay | Original local history remains on the local host |

Self-hosted cloud means **your own VPS or server running Waking and a supported agent host**. v0.1.0 has no OpenAI-hosted Codex Cloud adapter. A managed cloud task is not a continuously running Waking service, and deploying Waking on a server does not migrate a local thread or its login. An online relay can retain incoming events while your local machine is offline; execution resumes only when the local worker and host are available.

Webhooks need a reachable HTTPS endpoint or an explicitly configured self-hosted relay. There is no assumed public hosted relay. Keep provider credentials and agent login on the execution host where possible; the relay can read the webhook payload it receives. See [deployment and operation](docs/deployment.md) for the appropriate service and relay setup.

## Development and validation

```sh
cargo fmt --all -- --check
cargo clippy --locked --all-targets -- -D warnings
cargo test --locked
cargo build --locked
sh scripts/smoke.sh target/debug/waking
python3 scripts/check-plugin.py
```

Python is used for repository/release checks and the optional utility on a separate reviewer device, not by the installed Waking runtime. The pinned Rust toolchain is in [rust-toolchain.toml](rust-toolchain.toml). Routine CI runs on one Linux runner. Successful main-branch CI publishes an unpublished package version through four native build/test jobs; an already published version skips that matrix. Tags and manual release runs are also supported. See [release controls](docs/installation.md#release-process).

## Documentation

- [Linear webhook setup and existing-thread delivery](docs/linear.md)
- [Local, VPS, and cloud relay deployment](docs/deployment.md)
- [Product definition and user journeys](docs/product.md)
- [Architecture and context routing](docs/architecture.md)
- [Status and recovery language](docs/states.md)
- [Installation and distribution](docs/installation.md)
- [Codex plugin and agent workflow](docs/plugin.md)
- [Acceptance criteria and evidence](docs/acceptance.md)
- [Configuration and adapter examples](examples/README.md)
- [Independent review and publication](docs/approvals.md)
- [Official protocol and product references](docs/references.md)
- [Contribution guide](CONTRIBUTING.md) and [security policy](SECURITY.md)

## License and acknowledgements

[Apache License 2.0](LICENSE). Waking is an independent Rust implementation. [WakeWire](https://github.com/glenncalleja/wakewire) informed the event-bridge problem framing and adapter considerations. Referencing that project does not imply compatibility, affiliation, or copied implementation. See [references](docs/references.md) for official host and provider contracts.
