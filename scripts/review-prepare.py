#!/usr/bin/env python3
"""Prepare an exact Waking review on a separate reviewer device (stdlib only).

This program never opens a private key, signs an approval, or sends a reply.
Read docs/approvals.md before using the output with an independent signing key.
"""

import argparse
import base64
from datetime import datetime, timezone
import hashlib
import hmac
import json
import os
from pathlib import Path
import re
import sys
import time
from urllib.parse import urlsplit


DOMAIN = b"waking.human-approval.v1\n"
STATEMENT_FIELDS = ("schema_version", "draft_id", "draft_digest", "expires_at_ms")
TARGET_FIELDS = {
    "domain", "draft_id", "draft_revision", "source_id", "resource_key",
    "event_id", "body", "reply_target", "source_revision", "source_event_revision",
    "binding_key", "binding_version", "sending_identity", "configured_identity",
    "instance", "api_url", "token_reference", "credential_fingerprint",
    "review_public_key", "configured_tenant", "allowed_resources",
}
MAX_EXPORT_BYTES = 4 * 1024 * 1024
DAY_MS = 24 * 60 * 60 * 1000


def require(condition, message):
    if not condition:
        raise ValueError(message)


def unique_object(pairs):
    value = {}
    for key, item in pairs:
        require(key not in value, "duplicate JSON field")
        value[key] = item
    return value


def reject_constant(_):
    raise ValueError("non-finite JSON number")


def parse_json(text):
    return json.loads(text, object_pairs_hook=unique_object, parse_constant=reject_constant)


def compact(value):
    # This order/encoding matches serde_json's ApprovalStatement serialization.
    return json.dumps(value, ensure_ascii=False, allow_nan=False, separators=(",", ":")).encode("utf-8")


def string(value, label, nonempty=True):
    require(isinstance(value, str), label + " must be a string")
    value.encode("utf-8", errors="strict")
    require(not nonempty or bool(value.strip()), label + " must not be empty")


def optional_string(value, label):
    if value is not None:
        string(value, label)


def hex_digest(value):
    return isinstance(value, str) and re.fullmatch(r"[0-9a-f]{64}", value) is not None


def validate_target(target, statement):
    require(isinstance(target, dict) and set(target) == TARGET_FIELDS,
            "unsupported review target fields; use the matching reviewed tool version")
    require(target["domain"] == "waking.review.v1", "unsupported review target domain")
    require(target["draft_id"] == statement["draft_id"], "review target draft_id mismatch")
    for field in (
        "draft_id", "source_id", "resource_key", "event_id", "body", "source_revision",
        "binding_key", "sending_identity", "configured_identity", "instance", "api_url",
    ):
        string(target[field], field)
    string(target["source_event_revision"], "source_event_revision", nonempty=False)
    require(len(target["body"].encode("utf-8")) <= 256 * 1024, "reply body is too large")
    for field in ("draft_revision", "binding_version"):
        require(type(target[field]) is int and 0 <= target[field] <= 2**63 - 1,
                field + " must be a nonnegative i64")
    require(target["sending_identity"] == target["configured_identity"],
            "sending identity differs from configured identity")
    reply = target["reply_target"]
    require(isinstance(reply, dict) and set(reply) == {
        "provider", "container_id", "resource_id", "comment_id", "discussion_id"
    }, "unsupported reply target fields")
    require(reply["provider"] in ("linear", "github", "gitlab"), "unsupported outbound provider")
    for field in ("container_id", "resource_id"):
        string(reply[field], "reply_target." + field)
    for field in ("comment_id", "discussion_id"):
        optional_string(reply[field], "reply_target." + field)
    for field in ("instance", "api_url"):
        url = urlsplit(target[field])
        require(url.scheme in ("http", "https") and url.hostname is not None
                and url.username is None and url.password is None
                and not url.query and not url.fragment, "invalid " + field)
    for field in ("token_reference", "configured_tenant"):
        optional_string(target[field], field)
    fingerprint = target["credential_fingerprint"]
    require((target["token_reference"] is None and fingerprint is None)
            or (target["token_reference"] is not None and hex_digest(fingerprint)),
            "credential fingerprint/reference mismatch")
    require(isinstance(target["allowed_resources"], list), "allowed_resources must be a list")
    for resource in target["allowed_resources"]:
        string(resource, "allowed_resources entry")
    key = target["review_public_key"]
    string(key, "review_public_key")
    require(len(base64.b64decode(key, validate=True)) == 32, "review_public_key must encode 32 bytes")


def prepare(document, current_ms):
    require(isinstance(document, dict), "export must be a JSON object")
    if "data" in document:
        require(set(document) == {"schema_version", "ok", "data"}
                and type(document["schema_version"]) is int and document["schema_version"] == 1
                and document["ok"] is True, "export envelope is not a successful schema 1 result")
        document = document["data"]
    require(isinstance(document, dict), "export data must be a JSON object")
    statement = document.get("statement")
    require(isinstance(statement, dict) and set(statement) == set(STATEMENT_FIELDS),
            "statement must contain exactly the four schema 1 fields")
    require(type(statement["schema_version"]) is int and statement["schema_version"] == 1,
            "unsupported statement schema_version")
    string(statement["draft_id"], "statement.draft_id")
    require(hex_digest(statement["draft_digest"]), "invalid statement.draft_digest")
    expiry = statement["expires_at_ms"]
    require(type(expiry) is int and current_ms < expiry <= current_ms + DAY_MS,
            "statement must expire in the next 24 hours")
    exact = document.get("review_target_json")
    string(exact, "review_target_json")
    exact_bytes = exact.encode("utf-8")
    require(hmac.compare_digest(hashlib.sha256(exact_bytes).hexdigest(), statement["draft_digest"]),
            "review_target_json SHA-256 does not match statement.draft_digest")
    target = parse_json(exact)
    validate_target(target, statement)
    ordered = {field: statement[field] for field in STATEMENT_FIELDS}
    statement_bytes = compact(ordered)
    signing = DOMAIN + statement_bytes
    expires_utc = datetime.fromtimestamp(expiry / 1000, timezone.utc).isoformat()
    # ASCII JSON makes every control/bidi/non-ASCII character explicit. body.txt
    # separately contains the exact UTF-8 body, without an appended newline.
    review = (
        "Waking independent review (not yet approved)\n"
        "The complete payload below was parsed from the exact SHA-256-verified bytes.\n"
        "Outer draft fields and exported signing_bytes_base64 are not used.\n"
        "JSON string escapes are literal notation, not additional message characters.\n"
        "Use body.txt in a trusted text editor for the exact UTF-8 body.\n"
        "A matching hash does not establish permission: independently check the recipient,\n"
        "account, API origin, reviewer public key and requested action before signing.\n\n"
        "Expires (UTC): " + expires_utc + "\n"
        "Signing bytes SHA-256: " + hashlib.sha256(signing).hexdigest() + "\n\n"
        "STATEMENT\n" + json.dumps(ordered, ensure_ascii=True, indent=2) + "\n\n"
        "COMPLETE VERIFIED REVIEW TARGET\n" + json.dumps(target, ensure_ascii=True, indent=2) + "\n"
    ).encode("utf-8")
    return {
        "signing.bin": signing,
        "review.txt": review,
        "statement.json": statement_bytes,
        "body.txt": target["body"].encode("utf-8"),
        "review-target.json": exact_bytes,
    }


def write_new_private_directory(output, files):
    # Refuse even an existing empty directory or symlink; never overwrite a review.
    output.mkdir(mode=0o700)
    os.chmod(output, 0o700)
    for name, content in files.items():
        flags = os.O_WRONLY | os.O_CREAT | os.O_EXCL | getattr(os, "O_NOFOLLOW", 0)
        fd = os.open(output / name, flags, 0o600)
        with os.fdopen(fd, "wb") as file:
            os.fchmod(file.fileno(), 0o600)
            file.write(content)
            file.flush()
            os.fsync(file.fileno())


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("export", type=Path, help="JSON produced by waking drafts export")
    parser.add_argument("--out", required=True, type=Path, help="new private review directory (must not exist)")
    args = parser.parse_args()
    require(os.name == "posix", "use a Linux/macOS reviewer device with private file permissions")
    require(args.export.is_file(), "export must be a regular file")
    with args.export.open("rb") as file:
        raw = file.read(MAX_EXPORT_BYTES + 1)
    require(len(raw) <= MAX_EXPORT_BYTES, "export exceeds the 4 MiB review limit")
    files = prepare(parse_json(raw.decode("utf-8", errors="strict")), time.time_ns() // 1_000_000)
    write_new_private_directory(args.out, files)
    print("Prepared an unsigned private review at " + json.dumps(str(args.out), ensure_ascii=True))
    print("Read review.txt and body.txt. This tool has not signed or sent anything.")


if __name__ == "__main__":
    try:
        main()
    except (OSError, ValueError, TypeError, RecursionError, OverflowError) as error:
        print("Review preparation refused: " + ascii(str(error)), file=sys.stderr)
        sys.exit(1)
