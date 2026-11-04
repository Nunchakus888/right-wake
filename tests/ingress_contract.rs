use axum::{
    body::{to_bytes, Body},
    http::{Request, StatusCode},
};
use hmac::{Hmac, Mac};
use right_wake::{
    config::Config,
    db::Db,
    engine::Engine,
    model::now_ms,
    provider::SourceConfig,
    server::{app, AppState},
};
use serde_json::{json, Value};
use sha2::Sha256;
use std::sync::Arc;
use tower::ServiceExt;

struct Fixture {
    _dir: tempfile::TempDir,
    cfg: Config,
    engine: Engine,
    secret: String,
}
impl Fixture {
    fn new() -> Self {
        let dir = tempfile::tempdir().unwrap();
        let mut cfg = Config::setup(dir.path()).unwrap();
        let secret = uuid::Uuid::new_v4().to_string();
        let env = format!("RIGHT_WAKE_HTTP_TEST_{}", uuid::Uuid::new_v4().simple());
        std::env::set_var(&env, &secret);
        let source: SourceConfig = serde_json::from_value(
            json!({"id":"test","kind":"linear","secret_env":env,"tenant_id":"test-workspace"}),
        )
        .unwrap();
        cfg.sources.push(source);
        cfg.save(dir.path()).unwrap();
        let db = Db::open(dir.path()).unwrap();
        let engine = Engine::new(dir.path().into(), db);
        Self {
            _dir: dir,
            cfg,
            engine,
            secret,
        }
    }
    fn body(&self, id: &str) -> Vec<u8> {
        serde_json::to_vec_pretty(&json!({"type":"Comment","action":"create","organizationId":"test-workspace","webhookTimestamp":now_ms(),"data":{"id":id,"issueId":"test-issue","body":"@right-wake 继续先前的工作。\nPreserve the original context.","user":{"id":"requester"},"createdAt":"2026-11-03T08:00:00Z","updatedAt":"2026-11-03T08:00:00Z"}})).unwrap()
    }
    fn request(&self, body: Vec<u8>, delivery: &str, valid: bool) -> Request<Body> {
        let mut mac = Hmac::<Sha256>::new_from_slice(self.secret.as_bytes()).unwrap();
        mac.update(&body);
        let signature = if valid {
            hex::encode(mac.finalize().into_bytes())
        } else {
            "00".repeat(32)
        };
        Request::post("/hooks/test")
            .header("Content-Type", "application/json")
            .header("Linear-Signature", signature)
            .header("Linear-Delivery", delivery)
            .body(Body::from(body))
            .unwrap()
    }
    fn router(&self, limit: usize) -> axum::Router {
        app(
            AppState {
                engine: self.engine.clone(),
                relay_only: false,
                relay_wake: Arc::new(tokio::sync::Notify::new()),
                relay_sessions: Default::default(),
            },
            limit,
        )
    }
}

#[tokio::test]
async fn authenticated_raw_bytes_are_durable_before_http_ack_and_duplicates_do_not_repeat() {
    let fixture = Fixture::new();
    let raw = fixture.body("comment-a");
    let response = fixture
        .router(1024 * 1024)
        .oneshot(fixture.request(raw.clone(), "delivery-a", true))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let receipt: Value =
        serde_json::from_slice(&to_bytes(response.into_body(), 1024 * 1024).await.unwrap())
            .unwrap();
    assert_eq!(receipt["data"]["state"], "received");
    assert_eq!(fixture.engine.db.events(None, 100).unwrap().len(), 1);
    assert_eq!(fixture.engine.db.jobs(None, 100).unwrap().len(), 0);
    let response = fixture
        .router(1024 * 1024)
        .oneshot(fixture.request(raw, "delivery-a", true))
        .await
        .unwrap();
    let receipt: Value =
        serde_json::from_slice(&to_bytes(response.into_body(), 1024 * 1024).await.unwrap())
            .unwrap();
    assert_eq!(receipt["data"]["state"], "duplicate");
    assert_eq!(fixture.engine.db.events(None, 100).unwrap().len(), 1);
}

#[tokio::test]
async fn invalid_signature_and_oversize_body_never_enter_the_inbox() {
    let fixture = Fixture::new();
    let response = fixture
        .router(1024 * 1024)
        .oneshot(fixture.request(fixture.body("invalid"), "invalid", false))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    let response = fixture
        .router(10)
        .oneshot(fixture.request(fixture.body("oversize"), "oversize", true))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::PAYLOAD_TOO_LARGE);
    assert!(fixture.engine.db.events(None, 100).unwrap().is_empty());
}

#[tokio::test]
async fn capacity_failure_does_not_acknowledge_success() {
    let mut fixture = Fixture::new();
    fixture.cfg.max_pending = 1;
    fixture.cfg.save(&fixture.engine.dir).unwrap();
    let first = fixture
        .router(1024 * 1024)
        .oneshot(fixture.request(fixture.body("first"), "first", true))
        .await
        .unwrap();
    assert_eq!(first.status(), StatusCode::OK);
    let second = fixture
        .router(1024 * 1024)
        .oneshot(fixture.request(fixture.body("second"), "second", true))
        .await
        .unwrap();
    assert_eq!(second.status(), StatusCode::SERVICE_UNAVAILABLE);
    assert_eq!(fixture.engine.db.events(None, 100).unwrap().len(), 1);
}

#[tokio::test]
async fn paused_source_still_accepts_durable_input_without_model_execution() {
    let mut fixture = Fixture::new();
    fixture.cfg.sources[0].enabled = false;
    fixture.cfg.save(&fixture.engine.dir).unwrap();
    let response = fixture
        .router(1024 * 1024)
        .oneshot(fixture.request(fixture.body("paused"), "paused", true))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(fixture.engine.run_once().await.unwrap(), 0);
    assert_eq!(
        fixture.engine.db.events(Some("held"), 100).unwrap().len(),
        1
    );
}

#[tokio::test]
async fn empty_inbox_is_a_deterministic_zero_execution_pass() {
    let fixture = Fixture::new();
    for _ in 0..3 {
        assert_eq!(fixture.engine.run_once().await.unwrap(), 0);
    }
    assert!(fixture.engine.db.jobs(None, 100).unwrap().is_empty());
    assert!(fixture.engine.db.drafts().unwrap().is_empty());
}

#[tokio::test]
async fn relay_rejects_an_untransmittable_envelope_before_persisting_or_acknowledging() {
    let mut fixture = Fixture::new();
    fixture.cfg.max_payload_bytes = 4 * 1024 * 1024;
    fixture.cfg.sources[0].max_payload_bytes = fixture.cfg.max_payload_bytes;
    fixture.cfg.sources[0].max_content_chars = fixture.cfg.max_payload_bytes;
    fixture.cfg.save(&fixture.engine.dir).unwrap();
    let mut payload: Value = serde_json::from_slice(&fixture.body("large-relay")).unwrap();
    payload["data"]["body"] = json!(format!(
        "@right-wake {}",
        "x".repeat(right_wake::relay::MAX_FRAME)
    ));
    let raw = serde_json::to_vec(&payload).unwrap();
    let router = app(
        AppState {
            engine: fixture.engine.clone(),
            relay_only: true,
            relay_wake: Arc::new(tokio::sync::Notify::new()),
            relay_sessions: Default::default(),
        },
        fixture.cfg.max_payload_bytes,
    );
    let response = router
        .oneshot(fixture.request(raw, "large-relay", true))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::PAYLOAD_TOO_LARGE);
    let receipt: Value =
        serde_json::from_slice(&to_bytes(response.into_body(), 4096).await.unwrap()).unwrap();
    assert_eq!(receipt["error"]["code"], "relay_envelope_too_large");
    assert!(fixture.engine.db.events(None, 100).unwrap().is_empty());
}
