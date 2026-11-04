//! Operator controls keep ownership stable and never turn a pause into a new session.
use clap::Parser;
use right_wake::{
    cli::{self, Cli},
    config::{Config, HostConfig, RouterConfig},
    db::Db,
    engine::Engine,
    host::{DispatchOutcome, DispatchRequest},
    model::{ContextCard, NormalizedEvent, ProviderKind, ThreadTarget},
};
use serde_json::{json, Value};
use std::{path::Path, time::Duration};
use tempfile::TempDir;

struct Fixture {
    _dir: TempDir,
    engine: Engine,
    config: Config,
}
impl Fixture {
    fn new() -> Self {
        let dir = tempfile::tempdir().unwrap();
        let mut config = Config {
            sources: vec![serde_json::from_value(json!({
                "id":"source","kind":"linear","secret_env":"CONTROL_TEST_SECRET",
                "tenant_id":"tenant","sending_identity":"test-only"
            }))
            .unwrap()],
            ..Config::default()
        };
        for host in ["alpha", "beta"] {
            config.hosts.push(HostConfig::Command {
                id: host.into(),
                program: "/bin/sh".into(),
                args: vec![
                    "-c".into(),
                    "printf '%s\\n' started >> \"$1\"; exec \"$2\" --data-dir \"$3\" demo-worker"
                        .into(),
                    "trusted-control-fixture".into(),
                    dir.path()
                        .join(format!("{host}.calls"))
                        .to_string_lossy()
                        .into_owned(),
                    env!("CARGO_BIN_EXE_right-wake").into(),
                    dir.path().to_string_lossy().into_owned(),
                ],
                cwd: None,
                timeout_seconds: 5,
            });
        }
        config.save(dir.path()).unwrap();
        std::fs::write(
            dir.path().join("demo-threads.json"),
            serde_json::to_vec(&json!({
                "original-alpha":{"prior_decision":"keep the existing alpha context"},
                "original-beta":{"prior_decision":"keep the existing beta context"},
                "router-thread":{"prior_decision":"dedicated synthetic router"}
            }))
            .unwrap(),
        )
        .unwrap();
        let db = Db::open(dir.path()).unwrap();
        Self {
            engine: Engine::new(dir.path().into(), db),
            _dir: dir,
            config,
        }
    }

    fn path(&self) -> &Path {
        &self.engine.dir
    }

    fn receive(&self, id: &str) -> NormalizedEvent {
        let event = NormalizedEvent {
            event_id: id.into(),
            delivery_id: format!("delivery-{id}"),
            source_id: "source".into(),
            provider: ProviderKind::Linear,
            tenant_id: "tenant".into(),
            resource_key: format!("linear:https://linear.app:tenant:{id}"),
            conversation_key: None,
            actor_id: "human".into(),
            action: "comment.create".into(),
            occurred_at_ms: 100,
            received_at_ms: 100,
            source_url: None,
            content: "@right-wake continue the original work".into(),
            revision: "revision-1".into(),
            actionable: true,
            explicit_trigger: true,
            skip_reason: None,
            reply_target: None,
            linear: None,
        };
        self.engine
            .db
            .receive(&event, id.as_bytes(), &self.config)
            .unwrap();
        event
    }

    fn bind(&self, event: &NormalizedEvent, host: &str, thread: &str) {
        self.engine
            .db
            .bind(
                "source",
                &event.resource_key,
                &target(host, thread),
                "explicit",
                Some(0),
            )
            .unwrap();
    }

    fn calls(&self, host: &str) -> usize {
        std::fs::read_to_string(self.path().join(format!("{host}.calls")))
            .map(|text| text.lines().count())
            .unwrap_or(0)
    }

    async fn command(&self, arguments: &[&str]) -> Value {
        let mut parsed =
            Cli::try_parse_from(std::iter::once("right-wake").chain(arguments.iter().copied()))
                .unwrap();
        parsed.data_dir = Some(self.path().to_owned());
        cli::execute(parsed).await.unwrap().unwrap()
    }
}

fn target(host: &str, thread: &str) -> ThreadTarget {
    ThreadTarget {
        host_id: host.into(),
        thread_id: thread.into(),
        cwd: None,
    }
}

#[tokio::test]
async fn paused_host_holds_its_fifo_and_resume_without_daemon_keeps_original_threads() {
    let fixture = Fixture::new();
    let first = fixture.receive("first");
    let second = fixture.receive("second");
    let independent = fixture.receive("independent");
    fixture.bind(&first, "alpha", "original-alpha");
    fixture.bind(&second, "alpha", "original-alpha");
    fixture.bind(&independent, "beta", "original-beta");
    let bindings = serde_json::to_value(fixture.engine.db.bindings().unwrap()).unwrap();
    fixture.command(&["pause", "--host", "alpha"]).await;
    fixture.command(&["pause", "--host", "alpha"]).await; // idempotent control
    assert_eq!(
        fixture.command(&["status"]).await["paused_hosts"],
        json!(["alpha"])
    );
    assert!(!Config::load(fixture.path()).unwrap().paused);
    fixture.command(&["run-once"]).await;
    assert_eq!(fixture.calls("alpha"), 0);
    assert_eq!(fixture.calls("beta"), 1);
    assert_eq!(
        fixture.engine.db.event_detail("first").unwrap()["state"],
        "held"
    );
    assert_eq!(
        fixture.engine.db.event_detail("first").unwrap()["reason"],
        "host_paused"
    );
    assert_eq!(
        fixture.engine.db.event_detail("second").unwrap()["state"],
        "queued"
    );
    assert!(!fixture.engine.db.has_ready_work().unwrap());
    assert_eq!(
        serde_json::to_value(fixture.engine.db.bindings().unwrap()).unwrap(),
        bindings
    );

    fixture.command(&["resume", "--host", "alpha"]).await;
    assert_eq!(
        fixture.engine.db.event_detail("first").unwrap()["state"],
        "queued"
    );
    let result = fixture.command(&["run-once"]).await;
    assert_eq!(result["processed"], 2);
    assert_eq!(fixture.calls("alpha"), 2);
    assert_eq!(fixture.calls("beta"), 1);
    for id in ["first", "second"] {
        let job = fixture
            .engine
            .db
            .jobs(None, 10)
            .unwrap()
            .into_iter()
            .find(|job| job.event.event_id == id)
            .unwrap();
        assert_eq!(job.state, "completed");
        assert_eq!(job.target, target("alpha", "original-alpha"));
    }
    assert_eq!(
        serde_json::to_value(fixture.engine.db.bindings().unwrap()).unwrap(),
        bindings
    );
}

#[tokio::test]
async fn direct_dispatch_reloads_host_pause_instead_of_trusting_an_old_snapshot() {
    let fixture = Fixture::new();
    let request = DispatchRequest {
        dispatch_id: "direct-attempt".into(),
        target: target("alpha", "original-alpha"),
        prompt: "untrusted event".into(),
        timeout_ms: 1000,
        output_schema: None,
    };
    fixture.command(&["pause", "--host", "alpha"]).await;
    assert!(matches!(
        fixture.engine.dispatch(&fixture.config, &request).await,
        DispatchOutcome::Blocked { reason } if reason.starts_with("host_paused:")
    ));
    assert_eq!(fixture.calls("alpha"), 0);
}

#[tokio::test]
async fn paused_router_retains_its_target_and_resumes_once_without_creating_a_binding() {
    let mut fixture = Fixture::new();
    fixture.config.router = Some(RouterConfig {
        target: target("alpha", "router-thread"),
        source_ids: vec!["source".into()],
        max_candidates: 5,
    });
    fixture.config.save(fixture.path()).unwrap();
    fixture
        .engine
        .db
        .put_card(
            ContextCard {
                id: "candidate".into(),
                target: target("beta", "original-beta"),
                source_ids: vec!["source".into()],
                title: "Existing work".into(),
                summary: "continue the original work".into(),
                owned_resources: vec![],
                referenced_resources: vec![],
                evidence: vec![],
                open_questions: vec![],
                revision: 0,
                coverage: "reviewed-summary".into(),
                enabled: true,
            },
            Some(0),
        )
        .unwrap();
    let event = fixture.receive("unbound");
    fixture.command(&["pause", "--host", "alpha"]).await;
    fixture.command(&["run-once"]).await;
    assert_eq!(fixture.calls("alpha"), 0);
    let state = fixture.engine.db.event_detail(&event.event_id).unwrap();
    assert_eq!(state["state"], "router_waiting");
    assert_eq!(state["reason"], "host_paused");
    assert_eq!(state["routing_attempted"], false);
    let records = fixture.engine.db.router_dispatches().unwrap();
    assert_eq!(records[0]["target"]["thread_id"], "router-thread");

    fixture.command(&["resume", "--host", "alpha"]).await;
    assert_eq!(
        fixture.engine.db.event_detail(&event.event_id).unwrap()["state"],
        "received"
    );
    fixture.command(&["run-once"]).await;
    fixture.command(&["run-once"]).await;
    assert_eq!(fixture.calls("alpha"), 1);
    assert_eq!(fixture.calls("beta"), 0);
    assert_eq!(
        fixture.engine.db.event_detail(&event.event_id).unwrap()["routing_attempted"],
        true
    );
    // This generic synthetic worker does not return a semantic decision; the
    // router records that outcome instead of manufacturing ownership.
    assert!(fixture.engine.db.binding_for(&event).unwrap().is_none());
    assert!(fixture.engine.db.jobs(None, 10).unwrap().is_empty());
}

#[tokio::test]
async fn host_resume_preserves_unknown_outcomes_and_other_scope_controls() {
    let fixture = Fixture::new();
    let unknown = fixture.receive("unknown");
    fixture.bind(&unknown, "alpha", "original-alpha");
    let binding = fixture.engine.db.binding_for(&unknown).unwrap().unwrap();
    let job = fixture
        .engine
        .db
        .enqueue(&unknown, &binding)
        .unwrap()
        .unwrap();
    fixture.engine.db.claim(&job.id).unwrap().unwrap();
    fixture
        .engine
        .db
        .finish_job(&job.id, "unknown", "lost after acceptance", None, false)
        .unwrap();
    fixture.command(&["pause", "--host", "alpha"]).await;
    fixture.command(&["resume", "--host", "alpha"]).await;
    assert_eq!(fixture.engine.db.job(&job.id).unwrap().state, "unknown");

    let held = fixture.receive("source-held");
    fixture.bind(&held, "alpha", "original-beta");
    fixture.command(&["pause", "--source", "source"]).await;
    fixture.command(&["pause", "--host", "alpha"]).await;
    fixture.command(&["resume", "--host", "alpha"]).await;
    fixture.command(&["run-once"]).await;
    assert_eq!(fixture.calls("alpha"), 0);
    assert_eq!(
        fixture.engine.db.event_detail("source-held").unwrap()["state"],
        "held"
    );
    assert!(!Config::load(fixture.path()).unwrap().sources[0].enabled);
    assert_eq!(fixture.engine.db.job(&job.id).unwrap().state, "unknown");

    fixture.command(&["pause"]).await;
    fixture.command(&["pause", "--host", "alpha"]).await;
    fixture.command(&["resume", "--host", "alpha"]).await;
    assert!(Config::load(fixture.path()).unwrap().paused);
    assert_eq!(fixture.command(&["run-once"]).await["processed"], 0);
}

#[test]
fn host_scope_is_exclusive_and_schema_is_generated_from_clap() {
    for verb in ["pause", "resume"] {
        for arguments in [
            vec![verb, "--host", "alpha", "--source", "source"],
            vec![verb, "--host", "alpha", "--resource", "issue"],
            vec![verb, "--resource", "issue"],
        ] {
            assert!(Cli::try_parse_from(std::iter::once("right-wake").chain(arguments)).is_err());
        }
        assert!(Cli::try_parse_from(["right-wake", verb, "--host", "alpha"]).is_ok());
        assert!(Cli::try_parse_from([
            "right-wake",
            verb,
            "--source",
            "source",
            "--resource",
            "issue"
        ])
        .is_ok());
        assert!(Cli::try_parse_from(["right-wake", verb]).is_ok());
    }
    let schema = right_wake::mcp::schema();
    assert_eq!(schema["configuration_defaults"]["paused_hosts"], json!([]));
    for name in ["pause", "resume"] {
        let command = schema["cli"]["subcommands"]
            .as_array()
            .unwrap()
            .iter()
            .find(|command| command["name"] == name)
            .unwrap();
        let host = command["arguments"]
            .as_array()
            .unwrap()
            .iter()
            .find(|argument| argument["long"] == "host")
            .unwrap();
        let conflicts = host["conflicts_with"].as_array().unwrap();
        assert!(conflicts.contains(&json!("source")));
        assert!(conflicts.contains(&json!("resource")));
    }
}

#[tokio::test]
async fn unknown_host_is_rejected_and_legacy_configuration_defaults_to_no_host_pause() {
    let fixture = Fixture::new();
    let mut legacy = serde_json::to_value(&fixture.config).unwrap();
    legacy.as_object_mut().unwrap().remove("paused_hosts");
    let mut config: Config = serde_json::from_value(legacy).unwrap();
    assert!(config.paused_hosts.is_empty());
    config.paused_hosts = vec!["not-registered".into()];
    assert!(config.validate().is_err());
    config.paused_hosts = vec!["alpha".into(), "alpha".into()];
    assert!(config.validate().is_err());
    let mut invalid =
        Cli::try_parse_from(["right-wake", "pause", "--host", "not-registered"]).unwrap();
    invalid.data_dir = Some(fixture.path().into());
    assert!(cli::execute(invalid).await.is_err());
    assert!(Config::load(fixture.path())
        .unwrap()
        .paused_hosts
        .is_empty());
    assert_eq!(fixture.calls("alpha"), 0);
}

#[cfg(unix)]
#[tokio::test]
#[ignore = "native_socket: requires Unix sockets; verifies pause during an actual host handshake"]
async fn pause_during_host_handshake_prevents_resume_and_model_submission() {
    use futures_util::{SinkExt, StreamExt};
    use std::sync::{Arc, Mutex};
    use tokio::sync::oneshot;
    use tokio_tungstenite::tungstenite::Message;

    let mut fixture = Fixture::new();
    // Keep native socket names below Darwin's smaller sockaddr_un limit,
    // independent of the runner's TMPDIR length or /var symlink spelling.
    let socket_dir = tempfile::tempdir_in("/tmp").unwrap();
    let socket = socket_dir.path().join("host.sock");
    let listener = tokio::net::UnixListener::bind(&socket).unwrap();
    fixture.config.hosts[0] = HostConfig::Codex {
        delivery: Default::default(),
        id: "alpha".into(),
        socket,
        exclusive_writer: true,
        restricted_tools: true,
        timeout_seconds: 5,
    };
    fixture.config.save(fixture.path()).unwrap();
    let event = fixture.receive("handshake");
    fixture.bind(&event, "alpha", "original-alpha");
    let binding = fixture.engine.db.binding_for(&event).unwrap().unwrap();
    let job = fixture
        .engine
        .db
        .enqueue(&event, &binding)
        .unwrap()
        .unwrap();
    let claimed = fixture.engine.db.claim(&job.id).unwrap().unwrap();
    let calls = Arc::new(Mutex::new(Vec::<Value>::new()));
    let recorded = calls.clone();
    let (started, waiting) = oneshot::channel();
    let (release, resume) = oneshot::channel();
    let server = tokio::spawn(async move {
        let (stream, _) = listener.accept().await.unwrap();
        let mut websocket = tokio_tungstenite::accept_async(stream).await.unwrap();
        let first = websocket
            .next()
            .await
            .unwrap()
            .unwrap()
            .into_text()
            .unwrap();
        let initialize: Value = serde_json::from_str(&first).unwrap();
        recorded.lock().unwrap().push(initialize.clone());
        assert_eq!(initialize["method"], "initialize");
        started.send(()).unwrap();
        resume.await.unwrap();
        websocket
            .send(Message::Text(
                json!({"id":initialize["id"],"result":{
                    "userAgent":"control-test","platformFamily":"unix","platformOs":"test"
                }})
                .to_string()
                .into(),
            ))
            .await
            .unwrap();
        while let Some(Ok(Message::Text(text))) = websocket.next().await {
            recorded
                .lock()
                .unwrap()
                .push(serde_json::from_str(&text).unwrap());
        }
    });
    let engine = fixture.engine.clone();
    let snapshot = fixture.config.clone();
    let worker = tokio::spawn(async move { engine.run_job(&snapshot, claimed).await });
    tokio::time::timeout(Duration::from_secs(3), waiting)
        .await
        .unwrap()
        .unwrap();
    fixture.command(&["pause", "--host", "alpha"]).await;
    release.send(()).unwrap();
    tokio::time::timeout(Duration::from_secs(3), worker)
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    assert_eq!(fixture.engine.db.job(&job.id).unwrap().state, "held");
    assert_eq!(
        fixture.engine.db.job(&job.id).unwrap().reason.as_deref(),
        Some("host_paused")
    );
    assert!(calls
        .lock()
        .unwrap()
        .iter()
        .all(|call| matches!(call["method"].as_str(), Some("initialize" | "initialized"))));
    server.abort();
}
