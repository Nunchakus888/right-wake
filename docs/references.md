# Official contracts and product references

Reviewed during the initial design on 2026-11-03. Interfaces can change; the actual tested host/provider version belongs in the acceptance evidence.

| Reference | What it establishes |
| --- | --- |
| [Codex App Server](https://learn.chatgpt.com/docs/app-server) | Public thread/turn protocol; a documented method is not proof of target-version compatibility |
| [Codex in Linear](https://learn.chatgpt.com/docs/third-party/linear) | Native cloud task integration; distinct from arbitrary local-thread attachment |
| [OpenAI MCP Events](https://developers.openai.com/plugins/build/mcp-events) | Event subscription on supported Work cloud surfaces; do not infer local desktop support |
| [Plugin packaging](https://developers.openai.com/plugins/build/plugins) | Portable plugin, marketplace, and onboarding fields |
| [Local MCP plugin distribution](https://developers.openai.com/plugins/deploy/desktop-only) | Local-executor prerequisites and separate public-submission rules |
| [Portable plugin schema](https://agent-plugins.org/schemas/1.0.0/plugin.schema.json), [MCP schema](https://agent-plugins.org/schemas/1.0.0/mcp.schema.json) | Manifest field contracts used by the package |
| [Linear webhooks](https://linear.app/developers/webhooks), [agents](https://linear.app/developers/agents), [agent interactions](https://linear.app/developers/agent-interaction) | Provider event identity and application-agent scope; not a promise of the installer's entire personal Inbox |
| [Linear MCP](https://linear.app/docs/mcp) | Tool access does not by itself establish `events/list` or `events/subscribe` support |
| [GitHub signature validation](https://docs.github.com/en/webhooks/using-webhooks/validating-webhook-deliveries), [failed deliveries](https://docs.github.com/en/webhooks/using-webhooks/handling-failed-webhook-deliveries) | Exact-body HMAC and delivery recovery; do not assume automatic redelivery |
| [GitLab webhooks](https://docs.gitlab.com/user/project/integrations/webhooks/), [event payloads](https://docs.gitlab.com/user/project/integrations/webhook_events/) | Supported event identity and project/instance boundaries |
| [Claude Tag behavior](https://claude.com/resources/articles/claude-tag-now-reads-even-more-of-the-room), [identity and memory](https://support.claude.com/en/articles/15594475-what-is-claude-tag) | Context-aware Slack participation and existing workstream routing; no claim about resuming local Codex threads |
| [WakeWire](https://github.com/glenncalleja/wakewire) | Open-source event-bridge reference; Waking is an independent Rust implementation |
| [GitHub-hosted runners](https://docs.github.com/en/actions/reference/runners/github-hosted-runners) | Native release-runner labels; actual workflow execution still needs evidence |
| [systemd service](https://www.freedesktop.org/software/systemd/man/latest/systemd.service.html), [Apple launchd](https://developer.apple.com/library/archive/documentation/MacOSX/Conceptual/BPSystemStartup/Chapters/CreatingLaunchdJobs.html) | Native user service definition semantics |
| [Apache License 2.0](https://www.apache.org/licenses/LICENSE-2.0.txt) | Source of the unmodified repository license text |
