# Codex host compatibility and verification

## Implemented contract in 0.3.0

RightWake connects to an **explicit existing Unix socket**, using the WebSocket
HTTP Upgrade handshake described by the Codex App Server protocol. It sends
`initialize`, then `initialized`, and retains one connection for RPC replies and
notifications. It does not launch Codex, start a daemon, discover private session
files, log in, or automatically choose another transport.

The adapter can read stored thread metadata and paginated history without
invoking a model. A dispatch targets the configured `host_id` and the exact
persisted `thread_id`. If a working directory was included in the binding, the
returned directory must match. An ephemeral thread, an unexpected thread ID,
missing required status data, or an unavailable target is not substituted with
a different conversation. There are two explicitly selected delivery modes:

| CLI selection | Submission | Required deployment conditions |
| --- | --- | --- |
| `--delivery tool-output` (default) | `turn/start` with named tool output | `exclusive_writer` and `restricted_tools` |
| `--delivery queue` (opt-in, experimental) | `thread/queue/add` with a fixed dispatch notice | The same owning App Server, supported experimental Queue protocol, `restricted_tools`, and the dispatch-context MCP tool |

The serialized configuration value for the default is `"tool_output"`; the CLI
spelling is `tool-output`. An unsupported Queue endpoint never causes a fallback
to tool output, a CLI command, or a newly created thread.

For a permitted dispatch the adapter:

1. Subscribes to its own notification receiver before issuing host requests.
2. Reads the exact thread, resumes that thread with `excludeTurns: true`, checks
   its identity again, and reads its current status once more. It rechecks local
   source scope, assignment, binding, and host configuration after asynchronous
   preflight and immediately before resume/submission.
3. Returns `busy` for an active thread. Unknown or non-idle states do not receive
   work. These are event-time checks, not a periodic model or status polling loop.
4. Submits once using the configured mode. Tool-output delivery uses
   `turn/start`, `input: []`, and named `toolOutput` with `retain: true`. Queue
   delivery uses a persisted UUID and the immutable MCP-context flow below.
5. Waits for the matching thread **and** turn's terminal notification. Queue
   delivery first correlates the expected user message with that turn. A start
   or queue acknowledgement alone never completes a job. Other threads' or
   other turns' notifications cannot complete it.

Tool-output delivery can add an optional `outputSchema` to the turn; its payload
includes a dispatch correlation ID and requests retention in model history.
Verify `toolOutput.retain` against the installed schema and persisted history;
an older server may ignore an unknown field. Queue accepts neither `toolOutput`
nor `outputSchema` and does not apply per-dispatch settings. Both modes preserve
the thread's model, permissions, approval policy, working directory, and goal.
It never calls `thread/start`, `thread/fork`, `turn/steer`, `thread/inject_items`,
or `thread/shellCommand`. Unsupported server-initiated requests are answered
with an RPC error; this client never grants host tool approvals.

The terminal `output` is assembled from completed final agent-message items,
with duplicate item IDs replaced rather than appended. Commentary and partial
answers do not become the structured final answer. Request frames are capped at
four MiB and collected final output at one MiB.

## Tool-output delivery and the concurrent writer limitation

**Automatic submission is disabled by default.** The `exclusive_writer` setting
is an operator assertion that this integration is the only actor submitting to
the selected threads. It is not a lock provided by Codex and does not implement
one. Do not set it for a thread that can simultaneously receive human input,
another automation, an active goal, or input from another client.

The current official `TurnStartParams` implementation explicitly allows
`turn/start` to add input to an already active turn. It exposes no documented
atomic "start only if still idle" precondition. The documented `toolOutput`
behavior also permits delivery into an active regular turn. A read-idle followed
by start therefore cannot eliminate the race with a separate desktop writer.
The extra read and the adapter's local per-thread mutex do not fix that race.

Consequently, tool-output delivery may observe and index a shared desktop
thread, but automatic submission remains blocked when another actor can write
to it. Version 0.3.0 introduces a separate opt-in Queue adapter for the owning
server's atomic admission contract. It does not relax the default mode's guard
or silently invoke the `codex queue` command.

An explicitly owned, dedicated routing thread can use this adapter. It is a
persistent conversation, not an endlessly running model: external events cause
new turns, and idle periods cause no turn submissions. RightWake does not activate
Codex goals to simulate a daemon.

## Opt-in Queue delivery

The Queue adapter was checked against the experimental protocol generated by
the official Codex CLI 0.166.0 binary. It initializes with
`capabilities.experimentalApi: true`. The [pinned Queue service](https://github.com/openai/codex/blob/6ff1e1ca3b30fc8b914720681fcf5fe62e95f856/codex-rs/ext/queue/src/service.rs)
wakes a loaded idle thread after enqueueing, waits while another turn is active,
and admits work through the core's atomic `StartIfIdle` operation. This addresses
the race between the bridge's last idle read and a competing writer's turn.

The scheduler must belong to **the same App Server that owns the target
interactive thread**. A separately launched server sharing `CODEX_HOME` can see
the same persisted thread ID without sharing the original server's active-turn
state or scheduler. That is not a supported way to attach to Desktop. Verify
the actual owning socket; RightWake neither launches a replacement server nor
discovers ownership from private session files. `host inspect` cannot prove
that an endpoint is the Desktop's owning server.

### Immutable context instead of external user instructions

Queue accepts user input only. To keep event text out of that authority level,
RightWake uses the following contract for both work and router dispatches:

1. Persist the exact target, request context, and any output schema under the
   durable dispatch UUID before submitting. Reusing that UUID for different
   stored content is rejected.
2. Enqueue a fixed application notice whose only variable is that canonical UUID. No
   event body, title, context-card text, or schema content is interpolated into
   the user message. The notice instructs the existing thread to call the
   read-only `right_wake_dispatch_context` MCP tool with `{"dispatch_id":"…"}`.
3. Return the immutable context through the tool. External event and card data
   remain untrusted; the notice preserves the exact thread and requires a local
   draft and the specified result format. The tool rejects mismatching caller
   thread metadata when the host supplies it and records successful retrieval.
4. Match the acknowledgement's `clientUserMessageId` and input, then correlate
   `userMessage.clientId` plus the unchanged notice with the observed turn ID.
   A server-generated item ID is not the client ID. An edited queued message
   retaining the old client ID is not accepted as the original dispatch.
5. Accept a completed Queue result only after both the matching terminal event
   and a recorded successful context read. A turn that finishes without fetching
   context becomes `dispatch_context_not_read`; it cannot create a fabricated
   draft or routing result from the notice alone.

The context tool must already be available to that existing thread and use the
same RightWake state directory as the worker. Queue does not install the tool,
grant it access, or provide a per-turn MCP allowlist. A read receipt is evidence
of context retrieval, not proof of model correctness or tool isolation. Where
caller thread metadata is absent, the tool does not claim independent caller
identity verification.

### Paused queues and read-only inspection

An interrupted thread can leave its host queue paused. RightWake never calls
`thread/queue/start`, deletes or edits existing queue entries, or interrupts a
turn to make room. A queued dispatch that does not produce a matching terminal
event before its deadline remains `unknown` for reconciliation; it is not
automatically re-enqueued. The operator retains control over any resume/unpause.

To configure and inspect an explicit endpoint without enabling automatic work:

```sh
right-wake host add --id local-codex \
  --socket /absolute/path/to/owning-app-server.sock --delivery queue
right-wake host inspect --host local-codex --thread EXISTING_THREAD_UUID
```

This leaves `restricted_tools` false. After the actual tool and credential
boundary below has been established, the host can be configured with
`--restricted-tools`. That flag records the verified deployment condition; it
does not implement the restriction. Queue does not require `--exclusive-writer`
when it uses the verified owning server's scheduler.

`host inspect` reads that exact stored thread and a single queue page, limited
to 20 entries. It returns queue IDs and client IDs without queued message text,
plus `next_cursor`/`has_more` and `model_turn_requested: false`. It does not resume,
enqueue, start, or invoke the model. A successful handshake alone is not Queue
support; Queue inspection additionally requires a successful `thread/queue/list`.

The verified queue/list response exposes no paused-state field, so inspection
reports `queue_paused: null` and `pause_state: "not_exposed_by_protocol"`. An idle
thread is not proof of an unpaused queue. Absence from the first queue page—or
even absence from the pending queue after a message has started—is not proof
that a dispatch was never accepted. Inspection does not prove Desktop ownership,
context-tool availability, model execution, or remote-tool isolation.

## Tool isolation is a separate deployment requirement

Neither an exclusive writer nor atomic Queue admission stops an existing thread
from invoking a previously enabled remote write tool. Both Codex delivery modes
require the application's separate `restricted_tools` host gate, which defaults
to false. Setting it is an operator assertion that an actual tool
and credential boundary has been established and tested; it is not proof that
RightWake dynamically restricts the thread's tools.

The stable schema generated from Codex CLI 0.166.0 has no per-turn MCP tool
allowlist, and experimental Queue add inherits the thread's settings and tools.
`disabledPluginIds` changes the thread's disabled-plugin list;
permission, sandbox, and approval overrides can become later-turn defaults.
A filesystem read-only sandbox does not by itself establish that a remote MCP
service is read-only. A prompt saying "draft only" is not an authorization gate.

The host setup must remove remote mutation capabilities from automatic turns,
including alternative shell/API credential paths, and separate draft approval
authority from model-accessible tools and IPC. The deployment must also protect
the bridge's database and approval credentials from a model running under the
same effective identity. A dedicated configuration/service boundary is required
where same-process or same-user access would bypass those controls. If this has
not been established, automatic dispatch stays disabled.

## Delivery certainty and recovery

`completed` requires an observed matching `turn/completed` event whose status is
`completed`. Queue also requires the client-message correlation and context-read
receipt described above. `failed` includes a known failed or interrupted turn.
An RPC rejection is distinct from a completion and may require intervention.

Before submission, an unavailable connection or unconfirmed target returns
`blocked`: no dispatch has been submitted, so the durable queue can wait for the host
without retaining an uncertain execution lock. An explicit host rejection is
reported separately.

An acceptance timeout, connection loss, notification overflow, protocol mismatch
after submission, or missing completion returns `unknown`. Where available the
turn ID is included. Queue unknown diagnostics also retain the dispatch UUID
and known queued-submission ID. RightWake does not retry or interrupt that work
automatically: it may still be pending or executing. Persist this state and
reconcile both queue and stored history before deciding whether another
submission is safe. Queue's definite method/capability/parameter rejections are
distinct from an internal error after a potentially persisted enqueue; the
latter stays unknown.

The dispatch ID in tool output or Queue's `clientUserMessageId` supports
reconciliation but is not a server-side idempotency key. The [pinned Queue
store](https://github.com/openai/codex/blob/6ff1e1ca3b30fc8b914720681fcf5fe62e95f856/codex-rs/state/src/runtime/queued_items.rs)
generates a new queue entry on enqueue and does not deduplicate by client ID.
Neither mode promises exactly-once execution.

An App Server notification receiver is scoped to that connection and server.
It is not a persistent global desktop event bus. Reconnection does not replay
events or restore subscriptions automatically. `thread/read` does not subscribe;
`subscribe_existing` performs an explicit resume. The application must reconnect,
resubscribe, and reconcile its durable records deliberately.

The application should retain `host_id`, `thread_id`, `turn_id`, item IDs, input
hashes, and the last successfully indexed revision. Opaque pagination cursors
are not assumed to be permanent changefeed tokens. History reversal, deletion,
or an invalid cursor requires a bounded re-read and index repair.

## Generic command adapter

`GenericCommand` launches an operator-configured **absolute executable path**
with a literal argument vector. One JSON request is written to stdin; external
event content is never substituted into a command string or argument. The
configured program is trusted executable code, not a sandbox. It must honor the
same existing-thread and approval contract. An administrator can explicitly
configure an interpreter, but RightWake itself never selects one automatically.

The optional host-level `cwd` must be an absolute path to an existing directory.
When configured, every child process starts in that directory. If omitted, it
inherits the worker process's working directory. A `cwd` inside the input JSON,
event content, or request target cannot override this trusted host setting.
This working-directory selection does not impose a filesystem sandbox.

Both stdout and stderr are bounded. Stderr is not automatically exposed because
adapter logs may contain credentials. Timeout requests termination of the direct
child process, but cannot undo work already accepted by a remote host or promise
termination of independently spawned descendants. Such results remain unknown.

`run_json` is the primitive for a classifier. A worker adapter used through
`dispatch_existing` must return a complete correlation envelope:

```json
{
  "schema_version": 1,
  "dispatch_id": "the-request-dispatch-id",
  "target": {
    "host_id": "local-codex",
    "thread_id": "the-exact-existing-thread-id",
    "cwd": null
  },
  "outcome": {
    "status": "completed",
    "thread_id": "the-exact-existing-thread-id",
    "turn_id": "the-observed-terminal-turn-id",
    "output": "the-final-agent-output"
  }
}
```

A zero process exit code without this matching envelope is not success.
Malformed JSON, a different dispatch/host/thread, or a missing terminal turn ID
produces `unknown`. The adapter cannot independently prove that a third-party
command tells the truth; use a trusted, tested command implementation.

## What has and has not been verified

The repository includes mock protocol tests using an in-process duplex stream
and the same real WebSocket handshake and transport loop as production. They
cover existing-thread resumption, completion arriving
before acknowledgement, unrelated-thread events, busy targets, a changed target
ID, missing targets, lost connections, missing completions, and rejected host
approval requests. The native Unix-socket transport has a separately ignored
integration test, because this cloud environment returns `EPERM` for socket bind.
Run it on a Mac/Linux host that permits local sockets:

```sh
cargo test --lib host::tests::real_unix_transport_smoke -- --ignored
```

The 0.3.0 Queue fixtures in `src/host_queue_tests.rs` use the same production
WebSocket client. They cover explicit experimental initialization, immutable
notice delivery without external user-role text, exact client/thread/turn
correlation, completion before acknowledgement, a simulated idle-admission
race, busy targets, unsupported methods, uncertain admission, edited input,
paused/pending queues without forced start, UUID validation, and read-only
inspection. Guard tests reject a scope change after asynchronous preflight
before either resume or submission. These are protocol fixtures, not live
Desktop or model tests.

Command tests cover untrusted text passed literally through
stdin, a successful process exit without a valid receipt, rejection of invalid
working directories, and a real child process using the configured directory
while ignoring conflicting input fields.

### Historical tool-output protocol smoke test (RightWake 0.2.0; 2026-11-04)

The official Linux x86-64 Codex CLI 0.166.0
[`codex-x86_64-unknown-linux-musl.tar.gz` package](https://github.com/openai/codex/releases/download/rust-v0.166.0/codex-x86_64-unknown-linux-musl.tar.gz)
was downloaded from OpenAI's GitHub release and checked against its release-asset SHA-256 digest. The RightWake 0.2.0 reproduction script then regenerated the sanitized probe records:

```text
package SHA-256: a0bf572d2f93f056c6712a6da348e96078dc84f3885fd7be67807ef389cb3978
CLI binary SHA-256: b8a11fedf24b2dfb59a70c9e52b030f9a508dcd49cd2dd0d41956f4c785156b8
```

It was unpacked into a disposable workspace; no system installation, shell
profile, existing credential store, or user session was changed. The following
checks completed against that actual binary:

| Check | Result |
| --- | --- |
| `codex --version` | `codex-cli 0.166.0` |
| `app-server generate-json-schema` | Successful; the used request fields and `toolOutput.retain` are present |
| Isolated stdio `initialize` / `initialized` | Accepted |
| `thread/list` in a fresh configuration directory | Empty, as expected |
| `thread/read` for a nonexistent UUID | Rejected |
| `thread/resume` for the same nonexistent UUID | Rejected |
| `thread/list` after rejected resume | Still empty; no fallback thread created |
| Process exit | `0`; the reproducible temporary-directory run reports only the CLI's helper-alias warning |
| Model turns submitted | `0` |

The [sanitized report, protocol transcript, and standard-library reproduction
script](../tests/fixtures/codex-0.166.0/README.md) are checked in. The reproduction
uses a temporary configuration directory; Codex warns that it does not create
PATH helper aliases under `/tmp`, which does not prevent these protocol checks.

The stdio smoke probe is separate from the shipped Unix-socket adapter. It
validates actual CLI protocol parsing and missing-target behavior, not the
kernel transport or desktop integration. **Real existing-thread model execution,
context continuation, shared desktop busy handling, and remote-tool isolation
remain unverified.** No Codex login, user credential, private session, or model
task was used for these checks. These preserved 0.2.0 records did not establish
shared-desktop automatic delivery.

### Actual experimental Queue probe (RightWake 0.3.0; 2026-11-04)

A [separate reproducible probe](../tests/fixtures/codex-0.166.0-queue/README.md)
records fresh sanitized responses from the same official 0.166.0 executable.
It rechecked the executable digest shown above and generated both stable and
experimental schemas. This recorded run did not reverify the release archive;
the report states `archive_reverified_in_this_run: false`. The earlier fixture
and its historical download evidence are preserved unchanged.

| Check | Recorded result |
| --- | --- |
| Stable versus experimental schemas | Queue add parameters are present only with `--experimental` |
| Queue add fields | `threadId`, `input`, `clientUserMessageId`; no `toolOutput` or `outputSchema` |
| Queue list response fields | `data`, `nextCursor`; no paused-state field |
| Without `experimentalApi` | Queue list/add/start reject with `-32600` |
| With `experimentalApi` | The same methods reach target validation and reject the synthetic absent thread with `-32603` |
| Isolated thread lists before and after | Empty; no fallback thread created |
| Accepted queue submissions and model turns | `0` |

The probe uses fresh temporary profiles, an empty workspace, and a child
environment without account credentials or the user's home configuration.
Queue add/start are exercised only against an ID already verified absent in
that isolated store. It does not resume a real thread, log in, call a model,
connect to Desktop, or exercise an MCP tool. The owning-server scheduler and
client-ID behavior are supported separately by the pinned upstream source and
tests linked below; this runtime probe establishes method/capability parsing
and missing-target rejection only. **Live existing-thread Queue execution,
Desktop ownership, context continuation, and effective tool isolation remain
deployment verification requirements.**

Before enabling a host, record the exact CLI and desktop versions and generate
the installed protocol schema:

```sh
codex --version
codex app-server generate-json-schema --out ./codex-schemas
codex app-server generate-json-schema --experimental --out ./codex-experimental-schemas
```

Then verify in disposable, explicitly approved existing test threads:

- The configured socket belongs to the intended host and the known thread IDs
  are visible with the expected metadata and permissions. For Queue, establish
  that it is the same shared App Server owning the interactive thread; a second
  server sharing storage is insufficient.
- Two threads with different prior decisions retain their IDs and use their own
  history when receiving an event; neither is recreated or forked.
- A busy target receives no submission, and an idle notification is observable
  through the owning server connection. For Queue, verify concurrent admission
  waits for the competing turn, and an interrupted queue is not force-started.
- An unloaded thread resumes under its exact persisted identity. Archived,
  missing, mismatched-provider, disconnected, and expired targets never cause
  a new-thread fallback.
- Completion and history reconciliation identify the same dispatch and turn,
  including connection loss before/after acceptance and restart recovery.
- Original thread tools cannot bypass draft approval through connectors, direct
  network requests, shell credentials, or the bridge's own approval endpoint.
- For tool-output delivery, the ownership policy actually excludes another
  writer. For Queue, verify the experimental method and same-owner scheduling
  contract rather than asserting exclusive ownership of a shared desktop.
- For Queue, the existing thread can fetch its immutable dispatch context from
  the same RightWake state directory. Missing tooling, a target mismatch, or a
  missing read receipt must not become a successful draft or router result.

Continuing the same stored thread preserves conversation identity and access to
saved context. It does not promise that unlimited history remains verbatim in
the model context; Codex can compact long conversations. Context cards are
derived indexes and must not replace the bound working conversation.

## Official references reviewed

The stable-protocol references were reviewed on 2026-11-03. Queue sources and
the generated 0.166.0 schema were reviewed on 2026-11-04. The installed binary's
generated schema remains the compatibility authority. GitHub `main` links are
implementation evidence, not a claim about the user's installed release. Queue
evidence is pinned to the source commit behind `rust-v0.166.0`:
`6ff1e1ca3b30fc8b914720681fcf5fe62e95f856`.

- [Codex App Server protocol and transports](https://learn.chatgpt.com/docs/app-server)
- [Current TurnStartParams and tool-output types](https://github.com/openai/codex/blob/main/codex-rs/app-server-protocol/src/protocol/v2/turn.rs)
- [Thread pagination protocol](https://github.com/openai/codex/blob/main/codex-rs/app-server-protocol/src/protocol/v2/thread.rs)
- [Non-interactive execution and structured results](https://learn.chatgpt.com/docs/non-interactive-mode)
- [CLI queue reference](https://learn.chatgpt.com/docs/developer-commands)
- [Codex 0.166.0 release metadata](https://releases.openai.com/codex/releases/0.166.0/release.json)
- [Pinned Queue request processor](https://github.com/openai/codex/blob/6ff1e1ca3b30fc8b914720681fcf5fe62e95f856/codex-rs/app-server/src/request_processors/thread_queue_processor.rs)
- [Pinned Queue scheduler and interruption behavior](https://github.com/openai/codex/blob/6ff1e1ca3b30fc8b914720681fcf5fe62e95f856/codex-rs/ext/queue/src/service.rs)
- [Pinned atomic StartIfIdle admission](https://github.com/openai/codex/blob/6ff1e1ca3b30fc8b914720681fcf5fe62e95f856/codex-rs/core/src/session/turn_input.rs)
- [Pinned queue persistence and client-ID handling](https://github.com/openai/codex/blob/6ff1e1ca3b30fc8b914720681fcf5fe62e95f856/codex-rs/state/src/runtime/queued_items.rs)
- [Pinned upstream Queue behavior tests](https://github.com/openai/codex/blob/6ff1e1ca3b30fc8b914720681fcf5fe62e95f856/codex-rs/app-server/tests/suite/v2/thread_queue.rs)
- [Pinned user-message clientId mapping](https://github.com/openai/codex/blob/6ff1e1ca3b30fc8b914720681fcf5fe62e95f856/codex-rs/app-server-protocol/src/protocol/v2/item.rs)
