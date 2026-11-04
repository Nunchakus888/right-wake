# Codex plugin and agent workflow

The repository marketplace is `.agents/plugins/marketplace.json`. Its source path `./plugins/codex` is relative to the repository root. The plugin uses the portable `plugin.json` and `mcp.json` format with OpenAI-specific onboarding metadata under `extensions.com.openai`.

The display name is **RightWake**. Its marketplace and plugin identifiers are both `right-wake`; install `right-wake@right-wake`:

```sh
codex plugin marketplace add https://github.com/Nunchakus888/right-wake
codex plugin add right-wake@right-wake --json
```

In a compatible local Codex host, open `/plugins` and install RightWake. The package has three concise skills:

| Skill | Purpose |
| --- | --- |
| `right-wake-setup` | Locate/install the native binary, inspect existing state, and follow the bundled Linear setup workflow when applicable |
| `right-wake-route` | Resolve the intended existing thread, inspect context evidence, make an explicit binding |
| `right-wake-review` | Explain events/jobs/drafts and export exact independent review material |

The MCP launcher is inside the cached plugin. It tries an explicit `RIGHT_WAKE_BIN`, then `~/.local/bin/right-wake`, then PATH. It respects `RIGHT_WAKE_DATA_DIR`. It runs `right-wake mcp` over stdio and never downloads code or starts a background receiver implicitly. Diagnostic messages go to stderr so stdout remains protocol-only. A separate bundled installer lets the setup skill install the pinned native release when the user requests installation; the metadata check ensures this copy matches the release installer.

## Refresh an existing installation

Have the local Agent refresh the registered GitHub marketplace and install its current package before invoking a newly updated skill:

```sh
codex plugin marketplace upgrade right-wake --json
codex plugin add right-wake@right-wake --json
codex plugin list --marketplace right-wake --json
```

Use these commands for a marketplace already registered as `right-wake`. Inspect returned errors and installed/enabled state; the install result identifies the actual cached package. An unfamiliar host version should first be checked with its `--help`. A skill-only update does not require changing its profile or re-authorizing Linear. Personal Linear filtering and queue delivery require the **0.3.0 native binary** as well as the updated skill. Back up and reuse the existing `right-wake` profile, source IDs, bindings, secrets, and saved onboarding record; a fresh profile would discard that continuity.

## Agent-led Linear setup

Invoke `$right-wake-setup` from the **existing local work conversation** you intend to receive follow-ups, explicitly naming the Linear issue and that binding intent. To watch personal work, request new comments on your assigned issues and newly assigned tasks. The skill includes its [Linear reference](../plugins/codex/skills/right-wake-setup/references/linear.md) inside the cached plugin, so it does not depend on a separate repository checkout. It targets the v0.3.0 native CLI and adds no runtime dependency or fourth skill.

The workflow discovers the binary/profile, reads source/host/binding state, resolves exact Linear IDs through permitted tools, establishes the available event-delivery path, verifies the intended host/thread, then applies and reads back the authorized configuration. An installation/start request covers the corresponding service actions; it need not be confirmed again. Equivalent settings are reused. Existing `source add` and `host add` commands replace whole objects, so the agent must not blindly repeat abbreviated examples or reset existing fields. Binding changes use the observed version and intended target.

An existing Linear MCP connection can supply IDs through the tools it exposes. Automatic webhook registration additionally requires an available authorized management tool or a separately authorized admin API client. The skill neither assumes every Linear MCP has this capability nor extracts its OAuth cache. First-time authentication, a missing public route, an unavailable compatible host, or an ambiguous destination is reported as a concrete blocker after completing the useful work already possible.

For personal watching, setup resolves the authenticated user and organization, configures `linear_assignee_id`, and imports verified issue/assignee/revision snapshots. A prior onboarding file can be reused only after inspecting its actual format and verifying missing fields through authorized tools. The issue list is a partial baseline, not a permanent allowlist or proof of active subscription. New tasks still need an existing-thread binding or event-triggered routing/claim; the setup conversation does not become an automatic catch-all.

Direct tool-output delivery retains its actual exclusive-writer requirement. The optional `--delivery queue` uses the same owning App Server's atomic queue and requires `right_wake_dispatch_context` in the target thread's tool set, connected to the worker's profile. A second App Server opened over the same history is not shared Desktop ownership. Both modes need a real tool/credential restriction; changing a declaration cannot create that boundary. See [host compatibility](host-compatibility.md).

When an administrator must register the webhook, finish the independent local work and prepare a secret-free handoff with exact organization/user/team coverage, the resolved endpoint or its unresolved dependency, Issue and Comment resource types, and private signing-secret storage references. The skill does not send this handoff to anyone without explicit messaging authorization.

A single agent invocation orchestrates existing native commands; unattended execution depends on those prerequisites being available. Configuration, authenticated receipt, and continuation of the original thread are reported separately. Do not post a Linear test comment without explicit authorization. Real Linear automated onboarding and real model continuation still need their own acceptance evidence; see [the full setup flow](linear.md#agent-led-setup) and [acceptance](acceptance.md).

Earlier workflow trials are kept in the [historical evidence archive](archive/README.md). New-version validation belongs in [v0.3.0 verification](releases/v0.3.0-verification.md).

## Use the same local profile

If CLI setup or a deployment guide uses a custom `--data-dir`, configure `RIGHT_WAKE_DATA_DIR` with that same absolute path in the environment of the plugin's MCP process. A `--data-dir` flag on a previous CLI command does not change the plugin's default. For a Codex process launched from your terminal, export `RIGHT_WAKE_DATA_DIR` before starting that process; an already running desktop app does not inherit later shell changes. Use the host's supported environment configuration and restart the relevant MCP connection when needed.

Ask `right_wake_setup` to report its `data_dir` and compare it with the receiver/local worker's configured directory before adding a binding. In a hybrid deployment, the plugin manages the **local worker's** profile, while the cloud relay keeps its separate profile. Do not copy the receiver's secret values into plugin metadata to select a profile.

An onboarding skill may run in a new setup conversation. That conversation is not automatically the destination for work. Resolve a real existing work thread through the supported host and the user's intended scope. Never derive ownership from a window title or event payload.

Agents read the installed `schema` and MCP tool list instead of guessing flags. Setup preserves existing configuration. Source credentials are named environment references. Bindings are explicit and versioned. Draft review does not authorize a send; a separate configured approver must sign the exact current action. The agent must not create or access the approver's private signing key to satisfy its own approval check.

## Clean-profile verification

Use an authorized disposable Codex profile and test resource, without modifying the user's working profile:

1. Record Codex version, OS, and plugin-format support.
2. Add the real repository marketplace and install from its cached copy.
3. Confirm setup finds a preinstalled binary, then separately test the missing-binary guidance.
4. Confirm the local MCP server initializes and tools list correctly.
5. Run setup twice and verify no duplicate source, host, or service is created.
6. Select and bind an existing work conversation; check setup's conversation was not used accidentally.
7. Deliver an authorized real event and record actual thread/turn/completion and draft evidence.

`python3 scripts/check-plugin.py` and skill validators check local structure only. They do not constitute this host installation test.

## Verification status

Current **0.3.0** installation, upgrade, skill-validation, and twelve-tool MCP results are recorded in [v0.3.0 verification](releases/v0.3.0-verification.md). The records below describe the already-published **0.2.0** package and are preserved as historical evidence; they do not establish a new-version result.

On 2026-11-04, official Codex CLI **0.166.0** installed the local repository marketplace in a fresh credential-free profile. `right-wake@right-wake` **0.2.0** was installed and enabled; all **11 cached package files** matched the source byte for byte. Exactly three `right-wake-*` skills passed the official validator from inside the installed cache. The MCP server key was `right-wake`, and the interface slogan was **Wake the right Agent thread.** This check did not start a model, MCP process, provider operation, service, or user profile. Native cached-launcher and public-install results are recorded separately in [v0.2.0 verification](releases/v0.2.0-verification.md).

The same installed cache then launched the actual 0.2.0 native binary through `scripts/mcp.sh`, using `RIGHT_WAKE_BIN` and a separate `RIGHT_WAKE_DATA_DIR`. MCP initialization reported `right-wake` 0.2.0, all 11 tools used `right_wake_*`, and repeated setup plus status passed. This was a real stdio launcher check without a model or external provider.

A second fresh credential-free Codex profile installed the real GitHub marketplace URL `https://github.com/Nunchakus888/right-wake`. Its repository snapshot resolved to [`533e57560e36780b7a3688a375c7ec07cf9fb8ef`](https://github.com/Nunchakus888/right-wake/commit/533e57560e36780b7a3688a375c7ec07cf9fb8ef), with `right-wake@right-wake` 0.2.0 installed and enabled. All 11 cached files matched that commit, and all three cached skills passed validation. This installed cache's launcher also initialized the actual 0.2.0 native binary, exposed the 11 `right_wake_*` tools, and completed setup/status against a separate database. After publication, the same GitHub-cached launcher passed again with the binary installed directly from the public release assets. No authentication, model turn, or provider call was used. This verifies GitHub retrieval, the cached package path, and discovery of the public native binary; real model continuation and Linear onboarding remain separate checks.

Earlier CLI/plugin installation and same-version cache-update checks are available in the [historical archive](archive/README.md). Those observations do not establish the new package's installation or model-continuation behavior.

Repository installation is distinct from official public-directory review. The package does not use lifecycle hooks. Current local-MCP public submission eligibility is a separate host policy described in the [official local-plugin documentation](https://developers.openai.com/plugins/deploy/desktop-only).
