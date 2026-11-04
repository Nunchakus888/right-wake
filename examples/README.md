# Configuration and adapter examples

These files contain synthetic identifiers and environment-variable references, never credentials. Replace the organization, instance, and scope with your authorized values. `events.invalid` is deliberately a non-deployed example origin, not a hosted RightWake service. Binary/plugin installation uses the real repository and releases linked from the main README.

## Native Linear source

Copy `linear-source.json`, replace its organization UUID, and import it:

```sh
waking setup
waking --json source add --file /absolute/path/to/linear-source.json
```

An empty `allowed_resources` permits resources within the configured organization; use exact canonical keys or a single suffix `*` to narrow this. Configure `bot_actor_ids` for the identity that will publish replies. The example remains draft-only: it has no review public key or sending identity. Receive the actual API token outside chat; it is only required for authorized platform reads/writes, not raw webhook verification.

Register an existing, supported local host, inspect its real threads, and bind the full resource identity:

```sh
waking --json host add --id local --socket /absolute/path/to/codex.sock
waking --json doctor --probe
waking --json host threads --host local
waking --json bind --source linear-work \
  --resource 'linear:https://linear.app:ORGANIZATION_UUID:ISSUE_UUID' \
  --host local --thread EXISTING_THREAD_UUID --expected-version 0
```

This deliberately leaves automatic Codex execution held. Only set `--exclusive-writer` and `--restricted-tools` after establishing those actual deployment conditions, as described in [host compatibility](../docs/host-compatibility.md). They are declarations, not sandbox switches. Configure public HTTPS ingress or a self-hosted relay before expecting Linear to reach a local receiver.

## Generic signed Webhook

`generic-source.json` maps `generic-event.json` using JSON Pointers. Change the source's trusted origin, tenant, and field paths; no Rust changes or expression evaluation are needed:

```sh
waking --json source add --file /absolute/path/to/generic-source.json
```

The sender uses the exact UTF-8 request bytes, a current Unix timestamp in **seconds**, and its configured secret:

```text
X-Waking-Timestamp: <unix seconds>
X-Waking-Delivery: <unique delivery identifier>
X-Waking-Signature: sha256=<hex HMAC-SHA256(secret, timestamp + "." + raw_body)>
```

Do not reserialize the JSON after signing. A valid signature and mapped event do not assign ownership. Inspect `events list` for its canonical resource key and explicitly bind an existing destination. Generic sources have no built-in external reply adapter; their execution results stay local.

## GenericCommand executor

Register a trusted adapter with a fixed absolute executable and working directory:

```sh
waking --json host add --id custom \
  --program /absolute/path/to/my-adapter \
  --cwd /absolute/path/to/trusted-workspace --timeout 120
```

Use repeated `--arg` flags for literal arguments. RightWake does not construct a shell command from event text. The program receives one bounded JSON request on stdin; stdout must contain one complete JSON response. Diagnostic output belongs on stderr. The adapter must resolve and continue the specified existing thread itself; missing targets must be rejected without creating a replacement.

The request is the flat `DispatchRequest`: `dispatch_id`, `target`, `prompt`, `timeout_ms`, and optional `output_schema`. It has no outer request envelope; only the response carries `schema_version`. Inspect the implementation's [wire contract](../src/host.rs) when implementing an adapter. The response is:

```json
{
  "schema_version": 1,
  "dispatch_id": "COPY_THE_REQUEST_DISPATCH_ID",
  "target": {"host_id": "custom", "thread_id": "COPY_THE_EXISTING_THREAD_ID", "cwd": null},
  "outcome": {
    "status": "completed",
    "thread_id": "COPY_THE_EXISTING_THREAD_ID",
    "turn_id": "THE_ACTUAL_COMPLETED_TURN_ID",
    "output": "{\"reply\":null,\"context_summary\":\"Updated work summary\",\"open_questions\":[]}"
  }
}
```

Exit code 0 alone is not completion. Mismatched receipt identifiers, truncated output, timeouts, and possible acceptance without a result remain uncertain. The fixed directory is not a sandbox: use a trusted adapter and isolate credentials and bridge state appropriately.

For an immediately runnable demonstration without credentials, use `waking demo`. It verifies signed fixture events and resumes two pre-existing **mock** threads; it does not represent real Codex or provider acceptance.
