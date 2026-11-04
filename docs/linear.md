# Watch your Linear work in the right existing thread

RightWake 0.3.0 can watch **new comments on issues assigned to your exact Linear user ID, plus newly assigned issues**, without requiring a mention. Signed webhooks update a durable local assignment index; only eligible events continue to routing and an existing agent thread. No event means no model call, and ownership checks do not require a model or an API polling loop. The normal result is a local reply draft.

Personal issue membership and conversation ownership are separate: a newly relevant issue still needs an existing binding, registered ownership, or a reviewed router claim. RightWake does not create a new thread or assign every issue to whichever conversation performed setup.

If Linear MCP is already connected to your agent, keep that connection for its permitted tool operations. It does **not** automatically create RightWake's webhook, public endpoint, signing-secret configuration, source scope, or thread binding. RightWake does not import or reuse the MCP connection's OAuth credentials. Linear's [MCP documentation](https://linear.app/docs/mcp) describes that tool integration, including its read-only endpoint; Linear's [webhook documentation](https://linear.app/developers/webhooks) describes the event transport used here.

Receiving events and producing drafts do not require a Linear API token. They require the webhook signing secret at the receiver and a separately working execution host. Sending a draft adds an independently configured API credential, verified sending identity, and human Ed25519 approval.

The personal-scope commands below require **RightWake 0.3.0**. Use the [installation guide](installation.md) and the matching [release](https://github.com/Nunchakus888/right-wake/releases/tag/v0.3.0), then run `right-wake --version` and `right-wake --json schema` to inspect the installed contract. Local configuration and synthetic checks do not establish a successful live Linear-to-thread integration.

## Agent-led setup

The existing **`right-wake-setup`** skill includes a Linear workflow. Invoke it from the local existing work conversation you want to continue, with an explicit request such as:

> Use $right-wake-setup to watch new comments on issues assigned to me and issues newly assigned to me. Resolve my exact Linear user and organization IDs through authorized tools, bootstrap the assignment index, and reuse the correct existing work threads. Install and configure what is needed, reuse existing settings, and report the verified result or concrete blocker. Keep replies as drafts.

For a single issue, name that issue and its intended existing thread instead. The skill uses the installed CLI and its bundled installer/reference; it does not require another runtime or a fourth skill. These are the minimum stages it coordinates:

| Stage | Automatic work when the prerequisites are available |
| --- | --- |
| Install and select state | Reuse the binary and existing profile, install v0.3.0 if needed, run idempotent setup, and align the CLI/MCP data directory. |
| Resolve Linear identity | Use permitted MCP/API results to resolve the exact organization, watched user, and issue UUIDs; preserve the intended source scope. |
| Establish delivery | Reuse an existing HTTPS route or paired relay and signing-secret reference. Inspect and register a webhook only through an available authorized management tool or separately authorized admin API client. |
| Seed observed ownership | Import timestamped issue ownership facts from authorized read tools, then maintain them through signed Issue events. |
| Verify the destination | Reuse the binding or resolve the explicitly intended existing thread through its configured host; verify the exact thread/cwd and host prerequisites. |
| Apply and start | Read before writing, skip equivalent source/host/binding settings, preserve unrelated fields, and start the appropriate already-authorized process. |
| Check the result | Distinguish configured state, real signed receipt, original-thread completion, and local draft creation. |

When the required access, endpoint, and exact target are already available, the agent can carry out these steps without another conversational handoff. It should first discover and reuse what exists, then ask only for a genuinely missing authorization, public route, or ambiguous work destination. A new onboarding conversation is not automatically the target. Reusing Linear MCP means invoking its allowed tools; never export its OAuth cache or treat that credential as the webhook signing secret.

The skill coordinates the existing native CLI commands. The native service helper starts a direct `serve` receiver; a relay client needs the separate lifecycle described in the [deployment guide](deployment.md#d-supervise-the-actual-commands-and-verify-offline-recovery). Configuration checks and local fixtures do not establish live Linear/model continuation, and this workflow has not yet completed that real integration acceptance. Do not send a test comment without explicit authorization; an actual incoming event or an authorized provider delivery test can supply receipt evidence.

## 1. Choose where the receiver runs

For an online local trial, run RightWake beside the host that owns the conversation and give its receiver a public HTTPS route through your existing tunnel or reverse proxy. The internal listener can remain `127.0.0.1:7433`; Linear must reach the public HTTPS URL, not that loopback address.

If the computer may be offline, use the [cloud relay plus local worker deployment](deployment.md#cloud-relay-plus-local-worker). The relay receives and stores webhooks while the local worker reconnects over outbound WSS. The original thread remains on its original host.

This guide first shows direct local receipt. In a relay deployment, configure **both relay and worker** with the same source ID, provider origin, exact tenant, and `linear_assignee_id`, and bootstrap the assignment index in each profile that makes admission decisions. A relay configured only for mentions can suppress assignment events before the personal worker receives them. The worker must also bootstrap issues that existed before subscription; a bare Comment event cannot supply their assignment history.

## 2. Record exact identifiers

| Value | Meaning and source |
| --- | --- |
| `linear-work` | An operator-chosen RightWake source ID. It selects `/hooks/linear-work`; it is not a Linear workspace ID. |
| Organization UUID | The exact Linear `organizationId` in webhook data. Copy the underlying organization ID, not a workspace name or URL slug. |
| Watched user UUID | The exact Linear user ID whose issue assignments you want to watch. Resolve the authenticated user's identity through an authorized tool; a name, email, `me`, comment author, or RightWake host ID is not this value. |
| Issue UUID | The exact `data.issueId` for a Comment event, or `data.id` for an Issue event. A display identifier such as `ENG-123` is not this UUID. |
| `mac-codex` | An operator-chosen RightWake host ID for one explicitly configured App Server. |
| Thread UUID | The exact existing thread ID returned by that host. A conversation title, another client's family/session label, or a managed cloud task ID is not a substitute. |
| `RIGHT_WAKE_LINEAR_WEBHOOK_SECRET` | An environment-variable **name**. Its value is the webhook signing secret, kept outside the source configuration. |

Obtain organization and issue IDs from trusted Linear API/tool results that expose those underlying IDs, or from the corresponding webhook records in your own workspace. An existing MCP tool may help inspect an issue if it exposes the required ID; do not infer an organization UUID from a human-readable workspace name.

The native canonical issue key is:

```text
linear:https://linear.app:ORGANIZATION_UUID:ISSUE_UUID
```

For a comment discussion, `conversation_key` adds `:comment:COMMENT_ROOT_UUID`. Prefer the exact normalized `resource_key` and `conversation_key` shown by `events show` when inspecting received work. The default Linear instance origin is `https://linear.app`; using a custom public receiver domain does not change that provider origin.

## 3. Initialize one private profile and source

The uppercase identifier strings below are placeholders, not credentials. Replace them with the exact values from your workspace before running the commands. Use the same profile path in every command in this guide.

```sh
RIGHT_WAKE_LINEAR_DATA="$HOME/.local/share/right-wake-linear"
RIGHT_WAKE_LINEAR_ORG='REPLACE_WITH_ORGANIZATION_UUID'
RIGHT_WAKE_LINEAR_USER='REPLACE_WITH_WATCHED_USER_UUID'
RIGHT_WAKE_LINEAR_ISSUE='REPLACE_WITH_ISSUE_UUID'
RIGHT_WAKE_LINEAR_RESOURCE="linear:https://linear.app:$RIGHT_WAKE_LINEAR_ORG:$RIGHT_WAKE_LINEAR_ISSUE"

right-wake --data-dir "$RIGHT_WAKE_LINEAR_DATA" setup
right-wake --data-dir "$RIGHT_WAKE_LINEAR_DATA" source add \
  --id linear-work \
  --kind linear \
  --instance https://linear.app \
  --tenant "$RIGHT_WAKE_LINEAR_ORG" \
  --secret-env RIGHT_WAKE_LINEAR_WEBHOOK_SECRET \
  --linear-assignee-id "$RIGHT_WAKE_LINEAR_USER"
right-wake --data-dir "$RIGHT_WAKE_LINEAR_DATA" --json source list
```

This source follows the watched user's changing assignments within one explicit organization. `--tenant` is required in personal mode. The issue variable is for the later example binding; it does not restrict the watched issue set.

Leave `allowed_resources` empty when you want **all dynamically assigned issues** in the configured webhook scope. Copying today's assigned issue IDs into `--resource` creates a fixed allowlist and excludes future assignments. When converting an existing source to this broader personal scope, deliberately clear that old list while preserving its unrelated settings. A read-only snapshot belongs in the assignment index, not in that allowlist. An intentionally narrower `--resource` filter remains an additional intersection with personal membership; actor exclusions remain effective too.

### What personal mode selects

| Incoming event | Behavior |
| --- | --- |
| Nonempty `Comment.create` on an observed owned issue | Eligible without `@right-wake`; route to the existing owner or wait for a claim. |
| `Issue.create` already assigned to the watched user | Eligible, including an issue with only a title and no description. |
| `Issue.update` with an actual assignee change to the watched user | Eligible when signed `updatedFrom.assigneeId` proves the previous value and `data.updatedAt` supplies the issue version. |
| Assignment away, explicit unassignment, or issue removal | Update membership and stop pending work that no longer qualifies. |
| Comment edits/deletes, title/label/status-only changes, or an old mention in the description | Do not create a new personal request. Issue snapshots can still update observed membership. |

`linear_assignee_id` is optional. Sources without it retain their existing text-trigger and bound-follow-up behavior: `trigger_patterns` selects explicit unbound requests, while eligible follow-ups can reuse an existing binding. Label or other metadata-only changes do not replay an old description request. Personal mode does not change the selection rules of other providers.

If the Codex plugin will manage this named profile, set `RIGHT_WAKE_DATA_DIR` to the same absolute directory in its MCP process environment and verify the `data_dir` reported by `right_wake_setup`. The plugin otherwise uses its default profile, even when a CLI command used `--data-dir`. See [plugin profile selection](plugin.md#use-the-same-local-profile).

Read `source list` before creating a source. Reuse an equivalent existing source without rewriting it. `source add` with an existing ID replaces that source object. To make an intended update to a source that already has a review key, sending identity, special limits, or other configuration, preserve the complete object and use `source add --file /absolute/path/to/source.json`. Do not accidentally reset those fields by rerunning an incomplete flag example.

## 4. Register the Linear webhook and its secret

Linear documents workspace-admin or appropriately scoped OAuth-admin authorization for webhook administration. The RightWake binary has no registration command. An agent can perform registration when its available tools expose authorized webhook management, or through a separately authorized admin API client. Inspect the actual capability and existing registrations first, and reuse a matching webhook instead of duplicating it. An existing Linear MCP connection alone does not establish that capability, and its OAuth cache must not be exported. If no authorized management path is available, create the webhook in Linear's API settings with an account that has the required permission.

Set the webhook URL to your receiver's real public HTTPS address, preserving the exact source path, for example:

```text
https://events.example.com/hooks/linear-work
```

The hostname is illustrative; use a domain or tunnel URL that you control. Subscribe to **Issues** and **Issue comments** in the intended workspace/team scope. Securely copy the webhook's signing secret into the environment used by the receiving process. Do not use a Linear API token in place of that secret.

Prepare a private environment file without overwriting an existing file:

```sh
umask 077
mkdir -p "$RIGHT_WAKE_LINEAR_DATA"
touch "$RIGHT_WAKE_LINEAR_DATA/private.env"
chmod 600 "$RIGHT_WAKE_LINEAR_DATA/private.env"
```

Use a trusted local editor or authorized secret mechanism to place the actual signing secret in that file as `RIGHT_WAKE_LINEAR_WEBHOOK_SECRET`. An agent may reuse an existing private file or securely store a secret obtained through an authorized client without printing it. The file format is plain `NAME=VALUE`, one entry per line: no `export`, shell expansion, or surrounding shell quotes. Keep the actual value out of command arguments, chat, the repository, and logs. Do not paste its contents into a diagnostic report.

The public proxy must preserve the request body bytes and the `Linear-Signature` and `Linear-Delivery` headers. RightWake verifies HMAC over the original bytes and checks the signed `webhookTimestamp`; the default Linear timestamp window is 60 seconds. Keep the receiving machine's clock synchronized. A valid receipt is committed before RightWake responds successfully, and model execution is not part of that HTTP acknowledgement. Proxy timeouts and body-size limits still need to permit that delivery path; see [Linear's current delivery requirements](https://linear.app/developers/webhooks).

The webhook signing secret is loaded when the process starts. Editing `private.env` or exporting a variable in a different terminal does not update an already running receiver; restart the process after changing secret values.

### Bootstrap and repair the observed assignment index

Subscribe to **both Issue and Comment events**, covering every intended team; a personal filter cannot see events outside that webhook scope. Start durable receipt before taking the bootstrap snapshot so subsequent Issue changes can be merged by their provider version. Use the receiver command in section 6; when changing an already active setup, pause this source while you prepare the index and target. Pausing does not stop verified receipt:

```sh
right-wake --data-dir "$RIGHT_WAKE_LINEAR_DATA" pause --source linear-work
```

Use authorized Linear MCP/API read tools to obtain each issue's actual UUID, current assignee ID or explicit unassigned value, and issue `updatedAt`. Convert that provider timestamp to Unix milliseconds. The [official webhook schema](https://linear.app/developers/webhooks) does not provide the parent issue's assignee or issue revision in the Comment's issue summary; RightWake does not invent those fields or infer assignment from the comment author. No API token is needed in RightWake to import facts obtained through existing authorized tools, and MCP OAuth credentials must stay in their original credential store.

Prepare a private JSON file from those returned values. The values below illustrate the strict format; replace the IDs and timestamps with observed facts:

```json
{
  "schema_version": 1,
  "organization_id": "ORGANIZATION_UUID",
  "issues": [
    {
      "issue_id": "ISSUE_UUID_A",
      "assignee": {"state": "assigned", "user_id": "WATCHED_USER_UUID"},
      "updated_at_ms": 1793697000000
    },
    {
      "issue_id": "ISSUE_UUID_B",
      "assignee": {"state": "unassigned"},
      "updated_at_ms": 1793697060000
    }
  ]
}
```

```sh
right-wake --data-dir "$RIGHT_WAKE_LINEAR_DATA" --json source linear-import \
  --id linear-work --file /absolute/path/to/linear-state.json
right-wake --data-dir "$RIGHT_WAKE_LINEAR_DATA" --json source linear-state \
  --id linear-work
```

The import accepts at most 10,000 issues per file within the CLI's 4 MiB JSON input limit, requires positive issue revision timestamps and the source's exact organization, and rejects unknown fields or an `unknown` owner. Use `assigned` with another user's ID when that is the observed owner. Imports are **partial merges**: omitted issues remain unchanged, and a current-state snapshot creates no assignment event or historical-comment replay. It can release already waiting eligible events when the worker runs. Paginate the authorized read result and import additional bounded files as needed.

Signed Issue events continuously update this SQLite projection. Older revisions cannot overwrite newer evidence; conflicting owners at the same version wait for resolution, and a removal tombstone wins at the same version. A newer snapshot can resolve the conflict or restore a removed issue. Known assignment transition times exclude comments or assignment events from a previous ownership interval. A late, authenticated transition may resolve a comment that arrived first; a later assignment does not replay comments already known to belong outside the current interval.

| Stored state | Meaning and next step |
| --- | --- |
| `waiting_assignment` | Ownership is missing or conflicting. No router or work model runs. Supply a relevant signed Issue update or a current authorized snapshot. |
| `outside_assignment` | The issue is observed assigned elsewhere, unassigned, removed, or the event predates the current assignment interval. Preserve the receipt; it is not runnable work. |
| `waiting_capacity` | The event is durable but cannot enter the bounded pending queue yet. Actual completion, receipt, import, or control events can recheck capacity; this state does not start an idle polling loop. |

After a known webhook gap, disabled subscription, or uncertain delivery coverage, pause the source and refresh **previously tracked issues as well as currently assigned issues**. An issue missing from today's “assigned to me” list may have been reassigned, removed, or become inaccessible; absence alone is not unassignment evidence. Resolve its actual state through authorized tools before resuming, and leave the source paused if ownership cannot be established safely. Inspect `source linear-state` and its `truncated` flag when preparing the refresh. Import explicit new owners/unassignments where available; restore webhook coverage for future updates. RightWake does not detect every remote delivery gap automatically.

“Currently assigned” means **the latest observed assignment evidence at the deciding worker**, from verified webhooks or authorized imports, not an instantaneous Linear API read. A remote change still in transit is not yet known; during relay backlog replay, an earlier comment can arrive before a later assignment change. RightWake rechecks the local projection before routing and immediately before dispatch, and only future, not already running, turns can be stopped by later revocation evidence. Repairing a known gap is an explicit event-driven operation, not periodic polling.

## 5. Select the actual existing host and thread

RightWake does not start or log in to Codex. First establish an App Server endpoint owned by the intended host, using the installed Codex version's supported transport. Its socket must already be a **Unix-domain WebSocket** endpoint, reachable by the RightWake process. Consult `codex app-server --help` and the [host compatibility guide](host-compatibility.md); a generic Unix stream, an HTTP URL, and a managed cloud task are different interfaces.

Codex CLI 0.166.0 advertises this Unix listen syntax when you deliberately operate the owning App Server:

```sh
codex app-server --listen unix:///absolute/path/to/owned-codex.sock
```

Run it only in the intended host environment containing the original sessions. If that host already exists, use its supported endpoint instead of starting another server or guessing private history paths. RightWake's `--socket` takes the filesystem path below, not the `unix://` URL. The supported `stdio://` and TCP `ws://` Codex modes do not replace RightWake's current Unix adapter.

Read `host list` first and reuse a matching host. `host add` replaces the whole entry with that ID, including its declarations and timeout; do not rerun the following new-host example against an existing configured host. For a new host, register it and inspect its available threads without enabling automatic submission:

```sh
right-wake --data-dir "$RIGHT_WAKE_LINEAR_DATA" host add \
  --id mac-codex \
  --socket /absolute/path/to/owned-codex.sock
right-wake --data-dir "$RIGHT_WAKE_LINEAR_DATA" --json doctor --probe
right-wake --data-dir "$RIGHT_WAKE_LINEAR_DATA" --json host threads --host mac-codex
```

Thread listing is paginated. Use `host threads --host mac-codex --cursor NEXT_CURSOR` when the response provides another page. Select the actual thread containing the prior work and record its UUID and working directory. Visibility and a successful handshake are not proof that model continuation has been tested.

The default `tool-output` host example below requires this integration to be the exclusive writer for the selected threads, and the host's actual tools/credentials must prevent automatic turns from bypassing reply approval. A still-concurrently-writable Desktop conversation does not satisfy that exclusive-writer condition. The opt-in `queue` mode has a separate atomic admission and context-read contract; follow the [host compatibility guide](host-compatibility.md) before selecting it. Both modes require the actual restricted-tools boundary. A connected read/write Linear MCP or shell-accessible API token can bypass that boundary; Linear's read-only MCP endpoint can be part of the chosen host setup.

Only after those conditions are true, re-register the same host with the declarations below and bind the exact issue to the existing thread:

```sh
right-wake --data-dir "$RIGHT_WAKE_LINEAR_DATA" host add \
  --id mac-codex \
  --socket /absolute/path/to/owned-codex.sock \
  --exclusive-writer \
  --restricted-tools
right-wake --data-dir "$RIGHT_WAKE_LINEAR_DATA" bind \
  --source linear-work \
  --resource "$RIGHT_WAKE_LINEAR_RESOURCE" \
  --host mac-codex \
  --thread EXISTING_CODEX_THREAD_UUID \
  --cwd /absolute/path/to/original-worktree \
  --expected-version 0
right-wake --data-dir "$RIGHT_WAKE_LINEAR_DATA" --json bindings
```

Use the worktree path observed by that App Server, which must also be available to the executor. Read `bindings` first and skip an equivalent enabled assignment. `--expected-version 0` creates only a previously absent binding; it will conflict when repeated after creation. If the intended change affects an existing binding, use its observed version and exact target. The two host flags assert conditions you have established; they do not configure an OS sandbox, disable MCP tools, or create an atomic lock across Codex clients. Preserve the existing host's other fields when adding these declarations.

A narrower discussion binding takes precedence over the issue binding. A known target that is busy, offline, or paused keeps its assignment; RightWake does not silently choose another conversation. A configured semantic router can suggest an existing candidate for unbound work, but this release still requires a trusted binding/claim before that candidate owns the resource.

## 6. Start receipt and verify each stage

For bootstrap, start the receiver after configuring its public endpoint, signing secret, and source, keeping dispatch paused until the index and intended host/thread are ready:

```sh
right-wake --data-dir "$RIGHT_WAKE_LINEAR_DATA" \
  --env-file "$RIGHT_WAKE_LINEAR_DATA/private.env" \
  serve --listen 127.0.0.1:7433
```

Configure your tunnel or reverse proxy to reach that listener. After completing the index and target setup, resume the source in a separate terminal if it was paused:

```sh
right-wake --data-dir "$RIGHT_WAKE_LINEAR_DATA" resume --source linear-work
```

Use the same profile path to inspect the process and durable records:

```sh
right-wake --data-dir "$RIGHT_WAKE_LINEAR_DATA" \
  --env-file "$RIGHT_WAKE_LINEAR_DATA/private.env" --json doctor
right-wake --data-dir "$RIGHT_WAKE_LINEAR_DATA" --json status
right-wake --data-dir "$RIGHT_WAKE_LINEAR_DATA" --json source linear-state --id linear-work
right-wake --data-dir "$RIGHT_WAKE_LINEAR_DATA" --json events list
right-wake --data-dir "$RIGHT_WAKE_LINEAR_DATA" --json jobs list
right-wake --data-dir "$RIGHT_WAKE_LINEAR_DATA" --json drafts list
```

For an explicitly authorized comment test in personal mode, use a nonempty comment from an allowed actor on an issue whose current observed assignee is the watched user; no mention is required. A real assignment change to that user is the other eligible trigger. An agent must not post a comment or reassign an issue solely because installation was requested; observe a real incoming event or use an authorized delivery test. The native source supports Issue and Comment events; it does not provision a Linear agent or claim full personal Inbox/agent-session coverage.

For a source without personal scope, use its configured text trigger for an unbound request, such as the literal `@right-wake`; this token is not a Linear bot identity that RightWake registers. Bound eligible follow-ups can omit it. Ordinary status changes do not replay an old mention. In either mode, a model turn may conclude that no reply is needed.

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

RightWake verifies the actual sender and source discussion before sending, then rechecks the current local draft/configuration. It does not implement automatic OAuth acquisition or refresh from an existing MCP connection. The signing private key stays on the independent reviewer device, outside the worker and agent access boundary.

Configure sending identity before generating drafts intended for publication. Drafts generated with no sending identity do not become publishable just because a token is added later; generate and review a fresh draft under the intended configuration. Exporting or signing an approval alone does not publish a reply.

## Troubleshooting without losing the original assignment

| Symptom | Check |
| --- | --- |
| HTTP 404 for the webhook | The URL's `/hooks/linear-work` source ID must match a configured source on that receiver. |
| Missing secret, bad signature, or stale timestamp | Check the receiver's actual environment-file path, restart after secret changes, preserve raw bytes/headers, and verify its clock. |
| `tenant_mismatch` or outside-scope event | Compare the real organization UUID and canonical issue key with the source configuration. |
| `invalid_linear_tenant` | Personal scope requires the explicit organization UUID in `--tenant` / `tenant_id`. |
| Assignment-only event stays cached | Confirm version 0.3.0, personal mode on both relay and worker, the exact watched user ID, and valid signed assignment-change/version fields. |
| `waiting_assignment` | Import current issue ownership with its real `updatedAt`, or receive a usable Issue update. A Comment payload or static list of issue IDs cannot prove membership. |
| `outside_assignment` | Inspect the current projected owner, removal/conflict flags, and assignment interval; keep the receipt rather than forcing a retry. |
| `waiting_capacity` | Inspect pending jobs and capacity. Already stored work is reconsidered on real state changes; idle waiting does not invoke a model. |
| `unassigned` | Bind the exact existing host/thread; a title or a semantic suggestion is not ownership. |
| `tools_not_restricted`, ownership gate, or unavailable socket | Establish the documented host prerequisites; changing a flag alone does not establish them. |
| Waiting on a busy/offline host | Keep the binding. Restore that host and use `resume --host mac-codex` for an explicit event-driven recheck where needed. |
| Draft exists but nothing appears in Linear | This is the default behavior. Complete independent approval and the explicit send command if publication is intended. |

For implementation details see [provider normalization](../src/provider.rs), [receiver routes](../src/server.rs), [CLI arguments](../src/cli.rs), and [outbound verification](../src/outbound.rs). See [deployment choices](deployment.md) for persistent public receipt and local execution.
