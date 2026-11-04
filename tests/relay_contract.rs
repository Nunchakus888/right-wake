use futures_util::{SinkExt, StreamExt};
use right_wake::{
    config::{Config, RelayDevice},
    db::Db,
    engine::Engine,
    model::{NormalizedEvent, ProviderKind, ThreadTarget},
    relay::{receive_wire, serve_wire, RelayWire},
    server::AppState,
};
use serde_json::{json, Value};
use std::{sync::Arc, time::Duration};
use tempfile::TempDir;
use tokio::{
    io::DuplexStream,
    sync::{oneshot, Notify},
    time::timeout,
};
use tokio_tungstenite::{
    tungstenite::{protocol::Role, Message},
    WebSocketStream,
};

type Socket = WebSocketStream<DuplexStream>;
const TOKEN: &str = "unit-test-token-not-a-real-credential-0123456789";

fn setup() -> (TempDir, Engine, Config) {
    let dir = tempfile::tempdir().unwrap();
    let db = Db::open(dir.path()).unwrap();
    let cfg = Config {
        sources: vec![serde_json::from_value(json!({
            "id":"source","kind":"linear","tenant_id":"tenant","secret_env":"NO_REAL_SECRET"
        }))
        .unwrap()],
        ..Config::default()
    };
    cfg.save(dir.path()).unwrap();
    let engine = Engine::new(dir.path().to_owned(), db);
    (dir, engine, cfg)
}

fn event(id: &str) -> NormalizedEvent {
    NormalizedEvent {
        event_id: id.into(),
        delivery_id: format!("delivery-{id}"),
        source_id: "source".into(),
        provider: ProviderKind::Linear,
        tenant_id: "tenant".into(),
        resource_key: format!("linear:https://linear.app:tenant:{id}"),
        conversation_key: None,
        actor_id: "person".into(),
        action: "comment.create".into(),
        occurred_at_ms: 1,
        received_at_ms: 1,
        source_url: None,
        content: "@right-wake continue".into(),
        revision: format!("rev-{id}"),
        actionable: true,
        explicit_trigger: true,
        skip_reason: None,
        reply_target: None,
    }
}

fn persist(engine: &Engine, cfg: &Config, event: &NormalizedEvent) {
    engine
        .db
        .receive(event, &serde_json::to_vec(event).unwrap(), cfg)
        .unwrap();
}

fn complete(engine: &Engine, cfg: &Config, event: &NormalizedEvent, relay: Option<&str>) {
    persist(engine, cfg, event);
    let target = ThreadTarget {
        host_id: "test".into(),
        thread_id: format!("thread-{}", event.event_id),
        cwd: None,
    };
    let binding = engine
        .db
        .bind("source", &event.resource_key, &target, "explicit", None)
        .unwrap();
    let job = engine.db.enqueue(event, &binding).unwrap().unwrap();
    engine.db.claim(&job.id).unwrap().unwrap();
    engine
        .db
        .finish_job(
            &job.id,
            "completed",
            "test host completed",
            Some("turn"),
            true,
        )
        .unwrap();
    if let Some(relay) = relay {
        engine
            .db
            .record_relay_receipt(relay, &event.event_id)
            .unwrap();
    }
}

async fn sockets() -> (Socket, Socket) {
    // Tiny in-memory network buffers make reciprocal write backpressure
    // reproducible without permission to bind OS sockets.
    let (a, b) = tokio::io::duplex(64);
    let left = WebSocketStream::from_raw_socket(a, Role::Server, None).await;
    let right = WebSocketStream::from_raw_socket(b, Role::Client, None).await;
    (left, right)
}

async fn read(socket: &mut Socket) -> Value {
    timeout(Duration::from_secs(3), async {
        loop {
            match socket.next().await.unwrap().unwrap() {
                Message::Text(text) => return serde_json::from_str(&text).unwrap(),
                Message::Ping(bytes) => socket.send(Message::Pong(bytes)).await.unwrap(),
                frame => panic!("unexpected frame {frame:?}"),
            }
        }
    })
    .await
    .expect("protocol response deadline")
}

async fn send(socket: &mut Socket, value: Value) {
    timeout(
        Duration::from_secs(3),
        socket.send(Message::Text(value.to_string().into())),
    )
    .await
    .expect("write deadline")
    .unwrap();
}

fn relay_state(engine: &Engine, mut cfg: Config) -> (AppState, String) {
    let variable = format!("RIGHT_WAKE_TEST_RELAY_{}", uuid::Uuid::new_v4().simple());
    std::env::set_var(&variable, TOKEN);
    cfg.relay_devices.push(RelayDevice {
        id: "device".into(),
        token_env: variable.clone(),
        source_ids: vec!["source".into()],
        enabled: true,
    });
    cfg.save(&engine.dir).unwrap();
    (
        AppState {
            engine: engine.clone(),
            relay_only: true,
            relay_wake: Arc::new(Notify::new()),
            relay_sessions: Default::default(),
        },
        variable,
    )
}

#[tokio::test]
async fn duplicate_ack_and_unknown_completion_do_not_lose_pending_events() {
    let (_dir, engine, cfg) = setup();
    persist(&engine, &cfg, &event("first"));
    persist(&engine, &cfg, &event("second"));
    let (state, variable) = relay_state(&engine, cfg);
    let (server, mut peer) = sockets().await;
    let (cancel, cancelled) = oneshot::channel();
    let task = tokio::spawn(serve_wire(
        RelayWire::websocket(server),
        state,
        "device".into(),
        TOKEN.into(),
        cancelled,
    ));
    assert_eq!(read(&mut peer).await["event"]["event_id"], "first");
    send(
        &mut peer,
        json!({"type":"ack","phase":"executed","event_id":"not-on-this-relay"}),
    )
    .await;
    assert_eq!(read(&mut peer).await["type"], "ack_rejected");
    assert_eq!(engine.db.relay_phase("device", "first").unwrap(), None);
    send(
        &mut peer,
        json!({"type":"ack","phase":"executed","event_id":"second"}),
    )
    .await;
    assert_eq!(read(&mut peer).await["reason"], "delivery_not_received");
    assert_eq!(engine.db.relay_phase("device", "second").unwrap(), None);
    send(
        &mut peer,
        json!({"type":"ack","phase":"received","event_id":"first"}),
    )
    .await;
    assert_eq!(read(&mut peer).await["type"], "ack_confirmed");
    assert_eq!(read(&mut peer).await["event"]["event_id"], "second");
    for phase in ["received", "executed", "received"] {
        send(
            &mut peer,
            json!({"type":"ack","phase":phase,"event_id":"first"}),
        )
        .await;
        assert_eq!(read(&mut peer).await["type"], "ack_confirmed");
    }
    assert_eq!(
        engine.db.relay_phase("device", "first").unwrap().as_deref(),
        Some("executed")
    );
    cancel.send(()).unwrap();
    assert!(timeout(Duration::from_secs(1), task)
        .await
        .unwrap()
        .unwrap()
        .is_err());
    std::env::remove_var(variable);
}

#[tokio::test(start_paused = true)]
async fn a_nonreading_peer_hits_the_io_deadline_without_any_model_work() {
    let (_dir, engine, cfg) = setup();
    let mut large = event("first");
    large.content = "x".repeat(128 * 1024);
    persist(&engine, &cfg, &large);
    let (state, variable) = relay_state(&engine, cfg);
    let (server, _unread_peer) = sockets().await;
    let (_cancel, cancelled) = oneshot::channel();
    let task = tokio::spawn(serve_wire(
        RelayWire::websocket(server),
        state,
        "device".into(),
        TOKEN.into(),
        cancelled,
    ));
    let error = task.await.unwrap().unwrap_err().to_string();
    assert!(error.contains("write deadline"), "{error}");
    assert!(engine.db.jobs(None, 10).unwrap().is_empty());
    assert_eq!(engine.db.relay_phase("device", "first").unwrap(), None);
    std::env::remove_var(variable);
}

#[tokio::test]
async fn config_revocation_is_rechecked_before_ack_without_a_watch_notification() {
    let (_dir, engine, cfg) = setup();
    persist(&engine, &cfg, &event("first"));
    let (state, variable) = relay_state(&engine, cfg);
    let (server, mut peer) = sockets().await;
    let (_cancel, cancelled) = oneshot::channel();
    let task = tokio::spawn(serve_wire(
        RelayWire::websocket(server),
        state,
        "device".into(),
        TOKEN.into(),
        cancelled,
    ));
    read(&mut peer).await;
    let mut revoked = Config::load(&engine.dir).unwrap();
    revoked.relay_devices[0].enabled = false;
    revoked.save(&engine.dir).unwrap();
    send(
        &mut peer,
        json!({"type":"ack","phase":"received","event_id":"first"}),
    )
    .await;
    assert!(timeout(Duration::from_secs(1), task)
        .await
        .unwrap()
        .unwrap()
        .is_err());
    assert_eq!(engine.db.relay_phase("device", "first").unwrap(), None);
    std::env::remove_var(variable);
}

#[tokio::test(start_paused = true)]
async fn an_unacknowledged_delivery_is_replayed_after_reconnection() {
    let (_dir, engine, cfg) = setup();
    persist(&engine, &cfg, &event("first"));
    let (state, variable) = relay_state(&engine, cfg);
    let (server, mut peer) = sockets().await;
    let (_cancel, cancelled) = oneshot::channel();
    let task = tokio::spawn(serve_wire(
        RelayWire::websocket(server),
        state.clone(),
        "device".into(),
        TOKEN.into(),
        cancelled,
    ));
    assert_eq!(read(&mut peer).await["event"]["event_id"], "first");
    let error = task.await.unwrap().unwrap_err().to_string();
    assert!(error.contains("ACK deadline"), "{error}");
    assert_eq!(engine.db.relay_phase("device", "first").unwrap(), None);
    let (server, mut peer) = sockets().await;
    let (cancel, cancelled) = oneshot::channel();
    let task = tokio::spawn(serve_wire(
        RelayWire::websocket(server),
        state,
        "device".into(),
        TOKEN.into(),
        cancelled,
    ));
    assert_eq!(read(&mut peer).await["event"]["event_id"], "first");
    send(
        &mut peer,
        json!({"type":"ack","phase":"received","event_id":"first"}),
    )
    .await;
    assert_eq!(read(&mut peer).await["type"], "ack_confirmed");
    cancel.send(()).unwrap();
    assert!(task.await.unwrap().is_err());
    std::env::remove_var(variable);
}

#[tokio::test]
async fn replacement_cancels_a_connection_even_while_its_peer_does_not_read() {
    let (_dir, engine, cfg) = setup();
    let mut large = event("first");
    large.content = "x".repeat(128 * 1024);
    persist(&engine, &cfg, &large);
    let (state, variable) = relay_state(&engine, cfg);
    let (server, _unread_peer) = sockets().await;
    let (cancel, cancelled) = oneshot::channel();
    let task = tokio::spawn(serve_wire(
        RelayWire::websocket(server),
        state,
        "device".into(),
        TOKEN.into(),
        cancelled,
    ));
    tokio::task::yield_now().await;
    cancel.send(()).unwrap();
    assert!(timeout(Duration::from_secs(1), task)
        .await
        .unwrap()
        .unwrap()
        .is_err());
    assert_eq!(engine.db.relay_phase("device", "first").unwrap(), None);
    std::env::remove_var(variable);
}

#[tokio::test]
async fn local_resource_scope_is_applied_before_persistence_and_received_ack() {
    let (_dir, engine, mut cfg) = setup();
    cfg.sources[0].allowed_resources = vec!["linear:https://linear.app:tenant:allowed".into()];
    cfg.save(&engine.dir).unwrap();
    let (mut peer, client) = sockets().await;
    let completions = engine.completed.subscribe();
    let task = tokio::spawn(receive_wire(
        RelayWire::websocket(client),
        engine.clone(),
        "relay-a".into(),
        vec!["source".into()],
        completions,
    ));
    send(
        &mut peer,
        json!({"type":"event","schema_version":1,"event":event("outside")}),
    )
    .await;
    let ack = read(&mut peer).await;
    assert_eq!(ack["phase"], "received");
    let stored = engine.db.event("outside").unwrap();
    assert!(!stored.actionable);
    assert!(!stored.explicit_trigger);
    assert_eq!(
        engine.db.event_detail("outside").unwrap()["state"],
        "cached"
    );
    assert!(engine.db.relay_received_from("relay-a", "outside").unwrap());
    // The worker signal must already exist when the peer sees its disk ACK.
    timeout(Duration::from_secs(1), engine.wake.notified())
        .await
        .unwrap();
    task.abort();
}

#[tokio::test]
async fn local_tenant_mismatch_is_rejected_without_a_receipt() {
    let (_dir, engine, _cfg) = setup();
    let (mut peer, client) = sockets().await;
    let completions = engine.completed.subscribe();
    let task = tokio::spawn(receive_wire(
        RelayWire::websocket(client),
        engine.clone(),
        "relay-a".into(),
        vec!["source".into()],
        completions,
    ));
    let mut wrong = event("wrong");
    wrong.tenant_id = "different-tenant".into();
    send(
        &mut peer,
        json!({"type":"event","schema_version":1,"event":wrong}),
    )
    .await;
    assert!(timeout(Duration::from_secs(1), task)
        .await
        .unwrap()
        .unwrap()
        .is_err());
    assert!(engine.db.event("wrong").is_err());
    assert!(!engine.db.relay_received_from("relay-a", "wrong").unwrap());
}

#[tokio::test]
async fn bounded_replay_reads_live_events_and_only_replays_its_own_receipts() {
    let (_dir, engine, cfg) = setup();
    complete(&engine, &cfg, &event("direct"), None);
    complete(&engine, &cfg, &event("other-relay"), Some("relay-b"));
    for index in 0..12 {
        complete(
            &engine,
            &cfg,
            &event(&format!("old-{index:02}")),
            Some("relay-a"),
        );
    }
    let (mut peer, client) = sockets().await;
    let completions = engine.completed.subscribe();
    let task = tokio::spawn(receive_wire(
        RelayWire::websocket(client),
        engine.clone(),
        "relay-a".into(),
        vec!["source".into()],
        completions,
    ));
    let mut live = event("live");
    live.content = format!("@right-wake {}", "x".repeat(96 * 1024));
    // The peer writes a large new event while the receiver writes the first
    // history ACK. A single blocking read/write loop deadlocks with 64 B pipes.
    send(
        &mut peer,
        json!({"type":"event","schema_version":1,"event":live}),
    )
    .await;
    let mut executed = Vec::new();
    let mut received_live = false;
    for _ in 0..13 {
        let ack = read(&mut peer).await;
        let id = ack["event_id"].as_str().unwrap().to_owned();
        if ack["phase"] == "received" {
            assert_eq!(id, "live");
            received_live = true;
            assert!(engine.db.event("live").is_ok());
        } else {
            assert!(id.starts_with("old-"), "unrelated receipt leaked: {id}");
            executed.push(id);
        }
        send(
            &mut peer,
            json!({"type":"ack_confirmed","event_id":ack["event_id"],"phase":ack["phase"]}),
        )
        .await;
    }
    assert!(received_live);
    assert_eq!(executed.len(), 12);
    assert!(engine.db.relay_received_from("relay-a", "live").unwrap());
    task.abort();
}
