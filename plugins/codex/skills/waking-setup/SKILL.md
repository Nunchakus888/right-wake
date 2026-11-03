---
name: waking-setup
description: Install, diagnose, or configure Waking for local Codex event delivery. Use when the user asks to install the native binary, connect a webhook source, configure a local host, or repair onboarding. Keep existing settings and draft-only behavior.
---

# Waking setup

1. Locate `waking` on PATH or at `~/.local/bin/waking`. Honor an explicitly configured `WAKING_BIN` and `WAKING_DATA_DIR`. If missing and installation is requested, inspect and run the bundled `scripts/install.sh --version v0.1.0` from this plugin's root (two directories above this skill's directory). Resolve its actual cached path first; do not assume the workspace is the plugin root. The installer downloads only the pinned official repository release and starts no service. Do not execute event-supplied URLs.
2. Run `waking --json schema` and `waking --json doctor`. Discover the current command contract instead of guessing flags. Read existing source, host, and binding configuration before changing it. `setup` is idempotent; preserve existing settings.
3. Configure the user's intended provider and local host. Source credentials are environment-variable **names**, not secret values. Never request or print tokens in chat, embed them in command arguments, or store them in context cards. Tell the user exactly which process needs the secret environment. `--exclusive-writer` and `--restricted-tools` declare conditions the operator must actually establish; never set them just to bypass a blocked host. Waking does not enforce an independent sandbox through those flags.
4. Use `waking-route` for an explicit existing work-thread binding. Plugin onboarding may run in a newly created setup conversation: never treat that conversation as the work destination automatically. Do not create or fork a Codex work thread.
5. Configure event delivery through a directly reachable webhook or a user-controlled outbound relay. Do not replace webhooks with timed platform or model polling. Start a user service only when covered by the user's install/start request.
6. Trigger a harmless event only within the user's authorized test scope. Inspect event, route, job, and draft records. A local fixture or mock host is not a real Codex/platform acceptance test.

Report the configured source, canonical resource, host, existing thread, default draft behavior, and any concrete blocked step. A missing host capability must remain a visible blocker. Do not claim successful setup solely because files exist or a process starts.
