# Connect Linear to an existing local thread

Waking receives signed Linear webhooks, stores them durably, and routes relevant events to an explicitly configured existing host/thread. The normal result is a local reply draft. Registering a webhook, obtaining its signing secret, establishing a reachable HTTPS endpoint, and assigning the existing thread are separate setup steps.

If Linear MCP is already connected to your agent, keep that connection for its permitted tool operations. It does **not** automatically create Waking's webhook, public endpoint, signing-secret configuration, source scope, or thread binding. Waking does not import or reuse the MCP connection's OAuth credentials. Linear's [MCP documentation](https://linear.app/docs/mcp) describes that tool integration, including its read-only endpoint; Linear's [webhook documentation](https://linear.app/developers/webhooks) describes the event transport used here.

Receiving events and producing drafts do not require a Linear API token. They require the webhook signing secret at the receiver and a separately working execution host. Sending a draft adds an independently configured API credential, verified sending identity, and human Ed25519 approval.

Install the published [v0.1.0 native release](https://github.com/Nunchakus888/waking/releases/tag/v0.1.0) using the [installation guide](installation.md), then use `waking --json schema` to inspect the installed command contract.

## 1. Choose where the receiver runs

For an online local trial, run Waking beside the host that owns the conversation and give its receiver a public HTTPS route through your existing tunnel or reverse proxy. The internal listener can remain `127.0.0.1:7433`; Linear must reach the public HTTPS URL, not that loopback address.

If the computer may be offline, use the [cloud relay plus local worker deployment](deployment.md#cloud-relay-plus-local-worker). The relay receives and stores webhooks while the local worker reconnects over outbound WSS. The original thread remains on its original host.

This guide first shows direct local receipt. The same source identity is used on both sides of a relay deployment.

## 2. Record exact identifiers

| Value | Meaning and source |
| --- | --- |
| `linear-work` | An operator-chosen Waking source ID. It selects `/hooks/linear-work`; it is not a Linear workspace ID. |
| Organization UUID | The exact Linear `organizationId` in webhook data. Copy the underlying organization ID, not a workspace name or URL slug. |
| Issue UUID | The exact `data.issueId` for a Comment event, or `data.id` for an Issue event. A display identifier such as `ENG-123` is not this UUID. |
| `mac-codex` | An operator-chosen Waking host ID for one explicitly configured App Server. |
| Thread UUID | The exact existing thread ID returned by that host. A conversation title, another client's family/session label, or a managed cloud task ID is not a substitute. |
| `WAKING_LINEAR_WEBHOOK_SECRET` | An environment-variable **name**. Its value is the webhook signing secret, kept outside the source configuration. |

Obtain organization and issue IDs from trusted Linear API/tool results that expose those underlying IDs, or from the corresponding webhook records in your own workspace. An existing MCP tool may help inspect an issue if it exposes the required ID; do not infer an organization UUID from a human-readable workspace name.

The native canonical issue key is:

```text
linear:https://linear.app:ORGANIZATION_UUID:ISSUE_UUID
```

For a comment discussion, `conversation_key` adds `:comment:COMMENT_ROOT_UUID`. Prefer the exact normalized `resource_key` and `conversation_key` shown by `events show` when inspecting received work. The default Linear instance origin is `https://linear.app`; using a custom public receiver domain does not change that provider origin.

## 3. Initialize one private profile and source

The uppercase identifier strings below are placeholders, not credentials. Replace them with the exact values from your workspace before running the commands. Use the same profile path in every command in this guide.

```sh
WAKING_LINEAR_DATA="$HOME/.local/share/waking-linear"
WAKING_LINEAR_ORG='REPLACE_WITH_ORGANIZATION_UUID'
WAKING_LINEAR_ISSUE='REPLACE_WITH_ISSUE_UUID'
WAKING_LINEAR_RESOURCE="linear:https://linear.app:$WAKING_LINEAR_ORG:$WAKING_LINEAR_ISSUE"

waking --data-dir "$WAKING_LINEAR_DATA" setup
waking --data-dir "$WAKING_LINEAR_DATA" source add \
  --id linear-work \
  --kind linear \
  --instance https://linear.app \
  --tenant "$WAKING_LINEAR_ORG" \
  --secret-env WAKING_LINEAR_WEBHOOK_SECRET \
  --trigger '@waking' \
  --resource "$WAKING_LINEAR_RESOURCE"
waking --data-dir "$WAKING_LINEAR_DATA" --json source list
```

This example admits one exact issue into automatic routing. Other signed events can be retained as outside-scope records without waking a model. `--resource` is repeatable; its values are canonical resource keys, or configured prefixes ending in `*`. An empty resource list allows the configured tenant's supported resources, so scope it deliberately.

If the Codex plugin will manage this named profile, set `WAKING_DATA_DIR` to the same absolute directory in its MCP process environment and verify the `data_dir` reported by `waking_setup`. The plugin otherwise uses its default profile, even when a CLI command used `--data-dir`. See [plugin profile selection](plugin.md#use-the-same-local-profile).

`source add` with an existing ID replaces that source object. To update a source that already has a review key, sending identity, special limits, or other configuration, preserve the complete object and use `source add --file /absolute/path/to/source.json`. Do not accidentally reset those fields by rerunning an incomplete flag example.

## 4. Register the Linear webhook and its secret

Using a Linear account with permission to administer webhooks, create a webhook in Linear's API settings. Linear documents workspace-admin or appropriately scoped OAuth-admin authorization for this operation. Waking does not perform the registration for you.

Set the webhook URL to your receiver's real public HTTPS address, preserving the exact source path, for example:

```text
https://events.example.com/hooks/linear-work
```

The hostname is illustrative; use a domain or tunnel URL that you control. Subscribe to **Issues** and **Issue comments** in the intended workspace/team scope. Securely copy the webhook's signing secret into the environment used by the receiving process. Do not use a Linear API token in place of that secret.

Prepare a private environment file without overwriting an existing file:

```sh
umask 077
mkdir -p "$WAKING_LINEAR_DATA"
touch "$WAKING_LINEAR_DATA/private.env"
chmod 600 "$WAKING_LINEAR_DATA/private.env"
```

In a trusted local editor, add the actual signing secret as the value of `WAKING_LINEAR_WEBHOOK_SECRET` in that file. The file format is plain `NAME=VALUE`, one entry per line: no `export`, shell expansion, or surrounding shell quotes. Keep the actual value out of commands, chat, the repository, and logs. Do not paste its contents into a diagnostic report.

The public proxy must preserve the request body bytes and the `Linear-Signature` and `Linear-Delivery` headers. Waking verifies HMAC over the original bytes and checks the signed `webhookTimestamp`; the default Linear timestamp window is 60 seconds. Keep the receiving machine's clock synchronized. A valid receipt is committed before Waking responds successfully, and model execution is not part of that HTTP acknowledgement. Proxy timeouts and body-size limits still need to permit that delivery path; see [Linear's current delivery requirements](https://linear.app/developers/webhooks).

The webhook signing secret is loaded when the process starts. Editing `private.env` or exporting a variable in a different terminal does not update an already running receiver; restart the process after changing secret values.

## 5. Select the actual existing host and thread

Waking does not start or log in to Codex. First establish an App Server endpoint owned by the intended host, using the installed Codex version's supported transport. Its socket must already be a **Unix-domain WebSocket** endpoint, reachable by the Waking process. Consult `codex app-server --help` and the [host compatibility guide](host-compatibility.md); a generic Unix stream, an HTTP URL, and a managed cloud task are different interfaces.

Codex CLI 0.166.0 advertises this Unix listen syntax when you deliberately operate the owning App Server:

```sh
codex app-server --listen unix:///absolute/path/to/owned-codex.sock
```

Run it only in the intended host environment containing the original sessions. If that host already exists, use its supported endpoint instead of starting another server or guessing private history paths. Waking's `--socket` takes the filesystem path below, not the `unix://` URL. The supported `stdio://` and TCP `ws://` Codex modes do not replace Waking's current Unix adapter.

Register the host and inspect its available threads without enabling automatic submission:

```sh
waking --data-dir "$WAKING_LINEAR_DATA" host add \
  --id mac-codex \
  --socket /absolute/path/to/owned-codex.sock
waking --data-dir "$WAKING_LINEAR_DATA" --json doctor --probe
waking --data-dir "$WAKING_LINEAR_DATA" --json host threads --host mac-codex
```

Thread listing is paginated. Use `host threads --host mac-codex --cursor NEXT_CURSOR` when the response provides another page. Select the actual thread containing the prior work and record its UUID and working directory. Visibility and a successful handshake are not proof that model continuation has been tested.

Before enabling automatic dispatch, establish both host prerequisites: this integration must be the exclusive writer for the selected threads, and the host's actual tools/credentials must prevent automatic turns from bypassing reply approval. A still-concurrently-writable Desktop conversation does not satisfy the first condition. A connected read/write Linear MCP or shell-accessible API token can violate the second. Linear's read-only MCP endpoint can be part of the chosen host setup, but the entire host tool boundary must be assessed.

Only after those conditions are true, re-register the same host with the declarations below and bind the exact issue to the existing thread:

```sh
waking --data-dir "$WAKING_LINEAR_DATA" host add \
  --id mac-codex \
  --socket /absolute/path/to/owned-codex.sock \
  --exclusive-writer \
  --restricted-tools
waking --data-dir "$WAKING_LINEAR_DATA" bind \
  --source linear-work \
  --resource "$WAKING_LINEAR_RESOURCE" \
  --host mac-codex \
  --thread EXISTING_CODEX_THREAD_UUID \
  --cwd /absolute/path/to/original-worktree \
  --expected-version 0
waking --data-dir "$WAKING_LINEAR_DATA" --json bindings
```

Use the worktree path observed by that App Server, which must also be available to the executor. `--expected-version 0` refuses to overwrite an existing binding. If a binding already exists, inspect its target/version before deliberately changing it. The two host flags assert conditions you have established; they do not configure an OS sandbox, disable MCP tools, or create an atomic lock across Codex clients.

A narrower discussion binding takes precedence over the issue binding. A known target that is busy, offline, or paused keeps its assignment; Waking does not silently choose another conversation. A configured semantic router can suggest an existing candidate for unbound work, but this release still requires a trusted binding/claim before that candidate owns the resource.

## 6. Start receipt and verify each stage

Once the public endpoint, signing secret, source, and host are ready, run the receiver in the foreground:

```sh
waking --data-dir "$WAKING_LINEAR_DATA" \
  --env-file "$WAKING_LINEAR_DATA/private.env" \
  serve --listen 127.0.0.1:7433
```

Configure your tunnel or reverse proxy to reach that listener. In a separate terminal, use the same profile path and inspect the process and durable records:

```sh
waking --data-dir "$WAKING_LINEAR_DATA" \
  --env-file "$WAKING_LINEAR_DATA/private.env" --json doctor
waking --data-dir "$WAKING_LINEAR_DATA" --json status
waking --data-dir "$WAKING_LINEAR_DATA" --json events list
waking --data-dir "$WAKING_LINEAR_DATA" --json jobs list
waking --data-dir "$WAKING_LINEAR_DATA" --json drafts list
```

Create one deliberate test comment in the selected issue from an allowed human actor, containing the plain-text token `@waking` and the requested follow-up. **`@waking` is a configured text trigger, not a Linear bot identity that Waking registers.** Do not assume selecting a similarly named mention automatically emits that literal token in the webhook body. The native source supports Issue and Comment events; it does not provision a Linear agent or claim full personal Inbox/agent-session coverage.

After an issue/discussion is bound, actionable follow-up comments can reuse the binding without repeating the token. Ordinary status updates do not become new requests merely because an older description contains `@waking`. A model turn may conclude that no reply is needed.

Inspect a specific event with `events show EVENT_ID` and its job with `jobs show JOB_ID`. Check these facts separately:

| Evidence | What it establishes |
| --- | --- |
| A successful webhook receipt and stored event | The authenticated input was accepted durably. |
| A job with the expected host/thread and dispatch/turn IDs | The selected original target was used or is waiting for the recorded reason. |
| `drafted`, `completed`, or `no_reply` | A successful model terminal outcome was recorded; a draft may exist. |
| A draft visible in `drafts list` | Local reply text is available for review. No Linear reply has been sent. |
| `sent` after the independent send workflow | The provider confirmed that specific write. |

`doctor` explicitly reports that public webhook delivery is unverified until you perform the actual delivery test. Its local secret/handshake checks do not replace this test. An `unknown` dispatch or send outcome requires reconciliation; do not convert it into a blind retry.

For a background **direct receiver**, follow the [native service lifecycle](installation.md#user-service). The installed service runs `serve`; the hybrid relay client has a different lifecycle described in the deployment guide.

## 7. Add external sending only when needed

Continue with [Independently approve one exact reply](approvals.md). Configure the full source with:

- A separate `token_env` reference for the authorized Linear API credential.
- `sending_identity` equal to the actual Linear account UUID returned by `viewer.id` for that credential.
- `review_public_key` containing the independent reviewer's Ed25519 public key.
- Appropriate `bot_actor_ids` so comments from the sending identity do not create a self-trigger loop. Filtering an actor excludes that actor's requests too; choose the account and policy deliberately.

Waking verifies the actual sender and source discussion before sending, then rechecks the current local draft/configuration. It does not implement automatic OAuth acquisition or refresh from an existing MCP connection. The signing private key stays on the independent reviewer device, outside the worker and agent access boundary.

Configure sending identity before generating drafts intended for publication. Drafts generated with no sending identity do not become publishable just because a token is added later; generate and review a fresh draft under the intended configuration. Exporting or signing an approval alone does not publish a reply.

## Troubleshooting without losing the original assignment

| Symptom | Check |
| --- | --- |
| HTTP 404 for the webhook | The URL's `/hooks/linear-work` source ID must match a configured source on that receiver. |
| Missing secret, bad signature, or stale timestamp | Check the receiver's actual environment-file path, restart after secret changes, preserve raw bytes/headers, and verify its clock. |
| `tenant_mismatch` or outside-scope event | Compare the real organization UUID and canonical issue key with the source configuration. |
| `unassigned` | Bind the exact existing host/thread; a title or a semantic suggestion is not ownership. |
| `tools_not_restricted`, ownership gate, or unavailable socket | Establish the documented host prerequisites; changing a flag alone does not establish them. |
| Waiting on a busy/offline host | Keep the binding. Restore that host and use `resume --host mac-codex` for an explicit event-driven recheck where needed. |
| Draft exists but nothing appears in Linear | This is the default behavior. Complete independent approval and the explicit send command if publication is intended. |

For implementation details see [provider normalization](../src/provider.rs), [receiver routes](../src/server.rs), [CLI arguments](../src/cli.rs), and [outbound verification](../src/outbound.rs). See [deployment choices](deployment.md) for persistent public receipt and local execution.
