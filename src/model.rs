use serde::{Deserialize, Serialize};
use serde_json::Value;

pub fn now_ms() -> i64 {
    chrono::Utc::now().timestamp_millis()
}
pub fn digest(bytes: &[u8]) -> String {
    use sha2::Digest;
    hex::encode(sha2::Sha256::digest(bytes))
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ProviderKind {
    Linear,
    Github,
    Gitlab,
    Generic,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ThreadTarget {
    pub host_id: String,
    pub thread_id: String,
    #[serde(default)]
    pub cwd: Option<String>,
}

/// Canonical event. No field may specify local commands, permissions, or thread ownership.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NormalizedEvent {
    pub event_id: String,
    pub delivery_id: String,
    pub source_id: String,
    pub provider: ProviderKind,
    pub tenant_id: String,
    pub resource_key: String,
    pub conversation_key: Option<String>,
    pub actor_id: String,
    pub action: String,
    pub occurred_at_ms: i64,
    pub received_at_ms: i64,
    pub source_url: Option<String>,
    pub content: String,
    pub revision: String,
    pub actionable: bool,
    #[serde(default)]
    pub explicit_trigger: bool,
    pub skip_reason: Option<String>,
    pub reply_target: Option<ReplyTarget>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ReplyTarget {
    pub provider: ProviderKind,
    pub container_id: String,
    pub resource_id: String,
    #[serde(default)]
    pub comment_id: Option<String>,
    #[serde(default)]
    pub discussion_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Binding {
    pub key: String,
    pub source_id: String,
    pub target: ThreadTarget,
    pub version: i64,
    pub enabled: bool,
    pub origin: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContextCard {
    pub id: String,
    pub target: ThreadTarget,
    pub source_ids: Vec<String>,
    pub title: String,
    pub summary: String,
    #[serde(default)]
    pub owned_resources: Vec<String>,
    #[serde(default)]
    pub referenced_resources: Vec<String>,
    #[serde(default)]
    pub evidence: Vec<String>,
    #[serde(default)]
    pub open_questions: Vec<String>,
    pub revision: i64,
    pub coverage: String,
    pub enabled: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Job {
    pub id: String,
    pub event: NormalizedEvent,
    pub target: ThreadTarget,
    pub binding_key: String,
    pub binding_version: i64,
    #[serde(default)]
    pub source_revision: String,
    pub state: String,
    pub dispatch_id: Option<String>,
    pub turn_id: Option<String>,
    pub reason: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Draft {
    pub id: String,
    pub job_id: String,
    pub source_id: String,
    pub resource_key: String,
    pub event_id: String,
    pub body: String,
    pub reply_target: ReplyTarget,
    pub source_revision: String,
    #[serde(default)]
    pub source_event_revision: String,
    pub binding_key: String,
    pub binding_version: i64,
    pub revision: i64,
    pub sending_identity: String,
    pub state: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApiError {
    pub code: String,
    pub message: String,
    pub next_action: String,
    pub retryable: bool,
}

pub fn ok(data: impl Serialize) -> Value {
    serde_json::json!({"schema_version": 1, "ok": true, "data": data})
}
