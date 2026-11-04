# Linear onboarding

Use this reference with `right-wake-setup` in a local Agent that can run the native CLI. The RightWake MCP management tools alone do not install services, add sources/hosts, or register provider webhooks. Reuse the user's existing Linear connection within its actual capabilities.

Follow [discovery](#discover-before-asking), [personal scope](#watch-comments-and-new-assignments), [deployment](#choose-the-existing-deployment), [configuration](#preserve-local-configuration), [registration](#register-or-reuse-the-provider-webhook), [administrator handoff](#prepare-a-concrete-administrator-handoff), and [verification](#start-and-verify-without-posting-a-message).

## Discover before asking

| Needed fact | Discover or reuse | Block only when unresolved |
| --- | --- | --- |
| Work scope | Supplied issue URL/identifier or explicit assigned-to-me policy, exact existing binding, trusted prior instruction | Multiple accounts/scopes with no authorized choice |
| Linear IDs | Authorized tools/API returning organization, authenticated user, issue and team UUIDs | Missing underlying ID; never substitute a slug, display number, actor, or team ID for organization/user ID |
| Event endpoint | Existing trusted HTTPS proxy/tunnel or user-controlled relay configuration | No reachable HTTPS route or authority to provision one |
| Webhook management | Inspect actual tool definitions and granted permissions; otherwise an explicitly configured, legitimate admin API client | Tool/API capability or admin permission unavailable |
| Signing secret | Existing secret-store/environment reference or privately stored webhook secret | No authorized path to retrieve/store it safely |
| Original host/thread | Existing configuration/binding, trusted caller metadata, supported host thread listing | Unknown socket, inaccessible host, or ambiguous original thread |

Do not assume an MCP tool exists because its name sounds plausible. Linear documents issue/project/comment tools, but not a complete public tool catalog. Discover actual tools and inspect their returned fields. An existing MCP login does not prove webhook-management capability or permission. Never read/export its OAuth cache to manufacture a second API credential.

If an authorized GraphQL client is already available, this read query illustrates identity discovery; use variables and the actual supported schema:

```graphql
query RightWakeIdentity($issue: String!) {
  organization { id name }
  issue(id: $issue) {
    id
    identifier
    team { id name }
  }
}
```

Use the true organization UUID as RightWake's tenant. The canonical issue key is `linear:https://linear.app:ORGANIZATION_UUID:ISSUE_UUID`; the provider origin stays `https://linear.app` even when the receiver uses another domain. Limit a new source to the requested issue or explicitly authorized broader scope. An empty `allowed_resources` list allows the tenant's supported resources; it is not a disabled source.

## Watch comments and new assignments

For “new comments on my issues and newly assigned tasks,” require native **0.3.0 or newer** and resolve the authenticated user's exact UUID. Set `linear_assignee_id` on the existing source while preserving its tenant and other fields. New sources can use `source add --linear-assignee-id USER_UUID --tenant ORGANIZATION_UUID` together with the normal source arguments. The user ID is the assignee, not the commenter or bot identity.

The resulting policy selects new nonempty comments on issues whose latest observed owner is that user, and Issue creation/actual reassignment to that user. It needs no `@right-wake` text. Comment edits, unchanged assignment, and an old mention in an unrelated update do not trigger it. Explicit static resource constraints and bot exclusions still apply. A list of today's assigned issue IDs must not become the permanent `allowed_resources` for an all-my-work request: it would exclude tomorrow's assignments. Change a pre-existing static constraint only when the broader scope is already authorized, preserving intentional exclusions.

Linear Comment webhooks do not contain issue assignee data. RightWake maintains a source-scoped assignment projection from signed Issue snapshots. Unknown/conflicting ownership is durable `waiting_assignment`, without a router/model call. Older snapshots do not replace newer ones. The policy means **latest observed ownership**; a missed webhook stream requires a refreshed read-only snapshot. Never claim this is a continuously authoritative API query.

Reuse an existing `linear-onboarding.json` as evidence, inspect its actual shape, and preserve it. A saved count such as 117 proves neither current membership nor active subscription. Read `source linear-state --id SOURCE`; use the existing authorized MCP/API to fill missing exact issue IDs, assignee IDs (including explicit unassigned), and issue `updatedAt`, following pagination. Do not invent timestamps, extract credentials, or add recurring API/model polling.

Convert verified issue facts into the installed `schema`'s `linear_personal_scope.import_schema`, for example:

```json
{
  "schema_version": 1,
  "organization_id": "ORGANIZATION_UUID",
  "issues": [
    {
      "issue_id": "ISSUE_UUID",
      "assignee": { "state": "assigned", "user_id": "USER_UUID" },
      "updated_at_ms": 1793750400000
    }
  ]
}
```

Use actual provider timestamps; the number above is illustrative. Explicitly unassigned is `{ "state": "unassigned" }`; missing assignment data is not that state. Save the converted snapshot privately, then run `source linear-import --id SOURCE --file PATH` and inspect `source linear-state` again. Imports merge idempotently and are always partial: an omitted issue is never implicitly unassigned. A complete known-gap recovery must also refresh previously tracked issues that no longer appear in “assigned to me,” otherwise stale positive membership can remain. Capture completeness and inaccessible issues honestly.

The webhook must cover **Issue and Comment changes across the authorized teams where future assignments can arrive**. Linear's webhook subscription has no assignee filter. The personal-watch request authorizes local assignee filtering and removal of an obsolete baseline allowlist; it does not automatically authorize collecting every unrelated team's events. Reuse previously authorized team coverage, or have the administrator determine the smallest sufficient coverage in the handoff. Do not ask once per issue. Use `allPublicTeams` only when that broader receipt scope is already authorized; private teams require their own allowed scope. Report undiscovered or inaccessible team coverage as incomplete. For relay deployment, configure the same personal policy at receiver and worker, and import needed bootstrap facts into both profiles. A worker must not upgrade an event the relay's static policy denied.

Personal scope selects relevant events; it does not assign all tasks to the onboarding conversation. Preserve existing resource/thread bindings and route new resources using the configured context index or an explicit claim of the intended existing thread. Unknown ownership never creates a replacement work thread.

### Activate without a baseline-to-webhook gap

An early baseline import is preparation only. Keep automatic processing of this source paused during first activation or a known-gap recovery; `pause --source SOURCE` preserves signed receipt while holding execution. Record its prior pause state and do not resume a user-paused source merely to finish setup. Do not pause unrelated sources.

First establish the receiver/relay's durable receipt and the authorized Issue/Comment webhook coverage. With receipt continuing and execution held, refresh current assigned issues **and previously tracked issues**, merge their actual revisions, and record inaccessible or incomplete coverage. For a relay, the recovery set is the union of the preserved baseline, relay inventory, worker inventory, and current assigned issues; import verified recovery facts into both projections and read back their issue revisions after the authenticated connection is established. A truncated inventory or missing facts that could retain stale positive ownership is `assignment_recovery_incomplete`: keep this source paused. This release has no catch-up-complete barrier, so neither an empty local queue nor a single completed read proves recovery. Resume only the setup-owned pause once this recovery set is covered and the host prerequisites are established. If the endpoint or registration is still missing, retain the baseline and record `refresh_required_before_activation`; do not claim that this initial import closes the delivery gap. This is a bounded activation/recovery read, never recurring model/API polling.

## Choose the existing deployment

**Direct local receiver:** reuse a public HTTPS route to the local receiver, normally `127.0.0.1:7433`. The public webhook path must be `/hooks/SOURCE_ID`. Check that the proxy preserves raw request bytes and `Linear-Signature` / `Linear-Delivery` headers. Starting RightWake alone does not create a public URL.

**Cloud relay plus local worker:** keep two distinct profiles. Register the provider webhook and signing secret at the cloud receiver; configure the local source with the same provider/organization identity and matching or narrower resource scope. The local worker uses its own host, exact thread binding, and authorized device token for outbound WSS. A relay-only local worker need not hold the provider signing-secret value, although its source still declares the `secret_env` field. Reuse the selected relay/device; do not create an unrelated hosted account or move local session history into the cloud.

If deployment infrastructure is missing, complete independent installation/discovery and report `public_endpoint_required` or `relay_access_required`. Provision an already-authorized tunnel/proxy/VPS when the available tools and scope support it. Do not pick a paid provider or an unowned URL automatically. For full operator details, use the [deployment guide](https://github.com/Nunchakus888/right-wake/blob/main/docs/deployment.md).

## Preserve local configuration

Read `source list`, `host list`, `bindings`, and status with `--json` in the chosen profile. Inspect the installed schema before composing any writes.

1. Reuse a Linear source with the correct organization, instance, admitted resource, and intended webhook endpoint. Compare its complete object. Preserve tokens, review keys, actor rules, trigger patterns, limits, and unrelated resources. An existing compatible source needs no `source add`.
2. For a new source, choose an unused stable ID and an explicit signing-secret environment-variable name. Admit only the authorized resource scope. Do not provision external sending credentials or approval keys as part of receipt-only setup.
3. When changing an existing source is necessary, retain the full object, modify only requested fields, and pass a private JSON file to `source add --file`. Re-read state if another writer changed it. Do not use `sed`/`awk` to patch JSON or rebuild an existing source from minimal flags.
4. Reuse a supported host/socket in the owning environment. For a new host, register it for inspection first, then run `doctor --probe` and paginate `host threads`. Inspect `codex app-server --help` if an explicitly authorized owning App Server must be started; use that installed version's supported Unix-domain WebSocket. Do not guess a Desktop socket or launch a substitute host with no access to the original thread.
5. Verify the exact existing work thread and its worktree through that host. A UUID-shaped string and a successful handshake are insufficient. For an explicit request to attach the current original work conversation, trusted `right_wake_identity` metadata may supply the ID; null identity or an onboarding-only conversation requires another source of exact identity.
6. Inspect the exact host's delivery capabilities using `host inspect --host HOST --thread UUID`. Direct `tool-output` requires exclusive writer control. Supported experimental `--delivery queue` uses atomic host admission and does not require that declaration; the exact work thread must have the current `right_wake_dispatch_context` MCP tool using the worker's profile. Verify that the endpoint is the original thread's owning shared App Server. A separate server reading the same `CODEX_HOME` and UUID does not share Desktop scheduling; `host inspect` alone cannot establish that ownership. Queue notices contain only a dispatch reference; source content is fetched as a tool result. Queue RPCs are version-dependent, and queue pause state is not exposed by the checked protocol. Never force-resume an intentional pause or use another host/transport as a silent fallback.
7. Establish restricted mutation tools/credentials for either mode before setting `--restricted-tools`. A writable Linear MCP or accessible API token may bypass draft approval; queue delivery does not remove it. Inspect actual capabilities, preserve existing configuration, and report the precise remaining boundary. Do not invent a new history/thread to make a prerequisite appear satisfied.
8. Reuse an enabled exact binding. For an absent binding, use `--expected-version 0`; for an explicitly authorized rebind, use the current version and preserve the requested scope. Read it back. Preserve a target that is busy/offline; do not route it elsewhere.

Accept owning-server evidence from a supported host connection/configuration that explicitly identifies the endpoint used by the original client, or a controlled operator-managed App Server to which that original client is already connected. A successful thread listing, process name, copied UUID, or shared history directory is insufficient. If Desktop exposes no such supported attachment capability, report `desktop_attachment_unsupported` and continue receipt-only setup; do not turn this into an open-ended socket search.

The checked Queue protocol has no per-dispatch tool/credential override. If the original thread must retain writable Linear tools and its host offers no actual restriction for automatic turns, report `automatic_execution_tools_unrestricted`. Continue receiving while execution remains paused. Changing existing tool availability requires a concrete authorized host configuration; do not delete the user's MCP connection, treat a prompt as a permission boundary, or create another work thread to bypass the condition.

Keep CLI and MCP pointed at the same local worker profile. Source/host creation, binding, and provider webhook registration are separate operations; retain completed work when a later operation is blocked.

## Register or reuse the provider webhook

Linear requires a workspace admin or OAuth application with `admin` scope to create/read webhooks. First use an already-authorized management tool if actually exposed. Otherwise use an existing legal admin API client and its named credential reference; do not assume the issue-reading MCP credential can be extracted or repurposed.

For a management-capable API path:

1. Resolve the current organization, requested team scope, and exact HTTPS URL. List existing webhooks with pagination before creating one. A matching label/URL alone does not prove ownership: use a saved registration ID, explicit operator designation, or other trusted provenance and verify organization, team, resource types, and enabled state.
2. Reuse an appropriate owned webhook and its original secret. Do not rotate its secret, widen its team scope, or duplicate it on each invocation. If the secret is missing locally, retrieve it through an authorized private channel when supported; report only its presence/reference. A missing local secret does not authorize replacing a shared webhook.
3. For a new webhook, use `webhookCreate` with the intended `teamId` (or deliberately authorized `allPublicTeams`), exact `url`, and `resourceTypes: ["Issue", "Comment"]`. Prefer `enabled: false` while initializing the receiver. Record its returned ID and exact scope in the private setup result.
4. The API can accept a caller-generated `secret` or return a generated `webhook.secret`. Use a cryptographically secure secret and an authorized client that handles it privately. Write it directly to the receiver's secret store/private environment file without emitting it in chat, arguments, logs, or the setup report. Do not make manual copying the only supported path, and do not claim secure storage if the available client cannot provide it.
5. Preserve existing environment entries and ownership, reject unsafe symlink destinations, and use mode `0600`. Source JSON contains only the environment-variable name. Do not confuse the webhook signing secret with an API access token. For hybrid deployment, keep admin API credentials and the webhook signing secret off the local automatic worker unless separately required and authorized.
6. Start or update the appropriate receiver after its source and secret are ready, with execution of this source held as described in the activation section. Verify the controlled public route, then enable only the newly created/owned webhook. Complete the post-activation ownership refresh before releasing that execution hold. An existing active webhook is not disabled merely to rerun setup. Native services load secret values at process start; changing a file requires a necessary authorized restart.
7. Read back registration metadata without secret values. If a create/update times out, reconcile by the saved ID and exact metadata before any retry. Do not issue a second create blindly. Keep a registration journal without secrets so an interrupted invocation can resume.

At the schema checked on 2026-11-04, `WebhookUpdateInput` supports URL, resource types, label, enabled state and secret, but **not team scope**. Do not silently replace a webhook or treat a team change as an ordinary update. Keep unmatched registrations untouched and report a precise conflict.

The native RightWake binary does not implement this provider registration itself; the skill must actually use an available authorized management client. If none exists, return `webhook_management_unavailable` or `webhook_admin_required`, naming the already-finished local steps and the one missing capability. Re-authenticating an ordinary MCP connection is not guaranteed to grant it.

## Prepare a concrete administrator handoff

When registration is blocked, finish all independent discovery/configuration first. Save a private, secret-free handoff alongside the existing onboarding record containing:

- Resolved organization and authenticated user UUID; the desired new-comment/new-assignment policy and authorized team scope, including private-team gaps.
- Exact owned public HTTPS `/hooks/SOURCE` URL, or explicit `public_endpoint_required` with no invented address.
- `resourceTypes: ["Issue", "Comment"]`, the chosen authorized `teamId`/`allPublicTeams`, and disabled-first creation for a new registration. If team coverage is unresolved, record `team_scope_unresolved`, distinguish already-authorized coverage from merely observed candidate teams, and ask the administrator to determine the smallest sufficient coverage. Do not promote candidates to authorized scope or default to `allPublicTeams`.
- Existing owned registration ID and reconciliation result, or a statement that the current credential could not list registrations; do not falsely claim none exist.
- The receiver's secret-store/environment-variable **reference**, profile, and process mode. Include no secret value or OAuth cache.
- Administrator actions: verify/reuse matching registration; create if absent; transfer the signing secret through the authorized private channel; enable after receiver setup; return registration ID, scope and enabled state; inspect the first real delivery.
- Separate host/tool/route blockers, with completed steps preserved. Registration alone does not enable original-thread execution.

Report this as an actionable handoff, never as a configured webhook. No message is sent to an administrator without the user's explicit instruction to send it.

## Start and verify without posting a message

For a direct receiver, an install/configure/start request authorizes the ordinary owned user-service lifecycle with the selected profile and private environment file. Confirm service status. The built-in service starts `serve`; it does not manage Codex, the public tunnel, `relay serve`, or `relay connect`. Reuse or explicitly configure the correct supervisor for a hybrid worker and keep only one worker per local profile.

Keep the report staged:

| Stage | Required evidence |
| --- | --- |
| Local configuration | Real binary/version/profile; read-back source, host and exact binding |
| Desired personal scope / baseline | Exact user/organization and requested policy; imported issue facts, timestamps and pagination coverage. These do not prove active listening. |
| Host readiness | Supported delivery mode, original thread exists, actual tool boundary; direct ownership or queued-context MCP prerequisite |
| Registration | Provider returned/read-back webhook ID, exact scope/URL, enabled state; secret stored privately |
| Receiver/worker running | Correct command/profile/environment and actual process/service status |
| Real receipt | Authentic in-scope Linear delivery committed by RightWake, with its delivery/event ID |
| Same-thread completion | That event's job used the intended host/thread and reached a successful terminal state, with dispatch/turn evidence |

`doctor` deliberately does not prove public delivery or model execution. A local synthetic test remains synthetic; use a disposable profile and avoid real host dispatch. Webhook registration alone is not delivery evidence. Do not claim a public webhook test/replay API unless the current exposed contract actually provides one; the checked public schema supplies no such mutation.

Without explicit permission to create a test issue/comment, inspect existing in-scope deliveries. Return `configured_waiting_event` only when receiver configuration, the recovery gate, and every execution prerequisite are established, with only a real event/turn still unverified. Receipt-only operation, unresolved recovery, or unsupported/unrestricted execution remains `partial` / `blocked`, with its concrete reason. Do not send a test comment, invoke provider write tools, or schedule model polling to obtain acceptance evidence. Once a real event arrives and the execution prerequisites are met, native event-driven processing can continue independently of this setup conversation. Keep any `unknown` execution outcome visible for reconciliation.

An authorized receiver can collect durable events while execution remains blocked; report receiving and execution separately. Return exact completed stages and blockers. One targeted account/endpoint/ownership question is appropriate only when discovery cannot resolve it. Do not ask for secret values in chat or treat missing external-send credentials as a blocker for receipt and drafts.

## Sources and operating details

- [Linear MCP capabilities and authentication](https://linear.app/docs/mcp)
- [Linear webhook permissions, endpoint and API requirements](https://linear.app/developers/webhooks)
- [Official webhook creation example, including a returned secret](https://github.com/linear/linear-solutions/blob/main/integration_guides/scenarios/03-webhooks/README.md)
- [Checked official GraphQL and webhook schema](https://github.com/linear/linear/blob/76164675dfd29f26a09ea124d7b6b9e04b542693/packages/sdk/src/schema.graphql)
- [RightWake's detailed Linear guide](https://github.com/Nunchakus888/right-wake/blob/main/docs/linear.md)
