# Independently approve one exact reply

RightWake creates drafts by default. Sending a draft requires an Ed25519 signature from an independent reviewer. This guide provides a manual, usable approval path for v0.1.0; there is no approval button exposed to the worker or its agent.

The RightWake service remains a native binary with no Python dependency. The optional [review-prepare.py](../scripts/review-prepare.py) utility uses only Python 3's standard library **on a separate Linux/macOS reviewer device**. The signing commands below require OpenSSL 3. Check `openssl version`; the system `openssl` on some machines may be a different implementation.

The private key and its passphrase must never enter the worker, agent filesystem, environment, repository, or chat. The reviewer runs the signing commands personally on their trusted device. The prepare utility never reads a key, signs, contacts a provider, or sends a reply.

## 1. Create the reviewer key on the reviewer device

Run these commands in a trusted terminal. The new directory must not already exist. OpenSSL asks for a passphrase to encrypt the private key; keep that passphrase out of command arguments and shell history.

```sh
set -eu
umask 077
mkdir -m 700 waking-reviewer
cd waking-reviewer
openssl genpkey -algorithm ED25519 -aes-256-cbc -out reviewer-private.pem
openssl pkey -in reviewer-private.pem -pubout -out reviewer-public.pem
openssl pkey -pubin -in reviewer-public.pem -outform DER -out reviewer-public.der
```

RightWake expects standard base64 of the **32 raw public-key bytes**, rather than base64 of a PEM or complete DER file. This extraction checks the Ed25519 SubjectPublicKeyInfo prefix and length before writing the public value:

```sh
python3 - <<'PY'
import base64
from pathlib import Path

der = Path("reviewer-public.der").read_bytes()
prefix = bytes.fromhex("302a300506032b6570032100")
if len(der) != len(prefix) + 32 or not der.startswith(prefix):
    raise SystemExit("Unexpected Ed25519 public-key encoding")
value = base64.b64encode(der[len(prefix):]).decode("ascii")
with Path("reviewer-public.b64").open("x", encoding="ascii") as output:
    output.write(value + "\n")
print(value)
PY
```

Keep an encrypted backup of the private key under the reviewer's control. Only the public value is transferred to the RightWake operator. The command formats follow OpenSSL's [genpkey](https://docs.openssl.org/3.0/man1/openssl-genpkey/) and [pkey](https://docs.openssl.org/3.0/man1/openssl-pkey/) documentation; the Ed25519 public-key encoding is specified in [RFC 8410](https://www.rfc-editor.org/rfc/rfc8410.html#section-10.1).

## 2. Configure the public key and sending identity

On the RightWake machine, the trusted operator obtains the existing source configuration with `waking --json source list`, copies the relevant source object into a separate `source.json`, and adds these fields while preserving its existing tenant, triggers, scope, and other settings:

```json
{
  "review_public_key": "BASE64_OF_THE_32_RAW_PUBLIC_BYTES",
  "sending_identity": "EXACT_PROVIDER_ACCOUNT_ID",
  "token_env": "WAKING_PROVIDER_TOKEN"
}
```

This is a field fragment, **not a complete source file**. `source add --file` replaces the source with the same ID; importing only this fragment would omit required settings. The sending identity is the provider's account ID, not a display name. RightWake checks it against the account returned by the provider before sending. Configure the referenced provider token through the protected environment file already used by the service, then import the complete source object:

```sh
waking --data-dir /path/to/waking-data source add --file /path/to/source.json
```

The public key is trusted operator configuration. An executing agent that can replace this configuration can replace the approval authority, so restrict access to the configuration as well as to the private key. Configure the identity before generating the draft; changing an existing draft's identity is not an approval operation. Key, credential, origin, identity, content, or version changes invalidate old approval statements.

## 3. Export the draft, then prepare the review independently

On the RightWake machine, substitute the exact draft ID from `waking drafts list`:

```sh
set -eu
umask 077
set -C
waking --data-dir /path/to/waking-data --env-file /path/to/waking.env --json \
  drafts export EXACT_DRAFT_ID --expires-in 3600 > draft-export.json
```

The export contains plaintext reply content and identifiers, plus a credential fingerprint and environment-variable name; it does not contain the API token. Transfer the file to the reviewer through your trusted transfer channel. Obtain the prepare utility from a reviewed RightWake revision independently of the draft export. Run it on the reviewer device with a fresh output directory:

```sh
python3 /path/to/waking/scripts/review-prepare.py draft-export.json --out review-001
```

The utility verifies SHA-256 of the **exact UTF-8 `review_target_json` string** against `statement.draft_digest`. It then parses that same payload and checks the draft ID, schema, strict statement fields, and an expiry in the next 24 hours. It rejects duplicate JSON fields and malformed or unsupported targets. It reconstructs the bytes to sign using the Rust field order, independent of any exported `signing_bytes_base64`:

```text
waking.human-approval.v1\n
{"schema_version":1,"draft_id":"...","draft_digest":"...","expires_at_ms":...}
```

Here `\n` denotes one newline byte between the domain and compact JSON. There is no trailing newline. The actual output is binary-safe `signing.bin`, not the illustrative text above.

The output directory has mode `0700`; its files have mode `0600`. An existing output directory is refused, including an empty directory or symlink. The files are:

| File | Purpose |
| --- | --- |
| `review.txt` | Complete validated payload, statement, expiry, and signing-byte digest. JSON escapes expose control and Unicode characters explicitly. |
| `body.txt` | Exact UTF-8 reply body, without an added final newline. Open in a trusted text editor. |
| `review-target.json` | Exact original bytes whose SHA-256 was checked. |
| `statement.json` | Validated four-field statement in the required order. |
| `signing.bin` | Reconstructed domain-prefixed bytes for the independent signature. |

**Personally read the verified `review.txt` and exact `body.txt` before signing.** Check the provider, instance/API origin, container, resource, comment/discussion parent, sending identity, tenant and resource scope, source/event revision, binding and draft versions, and expiry. Match `review_public_key` to your trusted `reviewer-public.b64`. A matching hash establishes consistency, not that the requested action is appropriate. Read from this verified payload; the outer export's `draft` field, a chat summary, or an agent's explanation is not an independent review.

If anything is wrong, do not sign. Request an edited/replaced draft and a new export. If the statement expires during review, export again and use a fresh review directory.

## 4. Sign the reviewed bytes and assemble the envelope

Only after the human review, run these commands on the reviewer device. This example assumes the key files and `review-001` are in the current directory:

```sh
openssl pkeyutl -sign -rawin -inkey reviewer-private.pem \
  -in review-001/signing.bin -out review-001/signature.bin
openssl pkeyutl -verify -rawin -pubin -inkey reviewer-public.pem \
  -in review-001/signing.bin -sigfile review-001/signature.bin
```

OpenSSL prompts for the private-key passphrase. For Ed25519 use `-rawin` with the actual file and no `-digest` option; do not sign a SHA-256 text string or a prettified statement. See the official [pkeyutl Ed25519 documentation](https://docs.openssl.org/3.0/man1/openssl-pkeyutl/#ed25519-and-ed448-algorithms).

Assemble the two-field `ApprovalEnvelope` without changing the statement:

```sh
python3 - <<'PY'
import base64
import json
from pathlib import Path
import time

folder = Path("review-001")
statement = json.loads((folder / "statement.json").read_text(encoding="utf-8"))
fields = ("schema_version", "draft_id", "draft_digest", "expires_at_ms")
if set(statement) != set(fields) or statement["schema_version"] != 1:
    raise SystemExit("Unexpected statement")
now = time.time_ns() // 1_000_000
if not now < statement["expires_at_ms"] <= now + 86_400_000:
    raise SystemExit("Export again: statement expiry is invalid")
ordered = {field: statement[field] for field in fields}
expected = b"waking.human-approval.v1\n" + json.dumps(
    ordered, ensure_ascii=False, separators=(",", ":")
).encode("utf-8")
if (folder / "signing.bin").read_bytes() != expected:
    raise SystemExit("Statement changed after preparation")
signature = (folder / "signature.bin").read_bytes()
if len(signature) != 64:
    raise SystemExit("Expected a 64-byte Ed25519 signature")
envelope = {"statement": ordered, "signature": base64.b64encode(signature).decode("ascii")}
with (folder / "approval.json").open("x", encoding="utf-8") as output:
    json.dump(envelope, output, ensure_ascii=False, separators=(",", ":"))
print("Created review-001/approval.json; no platform request was sent")
PY
```

## 5. Send exactly the approved draft

Return only `approval.json` to the authorized operator. Keep all private-key material on the reviewer device. The operator invokes the native CLI:

```sh
waking --data-dir /path/to/waking-data --env-file /path/to/waking.env --json \
  drafts send EXACT_DRAFT_ID --approval /path/to/approval.json
```

RightWake verifies the signature, current content, target, identity, configuration and revisions before the provider mutation. It also performs a remote preflight. A changed or expired action requires a new independent review. `sent` means the provider confirmed the write; `unknown` requires reconciliation and must not be automatically resent. Exporting, preparing, and signing do not themselves send a platform message.

This manual workflow establishes exact-message approval. It does not establish unrestricted agent tool safety or replace the [host deployment prerequisites](architecture.md#codex-host-contract).
