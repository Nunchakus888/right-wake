use crate::{
    config::{private_dir, Config},
    model::*,
    provider::SourceConfig,
};
use anyhow::{bail, Context, Result};
use rusqlite::{params, Connection, OptionalExtension, TransactionBehavior};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{
    path::{Path, PathBuf},
    sync::Arc,
    time::Duration,
};

// SQLite fixed the WAL-reset race in the 3.51.3 mainline release. RightWake
// uses WAL with multiple connections, so validate the linked engine rather
// than relying on a Cargo package version or the system sqlite3 command.
pub const MIN_SQLITE_VERSION_NUMBER: i32 = 3_051_003;
pub const MAX_LINEAR_IMPORT_ISSUES: usize = 10_000;

/// A partial, explicitly supplied read-only snapshot. Omitted issues say nothing
/// about membership and must never clear existing assignment evidence.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LinearStateImport {
    pub schema_version: u32,
    pub organization_id: String,
    pub issues: Vec<LinearIssueSnapshot>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LinearIssueSnapshot {
    pub issue_id: String,
    pub assignee: LinearAssignee,
    pub updated_at_ms: i64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LinearScopeDecision {
    Admitted,
    Waiting(&'static str),
    Outside(&'static str),
    Suppressed(&'static str),
}

impl LinearScopeDecision {
    pub fn state(self) -> &'static str {
        match self {
            Self::Admitted => "received",
            Self::Waiting(_) => "waiting_assignment",
            Self::Outside(_) => "outside_assignment",
            Self::Suppressed(_) => "cached",
        }
    }
    pub fn reason(self) -> Option<&'static str> {
        match self {
            Self::Admitted => None,
            Self::Waiting(reason) | Self::Outside(reason) | Self::Suppressed(reason) => {
                Some(reason)
            }
        }
    }
}

#[derive(Debug, PartialEq, Eq)]
struct LinearProjection {
    assignee: LinearAssignee,
    updated_at_ms: i64,
    conflicted: bool,
    removed: bool,
    assigned_since_ms: Option<i64>,
}

fn linear_resource_key(source: &SourceConfig, organization: &str, issue: &str) -> Result<String> {
    if source.kind != ProviderKind::Linear
        || organization.is_empty()
        || organization.len() > 1024
        || organization
            .chars()
            .any(|value| value.is_whitespace() || value.is_control())
        || source.tenant_id.as_deref() != Some(organization)
        || issue.is_empty()
        || issue.len() > 1024
        || issue
            .chars()
            .any(|value| value.is_whitespace() || value.is_control())
    {
        bail!("linear_scope_mismatch: exact configured Linear organization and issue identity required");
    }
    let component =
        |value: &str| url::form_urlencoded::byte_serialize(value.as_bytes()).collect::<String>();
    Ok(format!(
        "linear:{}:{}:{}",
        source.instance_origin()?,
        component(organization),
        component(issue)
    ))
}

fn validate_linear_assignee(assignee: &LinearAssignee) -> Result<()> {
    if let LinearAssignee::Assigned { user_id } = assignee {
        if user_id.is_empty()
            || user_id.len() > 1024
            || user_id
                .chars()
                .any(|value| value.is_whitespace() || value.is_control())
        {
            bail!("invalid_linear_assignee: assigned state requires a bounded user ID");
        }
    }
    Ok(())
}

fn read_linear_projection(
    connection: &Connection,
    source: &str,
    resource: &str,
) -> Result<Option<LinearProjection>> {
    let row: Option<(String, i64, bool, bool, Option<i64>)> = connection.query_row(
        "SELECT assignee,updated_at_ms,conflicted,removed,assigned_since_ms FROM linear_issues WHERE source_id=?1 AND resource_key=?2",
        params![source, resource],
        |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?, row.get(4)?)),
    ).optional()?;
    row.map(
        |(assignee, updated_at_ms, conflicted, removed, assigned_since_ms)| {
            Ok(LinearProjection {
                assignee: serde_json::from_str(&assignee)?,
                updated_at_ms,
                conflicted,
                removed,
                assigned_since_ms,
            })
        },
    )
    .transpose()
}

/// Equal revisions cannot arbitrate conflicting owners. A tombstone wins an
/// equal revision, and only a newer snapshot can restore a removed issue.
fn merge_linear_projection(
    tx: &rusqlite::Transaction<'_>,
    source: &SourceConfig,
    organization: &str,
    resource: &str,
    snapshot: &LinearIssueSnapshot,
    removed: bool,
    assignment_transition: Option<i64>,
) -> Result<bool> {
    let old = read_linear_projection(tx, &source.id, resource)?;
    let mut next = LinearProjection {
        assignee: snapshot.assignee.clone(),
        updated_at_ms: snapshot.updated_at_ms,
        conflicted: false,
        removed,
        assigned_since_ms: assignment_transition,
    };
    if let Some(current) = &old {
        if snapshot.updated_at_ms < current.updated_at_ms {
            next = LinearProjection {
                assignee: current.assignee.clone(),
                updated_at_ms: current.updated_at_ms,
                conflicted: current.conflicted,
                removed: current.removed,
                assigned_since_ms: current.assigned_since_ms,
            };
        } else if snapshot.updated_at_ms == current.updated_at_ms {
            next.conflicted = current.conflicted;
            next.removed |= current.removed;
            next.assigned_since_ms = current.assigned_since_ms;
            if current.conflicted || current.removed || snapshot.assignee == LinearAssignee::Unknown
            {
                next.assignee = current.assignee.clone();
            } else if current.assignee != LinearAssignee::Unknown
                && current.assignee != snapshot.assignee
            {
                next.assignee = LinearAssignee::Unknown;
                next.conflicted = true;
                next.assigned_since_ms = None;
            }
        } else if current.assignee == snapshot.assignee && !current.conflicted && !current.removed {
            next.assigned_since_ms = current.assigned_since_ms;
        }
    }
    // A late transition can refine a same-owner interval without rolling the
    // current projection back to the older revision.
    if !next.conflicted && !next.removed && next.assignee == snapshot.assignee {
        if let Some(at) = assignment_transition {
            next.assigned_since_ms = Some(next.assigned_since_ms.map_or(at, |old| old.max(at)));
        }
    }
    if next.removed || next.assignee == LinearAssignee::Unknown {
        next.assigned_since_ms = None;
    }
    if old.as_ref() == Some(&next) {
        return Ok(false);
    }
    tx.execute("INSERT INTO linear_issues(source_id,resource_key,organization_id,issue_id,assignee,updated_at_ms,conflicted,removed,assigned_since_ms) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9) ON CONFLICT(source_id,resource_key) DO UPDATE SET assignee=excluded.assignee,updated_at_ms=excluded.updated_at_ms,conflicted=excluded.conflicted,removed=excluded.removed,assigned_since_ms=excluded.assigned_since_ms",
        params![source.id, resource, organization, snapshot.issue_id, serde_json::to_string(&next.assignee)?, next.updated_at_ms, next.conflicted, next.removed, next.assigned_since_ms])?;
    Ok(true)
}

fn invalidate_linear_projection(
    tx: &rusqlite::Transaction<'_>,
    source: &SourceConfig,
    organization: &str,
    resource: &str,
    issue: &str,
    removed: bool,
) -> Result<()> {
    // An unorderable Issue observation cannot leave a previous positive owner
    // actionable. Retain the last ordered revision as a recovery floor; an
    // equal-version replay cannot resolve this uncertainty.
    tx.execute("INSERT INTO linear_issues(source_id,resource_key,organization_id,issue_id,assignee,updated_at_ms,conflicted,removed,assigned_since_ms) VALUES(?1,?2,?3,?4,?5,0,1,?6,NULL) ON CONFLICT(source_id,resource_key) DO UPDATE SET assignee=excluded.assignee,conflicted=1,removed=max(removed,excluded.removed),assigned_since_ms=NULL",
        params![source.id, resource, organization, issue, serde_json::to_string(&LinearAssignee::Unknown)?, removed])?;
    Ok(())
}

fn linear_scope(
    connection: &Connection,
    source: &SourceConfig,
    event: &NormalizedEvent,
) -> Result<LinearScopeDecision> {
    let Some(expected) = &source.linear_assignee_id else {
        return Ok(LinearScopeDecision::Admitted);
    };
    if !event.actionable || !crate::provider::linear_personal_candidate(event, expected) {
        return Ok(LinearScopeDecision::Suppressed(
            "linear_intrinsic_or_static_denial",
        ));
    }
    // Reapply current static/actor policy monotonically; ownership evidence must
    // never turn an event rejected at either relay boundary into a request.
    let mut checked = event.clone();
    crate::provider::apply_local_policy(source, &mut checked)?;
    if !checked.actionable {
        return Ok(LinearScopeDecision::Suppressed(
            "linear_intrinsic_or_static_denial",
        ));
    }
    let Some(projection) = read_linear_projection(connection, &source.id, &event.resource_key)?
    else {
        return Ok(LinearScopeDecision::Waiting("linear_assignment_unknown"));
    };
    if projection.removed {
        return Ok(LinearScopeDecision::Outside("linear_issue_removed"));
    }
    if projection.conflicted {
        return Ok(LinearScopeDecision::Waiting("linear_assignment_conflict"));
    }
    if projection.assignee == LinearAssignee::Unknown {
        return Ok(LinearScopeDecision::Waiting("linear_assignment_unknown"));
    }
    if !projection.assignee.is_assigned_to(expected) {
        return Ok(LinearScopeDecision::Outside(
            "linear_not_assigned_to_watched_user",
        ));
    }
    if event.action == "comment.create"
        && projection
            .assigned_since_ms
            .is_some_and(|at| at > event.occurred_at_ms)
    {
        return Ok(LinearScopeDecision::Outside(
            "linear_comment_predates_assignment",
        ));
    }
    if matches!(event.action.as_str(), "issue.create" | "issue.update")
        && projection.assigned_since_ms.is_some_and(|since| {
            event
                .linear
                .as_ref()
                .and_then(|metadata| metadata.issue_updated_at_ms)
                .is_none_or(|at| at < since)
        })
    {
        return Ok(LinearScopeDecision::Outside(
            "linear_event_predates_assignment",
        ));
    }
    Ok(LinearScopeDecision::Admitted)
}

fn pending_count(connection: &Connection) -> Result<usize> {
    Ok(connection.query_row("SELECT count(*) FROM events WHERE state IN ('received','unassigned','routing','router_waiting','router_unknown','queued','held','waiting_assignment','waiting_host','waiting_busy','dispatching','unknown')", [], |row| row.get(0))?)
}

fn refresh_linear_pending(
    tx: &rusqlite::Transaction<'_>,
    source: &SourceConfig,
    resource: &str,
    late_assignment_at: Option<i64>,
    max_pending: usize,
) -> Result<usize> {
    if source.linear_assignee_id.is_none() {
        return Ok(0);
    }
    let rows = {
        let mut statement = tx.prepare("SELECT data,state FROM events WHERE source_id=?1 AND resource_key=?2 AND state IN ('waiting_assignment','outside_assignment','waiting_capacity','received','unassigned','queued','waiting_host','waiting_busy','held','router_waiting')")?;
        let rows = statement.query_map(params![source.id, resource], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })?;
        rows.collect::<rusqlite::Result<Vec<_>>>()?
    };
    let mut awakened = 0;
    for (data, old_state) in rows {
        let event: NormalizedEvent = serde_json::from_str(&data)?;
        let decision = linear_scope(tx, source, &event)?;
        if old_state == "outside_assignment"
            && !(decision == LinearScopeDecision::Admitted
                && event.action == "comment.create"
                && late_assignment_at.is_some_and(|at| at <= event.occurred_at_ms))
        {
            continue;
        }
        let needs_slot = matches!(
            old_state.as_str(),
            "outside_assignment" | "waiting_capacity"
        ) && matches!(
            decision,
            LinearScopeDecision::Admitted | LinearScopeDecision::Waiting(_)
        );
        if needs_slot && pending_count(tx)? >= max_pending {
            tx.execute("UPDATE events SET state='waiting_capacity',reason='linear_scope_capacity_wait' WHERE id=?1", [&event.event_id])?;
            tx.execute("UPDATE jobs SET state='waiting_capacity',reason='linear_scope_capacity_wait' WHERE event_id=?1 AND state IN ('waiting_assignment','outside_assignment','waiting_capacity')", [&event.event_id])?;
            continue;
        }
        let state = if decision == LinearScopeDecision::Admitted {
            if !source.enabled
                || !matches!(
                    old_state.as_str(),
                    "waiting_assignment" | "outside_assignment" | "waiting_capacity"
                )
            {
                continue;
            }
            let changed = tx.execute("UPDATE jobs SET state='queued',reason=NULL WHERE event_id=?1 AND state IN ('waiting_assignment','outside_assignment','waiting_capacity')", [&event.event_id])?;
            awakened += 1;
            if changed == 1 {
                "queued"
            } else {
                "received"
            }
        } else {
            tx.execute("UPDATE jobs SET state=?2,reason=?3 WHERE event_id=?1 AND state IN ('queued','waiting_assignment','outside_assignment','waiting_capacity','waiting_host','waiting_busy','held')",
                params![event.event_id, decision.state(), decision.reason()])?;
            decision.state()
        };
        tx.execute(
            "UPDATE events SET state=?2,reason=?3 WHERE id=?1",
            params![event.event_id, state, decision.reason()],
        )?;
    }
    Ok(awakened)
}

/// Stored late-evidence work is promoted only by an actual state-changing
/// event. This backlog never self-notifies or bypasses the runnable limit.
fn wake_linear_capacity(tx: &rusqlite::Transaction<'_>, cfg: &Config) -> Result<usize> {
    let resources = {
        let mut statement = tx.prepare("SELECT DISTINCT source_id,resource_key FROM events WHERE state='waiting_capacity' ORDER BY source_id,resource_key LIMIT 1000")?;
        let rows = statement.query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })?;
        rows.collect::<rusqlite::Result<Vec<_>>>()?
    };
    let mut awakened = 0;
    for (source_id, resource) in resources {
        if let Ok(source) = cfg.source(&source_id) {
            if source.enabled {
                awakened += refresh_linear_pending(tx, source, &resource, None, cfg.max_pending)?;
            }
        }
    }
    Ok(awakened)
}

#[derive(Clone)]
pub struct Db {
    path: Arc<PathBuf>,
}

impl Db {
    pub fn open(dir: &Path) -> Result<Self> {
        if rusqlite::version_number() < MIN_SQLITE_VERSION_NUMBER {
            bail!(
                "sqlite_version_unsupported: linked SQLite {} requires an upgrade to 3.51.3 or later before opening RightWake state",
                rusqlite::version()
            );
        }
        private_dir(dir)?;
        let db = Self {
            path: Arc::new(dir.join("state.sqlite3")),
        };
        let c = db.conn()?;
        c.pragma_update(None, "journal_mode", "WAL")?;
        let version: i64 = c.pragma_query_value(None, "user_version", |r| r.get(0))?;
        if version > 4 {
            bail!("database schema is newer than this binary; upgrade without deleting data");
        }
        c.execute_batch("BEGIN IMMEDIATE;
          CREATE TABLE IF NOT EXISTS deliveries(source_id TEXT NOT NULL, delivery_id TEXT NOT NULL, hash TEXT NOT NULL, raw BLOB NOT NULL, received INTEGER NOT NULL, PRIMARY KEY(source_id,delivery_id));
          CREATE TABLE IF NOT EXISTS events(id TEXT PRIMARY KEY, source_id TEXT NOT NULL, resource_key TEXT NOT NULL, data TEXT NOT NULL, state TEXT NOT NULL, reason TEXT, received INTEGER NOT NULL, routing_attempted INTEGER NOT NULL DEFAULT 0, source_version INTEGER NOT NULL DEFAULT 0);
          CREATE INDEX IF NOT EXISTS events_state ON events(state,received);
          CREATE TABLE IF NOT EXISTS resources(source_id TEXT NOT NULL, resource_key TEXT NOT NULL, revision INTEGER NOT NULL, latest_at INTEGER NOT NULL, latest_event TEXT NOT NULL, PRIMARY KEY(source_id,resource_key));
          CREATE TABLE IF NOT EXISTS bindings(source_id TEXT NOT NULL, key TEXT NOT NULL, target TEXT NOT NULL, version INTEGER NOT NULL, enabled INTEGER NOT NULL, origin TEXT NOT NULL, PRIMARY KEY(source_id,key));
          CREATE TABLE IF NOT EXISTS cards(id TEXT PRIMARY KEY, data TEXT NOT NULL, revision INTEGER NOT NULL);
          CREATE TABLE IF NOT EXISTS jobs(id TEXT PRIMARY KEY, event_id TEXT NOT NULL UNIQUE REFERENCES events(id), data TEXT NOT NULL, target_key TEXT NOT NULL, state TEXT NOT NULL, reason TEXT, dispatch_id TEXT, turn_id TEXT, created INTEGER NOT NULL);
          CREATE INDEX IF NOT EXISTS jobs_state ON jobs(state,created);
          CREATE TABLE IF NOT EXISTS locks(target_key TEXT PRIMARY KEY, job_id TEXT NOT NULL UNIQUE);
          CREATE TABLE IF NOT EXISTS drafts(id TEXT PRIMARY KEY, job_id TEXT NOT NULL UNIQUE, source_id TEXT NOT NULL, resource_key TEXT NOT NULL, data TEXT NOT NULL, state TEXT NOT NULL, reason TEXT, result TEXT);
          CREATE TABLE IF NOT EXISTS audit(seq INTEGER PRIMARY KEY AUTOINCREMENT, at INTEGER NOT NULL, kind TEXT NOT NULL, subject TEXT NOT NULL, detail TEXT NOT NULL);
          CREATE TABLE IF NOT EXISTS relay_acks(device TEXT NOT NULL, event_id TEXT NOT NULL REFERENCES events(id), phase TEXT NOT NULL, PRIMARY KEY(device,event_id));
          CREATE TABLE IF NOT EXISTS router_dispatches(event_id TEXT PRIMARY KEY REFERENCES events(id), dispatch_id TEXT NOT NULL UNIQUE, target_key TEXT NOT NULL, target TEXT NOT NULL, state TEXT NOT NULL, turn_id TEXT, reason TEXT, created INTEGER NOT NULL);
          CREATE INDEX IF NOT EXISTS router_dispatches_state ON router_dispatches(state,created);
          CREATE INDEX IF NOT EXISTS jobs_target_state ON jobs(target_key,state);
          CREATE TABLE IF NOT EXISTS relay_receipts(relay_id TEXT NOT NULL, event_id TEXT NOT NULL REFERENCES events(id), PRIMARY KEY(relay_id,event_id));
          CREATE TABLE IF NOT EXISTS linear_issues(source_id TEXT NOT NULL, resource_key TEXT NOT NULL, organization_id TEXT NOT NULL, issue_id TEXT NOT NULL, assignee TEXT NOT NULL, updated_at_ms INTEGER NOT NULL, conflicted INTEGER NOT NULL, removed INTEGER NOT NULL, assigned_since_ms INTEGER, PRIMARY KEY(source_id,resource_key));
          CREATE TABLE IF NOT EXISTS dispatch_contexts(dispatch_id TEXT PRIMARY KEY, target TEXT NOT NULL, payload TEXT NOT NULL, output_schema TEXT, read_at INTEGER);
          PRAGMA user_version=4; COMMIT;")?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(db.path.as_ref(), std::fs::Permissions::from_mode(0o600))?;
        }
        Ok(db)
    }
    fn conn(&self) -> Result<Connection> {
        let c = Connection::open(self.path.as_ref())?;
        c.busy_timeout(Duration::from_secs(3))?;
        c.pragma_update(None, "foreign_keys", true)?;
        c.pragma_update(None, "synchronous", "FULL")?;
        Ok(c)
    }
    /// Publish a complete, private SQLite snapshot without ever opening an
    /// existing destination for writing. The final hard link is create-only,
    /// including when another process creates a file/symlink during backup.
    pub fn backup(&self, path: &Path) -> Result<()> {
        match std::fs::symlink_metadata(path) {
            Ok(_) => bail!("backup destination already exists"),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(error).context("cannot inspect backup destination"),
        }
        let parent = path
            .parent()
            .filter(|parent| !parent.as_os_str().is_empty())
            .unwrap_or_else(|| Path::new("."))
            .canonicalize()
            .context("backup parent directory must exist and be accessible")?;
        // macOS commonly reaches its temporary directory through /var ->
        // /private/var. Resolve existing ancestors once, without following or
        // replacing the final destination entry. SQLite NOFOLLOW stays enabled.
        let destination_path = parent.join(
            path.file_name()
                .context("backup destination needs a filename")?,
        );
        let (staging, file) = BackupStaging::create(&parent)?;
        let source = self.conn()?;
        let mut destination = Connection::open_with_flags(
            &staging.path,
            rusqlite::OpenFlags::SQLITE_OPEN_READ_WRITE
                | rusqlite::OpenFlags::SQLITE_OPEN_NO_MUTEX
                | rusqlite::OpenFlags::SQLITE_OPEN_NOFOLLOW,
        )?;
        destination.pragma_update(None, "journal_mode", "DELETE")?;
        destination.pragma_update(None, "synchronous", "FULL")?;
        {
            let backup = rusqlite::backup::Backup::new(&source, &mut destination)?;
            if !matches!(backup.step(-1)?, rusqlite::backup::StepResult::Done) {
                bail!("backup_busy: snapshot could not complete; retry when the database is available");
            }
        }
        destination.close().map_err(|(_, error)| error)?;
        file.sync_all()?;
        std::fs::hard_link(&staging.path,&destination_path).context("cannot publish backup without overwriting; choose a new path on a filesystem supporting hard links")?;
        #[cfg(unix)]
        std::fs::File::open(&parent)?.sync_all()?;
        Ok(())
    }
    pub fn audit(&self, kind: &str, subject: &str, detail: &Value) -> Result<()> {
        self.conn()?.execute(
            "INSERT INTO audit(at,kind,subject,detail) VALUES(?1,?2,?3,?4)",
            params![now_ms(), kind, subject, detail.to_string()],
        )?;
        Ok(())
    }

    /// Queue notices carry only this identity; external work stays in a tool
    /// result. An accepted dispatch identity can never be repointed or edited.
    pub fn put_dispatch_context(
        &self,
        dispatch_id: &str,
        target: &ThreadTarget,
        prompt: &str,
        output_schema: Option<&Value>,
    ) -> Result<()> {
        if dispatch_id.is_empty() || dispatch_id.len() > 128 || prompt.len() > 2 * 1024 * 1024 {
            bail!("invalid_dispatch_context: identity and payload must be bounded");
        }
        let payload = serde_json::to_string(&serde_json::from_str::<Value>(prompt)?)?;
        let target = serde_json::to_string(target)?;
        let schema = output_schema.map(serde_json::to_string).transpose()?;
        if schema
            .as_ref()
            .is_some_and(|value| value.len() > 256 * 1024)
        {
            bail!("invalid_dispatch_context: output schema exceeds the limit");
        }
        let mut connection = self.conn()?;
        let tx = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let existing: Option<(String, String, Option<String>)> = tx
            .query_row(
                "SELECT target,payload,output_schema FROM dispatch_contexts WHERE dispatch_id=?1",
                [dispatch_id],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .optional()?;
        if let Some(existing) = existing {
            if existing != (target, payload, schema) {
                bail!("dispatch_context_conflict: dispatch identity already has different immutable content");
            }
        } else {
            tx.execute(
                "INSERT INTO dispatch_contexts(dispatch_id,target,payload,output_schema) VALUES(?1,?2,?3,?4)",
                params![dispatch_id, target, payload, schema],
            )?;
        }
        tx.commit()?;
        Ok(())
    }

    pub fn dispatch_context(&self, dispatch_id: &str) -> Result<Value> {
        let (target, payload, output_schema): (String, String, Option<String>) = self
            .conn()?
            .query_row(
                "SELECT target,payload,output_schema FROM dispatch_contexts WHERE dispatch_id=?1",
                [dispatch_id],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .context("dispatch_context_not_found")?;
        Ok(json!({
            "dispatch_id":dispatch_id,
            "target":serde_json::from_str::<Value>(&target)?,
            "payload":serde_json::from_str::<Value>(&payload)?,
            "output_schema":output_schema.map(|value| serde_json::from_str::<Value>(&value)).transpose()?
        }))
    }

    pub fn mark_dispatch_context_read(&self, dispatch_id: &str) -> Result<()> {
        if self.conn()?.execute(
            "UPDATE dispatch_contexts SET read_at=coalesce(read_at,?2) WHERE dispatch_id=?1",
            params![dispatch_id, now_ms()],
        )? != 1
        {
            bail!("dispatch_context_not_found");
        }
        Ok(())
    }

    pub fn dispatch_context_read(&self, dispatch_id: &str) -> Result<bool> {
        Ok(self.conn()?.query_row(
            "SELECT EXISTS(SELECT 1 FROM dispatch_contexts WHERE dispatch_id=?1 AND read_at IS NOT NULL)",
            [dispatch_id], |row| row.get(0),
        )?)
    }

    pub fn import_linear_state(
        &self,
        source: &SourceConfig,
        snapshot: &LinearStateImport,
    ) -> Result<Value> {
        source.validate()?;
        if snapshot.schema_version != 1 || snapshot.issues.len() > MAX_LINEAR_IMPORT_ISSUES {
            bail!("invalid_linear_import: expected schema_version 1 and at most {MAX_LINEAR_IMPORT_ISSUES} issues");
        }
        // Validate the complete input before changing any projection or event.
        for issue in &snapshot.issues {
            linear_resource_key(source, &snapshot.organization_id, &issue.issue_id)?;
            validate_linear_assignee(&issue.assignee)?;
            if issue.assignee == LinearAssignee::Unknown {
                bail!("invalid_linear_import: bootstrap requires an explicitly assigned or unassigned owner");
            }
            if issue.updated_at_ms <= 0 {
                bail!("invalid_linear_import: updated_at_ms must be a positive issue revision timestamp");
            }
        }
        // Empty imports must still prove the intended source and organization.
        linear_resource_key(source, &snapshot.organization_id, "scope-check")?;
        let config = Config::load(self.path.parent().context("database profile missing")?)?;
        let mut connection = self.conn()?;
        let tx = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let mut changed = 0;
        let mut awakened = 0;
        for issue in &snapshot.issues {
            let resource = linear_resource_key(source, &snapshot.organization_id, &issue.issue_id)?;
            changed += usize::from(merge_linear_projection(
                &tx,
                source,
                &snapshot.organization_id,
                &resource,
                issue,
                false,
                None,
            )?);
            // An import is current state, not authenticated evidence of a past
            // transition. It never replays already-known nonmember comments.
            awakened += refresh_linear_pending(&tx, source, &resource, None, config.max_pending)?;
        }
        awakened += wake_linear_capacity(&tx, &config)?;
        tx.execute("INSERT INTO audit(at,kind,subject,detail) VALUES(?1,'linear_state_import',?2,?3)",
            params![now_ms(), source.id, json!({"issues":snapshot.issues.len(),"changed":changed,"awakened":awakened,"partial":true}).to_string()])?;
        tx.commit()?;
        Ok(
            json!({"source_id":source.id,"organization_id":snapshot.organization_id,
            "imported":snapshot.issues.len(),"changed":changed,"awakened":awakened,
            "partial":true,"absence_unassigns":false,"model_invoked":false}),
        )
    }

    pub fn linear_state(&self, source: &SourceConfig) -> Result<Value> {
        let organization = source
            .tenant_id
            .as_deref()
            .context("linear_tenant_required")?;
        linear_resource_key(source, organization, "scope-check")?;
        let connection = self.conn()?;
        let mut statement = connection.prepare(
            "SELECT issue_id,resource_key,assignee,updated_at_ms,conflicted,removed,assigned_since_ms FROM linear_issues WHERE source_id=?1 AND organization_id=?2 ORDER BY issue_id LIMIT 10001",
        )?;
        let rows = statement.query_map(params![source.id, organization], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, i64>(3)?,
                row.get::<_, bool>(4)?,
                row.get::<_, bool>(5)?,
                row.get::<_, Option<i64>>(6)?,
            ))
        })?;
        let mut issues = Vec::new();
        for row in rows {
            let (id, key, assignee, version, conflicted, removed, since) = row?;
            issues.push(json!({"issue_id":id,"resource_key":key,"assignee":serde_json::from_str::<LinearAssignee>(&assignee)?,
                "updated_at_ms":version,"conflicted":conflicted,"removed":removed,"assigned_since_ms":since}));
        }
        let truncated = issues.len() > MAX_LINEAR_IMPORT_ISSUES;
        issues.truncate(MAX_LINEAR_IMPORT_ISSUES);
        let (waiting_assignment, outside_assignment, waiting_capacity): (i64, i64, i64) = connection.query_row(
            "SELECT coalesce(sum(state='waiting_assignment'),0),coalesce(sum(state='outside_assignment'),0),coalesce(sum(state='waiting_capacity'),0) FROM events WHERE source_id=?1 AND json_extract(data,'$.tenant_id')=?2",
            params![source.id, organization], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )?;
        Ok(
            json!({"schema_version":1,"source_id":source.id,"organization_id":organization,
            "linear_assignee_id":source.linear_assignee_id,"issues":issues,"truncated":truncated,
            "partial":true,"absence_unassigns":false,
            "waiting_assignment":waiting_assignment,"outside_assignment":outside_assignment,"waiting_capacity":waiting_capacity}),
        )
    }

    pub fn linear_scope(
        &self,
        source: &SourceConfig,
        event: &NormalizedEvent,
    ) -> Result<LinearScopeDecision> {
        linear_scope(&self.conn()?, source, event)
    }

    pub fn wake_linear_capacity(&self, cfg: &Config) -> Result<usize> {
        let mut connection = self.conn()?;
        let tx = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let awakened = wake_linear_capacity(&tx, cfg)?;
        tx.commit()?;
        Ok(awakened)
    }

    /// Returning accepted is only possible after the raw delivery and canonical
    /// event commit. A failed commit propagates to the HTTP caller as failure.
    pub fn receive(&self, event: &NormalizedEvent, raw: &[u8], cfg: &Config) -> Result<Value> {
        let mut c = self.conn()?;
        let tx = c.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let hash = digest(raw);
        if let Some(old) = tx
            .query_row(
                "SELECT hash FROM deliveries WHERE source_id=?1 AND delivery_id=?2",
                params![event.source_id, event.delivery_id],
                |r| r.get::<_, String>(0),
            )
            .optional()?
        {
            if old != hash {
                bail!("delivery_conflict: delivery id reused with different payload");
            }
            return Ok(json!({"state":"duplicate","event_id":event.event_id}));
        }
        let count: i64 = tx.query_row("SELECT count(*) FROM events", [], |r| r.get(0))?;
        let bytes: i64 = tx.query_row(
            "SELECT coalesce(sum(length(raw)),0) FROM deliveries",
            [],
            |r| r.get(0),
        )?;
        if count >= cfg.max_stored_events as i64
            || bytes + raw.len() as i64 > cfg.max_stored_raw_bytes as i64
        {
            bail!("storage_limit: archive state before accepting more events");
        }
        let exists: bool = tx.query_row(
            "SELECT EXISTS(SELECT 1 FROM events WHERE id=?1)",
            [&event.event_id],
            |r| r.get(0),
        )?;
        tx.execute(
            "INSERT INTO deliveries VALUES(?1,?2,?3,?4,?5)",
            params![
                event.source_id,
                event.delivery_id,
                hash,
                raw,
                event.received_at_ms
            ],
        )?;
        if exists {
            tx.commit()?;
            return Ok(json!({"state":"duplicate","event_id":event.event_id}));
        }
        let source = cfg.source(&event.source_id).ok();
        if let Some(source) = source
            .filter(|source| source.kind == ProviderKind::Linear && source.tenant_id.is_some())
        {
            if let Some(metadata) = &event.linear {
                let key = linear_resource_key(source, &event.tenant_id, &metadata.issue_id)?;
                if key != event.resource_key || event.provider != ProviderKind::Linear {
                    bail!("linear_scope_mismatch: event metadata does not identify its resource");
                }
                if matches!(
                    event.action.as_str(),
                    "issue.create" | "issue.update" | "issue.remove"
                ) {
                    validate_linear_assignee(&metadata.assignee)?;
                    let observed_version = metadata.issue_updated_at_ms.filter(|at| *at > 0);
                    if let Some(version) = observed_version {
                        let transition = observed_version.filter(|_| {
                            matches!(metadata.assignee, LinearAssignee::Assigned { .. })
                                && (event.action == "issue.create"
                                    || (event.action == "issue.update"
                                        && metadata.previous_assignee != LinearAssignee::Unknown
                                        && metadata.previous_assignee != metadata.assignee))
                        });
                        merge_linear_projection(
                            &tx,
                            source,
                            &event.tenant_id,
                            &key,
                            &LinearIssueSnapshot {
                                issue_id: metadata.issue_id.clone(),
                                assignee: metadata.assignee.clone(),
                                updated_at_ms: version,
                            },
                            event.action == "issue.remove",
                            transition,
                        )?;
                        refresh_linear_pending(&tx, source, &key, transition, cfg.max_pending)?;
                    } else {
                        invalidate_linear_projection(
                            &tx,
                            source,
                            &event.tenant_id,
                            &key,
                            &metadata.issue_id,
                            event.action == "issue.remove",
                        )?;
                        refresh_linear_pending(&tx, source, &key, None, cfg.max_pending)?;
                    }
                }
            }
        }
        let scope = source
            .map(|source| linear_scope(&tx, source, event))
            .transpose()?
            .unwrap_or(LinearScopeDecision::Admitted);
        let mut state = if event.actionable {
            scope.state()
        } else {
            "cached"
        };
        let mut reason = event.skip_reason.as_deref().or(scope.reason());
        if matches!(state, "received" | "waiting_assignment")
            && pending_count(&tx)? >= cfg.max_pending
        {
            if source.is_some_and(|source| source.linear_assignee_id.is_some())
                && matches!(
                    event.action.as_str(),
                    "issue.create" | "issue.update" | "issue.remove"
                )
            {
                // Preserve signed ownership evidence even when model capacity
                // is exhausted. Its work waits durably behind active work.
                state = "waiting_capacity";
                reason = Some("linear_scope_capacity_wait");
            } else {
                bail!("queue_full: pending event capacity reached");
            }
        }
        tx.execute("INSERT INTO events(id,source_id,resource_key,data,state,reason,received) VALUES(?1,?2,?3,?4,?5,?6,?7)",params![event.event_id,event.source_id,event.resource_key,serde_json::to_string(event)?,state,reason,event.received_at_ms])?;
        tx.execute("INSERT INTO resources VALUES(?1,?2,1,?3,?4) ON CONFLICT(source_id,resource_key) DO UPDATE SET revision=revision+1, latest_event=CASE WHEN excluded.latest_at>=latest_at THEN excluded.latest_event ELSE latest_event END, latest_at=max(latest_at,excluded.latest_at)",params![event.source_id,event.resource_key,event.occurred_at_ms,event.event_id])?;
        tx.execute("UPDATE events SET source_version=(SELECT revision FROM resources WHERE source_id=?2 AND resource_key=?3) WHERE id=?1",params![event.event_id,event.source_id,event.resource_key])?;
        tx.execute("UPDATE drafts SET state='stale',reason='A new source event changed the discussion' WHERE source_id=?1 AND resource_key=?2 AND state IN ('draft','approved')",params![event.source_id,event.resource_key])?;
        wake_linear_capacity(&tx, cfg)?;
        tx.commit()?;
        Ok(
            json!({"state":state,"event_id":event.event_id,"received":true,"model_completed":false,"reply_sent":false}),
        )
    }
    pub fn events(&self, state: Option<&str>, limit: usize) -> Result<Vec<Value>> {
        let c = self.conn()?;
        let mut stmt=c.prepare("SELECT data,state,reason,routing_attempted FROM events WHERE (?1 IS NULL OR state=?1) ORDER BY rowid LIMIT ?2")?;
        let rows = stmt.query_map(params![state, limit.min(1000)], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, Option<String>>(2)?,
                r.get::<_, i64>(3)?,
            ))
        })?;
        rows.map(|r| { let(d,s,why,attempt)=r?; Ok(json!({"event":serde_json::from_str::<Value>(&d)?,"state":s,"reason":why,"routing_attempted":attempt>0})) }).collect()
    }
    pub fn event_detail(&self, id: &str) -> Result<Value> {
        let(data,state,reason,attempted,version):(String,String,Option<String>,i64,i64)=self.conn()?.query_row("SELECT data,state,reason,routing_attempted,source_version FROM events WHERE id=?1",[id],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?,r.get(4)?))).context("event not found")?;
        Ok(
            json!({"event":serde_json::from_str::<Value>(&data)?,"state":state,"reason":reason,"routing_attempted":attempted>0,"source_version":version}),
        )
    }
    pub fn event(&self, id: &str) -> Result<NormalizedEvent> {
        let data: String = self
            .conn()?
            .query_row("SELECT data FROM events WHERE id=?1", [id], |r| r.get(0))
            .context("event not found")?;
        Ok(serde_json::from_str(&data)?)
    }
    pub fn event_state(&self, id: &str, state: &str, reason: &str) -> Result<()> {
        self.conn()?.execute(
            "UPDATE events SET state=?2,reason=?3 WHERE id=?1",
            params![id, state, reason],
        )?;
        Ok(())
    }
    pub fn mark_routing(&self, id: &str) -> Result<bool> {
        Ok(self.conn()?.execute("UPDATE events SET state='routing',routing_attempted=1 WHERE id=?1 AND routing_attempted=0 AND state IN ('received','unassigned')",[id])?==1)
    }
    pub fn wake_unassigned(&self, source: &str, key: &str) -> Result<()> {
        self.conn()?.execute("UPDATE events SET state='received' WHERE source_id=?1 AND (resource_key=?2 OR json_extract(data,'$.conversation_key')=?2) AND state IN ('held','unassigned')",params![source,key])?;
        Ok(())
    }

    pub fn bind(
        &self,
        source: &str,
        key: &str,
        target: &ThreadTarget,
        origin: &str,
        expected: Option<i64>,
    ) -> Result<Binding> {
        if key.is_empty() || target.host_id.is_empty() || target.thread_id.is_empty() {
            bail!("binding requires exact resource, host, and thread identities");
        }
        let mut c = self.conn()?;
        let tx = c.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let old = tx
            .query_row(
                "SELECT version,target,enabled FROM bindings WHERE source_id=?1 AND key=?2",
                params![source, key],
                |r| {
                    Ok((
                        r.get::<_, i64>(0)?,
                        r.get::<_, String>(1)?,
                        r.get::<_, bool>(2)?,
                    ))
                },
            )
            .optional()?;
        let previous = old.as_ref().map(|row| row.0).unwrap_or(0);
        if expected.is_some_and(|value| value != previous) {
            bail!("binding_conflict: refresh the current binding before assigning");
        }
        let target_json = serde_json::to_string(target)?;
        let unchanged = old
            .as_ref()
            .is_some_and(|(_, value, enabled)| value == &target_json && *enabled);
        let version = if unchanged { previous } else { previous + 1 };
        if !unchanged {
            tx.execute("INSERT INTO bindings VALUES(?1,?2,?3,?4,1,?5) ON CONFLICT(source_id,key) DO UPDATE SET target=excluded.target,version=excluded.version,enabled=1,origin=excluded.origin",params![source,key,target_json,version,origin])?;
            tx.execute("UPDATE events SET state='binding_changed',reason='Binding changed; explicitly requeue to the reviewed target' WHERE id IN (SELECT event_id FROM jobs WHERE json_extract(data,'$.event.source_id')=?1 AND (json_extract(data,'$.binding_key')=?2 OR json_extract(data,'$.event.conversation_key')=?2) AND state IN ('queued','waiting_host','waiting_busy','held'))",params![source,key])?;
            tx.execute("UPDATE jobs SET state='binding_changed',reason='Binding changed; explicitly requeue to the reviewed target' WHERE json_extract(data,'$.event.source_id')=?1 AND (json_extract(data,'$.binding_key')=?2 OR json_extract(data,'$.event.conversation_key')=?2) AND state IN ('queued','waiting_host','waiting_busy','held')",params![source,key])?;
            tx.execute("UPDATE drafts SET state='stale',reason='Binding changed' WHERE source_id=?1 AND (json_extract(data,'$.binding_key')=?2 OR job_id IN (SELECT id FROM jobs WHERE json_extract(data,'$.event.source_id')=?1 AND json_extract(data,'$.event.conversation_key')=?2)) AND state IN ('draft','approved')",params![source,key])?;
        }
        if matches!(origin, "explicit" | "user" | "cli") {
            register_ownership(&tx, source, key, target)?;
        }
        tx.execute("UPDATE events SET state='received',reason=NULL WHERE source_id=?1 AND (resource_key=?2 OR json_extract(data,'$.conversation_key')=?2) AND state IN ('held','unassigned')",params![source,key])?;
        tx.execute(
            "INSERT INTO audit(at,kind,subject,detail) VALUES(?1,'binding_set',?2,?3)",
            params![
                now_ms(),
                key,
                json!({"source":source,"target":target,"version":version,"origin":origin})
                    .to_string()
            ],
        )?;
        tx.commit()?;
        Ok(Binding {
            key: key.into(),
            source_id: source.into(),
            target: target.clone(),
            version,
            enabled: true,
            origin: origin.into(),
        })
    }
    pub fn binding(&self, source: &str, key: &str) -> Result<Option<Binding>> {
        let c = self.conn()?;
        c.query_row(
            "SELECT target,version,enabled,origin FROM bindings WHERE source_id=?1 AND key=?2",
            params![source, key],
            |r| Ok((r.get::<_, String>(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
        )
        .optional()?
        .map(|(target, version, enabled, origin)| {
            Ok(Binding {
                key: key.into(),
                source_id: source.into(),
                target: serde_json::from_str(&target)?,
                version,
                enabled,
                origin,
            })
        })
        .transpose()
    }
    pub fn binding_for(&self, event: &NormalizedEvent) -> Result<Option<Binding>> {
        if let Some(key) = &event.conversation_key {
            if let Some(b) = self.binding(&event.source_id, key)? {
                return Ok(Some(b));
            }
        }
        self.binding(&event.source_id, &event.resource_key)
    }
    pub fn bindings(&self) -> Result<Vec<Binding>> {
        let c = self.conn()?;
        let mut s = c.prepare("SELECT source_id,key FROM bindings ORDER BY source_id,key")?;
        let keys = s
            .query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        keys.into_iter()
            .map(|(s, k)| self.binding(&s, &k)?.context("binding disappeared"))
            .collect()
    }
    pub fn pause_binding(&self, source: &str, key: &str, paused: bool) -> Result<()> {
        let mut c = self.conn()?;
        let tx = c.transaction_with_behavior(TransactionBehavior::Immediate)?;
        if tx.execute(
            "UPDATE bindings SET enabled=?3,version=version+1 WHERE source_id=?1 AND key=?2",
            params![source, key, !paused],
        )? == 0
        {
            bail!("binding not found");
        }
        tx.execute("UPDATE events SET state='binding_changed',reason='Binding pause/version changed; explicitly requeue after review' WHERE id IN (SELECT event_id FROM jobs WHERE json_extract(data,'$.event.source_id')=?1 AND json_extract(data,'$.binding_key')=?2 AND state IN ('queued','waiting_host','waiting_busy','held'))",params![source,key])?;
        tx.execute("UPDATE jobs SET state='binding_changed',reason='Binding pause/version changed; explicitly requeue after review' WHERE json_extract(data,'$.event.source_id')=?1 AND json_extract(data,'$.binding_key')=?2 AND state IN ('queued','waiting_host','waiting_busy','held')",params![source,key])?;
        tx.execute("UPDATE drafts SET state='stale',reason='Binding pause/version changed' WHERE source_id=?1 AND (json_extract(data,'$.binding_key')=?2 OR job_id IN (SELECT id FROM jobs WHERE json_extract(data,'$.event.source_id')=?1 AND json_extract(data,'$.event.conversation_key')=?2)) AND state IN ('draft','approved')",params![source,key])?;
        if !paused {
            tx.execute("UPDATE events SET state='received',reason=NULL WHERE source_id=?1 AND (resource_key=?2 OR json_extract(data,'$.conversation_key')=?2) AND state IN ('held','unassigned')",params![source,key])?;
        }
        tx.commit()?;
        Ok(())
    }

    pub fn put_card(&self, mut card: ContextCard, expected: Option<i64>) -> Result<ContextCard> {
        if card.id.is_empty()
            || card.source_ids.is_empty()
            || card.target.thread_id.is_empty()
            || card.summary.len() > 32768
            || card.evidence.len() > 100
        {
            bail!("invalid or unbounded context card");
        }
        let mut c = self.conn()?;
        let tx = c.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let old: Option<(String, i64)> = tx
            .query_row(
                "SELECT data,revision FROM cards WHERE id=?1",
                [&card.id],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()?;
        let revision = old.as_ref().map_or(0, |row| row.1);
        if expected.is_some_and(|value| value != revision) {
            bail!("card_conflict: refresh card revision");
        }
        let markers: Vec<_> = card
            .evidence
            .iter()
            .filter(|item| item.starts_with("ownership:"))
            .cloned()
            .collect();
        if let Some((data, _)) = old {
            let previous: ContextCard = serde_json::from_str(&data)?;
            let previous_markers: Vec<_> = previous
                .evidence
                .iter()
                .filter(|item| item.starts_with("ownership:"))
                .cloned()
                .collect();
            if markers != previous_markers || card.owned_resources != previous.owned_resources {
                bail!("ownership_protected: use an explicit binding action");
            }
            if !markers.is_empty()
                && (card.target != previous.target || card.source_ids != previous.source_ids)
            {
                bail!("ownership_protected: a bound card cannot change target or source scope");
            }
        } else if !markers.is_empty() || !card.owned_resources.is_empty() {
            bail!("ownership_protected: only explicit binding establishes ownership");
        }
        card.revision = revision + 1;
        tx.execute("INSERT INTO cards VALUES(?1,?2,?3) ON CONFLICT(id) DO UPDATE SET data=excluded.data,revision=excluded.revision",params![card.id,serde_json::to_string(&card)?,card.revision])?;
        tx.commit()?;
        Ok(card)
    }
    pub fn cards(&self, source: Option<&str>) -> Result<Vec<ContextCard>> {
        let c = self.conn()?;
        let mut s=c.prepare("SELECT data FROM cards WHERE ?1 IS NULL OR EXISTS(SELECT 1 FROM json_each(cards.data,'$.source_ids') WHERE value=?1) ORDER BY id")?;
        let rows = s.query_map([source], |r| r.get::<_, String>(0))?;
        rows.map(|row| Ok(serde_json::from_str(&row?)?)).collect()
    }
    /// Count all exact, source-scoped owners before candidate ranking/truncation.
    pub fn owners(&self, event: &NormalizedEvent) -> Result<Vec<ContextCard>> {
        let marker = format!("ownership:{}:{}", event.source_id, event.resource_key);
        let mut owners = std::collections::BTreeMap::new();
        for card in self.cards(Some(&event.source_id))? {
            if card.enabled
                && card.owned_resources.contains(&event.resource_key)
                && card.evidence.contains(&marker)
            {
                owners.entry(target_key(&card.target)).or_insert(card);
            }
        }
        Ok(owners.into_values().collect())
    }
    pub fn mark_cards_stale(
        &self,
        host: &str,
        thread: Option<&str>,
        reason: &str,
    ) -> Result<usize> {
        let mut c = self.conn()?;
        let tx = c.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let rows = {
            let mut s=tx.prepare("SELECT data FROM cards WHERE json_extract(data,'$.target.host_id')=?1 AND (?2 IS NULL OR json_extract(data,'$.target.thread_id')=?2)")?;
            let rows = s
                .query_map(params![host, thread], |r| r.get::<_, String>(0))?
                .collect::<rusqlite::Result<Vec<_>>>()?;
            rows
        };
        let mut count = 0;
        for data in rows {
            let mut card: ContextCard = serde_json::from_str(&data)?;
            if !card.coverage.starts_with("stale:") {
                card.coverage = format!("stale:{reason}");
                card.revision += 1;
                tx.execute(
                    "UPDATE cards SET data=?2,revision=?3 WHERE id=?1",
                    params![card.id, serde_json::to_string(&card)?, card.revision],
                )?;
                count += 1;
            }
        }
        tx.commit()?;
        Ok(count)
    }
    pub fn card(&self, id: &str) -> Result<ContextCard> {
        let data: String =
            self.conn()?
                .query_row("SELECT data FROM cards WHERE id=?1", [id], |r| r.get(0))?;
        Ok(serde_json::from_str(&data)?)
    }
    pub fn candidates(&self, event: &NormalizedEvent, limit: usize) -> Result<Vec<ContextCard>> {
        let words: Vec<String> = event
            .content
            .split(|c: char| !c.is_alphanumeric())
            .filter(|s| s.len() > 2)
            .take(40)
            .map(str::to_lowercase)
            .collect();
        let mut cards: Vec<_> = self
            .cards(Some(&event.source_id))?
            .into_iter()
            .filter(|c| c.enabled)
            .map(|c| {
                let text = format!("{} {}", c.title, c.summary).to_lowercase();
                let exact = c.owned_resources.contains(&event.resource_key);
                let refs = c.referenced_resources.contains(&event.resource_key);
                let score = if exact {
                    10000
                } else if refs {
                    1000
                } else {
                    0
                } + words.iter().filter(|w| text.contains(w.as_str())).count();
                (score, c)
            })
            .collect();
        cards.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.id.cmp(&b.1.id)));
        Ok(cards
            .into_iter()
            .take(limit.min(10))
            .map(|(_, c)| c)
            .collect())
    }

    pub fn enqueue(&self, event: &NormalizedEvent, binding: &Binding) -> Result<Option<Job>> {
        let mut c = self.conn()?;
        let tx = c.transaction_with_behavior(TransactionBehavior::Immediate)?;
        if event.provider == ProviderKind::Linear {
            let config = Config::load(self.path.parent().context("database profile missing")?)?;
            let source = config.source(&event.source_id)?;
            let scope = linear_scope(&tx, source, event)?;
            if scope != LinearScopeDecision::Admitted {
                tx.execute(
                    "UPDATE events SET state=?2,reason=?3 WHERE id=?1",
                    params![event.event_id, scope.state(), scope.reason()],
                )?;
                tx.commit()?;
                return Ok(None);
            }
        }
        let valid:bool=tx.query_row("SELECT EXISTS(SELECT 1 FROM bindings WHERE source_id=?1 AND key=?2 AND version=?3 AND enabled=1)",params![binding.source_id,binding.key,binding.version],|r|r.get(0))?;
        if !valid {
            bail!("binding_conflict: binding changed before enqueue");
        }
        let source_version: i64 = tx.query_row(
            "SELECT source_version FROM events WHERE id=?1",
            [&event.event_id],
            |r| r.get(0),
        )?;
        let job = Job {
            id: uuid::Uuid::new_v4().to_string(),
            event: event.clone(),
            target: binding.target.clone(),
            binding_key: binding.key.clone(),
            binding_version: binding.version,
            source_revision: source_version.to_string(),
            state: "queued".into(),
            dispatch_id: None,
            turn_id: None,
            reason: None,
        };
        let inserted=tx.execute("INSERT OR IGNORE INTO jobs(id,event_id,data,target_key,state,created) VALUES(?1,?2,?3,?4,'queued',?5)",params![job.id,event.event_id,serde_json::to_string(&job)?,target_key(&job.target),now_ms()])?;
        if inserted == 1 {
            tx.execute(
                "UPDATE events SET state='queued',reason=NULL WHERE id=?1",
                [&event.event_id],
            )?;
        }
        tx.commit()?;
        Ok(if inserted == 1 { Some(job) } else { None })
    }
    pub fn jobs(&self, state: Option<&str>, limit: usize) -> Result<Vec<Job>> {
        let c = self.conn()?;
        let mut s=c.prepare("SELECT data,state,reason,dispatch_id,turn_id FROM jobs WHERE (?1 IS NULL OR state=?1) ORDER BY rowid LIMIT ?2")?;
        let rows = s.query_map(params![state, limit.min(1000)], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get(1)?,
                r.get(2)?,
                r.get(3)?,
                r.get(4)?,
            ))
        })?;
        rows.map(|r| {
            let (d, state, reason, dispatch_id, turn_id) = r?;
            let mut j: Job = serde_json::from_str(&d)?;
            j.state = state;
            j.reason = reason;
            j.dispatch_id = dispatch_id;
            j.turn_id = turn_id;
            Ok(j)
        })
        .collect()
    }
    pub fn job(&self, id: &str) -> Result<Job> {
        let c = self.conn()?;
        let (d, state, reason, dispatch_id, turn_id) = c
            .query_row(
                "SELECT data,state,reason,dispatch_id,turn_id FROM jobs WHERE id=?1",
                [id],
                |r| {
                    Ok((
                        r.get::<_, String>(0)?,
                        r.get(1)?,
                        r.get(2)?,
                        r.get(3)?,
                        r.get(4)?,
                    ))
                },
            )
            .context("job not found")?;
        let mut j: Job = serde_json::from_str(&d)?;
        j.state = state;
        j.reason = reason;
        j.dispatch_id = dispatch_id;
        j.turn_id = turn_id;
        Ok(j)
    }
    /// Persist intent and reserve the entire target, not just one resource.
    pub fn claim(&self, id: &str) -> Result<Option<Job>> {
        let mut c = self.conn()?;
        let tx = c.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let raw: Option<(String, String, i64)> = tx
            .query_row(
                "SELECT data,target_key,rowid FROM jobs WHERE id=?1 AND state='queued'",
                [id],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .optional()?;
        let Some((data, target, order)) = raw else {
            return Ok(None);
        };
        let mut job: Job = serde_json::from_str(&data)?;
        if job.event.provider == ProviderKind::Linear {
            let config = Config::load(self.path.parent().context("database profile missing")?)?;
            let source = config.source(&job.event.source_id)?;
            let scope = linear_scope(&tx, source, &job.event)?;
            if scope != LinearScopeDecision::Admitted {
                tx.execute(
                    "UPDATE jobs SET state=?2,reason=?3 WHERE id=?1",
                    params![id, scope.state(), scope.reason()],
                )?;
                tx.execute(
                    "UPDATE events SET state=?2,reason=?3 WHERE id=?1",
                    params![job.event.event_id, scope.state(), scope.reason()],
                )?;
                tx.commit()?;
                return Ok(None);
            }
        }
        let valid = transaction_binding_for(&tx, &job.event)?.is_some_and(|binding| {
            binding.enabled
                && binding.key == job.binding_key
                && binding.version == job.binding_version
                && binding.target == job.target
        });
        if !valid {
            tx.execute("UPDATE jobs SET state='binding_changed',reason='Binding version or enabled state changed' WHERE id=?1",[id])?;
            tx.execute("UPDATE events SET state='binding_changed',reason='Binding version or enabled state changed' WHERE id=?1",[&job.event.event_id])?;
            tx.commit()?;
            return Ok(None);
        }
        let earlier:bool=tx.query_row("SELECT EXISTS(SELECT 1 FROM jobs WHERE target_key=?1 AND rowid<?2 AND state IN ('queued','waiting_host','waiting_busy','held','dispatching','unknown'))",params![target,order],|row|row.get(0))?;
        if earlier {
            return Ok(None);
        }
        if tx.execute(
            "INSERT OR IGNORE INTO locks VALUES(?1,?2)",
            params![target, id],
        )? == 0
        {
            return Ok(None);
        }
        job.state = "dispatching".into();
        job.dispatch_id = Some(uuid::Uuid::new_v4().to_string());
        tx.execute("UPDATE jobs SET state='dispatching',dispatch_id=?2,reason=NULL WHERE id=?1 AND state='queued'",params![id,job.dispatch_id])?;
        tx.execute(
            "UPDATE events SET state='dispatching',reason=NULL WHERE id=?1",
            [&job.event.event_id],
        )?;
        tx.commit()?;
        Ok(Some(job))
    }
    pub fn finish_job(
        &self,
        id: &str,
        state: &str,
        reason: &str,
        turn: Option<&str>,
        release: bool,
    ) -> Result<()> {
        let mut c = self.conn()?;
        let tx = c.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let target: String =
            tx.query_row("SELECT target_key FROM jobs WHERE id=?1", [id], |row| {
                row.get(0)
            })?;
        tx.execute(
            "UPDATE jobs SET state=?2,reason=?3,turn_id=coalesce(?4,turn_id) WHERE id=?1",
            params![id, state, reason, turn],
        )?;
        if release {
            tx.execute("DELETE FROM locks WHERE job_id=?1", [id])?;
        }
        tx.execute(
            "UPDATE events SET state=?2,reason=?3 WHERE id=(SELECT event_id FROM jobs WHERE id=?1)",
            params![id, state, reason],
        )?;
        if release && !matches!(state, "waiting_host" | "waiting_busy" | "held") {
            wake_router_target(&tx, &target)?;
        }
        if release {
            if let Some(dir) = self.path.parent() {
                if let Ok(config) = Config::load(dir) {
                    wake_linear_capacity(&tx, &config)?;
                }
            }
        }
        tx.commit()?;
        Ok(())
    }
    pub fn retry(&self, id: &str) -> Result<()> {
        let mut c = self.conn()?;
        let tx = c.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let (data, state): (String, String) =
            tx.query_row("SELECT data,state FROM jobs WHERE id=?1", [id], |row| {
                Ok((row.get(0)?, row.get(1)?))
            })?;
        if !matches!(
            state.as_str(),
            "waiting_host" | "waiting_busy" | "held" | "rejected" | "binding_changed"
        ) {
            bail!("unsafe_retry: only work known not to have started can be retried; reconcile unknown work first");
        }
        let mut job: Job = serde_json::from_str(&data)?;
        let binding =
            transaction_binding_for(&tx, &job.event)?.context("bind an existing thread first")?;
        if !binding.enabled {
            bail!("target paused");
        }
        let locked: bool = tx.query_row(
            "SELECT EXISTS(SELECT 1 FROM locks WHERE job_id=?1)",
            [id],
            |row| row.get(0),
        )?;
        if locked {
            bail!("unsafe_retry: a target reservation still exists; reconcile before retrying");
        }
        job.target = binding.target;
        job.binding_key = binding.key;
        job.binding_version = binding.version;
        job.state = "queued".into();
        job.dispatch_id = None;
        job.turn_id = None;
        job.reason = None;
        let changed=tx.execute("UPDATE jobs SET data=?2,target_key=?3,state='queued',reason=NULL,dispatch_id=NULL,turn_id=NULL WHERE id=?1 AND state=?4",params![id,serde_json::to_string(&job)?,target_key(&job.target),state])?;
        if changed != 1 {
            bail!("retry_conflict: job state changed");
        }
        tx.execute(
            "UPDATE events SET state='queued',reason=NULL WHERE id=?1",
            [&job.event.event_id],
        )?;
        tx.commit()?;
        Ok(())
    }
    pub fn wake_waiters(&self, host_id: Option<&str>) -> Result<()> {
        let mut c = self.conn()?;
        let tx = c.transaction_with_behavior(TransactionBehavior::Immediate)?;
        tx.execute("UPDATE events SET state='queued',reason=NULL WHERE id IN (SELECT event_id FROM jobs WHERE state IN ('waiting_host','waiting_busy','held') AND (?1 IS NULL OR json_extract(data,'$.target.host_id')=?1))",params![host_id])?;
        tx.execute("UPDATE jobs SET state='queued',reason=NULL WHERE state IN ('waiting_host','waiting_busy','held') AND (?1 IS NULL OR json_extract(data,'$.target.host_id')=?1)",params![host_id])?;
        if host_id.is_none() {
            tx.execute("UPDATE events SET state='received',reason=NULL WHERE state='held' AND NOT EXISTS(SELECT 1 FROM jobs WHERE event_id=events.id)",[])?;
            let config = Config::load(self.path.parent().context("database profile missing")?)?;
            for source in config
                .sources
                .iter()
                .filter(|source| source.enabled && source.linear_assignee_id.is_some())
            {
                let resources = {
                    let mut statement = tx.prepare("SELECT DISTINCT resource_key FROM events WHERE source_id=?1 AND state='waiting_assignment'")?;
                    let rows = statement.query_map([&source.id], |row| row.get::<_, String>(0))?;
                    rows.collect::<rusqlite::Result<Vec<_>>>()?
                };
                for resource in resources {
                    refresh_linear_pending(&tx, source, &resource, None, config.max_pending)?;
                }
            }
            wake_linear_capacity(&tx, &config)?;
        }
        tx.execute("UPDATE events SET state='received',reason=NULL WHERE state='router_waiting' AND (?1 IS NULL OR id IN (SELECT event_id FROM router_dispatches WHERE json_extract(target,'$.host_id')=?1))",params![host_id])?;
        tx.commit()?;
        Ok(())
    }

    pub fn ready_jobs(&self, limit: usize) -> Result<Vec<Job>> {
        let c = self.conn()?;
        let mut s=c.prepare("SELECT j.id FROM jobs j WHERE j.state='queued' AND NOT EXISTS(SELECT 1 FROM locks l WHERE l.target_key=j.target_key) AND NOT EXISTS(SELECT 1 FROM jobs p WHERE p.target_key=j.target_key AND p.rowid<j.rowid AND p.state IN ('queued','waiting_host','waiting_busy','held','dispatching','unknown')) ORDER BY j.rowid LIMIT ?1")?;
        let ids = s
            .query_map([limit.min(1000)], |row| row.get::<_, String>(0))?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        ids.iter().map(|id| self.job(id)).collect()
    }
    pub fn has_ready_work(&self) -> Result<bool> {
        let received: bool = self.conn()?.query_row(
            "SELECT EXISTS(SELECT 1 FROM events WHERE state='received')",
            [],
            |r| r.get(0),
        )?;
        Ok(received || !self.ready_jobs(1)?.is_empty())
    }
    pub fn job_binding_current(&self, job: &Job) -> Result<bool> {
        Ok(self.binding_for(&job.event)?.is_some_and(|b| {
            b.enabled
                && b.key == job.binding_key
                && b.version == job.binding_version
                && b.target == job.target
        }))
    }
    /// Router requests reserve the same host/thread namespace as work turns.
    /// A durable intent is committed before any host request may be made.
    pub fn begin_router(&self, event_id: &str, target: &ThreadTarget) -> Result<Option<String>> {
        let mut c = self.conn()?;
        let tx = c.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let (state, attempted): (String, bool) = tx.query_row(
            "SELECT state,routing_attempted FROM events WHERE id=?1",
            [event_id],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )?;
        if attempted || !matches!(state.as_str(), "received" | "router_waiting" | "unassigned") {
            return Ok(None);
        }
        let key = target_key(target);
        let dispatch = uuid::Uuid::new_v4().to_string();
        let lock = format!("router:{event_id}");
        let waiting:bool=tx.query_row("SELECT EXISTS(SELECT 1 FROM locks WHERE target_key=?1) OR EXISTS(SELECT 1 FROM jobs WHERE target_key=?1 AND state IN ('queued','waiting_host','waiting_busy','held','dispatching','unknown')) OR EXISTS(SELECT 1 FROM router_dispatches r JOIN events e ON e.id=r.event_id WHERE r.target_key=?1 AND r.state IN ('waiting','dispatching','unknown') AND e.state IN ('received','routing','router_waiting','router_unknown') AND e.rowid<(SELECT rowid FROM events WHERE id=?2))",params![key,event_id],|r|r.get(0))?;
        let state = if waiting { "waiting" } else { "dispatching" };
        tx.execute("INSERT INTO router_dispatches(event_id,dispatch_id,target_key,target,state,created) VALUES(?1,?2,?3,?4,?5,?6) ON CONFLICT(event_id) DO UPDATE SET dispatch_id=excluded.dispatch_id,target_key=excluded.target_key,target=excluded.target,state=excluded.state,turn_id=NULL,reason=NULL,created=excluded.created",params![event_id,dispatch,key,serde_json::to_string(target)?,state,now_ms()])?;
        if waiting {
            tx.execute("UPDATE events SET state='router_waiting',reason='Router target has earlier or uncertain work' WHERE id=?1",[event_id])?;
            tx.commit()?;
            return Ok(None);
        }
        tx.execute("INSERT INTO locks VALUES(?1,?2)", params![key, lock])?;
        tx.execute(
            "UPDATE events SET state='routing',reason=NULL,routing_attempted=1 WHERE id=?1",
            [event_id],
        )?;
        tx.execute(
            "INSERT INTO audit(at,kind,subject,detail) VALUES(?1,'router_dispatch_intent',?2,?3)",
            params![
                now_ms(),
                event_id,
                json!({"dispatch_id":dispatch,"target":target}).to_string()
            ],
        )?;
        tx.commit()?;
        Ok(Some(dispatch))
    }
    pub fn finish_router(
        &self,
        event_id: &str,
        state: &str,
        reason: &str,
        turn: Option<&str>,
    ) -> Result<()> {
        if !matches!(
            state,
            "completed" | "failed" | "rejected" | "waiting" | "unknown"
        ) {
            bail!("invalid router outcome state");
        }
        let mut c = self.conn()?;
        let tx = c.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let key: String = tx.query_row(
            "SELECT target_key FROM router_dispatches WHERE event_id=?1",
            [event_id],
            |r| r.get(0),
        )?;
        tx.execute("UPDATE router_dispatches SET state=?2,reason=?3,turn_id=coalesce(?4,turn_id) WHERE event_id=?1",params![event_id,state,reason,turn])?;
        let event_state = match state {
            "unknown" => "router_unknown",
            "waiting" => "router_waiting",
            _ => "unassigned",
        };
        tx.execute("UPDATE events SET state=?2,reason=?3,routing_attempted=CASE WHEN ?4='waiting' THEN 0 ELSE routing_attempted END WHERE id=?1",params![event_id,event_state,reason,state])?;
        if state != "unknown" {
            tx.execute(
                "DELETE FROM locks WHERE job_id=?1",
                [format!("router:{event_id}")],
            )?;
            if state != "waiting" {
                wake_router_target(&tx, &key)?;
            }
        }
        tx.execute(
            "INSERT INTO audit(at,kind,subject,detail) VALUES(?1,'router_dispatch_outcome',?2,?3)",
            params![
                now_ms(),
                event_id,
                json!({"state":state,"turn_id":turn,"reason":reason}).to_string()
            ],
        )?;
        tx.commit()?;
        Ok(())
    }
    pub fn router_dispatches(&self) -> Result<Vec<Value>> {
        let c = self.conn()?;
        let mut s=c.prepare("SELECT event_id,dispatch_id,target,state,turn_id,reason FROM router_dispatches ORDER BY created,event_id LIMIT 1000")?;
        let rows = s.query_map([], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
                r.get::<_, String>(3)?,
                r.get::<_, Option<String>>(4)?,
                r.get::<_, Option<String>>(5)?,
            ))
        })?;
        rows.map(|row|{let(event,dispatch,target,state,turn,reason)=row?;Ok(json!({"event_id":event,"dispatch_id":dispatch,"target":serde_json::from_str::<Value>(&target)?,"state":state,"turn_id":turn,"reason":reason}))}).collect()
    }
    pub fn reconcile_router_not_started(&self, event_id: &str, evidence: &str) -> Result<()> {
        self.reconcile_router(event_id, evidence, false)
    }
    pub fn reconcile_router_completed(&self, event_id: &str, evidence: &str) -> Result<()> {
        self.reconcile_router(event_id, evidence, true)
    }
    fn reconcile_router(&self, event_id: &str, evidence: &str, completed: bool) -> Result<()> {
        if evidence.trim().len() < 8 {
            bail!("concrete host reconciliation evidence is required");
        }
        let mut c = self.conn()?;
        let tx = c.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let (state, key, dispatch): (String, String, String) = tx.query_row(
            "SELECT state,target_key,dispatch_id FROM router_dispatches WHERE event_id=?1",
            [event_id],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )?;
        if state != "unknown" {
            bail!("only unknown router dispatches can be reconciled");
        }
        tx.execute(
            "UPDATE router_dispatches SET state=?2,reason=?3 WHERE event_id=?1 AND state='unknown'",
            params![
                event_id,
                if completed { "completed" } else { "rejected" },
                evidence
            ],
        )?;
        tx.execute(
            "DELETE FROM locks WHERE job_id=?1",
            [format!("router:{event_id}")],
        )?;
        tx.execute(
            "UPDATE events SET state=?2,routing_attempted=?3,reason=?4 WHERE id=?1",
            params![
                event_id,
                if completed { "unassigned" } else { "received" },
                completed,
                evidence
            ],
        )?;
        wake_router_target(&tx, &key)?;
        tx.execute(
            "INSERT INTO audit(at,kind,subject,detail) VALUES(?1,'router_reconciled',?2,?3)",
            params![
                now_ms(),
                event_id,
                json!({"completed":completed,"dispatch_id":dispatch,"evidence":evidence})
                    .to_string()
            ],
        )?;
        tx.commit()?;
        Ok(())
    }

    /// Called under the daemon's exclusive process lock, once on startup.
    pub fn recover(&self) -> Result<usize> {
        let mut c = self.conn()?;
        let tx = c.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let n=tx.execute("UPDATE jobs SET state='unknown',reason='Process stopped after durable dispatch intent; reconcile before retry' WHERE state='dispatching'",[])?;
        tx.execute("UPDATE events SET state='unknown',reason='Dispatch outcome requires reconciliation' WHERE id IN (SELECT event_id FROM jobs WHERE state='unknown')",[])?;
        tx.execute("UPDATE drafts SET state='unknown',reason='Process stopped during send; reconcile provider before retry' WHERE state='sending'",[])?;
        let routers=tx.execute("UPDATE router_dispatches SET state='unknown',reason='Process stopped after router dispatch intent; reconcile before retry' WHERE state='dispatching'",[])?;
        tx.execute("UPDATE events SET state='router_unknown',reason='Router outcome requires reconciliation' WHERE id IN (SELECT event_id FROM router_dispatches WHERE state='unknown')",[])?;
        tx.execute("UPDATE events SET state='unassigned',reason='Legacy router interruption requires review' WHERE state='routing'",[])?;
        tx.execute("UPDATE events SET state='queued',reason=NULL WHERE id IN (SELECT event_id FROM jobs WHERE state IN ('waiting_host','waiting_busy'))",[])?;
        tx.execute("UPDATE jobs SET state='queued',reason=NULL WHERE state IN ('waiting_host','waiting_busy')",[])?;
        tx.commit()?;
        Ok(n + routers)
    }
    pub fn reconcile_not_started(&self, id: &str, evidence: &str) -> Result<()> {
        if evidence.trim().len() < 8 {
            bail!("concrete host reconciliation evidence is required");
        }
        let mut c = self.conn()?;
        let tx = c.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let (state, key, event, dispatch): (String, String, String, Option<String>) = tx
            .query_row(
                "SELECT state,target_key,event_id,dispatch_id FROM jobs WHERE id=?1",
                [id],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
            )?;
        if state != "unknown" {
            bail!("reconciliation needs an unknown job");
        }
        tx.execute("UPDATE jobs SET state='rejected',reason='Operator verified host did not accept this dispatch' WHERE id=?1 AND state='unknown'",[id])?;
        tx.execute("UPDATE events SET state='rejected',reason='Operator verified host did not accept this dispatch' WHERE id=?1",[event])?;
        tx.execute("DELETE FROM locks WHERE job_id=?1", [id])?;
        wake_router_target(&tx, &key)?;
        tx.execute("INSERT INTO audit(at,kind,subject,detail) VALUES(?1,'dispatch_reconciled_not_started',?2,?3)",params![now_ms(),id,json!({"evidence":evidence,"dispatch_id":dispatch}).to_string()])?;
        tx.commit()?;
        Ok(())
    }

    pub fn resource_revision(&self, source: &str, key: &str) -> Result<String> {
        let n: i64 = self.conn()?.query_row(
            "SELECT revision FROM resources WHERE source_id=?1 AND resource_key=?2",
            params![source, key],
            |r| r.get(0),
        )?;
        Ok(n.to_string())
    }
    pub fn save_draft(&self, job: &Job, body: &str, identity: &str) -> Result<Option<Draft>> {
        let Some(target) = job.event.reply_target.clone() else {
            return Ok(None);
        };
        let mut c = self.conn()?;
        let tx = c.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let revision: i64 = tx.query_row(
            "SELECT revision FROM resources WHERE source_id=?1 AND resource_key=?2",
            params![job.event.source_id, job.event.resource_key],
            |row| row.get(0),
        )?;
        let binding_ok = transaction_binding_for(&tx, &job.event)?.is_some_and(|binding| {
            binding.enabled
                && binding.key == job.binding_key
                && binding.version == job.binding_version
                && binding.target == job.target
        });
        let state = if revision.to_string() == job.source_revision && binding_ok {
            "draft"
        } else {
            "stale"
        };
        let draft = Draft {
            id: uuid::Uuid::new_v4().to_string(),
            job_id: job.id.clone(),
            source_id: job.event.source_id.clone(),
            resource_key: job.event.resource_key.clone(),
            event_id: job.event.event_id.clone(),
            body: body.to_string(),
            reply_target: target,
            source_revision: job.source_revision.clone(),
            source_event_revision: job.event.revision.clone(),
            binding_key: job.binding_key.clone(),
            binding_version: job.binding_version,
            revision: 1,
            sending_identity: identity.into(),
            state: state.into(),
        };
        tx.execute("INSERT INTO drafts(id,job_id,source_id,resource_key,data,state) VALUES(?1,?2,?3,?4,?5,?6)",params![draft.id,draft.job_id,draft.source_id,draft.resource_key,serde_json::to_string(&draft)?,state])?;
        tx.commit()?;
        Ok(Some(draft))
    }
    pub fn edit_draft(&self, id: &str, body: &str) -> Result<Draft> {
        if body.trim().is_empty() || body.len() > 65536 {
            bail!("reply must be non-empty and at most 64 KiB");
        }
        let mut c = self.conn()?;
        let tx = c.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let (data, state): (String, String) =
            tx.query_row("SELECT data,state FROM drafts WHERE id=?1", [id], |row| {
                Ok((row.get(0)?, row.get(1)?))
            })?;
        if !matches!(state.as_str(), "draft" | "stale") {
            bail!("draft cannot be edited while sending, sent, skipped, or uncertain");
        }
        let mut draft: Draft = serde_json::from_str(&data)?;
        draft.body = body.into();
        draft.revision += 1;
        draft.state = state;
        let current: i64 = tx.query_row(
            "SELECT revision FROM resources WHERE source_id=?1 AND resource_key=?2",
            params![draft.source_id, draft.resource_key],
            |row| row.get(0),
        )?;
        let event_data: String = tx.query_row(
            "SELECT data FROM events WHERE id=?1",
            [&draft.event_id],
            |row| row.get(0),
        )?;
        let event: NormalizedEvent = serde_json::from_str(&event_data)?;
        let binding_ok = transaction_binding_for(&tx, &event)?.is_some_and(|binding| {
            binding.enabled
                && binding.key == draft.binding_key
                && binding.version == draft.binding_version
        });
        if draft.state == "stale" || current.to_string() != draft.source_revision || !binding_ok {
            draft.state = "stale".into();
        }
        tx.execute("UPDATE drafts SET data=?2,state=?3,reason='Edited; previous approval is invalid' WHERE id=?1",params![id,serde_json::to_string(&draft)?,draft.state])?;
        tx.commit()?;
        Ok(draft)
    }
    pub fn drafts(&self) -> Result<Vec<Draft>> {
        let c = self.conn()?;
        let mut s = c.prepare("SELECT data,state FROM drafts ORDER BY rowid DESC LIMIT 1000")?;
        let rows = s.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))?;
        rows.map(|r| {
            let (data, state) = r?;
            let mut d: Draft = serde_json::from_str(&data)?;
            d.state = state;
            Ok(d)
        })
        .collect()
    }
    pub fn draft(&self, id: &str) -> Result<Draft> {
        let (data, state): (String, String) =
            self.conn()?
                .query_row("SELECT data,state FROM drafts WHERE id=?1", [id], |r| {
                    Ok((r.get(0)?, r.get(1)?))
                })?;
        let mut d: Draft = serde_json::from_str(&data)?;
        d.state = state;
        Ok(d)
    }
    pub fn begin_send(&self, id: &str) -> Result<Draft> {
        let mut c = self.conn()?;
        let tx = c.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let (data, state): (String, String) =
            tx.query_row("SELECT data,state FROM drafts WHERE id=?1", [id], |r| {
                Ok((r.get(0)?, r.get(1)?))
            })?;
        if state != "draft" {
            bail!("draft cannot be sent from its current state");
        }
        let mut d: Draft = serde_json::from_str(&data)?;
        let current: i64 = tx.query_row(
            "SELECT revision FROM resources WHERE source_id=?1 AND resource_key=?2",
            params![d.source_id, d.resource_key],
            |r| r.get(0),
        )?;
        let event_data: String = tx.query_row(
            "SELECT data FROM events WHERE id=?1",
            [&d.event_id],
            |row| row.get(0),
        )?;
        let event: NormalizedEvent = serde_json::from_str(&event_data)?;
        let binding_ok = transaction_binding_for(&tx, &event)?.is_some_and(|binding| {
            binding.enabled && binding.key == d.binding_key && binding.version == d.binding_version
        });
        if current.to_string() != d.source_revision || !binding_ok {
            bail!("draft_stale: discussion or binding changed; generate and review a fresh draft");
        }
        tx.execute("UPDATE drafts SET state='sending' WHERE id=?1", [id])?;
        tx.commit()?;
        d.state = "draft".into();
        Ok(d)
    }
    /// Compare-and-set only known-unsent drafts. In-flight or uncertain writes
    /// retain their evidence and can never be hidden by a skip action.
    pub fn skip_draft(&self, id: &str) -> Result<()> {
        let mut connection = self.conn()?;
        let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        if transaction.execute("UPDATE drafts SET state='skipped',reason='Operator skipped this unsent draft' WHERE id=?1 AND state IN ('draft','stale')",[id])?!=1 {
            bail!("draft_not_skippable: only draft or stale replies may be skipped; reconcile sending or unknown outcomes first");
        }
        transaction.execute(
            "INSERT INTO audit(at,kind,subject,detail) VALUES(?1,'draft_skipped',?2,'{}')",
            params![now_ms(), id],
        )?;
        transaction.commit()?;
        Ok(())
    }
    pub fn draft_state(&self, id: &str, state: &str, reason: &str, result: &Value) -> Result<()> {
        self.conn()?.execute(
            "UPDATE drafts SET state=?2,reason=?3,result=?4 WHERE id=?1",
            params![id, state, reason, result.to_string()],
        )?;
        Ok(())
    }
    pub fn status(&self) -> Result<Value> {
        let c = self.conn()?;
        let counts = |table: &str| -> Result<Value> {
            let mut s = c.prepare(&format!(
                "SELECT state,count(*) FROM {table} GROUP BY state"
            ))?;
            let rows = s.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?)))?;
            let mut map = serde_json::Map::new();
            for r in rows {
                let (k, v) = r?;
                map.insert(k, json!(v));
            }
            Ok(Value::Object(map))
        };
        Ok(
            json!({"events":counts("events")?,"jobs":counts("jobs")?,"drafts":counts("drafts")?,"router_dispatches":counts("router_dispatches")?,"bindings":self.bindings()?.len(),"context_cards":self.cards(None)?.len()}),
        )
    }
    pub fn relay_pending(
        &self,
        device: &str,
        sources: &[String],
        limit: usize,
    ) -> Result<Vec<NormalizedEvent>> {
        if sources.is_empty() || limit == 0 {
            return Ok(Vec::new());
        }
        let c = self.conn()?;
        let mut statement=c.prepare("SELECT data FROM events e WHERE e.source_id IN (SELECT value FROM json_each(?2)) AND NOT EXISTS(SELECT 1 FROM relay_acks a WHERE a.device=?1 AND a.event_id=e.id) ORDER BY e.rowid LIMIT ?3")?;
        let rows = statement.query_map(
            params![device, serde_json::to_string(sources)?, limit.min(1000)],
            |row| row.get::<_, String>(0),
        )?;
        rows.map(|row| Ok(serde_json::from_str(&row?)?)).collect()
    }
    pub fn completion_receipts(
        &self,
        sources: &[String],
        offset: usize,
        limit: usize,
    ) -> Result<Vec<String>> {
        if sources.is_empty() || limit == 0 {
            return Ok(Vec::new());
        }
        let c = self.conn()?;
        let mut s=c.prepare("SELECT event_id FROM jobs WHERE state IN ('drafted','completed','no_reply') AND json_extract(data,'$.event.source_id') IN (SELECT value FROM json_each(?1)) ORDER BY created,id LIMIT ?2 OFFSET ?3")?;
        let rows = s.query_map(
            params![serde_json::to_string(sources)?, limit.min(1000), offset],
            |r| r.get::<_, String>(0),
        )?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }
    pub fn record_relay_receipt(&self, relay_id: &str, event_id: &str) -> Result<()> {
        if relay_id.is_empty() {
            bail!("relay identity is required");
        }
        self.conn()?.execute("INSERT INTO relay_receipts(relay_id,event_id) VALUES(?1,?2) ON CONFLICT(relay_id,event_id) DO NOTHING",params![relay_id,event_id])?;
        Ok(())
    }
    pub fn relay_received_from(&self, relay_id: &str, event_id: &str) -> Result<bool> {
        Ok(self.conn()?.query_row(
            "SELECT EXISTS(SELECT 1 FROM relay_receipts WHERE relay_id=?1 AND event_id=?2)",
            params![relay_id, event_id],
            |r| r.get(0),
        )?)
    }
    pub fn relay_completion_receipts(
        &self,
        relay_id: &str,
        sources: &[String],
        after_event_id: Option<&str>,
        limit: usize,
    ) -> Result<Vec<String>> {
        if sources.is_empty() || limit == 0 {
            return Ok(Vec::new());
        }
        let c = self.conn()?;
        let mut s=c.prepare("SELECT j.event_id FROM jobs j INNER JOIN relay_receipts r ON r.event_id=j.event_id WHERE r.relay_id=?1 AND j.state IN ('drafted','completed','no_reply') AND json_extract(j.data,'$.event.source_id') IN (SELECT value FROM json_each(?2)) AND (?3 IS NULL OR j.event_id>?3) ORDER BY j.event_id LIMIT ?4")?;
        let rows = s.query_map(
            params![
                relay_id,
                serde_json::to_string(sources)?,
                after_event_id,
                limit.min(1000)
            ],
            |r| r.get::<_, String>(0),
        )?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }
    pub fn relay_phase(&self, device: &str, event: &str) -> Result<Option<String>> {
        Ok(self
            .conn()?
            .query_row(
                "SELECT phase FROM relay_acks WHERE device=?1 AND event_id=?2",
                params![device, event],
                |r| r.get(0),
            )
            .optional()?)
    }
    pub fn relay_ack(&self, device: &str, event: &str, phase: &str) -> Result<()> {
        if phase != "received" && phase != "executed" {
            bail!("invalid relay ACK phase");
        }
        self.conn()?.execute("INSERT INTO relay_acks VALUES(?1,?2,?3) ON CONFLICT(device,event_id) DO UPDATE SET phase=CASE WHEN phase='executed' THEN phase ELSE excluded.phase END",params![device,event,phase])?;
        Ok(())
    }
}
pub fn target_key(target: &ThreadTarget) -> String {
    format!(
        "{}:{}:{}",
        target.host_id.len(),
        target.host_id,
        target.thread_id
    )
}

fn transaction_binding_for(
    tx: &rusqlite::Transaction<'_>,
    event: &NormalizedEvent,
) -> Result<Option<Binding>> {
    for key in event
        .conversation_key
        .iter()
        .map(String::as_str)
        .chain(std::iter::once(event.resource_key.as_str()))
    {
        if let Some((target, version, enabled, origin)) = tx
            .query_row(
                "SELECT target,version,enabled,origin FROM bindings WHERE source_id=?1 AND key=?2",
                params![event.source_id, key],
                |r| Ok((r.get::<_, String>(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
            )
            .optional()?
        {
            return Ok(Some(Binding {
                key: key.into(),
                source_id: event.source_id.clone(),
                target: serde_json::from_str(&target)?,
                version,
                enabled,
                origin,
            }));
        }
    }
    Ok(None)
}
fn wake_router_target(tx: &rusqlite::Transaction<'_>, target: &str) -> Result<()> {
    tx.execute("UPDATE events SET state='received',reason=NULL WHERE state='router_waiting' AND id IN (SELECT event_id FROM router_dispatches WHERE target_key=?1 AND state='waiting')",[target])?;
    Ok(())
}
fn register_ownership(
    tx: &rusqlite::Transaction<'_>,
    source: &str,
    key: &str,
    target: &ThreadTarget,
) -> Result<()> {
    let marker = format!("ownership:{source}:{key}");
    let card_id = format!("{}:{}", target.host_id, target.thread_id);
    let rows = {
        let mut s = tx.prepare("SELECT data FROM cards ORDER BY id")?;
        let rows = s
            .query_map([], |r| r.get::<_, String>(0))?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        rows
    };
    let mut destination = None;
    for data in rows {
        let mut card: ContextCard = serde_json::from_str(&data)?;
        if card.id == card_id {
            if card.target != *target {
                bail!("card_target_conflict: existing host/thread card has a different worktree");
            }
            destination = Some(card);
            continue;
        }
        if card.evidence.contains(&marker) {
            card.evidence.retain(|v| v != &marker);
            card.owned_resources.retain(|v| v != key);
            card.revision += 1;
            tx.execute(
                "UPDATE cards SET data=?2,revision=?3 WHERE id=?1",
                params![card.id, serde_json::to_string(&card)?, card.revision],
            )?;
        }
    }
    let mut card = destination.unwrap_or_else(|| ContextCard {
        id: card_id,
        target: target.clone(),
        source_ids: vec![],
        title: target.thread_id.clone(),
        summary: String::new(),
        owned_resources: vec![],
        referenced_resources: vec![],
        evidence: vec![],
        open_questions: vec![],
        revision: 0,
        coverage: "metadata_only:explicit_binding".into(),
        enabled: true,
    });
    let mut changed = card.revision == 0;
    if !card.source_ids.iter().any(|v| v == source) {
        card.source_ids.push(source.into());
        changed = true;
    }
    if !card.owned_resources.iter().any(|v| v == key) {
        card.owned_resources.push(key.into());
        changed = true;
    }
    if !card.evidence.contains(&marker) {
        card.evidence.push(marker);
        changed = true;
    }
    if changed {
        card.revision += 1;
        tx.execute("INSERT INTO cards(id,data,revision) VALUES(?1,?2,?3) ON CONFLICT(id) DO UPDATE SET data=excluded.data,revision=excluded.revision",params![card.id,serde_json::to_string(&card)?,card.revision])?;
    }
    Ok(())
}

/// This randomly named directory is created by this operation with mode 0700.
/// Cleanup removes only its own known SQLite filenames, never the caller's
/// destination or unrelated files in the destination directory.
struct BackupStaging {
    directory: PathBuf,
    path: PathBuf,
}
impl BackupStaging {
    fn create(parent: &Path) -> Result<(Self, std::fs::File)> {
        let directory = parent.join(format!(".right-wake-backup-{}", uuid::Uuid::new_v4()));
        let mut builder = std::fs::DirBuilder::new();
        #[cfg(unix)]
        {
            use std::os::unix::fs::DirBuilderExt;
            builder.mode(0o700);
        }
        builder.create(&directory)?;
        let staging = Self {
            path: directory.join("snapshot.sqlite3"),
            directory,
        };
        let mut options = std::fs::OpenOptions::new();
        options.read(true).write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let file = options.open(&staging.path)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            file.set_permissions(std::fs::Permissions::from_mode(0o600))?;
        }
        Ok((staging, file))
    }
}
impl Drop for BackupStaging {
    fn drop(&mut self) {
        for suffix in ["", "-journal", "-wal", "-shm"] {
            let _ = std::fs::remove_file(self.directory.join(format!("snapshot.sqlite3{suffix}")));
        }
        let _ = std::fs::remove_dir(&self.directory);
    }
}
