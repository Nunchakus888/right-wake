//! Webhook authentication and bounded provider normalization.
//!
//! Authentication uses the original bytes. Remote content never supplies a local
//! thread, working directory, command, credential, or API origin.

use crate::model::{NormalizedEvent, ProviderKind, ReplyTarget};
use base64::{engine::general_purpose::STANDARD, Engine as _};
use hmac::{Hmac, Mac};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::{collections::BTreeMap, fmt};

pub type Headers = BTreeMap<String, String>;

fn enabled_default() -> bool {
    true
}
fn payload_limit_default() -> usize {
    1024 * 1024
}
fn content_limit_default() -> usize {
    16 * 1024
}
fn trigger_patterns_default() -> Vec<String> {
    vec!["@right-wake".to_owned()]
}

/// Authentication, origins, scope, and identity are trusted local configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SourceConfig {
    pub id: String,
    pub kind: ProviderKind,
    #[serde(default)]
    pub instance: String,
    #[serde(default)]
    pub tenant_id: Option<String>,
    pub secret_env: String,
    #[serde(default)]
    pub token_env: Option<String>,
    #[serde(default = "enabled_default")]
    pub enabled: bool,
    #[serde(default = "trigger_patterns_default")]
    pub trigger_patterns: Vec<String>,
    #[serde(default)]
    pub bot_actor_ids: Vec<String>,
    /// Exact canonical resource keys, or prefixes ending in a single `*`.
    #[serde(default)]
    pub allowed_resources: Vec<String>,
    /// Trusted API origin/path override for self-hosted instances and test servers.
    #[serde(default)]
    pub api_base: Option<String>,
    #[serde(default)]
    pub signature: Option<SignatureScheme>,
    /// Base64 Ed25519 public key. No signing private key belongs in this service.
    #[serde(default)]
    pub review_public_key: Option<String>,
    #[serde(default)]
    pub sending_identity: String,
    #[serde(default)]
    pub generic: Option<GenericMapping>,
    #[serde(default = "payload_limit_default")]
    pub max_payload_bytes: usize,
    #[serde(default = "content_limit_default")]
    pub max_content_chars: usize,
    #[serde(default)]
    pub max_age_seconds: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GenericMapping {
    pub namespace: String,
    pub resource_kind: String,
    pub event_id: String,
    pub resource_id: String,
    pub content: String,
    #[serde(default)]
    pub tenant_id: Option<String>,
    #[serde(default)]
    pub conversation_id: Option<String>,
    #[serde(default)]
    pub actor_id: Option<String>,
    #[serde(default)]
    pub action: Option<String>,
    #[serde(default)]
    pub source_url: Option<String>,
    #[serde(default)]
    pub occurred_at: Option<String>,
    #[serde(default)]
    pub revision: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "snake_case")]
// One result is consumed immediately; keep the public event value unboxed.
#[allow(clippy::large_enum_variant)]
pub enum IngestOutcome {
    Event(NormalizedEvent),
    Ignored { reason: String },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProviderError {
    pub code: &'static str,
    pub message: String,
}

impl ProviderError {
    fn new(code: &'static str, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
        }
    }
}

impl fmt::Display for ProviderError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}", self.code, self.message)
    }
}

impl std::error::Error for ProviderError {}

pub type Result<T> = std::result::Result<T, ProviderError>;

/// Chosen by trusted source configuration, never by a missing request header.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SignatureScheme {
    Linear,
    #[serde(rename = "github")]
    GitHub,
    #[serde(rename = "gitlab_signing")]
    GitLabSigning,
    #[serde(rename = "gitlab_token")]
    GitLabToken,
    GenericHmac,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Verification {
    pub scheme: SignatureScheme,
    pub verified_at_ms: i64,
    pub payload_sha256: String,
}

/// Enough provenance to re-read the exact triggering remote entity before a
/// human-approved mutation. This is serialized into NormalizedEvent.revision.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SourceRevision {
    pub format_version: u32,
    pub entity_kind: String,
    pub entity_id: String,
    pub text_sha256: String,
    pub updated_at_ms: Option<i64>,
    pub resource_updated_at_ms: Option<i64>,
}

pub fn parse_timestamp(value: &Value) -> Option<i64> {
    value.as_i64().or_else(|| {
        value.as_str().and_then(|text| {
            chrono::DateTime::parse_from_rfc3339(text)
                .ok()
                .map(|time| time.timestamp_millis())
                .or_else(|| {
                    chrono::NaiveDateTime::parse_from_str(text, "%Y-%m-%d %H:%M:%S UTC")
                        .ok()
                        .map(|time| time.and_utc().timestamp_millis())
                })
        })
    })
}

fn source_revision(
    entity_kind: &str,
    entity_id: &str,
    text: &str,
    updated_at: Option<&Value>,
    resource_updated_at: Option<&Value>,
) -> String {
    serde_json::to_string(&SourceRevision {
        format_version: 1,
        entity_kind: entity_kind.to_owned(),
        entity_id: entity_id.to_owned(),
        text_sha256: sha256_hex(text.as_bytes()),
        updated_at_ms: updated_at.and_then(parse_timestamp),
        resource_updated_at_ms: resource_updated_at.and_then(parse_timestamp),
    })
    .expect("serializing a fixed source revision cannot fail")
}

fn header<'a>(headers: &'a Headers, name: &str) -> Result<&'a str> {
    let mut values = headers
        .iter()
        .filter(|(key, _)| key.eq_ignore_ascii_case(name));
    let value = values
        .next()
        .map(|(_, value)| value.as_str())
        .ok_or_else(|| {
            ProviderError::new(
                "missing_header",
                format!("Required {name} header is missing"),
            )
        })?;
    if values.next().is_some() || value.is_empty() {
        return Err(ProviderError::new(
            "invalid_header",
            format!("Ambiguous or empty {name} header"),
        ));
    }
    Ok(value)
}

fn decode_hex(value: &str) -> Result<Vec<u8>> {
    if value.len() != 64 {
        return Err(ProviderError::new(
            "invalid_signature",
            "Expected a SHA-256 signature",
        ));
    }
    let mut bytes = Vec::with_capacity(32);
    for pair in value.as_bytes().as_chunks::<2>().0 {
        let parse = |byte: u8| match byte {
            b'0'..=b'9' => Some(byte - b'0'),
            b'a'..=b'f' => Some(byte - b'a' + 10),
            b'A'..=b'F' => Some(byte - b'A' + 10),
            _ => None,
        };
        let high = parse(pair[0])
            .ok_or_else(|| ProviderError::new("invalid_signature", "Invalid signature encoding"))?;
        let low = parse(pair[1])
            .ok_or_else(|| ProviderError::new("invalid_signature", "Invalid signature encoding"))?;
        bytes.push(high * 16 + low);
    }
    Ok(bytes)
}

pub fn sha256_hex(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut output = String::with_capacity(64);
    for byte in digest {
        output.push(HEX[(byte >> 4) as usize] as char);
        output.push(HEX[(byte & 15) as usize] as char);
    }
    output
}

fn verify_hmac(key: &[u8], bytes: &[u8], signature: &[u8]) -> Result<()> {
    let mut mac = Hmac::<Sha256>::new_from_slice(key)
        .map_err(|_| ProviderError::new("invalid_secret", "Invalid webhook secret"))?;
    mac.update(bytes);
    mac.verify_slice(signature)
        .map_err(|_| ProviderError::new("invalid_signature", "Webhook signature did not match"))
}

fn timestamp_recent(timestamp_ms: i64, now_ms: i64, max_age_ms: i64) -> Result<()> {
    if timestamp_ms <= 0 || max_age_ms <= 0 || timestamp_ms.abs_diff(now_ms) > max_age_ms as u64 {
        return Err(ProviderError::new(
            "stale_timestamp",
            "Webhook timestamp is outside the accepted window",
        ));
    }
    Ok(())
}

/// Validate exactly the authentication mode configured for this source.
///
/// Generic HMAC uses `X-RightWake-Timestamp` (Unix seconds) and
/// `X-RightWake-Signature: sha256=<hex(HMAC(secret, timestamp + '.' + raw_body))>`.
pub fn verify(
    scheme: SignatureScheme,
    secret: &[u8],
    headers: &Headers,
    raw: &[u8],
    now_ms: i64,
    max_age_ms: i64,
) -> Result<Verification> {
    if secret.is_empty() {
        return Err(ProviderError::new(
            "missing_secret",
            "Webhook authentication secret is not configured",
        ));
    }
    match scheme {
        SignatureScheme::Linear => {
            let signature = decode_hex(header(headers, "Linear-Signature")?)?;
            verify_hmac(secret, raw, &signature)?;
            let payload: Value = serde_json::from_slice(raw).map_err(|_| {
                ProviderError::new("invalid_json", "Authenticated payload is not valid JSON")
            })?;
            let timestamp = payload
                .get("webhookTimestamp")
                .and_then(Value::as_i64)
                .ok_or_else(|| {
                    ProviderError::new("missing_timestamp", "Signed webhookTimestamp is missing")
                })?;
            timestamp_recent(timestamp, now_ms, max_age_ms)?;
        }
        SignatureScheme::GitHub => {
            let signature = header(headers, "X-Hub-Signature-256")?
                .strip_prefix("sha256=")
                .ok_or_else(|| {
                    ProviderError::new("invalid_signature", "Expected sha256 signature prefix")
                })?;
            verify_hmac(secret, raw, &decode_hex(signature)?)?;
        }
        SignatureScheme::GitLabSigning => {
            let message_id = header(headers, "webhook-id")?;
            let timestamp_text = header(headers, "webhook-timestamp")?;
            let timestamp = timestamp_text
                .parse::<i64>()
                .ok()
                .and_then(|x| x.checked_mul(1000))
                .ok_or_else(|| {
                    ProviderError::new("invalid_timestamp", "Invalid webhook timestamp")
                })?;
            let encoded_key = std::str::from_utf8(secret)
                .map_err(|_| ProviderError::new("invalid_secret", "Signing token must be UTF-8"))?
                .strip_prefix("whsec_")
                .ok_or_else(|| {
                    ProviderError::new("invalid_secret", "Signing token must use the whsec_ prefix")
                })?;
            let key = STANDARD.decode(encoded_key).map_err(|_| {
                ProviderError::new(
                    "invalid_secret",
                    "Signing token has invalid base64 encoding",
                )
            })?;
            if key.is_empty() {
                return Err(ProviderError::new("invalid_secret", "Signing key is empty"));
            }
            let mut message = format!("{message_id}.{timestamp_text}.").into_bytes();
            message.extend_from_slice(raw);
            let signatures = header(headers, "webhook-signature")?;
            let matched = signatures
                .split_whitespace()
                .filter_map(|entry| entry.strip_prefix("v1,"))
                .filter_map(|value| STANDARD.decode(value).ok())
                .any(|signature| verify_hmac(&key, &message, &signature).is_ok());
            if !matched {
                return Err(ProviderError::new(
                    "invalid_signature",
                    "Webhook signature did not match",
                ));
            }
            timestamp_recent(timestamp, now_ms, max_age_ms)?;
        }
        SignatureScheme::GitLabToken => {
            // Compare fixed-size hashes with HMAC's constant-time verifier; do not
            // fall back here from a source configured to require signed payloads.
            let received = header(headers, "X-Gitlab-Token")?;
            let comparison_key = b"right-wake-secret-comparison-v1";
            let mut mac = Hmac::<Sha256>::new_from_slice(comparison_key)
                .map_err(|_| ProviderError::new("invalid_secret", "Invalid comparison key"))?;
            mac.update(secret);
            let expected = mac.finalize().into_bytes();
            verify_hmac(comparison_key, received.as_bytes(), &expected)?;
        }
        SignatureScheme::GenericHmac => {
            let timestamp_text = header(headers, "X-RightWake-Timestamp")?;
            let timestamp = timestamp_text
                .parse::<i64>()
                .ok()
                .and_then(|x| x.checked_mul(1000))
                .ok_or_else(|| {
                    ProviderError::new("invalid_timestamp", "Invalid webhook timestamp")
                })?;
            let signature = header(headers, "X-RightWake-Signature")?
                .strip_prefix("sha256=")
                .ok_or_else(|| {
                    ProviderError::new("invalid_signature", "Expected sha256 signature prefix")
                })?;
            let mut message = format!("{timestamp_text}.").into_bytes();
            message.extend_from_slice(raw);
            verify_hmac(secret, &message, &decode_hex(signature)?)?;
            timestamp_recent(timestamp, now_ms, max_age_ms)?;
        }
    }
    Ok(Verification {
        scheme,
        verified_at_ms: now_ms,
        payload_sha256: sha256_hex(raw),
    })
}

impl SourceConfig {
    pub fn validate(&self) -> Result<()> {
        if self.id.is_empty() || self.id.len() > 128 || self.id.chars().any(char::is_control) {
            return Err(ProviderError::new(
                "invalid_source",
                "Source id must be a short non-empty identifier",
            ));
        }
        if self.secret_env.is_empty() || self.secret_env.contains('=') {
            return Err(ProviderError::new(
                "invalid_source",
                "A webhook secret environment-variable reference is required",
            ));
        }
        if self.max_payload_bytes == 0
            || self.max_payload_bytes > 16 * 1024 * 1024
            || self.max_content_chars == 0
            || self.max_content_chars > self.max_payload_bytes
        {
            return Err(ProviderError::new(
                "invalid_limits",
                "Payload/content limits must be bounded and non-zero (maximum payload 16 MiB)",
            ));
        }
        self.instance_origin()?;
        let compatible_signature = matches!(
            (self.kind, self.signature_scheme()),
            (ProviderKind::Linear, SignatureScheme::Linear)
                | (ProviderKind::Github, SignatureScheme::GitHub)
                | (
                    ProviderKind::Gitlab,
                    SignatureScheme::GitLabSigning | SignatureScheme::GitLabToken
                )
                | (ProviderKind::Generic, SignatureScheme::GenericHmac)
        );
        if !compatible_signature {
            return Err(ProviderError::new(
                "invalid_signature_mode",
                "Signature mode does not match the configured provider",
            ));
        }
        if let Some(base) = &self.api_base {
            validate_base_url(base)?;
        }
        if self
            .trigger_patterns
            .iter()
            .any(|x| x.is_empty() || x.len() > 1024)
            || self.trigger_patterns.len() > 64
        {
            return Err(ProviderError::new(
                "invalid_trigger",
                "Trigger patterns must be non-empty bounded literal strings",
            ));
        }
        if self.kind == ProviderKind::Generic {
            let mapping = self.generic.as_ref().ok_or_else(|| {
                ProviderError::new(
                    "missing_mapping",
                    "Generic sources require a JSON Pointer mapping",
                )
            })?;
            mapping.validate()?;
        }
        Ok(())
    }

    pub fn instance_origin(&self) -> Result<String> {
        let value = if self.instance.is_empty() {
            match self.kind {
                ProviderKind::Linear => "https://linear.app",
                ProviderKind::Github => "https://github.com",
                ProviderKind::Gitlab => "https://gitlab.com",
                ProviderKind::Generic => {
                    return Err(ProviderError::new(
                        "missing_instance",
                        "Generic sources require an explicit trusted instance URL",
                    ))
                }
            }
        } else {
            &self.instance
        };
        let parsed = validate_base_url(value)?;
        if parsed.path() != "/" && !parsed.path().is_empty() {
            return Err(ProviderError::new(
                "invalid_instance",
                "Instance must be an origin; use api_base for an API path prefix",
            ));
        }
        Ok(parsed.origin().ascii_serialization())
    }

    pub fn signature_scheme(&self) -> SignatureScheme {
        self.signature.unwrap_or(match self.kind {
            ProviderKind::Linear => SignatureScheme::Linear,
            ProviderKind::Github => SignatureScheme::GitHub,
            ProviderKind::Gitlab => SignatureScheme::GitLabSigning,
            ProviderKind::Generic => SignatureScheme::GenericHmac,
        })
    }

    pub fn api_url(&self) -> Result<url::Url> {
        if let Some(base) = &self.api_base {
            return validate_base_url(base);
        }
        let origin = self.instance_origin()?;
        let base = match self.kind {
            ProviderKind::Linear => "https://api.linear.app/graphql".to_owned(),
            ProviderKind::Github if origin == "https://github.com" => {
                "https://api.github.com".to_owned()
            }
            ProviderKind::Github => format!("{origin}/api/v3"),
            ProviderKind::Gitlab => format!("{origin}/api/v4"),
            ProviderKind::Generic => {
                return Err(ProviderError::new(
                    "unsupported_send",
                    "Generic Webhook sources do not have an implicit outbound writer",
                ))
            }
        };
        validate_base_url(&base)
    }
}

pub fn validate_base_url(value: &str) -> Result<url::Url> {
    let parsed = url::Url::parse(value)
        .map_err(|_| ProviderError::new("invalid_url", "Configured URL must be absolute"))?;
    let loopback = matches!(
        parsed.host_str(),
        Some("localhost" | "127.0.0.1" | "[::1]" | "::1")
    );
    if parsed.scheme() != "https" && !(parsed.scheme() == "http" && loopback) {
        return Err(ProviderError::new(
            "invalid_url",
            "Use HTTPS (HTTP is allowed only for explicit loopback development endpoints)",
        ));
    }
    if parsed.host_str().is_none()
        || !parsed.username().is_empty()
        || parsed.password().is_some()
        || parsed.query().is_some()
        || parsed.fragment().is_some()
    {
        return Err(ProviderError::new(
            "invalid_url",
            "Configured URL cannot contain credentials, query, or fragment",
        ));
    }
    Ok(parsed)
}

impl GenericMapping {
    fn validate(&self) -> Result<()> {
        if self.namespace.is_empty()
            || self.resource_kind.is_empty()
            || self.namespace.len() > 128
            || self.resource_kind.len() > 128
        {
            return Err(ProviderError::new(
                "invalid_mapping",
                "Generic namespace and resource_kind are required",
            ));
        }
        let pointers = [
            Some(&self.event_id),
            Some(&self.resource_id),
            Some(&self.content),
            self.tenant_id.as_ref(),
            self.conversation_id.as_ref(),
            self.actor_id.as_ref(),
            self.action.as_ref(),
            self.source_url.as_ref(),
            self.occurred_at.as_ref(),
            self.revision.as_ref(),
        ];
        for pointer in pointers.into_iter().flatten() {
            if !pointer.starts_with('/') || pointer.len() > 512 {
                return Err(ProviderError::new(
                    "invalid_mapping",
                    "Mapping fields must be bounded absolute JSON Pointers",
                ));
            }
            // JSON Pointer accepts only ~0 and ~1 escapes. Reject expression-like
            // paths rather than interpreting anything beyond RFC 6901 pointers.
            let mut bytes = pointer.bytes();
            while let Some(byte) = bytes.next() {
                if byte == b'~' && !matches!(bytes.next(), Some(b'0' | b'1')) {
                    return Err(ProviderError::new(
                        "invalid_mapping",
                        "Invalid JSON Pointer escape",
                    ));
                }
            }
        }
        Ok(())
    }
}

pub fn ingest(
    source: &SourceConfig,
    headers: &Headers,
    raw: &[u8],
    now_ms: i64,
) -> Result<IngestOutcome> {
    let secret = std::env::var(&source.secret_env).map_err(|_| {
        ProviderError::new(
            "missing_secret",
            format!("Set the secret referenced by {}", source.secret_env),
        )
    })?;
    ingest_with_secret(source, secret.as_bytes(), headers, raw, now_ms)
}

pub fn ingest_with_secret(
    source: &SourceConfig,
    secret: &[u8],
    headers: &Headers,
    raw: &[u8],
    now_ms: i64,
) -> Result<IngestOutcome> {
    source.validate()?;

    if raw.len() > source.max_payload_bytes {
        return Err(ProviderError::new(
            "payload_too_large",
            "Webhook body exceeds the configured limit",
        ));
    }
    let max_age = source
        .max_age_seconds
        .unwrap_or(if source.kind == ProviderKind::Linear {
            60
        } else {
            300
        });
    let max_age_ms = i64::try_from(max_age)
        .ok()
        .and_then(|x| x.checked_mul(1000))
        .ok_or_else(|| ProviderError::new("invalid_limits", "Timestamp tolerance is too large"))?;
    verify(
        source.signature_scheme(),
        secret,
        headers,
        raw,
        now_ms,
        max_age_ms,
    )?;
    let payload: Value = serde_json::from_slice(raw).map_err(|_| {
        ProviderError::new("invalid_json", "Authenticated payload is not valid JSON")
    })?;
    if !payload.is_object() {
        return Err(ProviderError::new(
            "invalid_payload",
            "Webhook payload must be an object",
        ));
    }
    let event = match source.kind {
        ProviderKind::Linear => normalize_linear(source, headers, &payload, now_ms)?,
        ProviderKind::Github => normalize_github(source, headers, &payload, now_ms)?,
        ProviderKind::Gitlab => normalize_gitlab(source, headers, &payload, now_ms)?,
        ProviderKind::Generic => Some(normalize_generic(source, headers, &payload, now_ms)?),
    };
    let Some(mut event) = event else {
        return Ok(IngestOutcome::Ignored {
            reason: "unsupported_or_auxiliary_event".to_owned(),
        });
    };
    apply_local_policy(source, &mut event)?;
    Ok(IngestOutcome::Event(event))
}

/// Reapply trusted local scope to a persisted, relay-authenticated event. This
/// never upgrades the relay's semantic flags and deliberately does not rerun a
/// webhook replay-time check after an offline interval.
pub fn apply_local_policy(source: &SourceConfig, event: &mut NormalizedEvent) -> Result<()> {
    source.validate()?;
    if event.source_id != source.id || event.provider != source.kind {
        return Err(ProviderError::new(
            "source_mismatch",
            "Normalized event does not belong to this configured source",
        ));
    }
    let origin = source.instance_origin()?;
    let prefix = match source.kind {
        ProviderKind::Linear => format!("linear:{origin}:{}:", component(&event.tenant_id)),
        ProviderKind::Github => format!("github:{origin}:repository:"),
        ProviderKind::Gitlab => format!("gitlab:{origin}:project:"),
        ProviderKind::Generic => {
            let mapping = source.generic.as_ref().ok_or_else(|| {
                ProviderError::new("missing_mapping", "Generic mapping is required")
            })?;
            format!(
                "generic:{}:{origin}:{}:{}:",
                component(&source.id),
                component(&mapping.namespace),
                component(&mapping.resource_kind)
            )
        }
    };
    if !event.resource_key.starts_with(&prefix) || event.resource_key.len() == prefix.len() {
        return Err(ProviderError::new(
            "instance_mismatch",
            "Normalized resource belongs to a different configured instance or namespace",
        ));
    }
    if event
        .conversation_key
        .as_ref()
        .is_some_and(|key| !key.starts_with(&format!("{}:", event.resource_key)))
    {
        return Err(ProviderError::new(
            "conversation_scope_mismatch",
            "Conversation does not belong to the normalized resource",
        ));
    }
    if event
        .reply_target
        .as_ref()
        .is_some_and(|target| target.provider != source.kind)
    {
        return Err(ProviderError::new(
            "reply_provider_mismatch",
            "Reply target belongs to a different provider",
        ));
    }
    if let Some(expected) = &source.tenant_id {
        if event.tenant_id != *expected {
            return Err(ProviderError::new(
                "tenant_mismatch",
                "Event belongs to a different configured tenant",
            ));
        }
    }
    if source.bot_actor_ids.contains(&event.actor_id) {
        event.actionable = false;
        event.explicit_trigger = false;
        event.skip_reason = Some("self_actor".to_owned());
    } else if !source.allowed_resources.is_empty()
        && !source.allowed_resources.iter().any(|pattern| {
            pattern.strip_suffix('*').map_or_else(
                || pattern == &event.resource_key,
                |prefix| event.resource_key.starts_with(prefix),
            )
        })
    {
        event.actionable = false;
        event.explicit_trigger = false;
        event.skip_reason = Some("outside_scope".to_owned());
    }
    if event.content.chars().count() > source.max_content_chars {
        event.content = event
            .content
            .chars()
            .take(source.max_content_chars)
            .collect();
        event
            .content
            .push_str("\n[Content truncated; inspect the source before making factual claims.]");
    }
    event.actionable &= !event.content.trim().is_empty();
    event.explicit_trigger &= event.actionable && trigger_matches(source, &event.content);
    Ok(())
}

fn scalar(value: &Value) -> Option<String> {
    match value {
        Value::String(s) => Some(s.clone()),
        Value::Number(n) => Some(n.to_string()),
        _ => None,
    }
}

fn at(value: &Value, pointer: &str) -> Option<String> {
    value.pointer(pointer).and_then(scalar)
}

fn required(value: &Value, pointer: &str) -> Result<String> {
    let text = at(value, pointer)
        .filter(|x| !x.is_empty() && x.len() <= 1024 && !x.chars().any(char::is_control))
        .ok_or_else(|| {
            ProviderError::new(
                "missing_identity",
                format!("Required identity field {pointer} is missing or invalid"),
            )
        })?;
    Ok(text)
}

fn component(value: &str) -> String {
    url::form_urlencoded::byte_serialize(value.as_bytes()).collect()
}

fn revision(value: &Value) -> String {
    // serde_json's default map representation has stable key ordering.
    sha256_hex(value.to_string().as_bytes())
}

fn occurred(value: Option<&Value>, fallback: i64) -> i64 {
    let Some(value) = value else {
        return fallback;
    };
    parse_timestamp(value).unwrap_or(fallback)
}

fn trigger_matches(source: &SourceConfig, content: &str) -> bool {
    if content.trim().is_empty() {
        return false;
    }
    if source.trigger_patterns.is_empty() {
        return true;
    }
    let content = content.to_lowercase();
    source.trigger_patterns.iter().any(|pattern| {
        let pattern = pattern.to_lowercase();
        content.match_indices(&pattern).any(|(start, _)| {
            if !pattern.starts_with('@') {
                return true;
            }
            let end = start + pattern.len();
            let username = |c: char| c.is_ascii_alphanumeric() || c == '_' || c == '-';
            let before_ok = content[..start]
                .chars()
                .next_back()
                .is_none_or(|c| !username(c));
            let after_ok = content[end..].chars().next().is_none_or(|c| !username(c));
            before_ok && after_ok
        })
    })
}

#[allow(clippy::too_many_arguments)]
fn make_event(
    source: &SourceConfig,
    delivery_id: String,
    tenant_id: String,
    resource_key: String,
    conversation_key: Option<String>,
    actor_id: String,
    action: String,
    entity_id: &str,
    occurred_at_ms: i64,
    now_ms: i64,
    source_url: Option<String>,
    content: String,
    revision: String,
    eligible: bool,
    reply_target: Option<ReplyTarget>,
) -> NormalizedEvent {
    let actionable = eligible && !content.trim().is_empty();
    let explicit_trigger = actionable && trigger_matches(source, &content);
    let skip_reason = if !actionable {
        Some("state_sync_only".to_owned())
    } else if !explicit_trigger {
        Some("no_explicit_trigger".to_owned())
    } else {
        None
    };
    // Structured boundaries prevent an untrusted action/entity containing '|'
    // from colliding with a different tuple before hashing.
    let event_id = sha256_hex(
        serde_json::json!([
            source.id,
            resource_key,
            conversation_key,
            action,
            entity_id,
            revision
        ])
        .to_string()
        .as_bytes(),
    );
    NormalizedEvent {
        event_id,
        delivery_id,
        source_id: source.id.clone(),
        provider: source.kind,
        tenant_id,
        resource_key,
        conversation_key,
        actor_id,
        action,
        occurred_at_ms,
        received_at_ms: now_ms,
        source_url,
        content,
        revision,
        actionable,
        explicit_trigger,
        skip_reason,
        reply_target,
    }
}

fn normalize_linear(
    source: &SourceConfig,
    headers: &Headers,
    payload: &Value,
    now_ms: i64,
) -> Result<Option<NormalizedEvent>> {
    let kind = at(payload, "/type").unwrap_or_default();
    if !matches!(kind.as_str(), "Issue" | "Comment") {
        return Ok(None);
    }
    let action = required(payload, "/action")?;
    let delivery = header(headers, "Linear-Delivery")?.to_owned();
    let tenant = required(payload, "/organizationId")?;
    let entity_id = required(payload, "/data/id")?;
    let issue_id = if kind == "Comment" {
        required(payload, "/data/issueId")?
    } else {
        entity_id.clone()
    };
    let resource = format!(
        "linear:{}:{}:{}",
        source.instance_origin()?,
        component(&tenant),
        component(&issue_id)
    );
    let content = if kind == "Comment" {
        at(payload, "/data/body").unwrap_or_default()
    } else {
        at(payload, "/data/description").unwrap_or_default()
    };
    let comment_id = if kind == "Comment" {
        Some(at(payload, "/data/parentId").unwrap_or_else(|| entity_id.clone()))
    } else {
        None
    };
    let conversation = comment_id
        .as_ref()
        .map(|id| format!("{resource}:comment:{}", component(id)));
    let eligible = if kind == "Comment" {
        matches!(action.as_str(), "create" | "update")
    } else {
        action == "create"
            || (action == "update" && payload.pointer("/updatedFrom/description").is_some())
    };
    let actor = at(payload, "/actor/id")
        .or_else(|| at(payload, "/data/userId"))
        .unwrap_or_else(|| "unknown".to_owned());
    let target = ReplyTarget {
        provider: source.kind,
        container_id: tenant.clone(),
        resource_id: issue_id,
        comment_id,
        discussion_id: None,
    };
    let data = payload.get("data").unwrap_or(&Value::Null);
    let updated = data.get("updatedAt").or_else(|| data.get("createdAt"));
    let snapshot = source_revision(
        &format!("linear.{}", kind.to_lowercase()),
        &entity_id,
        &content,
        updated,
        if kind == "Issue" { updated } else { None },
    );
    Ok(Some(make_event(
        source,
        delivery,
        tenant,
        resource,
        conversation,
        actor,
        format!("{}.{}", kind.to_lowercase(), action),
        &entity_id,
        occurred(payload.get("createdAt"), now_ms),
        now_ms,
        at(payload, "/url"),
        content,
        snapshot,
        eligible,
        Some(target),
    )))
}

fn normalize_github(
    source: &SourceConfig,
    headers: &Headers,
    payload: &Value,
    now_ms: i64,
) -> Result<Option<NormalizedEvent>> {
    let kind = header(headers, "X-GitHub-Event")?;
    if !matches!(
        kind,
        "issues"
            | "issue_comment"
            | "pull_request"
            | "pull_request_review"
            | "pull_request_review_comment"
            | "pull_request_review_thread"
    ) {
        return Ok(None);
    }
    let delivery = header(headers, "X-GitHub-Delivery")?.to_owned();
    let repository_id = required(payload, "/repository/id")?;
    let repository = required(payload, "/repository/full_name")?;
    let tenant = at(payload, "/repository/owner/id").unwrap_or_else(|| repository_id.clone());
    let action = at(payload, "/action").unwrap_or_else(|| "updated".to_owned());
    let is_pr =
        kind.starts_with("pull_request") || payload.pointer("/issue/pull_request").is_some();
    let resource_kind = if is_pr { "pr" } else { "issue" };
    let number = if kind.starts_with("pull_request") {
        required(payload, "/pull_request/number")?
    } else {
        required(payload, "/issue/number")?
    };
    if number.parse::<u64>().ok().filter(|x| *x > 0).is_none() {
        return Err(ProviderError::new(
            "invalid_identity",
            "Resource number must be a positive integer",
        ));
    }
    let resource = format!(
        "github:{}:repository:{}:{}:{}",
        source.instance_origin()?,
        component(&repository_id),
        resource_kind,
        number
    );
    let entity_pointer = match kind {
        "issue_comment" | "pull_request_review_comment" => "/comment",
        "pull_request_review" => "/review",
        "pull_request_review_thread" => "/thread",
        "issues" => "/issue",
        _ => "/pull_request",
    };
    let entity = payload
        .pointer(entity_pointer)
        .ok_or_else(|| ProviderError::new("missing_identity", "Event entity is missing"))?;
    let entity_id = required(entity, "/id")?;
    let content = at(entity, "/body").unwrap_or_default();
    let inline = kind == "pull_request_review_comment";
    let comment_id = if inline {
        Some(at(entity, "/in_reply_to_id").unwrap_or_else(|| entity_id.clone()))
    } else {
        None
    };
    let conversation = comment_id
        .as_ref()
        .map(|id| format!("{resource}:review:{}", component(id)));
    let eligible = match kind {
        "issue_comment" | "pull_request_review_comment" => {
            matches!(action.as_str(), "created" | "edited")
        }
        "pull_request_review" => matches!(action.as_str(), "submitted" | "edited"),
        "issues" | "pull_request" => {
            action == "opened" || (action == "edited" && payload.pointer("/changes/body").is_some())
        }
        _ => false,
    };
    // A review summary cannot be replied to with the inline-comment API. Keep a
    // draft without a fabricated target instead of silently posting at top level.
    let target = if kind == "pull_request_review" || kind == "pull_request_review_thread" {
        None
    } else {
        Some(ReplyTarget {
            provider: source.kind,
            container_id: repository,
            resource_id: number,
            comment_id,
            discussion_id: None,
        })
    };
    let actor = at(payload, "/sender/id")
        .or_else(|| at(entity, "/user/id"))
        .unwrap_or_else(|| "unknown".to_owned());
    let updated = entity
        .get("updated_at")
        .or_else(|| entity.get("submitted_at"))
        .or_else(|| entity.get("created_at"));
    let parent_updated = payload.pointer(if kind.starts_with("pull_request") {
        "/pull_request/updated_at"
    } else {
        "/issue/updated_at"
    });
    let snapshot = source_revision(
        &format!("github.{kind}"),
        &entity_id,
        &content,
        updated,
        parent_updated,
    );
    Ok(Some(make_event(
        source,
        delivery,
        tenant,
        resource,
        conversation,
        actor,
        format!("{kind}.{action}"),
        &entity_id,
        occurred(updated, now_ms),
        now_ms,
        at(entity, "/html_url"),
        content,
        snapshot,
        eligible,
        target,
    )))
}

fn normalize_gitlab(
    source: &SourceConfig,
    headers: &Headers,
    payload: &Value,
    now_ms: i64,
) -> Result<Option<NormalizedEvent>> {
    let kind = at(payload, "/object_kind").unwrap_or_default();
    if !matches!(
        kind.as_str(),
        "issue" | "merge_request" | "note" | "pipeline"
    ) {
        return Ok(None);
    }
    let project_id = required(payload, "/project/id")?;
    let tenant = at(payload, "/project/namespace_id").unwrap_or_else(|| project_id.clone());
    let delivery = header(headers, "webhook-id")
        .or_else(|_| header(headers, "Idempotency-Key"))
        .map(str::to_owned)
        .unwrap_or_else(|_| format!("legacy:{}", revision(payload)));
    let entity = payload.get("object_attributes").ok_or_else(|| {
        ProviderError::new("missing_identity", "Event object_attributes are missing")
    })?;
    let entity_id = required(entity, "/id")?;
    let (resource_kind, iid) = match kind.as_str() {
        "issue" => ("issue", required(entity, "/iid")?),
        "merge_request" => ("mr", required(entity, "/iid")?),
        "note" => match at(entity, "/noteable_type").as_deref() {
            Some("Issue") => ("issue", required(payload, "/issue/iid")?),
            Some("MergeRequest") => ("mr", required(payload, "/merge_request/iid")?),
            _ => return Ok(None),
        },
        "pipeline" => match at(payload, "/merge_request/iid") {
            Some(iid) => ("mr", iid),
            None => return Ok(None),
        },
        _ => return Ok(None),
    };
    if iid.parse::<u64>().ok().filter(|x| *x > 0).is_none() {
        return Err(ProviderError::new(
            "invalid_identity",
            "Resource iid must be a positive integer",
        ));
    }
    let resource = format!(
        "gitlab:{}:project:{}:{}:{}",
        source.instance_origin()?,
        component(&project_id),
        resource_kind,
        iid
    );
    let action = at(entity, "/action").unwrap_or_else(|| {
        if kind == "note" {
            "create".to_owned()
        } else {
            "update".to_owned()
        }
    });
    let content = if kind == "note" {
        at(entity, "/note")
    } else {
        at(entity, "/description")
    }
    .unwrap_or_default();
    let discussion_id = if kind == "note" {
        at(entity, "/discussion_id")
    } else {
        None
    };
    let conversation = discussion_id
        .as_ref()
        .map(|id| format!("{resource}:discussion:{}", component(id)));
    let eligible = match kind.as_str() {
        "note" => {
            matches!(action.as_str(), "create" | "update")
                && entity.get("system").and_then(Value::as_bool) != Some(true)
        }
        "issue" | "merge_request" => {
            matches!(action.as_str(), "open" | "reopen")
                || (action == "update" && payload.pointer("/changes/description").is_some())
        }
        _ => false,
    };
    // An inline note without a discussion id cannot be accurately replied to.
    let unknown_inline = kind == "note"
        && at(entity, "/type").as_deref() == Some("DiffNote")
        && discussion_id.is_none();
    let target = if unknown_inline {
        None
    } else {
        Some(ReplyTarget {
            provider: source.kind,
            container_id: project_id,
            resource_id: format!("{resource_kind}:{iid}"),
            comment_id: if kind == "note" {
                Some(entity_id.clone())
            } else {
                None
            },
            discussion_id,
        })
    };
    let actor = at(payload, "/user/id")
        .or_else(|| at(entity, "/author_id"))
        .unwrap_or_else(|| "unknown".to_owned());
    let updated = entity
        .get("updated_at")
        .or_else(|| entity.get("created_at"));
    let parent_updated = if kind == "note" {
        payload.pointer(if resource_kind == "mr" {
            "/merge_request/updated_at"
        } else {
            "/issue/updated_at"
        })
    } else {
        updated
    };
    let snapshot = source_revision(
        &format!("gitlab.{kind}"),
        &entity_id,
        &content,
        updated,
        parent_updated,
    );
    Ok(Some(make_event(
        source,
        delivery,
        tenant,
        resource,
        conversation,
        actor,
        format!("{kind}.{action}"),
        &entity_id,
        occurred(updated, now_ms),
        now_ms,
        at(entity, "/url"),
        content,
        snapshot,
        eligible,
        target,
    )))
}

fn normalize_generic(
    source: &SourceConfig,
    headers: &Headers,
    payload: &Value,
    now_ms: i64,
) -> Result<NormalizedEvent> {
    let mapping = source.generic.as_ref().ok_or_else(|| {
        ProviderError::new("missing_mapping", "Generic source mapping is missing")
    })?;
    let event_id = required(payload, &mapping.event_id)?;
    let resource_id = required(payload, &mapping.resource_id)?;
    let tenant = mapping
        .tenant_id
        .as_ref()
        .and_then(|p| at(payload, p))
        .or_else(|| source.tenant_id.clone())
        .unwrap_or_else(|| source.id.clone());
    let resource = format!(
        "generic:{}:{}:{}:{}:{}",
        component(&source.id),
        source.instance_origin()?,
        component(&mapping.namespace),
        component(&mapping.resource_kind),
        component(&resource_id)
    );
    let conversation = mapping
        .conversation_id
        .as_ref()
        .and_then(|p| at(payload, p))
        .map(|id| format!("{resource}:discussion:{}", component(&id)));
    let content = at(payload, &mapping.content).unwrap_or_default();
    let actor = mapping
        .actor_id
        .as_ref()
        .and_then(|p| at(payload, p))
        .unwrap_or_else(|| "unknown".to_owned());
    let action = mapping
        .action
        .as_ref()
        .and_then(|p| at(payload, p))
        .unwrap_or_else(|| "request".to_owned());
    let source_url = mapping.source_url.as_ref().and_then(|p| at(payload, p));
    let source_revision = mapping
        .revision
        .as_ref()
        .and_then(|p| at(payload, p))
        .unwrap_or_else(|| sha256_hex(content.as_bytes()));
    let delivery = header(headers, "X-RightWake-Delivery")
        .map(str::to_owned)
        .unwrap_or_else(|_| event_id.clone());
    let occurred_at_ms = occurred(
        mapping
            .occurred_at
            .as_ref()
            .and_then(|p| payload.pointer(p)),
        now_ms,
    );
    Ok(make_event(
        source,
        delivery,
        tenant,
        resource,
        conversation,
        actor,
        format!("generic.{action}"),
        &event_id,
        occurred_at_ms,
        now_ms,
        source_url,
        content,
        source_revision,
        true,
        None,
    ))
}
