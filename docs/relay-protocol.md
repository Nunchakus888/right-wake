# Relay delivery and recovery

The relay stores verified webhook events before acknowledging their public
delivery. A source-scoped device connection then transports normalized events
to the local worker. The protocol separates three facts:

| Receipt | What it proves |
| --- | --- |
| Webhook HTTP success | The relay committed the admitted delivery and canonical event |
| Device `received` ACK | The device committed the event and its association with this relay endpoint |
| Device `executed` ACK | A successful terminal job was durably recorded locally; it does not mean a reply was published |

The local worker applies its own source, provider, tenant, instance, resource,
bot, trigger, and content policy to the authenticated relay envelope. These
checks can only narrow the relay's actionable and explicit-trigger flags.
Old webhook timestamps are not rechecked during offline delivery: authentication
and initial freshness were checked when the public relay received the webhook.

## Bounded, bidirectional transport

WSS is required except for explicitly enabled loopback test connections. Device
bearer tokens remain environment references. Local receipt associations use a
digest of the normalized relay URL, including its device path, so receipts from
another relay or direct local ingress are not replayed to this endpoint.

In 0.3.1, `relay configure` persists this endpoint and the exact source IDs in
the existing profile. `relay show` displays references without credentials.
The no-argument `relay connect` reads it after each process start. Changing it
requires stopping the current profile's worker; equivalent configuration is a
no-op. A saved connection always requires WSS. Explicit loopback WS is available
only for one-run tests, never a persisted service setting.

The server keeps one event awaiting receipt per connection. The client keeps
one ACK awaiting confirmation while continuing to read new events. Independent
bounded writers prevent a large event and historical completion ACK from
blocking one another. Frames are limited to 2 MiB; network writes have a
10-second deadline and outstanding acknowledgements have a 30-second deadline.
The worker sends an idle Ping after 20 seconds and requires the matching Pong
within 10 seconds, detecting silent network loss. These are transport deadlines,
not model schedules. An idle connection does not start a model turn.

After local persistence, the worker is notified before the network ACK is
queued. Losing the connection at that point cannot discard the durable event.
Successful completion receipts are replayed in stable event-ID order using a
bounded database query. Completion notifications arriving during that replay
request another finite reconciliation pass; an in-memory notification loss
does not discard durable completion state.

## ACK and authorization rules

- Duplicate ACKs are idempotent. A late `received` ACK cannot downgrade an
  already recorded `executed` phase.
- An `executed` ACK may recover a lost `received` ACK for the currently delivered
  event, or update an event that already has a durable receipt for this device.
  It cannot skip an unseen event in the queue.
- An unknown or not-yet-delivered completion receives `ack_rejected`; it does
  not close the connection or delete a pending event.
- Device authorization is reloaded before ACK processing. Revoked source
  scope, disabled devices, and changed token references stop the connection.
- A newly authenticated connection replaces the previous connection for the
  same device. Cancellation also stops its writer if the old peer is not reading.

Changing an environment variable in a shell does not modify an already running
service's environment. Token rotation must update the service's environment
and restart it, or change its configured token reference to one the running
service already has. Device-disable configuration changes are watched by the
resident service and require no restart.

## Disconnection and evidence

Unacknowledged events remain in relay storage. A new connection resumes them;
local delivery deduplication prevents a redelivery from creating a second event
or job. Version 0.3.1 keeps one worker and profile lock while retrying temporary
network failures. The backoff ceilings are 1, 2, 4, 8, 16 and 30 seconds, with
75–100% jitter in each interval; 60 seconds of stable connection resets the
backoff. Startup while offline follows the same path. The local engine is not
cancelled or recovered on each network failure, and existing unknown outcomes
are never blindly resubmitted.

Invalid device credentials, TLS verification, wrong endpoint, malformed
protocol, configuration or storage stop the command with a bounded diagnostic.
An OS supervisor may restart the process, but cannot repair those prerequisites.
SIGINT/SIGTERM cancels the worker before releasing its lock; ambiguous model
acceptance remains an unknown outcome on process restart. The built-in
`service install --mode relay-connect` manages this command; the existing
Compose deployment manages cloud `relay serve`. See [service lifecycle](installation.md#user-service).

The v1 protocol has no replay-complete watermark. Connection/ACK success does
not prove that every queued ownership change has reached the worker. Personal
filtering uses the latest observed ownership, not a transactional current-state
query to Linear; an older comment may arrive before a buffered reassignment.
Keep the initial setup paused through receipt and membership reconciliation.
After known provider gaps, or when an offline backlog needs current-owner
assurance, refresh both current assignments and every previously tracked issue
with authorized read tools before resuming. No timeout can substitute for this
evidence. This patch does not change that protocol or promise global exactly-once
execution/publication.

Run the transport contract checks with:

```sh
cargo test --test relay_contract
cargo test --lib relay::tests
cargo test --test relay_config_contract
```

The tests use real WebSocket framing over 64-byte in-memory duplex streams.
They cover reciprocal backpressure, origin-scoped history replay, local tenant
and resource policy, receipt ordering, duplicate and rejected ACKs, config
revocation, connection cancellation, transport deadlines, and unacknowledged
event replay. Added tests cover timed reconnect and idle loss, authorization
failure, stable saved configuration, reopened SQLite stores and lost-ACK
redelivery without another job. They use synthetic events and temporary files;
they do not prove public DNS/TLS, OS startup, a real provider event, or a Desktop
turn. Public WSS deployment and an actual remote-device network path remain
separate installation acceptance checks.
