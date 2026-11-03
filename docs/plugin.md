# Codex plugin and agent workflow

The repository marketplace is `.agents/plugins/marketplace.json`. Its source path `./plugins/codex` is relative to the repository root. The plugin uses the portable `plugin.json` and `mcp.json` format with OpenAI-specific onboarding metadata under `extensions.com.openai`.

```sh
codex plugin marketplace add https://github.com/Nunchakus888/waking
codex plugin add waking@waking --json
```

In a compatible local Codex host, open `/plugins` and install Waking. The package has three concise skills:

| Skill | Purpose |
| --- | --- |
| `waking-setup` | Locate/install the native binary, inspect `schema`/`doctor`, configure a source and host |
| `waking-route` | Resolve the intended existing thread, inspect context evidence, make an explicit binding |
| `waking-review` | Explain events/jobs/drafts and export exact independent review material |

The MCP launcher is inside the cached plugin. It tries an explicit `WAKING_BIN`, then `~/.local/bin/waking`, then PATH. It respects `WAKING_DATA_DIR`. It runs `waking mcp` over stdio and never downloads code or starts a background receiver implicitly. Diagnostic messages go to stderr so stdout remains protocol-only. A separate bundled installer lets the setup skill install the pinned native release when the user requests installation; the metadata check ensures this copy matches the release installer.

## Use the same local profile

If CLI setup or a deployment guide uses a custom `--data-dir`, configure `WAKING_DATA_DIR` with that same absolute path in the environment of the plugin's MCP process. A `--data-dir` flag on a previous CLI command does not change the plugin's default. For a Codex process launched from your terminal, export `WAKING_DATA_DIR` before starting that process; an already running desktop app does not inherit later shell changes. Use the host's supported environment configuration and restart the relevant MCP connection when needed.

Ask `waking_setup` to report its `data_dir` and compare it with the receiver/local worker's configured directory before adding a binding. In a hybrid deployment, the plugin manages the **local worker's** profile, while the cloud relay keeps its separate profile. Do not copy the receiver's secret values into plugin metadata to select a profile.

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

## Recorded local installation result

On 2026-11-03, the official **Codex CLI 0.166.0** installed `waking@waking` version `0.1.0` from the local repository marketplace in a newly created, isolated empty Codex profile on Linux x86_64. `plugin marketplace add`, `plugin add`, and `plugin list --marketplace waking --json` all returned success; the list reported `installed: true` and `enabled: true`. The cached package contained all three skills.

The probe used an allowlisted environment with a separate configuration/cache/data directory, no inherited API keys or login, and no model turn. The original user profile was not the installation target. Reproduce the marketplace portion with a disposable profile and the actual repository path:

```sh
WAKING_PLUGIN_TEST_DIR=$(mktemp -d)
env -i PATH="$PATH" LANG=C.UTF-8 \
  CODEX_HOME="$WAKING_PLUGIN_TEST_DIR/codex" \
  codex plugin marketplace add "$PWD" --json
env -i PATH="$PATH" LANG=C.UTF-8 \
  CODEX_HOME="$WAKING_PLUGIN_TEST_DIR/codex" \
  codex plugin add waking@waking --json
env -i PATH="$PATH" LANG=C.UTF-8 \
  CODEX_HOME="$WAKING_PLUGIN_TEST_DIR/codex" \
  codex plugin list --marketplace waking --json
```

Separately, the **cached package's** `scripts/mcp.sh` launched the real Waking 0.1.0 binary with an isolated data directory. MCP initialization, listing 11 tools, `waking_setup`, and `waking_status` passed. This was a direct cached-launcher protocol check; it did not involve a Codex model invoking a tool.

A second fresh isolated profile successfully installed the real Git marketplace URL `https://github.com/Nunchakus888/waking` at commit `a9e8f2878dc4f82bb5a9f320b2c0a4b49fb9934d`. The official CLI reported `marketplaceSource.sourceType: git`, `installed: true`, and `enabled: true`. All three cached skills were present. The cached launcher again passed MCP initialization, 11 tools, setup, and status using the local 0.1.0 release binary. Standard existing proxy and CA variables were preserved for network transport; no API credential or login was inherited.

Not established by these checks: automatic onboarding dialogue, the missing-binary download path from a published release, desktop live display, or a real model continuing an existing work thread. Those require their own evidence in [acceptance](acceptance.md).

Repository installation is distinct from official public-directory review. The package does not use lifecycle hooks. Current local-MCP public submission eligibility is a separate host policy described in the [official local-plugin documentation](https://developers.openai.com/plugins/deploy/desktop-only).
