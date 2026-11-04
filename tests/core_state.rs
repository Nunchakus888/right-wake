use right_wake::{
    config::{Config, HostConfig},
    db::Db,
    engine::Engine,
    model::*,
    provider::SourceConfig,
};
use serde_json::json;
use std::sync::{Arc, Barrier};
use tempfile::TempDir;

fn target(thread: &str) -> ThreadTarget {
    ThreadTarget {
        host_id: "host".into(),
        thread_id: thread.into(),
        cwd: None,
    }
}
fn source() -> SourceConfig {
    serde_json::from_value(json!({"id":"source","kind":"github","secret_env":"TEST_SECRET"}))
        .unwrap()
}
fn event(id: &str, resource: &str) -> NormalizedEvent {
    NormalizedEvent {
        event_id: id.into(),
        delivery_id: format!("delivery-{id}"),
        source_id: "source".into(),
        provider: ProviderKind::Github,
        tenant_id: "tenant".into(),
        resource_key: resource.into(),
        conversation_key: None,
        actor_id: "human".into(),
        action: "created".into(),
        occurred_at_ms: 100,
        received_at_ms: 100,
        source_url: None,
        content: "@right-wake follow up".into(),
        revision: "original-source-provenance".into(),
        actionable: true,
        explicit_trigger: true,
        skip_reason: None,
        reply_target: Some(ReplyTarget {
            provider: ProviderKind::Github,
            container_id: "example/repo".into(),
            resource_id: "1".into(),
            comment_id: None,
            discussion_id: None,
        }),
        linear: None,
    }
}
fn setup() -> (TempDir, Db, Config) {
    let dir = tempfile::tempdir().unwrap();
    let db = Db::open(dir.path()).unwrap();
    let cfg = Config {
        sources: vec![source()],
        ..Config::default()
    };
    cfg.save(dir.path()).unwrap();
    (dir, db, cfg)
}
fn receive(db: &Db, cfg: &Config, event: &NormalizedEvent) {
    db.receive(event, event.event_id.as_bytes(), cfg).unwrap();
}
fn enqueue(db: &Db, cfg: &Config, event: &NormalizedEvent, target: &ThreadTarget) -> Job {
    receive(db, cfg, event);
    let binding = db
        .bind(
            &event.source_id,
            &event.resource_key,
            target,
            "explicit",
            None,
        )
        .unwrap();
    db.enqueue(event, &binding).unwrap().unwrap()
}

#[test]
fn linked_sqlite_includes_the_upstream_wal_reset_fix() {
    let connection = rusqlite::Connection::open_in_memory().unwrap();
    let (version, source_id): (String, String) = connection
        .query_row("SELECT sqlite_version(), sqlite_source_id()", [], |row| {
            Ok((row.get(0)?, row.get(1)?))
        })
        .unwrap();
    println!("linked SQLite {version}: {source_id}");
    assert_eq!(version, rusqlite::version());
    assert!(
        rusqlite::version_number() >= 3_051_003,
        "SQLite {version} predates the 3.51.3 mainline WAL-reset fix"
    );
    assert!(
        rusqlite::version_number() >= right_wake::db::MIN_SQLITE_VERSION_NUMBER,
        "linked engine does not satisfy RightWake's startup requirement"
    );
}

#[test]
fn receipt_deduplication_and_conflicting_raw_bytes_are_distinct() {
    let (_dir, db, cfg) = setup();
    let e = event("e1", "r1");
    receive(&db, &cfg, &e);
    assert_eq!(db.receive(&e, b"e1", &cfg).unwrap()["state"], "duplicate");
    assert!(db
        .receive(&e, b"tampered", &cfg)
        .unwrap_err()
        .to_string()
        .contains("delivery_conflict"));
    let mut redelivery = e.clone();
    redelivery.delivery_id = "another-delivery".into();
    assert_eq!(
        db.receive(&redelivery, b"new-envelope", &cfg).unwrap()["state"],
        "duplicate"
    );
    assert_eq!(db.resource_revision("source", "r1").unwrap(), "1");
    assert_eq!(db.event_detail("e1").unwrap()["source_version"], 1);
}

#[test]
fn fifo_holds_followups_behind_busy_and_unknown_work() {
    let (_dir, db, cfg) = setup();
    let t = target("thread");
    let first = enqueue(&db, &cfg, &event("first", "r1"), &t);
    let second = enqueue(&db, &cfg, &event("second", "r2"), &t);
    assert!(db.claim(&second.id).unwrap().is_none());
    let claimed = db.claim(&first.id).unwrap().unwrap();
    assert!(claimed.dispatch_id.is_some());
    db.finish_job(&first.id, "waiting_busy", "thread active", None, true)
        .unwrap();
    assert!(db.claim(&second.id).unwrap().is_none());
    assert!(!db.has_ready_work().unwrap());
    db.wake_waiters(Some("host")).unwrap();
    db.claim(&first.id).unwrap().unwrap();
    db.finish_job(&first.id, "unknown", "connection lost", None, false)
        .unwrap();
    assert!(db.claim(&second.id).unwrap().is_none());
    assert!(!db.has_ready_work().unwrap());
    assert!(db.retry(&first.id).is_err());
    db.reconcile_not_started(&first.id, "host log proves dispatch was not accepted")
        .unwrap();
    assert!(db.claim(&second.id).unwrap().is_some());
}

#[test]
fn concurrent_claims_have_one_intent_and_retry_cannot_overwrite_it() {
    let (_dir, db, cfg) = setup();
    let job = enqueue(&db, &cfg, &event("e1", "r1"), &target("thread"));
    let barrier = Arc::new(Barrier::new(8));
    let mut handles = vec![];
    for _ in 0..8 {
        let db = db.clone();
        let id = job.id.clone();
        let barrier = barrier.clone();
        handles.push(std::thread::spawn(move || {
            barrier.wait();
            db.claim(&id).unwrap().is_some()
        }));
    }
    assert_eq!(
        handles
            .into_iter()
            .map(|handle| handle.join().unwrap())
            .filter(|claimed| *claimed)
            .count(),
        1
    );
    let before = db.job(&job.id).unwrap();
    assert!(db.retry(&job.id).is_err());
    let after = db.job(&job.id).unwrap();
    assert_eq!(after.state, "dispatching");
    assert_eq!(after.dispatch_id, before.dispatch_id);
    assert_eq!(db.event_detail("e1").unwrap()["state"], "dispatching");
}

#[test]
fn rebind_and_pause_synchronize_event_job_and_draft_versions() {
    let (_dir, db, cfg) = setup();
    let e = event("e1", "r1");
    let job = enqueue(&db, &cfg, &e, &target("old"));
    let draft = db.save_draft(&job, "draft", "17").unwrap().unwrap();
    db.bind("source", "r1", &target("new"), "explicit", Some(1))
        .unwrap();
    assert_eq!(db.job(&job.id).unwrap().state, "binding_changed");
    assert_eq!(db.event_detail("e1").unwrap()["state"], "binding_changed");
    assert_eq!(db.draft(&draft.id).unwrap().state, "stale");
    assert!(db.begin_send(&draft.id).is_err());
    db.retry(&job.id).unwrap();
    let moved = db.job(&job.id).unwrap();
    assert_eq!(moved.target.thread_id, "new");
    assert_eq!(moved.binding_version, 2);
    db.pause_binding("source", "r1", true).unwrap();
    assert_eq!(db.event_detail("e1").unwrap()["state"], "binding_changed");
    assert!(db.retry(&job.id).is_err());
}

#[test]
fn discussion_binding_overrides_resource_fallback_and_invalidates_old_draft() {
    let (_dir, db, cfg) = setup();
    let mut e = event("e1", "r1");
    e.conversation_key = Some("discussion".into());
    let job = enqueue(&db, &cfg, &e, &target("resource-thread"));
    let draft = db.save_draft(&job, "old", "17").unwrap().unwrap();
    db.bind(
        "source",
        "discussion",
        &target("discussion-thread"),
        "explicit",
        Some(0),
    )
    .unwrap();
    assert!(!db.job_binding_current(&job).unwrap());
    assert_eq!(db.event_detail("e1").unwrap()["state"], "binding_changed");
    assert!(db.begin_send(&draft.id).is_err());
    db.retry(&job.id).unwrap();
    assert_eq!(db.job(&job.id).unwrap().binding_key, "discussion");
}

#[test]
fn stale_source_drafts_preserve_provenance_through_edits() {
    let (_dir, db, cfg) = setup();
    let job = enqueue(&db, &cfg, &event("e1", "r1"), &target("thread"));
    receive(&db, &cfg, &event("e2", "r1"));
    let draft = db
        .save_draft(&job, "based on first event", "17")
        .unwrap()
        .unwrap();
    assert_eq!(draft.state, "stale");
    let edited = db.edit_draft(&draft.id, "human-edited answer").unwrap();
    assert_eq!(edited.state, "stale");
    assert_eq!(edited.revision, 2);
    assert_eq!(edited.source_revision, "1");
    assert_eq!(edited.source_event_revision, "original-source-provenance");
    assert!(db.begin_send(&draft.id).is_err());
    db.draft_state(&draft.id, "unknown", "provider write uncertain", &json!({}))
        .unwrap();
    assert!(db.edit_draft(&draft.id, "try again").is_err());
}

#[test]
fn ownership_is_exact_scoped_and_created_only_by_explicit_binding() {
    let (_dir, db, _cfg) = setup();
    db.bind("source", "r1", &target("a"), "explicit", None)
        .unwrap();
    let mut card = db.card("host:a").unwrap();
    assert!(card.evidence.contains(&"ownership:source:r1".into()));
    card.owned_resources.push("r2".into());
    assert!(db.put_card(card, None).is_err());
    let mut card = db.card("host:a").unwrap();
    card.source_ids.push("untrusted-source".into());
    assert!(db.put_card(card, None).is_err());
    assert_eq!(db.owners(&event("e1", "r1")).unwrap().len(), 1);
    let mut other = event("other", "r1");
    other.source_id = "untrusted-source".into();
    assert!(db.owners(&other).unwrap().is_empty());
    db.bind("source", "r1", &target("b"), "explicit", Some(1))
        .unwrap();
    assert!(db.card("host:a").unwrap().owned_resources.is_empty());
    assert_eq!(
        db.owners(&event("e1", "r1")).unwrap()[0].target.thread_id,
        "b"
    );
}

#[test]
fn ownership_conflicts_are_counted_before_top_k_truncation() {
    let (dir, db, _cfg) = setup();
    db.bind("source", "r1", &target("a"), "explicit", None)
        .unwrap();
    // A migration fixture represents preexisting conflicting trusted evidence.
    let mut card = db.card("host:a").unwrap();
    card.id = "host:b".into();
    card.target = target("b");
    rusqlite::Connection::open(dir.path().join("state.sqlite3"))
        .unwrap()
        .execute(
            "INSERT INTO cards VALUES(?1,?2,?3)",
            rusqlite::params![
                card.id,
                serde_json::to_string(&card).unwrap(),
                card.revision
            ],
        )
        .unwrap();
    assert_eq!(db.candidates(&event("e1", "r1"), 1).unwrap().len(), 1);
    assert_eq!(db.owners(&event("e1", "r1")).unwrap().len(), 2);
}

#[test]
fn router_crash_keeps_target_lock_until_operator_reconciliation() {
    let (_dir, db, cfg) = setup();
    receive(&db, &cfg, &event("route1", "unbound1"));
    assert!(db
        .begin_router("route1", &target("router"))
        .unwrap()
        .is_some());
    assert_eq!(db.recover().unwrap(), 1);
    assert_eq!(
        db.event_detail("route1").unwrap()["state"],
        "router_unknown"
    );
    receive(&db, &cfg, &event("route2", "unbound2"));
    assert!(db
        .begin_router("route2", &target("router"))
        .unwrap()
        .is_none());
    let job = enqueue(&db, &cfg, &event("work", "work"), &target("router"));
    assert!(db.claim(&job.id).unwrap().is_none());
    assert!(!db.has_ready_work().unwrap());
    assert_eq!(db.status().unwrap()["router_dispatches"]["unknown"], 1);
    assert!(db.reconcile_router_not_started("route1", "vague").is_err());
    db.reconcile_router_not_started("route1", "host transcript confirms dispatch absent")
        .unwrap();
    assert_eq!(db.event_detail("route1").unwrap()["state"], "received");
    assert_eq!(db.event_detail("route2").unwrap()["state"], "received");
    assert!(db.claim(&job.id).unwrap().is_some());
}

#[test]
fn router_busy_releases_lock_but_does_not_spin_until_a_host_event() {
    let (_dir, db, cfg) = setup();
    receive(&db, &cfg, &event("route", "r1"));
    db.begin_router("route", &target("router"))
        .unwrap()
        .unwrap();
    db.finish_router("route", "waiting", "thread active", None)
        .unwrap();
    assert!(!db.has_ready_work().unwrap());
    assert_eq!(
        db.event_detail("route").unwrap()["routing_attempted"],
        false
    );
    db.wake_waiters(Some("host")).unwrap();
    assert!(db.has_ready_work().unwrap());
}

#[test]
fn relay_authorization_precedes_limit_and_ack_phase_never_regresses() {
    let (_dir, db, cfg) = setup();
    for n in 0..6 {
        let mut e = event(&format!("other{n}"), "r1");
        e.source_id = "other".into();
        receive(&db, &cfg, &e);
    }
    receive(&db, &cfg, &event("allowed", "r2"));
    let events = db.relay_pending("device", &["source".into()], 1).unwrap();
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].event_id, "allowed");
    assert_eq!(db.relay_phase("device", "allowed").unwrap(), None);
    db.relay_ack("device", "allowed", "received").unwrap();
    db.relay_ack("device", "allowed", "executed").unwrap();
    db.relay_ack("device", "allowed", "received").unwrap();
    assert_eq!(
        db.relay_phase("device", "allowed").unwrap().as_deref(),
        Some("executed")
    );
    assert!(db
        .relay_pending("device", &["source".into()], 1)
        .unwrap()
        .is_empty());
}

#[test]
fn completion_receipts_only_include_scoped_durable_terminal_work() {
    let (_dir, db, cfg) = setup();
    let first = enqueue(&db, &cfg, &event("done", "r1"), &target("a"));
    db.claim(&first.id).unwrap().unwrap();
    db.finish_job(&first.id, "drafted", "completed", Some("turn"), true)
        .unwrap();
    let unknown = enqueue(&db, &cfg, &event("uncertain", "r2"), &target("b"));
    db.claim(&unknown.id).unwrap().unwrap();
    db.finish_job(&unknown.id, "unknown", "lost", None, false)
        .unwrap();
    assert_eq!(
        db.completion_receipts(&["source".into()], 0, 1).unwrap(),
        vec!["done"]
    );
    assert!(db
        .completion_receipts(&["source".into()], 1, 1)
        .unwrap()
        .is_empty());
    assert!(db
        .completion_receipts(&["other".into()], 0, 1)
        .unwrap()
        .is_empty());
}

#[tokio::test]
async fn finite_ingress_batches_notify_until_drained_without_model_calls() {
    let (dir, db, cfg) = setup();
    for n in 0..205 {
        let mut e = event(&format!("event{n}"), "resource");
        e.explicit_trigger = false;
        receive(&db, &cfg, &e);
    }
    let engine = Engine::new(dir.path().into(), db.clone());
    assert_eq!(engine.run_once().await.unwrap(), 0);
    assert_eq!(db.events(Some("received"), 1000).unwrap().len(), 5);
    tokio::time::timeout(std::time::Duration::from_millis(50), engine.wake.notified())
        .await
        .expect("remaining finite batch must schedule itself");
    assert_eq!(engine.run_once().await.unwrap(), 0);
    assert!(!db.has_ready_work().unwrap());
    assert!(db.jobs(None, 1000).unwrap().is_empty());
}

#[tokio::test]
async fn restricted_tools_gate_prevents_host_contact_and_busy_spin() {
    let (dir, db, mut cfg) = setup();
    cfg.hosts.push(HostConfig::Codex {
        delivery: Default::default(),
        id: "host".into(),
        socket: "/nonexistent/socket".into(),
        exclusive_writer: true,
        restricted_tools: false,
        timeout_seconds: 1,
    });
    cfg.save(dir.path()).unwrap();
    let first = enqueue(&db, &cfg, &event("e1", "r1"), &target("thread"));
    let second = enqueue(&db, &cfg, &event("e2", "r2"), &target("thread"));
    let engine = Engine::new(dir.path().into(), db.clone());
    assert_eq!(engine.run_once().await.unwrap(), 1);
    assert_eq!(db.job(&first.id).unwrap().state, "held");
    assert!(db
        .job(&first.id)
        .unwrap()
        .reason
        .unwrap()
        .starts_with("tools_not_restricted"));
    assert_eq!(db.job(&second.id).unwrap().state, "queued");
    assert!(!db.has_ready_work().unwrap());
}

#[tokio::test]
async fn paused_source_receipts_resume_on_operator_notification() {
    let (dir, db, mut cfg) = setup();
    cfg.sources[0].enabled = false;
    cfg.save(dir.path()).unwrap();
    receive(&db, &cfg, &event("e1", "r1"));
    let engine = Engine::new(dir.path().into(), db.clone());
    engine.run_once().await.unwrap();
    assert_eq!(db.event_detail("e1").unwrap()["state"], "held");
    cfg.sources[0].enabled = true;
    cfg.save(dir.path()).unwrap();
    db.wake_waiters(None).unwrap();
    engine.run_once().await.unwrap();
    assert_eq!(db.event_detail("e1").unwrap()["state"], "unassigned");
}

#[tokio::test]
async fn claimed_job_reloads_config_instead_of_using_old_batch_snapshot() {
    let (dir, db, cfg) = setup();
    let job = enqueue(&db, &cfg, &event("e1", "r1"), &target("thread"));
    let claimed = db.claim(&job.id).unwrap().unwrap();
    let mut fresh = cfg.clone();
    fresh.paused = true;
    fresh.save(dir.path()).unwrap();
    let engine = Engine::new(dir.path().into(), db.clone());
    engine.run_job(&cfg, claimed).await.unwrap();
    assert_eq!(db.job(&job.id).unwrap().state, "held");
    assert!(db.drafts().unwrap().is_empty());
}
#[test]
fn relay_receipts_prove_origin_and_scope_before_keyset_pagination() {
    let (_dir, db, cfg) = setup();
    for id in ["a", "b", "c"] {
        let job = enqueue(&db, &cfg, &event(id, id), &target(id));
        db.claim(&job.id).unwrap().unwrap();
        db.finish_job(&job.id, "completed", "completed", Some("turn"), true)
            .unwrap();
    }
    assert!(db.record_relay_receipt("relay", "missing").is_err());
    db.record_relay_receipt("relay", "a").unwrap();
    db.record_relay_receipt("relay", "c").unwrap();
    db.record_relay_receipt("other-relay", "b").unwrap();
    db.record_relay_receipt("relay", "a").unwrap();
    assert!(db.relay_received_from("relay", "a").unwrap());
    assert!(!db.relay_received_from("relay", "b").unwrap());
    let scope = vec!["source".into()];
    assert_eq!(
        db.relay_completion_receipts("relay", &scope, None, 1)
            .unwrap(),
        vec!["a"]
    );
    assert_eq!(
        db.relay_completion_receipts("relay", &scope, Some("a"), 1)
            .unwrap(),
        vec!["c"]
    );
    assert!(db
        .relay_completion_receipts("relay", &["other-source".into()], None, 10)
        .unwrap()
        .is_empty());
}

#[test]
fn backup_is_private_complete_and_never_overwrites_existing_files() {
    let (dir, db, cfg) = setup();
    // Keep a reader connection open so the source may still have live WAL data.
    let live = rusqlite::Connection::open(dir.path().join("state.sqlite3")).unwrap();
    live.execute_batch("PRAGMA journal_mode=WAL; PRAGMA wal_autocheckpoint=0;")
        .unwrap();
    let e = event("private-event", "resource");
    db.receive(&e, b"private-provider-payload", &cfg).unwrap();
    let output = tempfile::tempdir().unwrap();
    let path = output.path().join("snapshot.sqlite3");
    db.backup(&path).unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            std::fs::metadata(&path).unwrap().permissions().mode() & 0o777,
            0o600
        );
    }
    let snapshot =
        rusqlite::Connection::open_with_flags(&path, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)
            .unwrap();
    assert_eq!(
        snapshot
            .query_row("PRAGMA quick_check", [], |row| row.get::<_, String>(0))
            .unwrap(),
        "ok"
    );
    assert_eq!(
        snapshot
            .query_row(
                "SELECT raw FROM deliveries WHERE delivery_id='delivery-private-event'",
                [],
                |row| row.get::<_, Vec<u8>>(0)
            )
            .unwrap(),
        b"private-provider-payload"
    );
    drop(snapshot);
    let existing = output.path().join("existing");
    std::fs::write(&existing, b"do-not-overwrite").unwrap();
    assert!(db.backup(&existing).is_err());
    assert_eq!(std::fs::read(&existing).unwrap(), b"do-not-overwrite");
    assert!(!std::fs::read_dir(output.path()).unwrap().any(|entry| entry
        .unwrap()
        .file_name()
        .to_string_lossy()
        .starts_with(".right-wake-backup-")));
}

#[cfg(unix)]
#[test]
fn backup_refuses_existing_and_dangling_symlinks_without_touching_targets() {
    use std::os::unix::fs::symlink;
    let (_dir, db, _cfg) = setup();
    let output = tempfile::tempdir().unwrap();
    let existing = output.path().join("existing");
    std::fs::write(&existing, b"existing-private-data").unwrap();
    let link = output.path().join("existing-link");
    symlink(&existing, &link).unwrap();
    assert!(db.backup(&link).is_err());
    assert_eq!(std::fs::read(&existing).unwrap(), b"existing-private-data");
    assert!(std::fs::symlink_metadata(&link)
        .unwrap()
        .file_type()
        .is_symlink());
    let absent = output.path().join("absent");
    let dangling = output.path().join("dangling-link");
    symlink(&absent, &dangling).unwrap();
    assert!(db.backup(&dangling).is_err());
    assert!(!absent.exists());
    assert!(std::fs::symlink_metadata(&dangling)
        .unwrap()
        .file_type()
        .is_symlink());
}

#[test]
fn failed_backup_cleans_only_its_own_staging_files() {
    let (dir, db, _cfg) = setup();
    let output = tempfile::tempdir().unwrap();
    let preserved = output.path().join("preserved");
    std::fs::write(&preserved, b"unrelated").unwrap();
    // Exercise a real SQLite failure after creating the staging directory.
    std::fs::write(dir.path().join("state.sqlite3"), b"not a sqlite database").unwrap();
    let destination = output.path().join("failed.sqlite3");
    assert!(db.backup(&destination).is_err());
    assert!(!destination.exists());
    assert_eq!(std::fs::read(&preserved).unwrap(), b"unrelated");
    assert_eq!(std::fs::read_dir(output.path()).unwrap().count(), 1);
}

#[test]
fn skip_draft_accepts_only_known_unsent_states_and_preserves_other_outcomes() {
    let (_dir, db, cfg) = setup();
    for state in ["draft", "stale", "sending", "sent", "unknown", "skipped"] {
        let e = event(state, state);
        let job = enqueue(&db, &cfg, &e, &target(state));
        let draft = db
            .save_draft(&job, "reviewed local body", "17")
            .unwrap()
            .unwrap();
        db.draft_state(
            &draft.id,
            state,
            "original outcome evidence",
            &json!({"provider_result":"keep"}),
        )
        .unwrap();
        if matches!(state, "draft" | "stale") {
            db.skip_draft(&draft.id).unwrap();
            assert_eq!(db.draft(&draft.id).unwrap().state, "skipped");
            assert!(db.skip_draft(&draft.id).is_err());
        } else {
            assert!(db.skip_draft(&draft.id).is_err());
            assert_eq!(db.draft(&draft.id).unwrap().state, state);
        }
    }
    assert!(db.skip_draft("missing").is_err());
}

#[test]
fn skip_and_begin_send_race_has_one_winner() {
    let (_dir, db, cfg) = setup();
    let job = enqueue(&db, &cfg, &event("race", "race"), &target("thread"));
    let draft = db
        .save_draft(&job, "reviewed local body", "17")
        .unwrap()
        .unwrap();
    let barrier = Arc::new(Barrier::new(2));
    let skip_db = db.clone();
    let skip_id = draft.id.clone();
    let skip_barrier = barrier.clone();
    let skip = std::thread::spawn(move || {
        skip_barrier.wait();
        skip_db.skip_draft(&skip_id).is_ok()
    });
    let send_db = db.clone();
    let send_id = draft.id.clone();
    let send = std::thread::spawn(move || {
        barrier.wait();
        send_db.begin_send(&send_id).is_ok()
    });
    let skipped = skip.join().unwrap();
    let sending = send.join().unwrap();
    assert_ne!(skipped, sending);
    assert_eq!(
        db.draft(&draft.id).unwrap().state,
        if skipped { "skipped" } else { "sending" }
    );
}

#[cfg(unix)]
#[test]
fn backup_resolves_symlinked_ancestors_without_following_the_final_entry() {
    use std::os::unix::fs::{symlink, PermissionsExt};
    let (_dir, db, cfg) = setup();
    receive(&db, &cfg, &event("ancestor", "resource"));
    let output = tempfile::tempdir().unwrap();
    let physical = output.path().join("physical");
    std::fs::create_dir(&physical).unwrap();
    let alias = output.path().join("alias");
    symlink(&physical, &alias).unwrap();
    db.backup(&alias.join("snapshot.sqlite3")).unwrap();
    let snapshot = physical.join("snapshot.sqlite3");
    assert_eq!(
        std::fs::metadata(&snapshot).unwrap().permissions().mode() & 0o777,
        0o600
    );
    let restored = rusqlite::Connection::open_with_flags(
        &snapshot,
        rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
    )
    .unwrap();
    assert_eq!(
        restored
            .query_row("SELECT id FROM events", [], |row| row.get::<_, String>(0))
            .unwrap(),
        "ancestor"
    );
    let final_link = physical.join("final-link");
    let untouched = physical.join("untouched");
    std::fs::write(&untouched, b"do not replace").unwrap();
    symlink(&untouched, &final_link).unwrap();
    assert!(db.backup(&alias.join("final-link")).is_err());
    assert_eq!(std::fs::read(&untouched).unwrap(), b"do not replace");
    assert!(std::fs::symlink_metadata(&final_link)
        .unwrap()
        .file_type()
        .is_symlink());
}
