# RightWake

**Wake the right Agent thread.**

Events and tasks wake the correct existing Agent thread  
so work continues in its original context.

[简体中文](README.zh-CN.md)

- **Wake on events.** Webhooks trigger work. While idle with no pending work, RightWake submits no model turns; there is no periodic model polling, reducing unnecessary token use.
- **Route to the right conversation.** Reuse discussion and issue bindings first. When ownership is unknown, an optional event-triggered semantic router compares permitted context cards and recommends an existing thread for a one-time claim.
- **Continue the original context.** Address the same host and thread and resume the history held by that host. Context cards help choose the destination; the work continues in the original conversation. RightWake does not create a replacement work thread.
- **Run locally or on your VPS.** Execute beside your existing local agent, on a self-hosted cloud server, or locally with an online relay receiving events while your machine is offline.

RightWake is one native Rust binary with a CLI, Codex plugin, and **embedded SQLite; no separate database service is needed**. Persistent bindings record **work resource → host → thread**, so switching windows does not change the destination. Busy, paused, or unavailable hosts keep their queue; ambiguous ownership waits for a claim. Replies remain drafts until independently approved.

Start with [the Codex plugin](#use-from-codex), [Linear setup](docs/linear.md), or [the deployment guide](docs/deployment.md).

## Project status

This branch prepares **v0.3.1**, adding saved fixed-relay connections, transport recovery, and native relay-worker service management. It reuses the 0.3.0 relay protocol and SQLite store. Until a release is verified, use a source build for the new commands; the published 0.3.0 installer below does not contain them. See [changes and boundaries](docs/releases/v0.3.1.md) and [v0.3.1 evidence](docs/releases/v0.3.1-verification.md).

**[v0.3.0 is available as an experimental prerelease](https://github.com/Nunchakus888/right-wake/releases/tag/v0.3.0)**, with personal Linear watching and opt-in Codex queue delivery. Watch new comments on issues assigned to you and newly assigned tasks, without adding a mention to each comment or freezing today's issue list. Ownership changes update a durable local projection; unknown ownership waits without calling a model. Each of four native platforms passed **129 tests, 0 failures, 0 ignored**; both Linux Docker/Compose checks passed. See [v0.3.0 verification](docs/releases/v0.3.0-verification.md) for exact runs and installation evidence. Earlier results remain in [v0.2.0 verification](docs/releases/v0.2.0-verification.md).

Automatic Codex delivery uses either direct tool-output with actual exclusive writer control, or an explicitly selected experimental queue on the original thread's owning App Server. Both require an appropriate tool/credential boundary; the queue also needs the local dispatch-context MCP tool. Live Desktop continuation and provider E2E remain separate acceptance work. See [host compatibility](docs/host-compatibility.md) and [acceptance](docs/acceptance.md).

RightWake is an independent project; it is not an official OpenAI, Linear, GitHub, GitLab, or Anthropic product.

## Install

The command is `right-wake`; the Codex plugin is `right-wake@right-wake`. Upgrading from 0.2.0 preserves the same profile, sources, bindings, and onboarding baseline after a backup. The database receives additive schema updates; no old-name aliases are installed.

**Use native installation beside your original local agent; use [Docker + Compose](deploy/README.md) for a VPS relay.** Native installation needs a shell, `curl`, `tar`, and a system SHA-256 tool, with **no Node.js, Python, Docker, Rust, or sudo**. It supports macOS and glibc Linux on arm64/x86_64. The installer verifies the archive and version, installs in `~/.local/bin`, and starts no service.

Install the pinned **v0.3.0** release after checking its [publication status](docs/releases/v0.3.0-verification.md):

```sh
curl --proto '=https' --tlsv1.2 --fail --location \
  --output right-wake-install.sh \
  https://github.com/Nunchakus888/right-wake/releases/download/v0.3.0/install.sh
# Inspect the downloaded installer, then run it.
sh right-wake-install.sh --version v0.3.0
export PATH="$HOME/.local/bin:$PATH"
right-wake --version
right-wake setup
right-wake --json doctor
```

For development, build from the checked-out source:

```sh
cargo install --locked --path .
right-wake setup
right-wake --json schema
right-wake --json doctor
```

Only source builds require Rust. Native configuration and SQLite state default to `~/.local/share/right-wake` (or `RIGHT_WAKE_DATA_DIR` / `--data-dir`). Docker keeps them in its named volume at `/data`; no database container is required. The [VPS guide](deploy/README.md) loads the matching amd64/arm64 release image with `docker load` and starts Compose explicitly. See [installation and lifecycle](docs/installation.md) for backups, upgrades, and services.

## Use from Codex

Add this repository as a plugin marketplace:

```sh
codex plugin marketplace add https://github.com/Nunchakus888/right-wake
codex plugin add right-wake@right-wake --json
```

You can also install **RightWake** through `/plugins` in a compatible local Codex host. For Linear, invoke the setup skill **from the existing local work conversation you want to continue**, replacing the example issue:

> Use $right-wake-setup to install and configure RightWake for Linear issue ENG-123, bound to this existing work conversation. Reuse my authorized connections and existing configuration, start the required local receiver, and verify each available stage. Keep replies as drafts and report any concrete blocker.

For your personal Linear inbox, ask:

> Use $right-wake-setup to upgrade and configure RightWake for new comments on issues assigned to me and newly assigned tasks. Preserve my profile, existing bindings, and saved onboarding baseline. Resolve each event to its correct existing work thread, prepare any missing administrator handoff, and report receipt and original-thread execution separately. Keep replies as drafts.

The same setup skill includes a [Linear workflow](docs/linear.md#agent-led-setup) for discovering existing settings, installing, configuring, and checking the result. With the required access, endpoint, and exact host/thread available, the agent can proceed in one invocation. A saved issue baseline seeds filtering; it does not register a webhook or assign every issue to the setup conversation. Missing admin access or a public endpoint produces a concrete handoff after the available local work is complete.

The plugin has three skills and twelve local MCP tools. Setup automation comes from the skill coordinating native CLI commands. Plugin installation has no hidden daemon-start hook, and a new setup conversation is not automatically the work destination. See [the plugin guide](docs/plugin.md).

For an existing `right-wake` installation, have the Agent [refresh its plugin package](docs/plugin.md#refresh-an-existing-installation) when the skills change, and check that its native binary satisfies the installed package's version requirement.

## First work event

To try the signed-event, binding, existing-context, and local-draft flow without credentials:

```sh
right-wake --json demo
```

This explicitly uses two pre-existing **mock** threads. It makes no model or provider call; real Codex continuation has its own acceptance check.

The minimum useful setup has one verified source, one reachable event receiver, one supported execution host, and one explicit binding to an existing work thread on that host.

**Already connected to Linear MCP?** That provides tool access to Linear. RightWake still needs a separately configured signed webhook, reachable receiver, source, and thread binding; an MCP connection alone does not establish event delivery. Follow [the Linear setup guide](docs/linear.md).

```sh
right-wake --json schema
right-wake --json doctor
right-wake --json source list
right-wake --json host list
right-wake --json bindings
```

Use the installed schema and each command's `--help` to configure the source and host. Provide environment-variable names for credentials; do not paste secret values into a chat or command line. Select the canonical work resource and actual existing thread, then read the binding back. Start `right-wake serve` in that environment or explicitly install its user service. Configure the provider's webhook to the reachable receiver, then submit one harmless event in an authorized test resource.

Inspect each stage separately:

```sh
right-wake --json status
right-wake --json events list
right-wake --json jobs list
right-wake --json drafts list
```

“Received,” “completed in the original thread,” and “sent to the provider” are different outcomes. `doctor` and the event/job record should identify the next action when one stage is blocked. Full setup and recovery behavior is specified in [user journeys](docs/product.md) and [states](docs/states.md).

## Deployment choices

| Mode | Event receiver | Agent execution | Conversation history |
| --- | --- | --- | --- |
| Local | Local receiver behind a reachable HTTPS endpoint | Supported host on your local machine | Continues an existing thread on that machine |
| VPS / self-hosted cloud | Receiver on your server | Supported host on that server | Continues a thread already available on the server |
| Cloud relay + local execution | Your own online relay | Local host connects outward to the relay | Original local history remains on the local host |

Self-hosted cloud means **your own VPS or server**. There is no OpenAI-hosted Codex Cloud adapter. A Docker relay receives and stores events; your native local worker resumes the original local thread when available. It does not migrate that history or login into the container. Running execution on the VPS instead requires a supported agent host and existing thread on that VPS.

Webhooks need a reachable HTTPS endpoint or an explicitly configured self-hosted relay. There is no assumed public hosted relay. Keep provider credentials and agent login on the execution host where possible; the relay can read the webhook payload it receives. See [deployment and operation](docs/deployment.md) for the appropriate service and relay setup.

For a **fixed endpoint and receipt while the laptop is offline**, use a controlled HTTPS/WSS cloud relay with persistent storage, plus the native local worker. Reuse the existing local profile and paired device. In 0.3.1, `relay configure` saves the connection and `service install --mode relay-connect` supervises it through the existing user service. The service reconnects to the saved URL; it does not provision a domain/server, launch a tunnel or establish a Desktop execution boundary. An anonymous Quick Tunnel is a temporary trial, not this deployment. See [the cloud/local workflow](docs/deployment.md#cloud-relay-plus-local-worker).

## Development and validation

```sh
cargo fmt --all -- --check
cargo clippy --locked --all-targets -- -D warnings
cargo test --locked
cargo build --locked
sh scripts/smoke.sh target/debug/right-wake
python3 scripts/check-plugin.py
```

Python is used for repository/release checks and the optional utility on a separate reviewer device, not by the installed RightWake runtime. The pinned Rust toolchain is in [rust-toolchain.toml](rust-toolchain.toml). Routine CI runs on one Linux runner. Successful main-branch CI publishes an unpublished package version through four native build/test jobs; an already published version skips that matrix. Tags and manual release runs are also supported. See [release controls](docs/installation.md#release-process).

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

[Apache License 2.0](LICENSE). RightWake is an independent Rust implementation. [WakeWire](https://github.com/glenncalleja/wakewire) informed the event-bridge problem framing and adapter considerations. Referencing that project does not imply compatibility, affiliation, or copied implementation. See [references](docs/references.md) for official host and provider contracts.
