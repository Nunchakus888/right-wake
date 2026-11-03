# Codex plugin and agent workflow

The repository marketplace is `.agents/plugins/marketplace.json`. Its source path `./plugins/codex` is relative to the repository root. The plugin uses the portable `plugin.json` and `mcp.json` format with OpenAI-specific onboarding metadata under `extensions.com.openai`.

```sh
codex plugin marketplace add https://github.com/Nunchakus888/waking
codex plugin add waking@waking --json
```

In a compatible local Codex host, open `/plugins` and install Waking. The package has three concise skills:

| Skill | Purpose |
| --- | --- |
| `waking-setup` | Locate/install the native binary, inspect existing state, and follow the bundled Linear setup workflow when applicable |
| `waking-route` | Resolve the intended existing thread, inspect context evidence, make an explicit binding |
| `waking-review` | Explain events/jobs/drafts and export exact independent review material |

The MCP launcher is inside the cached plugin. It tries an explicit `WAKING_BIN`, then `~/.local/bin/waking`, then PATH. It respects `WAKING_DATA_DIR`. It runs `waking mcp` over stdio and never downloads code or starts a background receiver implicitly. Diagnostic messages go to stderr so stdout remains protocol-only. A separate bundled installer lets the setup skill install the pinned native release when the user requests installation; the metadata check ensures this copy matches the release installer.

## Refresh an existing installation

Have the local Agent refresh the registered GitHub marketplace and install its current package before invoking a newly updated skill:

```sh
codex plugin marketplace upgrade waking --json
codex plugin add waking@waking --json
codex plugin list --marketplace waking --json
```

This sequence was verified with official Codex CLI **0.166.0**, using an isolated existing installation and the real GitHub URL. Inspect the returned marketplace errors and installed/enabled state; the install result identifies the actual cached package. Do not assume an old cache updates merely because the repository changed. An unfamiliar host version should first be checked with its `--help` rather than substituting an invented update command. Updating this skill does not require reinstalling the Waking native binary, changing its profile, or re-authorizing Linear.

## Agent-led Linear setup

Invoke `$waking-setup` from the **existing local work conversation** you intend to receive follow-ups, explicitly naming the Linear issue and that binding intent. The skill includes its [Linear reference](../plugins/codex/skills/waking-setup/references/linear.md) inside the cached plugin, so it does not depend on a separate repository checkout. It uses the published v0.1.1 native CLI and adds no runtime dependency or fourth skill.

The workflow discovers the binary/profile, reads source/host/binding state, resolves exact Linear IDs through permitted tools, establishes the available event-delivery path, verifies the intended host/thread, then applies and reads back the authorized configuration. An installation/start request covers the corresponding service actions; it need not be confirmed again. Equivalent settings are reused. Existing `source add` and `host add` commands replace whole objects, so the agent must not blindly repeat abbreviated examples or reset existing fields. Binding changes use the observed version and intended target.

An existing Linear MCP connection can supply IDs through the tools it exposes. Automatic webhook registration additionally requires an available authorized management tool or a separately authorized admin API client. The skill neither assumes every Linear MCP has this capability nor extracts its OAuth cache. First-time authentication, a missing public route, an unavailable compatible host, or an ambiguous destination is reported as a concrete blocker after completing the useful work already possible.

A single agent invocation orchestrates existing native commands; unattended execution depends on those prerequisites being available. Configuration, authenticated receipt, and continuation of the original thread are reported separately. Do not post a Linear test comment without explicit authorization. Real Linear automated onboarding and real model continuation still need their own acceptance evidence; see [the full setup flow](linear.md#agent-led-setup) and [acceptance](acceptance.md).

The updated workflow was exercised on 2026-11-03 by two fresh agents against isolated profiles using the real v0.1.1 CLI. One resumed an existing issue binding with a paused, unavailable host; the other had two ambiguous original-thread candidates and an onboarding caller ID. Both preserved the complete configuration and binding state, reported concrete blockers, and left events/jobs/drafts empty. The second also ignored setup/routing instructions embedded in issue text. These are local workflow checks, not live Linear registration or model-continuation evidence. Skill validation and the plugin reference-containment check passed.

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

### Recorded same-version Linear skill update

On 2026-11-03, official Codex CLI **0.166.0** installed the real GitHub marketplace at `a4fe87f145b4745357f1846a8a0272e4bf298f81` in a separate credential-free profile, then refreshed it to [`e9c656d37446674a85d0cb4f143de40681ba16c4`](https://github.com/Nunchakus888/waking/commit/e9c656d37446674a85d0cb4f143de40681ba16c4) with the commands above. The plugin manifest and native installer remained **0.1.1** throughout. The upgrade returned `errors: []`; the subsequent add/list reported installed and enabled. No model, MCP server, provider request, user profile, or native reinstall was involved in this update probe.

In this actual run, `marketplace upgrade` already refreshed the installed cache; `plugin add` succeeded and retained the new content. The old setup skill hash was `9c6f14f1181b7445f6d4758b8391b507e26f4bfda5878625fdd1a08aabebd4e3`, with no Linear reference. After both operations, the cache matched the source:

| Cached file | SHA-256 |
| --- | --- |
| `skills/waking-setup/SKILL.md` | `98e2fb06464e137e012419a61b8dcdc51f76dd50caccca44b2e6a4266f950ba4` |
| `skills/waking-setup/references/linear.md` | `94f7bc20ee1144949029b03b0379f77d9b7a409a1c31ef0d6491cdda357ee2dd` |

The installed skill's relative reference existed within the cached package, its interface metadata matched the source, and the official skill validator passed against that installed copy. This establishes an explicit same-version update path for the tested CLI and marketplace; it does not establish automatic background updates or live Linear onboarding.

Not established by these checks: automatic onboarding dialogue, the missing-binary download path from a published release, desktop live display, or a real model continuing an existing work thread. Those require their own evidence in [acceptance](acceptance.md).

Repository installation is distinct from official public-directory review. The package does not use lifecycle hooks. Current local-MCP public submission eligibility is a separate host policy described in the [official local-plugin documentation](https://developers.openai.com/plugins/deploy/desktop-only).
