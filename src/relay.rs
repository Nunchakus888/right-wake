//! Source-scoped durable relay. A received ACK follows local disk persistence;
//! an executed ACK reports a separate, durably recorded successful model result.
use crate::{
    config::{Config, RelayDevice},
    engine::Engine,
    model::{digest, now_ms, NormalizedEvent},
    server::AppState,
};
use anyhow::{bail, Context, Result};
use axum::{
    extract::{
        ws::{Message, WebSocket, WebSocketUpgrade},
        Path, State,
    },
    http::{HeaderMap, StatusCode},
    response::IntoResponse,
};
use futures_util::{future, Sink, SinkExt, Stream, StreamExt};
use hmac::{Hmac, Mac};
use serde_json::{json, Value};
use sha2::Sha256;
use std::{
    collections::{HashMap, VecDeque},
    fmt::Display,
    pin::Pin,
    sync::{Arc, Mutex},
    time::Duration,
};
use tokio::{
    sync::{broadcast, mpsc, oneshot},
    task::JoinHandle,
    time::{timeout, Instant},
};
use tokio_tungstenite::{
    tungstenite::{client::IntoClientRequest, Message as ClientMessage},
    WebSocketStream,
};

pub const MAX_FRAME: usize = 2 * 1024 * 1024;
const SEND_DEADLINE: Duration = Duration::from_secs(10);
const ACK_DEADLINE: Duration = Duration::from_secs(30);

type SessionEntry = (String, oneshot::Sender<()>);
pub type SessionRegistry = Arc<Mutex<HashMap<String, SessionEntry>>>;
struct SessionGuard {
    registry: SessionRegistry,
    device: String,
    generation: String,
}
impl Drop for SessionGuard {
    fn drop(&mut self) {
        let mut registry = self.registry.lock().unwrap_or_else(|e| e.into_inner());
        if registry
            .get(&self.device)
            .is_some_and(|(id, _)| id == &self.generation)
        {
            registry.remove(&self.device);
        }
    }
}

fn same_secret(a: &str, b: &str) -> bool {
    let Ok(mut first) = Hmac::<Sha256>::new_from_slice(a.as_bytes()) else {
        return false;
    };
    first.update(b"right-wake-relay-token");
    let hash = first.finalize().into_bytes();
    let Ok(mut second) = Hmac::<Sha256>::new_from_slice(b.as_bytes()) else {
        return false;
    };
    second.update(b"right-wake-relay-token");
    second.verify_slice(&hash).is_ok()
}

fn authorize(state: &AppState, device: &str, token: &str) -> Result<RelayDevice> {
    let cfg = Config::load(&state.engine.dir)?;
    let device = cfg
        .relay_devices
        .iter()
        .find(|d| d.id == device && d.enabled)
        .context("relay device unavailable")?;
    let expected = std::env::var(&device.token_env).context("relay token unavailable")?;
    if expected.len() < 32 || !same_secret(&expected, token) {
        bail!("relay device authorization revoked");
    }
    Ok(device.clone())
}

/// Internal frame adapter, public only to allow transport-independent contract tests.
#[doc(hidden)]
#[derive(Debug, Clone)]
pub enum RelayFrame {
    Text(String),
    Ping(Vec<u8>),
    Pong(Vec<u8>),
    Close,
    Unsupported,
}

/// The bounded writer runs independently of reads. A slow peer therefore cannot
/// block receipt processing; its write deadline eventually closes the connection.
#[doc(hidden)]
pub struct RelayWire {
    outgoing: mpsc::Sender<RelayFrame>,
    incoming: Pin<Box<dyn Stream<Item = Result<RelayFrame>> + Send>>,
    failed: oneshot::Receiver<String>,
    writer: JoinHandle<()>,
}
impl Drop for RelayWire {
    fn drop(&mut self) {
        self.writer.abort();
    }
}
impl RelayWire {
    pub fn from_parts<W, R, E>(mut writer: W, reader: R) -> Self
    where
        W: Sink<RelayFrame, Error = E> + Unpin + Send + 'static,
        R: Stream<Item = std::result::Result<RelayFrame, E>> + Send + 'static,
        E: Display + Send + 'static,
    {
        let (outgoing, mut messages) = mpsc::channel(2);
        let (failure, failed) = oneshot::channel();
        let task = tokio::spawn(async move {
            while let Some(message) = messages.recv().await {
                let reason = match timeout(SEND_DEADLINE, writer.send(message)).await {
                    Ok(Ok(())) => continue,
                    Ok(Err(error)) => format!("relay write failed: {error}"),
                    Err(_) => "relay write deadline exceeded".into(),
                };
                let _ = failure.send(reason);
                return;
            }
        });
        Self {
            outgoing,
            incoming: Box::pin(
                reader.map(|value| value.map_err(|error| anyhow::anyhow!("{error}"))),
            ),
            failed,
            writer: task,
        }
    }

    pub fn websocket<S>(socket: WebSocketStream<S>) -> Self
    where
        S: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin + Send + 'static,
    {
        let (writer, reader) = socket.split();
        let writer = writer.with(|frame| {
            future::ready(Ok::<_, tokio_tungstenite::tungstenite::Error>(
                match frame {
                    RelayFrame::Text(text) => ClientMessage::Text(text.into()),
                    RelayFrame::Ping(bytes) => ClientMessage::Ping(bytes.into()),
                    RelayFrame::Pong(bytes) => ClientMessage::Pong(bytes.into()),
                    RelayFrame::Close => ClientMessage::Close(None),
                    RelayFrame::Unsupported => ClientMessage::Binary(Vec::new().into()),
                },
            ))
        });
        let reader = reader.map(|result| {
            result.map(|frame| match frame {
                ClientMessage::Text(text) => RelayFrame::Text(text.to_string()),
                ClientMessage::Ping(bytes) => RelayFrame::Ping(bytes.to_vec()),
                ClientMessage::Pong(bytes) => RelayFrame::Pong(bytes.to_vec()),
                ClientMessage::Close(_) => RelayFrame::Close,
                _ => RelayFrame::Unsupported,
            })
        });
        Self::from_parts(writer, reader)
    }

    fn axum(socket: WebSocket) -> Self {
        let (writer, reader) = socket.split();
        let writer = writer.with(|frame| {
            future::ready(Ok::<_, axum::Error>(match frame {
                RelayFrame::Text(text) => Message::Text(text.into()),
                RelayFrame::Ping(bytes) => Message::Ping(bytes.into()),
                RelayFrame::Pong(bytes) => Message::Pong(bytes.into()),
                RelayFrame::Close => Message::Close(None),
                RelayFrame::Unsupported => Message::Binary(Vec::new().into()),
            }))
        });
        let reader = reader.map(|result| {
            result.map(|frame| match frame {
                Message::Text(text) => RelayFrame::Text(text.to_string()),
                Message::Ping(bytes) => RelayFrame::Ping(bytes.to_vec()),
                Message::Pong(bytes) => RelayFrame::Pong(bytes.to_vec()),
                Message::Close(_) => RelayFrame::Close,
                _ => RelayFrame::Unsupported,
            })
        });
        Self::from_parts(writer, reader)
    }

    async fn queue(&mut self, frame: RelayFrame) -> Result<()> {
        if matches!(&frame, RelayFrame::Text(text) if text.len() > MAX_FRAME) {
            bail!("relay frame exceeds the two MiB transport limit");
        }
        timeout(SEND_DEADLINE, self.outgoing.send(frame))
            .await
            .context("relay write queue deadline exceeded")?
            .context("relay writer closed")
    }

    async fn queue_cancel(
        &mut self,
        frame: RelayFrame,
        cancelled: &mut oneshot::Receiver<()>,
    ) -> Result<()> {
        tokio::select! {
            biased;
            _ = cancelled => bail!("relay connection replaced"),
            result = self.queue(frame) => result,
        }
    }
}

pub async fn upgrade(
    State(state): State<AppState>,
    Path(device): Path<String>,
    headers: HeaderMap,
    ws: WebSocketUpgrade,
) -> impl IntoResponse {
    if !state.relay_only {
        return StatusCode::NOT_FOUND.into_response();
    }
    let presented = headers
        .get("authorization")
        .and_then(|h| h.to_str().ok())
        .and_then(|s| s.strip_prefix("Bearer "))
        .unwrap_or("")
        .to_owned();
    if authorize(&state, &device, &presented).is_err() {
        return StatusCode::UNAUTHORIZED.into_response();
    }
    let generation = uuid::Uuid::new_v4().to_string();
    let (cancel, receiver) = oneshot::channel();
    if let Some((_, previous)) = state
        .relay_sessions
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .insert(device.clone(), (generation.clone(), cancel))
    {
        let _ = previous.send(());
    }
    let guard = SessionGuard {
        registry: state.relay_sessions.clone(),
        device: device.clone(),
        generation,
    };
    ws.max_message_size(MAX_FRAME)
        .max_frame_size(MAX_FRAME)
        .on_upgrade(move |socket| async move {
            let _guard = guard;
            if let Err(error) =
                serve_wire(RelayWire::axum(socket), state, device, presented, receiver).await
            {
                tracing::debug!(error = %error, "relay device connection ended");
            }
        })
        .into_response()
}

#[doc(hidden)]
pub async fn serve_wire(
    mut wire: RelayWire,
    state: AppState,
    device: String,
    token: String,
    mut cancelled: oneshot::Receiver<()>,
) -> Result<()> {
    let mut in_flight: Option<(String, String, Instant)> = None;
    loop {
        // Register before querying: a commit between query and sleep is visible.
        let notified = state.relay_wake.notified();
        tokio::pin!(notified);
        notified.as_mut().enable();
        let current = authorize(&state, &device, &token)?;
        if in_flight
            .as_ref()
            .is_some_and(|(_, source, _)| !current.source_ids.contains(source))
        {
            bail!("in-flight source authorization revoked");
        }
        if in_flight.is_none() {
            if let Some(event) = state
                .engine
                .db
                .relay_pending(&device, &current.source_ids, 1)?
                .first()
            {
                wire.queue_cancel(
                    RelayFrame::Text(
                        json!({"type":"event","schema_version":1,"event":event}).to_string(),
                    ),
                    &mut cancelled,
                )
                .await?;
                in_flight = Some((
                    event.event_id.clone(),
                    event.source_id.clone(),
                    Instant::now() + ACK_DEADLINE,
                ));
            }
        }
        let delivery_deadline = in_flight.as_ref().map(|(_, _, at)| *at);
        tokio::select! {
            biased;
            _ = &mut cancelled => bail!("relay connection replaced"),
            failure = &mut wire.failed => bail!("{}", failure.unwrap_or_else(|_| "relay writer stopped".into())),
            _ = async { match delivery_deadline { Some(at) => tokio::time::sleep_until(at).await, None => future::pending().await } } => {
                bail!("relay delivery ACK deadline exceeded; unacknowledged event remains pending");
            }
            _ = &mut notified => {},
            frame = wire.incoming.next() => {
                let Some(frame) = frame else { return Ok(()) };
                // Re-read after the await, before processing any ACK. A config
                // notification and frame may have become ready simultaneously.
                let current = authorize(&state, &device, &token)?;
                match frame? {
                    RelayFrame::Text(text) => {
                        if text.len() > MAX_FRAME { bail!("relay frame too large"); }
                        let value: Value = serde_json::from_str(&text)?;
                        let event_id = value["event_id"].as_str().unwrap_or("");
                        let phase = value["phase"].as_str().unwrap_or("");
                        if value["type"] != "ack" || !matches!(phase, "received" | "executed") || event_id.is_empty() {
                            bail!("invalid relay ACK");
                        }
                        let event = match state.engine.db.event(event_id) {
                            Ok(event) => event,
                            Err(_) if phase == "executed" => {
                                wire.queue_cancel(rejected_ack(event_id, phase, "unknown_event"), &mut cancelled).await?;
                                continue;
                            }
                            Err(error) => return Err(error),
                        };
                        if !current.source_ids.contains(&event.source_id) { bail!("ACK crossed current source scope"); }
                        let existing = state.engine.db.relay_phase(&device, event_id)?;
                        let delivered_now = in_flight.as_ref().is_some_and(|(id, _, _)| id == event_id);
                        // A terminal receipt may recover a lost received ACK,
                        // but only for a delivery made on this connection or a
                        // durable prior receipt. It cannot skip an unseen event.
                        if !delivered_now && existing.is_none() {
                            wire.queue_cancel(rejected_ack(event_id, phase, "delivery_not_received"), &mut cancelled).await?;
                            continue;
                        }
                        state.engine.db.relay_ack(&device, event_id, phase)?;
                        if delivered_now { in_flight = None; }
                        wire.queue_cancel(RelayFrame::Text(json!({"type":"ack_confirmed","phase":phase,"event_id":event_id}).to_string()), &mut cancelled).await?;
                    }
                    RelayFrame::Ping(bytes) => wire.queue_cancel(RelayFrame::Pong(bytes), &mut cancelled).await?,
                    RelayFrame::Pong(_) => {},
                    RelayFrame::Close => return Ok(()),
                    RelayFrame::Unsupported => bail!("unexpected relay frame"),
                }
            }
        }
    }
}

fn rejected_ack(event: &str, phase: &str, reason: &str) -> RelayFrame {
    RelayFrame::Text(
        json!({"type":"ack_rejected","event_id":event,"phase":phase,"reason":reason}).to_string(),
    )
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct Ack {
    event_id: String,
    phase: &'static str,
}
impl Ack {
    fn frame(&self) -> RelayFrame {
        RelayFrame::Text(
            json!({"type":"ack","phase":self.phase,"event_id":self.event_id}).to_string(),
        )
    }
}

fn terminal(engine: &Engine, event_id: &str) -> Result<bool> {
    Ok(matches!(
        engine.db.event_detail(event_id)?["state"].as_str(),
        Some("drafted" | "completed" | "no_reply")
    ))
}

#[doc(hidden)]
pub async fn receive_wire(
    mut wire: RelayWire,
    engine: Engine,
    relay_id: String,
    source_ids: Vec<String>,
    mut completions: broadcast::Receiver<String>,
) -> Result<()> {
    let mut received = VecDeque::<Ack>::new();
    let mut pending: Option<(Ack, Instant)> = None;
    let mut cursor: Option<String> = None;
    let mut replay = true;
    let mut replay_again = false;
    loop {
        if pending.is_none() {
            let mut next = received.pop_front();
            while next.is_none() && replay {
                let values = engine.db.relay_completion_receipts(
                    &relay_id,
                    &source_ids,
                    cursor.as_deref(),
                    1,
                )?;
                if let Some(event_id) = values.into_iter().next() {
                    cursor = Some(event_id.clone());
                    next = Some(Ack {
                        event_id,
                        phase: "executed",
                    });
                } else if replay_again {
                    replay_again = false;
                    cursor = None;
                } else {
                    replay = false;
                }
            }
            if let Some(ack) = next {
                wire.queue(ack.frame()).await?;
                pending = Some((ack, Instant::now() + ACK_DEADLINE));
            }
        }
        // No model timer exists here. This deadline only bounds an outstanding
        // network ACK; the idle branch waits for an event indefinitely.
        let deadline = pending.as_ref().map(|(_, at)| *at);
        tokio::select! {
            failure = &mut wire.failed => bail!("{}", failure.unwrap_or_else(|_| "relay writer stopped".into())),
            _ = async { match deadline { Some(at) => tokio::time::sleep_until(at).await, None => future::pending().await } } => {
                bail!("relay ACK deadline exceeded; durable receipts will reconcile on reconnect");
            }
            completed = completions.recv() => {
                match completed {
                    Ok(event_id) => {
                        let event = engine.db.event(&event_id)?;
                        if source_ids.contains(&event.source_id) && engine.db.relay_received_from(&relay_id, &event_id)? {
                            if replay { replay_again = true; } else { cursor = None; replay = true; }
                        }
                    }
                    Err(broadcast::error::RecvError::Lagged(_)) => {
                        if replay { replay_again = true; } else { cursor = None; replay = true; }
                    }
                    Err(broadcast::error::RecvError::Closed) => bail!("worker completion channel closed"),
                }
            }
            frame = wire.incoming.next() => {
                let Some(frame) = frame else { bail!("relay disconnected"); };
                match frame? {
                    RelayFrame::Text(text) => {
                        if text.len() > MAX_FRAME { bail!("relay frame too large"); }
                        let value: Value = serde_json::from_str(&text)?;
                        if matches!(value["type"].as_str(), Some("ack_confirmed" | "ack_rejected")) {
                            if pending.as_ref().is_some_and(|(ack, _)| value["event_id"] == ack.event_id && value["phase"] == ack.phase) {
                                if value["type"] == "ack_rejected" && value["phase"] == "received" {
                                    bail!("relay rejected the durable received ACK; reconnect after reviewing source/device scope");
                                }
                                pending = None;
                            }
                            continue;
                        }
                        if value["type"] != "event" || value["schema_version"] != 1 { bail!("unsupported relay envelope"); }
                        let mut event: NormalizedEvent = serde_json::from_value(value["event"].clone())?;
                        if !source_ids.contains(&event.source_id) { bail!("relay event crossed configured source boundary"); }
                        let cfg = Config::load(&engine.dir)?;
                        let source = cfg.source(&event.source_id)?;
                        // Relay flags retain intrinsic/static denials. Current
                        // Linear ownership is separate local DB state; a cloud
                        // membership wait must not erase a valid request.
                        crate::provider::apply_local_policy(source, &mut event)?;
                        let raw = serde_json::to_vec(&value["event"])?;
                        event.received_at_ms = now_ms();
                        event.delivery_id = format!("relay:{}:{}", relay_id, digest(&raw));
                        engine.db.receive(&event, &raw, &cfg)?;
                        engine.db.record_relay_receipt(&relay_id, &event.event_id)?;
                        // Wake immediately after the durable receipt, independently
                        // of whether the network ACK can subsequently be delivered.
                        engine.wake.notify_one();
                        let ack = Ack { event_id: event.event_id.clone(), phase: "received" };
                        if !received.contains(&ack) && !pending.as_ref().is_some_and(|(sent, _)| sent == &ack) {
                            if received.len() >= 32 { bail!("relay peer exceeded the bounded delivery window"); }
                            received.push_back(ack);
                        }
                        if terminal(&engine, &event.event_id)? {
                            if replay { replay_again = true; } else { cursor = None; replay = true; }
                        }
                    }
                    RelayFrame::Ping(bytes) => wire.queue(RelayFrame::Pong(bytes)).await?,
                    RelayFrame::Pong(_) => {},
                    RelayFrame::Close => bail!("relay disconnected"),
                    RelayFrame::Unsupported => bail!("unexpected relay frame"),
                }
            }
        }
    }
}

pub async fn connect(
    engine: Engine,
    url: &str,
    token_env: &str,
    source_ids: &[String],
    allow_loopback: bool,
) -> Result<()> {
    if source_ids.is_empty() {
        bail!("relay connect needs at least one explicit source scope");
    }
    let parsed = url::Url::parse(url)?;
    if !parsed.username().is_empty() || parsed.password().is_some() || parsed.fragment().is_some() {
        bail!("relay credentials belong in the named environment variable, never the URL");
    }
    let loopback = parsed
        .host_str()
        .is_some_and(|s| matches!(s, "localhost" | "127.0.0.1" | "[::1]"));
    if parsed.scheme() != "wss" && !(parsed.scheme() == "ws" && allow_loopback && loopback) {
        bail!("relay requires wss; --allow-loopback permits only local test ws");
    }
    let token = std::env::var(token_env).context("relay token environment variable is missing")?;
    if token.len() < 32 {
        bail!("relay token needs at least 32 characters");
    }
    // The endpoint includes the device path. Persist only its digest, not URL
    // query values, credentials, or authorization tokens.
    let relay_id = digest(parsed.as_str().as_bytes());
    let mut request = parsed.as_str().into_client_request()?;
    request
        .headers_mut()
        .insert("Authorization", format!("Bearer {token}").parse()?);
    let config = tokio_tungstenite::tungstenite::protocol::WebSocketConfig::default()
        .max_message_size(Some(MAX_FRAME))
        .max_frame_size(Some(MAX_FRAME));
    let _lock = crate::cli::acquire_worker_lock(&engine.dir)?;
    let (socket, _) = timeout(
        SEND_DEADLINE,
        tokio_tungstenite::connect_async_with_config(request, Some(config), false),
    )
    .await
    .context("relay connection deadline exceeded")??;
    engine.db.recover()?;
    let completions = engine.completed.subscribe();
    let state = AppState {
        engine: engine.clone(),
        relay_only: false,
        relay_wake: Arc::new(tokio::sync::Notify::new()),
        relay_sessions: Default::default(),
    };
    let _watcher = crate::server::watch_controls(&state)?;
    struct AbortWorker(JoinHandle<Result<()>>);
    impl Drop for AbortWorker {
        fn drop(&mut self) {
            self.0.abort();
        }
    }
    let _worker = AbortWorker(tokio::spawn(engine.clone().run()));
    receive_wire(
        RelayWire::websocket(socket),
        engine,
        relay_id,
        source_ids.to_vec(),
        completions,
    )
    .await
    .context("relay transport stopped; reconnect to reconcile durable deliveries and receipts")
}
