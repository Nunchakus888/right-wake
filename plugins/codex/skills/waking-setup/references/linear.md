# Linear onboarding

Use this reference with `waking-setup` in a local Agent that can run the native CLI. The Waking MCP management tools alone do not install services, add sources/hosts, or register provider webhooks. Reuse the user's existing Linear connection within its actual capabilities.

## Discover before asking

| Needed fact | Discover or reuse | Block only when unresolved |
| --- | --- | --- |
| Work scope | Supplied issue URL/identifier, exact existing binding, trusted prior instruction | Multiple accounts/issues with no authorized choice |
| Linear IDs | Authorized tools/API returning organization, issue and team UUIDs | Missing underlying ID; never substitute a slug, display number, or team ID for organization ID |
| Event endpoint | Existing trusted HTTPS proxy/tunnel or user-controlled relay configuration | No reachable HTTPS route or authority to provision one |
| Webhook management | Inspect actual tool definitions and granted permissions; otherwise an explicitly configured, legitimate admin API client | Tool/API capability or admin permission unavailable |
| Signing secret | Existing secret-store/environment reference or privately stored webhook secret | No authorized path to retrieve/store it safely |
| Original host/thread | Existing configuration/binding, trusted caller metadata, supported host thread listing | Unknown socket, inaccessible host, or ambiguous original thread |

Do not assume an MCP tool exists because its name sounds plausible. Linear documents issue/project/comment tools, but not a complete public tool catalog. Discover actual tools and inspect their returned fields. An existing MCP login does not prove webhook-management capability or permission. Never read/export its OAuth cache to manufacture a second API credential.

If an authorized GraphQL client is already available, this read query illustrates identity discovery; use variables and the actual supported schema:

```graphql
query WakingIdentity($issue: String!) {
  organization { id name }
  issue(id: $issue) {
    id
    identifier
    team { id name }
  }
}
```

Use the true organization UUID as Waking's tenant. The canonical issue key is `linear:https://linear.app:ORGANIZATION_UUID:ISSUE_UUID`; the provider origin stays `https://linear.app` even when the receiver uses another domain. Limit a new source to the requested issue or explicitly authorized broader scope. An empty `allowed_resources` list allows the tenant's supported resources; it is not a disabled source.

## Choose the existing deployment

**Direct local receiver:** reuse a public HTTPS route to the local receiver, normally `127.0.0.1:7433`. The public webhook path must be `/hooks/SOURCE_ID`. Check that the proxy preserves raw request bytes and `Linear-Signature` / `Linear-Delivery` headers. Starting Waking alone does not create a public URL.

**Cloud relay plus local worker:** keep two distinct profiles. Register the provider webhook and signing secret at the cloud receiver; configure the local source with the same provider/organization identity and matching or narrower resource scope. The local worker uses its own host, exact thread binding, and authorized device token for outbound WSS. A relay-only local worker need not hold the provider signing-secret value, although its source still declares the `secret_env` field. Reuse the selected relay/device; do not create an unrelated hosted account or move local session history into the cloud.

If deployment infrastructure is missing, complete independent installation/discovery and report `public_endpoint_required` or `relay_access_required`. Provision an already-authorized tunnel/proxy/VPS when the available tools and scope support it. Do not pick a paid provider or an unowned URL automatically. For full operator details, use the [published deployment guide](https://github.com/Nunchakus888/waking/blob/v0.1.1/docs/deployment.md).

## Preserve local configuration

Read `source list`, `host list`, `bindings`, and status with `--json` in the chosen profile. Inspect the installed schema before composing any writes.

1. Reuse a Linear source with the correct organization, instance, admitted resource, and intended webhook endpoint. Compare its complete object. Preserve tokens, review keys, actor rules, trigger patterns, limits, and unrelated resources. An existing compatible source needs no `source add`.
2. For a new source, choose an unused stable ID and an explicit signing-secret environment-variable name. Admit only the authorized resource scope. Do not provision external sending credentials or approval keys as part of receipt-only setup.
3. When changing an existing source is necessary, retain the full object, modify only requested fields, and pass a private JSON file to `source add --file`. Re-read state if another writer changed it. Do not use `sed`/`awk` to patch JSON or rebuild an existing source from minimal flags.
4. Reuse a supported host/socket in the owning environment. For a new host, register it for inspection first, then run `doctor --probe` and paginate `host threads`. Inspect `codex app-server --help` if an explicitly authorized owning App Server must be started; use that installed version's supported Unix-domain WebSocket. Do not guess a Desktop socket or launch a substitute host with no access to the original thread.
5. Verify the exact existing work thread and its worktree through that host. A UUID-shaped string and a successful handshake are insufficient. For an explicit request to attach the current original work conversation, trusted `waking_identity` metadata may supply the ID; null identity or an onboarding-only conversation requires another source of exact identity.
6. Establish actual exclusive writer ownership and restricted mutation tools before setting their host declarations. A writable Linear MCP connected to an automatic execution host may bypass Waking's draft approval. Inspect the actual deployment; preserve the user's MCP configuration and report `host_prerequisites_missing` when conditions are not established. Do not invent a new history/thread to resolve this.
7. Reuse an enabled exact binding. For an absent binding, use `--expected-version 0`; for an explicitly authorized rebind, use the current version and preserve the requested scope. Read it back. Preserve a target that is busy/offline; do not route it elsewhere.

Keep CLI and MCP pointed at the same local worker profile. Source/host creation, binding, and provider webhook registration are separate operations; retain completed work when a later operation is blocked.

## Register or reuse the provider webhook

Linear requires a workspace admin or OAuth application with `admin` scope to create/read webhooks. First use an already-authorized management tool if actually exposed. Otherwise use an existing legal admin API client and its named credential reference; do not assume the issue-reading MCP credential can be extracted or repurposed.

For a management-capable API path:

1. Resolve the current organization, requested team scope, and exact HTTPS URL. List existing webhooks with pagination before creating one. A matching label/URL alone does not prove ownership: use a saved registration ID, explicit operator designation, or other trusted provenance and verify organization, team, resource types, and enabled state.
2. Reuse an appropriate owned webhook and its original secret. Do not rotate its secret, widen its team scope, or duplicate it on each invocation. If the secret is missing locally, retrieve it through an authorized private channel when supported; report only its presence/reference. A missing local secret does not authorize replacing a shared webhook.
3. For a new webhook, use `webhookCreate` with the intended `teamId` (or deliberately authorized `allPublicTeams`), exact `url`, and `resourceTypes: ["Issue", "Comment"]`. Prefer `enabled: false` while initializing the receiver. Record its returned ID and exact scope in the private setup result.
4. The API can accept a caller-generated `secret` or return a generated `webhook.secret`. Use a cryptographically secure secret and an authorized client that handles it privately. Write it directly to the receiver's secret store/private environment file without emitting it in chat, arguments, logs, or the setup report. Do not make manual copying the only supported path, and do not claim secure storage if the available client cannot provide it.
5. Preserve existing environment entries and ownership, reject unsafe symlink destinations, and use mode `0600`. Source JSON contains only the environment-variable name. Do not confuse the webhook signing secret with an API access token. For hybrid deployment, keep admin API credentials and the webhook signing secret off the local automatic worker unless separately required and authorized.
6. Start or update the appropriate receiver after its source and secret are ready, verify the controlled public route, then enable only the newly created/owned webhook. An existing active webhook is not disabled merely to rerun setup. Native services load secret values at process start; changing a file requires a necessary authorized restart.
7. Read back registration metadata without secret values. If a create/update times out, reconcile by the saved ID and exact metadata before any retry. Do not issue a second create blindly. Keep a registration journal without secrets so an interrupted invocation can resume.

At the schema checked on 2026-11-03, `WebhookUpdateInput` supports URL, resource types, label, enabled state and secret, but **not team scope**. Do not silently replace a webhook or treat a team change as an ordinary update. Keep unmatched registrations untouched and report a precise conflict.

The native Waking binary does not implement this provider registration itself; the skill must actually use an available authorized management client. If none exists, return `webhook_management_unavailable` or `webhook_admin_required`, naming the already-finished local steps and the one missing capability. Re-authenticating an ordinary MCP connection is not guaranteed to grant it.

## Start and verify without posting a message

For a direct receiver, an install/configure/start request authorizes the ordinary owned user-service lifecycle with the selected profile and private environment file. Confirm service status. The built-in service starts `serve`; it does not manage Codex, the public tunnel, `relay serve`, or `relay connect`. Reuse or explicitly configure the correct supervisor for a hybrid worker and keep only one worker per local profile.

Keep the report staged:

| Stage | Required evidence |
| --- | --- |
| Local configuration | Real binary/version/profile; read-back source, host and exact binding |
| Host readiness | Supported transport, original thread exists, actual ownership/tool prerequisites |
| Registration | Provider returned/read-back webhook ID, exact scope/URL, enabled state; secret stored privately |
| Receiver/worker running | Correct command/profile/environment and actual process/service status |
| Real receipt | Authentic in-scope Linear delivery committed by Waking, with its delivery/event ID |
| Same-thread completion | That event's job used the intended host/thread and reached a successful terminal state, with dispatch/turn evidence |

`doctor` deliberately does not prove public delivery or model execution. A local synthetic test remains synthetic; use a disposable profile and avoid real host dispatch. Webhook registration alone is not delivery evidence. Do not claim a public webhook test/replay API unless the current exposed contract actually provides one; the checked public schema supplies no such mutation.

Without explicit permission to create a test issue/comment, inspect existing in-scope deliveries or finish as `configured_waiting_event`. Do not send a test comment, invoke provider write tools, or schedule model polling to obtain acceptance evidence. Once a real event arrives, native event-driven processing can continue independently of this setup conversation. Keep any `unknown` execution outcome visible for reconciliation.

Return exact completed stages and blockers, not a generic "all connected". One targeted account/endpoint/ownership question is appropriate only when discovery cannot resolve it. Do not ask for secret values in chat or treat missing external-send credentials as a blocker for receipt and drafts.

## Sources and operating details

- [Linear MCP capabilities and authentication](https://linear.app/docs/mcp)
- [Linear webhook permissions, endpoint and API requirements](https://linear.app/developers/webhooks)
- [Official webhook creation example, including a returned secret](https://github.com/linear/linear-solutions/blob/main/integration_guides/scenarios/03-webhooks/README.md)
- [Checked official GraphQL schema](https://github.com/linear/linear/blob/3ce13b6f334da16399ffc6c53c73a73366f728d6/packages/sdk/src/schema.graphql)
- [Waking's detailed Linear guide](https://github.com/Nunchakus888/waking/blob/main/docs/linear.md)
