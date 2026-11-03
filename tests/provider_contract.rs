use base64::{engine::general_purpose::STANDARD, Engine as _};
use hmac::{Hmac, Mac};
use serde_json::{json, Value};
use sha2::Sha256;
use waking::model::ProviderKind;
use waking::provider::{
    ingest_with_secret, verify, Headers, IngestOutcome, SignatureScheme, SourceConfig,
    SourceRevision,
};

const NOW: i64 = 1_793_695_800_000;
const SECRET: &[u8] = b"synthetic-webhook-secret-not-a-real-credential";

fn sign(key: &[u8], body: &[u8]) -> String {
    let mut mac = Hmac::<Sha256>::new_from_slice(key).unwrap();
    mac.update(body);
    hex::encode(mac.finalize().into_bytes())
}

fn source(kind: &str) -> SourceConfig {
    serde_json::from_value(
        json!({"id":"test-source","kind":kind,"secret_env":"SYNTHETIC_TEST_SECRET"}),
    )
    .unwrap()
}

fn github(payload: &Value, event: &str) -> (Vec<u8>, Headers) {
    let body = serde_json::to_vec(payload).unwrap();
    let headers = Headers::from([
        ("X-GitHub-Event".into(), event.into()),
        ("X-GitHub-Delivery".into(), "delivery-1".into()),
        (
            "X-Hub-Signature-256".into(),
            format!("sha256={}", sign(SECRET, &body)),
        ),
    ]);
    (body, headers)
}

fn github_comment(body: &str) -> Value {
    json!({"action":"created","repository":{"id":12,"full_name":"example/repo","owner":{"id":5}},
        "issue":{"id":88,"number":42,"pull_request":{"url":"https://api.github.com/repos/example/repo/pulls/42"},"updated_at":"2026-11-03T09:10:00Z"},
        "comment":{"id":90,"body":body,"updated_at":"2026-11-03T09:10:00Z","html_url":"https://github.com/example/repo/pull/42#issuecomment-90"},"sender":{"id":8}})
}

fn event(outcome: IngestOutcome) -> waking::model::NormalizedEvent {
    match outcome {
        IngestOutcome::Event(event) => event,
        other => panic!("Expected event, got {other:?}"),
    }
}

#[test]
fn raw_unicode_bytes_are_verified_without_reserialization() {
    let raw = "{ \"text\": \"你好 🌍\",\n \"n\":1 }".as_bytes();
    let headers = Headers::from([(
        "x-hub-signature-256".into(),
        format!("sha256={}", sign(SECRET, raw)),
    )]);
    verify(SignatureScheme::GitHub, SECRET, &headers, raw, NOW, 60_000).unwrap();
    let compact = serde_json::to_vec(&serde_json::from_slice::<Value>(raw).unwrap()).unwrap();
    assert!(verify(
        SignatureScheme::GitHub,
        SECRET,
        &headers,
        &compact,
        NOW,
        60_000
    )
    .is_err());
}

#[test]
fn linear_timestamp_is_taken_from_the_signed_body() {
    let raw = serde_json::to_vec(&json!({"webhookTimestamp":NOW-120_000})).unwrap();
    let headers = Headers::from([
        ("Linear-Signature".into(), sign(SECRET, &raw)),
        ("Linear-Timestamp".into(), NOW.to_string()),
    ]);
    assert_eq!(
        verify(SignatureScheme::Linear, SECRET, &headers, &raw, NOW, 60_000)
            .unwrap_err()
            .code,
        "stale_timestamp"
    );
}

#[test]
fn linear_stable_identity_and_provenance() {
    let payload = json!({"type":"Comment","action":"create","organizationId":"workspace-a","actor":{"id":"person"},"webhookTimestamp":NOW,
        "createdAt":"2026-11-03T09:10:00Z","data":{"id":"reply-2","issueId":"issue-1","parentId":"comment-1","body":"@waking 请继续","updatedAt":"2026-11-03T09:10:00Z"}});
    let body = serde_json::to_vec(&payload).unwrap();
    let headers = Headers::from([
        ("Linear-Signature".into(), sign(SECRET, &body)),
        ("Linear-Delivery".into(), "delivery-1".into()),
    ]);
    let mut config = source("linear");
    let normalized = event(ingest_with_secret(&config, SECRET, &headers, &body, NOW).unwrap());
    assert_eq!(
        normalized.resource_key,
        "linear:https://linear.app:workspace-a:issue-1"
    );
    assert_eq!(
        normalized
            .reply_target
            .as_ref()
            .unwrap()
            .comment_id
            .as_deref(),
        Some("comment-1")
    );
    assert!(normalized.actionable && normalized.explicit_trigger);
    let snapshot: SourceRevision = serde_json::from_str(&normalized.revision).unwrap();
    assert_eq!(snapshot.entity_id, "reply-2");
    config.id = "another-source".into();
    let another = event(ingest_with_secret(&config, SECRET, &headers, &body, NOW).unwrap());
    assert_ne!(normalized.event_id, another.event_id);
    config.tenant_id = Some("different-workspace".into());
    assert_eq!(
        ingest_with_secret(&config, SECRET, &headers, &body, NOW)
            .unwrap_err()
            .code,
        "tenant_mismatch"
    );
}

#[test]
fn github_pr_comments_and_inline_reviews_share_one_resource() {
    let config = source("github");
    let (body, headers) = github(&github_comment("@waking inspect"), "issue_comment");
    let first = event(ingest_with_secret(&config, SECRET, &headers, &body, NOW).unwrap());
    let review = json!({"action":"created","repository":{"id":12,"full_name":"example/repo","owner":{"id":5}},"pull_request":{"id":999,"number":42,"updated_at":"2026-11-03T09:10:00Z"},"comment":{"id":456,"in_reply_to_id":123,"body":"@waking inspect","updated_at":"2026-11-03T09:10:00Z"},"sender":{"id":8}});
    let (body, headers) = github(&review, "pull_request_review_comment");
    let second = event(ingest_with_secret(&config, SECRET, &headers, &body, NOW).unwrap());
    assert_eq!(first.resource_key, second.resource_key);
    assert_eq!(
        first.resource_key,
        "github:https://github.com:repository:12:pr:42"
    );
    assert!(first.reply_target.unwrap().comment_id.is_none());
    assert_eq!(
        second.reply_target.unwrap().comment_id.as_deref(),
        Some("123")
    );
}

#[test]
fn followups_are_candidates_but_unbound_mentions_require_exact_token() {
    let config = source("github");
    for text in [
        "Any progress?",
        "@waking_extra inspect",
        "somebody@waking inspect",
    ] {
        let (body, headers) = github(&github_comment(text), "issue_comment");
        let normalized = event(ingest_with_secret(&config, SECRET, &headers, &body, NOW).unwrap());
        assert!(normalized.actionable);
        assert!(
            !normalized.explicit_trigger,
            "Unexpected mention match: {text}"
        );
    }
}

#[test]
fn self_and_out_of_scope_events_never_wake_the_model() {
    let mut config = source("github");
    let (body, headers) = github(&github_comment("@waking inspect"), "issue_comment");
    config.bot_actor_ids = vec!["8".into()];
    let normalized = event(ingest_with_secret(&config, SECRET, &headers, &body, NOW).unwrap());
    assert!(!normalized.actionable && !normalized.explicit_trigger);
    assert_eq!(normalized.skip_reason.as_deref(), Some("self_actor"));
    config.bot_actor_ids.clear();
    config.allowed_resources = vec!["github:https://github.com:repository:99:*".into()];
    let normalized = event(ingest_with_secret(&config, SECRET, &headers, &body, NOW).unwrap());
    assert_eq!(normalized.skip_reason.as_deref(), Some("outside_scope"));
}

#[test]
fn issue_state_changes_with_old_mentions_only_update_cache() {
    let payload = json!({"action":"closed","repository":{"id":12,"full_name":"example/repo","owner":{"id":5}},"issue":{"id":88,"number":42,"body":"@waking inspect","updated_at":"2026-11-03T09:10:00Z"},"sender":{"id":8}});
    let (body, headers) = github(&payload, "issues");
    let normalized =
        event(ingest_with_secret(&source("github"), SECRET, &headers, &body, NOW).unwrap());
    assert!(!normalized.actionable);
}

#[test]
fn gitlab_standard_signature_is_bound_to_message_and_timestamp() {
    let raw = b"{\"object_kind\":\"note\"}";
    let timestamp = (NOW / 1000).to_string();
    let message_id = "message-42";
    let mut message = format!("{message_id}.{timestamp}.").into_bytes();
    message.extend_from_slice(raw);
    let mut mac = Hmac::<Sha256>::new_from_slice(SECRET).unwrap();
    mac.update(&message);
    let signature = STANDARD.encode(mac.finalize().into_bytes());
    let token = format!("whsec_{}", STANDARD.encode(SECRET));
    let mut headers = Headers::from([
        ("webhook-id".into(), message_id.into()),
        ("webhook-timestamp".into(), timestamp),
        (
            "webhook-signature".into(),
            format!("v2,ignored v1,{signature}"),
        ),
    ]);
    verify(
        SignatureScheme::GitLabSigning,
        token.as_bytes(),
        &headers,
        raw,
        NOW,
        300_000,
    )
    .unwrap();
    headers.insert("webhook-id".into(), "different".into());
    assert!(verify(
        SignatureScheme::GitLabSigning,
        token.as_bytes(),
        &headers,
        raw,
        NOW,
        300_000
    )
    .is_err());
    headers.remove("webhook-signature");
    headers.insert("X-Gitlab-Token".into(), token.clone());
    assert!(verify(
        SignatureScheme::GitLabSigning,
        token.as_bytes(),
        &headers,
        raw,
        NOW,
        300_000
    )
    .is_err());
}

#[test]
fn gitlab_discussion_location_is_not_replaced_by_top_level_comment() {
    let mut config = source("gitlab");
    config.signature = Some(SignatureScheme::GitLabToken);
    let headers = Headers::from([
        (
            "X-Gitlab-Token".into(),
            String::from_utf8(SECRET.to_vec()).unwrap(),
        ),
        ("Idempotency-Key".into(), "delivery-1".into()),
    ]);
    let mut payload = json!({"object_kind":"note","project":{"id":7},"user":{"id":8},"merge_request":{"iid":42},"object_attributes":{"id":99,"noteable_type":"MergeRequest","type":"DiffNote","note":"@waking inspect","discussion_id":"discussion-root","updated_at":"2026-11-03T09:10:00Z"}});
    let normalized = event(
        ingest_with_secret(
            &config,
            SECRET,
            &headers,
            &serde_json::to_vec(&payload).unwrap(),
            NOW,
        )
        .unwrap(),
    );
    assert_eq!(
        normalized.resource_key,
        "gitlab:https://gitlab.com:project:7:mr:42"
    );
    assert_eq!(
        normalized.reply_target.unwrap().discussion_id.as_deref(),
        Some("discussion-root")
    );
    payload["object_attributes"]
        .as_object_mut()
        .unwrap()
        .remove("discussion_id");
    let normalized = event(
        ingest_with_secret(
            &config,
            SECRET,
            &headers,
            &serde_json::to_vec(&payload).unwrap(),
            NOW,
        )
        .unwrap(),
    );
    assert!(normalized.reply_target.is_none());
}

#[test]
fn generic_json_pointer_mapping_cannot_select_local_execution() {
    let config:SourceConfig=serde_json::from_value(json!({"id":"custom","kind":"generic","instance":"https://example.test","secret_env":"TEST_SECRET","generic":{"namespace":"tasks","resource_kind":"work","event_id":"/id","resource_id":"/task/id","content":"/task/text"}})).unwrap();
    let payload = json!({"id":"event-1","task":{"id":"work-2","text":"@waking inspect"},"thread_id":"attacker","cwd":"/etc","command":"sh -c anything"});
    let raw = serde_json::to_vec(&payload).unwrap();
    let timestamp = (NOW / 1000).to_string();
    let mut message = format!("{timestamp}.").into_bytes();
    message.extend_from_slice(&raw);
    let headers = Headers::from([
        ("X-Waking-Timestamp".into(), timestamp),
        (
            "X-Waking-Signature".into(),
            format!("sha256={}", sign(SECRET, &message)),
        ),
    ]);
    let normalized = event(ingest_with_secret(&config, SECRET, &headers, &raw, NOW).unwrap());
    assert_eq!(normalized.content, "@waking inspect");
    assert!(normalized.reply_target.is_none());
    let serialized = serde_json::to_value(&normalized).unwrap();
    assert!(
        serialized.get("thread_id").is_none()
            && serialized.get("cwd").is_none()
            && serialized.get("command").is_none()
    );
    assert!(normalized.resource_key.ends_with(":tasks:work:work-2"));
}

#[test]
fn malformed_and_oversized_inputs_fail_before_model_routing() {
    let mut config = source("github");
    config.max_payload_bytes = 8;
    config.max_content_chars = 8;
    assert_eq!(
        ingest_with_secret(&config, SECRET, &Headers::new(), b"too-large-payload", NOW)
            .unwrap_err()
            .code,
        "payload_too_large"
    );
    let config = source("github");
    let (body, mut headers) = github(&github_comment("@waking inspect"), "issue_comment");
    headers.insert("x-hub-signature-256".into(), "sha256=duplicate".into());
    assert_eq!(
        ingest_with_secret(&config, SECRET, &headers, &body, NOW)
            .unwrap_err()
            .code,
        "invalid_header"
    );
    assert_eq!(config.kind, ProviderKind::Github);
}

#[test]
fn paused_sources_still_require_signature_and_preserve_receipts() {
    let mut config = source("github");
    config.enabled = false;
    let (body, headers) = github(&github_comment("@waking continue"), "issue_comment");
    let normalized = event(ingest_with_secret(&config, SECRET, &headers, &body, NOW).unwrap());
    assert!(normalized.actionable);
    assert!(ingest_with_secret(&config, b"wrong-secret", &headers, &body, NOW).is_err());
}

#[test]
fn relay_policy_is_source_scoped_and_can_only_narrow_trigger_flags() {
    let mut config = source("github");
    let (body, headers) = github(&github_comment("@waking continue"), "issue_comment");
    let normalized = event(ingest_with_secret(&config, SECRET, &headers, &body, NOW).unwrap());
    let mut narrowed = normalized.clone();
    config.trigger_patterns = vec!["@different-bot".into()];
    waking::provider::apply_local_policy(&config, &mut narrowed).unwrap();
    assert!(!narrowed.explicit_trigger);
    assert!(narrowed.actionable);
    config.trigger_patterns = vec!["@waking".into()];
    waking::provider::apply_local_policy(&config, &mut narrowed).unwrap();
    assert!(!narrowed.explicit_trigger);
    let mut wrong = normalized.clone();
    wrong.conversation_key = Some("another-resource:comment:1".into());
    assert!(waking::provider::apply_local_policy(&config, &mut wrong).is_err());
    let mut wrong = normalized.clone();
    wrong.resource_key = "github:https://untrusted.example:repository:12:pr:42".into();
    assert!(waking::provider::apply_local_policy(&config, &mut wrong).is_err());
    let mut wrong = normalized;
    wrong.source_id = "different-source".into();
    assert!(waking::provider::apply_local_policy(&config, &mut wrong).is_err());
}

#[test]
fn generic_semantic_identity_has_unambiguous_field_boundaries() {
    let config:SourceConfig=serde_json::from_value(json!({"id":"custom","kind":"generic","instance":"https://example.test","secret_env":"TEST_SECRET","generic":{"namespace":"tasks","resource_kind":"work","event_id":"/id","resource_id":"/resource","content":"/text","action":"/action"}})).unwrap();
    let mut ids = Vec::new();
    for (action, id) in [("request|x", "y"), ("request", "x|y")] {
        let raw = serde_json::to_vec(
            &json!({"id":id,"action":action,"resource":"1","text":"@waking continue"}),
        )
        .unwrap();
        let timestamp = (NOW / 1000).to_string();
        let mut message = format!("{timestamp}.").into_bytes();
        message.extend_from_slice(&raw);
        let headers = Headers::from([
            ("X-Waking-Timestamp".into(), timestamp),
            (
                "X-Waking-Signature".into(),
                format!("sha256={}", sign(SECRET, &message)),
            ),
        ]);
        ids.push(event(ingest_with_secret(&config, SECRET, &headers, &raw, NOW).unwrap()).event_id);
    }
    assert_ne!(ids[0], ids[1]);
}
