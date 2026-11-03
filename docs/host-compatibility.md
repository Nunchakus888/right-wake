# Codex host compatibility and verification

## Implemented contract

Waking connects to an **explicit existing Unix socket**, using the WebSocket
HTTP Upgrade handshake described by the Codex App Server protocol. It sends
`initialize`, then `initialized`, and retains one connection for RPC replies and
notifications. It does not launch Codex, start a daemon, discover private session
files, log in, or automatically choose another transport.

The adapter can read stored thread metadata and paginated history without
invoking a model. A dispatch targets the configured `host_id` and the exact
persisted `thread_id`. If a working directory was included in the binding, the
returned directory must match. An ephemeral thread, an unexpected thread ID,
missing required status data, or an unavailable target is not substituted with
a different conversation.

For a permitted dispatch the adapter:

1. Subscribes to its own notification receiver before issuing host requests.
2. Reads the exact thread, resumes that thread with `excludeTurns: true`, checks
   its identity again, and reads its current status once more.
3. Returns `busy` for an active thread. Unknown or non-idle states do not receive
   work. These are event-time checks, not a periodic model or status polling loop.
4. Submits one `turn/start` with `input: []` and a named `toolOutput` with
   `retain: true`. The external payload includes a dispatch correlation ID and
   explicitly requests retention in model history. Verify this field against
   the installed schema and test persisted history; an older server may ignore
   an unknown field.
5. Waits for the matching thread **and** turn's terminal notification. A start
   acknowledgement alone never completes a job. Other threads' notifications
   cannot complete it.

Only an optional `outputSchema` is added to a turn. The adapter does not change
the thread's model, permissions, approval policy, working directory, or goal.
It never calls `thread/start`, `thread/fork`, `turn/steer`, `thread/inject_items`,
or `thread/shellCommand`. Unsupported server-initiated requests are answered
with an RPC error; this client never grants host tool approvals.

The terminal `output` is assembled from completed final agent-message items,
with duplicate item IDs replaced rather than appended. Commentary and partial
answers do not become the structured final answer. Request frames are capped at
four MiB and collected final output at one MiB.

## The concurrent desktop writer limitation

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

Consequently, this version may observe and index a shared desktop thread, but
does not claim safe automatic delivery to concurrently writable desktop threads.
Such jobs remain blocked/pending until a supported ownership or atomic queueing
contract is established. A future `codex queue` adapter must be separately
validated; this implementation does not silently fall back to that command.

An explicitly owned, dedicated routing thread can use this adapter. It is a
persistent conversation, not an endlessly running model: external events cause
new turns, and idle periods cause no turn submissions. Waking does not activate
Codex goals to simulate a daemon.

## Tool isolation is a separate deployment requirement

An exclusive writer does not stop an existing thread from invoking a previously
enabled remote write tool. The application's separate `restricted_tools` host
gate defaults to false. Setting it is an operator assertion that an actual tool
and credential boundary has been established and tested; it is not proof that
Waking dynamically restricts the thread's tools.

The stable schema generated from Codex CLI 0.166.0 has no per-turn MCP tool
allowlist. `disabledPluginIds` changes the thread's disabled-plugin list;
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
`completed`. `failed` includes a known failed or interrupted turn. An RPC
rejection is distinct from a completion and may require operator intervention.

Before `turn/start`, an unavailable connection or unconfirmed target returns
`blocked`: no turn has been submitted, so the durable queue can wait for the host
without retaining an uncertain execution lock. An explicit host rejection is
reported separately.

An acceptance timeout, connection loss, notification overflow, protocol mismatch
after submission, or missing completion returns `unknown`. Where available the
turn ID is included. Waking does not retry or interrupt that turn automatically:
the host may still be executing it. The caller must persist this state and
reconcile stored history before deciding whether another submission is safe.
The dispatch ID in tool output supports reconciliation but is not a server-side
idempotency key and does not promise exactly-once execution.

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
configure an interpreter, but Waking itself never selects one automatically.

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

Command tests cover untrusted text passed literally through
stdin, a successful process exit without a valid receipt, rejection of invalid
working directories, and a real child process using the configured directory
while ignoring conflicting input fields.

### Actual CLI protocol smoke test (2026-11-03)

The official Linux x86-64 Codex CLI 0.166.0 package was downloaded from OpenAI's
release service and checked against its published SHA-256 metadata:

```text
package SHA-256: 1d44b28bc4ac9937d7d3532264baa68a507ce364956a0d4ebb856f331cdee254
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
task was used for these checks. There is no tested shared-desktop automatic
delivery claim in this release.

Before enabling a host, record the exact CLI and desktop versions and generate
the installed protocol schema:

```sh
codex --version
codex app-server generate-json-schema --out ./codex-schemas
```

Then verify in disposable, explicitly approved existing test threads:

- The configured socket belongs to the intended host and the known thread IDs
  are visible with the expected metadata and permissions.
- Two threads with different prior decisions retain their IDs and use their own
  history when receiving an event; neither is recreated or forked.
- A busy target receives no submission, and an idle notification is observable
  through the chosen owning server connection.
- Unloaded, archived, missing, mismatched-provider, disconnected, and expired
  targets remain recoverable pending work without a new-thread fallback.
- Completion and history reconciliation identify the same dispatch and turn,
  including connection loss before/after acceptance and restart recovery.
- Original thread tools cannot bypass draft approval through connectors, direct
  network requests, shell credentials, or the bridge's own approval endpoint.
- The chosen ownership policy genuinely prevents another writer. Until Codex
  offers an accepted atomic operation, a shared interactive writer is unsupported.

Continuing the same stored thread preserves conversation identity and access to
saved context. It does not promise that unlimited history remains verbatim in
the model context; Codex can compact long conversations. Context cards are
derived indexes and must not replace the bound working conversation.

## Official references reviewed

Reviewed on 2026-11-03; the installed binary's generated schema remains the
compatibility authority. GitHub `main` links are implementation evidence, not a
claim about the user's installed release.

- [Codex App Server protocol and transports](https://learn.chatgpt.com/docs/app-server)
- [Current TurnStartParams and tool-output types](https://github.com/openai/codex/blob/main/codex-rs/app-server-protocol/src/protocol/v2/turn.rs)
- [Thread pagination protocol](https://github.com/openai/codex/blob/main/codex-rs/app-server-protocol/src/protocol/v2/thread.rs)
- [Non-interactive execution and structured results](https://learn.chatgpt.com/docs/non-interactive-mode)
- [CLI queue reference](https://learn.chatgpt.com/docs/developer-commands)
- [Codex 0.166.0 release metadata](https://releases.openai.com/codex/releases/0.166.0/release.json)
