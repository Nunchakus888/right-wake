use base64::{engine::general_purpose::STANDARD, Engine as _};
use ed25519_dalek::{Signer, SigningKey};
use serde_json::{json, Value};
use waking::model::{now_ms, Draft, ProviderKind, ReplyTarget};
use waking::outbound::{
    approval_signing_bytes, build_write_request, prepare_approval_statement, verify_approval,
    ApprovalEnvelope, ApprovedAction, CurrentDraftState,
};
use waking::provider::{sha256_hex, SourceConfig, SourceRevision};

const ORIGINAL: &str = "@waking please inspect this issue";
const UPDATED: &str = "2026-11-03T09:10:00Z";

fn fixture(provider: &str) -> (SourceConfig, Draft, SigningKey) {
    // Synthetic test key only. Production code has no key-generation/sign API.
    let key = SigningKey::from_bytes(&[11; 32]);
    let source: SourceConfig = serde_json::from_value(json!({
        "id":"test-source", "kind":provider,"secret_env":"SYNTHETIC_TEST_SECRET",
        "sending_identity":"17", "review_public_key":STANDARD.encode(key.verifying_key().as_bytes())
    }))
    .unwrap();
    let (container, resource, comment, discussion, kind) = match source.kind {
        ProviderKind::Linear => (
            "workspace-1",
            "issue-1",
            Some("root-1"),
            None,
            "linear.comment",
        ),
        ProviderKind::Github => ("example/repo", "42", None, None, "github.issue_comment"),
        ProviderKind::Gitlab => ("7", "mr:42", Some("99"), Some("root-1"), "gitlab.note"),
        _ => unreachable!(),
    };
    let updated = chrono::DateTime::parse_from_rfc3339(UPDATED)
        .unwrap()
        .timestamp_millis();
    let provenance = serde_json::to_string(&SourceRevision {
        format_version: 1,
        entity_kind: kind.into(),
        entity_id: "99".into(),
        text_sha256: sha256_hex(ORIGINAL.as_bytes()),
        updated_at_ms: Some(updated),
        resource_updated_at_ms: None,
    })
    .unwrap();
    let draft = Draft {
        id: "draft-1".into(),
        job_id: "job-1".into(),
        source_id: source.id.clone(),
        resource_key: "canonical-resource".into(),
        event_id: "event-1".into(),
        body: "Reviewed: the implementation needs one additional guard.".into(),
        reply_target: ReplyTarget {
            provider: source.kind,
            container_id: container.into(),
            resource_id: resource.into(),
            comment_id: comment.map(str::to_owned),
            discussion_id: discussion.map(str::to_owned),
        },
        source_revision: "3".into(),
        source_event_revision: provenance,
        binding_key: "canonical-resource".into(),
        binding_version: 2,
        revision: 1,
        sending_identity: source.sending_identity.clone(),
        state: "draft".into(),
    };
    (source, draft, key)
}

fn signed(
    source: &SourceConfig,
    draft: &Draft,
    key: &SigningKey,
    time: i64,
) -> (ApprovedAction, CurrentDraftState) {
    let statement = prepare_approval_statement(source, draft, time + 600_000).unwrap();
    let signature = STANDARD.encode(
        key.sign(&approval_signing_bytes(&statement).unwrap())
            .to_bytes(),
    );
    (
        ApprovedAction {
            draft: draft.clone(),
            approval: ApprovalEnvelope {
                statement,
                signature,
            },
        },
        CurrentDraftState {
            draft: draft.clone(),
            source_revision: draft.source_revision.clone(),
            binding_version: draft.binding_version,
            binding_enabled: true,
        },
    )
}

#[test]
fn external_signature_is_required_and_bound_to_exact_review() {
    let (source, draft, key) = fixture("github");
    let time = now_ms();
    let (action, current) = signed(&source, &draft, &key, time);
    verify_approval(&source, &action, &current, time).unwrap();
    let mut invalid = action.clone();
    invalid.approval.signature = STANDARD.encode([0; 64]);
    assert!(verify_approval(&source, &invalid, &current, time).is_err());
    let mut edited = current.clone();
    edited.draft.body.push_str(" additional unreviewed content");
    assert!(verify_approval(&source, &action, &edited, time).is_err());
    let mut moved = current.clone();
    moved.binding_version += 1;
    assert!(verify_approval(&source, &action, &moved, time).is_err());
    let mut changed = current.clone();
    changed.source_revision = "4".into();
    assert!(verify_approval(&source, &action, &changed, time).is_err());
    for state in ["stale", "sent", "unknown", "skipped", "expired", "approved"] {
        let mut stale = current.clone();
        stale.draft.state = state.into();
        assert!(
            verify_approval(&source, &action, &stale, time).is_err(),
            "{state} cannot be sent"
        );
    }
    let mut paused = current.clone();
    paused.binding_enabled = false;
    assert!(verify_approval(&source, &action, &paused, time).is_err());
    let mut target = action.clone();
    target.draft.reply_target.resource_id = "43".into();
    assert!(verify_approval(&source, &target, &current, time).is_err());
    assert!(verify_approval(&source, &action, &current, time + 600_001).is_err());
}

#[test]
fn key_identity_api_origin_and_credential_rotation_revoke_old_approval() {
    let (mut source, draft, key) = fixture("github");
    let time = now_ms();
    let (action, current) = signed(&source, &draft, &key, time);
    source.review_public_key =
        Some(STANDARD.encode(SigningKey::from_bytes(&[12; 32]).verifying_key().as_bytes()));
    assert!(verify_approval(&source, &action, &current, time).is_err());
    let (mut source, draft, key) = fixture("github");
    let (action, current) = signed(&source, &draft, &key, time);
    source.api_base = Some("https://another-api.example.test".into());
    assert!(verify_approval(&source, &action, &current, time).is_err());
    let (mut source, draft, key) = fixture("github");
    let env = format!("WAKING_TEST_TOKEN_{}", uuid::Uuid::new_v4().simple());
    std::env::set_var(&env, "synthetic-credential-one");
    source.token_env = Some(env.clone());
    let (action, current) = signed(&source, &draft, &key, time);
    std::env::set_var(&env, "synthetic-credential-two");
    assert!(verify_approval(&source, &action, &current, time).is_err());
    std::env::remove_var(env);
}

#[test]
fn github_request_keeps_review_reply_on_root_comment() {
    let (source, mut draft, _) = fixture("github");
    draft.reply_target.comment_id = Some("123".into());
    let request =
        build_write_request(&source, &draft, &reqwest::Client::new(), "synthetic-token").unwrap();
    assert_eq!(
        request.url().as_str(),
        "https://api.github.com/repos/example/repo/pulls/42/comments/123/replies"
    );
    assert_eq!(request.method(), reqwest::Method::POST);
    assert_eq!(
        serde_json::from_slice::<Value>(request.body().unwrap().as_bytes().unwrap()).unwrap(),
        json!({"body":draft.body})
    );
}

#[test]
fn gitlab_request_preserves_discussion_and_refuses_silent_fallback() {
    let (source, mut draft, _) = fixture("gitlab");
    let request =
        build_write_request(&source, &draft, &reqwest::Client::new(), "synthetic-token").unwrap();
    assert_eq!(
        request.url().as_str(),
        "https://gitlab.com/api/v4/projects/7/merge_requests/42/discussions/root-1/notes"
    );
    draft.reply_target.discussion_id = None;
    assert_eq!(
        build_write_request(&source, &draft, &reqwest::Client::new(), "synthetic-token")
            .unwrap_err(),
        "discussion_location_unavailable"
    );
}

#[test]
fn linear_mutation_binds_parent_and_issue_without_interpolating_query() {
    let (source, mut draft, _) = fixture("linear");
    draft.body = "A quote: \"; mutation injected { anything }".into();
    let request = build_write_request(
        &source,
        &draft,
        &reqwest::Client::new(),
        "lin_api_synthetic",
    )
    .unwrap();
    let body: Value = serde_json::from_slice(request.body().unwrap().as_bytes().unwrap()).unwrap();
    assert_eq!(request.url().as_str(), "https://api.linear.app/graphql");
    assert_eq!(body["variables"]["input"]["parentId"], "root-1");
    assert_eq!(body["variables"]["input"]["issueId"], "issue-1");
    assert_eq!(body["variables"]["input"]["body"], draft.body);
    assert!(!body["query"]
        .as_str()
        .unwrap()
        .contains("mutation injected"));
}

// Native socket tests are intentionally separate from the deterministic contract
// suite: some hosted build sandboxes reject socket() with EPERM. Run them on CI
// or a native machine with `cargo test --test outbound_contract -- --ignored`.
mod native_socket {
    use super::*;
    use axum::{
        body::{to_bytes, Body},
        extract::State,
        http::{Request, StatusCode},
        response::IntoResponse,
        routing::any,
        Json, Router,
    };
    use std::sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    };
    use waking::outbound::{send_approved, send_approved_guarded, SendOutcome};

    #[derive(Clone)]
    struct Fake {
        provider: ProviderKind,
        writes: Arc<AtomicUsize>,
        new_activity: bool,
        mode: &'static str,
    }

    async fn handler(
        State(state): State<Fake>,
        request: Request<Body>,
    ) -> axum::response::Response {
        let method = request.method().clone();
        let path = request.uri().path().to_owned();
        let bytes = to_bytes(request.into_body(), 1024 * 1024).await.unwrap();
        let input: Value = serde_json::from_slice(&bytes).unwrap_or(Value::Null);
        let is_linear_query = state.provider == ProviderKind::Linear
            && input["query"]
                .as_str()
                .is_some_and(|query| query.starts_with("query "));
        if method == reqwest::Method::POST && !is_linear_query {
            state.writes.fetch_add(1, Ordering::SeqCst);
            if state.mode == "timeout" {
                tokio::time::sleep(std::time::Duration::from_secs(25)).await;
            }
            if state.mode == "server_error" {
                return (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(json!({"error":"synthetic"})),
                )
                    .into_response();
            }
            let body = if state.provider == ProviderKind::Linear {
                json!({"data":{"commentCreate":{"success":true,"comment":{"id":"500","url":"https://linear.app/example#comment-500"}}}})
            } else {
                json!({"id":500,"html_url":"https://example.test/reply/500"})
            };
            return (StatusCode::CREATED, Json(body)).into_response();
        }
        let entity = json!({"id":99,"body":ORIGINAL,"updated_at":UPDATED});
        let mut notes = vec![entity.clone()];
        if state.new_activity {
            notes.push(json!({"id":100,"body":"A human already replied","updated_at":"2026-11-03T09:11:00Z"}));
        }
        let output = match state.provider {
            ProviderKind::Github | ProviderKind::Gitlab if path.ends_with("/user") => {
                json!({"id":if state.mode=="identity_mismatch" {18}else{17}})
            }
            ProviderKind::Github if path.ends_with("/issues/comments/99") => entity,
            ProviderKind::Github if path.ends_with("/comments") => json!(notes),
            ProviderKind::Github => json!({"id":42,"updated_at":UPDATED,"locked":false}),
            ProviderKind::Gitlab if path.ends_with("/notes/99") => entity,
            ProviderKind::Gitlab if path.contains("/discussions/") => {
                json!({"id":"root-1","notes":notes})
            }
            ProviderKind::Gitlab => json!({"id":42,"updated_at":UPDATED,"discussion_locked":false}),
            ProviderKind::Linear
                if input["query"]
                    .as_str()
                    .is_some_and(|query| query.contains("WakingIdentity")) =>
            {
                json!({"data":{"viewer":{"id":if state.mode=="identity_mismatch" {"18"}else{"17"}}}})
            }
            ProviderKind::Linear => {
                json!({"data":{"comment":{"id":"99","body":ORIGINAL,"updatedAt":UPDATED,"issue":{"id":"issue-1"}},"issue":{"id":"issue-1","updatedAt":UPDATED,"archivedAt":null,"comments":{"nodes":[{"id":"99","updatedAt":UPDATED,"parent":{"id":"root-1"}}],"pageInfo":{"hasNextPage":false}}}}})
            }
            _ => unreachable!(),
        };
        Json(output).into_response()
    }

    async fn run(provider: &str, new_activity: bool, mode: &'static str) -> (SendOutcome, usize) {
        let (mut source, draft, key) = fixture(provider);
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("native_socket test needs permitted TCP sockets");
        source.api_base = Some(format!("http://{}", listener.local_addr().unwrap()));
        let writes = Arc::new(AtomicUsize::new(0));
        let fake = Fake {
            provider: source.kind,
            writes: writes.clone(),
            new_activity,
            mode,
        };
        let app = Router::new().fallback(any(handler)).with_state(fake);
        let task = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        let env = format!("WAKING_NATIVE_TEST_TOKEN_{}", uuid::Uuid::new_v4().simple());
        std::env::set_var(&env, "lin_api_synthetic");
        source.token_env = Some(env.clone());
        let time = now_ms();
        let (action, current) = signed(&source, &draft, &key, time);
        let result = if mode == "local_changed" {
            send_approved_guarded(&source, &action, &current, time, || {
                let mut refreshed = current.clone();
                refreshed.source_revision = "4".into();
                Ok((source.clone(), refreshed))
            })
            .await
        } else {
            send_approved(&source, &action, &current, time).await
        };
        let count = writes.load(Ordering::SeqCst);
        task.abort();
        std::env::remove_var(env);
        (result, count)
    }

    #[tokio::test]
    #[ignore = "native_socket: requires TCP sockets; hosted sandbox returns EPERM"]
    async fn three_provider_http_preflight_and_writes() {
        for provider in ["github", "gitlab", "linear"] {
            let (outcome, writes) = run(provider, false, "normal").await;
            assert!(
                matches!(outcome, SendOutcome::Sent { .. }),
                "{provider}: {outcome:?}"
            );
            assert_eq!(writes, 1);
        }
    }

    #[tokio::test]
    #[ignore = "native_socket: requires TCP sockets; hosted sandbox returns EPERM"]
    async fn newer_human_comment_prevents_write() {
        let (outcome, writes) = run("github", true, "normal").await;
        assert!(matches!(outcome, SendOutcome::Rejected { .. }));
        assert_eq!(writes, 0);
    }

    #[tokio::test]
    #[ignore = "native_socket: requires TCP sockets; hosted sandbox returns EPERM"]
    async fn provider_server_error_becomes_unknown_without_retry() {
        let (outcome, writes) = run("github", false, "server_error").await;
        assert!(matches!(outcome, SendOutcome::Unknown { .. }));
        assert_eq!(writes, 1);
    }

    #[tokio::test]
    #[ignore = "native_socket: requires TCP sockets and exercises the real 20 second deadline"]
    async fn write_timeout_becomes_unknown_without_retry() {
        let (outcome, writes) = run("github", false, "timeout").await;
        assert!(matches!(outcome, SendOutcome::Unknown { .. }));
        assert_eq!(writes, 1);
    }
    #[tokio::test]
    #[ignore = "native_socket: requires TCP sockets; hosted sandbox returns EPERM"]
    async fn current_local_state_is_reloaded_after_remote_reads() {
        let (outcome, writes) = run("github", false, "local_changed").await;
        assert!(matches!(outcome,SendOutcome::Rejected{reason} if reason=="source_changed"));
        assert_eq!(writes, 0);
    }
    #[tokio::test]
    #[ignore = "native_socket: requires TCP sockets; hosted sandbox returns EPERM"]
    async fn actual_provider_identity_must_match_reviewed_sender() {
        for provider in ["github", "gitlab", "linear"] {
            let (outcome, writes) = run(provider, false, "identity_mismatch").await;
            assert!(
                matches!(outcome,SendOutcome::Rejected{reason} if reason=="sending_identity_mismatch")
            );
            assert_eq!(writes, 0);
        }
    }
}
