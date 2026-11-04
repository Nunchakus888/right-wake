# Deployment choices and session ownership

Place execution beside the host that owns the existing conversation. A public receiver or relay can run elsewhere, but moving RightWake to a VPS does not move a Mac's Codex thread, credentials, worktree, or session history. A thread ID is meaningful only on the host that actually serves it.

Recommended starting point: **native RightWake on the local computer, Docker + Compose for the VPS relay**. Both include SQLite in the binary, so neither needs a separate database service. The [native installation guide](installation.md) and [container guide](../deploy/README.md) cover the published v0.1.1 packages and explicit setup.

RightWake currently has two host adapters: Codex App Server over an explicitly configured Unix-domain WebSocket, and a trusted fixed executable implementing the GenericCommand JSON contract. It has no implemented or accepted adapter that attaches to an existing managed Codex Cloud task. See the [host compatibility guide](host-compatibility.md) for the exact protocol and remaining real-host validation boundaries.

## Choose the shape

| Deployment | Public receipt | Execution and original thread | Operational boundary |
| --- | --- | --- | --- |
| Local computer with an HTTPS tunnel | Local `serve`, reached through your tunnel/proxy | Existing local App Server or command adapter | The computer and public route must be online to receive deliveries. |
| Everything on a VPS | `serve` on the VPS behind TLS | App Server, session storage, and worktree on that VPS | Appropriate when the intended thread actually belongs to that host. It cannot implicitly attach to a laptop's thread. |
| Self-hosted cloud VM/container | A persistent receiver or relay deployment behind TLS | An explicitly accessible supported host, with its own persistent state | You own lifecycle, storage, identity, network policy, and session compatibility. A container image alone supplies none of the user's Codex sessions. |
| Cloud relay plus local worker | Cloud `relay serve` commits webhooks | Local `relay connect` runs the executor beside the original thread | The local machine initiates outbound WSS; admitted events can wait durably while it is offline. |

These are placement choices, not automatic migration or provisioning commands. RightWake does not create a tunnel, DNS entry, TLS certificate, Linear webhook, hosted account, Codex login, or replacement thread. For Linear's source setup and exact identifiers, use [the Linear guide](linear.md).

## Shared prerequisites

Use a private, persistent data directory for each independently running receiver/worker. A RightWake profile contains `config.json`, `state.sqlite3`, raw/normalized deliveries, bindings, jobs, drafts, and receipt state. Native installation defaults to `~/.local/share/waking` when no explicit/XDG directory is set; use `--data-dir` or `WAKING_DATA_DIR` for a deliberate profile. The Compose service mounts a named volume at `/data`, with no additional database container. Use local persistent storage, retain any SQLite WAL/SHM sidecars when making an offline copy, and do not concurrently share one profile between unrelated hosts. The daemon/worker lock prevents `serve`, `relay connect`, and manual dispatch from owning the same profile simultaneously.

Credential values belong in the launching environment or a private `--env-file`, not in source JSON, URLs, images, or the repository. The file must be private on Unix, such as mode `0600`, and uses plain `NAME=VALUE` entries without shell syntax. Configuration contains environment-variable **names**. Editing the file does not change an existing process's environment; restart that process when values change.

For public receipt, terminate valid HTTPS/WSS at a controlled reverse proxy or ingress. Keep RightWake's plain HTTP listener private to that proxy or container network. Forward only the intended `/hooks/<source_id>` and, for relay mode, `/relay/<device_id>` routes. Webhooks need their original body bytes and signature headers; relay connections need WebSocket upgrade and the device Authorization header. Never expose a Codex control socket or bridge management interface as the public webhook endpoint.

Receiving credentials and external-write authority are separate. Protect both RightWake state and its trusted configuration from the automatically executing agent, and keep the independent approval private key outside that boundary. The `--exclusive-writer` and `--restricted-tools` host flags record prerequisites; they do not create those isolation properties. See [host requirements](host-compatibility.md#tool-isolation-is-a-separate-deployment-requirement) and [reply approval](approvals.md).

## Local receiver with an HTTPS tunnel

After configuring the source and exact local host/thread as in the Linear guide:

```sh
waking --data-dir /absolute/path/to/local-waking \
  --env-file /absolute/path/to/local-waking/private.env \
  serve --listen 127.0.0.1:7433
```

Point your existing tunnel or reverse proxy's public HTTPS URL at this listener and register the public `/hooks/<source_id>` URL with the provider. Do not put `localhost` into Linear's webhook URL. This shape is useful for an online trial or an always-on local machine. If the computer or tunnel is offline, RightWake cannot receive the request; provider delivery/retry behavior is then the only upstream protection. Add a durable relay when offline local periods must be covered.

The native helper can supervise this direct receiver on a supported logged-in Linux/macOS user session:

```sh
waking --data-dir /absolute/path/to/local-waking \
  --env-file /absolute/path/to/local-waking/private.env service install
waking --data-dir /absolute/path/to/local-waking service start
waking --data-dir /absolute/path/to/local-waking service status
```

The built-in service definition always starts `serve`. It does not launch Codex or the tunnel, and it does not install `relay connect` or `relay serve`. The [installation guide](installation.md#user-service) describes ownership, stop, and uninstall behavior.

## Everything on a VPS

Install the native binary on the VPS, create its private profile, and configure the provider source there. The signing secret must be available to that VPS receiver. The selected App Server, original thread history, authenticated Codex environment, and original worktree must also be present on that VPS and available through the explicit private Unix WebSocket endpoint.

The RightWake command remains ordinary `serve`:

```sh
waking --data-dir /absolute/path/to/vps-waking \
  --env-file /absolute/path/to/vps-waking/private.env \
  serve --listen 127.0.0.1:7433
```

Run TLS termination on the VPS or through your chosen ingress. Keep the App Server socket private; do not substitute a public TCP endpoint for the implemented Unix socket adapter. Verify the actual VPS thread with `host threads --host HOST_ID` and bind that host's exact UUID. A UUID copied from a Mac conversation does not cause the VPS to retrieve or migrate it.

If the original work still belongs to the Mac, use the hybrid deployment below. A custom GenericCommand adapter is another explicit integration path only if it implements the bounded request/response contract and actually preserves the intended existing session. Pointing `--program` at an arbitrary CLI is not sufficient. See [the GenericCommand contract](architecture.md#genericcommand-contract).

## Self-hosted cloud and containers

A long-running VM, persistent Linux container deployment, or orchestrated service can host the native receiver or relay. Provide stable storage, a supervised process, controlled TLS ingress, and the exact source/device configuration. Keep the executor and its own session/worktree storage persistent as well if execution also lives there.

The [Docker + Compose path](../deploy/README.md) loads a checksummed amd64/arm64 image from a published GitHub Release. It runs as UID/GID `10001`, persists state in the Compose project's `waking-data` volume mounted at `/data`, and uses `private.env` for process credentials. The Compose service defaults to `relay serve --listen 0.0.0.0:7433`, publishes `127.0.0.1:7433`, and uses `pull_policy: never` with the explicitly loaded `waking:0.1.1` image. The installer does not start the service or create secrets.

Follow the container guide to initialize the volume, configure the source/device, and start the service. Run management commands as `docker compose run --rm waking ...` in that same Compose project. The native `--data-dir` examples below do not populate a Docker volume; do not accidentally configure a host-side profile and then start an empty container profile. Keep the same project identity during upgrades. Docker Compose's `env_file` supplies process variables; it is separate from passing RightWake's own `--env-file` argument.

### Accessing a host's Codex socket from a container

Mounting a pathname is not enough to establish the host contract. The container must reach the actual Unix-domain WebSocket in a compatible OS/kernel environment, have the necessary directory/socket permissions under its runtime UID/GID, and see any required worktree paths at the expected locations. The bundled image uses UID/GID `10001`; a host socket owned by another user with restrictive permissions is not automatically accessible. Arrange an intentional identity/group boundary instead of making the socket or state world-writable.

On Docker Desktop, a macOS host and a Linux container do not share one kernel. Do not assume bind-mounting a Mac socket makes the Linux container an App Server client for that Mac. Keep the local worker native beside the Mac's App Server, or run the supported host and worker together in a verified Linux environment. A mount does not import the Mac's session store, credentials, or original worktree.

### Verified container evidence

[Container smoke run 41619362708](https://github.com/Nunchakus888/right-wake/actions/runs/41619362708) succeeded for commit [`52dff8a4fd2522c793ef2b6bc709d561922f4790`](https://github.com/Nunchakus888/right-wake/commit/52dff8a4fd2522c793ef2b6bc709d561922f4790). It built the image, ran the explicit synthetic demo, started the real receiver, and preserved the original two completed job records and two unsent drafts across restart and container replacement with the same volume.

The [v0.1.1 release run](https://github.com/Nunchakus888/right-wake/actions/runs/41634516676) also passed actual image build/load and Compose checks on Linux amd64 and arm64. It verified SQLite 3.54.1, original jobs/drafts, and a pending synthetic event across restart and container replacement. See [verification](releases/v0.1.1-verification.md) for the exact evidence. These local container checks did not use a real Codex model, provider write, public webhook, or public relay connection.

## Cloud relay plus local worker

Use this shape when the original conversation stays on a local computer but receipt must continue while that computer is offline. The cloud relay holds verified webhook events in its own durable database. The local worker makes an outbound WSS connection, commits received events to its separate local database, and continues the specifically bound original local thread. No inbound connection to the local Codex host is required.

### Credential and configuration placement

| Item | Cloud relay | Local hybrid worker |
| --- | --- | --- |
| Source ID, provider, organization/tenant and provider instance origin | Configure the real source, for example `linear-work`, `linear`, the organization UUID, and `https://linear.app`. | Configure the same identity and scope. Keep triggers/resource rules consistent; local policy can narrow what the relay admitted. |
| Linear webhook signing secret value | Required: this side verifies the original webhook. | Not required if this profile only receives authenticated relay envelopes. It is required if this process is also used for direct webhook ingestion. |
| Source `secret_env` field | Required environment-variable reference. | Still required as a source configuration field, even when that value is absent in relay-only local operation. |
| Device token | Required for the configured device; use a secret of at least 32 characters. | The matching secret is supplied by the named environment variable to `relay connect`. |
| Host, exact thread binding, worktree access | Not needed for a receipt-only relay. | Required for local execution. |
| Provider API credential and actual sending identity | Not needed for receipt/relay. | Needed only by the separately authorized external-send process; do not automatically give them to the executing agent. |
| Reviewer Ed25519 private key | Never place it here. | Never place it in the worker or agent environment; keep it on the independent reviewer device. |

The public webhook hostname and the provider instance origin have different roles. For example, `relay.example.com` is the deployment endpoint, while the canonical Linear source origin remains `https://linear.app` on both machines.

### A. Configure the cloud relay

Replace the organization/issue placeholders with the same exact IDs used locally. The following example admits one issue and pairs one named device:

```sh
WAKING_RELAY_DATA="$HOME/.local/share/waking-relay"
WAKING_LINEAR_ORG='REPLACE_WITH_ORGANIZATION_UUID'
WAKING_LINEAR_ISSUE='REPLACE_WITH_ISSUE_UUID'
WAKING_LINEAR_RESOURCE="linear:https://linear.app:$WAKING_LINEAR_ORG:$WAKING_LINEAR_ISSUE"

waking --data-dir "$WAKING_RELAY_DATA" setup
waking --data-dir "$WAKING_RELAY_DATA" source add \
  --id linear-work --kind linear \
  --instance https://linear.app --tenant "$WAKING_LINEAR_ORG" \
  --secret-env WAKING_LINEAR_WEBHOOK_SECRET \
  --trigger '@waking' --resource "$WAKING_LINEAR_RESOURCE"
waking --data-dir "$WAKING_RELAY_DATA" relay device \
  --id mac-work --token-env WAKING_RELAY_DEVICE_TOKEN --source linear-work

umask 077
touch "$WAKING_RELAY_DATA/private.env"
chmod 600 "$WAKING_RELAY_DATA/private.env"
```

Use a trusted editor/secret mechanism to put the actual webhook signing secret and device-token values into the named entries of the relay's `private.env`. Transfer only the paired device token to the authorized local client through your trusted channel. Then run:

```sh
waking --data-dir "$WAKING_RELAY_DATA" \
  --env-file "$WAKING_RELAY_DATA/private.env" \
  relay serve --listen 127.0.0.1:7433
```

Expose the provider route as `https://relay.example.com/hooks/linear-work` and the device route as `wss://relay.example.com/relay/mac-work` through your real TLS proxy. Register the HTTPS URL with Linear and store its signing secret on this relay. `relay serve` receives and queues; it does not start model work. Ordinary `serve` does not enable the relay WebSocket endpoint.

### B. Configure the local source and original host

On the local machine, initialize a separate persistent profile and recreate the same non-secret source identity:

```sh
WAKING_LOCAL_DATA="$HOME/.local/share/waking-local"
WAKING_LINEAR_ORG='REPLACE_WITH_ORGANIZATION_UUID'
WAKING_LINEAR_ISSUE='REPLACE_WITH_ISSUE_UUID'
WAKING_LINEAR_RESOURCE="linear:https://linear.app:$WAKING_LINEAR_ORG:$WAKING_LINEAR_ISSUE"

waking --data-dir "$WAKING_LOCAL_DATA" setup
waking --data-dir "$WAKING_LOCAL_DATA" source add \
  --id linear-work --kind linear \
  --instance https://linear.app --tenant "$WAKING_LINEAR_ORG" \
  --secret-env WAKING_LINEAR_WEBHOOK_SECRET \
  --trigger '@waking' --resource "$WAKING_LINEAR_RESOURCE"

umask 077
touch "$WAKING_LOCAL_DATA/private.env"
chmod 600 "$WAKING_LOCAL_DATA/private.env"
```

Put the matching `WAKING_RELAY_DEVICE_TOKEN` value in the local file. It need not contain `WAKING_LINEAR_WEBHOOK_SECRET` for this relay-client-only path: the local client validates the authenticated normalized envelope and its local source policy without rechecking an expired webhook timestamp. `doctor` may therefore report `secret_available: false` for direct ingress; that field does not describe device-token authorization.

Configure the actual local App Server and exact binding by following [the Linear host/thread steps](linear.md#5-select-the-actual-existing-host-and-thread), using this local profile path. The host must meet both exclusive-writer and tool-isolation prerequisites. Do not create that binding only on the cloud relay and expect it to select a local session.

For management through the Codex plugin, set its MCP process's `WAKING_DATA_DIR` to this local profile, not the cloud relay profile. Check the `data_dir` returned by `waking_setup` before assigning work. [Plugin profile selection](plugin.md#use-the-same-local-profile) explains terminal and already-running desktop environments.

### C. Start the outbound local worker

If a `serve` process already owns this local profile, stop it first. For an existing service-managed direct receiver, use `waking --data-dir /absolute/path/to/waking-local service stop`; do not run both processes against the same profile.

```sh
waking --data-dir "$WAKING_LOCAL_DATA" \
  --env-file "$WAKING_LOCAL_DATA/private.env" \
  relay connect \
  --url wss://relay.example.com/relay/mac-work \
  --token-env WAKING_RELAY_DEVICE_TOKEN \
  --source linear-work
```

Use your actual relay domain. The token belongs in the environment reference, never in the URL. The command opens one authenticated outbound connection and runs the local event engine; a separate local `serve` is unnecessary. It can execute locally persisted jobs even before a later device receipt is confirmed by the network.

The client requires WSS. `--allow-loopback` permits plain `ws://` only for explicitly selected loopback tests and is not a production TLS bypass. Each paired device ID has one active connection; a newly authenticated connection replaces the previous one. Separate local workers should use separately scoped device IDs.

### D. Supervise the actual commands and verify offline recovery

The foreground `relay connect` process exits when transport fails. Use an operator-managed systemd/launchd/container supervisor to restart **that exact command and argument list**, with the same persistent data directory, environment-file path, device URL and source scope, plus an appropriate reconnect backoff. The provided Compose file supervises the cloud `relay serve` process with `restart: unless-stopped`; it does not supervise your separate local worker. The built-in native `service install` helper runs only `serve`, so a native `relay connect` still needs its own explicitly configured supervisor.

Transport reconnection and ACK deadlines do not poll Linear or periodically invoke a model. A newly connected worker reconciles stored events and completion receipts in bounded batches. Keep the relay URL/device identity stable for the same deployment, because local receipt provenance is associated with that endpoint.

To verify a real deployment, deliberately take the local worker offline, submit one authorized test event through the provider, and confirm the relay stored it. Reconnect the local worker, then inspect both profiles with `--json status` and `events list`, and inspect local `jobs list` and `drafts list`. Use exact event/job/dispatch IDs to confirm assignment and deduplication. Repeat a controlled reconnect without creating another provider event and verify it does not create a second job. These steps require your actual infrastructure; transport unit tests and the container demo do not substitute for them.

Keep these receipts distinct:

| Receipt | Durable fact |
| --- | --- |
| Provider-facing webhook success | The cloud relay committed the admitted original delivery and canonical event. |
| Device `received` | The local worker committed the event and its association with this relay. |
| Device `executed` | A successful terminal local job was recorded; no publication is implied. |
| Provider `sent` result | The separate approved outbound action was confirmed by the provider. |

Offline retention covers events the running relay actually admitted while storage was available. Queue/storage limits are bounded; a full or unavailable store rejects admission instead of acknowledging lost work. Backups do not prune the queue. Maintain capacity and retain the relay's persistent directory/volume across restarts. If the relay itself is offline, RightWake has not received the delivery and cannot promise to retain it.

A timeout after possible model or provider acceptance produces `unknown`, which requires reconciliation rather than blind resubmission. The protocol does not promise global exactly-once execution or external publication. See [relay delivery and recovery](relay-protocol.md) and [states](states.md).

## Managed Codex Cloud is a separate integration question

[Codex Cloud environment documentation](https://learn.chatgpt.com/docs/environments/cloud-environments) describes its managed task environment and lifecycle. Installing a CLI or starting a process inside such a task does not establish a supported 24/7 public RightWake receiver, durable relay operation, or the ability to resume an arbitrary existing managed cloud task from RightWake.

This release has no managed Codex Cloud adapter or acceptance result for that continuation path. It does not automatically fall back from a local thread to a cloud task, and it does not migrate a local thread merely because a receiver is cloud-hosted. A future cloud adapter needs its own public task/session addressing, authorization, event delivery, lifecycle, completion, and recovery contract. Until then, use an implemented host adapter on the host that actually owns the original thread, and complete its target-machine acceptance checks before relying on automatic continuation.
