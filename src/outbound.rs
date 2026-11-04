//! Outbound mutations require a separately signed, exact human-review statement.
//!
//! This module never creates signing keys, signs approvals, or retries writes.
//! The caller must durably claim the outbox action before calling `send_approved`
//! and persist `Unknown` without automatically resending it.

use crate::model::{Draft, ProviderKind, ReplyTarget};
use crate::provider::{parse_timestamp, sha256_hex, SourceConfig, SourceRevision};
use base64::{engine::general_purpose::STANDARD, Engine as _};
use ed25519_dalek::{Signature, VerifyingKey};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::time::Duration;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ApprovalStatement {
    pub schema_version: u32,
    pub draft_id: String,
    pub draft_digest: String,
    pub expires_at_ms: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ApprovalEnvelope {
    pub statement: ApprovalStatement,
    /// Ed25519 signature of `approval_signing_bytes(statement)`, standard base64.
    pub signature: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApprovedAction {
    pub draft: Draft,
    pub approval: ApprovalEnvelope,
}

/// Latest authoritative local state, fetched immediately before outbox claiming.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CurrentDraftState {
    pub draft: Draft,
    pub source_revision: String,
    pub binding_version: i64,
    pub binding_enabled: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum SendOutcome {
    Sent {
        external_id: String,
        url: Option<String>,
    },
    Unknown {
        reason: String,
    },
    Rejected {
        reason: String,
    },
    Failed {
        reason: String,
        retryable: bool,
    },
}

#[derive(Serialize)]
struct ReviewTarget<'a> {
    domain: &'static str,
    draft_id: &'a str,
    draft_revision: i64,
    source_id: &'a str,
    resource_key: &'a str,
    event_id: &'a str,
    body: &'a str,
    reply_target: &'a ReplyTarget,
    source_revision: &'a str,
    source_event_revision: &'a str,
    binding_key: &'a str,
    binding_version: i64,
    sending_identity: &'a str,
    configured_identity: &'a str,
    instance: String,
    api_url: String,
    token_reference: &'a Option<String>,
    credential_fingerprint: Option<String>,
    review_public_key: &'a Option<String>,
    configured_tenant: &'a Option<String>,
    allowed_resources: &'a [String],
}

/// Exact public review bytes. The independent reviewer can hash these bytes
/// and display this same target before signing; no API credential is exported.
pub fn approval_target_bytes(source: &SourceConfig, draft: &Draft) -> anyhow::Result<Vec<u8>> {
    let credential_fingerprint = source
        .token_env
        .as_ref()
        .map(|name| {
            std::env::var(name)
                .map(|token| sha256_hex(format!("right-wake.credential.v1\0{token}").as_bytes()))
                .map_err(|_| anyhow::anyhow!("api_credential_unavailable"))
        })
        .transpose()?;
    let value = ReviewTarget {
        domain: "right-wake.review.v1",
        draft_id: &draft.id,
        draft_revision: draft.revision,
        source_id: &draft.source_id,
        resource_key: &draft.resource_key,
        event_id: &draft.event_id,
        body: &draft.body,
        reply_target: &draft.reply_target,
        source_revision: &draft.source_revision,
        source_event_revision: &draft.source_event_revision,
        binding_key: &draft.binding_key,
        binding_version: draft.binding_version,
        sending_identity: &draft.sending_identity,
        configured_identity: &source.sending_identity,
        instance: source.instance_origin()?,
        api_url: source.api_url()?.to_string(),
        token_reference: &source.token_env,
        credential_fingerprint,
        review_public_key: &source.review_public_key,
        configured_tenant: &source.tenant_id,
        allowed_resources: &source.allowed_resources,
    };
    Ok(serde_json::to_vec(&value)?)
}

pub fn approval_digest(source: &SourceConfig, draft: &Draft) -> anyhow::Result<String> {
    Ok(sha256_hex(&approval_target_bytes(source, draft)?))
}

/// Public review data only. Creating a statement does not approve an action.
pub fn prepare_approval_statement(
    source: &SourceConfig,
    draft: &Draft,
    expires_at_ms: i64,
) -> anyhow::Result<ApprovalStatement> {
    Ok(ApprovalStatement {
        schema_version: 1,
        draft_id: draft.id.clone(),
        draft_digest: approval_digest(source, draft)?,
        expires_at_ms,
    })
}

/// The external reviewer signs these exact bytes; the domain prevents a valid
/// signature on unrelated application data from being accepted as an approval.
pub fn approval_signing_bytes(statement: &ApprovalStatement) -> anyhow::Result<Vec<u8>> {
    let mut bytes = b"right-wake.human-approval.v1\n".to_vec();
    bytes.extend_from_slice(&serde_json::to_vec(statement)?);
    Ok(bytes)
}

pub fn verify_approval(
    source: &SourceConfig,
    action: &ApprovedAction,
    current: &CurrentDraftState,
    now_ms: i64,
) -> anyhow::Result<()> {
    anyhow::ensure!(source.enabled, "source_disabled");
    anyhow::ensure!(
        source.kind == action.draft.reply_target.provider,
        "provider_mismatch"
    );
    anyhow::ensure!(source.id == action.draft.source_id, "source_mismatch");
    anyhow::ensure!(
        !source.sending_identity.is_empty()
            && source.sending_identity == action.draft.sending_identity,
        "sending_identity_changed_or_unconfigured"
    );
    anyhow::ensure!(!action.draft.body.trim().is_empty(), "empty_reply");
    anyhow::ensure!(action.draft.body.len() <= 256 * 1024, "reply_too_large");
    anyhow::ensure!(current.binding_enabled, "binding_paused");
    anyhow::ensure!(
        current.binding_version == action.draft.binding_version,
        "binding_changed"
    );
    anyhow::ensure!(
        current.source_revision == action.draft.source_revision,
        "source_changed"
    );
    anyhow::ensure!(
        matches!(current.draft.state.as_str(), "draft" | "sending"),
        "draft_not_sendable"
    );
    anyhow::ensure!(
        approval_digest(source, &current.draft)? == approval_digest(source, &action.draft)?,
        "draft_changed"
    );
    let statement = &action.approval.statement;
    anyhow::ensure!(
        statement.schema_version == 1,
        "unsupported_approval_version"
    );
    anyhow::ensure!(
        statement.draft_id == action.draft.id,
        "approval_draft_mismatch"
    );
    anyhow::ensure!(
        statement.draft_digest == approval_digest(source, &action.draft)?,
        "approval_content_mismatch"
    );
    anyhow::ensure!(statement.expires_at_ms > now_ms, "approval_expired");
    anyhow::ensure!(
        statement.expires_at_ms.saturating_sub(now_ms) <= 24 * 60 * 60 * 1000,
        "approval_expiry_too_far"
    );
    let encoded_key = source
        .review_public_key
        .as_ref()
        .ok_or_else(|| anyhow::anyhow!("review_public_key_not_configured"))?;
    let key_bytes = STANDARD
        .decode(encoded_key)
        .map_err(|_| anyhow::anyhow!("invalid_review_public_key"))?;
    let key_array: [u8; 32] = key_bytes
        .try_into()
        .map_err(|_| anyhow::anyhow!("invalid_review_public_key"))?;
    let key = VerifyingKey::from_bytes(&key_array)
        .map_err(|_| anyhow::anyhow!("invalid_review_public_key"))?;
    let signature_bytes = STANDARD
        .decode(&action.approval.signature)
        .map_err(|_| anyhow::anyhow!("invalid_approval_signature"))?;
    let signature = Signature::from_slice(&signature_bytes)
        .map_err(|_| anyhow::anyhow!("invalid_approval_signature"))?;
    key.verify_strict(&approval_signing_bytes(statement)?, &signature)
        .map_err(|_| anyhow::anyhow!("invalid_approval_signature"))?;
    Ok(())
}

pub async fn send_approved(
    source: &SourceConfig,
    action: &ApprovedAction,
    current: &CurrentDraftState,
    now_ms: i64,
) -> SendOutcome {
    send_approved_guarded(source, action, current, now_ms, || {
        Ok((source.clone(), current.clone()))
    })
    .await
}

/// Production entry point. Reload local state after the network preflight, then
/// verify the approval immediately before POST. No SQLite transaction is held
/// across network I/O. The provider's remote read/create race cannot be made
/// atomic without provider support for conditional creates.
pub async fn send_approved_guarded<F>(
    source: &SourceConfig,
    action: &ApprovedAction,
    current: &CurrentDraftState,
    now_ms: i64,
    refresh: F,
) -> SendOutcome
where
    F: Fn() -> anyhow::Result<(SourceConfig, CurrentDraftState)>,
{
    if let Err(error) = verify_approval(source, action, current, now_ms) {
        return SendOutcome::Rejected {
            reason: error.to_string(),
        };
    }
    let Some(token_reference) = &source.token_env else {
        return SendOutcome::Rejected {
            reason: "api_credential_not_configured".to_owned(),
        };
    };
    let Ok(token) = std::env::var(token_reference) else {
        return SendOutcome::Rejected {
            reason: "api_credential_unavailable".to_owned(),
        };
    };
    if token.trim().is_empty() {
        return SendOutcome::Rejected {
            reason: "api_credential_unavailable".to_owned(),
        };
    }
    let client = match reqwest::Client::builder()
        .connect_timeout(Duration::from_secs(5))
        .timeout(Duration::from_secs(20))
        .redirect(reqwest::redirect::Policy::none())
        .user_agent(concat!("RightWake/", env!("CARGO_PKG_VERSION")))
        .build()
    {
        Ok(client) => client,
        Err(_) => {
            return SendOutcome::Failed {
                reason: "http_client_unavailable".to_owned(),
                retryable: false,
            }
        }
    };
    if let Err(reason) = preflight_remote(source, &action.draft, &client, &token).await {
        return SendOutcome::Rejected { reason };
    }
    let (fresh_source, fresh_current) = match refresh() {
        Ok(state) => state,
        Err(_) => {
            return SendOutcome::Rejected {
                reason: "local_state_refresh_failed".to_owned(),
            }
        }
    };
    if let Err(error) = verify_approval(
        &fresh_source,
        action,
        &fresh_current,
        crate::model::now_ms().max(now_ms),
    ) {
        return SendOutcome::Rejected {
            reason: error.to_string(),
        };
    }
    send_with_client(&fresh_source, action, &client, &token).await
}

fn path_url(base: &url::Url, segments: &[&str]) -> std::result::Result<url::Url, String> {
    let mut url = base.clone();
    url.path_segments_mut()
        .map_err(|_| "invalid_api_configuration".to_owned())?
        .pop_if_empty()
        .extend(segments.iter().copied());
    Ok(url)
}

async fn bounded_json(response: reqwest::Response) -> std::result::Result<Value, String> {
    if !response.status().is_success() {
        return Err("remote_preflight_failed".to_owned());
    }
    if response
        .content_length()
        .is_some_and(|length| length > 2 * 1024 * 1024)
    {
        return Err("remote_preflight_too_large".to_owned());
    }
    let mut response = response;
    let mut bytes = Vec::new();
    loop {
        match response.chunk().await {
            Ok(Some(chunk)) => {
                if bytes.len() + chunk.len() > 2 * 1024 * 1024 {
                    return Err("remote_preflight_too_large".to_owned());
                }
                bytes.extend_from_slice(&chunk);
            }
            Ok(None) => break,
            Err(_) => return Err("remote_preflight_incomplete".to_owned()),
        }
    }
    serde_json::from_slice(&bytes).map_err(|_| "remote_preflight_invalid_json".to_owned())
}

fn id(value: &Value) -> Option<String> {
    value.get("id").and_then(|value| match value {
        Value::String(text) => Some(text.clone()),
        Value::Number(number) => Some(number.to_string()),
        _ => None,
    })
}

fn validate_remote_entity(
    entity: &Value,
    snapshot: &SourceRevision,
    text_field: &str,
    timestamp_field: &str,
) -> std::result::Result<(), String> {
    if id(entity).as_deref() != Some(&snapshot.entity_id) {
        return Err("source_entity_changed".to_owned());
    }
    let text = entity.get(text_field).and_then(Value::as_str).unwrap_or("");
    if sha256_hex(text.as_bytes()) != snapshot.text_sha256 {
        return Err("source_body_changed".to_owned());
    }
    let updated = entity.get(timestamp_field).and_then(parse_timestamp);
    if updated.is_none() || updated != snapshot.updated_at_ms {
        return Err("source_revision_changed_or_unverifiable".to_owned());
    }
    Ok(())
}

fn new_comment_exists(
    comments: &[Value],
    snapshot: &SourceRevision,
    timestamp_field: &str,
    thread_root: Option<&str>,
    linear: bool,
) -> std::result::Result<bool, String> {
    let cutoff = snapshot
        .updated_at_ms
        .ok_or_else(|| "source_timestamp_unavailable".to_owned())?;
    for comment in comments {
        let comment_id =
            id(comment).ok_or_else(|| "remote_comment_identity_unavailable".to_owned())?;
        if comment_id == snapshot.entity_id
            && matches!(
                snapshot.entity_kind.as_str(),
                "linear.comment"
                    | "github.issue_comment"
                    | "github.pull_request_review_comment"
                    | "gitlab.note"
            )
        {
            continue;
        }
        if let Some(root) = thread_root {
            let parent = if linear {
                comment
                    .pointer("/parent/id")
                    .and_then(Value::as_str)
                    .map(str::to_owned)
            } else {
                comment.get("in_reply_to_id").and_then(|value| {
                    value
                        .as_u64()
                        .map(|x| x.to_string())
                        .or_else(|| value.as_str().map(str::to_owned))
                })
            };
            if comment_id != root && parent.as_deref() != Some(root) {
                continue;
            }
        }
        let updated = comment
            .get(timestamp_field)
            .and_then(parse_timestamp)
            .ok_or_else(|| "remote_comment_timestamp_unavailable".to_owned())?;
        // Equal-resolution timestamps are ambiguous; require a fresh review.
        if updated >= cutoff {
            return Ok(true);
        }
    }
    Ok(false)
}

/// Read-only, source-specific freshness verification. It happens only when a
/// signed action is submitted, never on a timer. Any incomplete read is a stop.
pub async fn preflight_remote(
    source: &SourceConfig,
    draft: &Draft,
    client: &reqwest::Client,
    token: &str,
) -> std::result::Result<(), String> {
    let snapshot: SourceRevision = serde_json::from_str(&draft.source_event_revision)
        .map_err(|_| "source_revision_provenance_unavailable".to_owned())?;
    if snapshot.format_version != 1 || snapshot.updated_at_ms.is_none() {
        return Err("source_revision_provenance_unavailable".to_owned());
    }
    let base = source
        .api_url()
        .map_err(|_| "invalid_api_configuration".to_owned())?;
    verify_remote_identity(source, &base, client, token).await?;
    let target = &draft.reply_target;
    match source.kind {
        ProviderKind::Github => {
            let parts: Vec<_> = target.container_id.split('/').collect();
            if parts.len() != 2
                || parts
                    .iter()
                    .any(|part| part.is_empty() || *part == "." || *part == "..")
            {
                return Err("invalid_github_reply_target".to_owned());
            }
            let prefix = ["repos", parts[0], parts[1]];
            let mut entity_path = prefix.to_vec();
            match snapshot.entity_kind.as_str() {
                "github.issue_comment" => {
                    entity_path.extend(["issues", "comments", &snapshot.entity_id])
                }
                "github.pull_request_review_comment" => {
                    entity_path.extend(["pulls", "comments", &snapshot.entity_id])
                }
                "github.issues" => entity_path.extend(["issues", &target.resource_id]),
                "github.pull_request" => entity_path.extend(["pulls", &target.resource_id]),
                _ => return Err("unsupported_remote_preflight".to_owned()),
            }
            let get = |url: url::Url| {
                client
                    .get(url)
                    .bearer_auth(token)
                    .header(reqwest::header::ACCEPT, "application/vnd.github+json")
                    .header("X-GitHub-Api-Version", "2022-11-28")
            };
            let entity = bounded_json(
                get(path_url(&base, &entity_path)?)
                    .send()
                    .await
                    .map_err(|_| "remote_preflight_unreachable".to_owned())?,
            )
            .await?;
            validate_remote_entity(&entity, &snapshot, "body", "updated_at")?;
            let mut parent_path = prefix.to_vec();
            parent_path.extend(["issues", &target.resource_id]);
            let parent = bounded_json(
                get(path_url(&base, &parent_path)?)
                    .send()
                    .await
                    .map_err(|_| "remote_preflight_unreachable".to_owned())?,
            )
            .await?;
            if parent.get("locked").and_then(Value::as_bool) == Some(true) {
                return Err("reply_target_locked".to_owned());
            }
            if let Some(baseline) = snapshot.resource_updated_at_ms {
                let actual = parent
                    .get("updated_at")
                    .and_then(parse_timestamp)
                    .ok_or_else(|| "resource_revision_unverifiable".to_owned())?;
                if actual != baseline {
                    return Err("resource_changed_since_draft".to_owned());
                }
            }
            let mut comments_path = prefix.to_vec();
            comments_path.extend([
                if target.comment_id.is_some() {
                    "pulls"
                } else {
                    "issues"
                },
                &target.resource_id,
                "comments",
            ]);
            let mut comments_url = path_url(&base, &comments_path)?;
            let cutoff = chrono::DateTime::<chrono::Utc>::from_timestamp_millis(
                snapshot.updated_at_ms.unwrap(),
            )
            .ok_or_else(|| "source_timestamp_unavailable".to_owned())?
            .to_rfc3339();
            comments_url
                .query_pairs_mut()
                .append_pair("since", &cutoff)
                .append_pair("per_page", "100");
            let response = get(comments_url)
                .send()
                .await
                .map_err(|_| "remote_preflight_unreachable".to_owned())?;
            if response
                .headers()
                .get(reqwest::header::LINK)
                .and_then(|x| x.to_str().ok())
                .is_some_and(|x| x.contains("rel=\"next\""))
            {
                return Err("remote_discussion_requires_more_pages".to_owned());
            }
            let comments = bounded_json(response).await?;
            let comments = comments
                .as_array()
                .ok_or_else(|| "remote_discussion_unverifiable".to_owned())?;
            if new_comment_exists(
                comments,
                &snapshot,
                "updated_at",
                target.comment_id.as_deref(),
                false,
            )? {
                return Err("new_discussion_activity_review_again".to_owned());
            }
        }
        ProviderKind::Gitlab => {
            let Some((kind, iid)) = target.resource_id.split_once(':') else {
                return Err("invalid_gitlab_reply_target".to_owned());
            };
            let segment = match kind {
                "issue" => "issues",
                "mr" => "merge_requests",
                _ => return Err("invalid_gitlab_reply_target".to_owned()),
            };
            let parent_path = ["projects", &target.container_id, segment, iid];
            let mut entity_path = parent_path.to_vec();
            if snapshot.entity_kind == "gitlab.note" {
                entity_path.extend(["notes", &snapshot.entity_id]);
            } else if !matches!(
                snapshot.entity_kind.as_str(),
                "gitlab.issue" | "gitlab.merge_request"
            ) {
                return Err("unsupported_remote_preflight".to_owned());
            }
            let get = |url: url::Url| client.get(url).header("PRIVATE-TOKEN", token);
            let entity = bounded_json(
                get(path_url(&base, &entity_path)?)
                    .send()
                    .await
                    .map_err(|_| "remote_preflight_unreachable".to_owned())?,
            )
            .await?;
            validate_remote_entity(
                &entity,
                &snapshot,
                if snapshot.entity_kind == "gitlab.note" {
                    "body"
                } else {
                    "description"
                },
                "updated_at",
            )?;
            let parent = bounded_json(
                get(path_url(&base, &parent_path)?)
                    .send()
                    .await
                    .map_err(|_| "remote_preflight_unreachable".to_owned())?,
            )
            .await?;
            if parent.get("discussion_locked").and_then(Value::as_bool) == Some(true) {
                return Err("reply_target_locked".to_owned());
            }
            if let Some(baseline) = snapshot.resource_updated_at_ms {
                if parent.get("updated_at").and_then(parse_timestamp) != Some(baseline) {
                    return Err("resource_changed_since_draft".to_owned());
                }
            }
            let mut comments_path = parent_path.to_vec();
            let comments = if let Some(discussion) = &target.discussion_id {
                comments_path.extend(["discussions", discussion]);
                let response = bounded_json(
                    get(path_url(&base, &comments_path)?)
                        .send()
                        .await
                        .map_err(|_| "remote_preflight_unreachable".to_owned())?,
                )
                .await?;
                response
                    .get("notes")
                    .and_then(Value::as_array)
                    .cloned()
                    .ok_or_else(|| "remote_discussion_unverifiable".to_owned())?
            } else {
                if target.comment_id.is_some() {
                    return Err("discussion_location_unavailable".to_owned());
                }
                comments_path.push("notes");
                let mut url = path_url(&base, &comments_path)?;
                url.query_pairs_mut()
                    .append_pair("order_by", "updated_at")
                    .append_pair("sort", "desc")
                    .append_pair("per_page", "100");
                let response = get(url)
                    .send()
                    .await
                    .map_err(|_| "remote_preflight_unreachable".to_owned())?;
                if response
                    .headers()
                    .get("X-Next-Page")
                    .and_then(|x| x.to_str().ok())
                    .is_some_and(|x| !x.is_empty())
                {
                    return Err("remote_discussion_requires_more_pages".to_owned());
                }
                bounded_json(response)
                    .await?
                    .as_array()
                    .cloned()
                    .ok_or_else(|| "remote_discussion_unverifiable".to_owned())?
            };
            if new_comment_exists(&comments, &snapshot, "updated_at", None, false)? {
                return Err("new_discussion_activity_review_again".to_owned());
            }
        }
        ProviderKind::Linear => {
            let is_comment = snapshot.entity_kind == "linear.comment";
            if !is_comment && snapshot.entity_kind != "linear.issue" {
                return Err("unsupported_remote_preflight".to_owned());
            }
            let query = if is_comment {
                "query RightWakePreflight($id: String!, $issueId: String!) { comment(id: $id) { id body updatedAt issue { id } } issue(id: $issueId) { id updatedAt archivedAt comments(first: 100) { nodes { id updatedAt parent { id } } pageInfo { hasNextPage } } } }"
            } else {
                "query RightWakePreflight($id: String!, $issueId: String!) { source: issue(id: $id) { id description updatedAt } issue(id: $issueId) { id updatedAt archivedAt comments(first: 100) { nodes { id updatedAt parent { id } } pageInfo { hasNextPage } } } }"
            };
            let request = client.post(base).json(&json!({"query":query,"variables":{"id":snapshot.entity_id,"issueId":target.resource_id}}));
            let request = if token.starts_with("lin_api_") {
                request.header(reqwest::header::AUTHORIZATION, token)
            } else {
                request.bearer_auth(token)
            };
            let payload = bounded_json(
                request
                    .send()
                    .await
                    .map_err(|_| "remote_preflight_unreachable".to_owned())?,
            )
            .await?;
            if payload.get("errors").is_some() {
                return Err("linear_preflight_query_failed".to_owned());
            }
            let entity = payload
                .pointer(if is_comment {
                    "/data/comment"
                } else {
                    "/data/source"
                })
                .ok_or_else(|| "source_entity_unavailable".to_owned())?;
            validate_remote_entity(
                entity,
                &snapshot,
                if is_comment { "body" } else { "description" },
                "updatedAt",
            )?;
            if is_comment
                && entity.pointer("/issue/id").and_then(Value::as_str) != Some(&target.resource_id)
            {
                return Err("source_comment_parent_changed".to_owned());
            }
            let issue = payload
                .pointer("/data/issue")
                .filter(|x| !x.is_null())
                .ok_or_else(|| "source_issue_unavailable".to_owned())?;
            if id(issue).as_deref() != Some(&target.resource_id)
                || issue
                    .get("archivedAt")
                    .is_some_and(|value| !value.is_null())
            {
                return Err("reply_target_changed_or_archived".to_owned());
            }
            if let Some(baseline) = snapshot.resource_updated_at_ms {
                if issue.get("updatedAt").and_then(parse_timestamp) != Some(baseline) {
                    return Err("resource_changed_since_draft".to_owned());
                }
            }
            if issue
                .pointer("/comments/pageInfo/hasNextPage")
                .and_then(Value::as_bool)
                != Some(false)
            {
                return Err("remote_discussion_requires_more_pages".to_owned());
            }
            let comments = issue
                .pointer("/comments/nodes")
                .and_then(Value::as_array)
                .ok_or_else(|| "remote_discussion_unverifiable".to_owned())?;
            if new_comment_exists(
                comments,
                &snapshot,
                "updatedAt",
                target.comment_id.as_deref(),
                true,
            )? {
                return Err("new_discussion_activity_review_again".to_owned());
            }
        }
        ProviderKind::Generic => return Err("generic_writer_not_configured".to_owned()),
    }
    Ok(())
}

async fn verify_remote_identity(
    source: &SourceConfig,
    base: &url::Url,
    client: &reqwest::Client,
    token: &str,
) -> std::result::Result<(), String> {
    let response = match source.kind {
        ProviderKind::Linear => {
            let request = client
                .post(base.clone())
                .json(&json!({"query":"query RightWakeIdentity { viewer { id } }"}));
            let request = if token.starts_with("lin_api_") {
                request.header(reqwest::header::AUTHORIZATION, token)
            } else {
                request.bearer_auth(token)
            };
            request.send().await
        }
        ProviderKind::Github => {
            client
                .get(path_url(base, &["user"])?)
                .bearer_auth(token)
                .header(reqwest::header::ACCEPT, "application/vnd.github+json")
                .header("X-GitHub-Api-Version", "2022-11-28")
                .send()
                .await
        }
        ProviderKind::Gitlab => {
            client
                .get(path_url(base, &["user"])?)
                .header("PRIVATE-TOKEN", token)
                .send()
                .await
        }
        ProviderKind::Generic => return Err("generic_writer_not_configured".to_owned()),
    }
    .map_err(|_| "sending_identity_unverifiable".to_owned())?;
    let payload = bounded_json(response)
        .await
        .map_err(|_| "sending_identity_unverifiable".to_owned())?;
    let entity = if source.kind == ProviderKind::Linear {
        payload
            .pointer("/data/viewer")
            .ok_or_else(|| "sending_identity_unverifiable".to_owned())?
    } else {
        &payload
    };
    let actual_id = id(entity).ok_or_else(|| "sending_identity_unverifiable".to_owned())?;
    if actual_id != source.sending_identity {
        return Err("sending_identity_mismatch".to_owned());
    }
    Ok(())
}

pub fn build_write_request(
    source: &SourceConfig,
    draft: &Draft,
    client: &reqwest::Client,
    token: &str,
) -> std::result::Result<reqwest::Request, String> {
    let target = &draft.reply_target;
    let mut url = match source.api_url() {
        Ok(url) => url,
        Err(_) => return Err("invalid_api_configuration".to_owned()),
    };
    let request = match source.kind {
        ProviderKind::Linear => {
            let mut input = json!({"issueId": target.resource_id, "body": draft.body});
            if let Some(parent) = &target.comment_id {
                input["parentId"] = json!(parent);
            }
            let body = json!({
                "query": "mutation RightWakeReply($input: CommentCreateInput!) { commentCreate(input: $input) { success comment { id url } } }",
                "variables": { "input": input }
            });
            let request = client.post(url).json(&body);
            if token.starts_with("lin_api_") {
                request.header(reqwest::header::AUTHORIZATION, token)
            } else {
                request.bearer_auth(token)
            }
        }
        ProviderKind::Github => {
            let parts: Vec<_> = target.container_id.split('/').collect();
            if parts.len() != 2
                || parts
                    .iter()
                    .any(|part| part.is_empty() || *part == "." || *part == "..")
                || target
                    .resource_id
                    .parse::<u64>()
                    .ok()
                    .filter(|id| *id > 0)
                    .is_none()
            {
                return Err("invalid_github_reply_target".to_owned());
            }
            {
                let Ok(mut path) = url.path_segments_mut() else {
                    return Err("invalid_api_configuration".to_owned());
                };
                path.pop_if_empty().extend(["repos", parts[0], parts[1]]);
                if let Some(comment_id) = &target.comment_id {
                    if comment_id
                        .parse::<u64>()
                        .ok()
                        .filter(|id| *id > 0)
                        .is_none()
                    {
                        return Err("invalid_review_comment_target".to_owned());
                    }
                    path.extend([
                        "pulls",
                        &target.resource_id,
                        "comments",
                        comment_id,
                        "replies",
                    ]);
                } else {
                    path.extend(["issues", &target.resource_id, "comments"]);
                }
            }
            client
                .post(url)
                .bearer_auth(token)
                .header(reqwest::header::ACCEPT, "application/vnd.github+json")
                .header("X-GitHub-Api-Version", "2022-11-28")
                .json(&json!({"body": draft.body}))
        }
        ProviderKind::Gitlab => {
            let Some((kind, iid)) = target.resource_id.split_once(':') else {
                return Err("invalid_gitlab_reply_target".to_owned());
            };
            let resource = match kind {
                "mr" => "merge_requests",
                "issue" => "issues",
                _ => return Err("invalid_gitlab_reply_target".to_owned()),
            };
            if target
                .container_id
                .parse::<u64>()
                .ok()
                .filter(|id| *id > 0)
                .is_none()
                || iid.parse::<u64>().ok().filter(|id| *id > 0).is_none()
            {
                return Err("invalid_gitlab_reply_target".to_owned());
            }
            if target.comment_id.is_some() && target.discussion_id.is_none() {
                return Err("discussion_location_unavailable".to_owned());
            }
            {
                let Ok(mut path) = url.path_segments_mut() else {
                    return Err("invalid_api_configuration".to_owned());
                };
                path.pop_if_empty()
                    .extend(["projects", &target.container_id, resource, iid]);
                if let Some(discussion) = &target.discussion_id {
                    path.extend(["discussions", discussion, "notes"]);
                } else {
                    path.push("notes");
                }
            }
            client
                .post(url)
                .header("PRIVATE-TOKEN", token)
                .json(&json!({"body": draft.body}))
        }
        ProviderKind::Generic => return Err("generic_writer_not_configured".to_owned()),
    };
    request
        .build()
        .map_err(|_| "invalid_outbound_request".to_owned())
}

async fn send_with_client(
    source: &SourceConfig,
    action: &ApprovedAction,
    client: &reqwest::Client,
    token: &str,
) -> SendOutcome {
    let request = match build_write_request(source, &action.draft, client, token) {
        Ok(request) => request,
        Err(reason) => return SendOutcome::Rejected { reason },
    };
    let response = match client.execute(request).await {
        Ok(response) => response,
        Err(_) => {
            return SendOutcome::Unknown {
                reason: "write_transport_outcome_unknown_do_not_retry".to_owned(),
            }
        }
    };
    let status = response.status();
    if status.is_server_error() {
        return SendOutcome::Unknown {
            reason: "provider_server_error_reconcile_before_retry".to_owned(),
        };
    }
    if status == reqwest::StatusCode::TOO_MANY_REQUESTS {
        return SendOutcome::Failed {
            reason: "provider_rate_limited".to_owned(),
            retryable: true,
        };
    }
    if !status.is_success() {
        let reason = match status.as_u16() {
            401 | 403 => "provider_authorization_failed",
            404 => "reply_target_not_found",
            409 | 412 => "remote_target_changed",
            400 | 422 => "provider_rejected_reply",
            300..=399 => "provider_redirect_rejected",
            _ => "provider_rejected_request",
        };
        return SendOutcome::Rejected {
            reason: reason.to_owned(),
        };
    }
    if response
        .content_length()
        .is_some_and(|size| size > 1024 * 1024)
    {
        return SendOutcome::Unknown {
            reason: "write_response_too_large_reconcile".to_owned(),
        };
    }
    let mut response = response;
    let mut bytes = Vec::new();
    loop {
        match response.chunk().await {
            Ok(Some(chunk)) => {
                if bytes.len() + chunk.len() > 1024 * 1024 {
                    return SendOutcome::Unknown {
                        reason: "write_response_too_large_reconcile".to_owned(),
                    };
                }
                bytes.extend_from_slice(&chunk);
            }
            Ok(None) => break,
            Err(_) => {
                return SendOutcome::Unknown {
                    reason: "write_response_incomplete_reconcile".to_owned(),
                }
            }
        }
    }
    let Ok(body) = serde_json::from_slice::<Value>(&bytes) else {
        return SendOutcome::Unknown {
            reason: "write_response_unreadable_reconcile".to_owned(),
        };
    };
    let entity = if source.kind == ProviderKind::Linear {
        if body
            .pointer("/data/commentCreate/success")
            .and_then(Value::as_bool)
            != Some(true)
        {
            return SendOutcome::Unknown {
                reason: "linear_mutation_not_confirmed_reconcile".to_owned(),
            };
        }
        body.pointer("/data/commentCreate/comment")
            .unwrap_or(&Value::Null)
    } else {
        &body
    };
    let external_id = entity.get("id").and_then(|value| match value {
        Value::String(text) => Some(text.clone()),
        Value::Number(number) => Some(number.to_string()),
        _ => None,
    });
    let Some(external_id) = external_id.filter(|value| !value.is_empty()) else {
        return SendOutcome::Unknown {
            reason: "write_result_id_missing_reconcile".to_owned(),
        };
    };
    let result_url = entity
        .get("html_url")
        .or_else(|| entity.get("web_url"))
        .or_else(|| entity.get("url"))
        .and_then(Value::as_str)
        .map(str::to_owned);
    SendOutcome::Sent {
        external_id,
        url: result_url,
    }
}
