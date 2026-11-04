//! Personal Linear scope uses durable evidence, never a polling model or a
//! permanent list of the issues discovered during installation.
use right_wake::{
    config::Config,
    db::{Db, LinearIssueSnapshot, LinearStateImport, MAX_LINEAR_IMPORT_ISSUES},
    engine::Engine,
    model::*,
    provider::{apply_local_policy, linear_personal_candidate},
};
use serde_json::{json, Value};
use tempfile::TempDir;

struct Fixture {
    _dir: TempDir,
    db: Db,
    config: Config,
}
impl Fixture {
    fn new() -> Self {
        let dir = tempfile::tempdir().unwrap();
        let config = Config {
            sources: vec![serde_json::from_value(json!({
                "id":"linear","kind":"linear","tenant_id":"org","secret_env":"TEST_ONLY",
                "linear_assignee_id":"watched"
            }))
            .unwrap()],
            ..Config::default()
        };
        config.save(dir.path()).unwrap();
        Self {
            db: Db::open(dir.path()).unwrap(),
            _dir: dir,
            config,
        }
    }
    fn import(&self, issues: Vec<LinearIssueSnapshot>) -> Value {
        self.db
            .import_linear_state(
                &self.config.sources[0],
                &LinearStateImport {
                    schema_version: 1,
                    organization_id: "org".into(),
                    issues,
                },
            )
            .unwrap()
    }
    fn receive(&self, event: &NormalizedEvent) -> Value {
        self.db
            .receive(event, &serde_json::to_vec(event).unwrap(), &self.config)
            .unwrap()
    }
    fn event(&self, id: &str, issue: &str, at: i64) -> NormalizedEvent {
        NormalizedEvent {
            event_id: id.into(),
            delivery_id: format!("delivery-{id}"),
            source_id: "linear".into(),
            provider: ProviderKind::Linear,
            tenant_id: "org".into(),
            resource_key: format!("linear:https://linear.app:org:{issue}"),
            conversation_key: Some(format!(
                "linear:https://linear.app:org:{issue}:comment:{id}"
            )),
            actor_id: "person".into(),
            action: "comment.create".into(),
            occurred_at_ms: at,
            received_at_ms: now_ms(),
            source_url: None,
            content: "Continue the existing task without a mention".into(),
            revision: format!("body-{id}"),
            actionable: true,
            explicit_trigger: true,
            skip_reason: None,
            reply_target: Some(ReplyTarget {
                provider: ProviderKind::Linear,
                container_id: "org".into(),
                resource_id: issue.into(),
                comment_id: Some(id.into()),
                discussion_id: None,
            }),
            linear: Some(LinearEventMetadata {
                issue_id: issue.into(),
                issue_title: Some("Issue".into()),
                assignee: LinearAssignee::Unknown,
                previous_assignee: LinearAssignee::Unknown,
                issue_updated_at_ms: None,
            }),
        }
    }
    fn issue(
        &self,
        id: &str,
        issue: &str,
        at: i64,
        owner: LinearAssignee,
        previous: LinearAssignee,
        action: &str,
    ) -> NormalizedEvent {
        let mut event = self.event(id, issue, at);
        event.action = action.into();
        event.conversation_key = None;
        event.reply_target.as_mut().unwrap().comment_id = None;
        let metadata = event.linear.as_mut().unwrap();
        metadata.assignee = owner;
        metadata.previous_assignee = previous;
        metadata.issue_updated_at_ms = Some(at);
        event.actionable = linear_personal_candidate(&event, "watched");
        event.explicit_trigger = event.actionable;
        apply_local_policy(&self.config.sources[0], &mut event).unwrap();
        event
    }
    fn bind_and_enqueue(&self, event: &NormalizedEvent) -> Job {
        let target = ThreadTarget {
            host_id: "synthetic".into(),
            thread_id: "original".into(),
            cwd: None,
        };
        let binding = self.db.binding_for(event).unwrap().unwrap_or_else(|| {
            self.db
                .bind("linear", &event.resource_key, &target, "explicit", Some(0))
                .unwrap()
        });
        self.db.enqueue(event, &binding).unwrap().unwrap()
    }
}
fn assigned(id: &str) -> LinearAssignee {
    LinearAssignee::Assigned { user_id: id.into() }
}
fn snapshot(issue: &str, owner: LinearAssignee, at: i64) -> LinearIssueSnapshot {
    LinearIssueSnapshot {
        issue_id: issue.into(),
        assignee: owner,
        updated_at_ms: at,
    }
}

#[tokio::test]
async fn unknown_comments_survive_restart_without_model_and_partial_import_resumes_once() {
    let f = Fixture::new();
    let comment = f.event("comment", "issue", 200);
    assert_eq!(f.receive(&comment)["state"], "waiting_assignment");
    let reopened = Db::open(f._dir.path()).unwrap();
    let engine = Engine::new(f._dir.path().into(), reopened);
    assert_eq!(engine.run_once().await.unwrap(), 0);
    assert!(!engine.db.has_ready_work().unwrap());
    assert!(engine.db.jobs(None, 10).unwrap().is_empty());
    assert!(engine.db.event("comment").unwrap().actionable);
    assert!(engine.db.event("comment").unwrap().explicit_trigger);
    assert_eq!(
        f.import(vec![snapshot("issue", assigned("watched"), 100)])["awakened"],
        1
    );
    assert_eq!(f.db.event_detail("comment").unwrap()["state"], "received");
    assert_eq!(
        f.import(vec![snapshot("issue", assigned("watched"), 100)])["changed"],
        0
    );
    assert_eq!(
        f.import(vec![snapshot("issue", assigned("watched"), 100)])["awakened"],
        0
    );
}

#[test]
fn ordered_snapshots_conflicts_and_missing_owner_are_distinct() {
    let f = Fixture::new();
    f.import(vec![snapshot("issue", assigned("watched"), 100)]);
    f.receive(&f.event("comment", "issue", 400));
    assert_eq!(
        f.import(vec![snapshot("issue", assigned("other"), 90)])["changed"],
        0
    );
    f.import(vec![snapshot("issue", assigned("other"), 100)]);
    assert_eq!(
        f.db.event_detail("comment").unwrap()["state"],
        "waiting_assignment"
    );
    assert_eq!(
        f.db.linear_state(&f.config.sources[0]).unwrap()["issues"][0]["conflicted"],
        true
    );
    f.import(vec![snapshot("issue", assigned("watched"), 100)]);
    assert_eq!(
        f.db.event_detail("comment").unwrap()["state"],
        "waiting_assignment"
    );
    f.import(vec![snapshot("issue", assigned("watched"), 110)]);
    assert_eq!(f.db.event_detail("comment").unwrap()["state"], "received");
    f.receive(&f.issue(
        "unknown-owner",
        "issue",
        120,
        LinearAssignee::Unknown,
        LinearAssignee::Unknown,
        "issue.update",
    ));
    assert_eq!(
        f.db.event_detail("comment").unwrap()["state"],
        "waiting_assignment"
    );
    f.import(vec![snapshot("issue", LinearAssignee::Unassigned, 120)]);
    assert_eq!(
        f.db.event_detail("comment").unwrap()["state"],
        "outside_assignment"
    );
}

#[test]
fn malformed_issue_revision_invalidates_stale_positive_membership_until_new_evidence() {
    let f = Fixture::new();
    f.import(vec![snapshot("issue", assigned("watched"), 100)]);
    let mut malformed = f.issue(
        "malformed",
        "issue",
        200,
        assigned("other"),
        assigned("watched"),
        "issue.update",
    );
    malformed.linear.as_mut().unwrap().issue_updated_at_ms = None;
    f.receive(&malformed);
    assert_eq!(
        f.receive(&f.event("comment", "issue", 300))["state"],
        "waiting_assignment"
    );
    let state = f.db.linear_state(&f.config.sources[0]).unwrap();
    assert_eq!(state["issues"][0]["updated_at_ms"], 100);
    assert_eq!(state["issues"][0]["conflicted"], true);
    f.import(vec![snapshot("issue", assigned("watched"), 100)]);
    assert_eq!(
        f.db.event_detail("comment").unwrap()["state"],
        "waiting_assignment"
    );
    f.import(vec![snapshot("issue", assigned("watched"), 101)]);
    assert_eq!(f.db.event_detail("comment").unwrap()["state"], "received");
}

#[test]
fn only_authenticated_late_assignment_revives_known_nonmember_comments() {
    for evidence in ["late", "new", "import"] {
        let f = Fixture::new();
        f.import(vec![snapshot("issue", assigned("other"), 100)]);
        f.receive(&f.event("comment", "issue", 200));
        assert_eq!(
            f.db.event_detail("comment").unwrap()["state"],
            "outside_assignment"
        );
        if evidence == "import" {
            f.import(vec![snapshot("issue", assigned("watched"), 150)]);
        } else {
            f.receive(&f.issue(
                "assignment",
                "issue",
                if evidence == "late" { 150 } else { 300 },
                assigned("watched"),
                assigned("other"),
                "issue.update",
            ));
        }
        assert_eq!(
            f.db.event_detail("comment").unwrap()["state"],
            if evidence == "late" {
                "received"
            } else {
                "outside_assignment"
            }
        );
    }
}

#[test]
fn older_assignment_interval_and_remove_tombstone_cannot_start_work() {
    let f = Fixture::new();
    f.import(vec![snapshot("issue", assigned("other"), 200)]);
    f.receive(&f.issue(
        "current",
        "issue",
        300,
        assigned("watched"),
        assigned("other"),
        "issue.update",
    ));
    f.receive(&f.issue(
        "older",
        "issue",
        100,
        assigned("watched"),
        LinearAssignee::Unassigned,
        "issue.update",
    ));
    assert_eq!(
        f.db.event_detail("older").unwrap()["state"],
        "outside_assignment"
    );
    let comment = f.event("comment", "issue", 400);
    f.receive(&comment);
    let job = f.bind_and_enqueue(&comment);
    f.receive(&f.issue(
        "removed",
        "issue",
        300,
        assigned("watched"),
        LinearAssignee::Unknown,
        "issue.remove",
    ));
    assert!(f.db.claim(&job.id).unwrap().is_none());
    assert_eq!(f.db.job(&job.id).unwrap().state, "outside_assignment");
    f.import(vec![snapshot("issue", assigned("watched"), 300)]);
    assert_eq!(
        f.db.linear_state(&f.config.sources[0]).unwrap()["issues"][0]["removed"],
        true
    );
    f.import(vec![snapshot("issue", assigned("watched"), 301)]);
    assert_eq!(f.db.job(&job.id).unwrap().state, "outside_assignment");
}

#[test]
fn partial_import_is_bounded_typed_atomic_and_never_unassigns_omissions() {
    let f = Fixture::new();
    f.import(vec![
        snapshot("one", assigned("watched"), 100),
        snapshot("two", assigned("watched"), 100),
    ]);
    f.import(vec![snapshot("one", LinearAssignee::Unassigned, 200)]);
    let state = f.db.linear_state(&f.config.sources[0]).unwrap();
    assert_eq!(state["issues"][1]["assignee"]["user_id"], "watched");
    assert!(serde_json::from_value::<LinearStateImport>(
        json!({"schema_version":1,"organization_id":"org","issues":[],"complete":true})
    )
    .is_err());
    for (org, issue) in [
        ("other-org", snapshot("bad", assigned("watched"), 300)),
        ("org", snapshot("bad", LinearAssignee::Unknown, 300)),
        ("org", snapshot("bad\0id", assigned("watched"), 300)),
        ("org", snapshot("bad", assigned("user\0id"), 300)),
        ("org", snapshot("bad", assigned("watched"), 0)),
    ] {
        assert!(f
            .db
            .import_linear_state(
                &f.config.sources[0],
                &LinearStateImport {
                    schema_version: 1,
                    organization_id: org.into(),
                    issues: vec![snapshot("one", assigned("watched"), 400), issue],
                }
            )
            .is_err());
    }
    assert_eq!(f.db.linear_state(&f.config.sources[0]).unwrap(), state);
    assert!(f
        .db
        .import_linear_state(
            &f.config.sources[0],
            &LinearStateImport {
                schema_version: 1,
                organization_id: "org".into(),
                issues: vec![
                    snapshot("oversized", assigned("watched"), 500);
                    MAX_LINEAR_IMPORT_ISSUES + 1
                ],
            }
        )
        .is_err());
}

#[test]
fn imports_preserve_source_pause_and_never_upgrade_static_or_bot_denials() {
    let mut f = Fixture::new();
    f.config.sources[0].bot_actor_ids.push("bot".into());
    f.config.sources[0].enabled = false;
    f.config.save(f._dir.path()).unwrap();
    let mut denied = f.event("denied", "issue", 200);
    denied.actor_id = "bot".into();
    apply_local_policy(&f.config.sources[0], &mut denied).unwrap();
    f.receive(&denied);
    f.receive(&f.event("waiting", "issue", 200));
    assert_eq!(
        f.import(vec![snapshot("issue", assigned("watched"), 100)])["awakened"],
        0
    );
    assert_eq!(f.db.event_detail("denied").unwrap()["state"], "cached");
    assert_eq!(
        f.db.event_detail("waiting").unwrap()["state"],
        "waiting_assignment"
    );
    f.config.sources[0].enabled = true;
    f.config.save(f._dir.path()).unwrap();
    f.db.wake_waiters(None).unwrap();
    assert_eq!(f.db.event_detail("waiting").unwrap()["state"], "received");
    assert_eq!(f.db.event_detail("denied").unwrap()["state"], "cached");
    assert!(!f.db.event("denied").unwrap().actionable);
}

#[tokio::test]
async fn onboarding_gap_refreshes_current_and_previously_tracked_owners_before_resume() {
    let mut f = Fixture::new();
    f.config.paused = true;
    f.config.save(f._dir.path()).unwrap();
    f.import(vec![
        snapshot("still-mine", assigned("watched"), 100),
        snapshot("formerly-mine", assigned("watched"), 100),
    ]);
    let kept = f.event("kept-comment", "still-mine", 400);
    let removed = f.event("removed-comment", "formerly-mine", 400);
    let discovered = f.event("new-comment", "newly-discovered", 400);
    for event in [&kept, &removed, &discovered] {
        f.receive(event);
    }
    let engine = Engine::new(f._dir.path().into(), f.db.clone());
    assert_eq!(engine.run_once().await.unwrap(), 0);
    assert!(f.db.jobs(None, 10).unwrap().is_empty());

    // The current assigned-to-me query alone cannot clear old membership.
    f.import(vec![
        snapshot("still-mine", assigned("watched"), 300),
        snapshot("newly-discovered", assigned("watched"), 300),
    ]);
    assert_eq!(
        f.db.event_detail(&removed.event_id).unwrap()["state"],
        "received"
    );
    // Refresh every previous ID explicitly before lifting the execution pause.
    f.import(vec![snapshot("formerly-mine", assigned("other"), 300)]);
    assert_eq!(
        f.db.event_detail(&removed.event_id).unwrap()["state"],
        "outside_assignment"
    );
    assert!(Config::load(f._dir.path()).unwrap().paused);
    assert_eq!(engine.run_once().await.unwrap(), 0);

    // Restart the store and prove only the two current owners are admissible.
    let reopened = Db::open(f._dir.path()).unwrap();
    assert_eq!(
        reopened.event_detail(&kept.event_id).unwrap()["state"],
        "received"
    );
    assert_eq!(
        reopened.event_detail(&discovered.event_id).unwrap()["state"],
        "received"
    );
    assert_eq!(
        reopened.event_detail(&removed.event_id).unwrap()["state"],
        "outside_assignment"
    );
    assert!(reopened.jobs(None, 10).unwrap().is_empty());
    // No real host or provider write participates in this recovery contract.
}

#[test]
fn late_evidence_commits_at_capacity_and_completion_promotes_a_bounded_backlog() {
    let mut f = Fixture::new();
    f.config.max_pending = 1;
    f.config.save(f._dir.path()).unwrap();
    f.import(vec![
        snapshot("active", assigned("watched"), 100),
        snapshot("issue", assigned("other"), 100),
    ]);
    let active = f.event("active-comment", "active", 200);
    f.receive(&active);
    let job = f.bind_and_enqueue(&active);
    f.db.claim(&job.id).unwrap().unwrap();
    for id in ["outside-one", "outside-two"] {
        assert_eq!(
            f.receive(&f.event(id, "issue", 200))["state"],
            "outside_assignment"
        );
    }
    let assignment = f.issue(
        "late-assignment",
        "issue",
        150,
        assigned("watched"),
        assigned("other"),
        "issue.update",
    );
    assert_eq!(f.receive(&assignment)["state"], "waiting_capacity");
    assert_eq!(f.db.events(Some("waiting_capacity"), 10).unwrap().len(), 3);
    assert!(!f.db.has_ready_work().unwrap());
    f.db.finish_job(
        &job.id,
        "completed",
        "synthetic completion",
        Some("turn"),
        true,
    )
    .unwrap();
    for remaining in (0..3).rev() {
        let ready = f.db.events(Some("received"), 10).unwrap();
        assert_eq!(ready.len(), 1);
        assert_eq!(
            f.db.events(Some("waiting_capacity"), 10).unwrap().len(),
            remaining
        );
        let event: NormalizedEvent = serde_json::from_value(ready[0]["event"].clone()).unwrap();
        let job = f.bind_and_enqueue(&event);
        f.db.claim(&job.id).unwrap().unwrap();
        f.db.finish_job(
            &job.id,
            "completed",
            "synthetic completion",
            Some("turn"),
            true,
        )
        .unwrap();
    }
    assert!(!f.db.has_ready_work().unwrap());
}

#[test]
fn dispatch_context_is_immutable_and_reads_are_separately_recorded() {
    let f = Fixture::new();
    let target = ThreadTarget {
        host_id: "host".into(),
        thread_id: "original".into(),
        cwd: None,
    };
    let prompt =
        json!({"operation":"continue_existing_work","event":{"content":"untrusted remote text"}})
            .to_string();
    let schema = json!({"type":"object"});
    f.db.put_dispatch_context("dispatch", &target, &prompt, Some(&schema))
        .unwrap();
    f.db.put_dispatch_context("dispatch", &target, &prompt, Some(&schema))
        .unwrap();
    assert!(!f.db.dispatch_context_read("dispatch").unwrap());
    let context = f.db.dispatch_context("dispatch").unwrap();
    assert_eq!(context["target"]["thread_id"], "original");
    assert_eq!(
        context["payload"]["event"]["content"],
        "untrusted remote text"
    );
    assert!(!f.db.dispatch_context_read("dispatch").unwrap());
    f.db.mark_dispatch_context_read("dispatch").unwrap();
    f.db.mark_dispatch_context_read("dispatch").unwrap();
    assert!(f.db.dispatch_context_read("dispatch").unwrap());
    assert!(f
        .db
        .put_dispatch_context("dispatch", &target, "{}", Some(&schema))
        .is_err());
    assert!(f
        .db
        .put_dispatch_context("dispatch", &target, &prompt, None)
        .is_err());
    let mut other = target;
    other.thread_id = "another".into();
    assert!(f
        .db
        .put_dispatch_context("dispatch", &other, &prompt, Some(&schema))
        .is_err());
    assert_eq!(f.db.dispatch_context("dispatch").unwrap(), context);
    assert!(f.db.mark_dispatch_context_read("unknown").is_err());
}

#[cfg(unix)]
#[tokio::test]
#[ignore = "native_socket: requires Unix sockets; verifies reassignment during actual host handshake"]
async fn reassignment_during_host_handshake_prevents_resume_and_submission() {
    use futures_util::{SinkExt, StreamExt};
    use right_wake::config::HostConfig;
    use std::{
        sync::{Arc, Mutex},
        time::Duration,
    };
    use tokio::sync::oneshot;
    use tokio_tungstenite::tungstenite::Message;
    let mut f = Fixture::new();
    let socket_dir = tempfile::tempdir_in("/tmp").unwrap();
    let socket = socket_dir.path().join("host.sock");
    let listener = tokio::net::UnixListener::bind(&socket).unwrap();
    f.config.hosts.push(HostConfig::Codex {
        id: "synthetic".into(),
        socket,
        delivery: Default::default(),
        exclusive_writer: true,
        restricted_tools: true,
        timeout_seconds: 5,
    });
    f.config.save(f._dir.path()).unwrap();
    f.import(vec![snapshot("issue", assigned("watched"), 100)]);
    let comment = f.event("comment", "issue", 200);
    f.receive(&comment);
    let job = f.bind_and_enqueue(&comment);
    let claimed = f.db.claim(&job.id).unwrap().unwrap();
    let calls = Arc::new(Mutex::new(Vec::<Value>::new()));
    let recorded = calls.clone();
    let (started, waiting) = oneshot::channel();
    let (release, resume) = oneshot::channel();
    let server = tokio::spawn(async move {
        let (stream, _) = listener.accept().await.unwrap();
        let mut socket = tokio_tungstenite::accept_async(stream).await.unwrap();
        let text = socket.next().await.unwrap().unwrap().into_text().unwrap();
        let initialize: Value = serde_json::from_str(&text).unwrap();
        recorded.lock().unwrap().push(initialize.clone());
        assert_eq!(initialize["method"], "initialize");
        started.send(()).unwrap();
        resume.await.unwrap();
        socket
            .send(Message::Text(
                json!({"id":initialize["id"],"result":{
                    "userAgent":"assignment-test","platformFamily":"unix","platformOs":"test"
                }})
                .to_string()
                .into(),
            ))
            .await
            .unwrap();
        while let Some(Ok(Message::Text(text))) = socket.next().await {
            recorded
                .lock()
                .unwrap()
                .push(serde_json::from_str(&text).unwrap());
        }
    });
    let engine = Engine::new(f._dir.path().into(), f.db.clone());
    let config = f.config.clone();
    let worker = tokio::spawn(async move { engine.run_job(&config, claimed).await });
    tokio::time::timeout(Duration::from_secs(3), waiting)
        .await
        .unwrap()
        .unwrap();
    f.receive(&f.issue(
        "reassigned",
        "issue",
        300,
        assigned("other"),
        assigned("watched"),
        "issue.update",
    ));
    release.send(()).unwrap();
    tokio::time::timeout(Duration::from_secs(3), worker)
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    assert_eq!(f.db.job(&job.id).unwrap().state, "outside_assignment");
    assert!(f.db.drafts().unwrap().is_empty());
    assert!(calls
        .lock()
        .unwrap()
        .iter()
        .all(|call| matches!(call["method"].as_str(), Some("initialize" | "initialized"))));
    server.abort();
}
