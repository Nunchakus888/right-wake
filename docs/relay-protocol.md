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

The server keeps one event awaiting receipt per connection. The client keeps
one ACK awaiting confirmation while continuing to read new events. Independent
bounded writers prevent a large event and historical completion ACK from
blocking one another. Frames are limited to 2 MiB; network writes have a
10-second deadline and outstanding acknowledgements have a 30-second deadline.
These are transport deadlines, not model schedules. An idle connection does
not start a model turn.

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
or job. A failed transport returns an error and must be reconnected or restarted
by its service supervisor. This release does not claim a global exactly-once
execution or publication guarantee.

Run the transport contract checks with:

```sh
cargo test --test relay_contract
```

The tests use real WebSocket framing over 64-byte in-memory duplex streams.
They cover reciprocal backpressure, origin-scoped history replay, local tenant
and resource policy, receipt ordering, duplicate and rejected ACKs, config
revocation, connection cancellation, transport deadlines, and unacknowledged
event replay. They require no OS socket permission, account, external service,
or model. Public WSS deployment and an actual remote-device network path remain
separate installation acceptance checks.
