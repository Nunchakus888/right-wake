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
    future::Future,
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
const HEARTBEAT_INTERVAL: Duration = Duration::from_secs(20);
const HEARTBEAT_DEADLINE: Duration = Duration::from_secs(10);
const STABLE_CONNECTION: Duration = Duration::from_secs(60);

/// Only transport failures may reconnect automatically. Protocol, authorization,
/// configuration and storage errors keep their original fail-closed boundary.
/// Messages here are static so network responses cannot leak bearer credentials.
#[derive(Debug, thiserror::Error)]
#[error("{0}")]
struct RetryableTransport(&'static str);

fn retryable(reason: &'static str) -> anyhow::Error {
    RetryableTransport(reason).into()
}

/// Safe, actionable handshake diagnostics may reach the CLI unchanged. Other
/// fatal errors retain a generic outer message because serde/peer errors can
/// contain values from an untrusted envelope.
#[derive(Debug, thiserror::Error)]
#[error("{0}")]
struct PermanentRelayError(&'static str);

fn permanent(reason: &'static str) -> anyhow::Error {
    PermanentRelayError(reason).into()
}

fn websocket_error(error: tokio_tungstenite::tungstenite::Error) -> anyhow::Error {
    use tokio_tungstenite::tungstenite::Error;
    match error {
        // tokio-tungstenite's rustls adapter wraps handshake/certificate errors
        // as Io, not Tls. Retrying those as ordinary outages hides a deployment
        // error indefinitely; never echo the inner TLS or peer-provided detail.
        Error::Io(error)
            if matches!(
                error.kind(),
                std::io::ErrorKind::InvalidData | std::io::ErrorKind::InvalidInput
            ) =>
        {
            permanent(
                "relay_tls_or_protocol_rejected: inspect the HTTPS/WSS endpoint, certificate and protocol"
            )
        }
        Error::Io(_)
        | Error::ConnectionClosed
        | Error::AlreadyClosed
        | Error::Protocol(
            tokio_tungstenite::tungstenite::error::ProtocolError::ResetWithoutClosingHandshake,
        ) => retryable("relay network connection unavailable"),
        Error::Http(response)
            if response.status().is_server_error()
                || matches!(response.status().as_u16(), 408 | 429) =>
        {
            retryable("relay endpoint temporarily unavailable")
        }
        Error::Http(response) if matches!(response.status().as_u16(), 401 | 403) => {
            permanent("relay_authorization_rejected: inspect device token and source scope")
        }
        Error::Tls(_) => permanent(
            "relay_tls_rejected: inspect the trusted HTTPS/WSS endpoint and certificate",
        ),
        Error::Http(_) => {
            permanent("relay_endpoint_rejected: inspect the fixed URL and WebSocket route")
        }
        _ => anyhow::anyhow!(
            "relay_protocol_rejected: inspect relay compatibility before restarting"
        ),
    }
}

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
    failed: oneshot::Receiver<anyhow::Error>,
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
        E: Into<anyhow::Error> + Send + 'static,
    {
        let (outgoing, mut messages) = mpsc::channel(2);
        let (failure, failed) = oneshot::channel();
        let task = tokio::spawn(async move {
            while let Some(message) = messages.recv().await {
                let reason = match timeout(SEND_DEADLINE, writer.send(message)).await {
                    Ok(Ok(())) => continue,
                    Ok(Err(error)) => error.into(),
                    Err(_) => retryable("relay write deadline exceeded"),
                };
                let _ = failure.send(reason);
                return;
            }
        });
        Self {
            outgoing,
            incoming: Box::pin(reader.map(|value| value.map_err(Into::into))),
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
            result.map_err(websocket_error).map(|frame| match frame {
                ClientMessage::Text(text) => RelayFrame::Text(text.to_string()),
                ClientMessage::Ping(bytes) => RelayFrame::Ping(bytes.to_vec()),
                ClientMessage::Pong(bytes) => RelayFrame::Pong(bytes.to_vec()),
                ClientMessage::Close(_) => RelayFrame::Close,
                _ => RelayFrame::Unsupported,
            })
        });
        Self::from_parts(writer.sink_map_err(websocket_error), reader)
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
            result
                .map_err(|_| retryable("relay network connection unavailable"))
                .map(|frame| match frame {
                    Message::Text(text) => RelayFrame::Text(text.to_string()),
                    Message::Ping(bytes) => RelayFrame::Ping(bytes.to_vec()),
                    Message::Pong(bytes) => RelayFrame::Pong(bytes.to_vec()),
                    Message::Close(_) => RelayFrame::Close,
                    _ => RelayFrame::Unsupported,
                })
        });
        Self::from_parts(
            writer.sink_map_err(|_| retryable("relay network connection unavailable")),
            reader,
        )
    }

    async fn queue(&mut self, frame: RelayFrame) -> Result<()> {
        if matches!(&frame, RelayFrame::Text(text) if text.len() > MAX_FRAME) {
            bail!("relay frame exceeds the two MiB transport limit");
        }
        timeout(SEND_DEADLINE, self.outgoing.send(frame))
            .await
            .map_err(|_| retryable("relay write queue deadline exceeded"))?
            .map_err(|_| retryable("relay writer closed"))
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
            failure = &mut wire.failed => return Err(failure.unwrap_or_else(|_| retryable("relay writer stopped"))),
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
    let mut heartbeat_at = Instant::now() + HEARTBEAT_INTERVAL;
    let mut heartbeat: Option<Vec<u8>> = None;
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
        // These timers only supervise the network. They never poll a provider,
        // enqueue work or invoke a model when the event inbox is idle.
        let deadline = pending.as_ref().map(|(_, at)| *at);
        tokio::select! {
            failure = &mut wire.failed => return Err(failure.unwrap_or_else(|_| retryable("relay writer stopped"))),
            _ = async { match deadline { Some(at) => tokio::time::sleep_until(at).await, None => future::pending().await } } => {
                return Err(retryable("relay ACK deadline exceeded; durable receipts will reconcile on reconnect"));
            }
            _ = tokio::time::sleep_until(heartbeat_at) => {
                if heartbeat.is_some() {
                    return Err(retryable("relay heartbeat deadline exceeded"));
                }
                // A matching nonce proves this connection is still bidirectional,
                // including when a network change silently drops an idle socket.
                let nonce = uuid::Uuid::new_v4().as_bytes().to_vec();
                wire.queue(RelayFrame::Ping(nonce.clone())).await?;
                heartbeat = Some(nonce);
                heartbeat_at = Instant::now() + HEARTBEAT_DEADLINE;
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
                let Some(frame) = frame else { return Err(retryable("relay disconnected")); };
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
                    RelayFrame::Pong(bytes) => {
                        if heartbeat.as_ref() == Some(&bytes) {
                            heartbeat = None;
                            heartbeat_at = Instant::now() + HEARTBEAT_INTERVAL;
                        }
                    }
                    RelayFrame::Close => return Err(retryable("relay disconnected")),
                    RelayFrame::Unsupported => bail!("unexpected relay frame"),
                }
            }
        }
    }
}

/// Validate the stable device endpoint without contacting it or reading secrets.
/// Saved production connections call this with `allow_loopback = false`.
pub fn validate_url(url: &str, allow_loopback: bool) -> Result<url::Url> {
    let parsed = url::Url::parse(url)?;
    if !parsed.username().is_empty()
        || parsed.password().is_some()
        || parsed.query().is_some()
        || parsed.fragment().is_some()
    {
        bail!("relay URL must not contain credentials, query parameters or fragments");
    }
    let loopback = match parsed.host() {
        Some(url::Host::Ipv4(address)) => address.is_loopback(),
        Some(url::Host::Ipv6(address)) => address.is_loopback(),
        Some(url::Host::Domain(name)) => name == "localhost",
        None => false,
    };
    if parsed.scheme() != "wss" && !(parsed.scheme() == "ws" && allow_loopback && loopback) {
        bail!("relay requires wss; --allow-loopback permits only local test ws");
    }
    let device = parsed
        .path()
        .strip_prefix("/relay/")
        .context("relay URL must select /relay/<device_id>")?;
    crate::config::validate_id(device)
        .context("relay URL must select one configured device, without extra path segments")?;
    Ok(parsed)
}

fn connection_request(
    parsed: &url::Url,
    token_env: &str,
) -> Result<tokio_tungstenite::tungstenite::http::Request<()>> {
    crate::config::validate_env(token_env)?;
    let token = std::env::var(token_env).context("relay token environment variable is missing")?;
    if token.len() < 32 {
        bail!("relay token needs at least 32 characters");
    }
    let mut request = parsed.as_str().into_client_request()?;
    request
        .headers_mut()
        .insert("Authorization", format!("Bearer {token}").parse()?);
    Ok(request)
}

async fn open_wire(parsed: &url::Url, token_env: &str) -> Result<RelayWire> {
    let request = connection_request(parsed, token_env)?;
    let config = tokio_tungstenite::tungstenite::protocol::WebSocketConfig::default()
        .max_message_size(Some(MAX_FRAME))
        .max_frame_size(Some(MAX_FRAME));
    let (socket, _) = timeout(
        SEND_DEADLINE,
        tokio_tungstenite::connect_async_with_config(request, Some(config), false),
    )
    .await
    .map_err(|_| retryable("relay connection deadline exceeded"))?
    .map_err(websocket_error)?;
    Ok(RelayWire::websocket(socket))
}

fn validate_sources(engine: &Engine, source_ids: &[String]) -> Result<()> {
    if source_ids.is_empty() {
        bail!("relay connect needs at least one explicit source scope");
    }
    let cfg = Config::load(&engine.dir)?;
    for source in source_ids {
        cfg.source(source)?;
    }
    Ok(())
}

fn reconnect_delay(failures: u32) -> Duration {
    // 75–100% jitter keeps repeated clients apart, even at the 30-second cap.
    let ceiling_ms = (1000_u64 << failures.min(5)).min(30_000);
    let floor_ms = ceiling_ms * 3 / 4;
    Duration::from_millis(floor_ms + rand::random::<u64>() % (ceiling_ms - floor_ms + 1))
}

async fn reconnect<F, Connection>(
    engine: &Engine,
    relay_id: &str,
    source_ids: &[String],
    mut open: F,
) -> Result<()>
where
    F: FnMut() -> Connection,
    Connection: Future<Output = Result<RelayWire>>,
{
    let mut failures = 0;
    loop {
        validate_sources(engine, source_ids)?;
        let started = Instant::now();
        let outcome = match open().await {
            Ok(wire) => {
                receive_wire(
                    wire,
                    engine.clone(),
                    relay_id.to_owned(),
                    source_ids.to_vec(),
                    engine.completed.subscribe(),
                )
                .await
            }
            Err(error) => Err(error),
        };
        let error = outcome
            .err()
            .unwrap_or_else(|| retryable("relay disconnected"));
        if !error.is::<RetryableTransport>() {
            if error.is::<PermanentRelayError>() {
                return Err(error);
            }
            return Err(error.context(
                "relay stopped; review device authorization, configuration, protocol and storage before restarting",
            ));
        }
        if started.elapsed() >= STABLE_CONNECTION {
            failures = 0;
        }
        let delay = reconnect_delay(failures);
        failures = failures.saturating_add(1);
        tracing::warn!(retry_in_ms = delay.as_millis(), reason = %error, "relay transport will reconnect; durable events and running local jobs are retained");
        tokio::time::sleep(delay).await;
    }
}

async fn shutdown_signal() -> Result<()> {
    #[cfg(unix)]
    {
        let mut terminate =
            tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
                .context("could not register relay shutdown handler")?;
        tokio::select! {
            signal = tokio::signal::ctrl_c() => signal.context("relay shutdown handler failed"),
            _ = terminate.recv() => Ok(()),
        }
    }
    #[cfg(not(unix))]
    {
        tokio::signal::ctrl_c()
            .await
            .context("relay shutdown handler failed")
    }
}

pub async fn connect(
    engine: Engine,
    url: &str,
    token_env: &str,
    source_ids: &[String],
    allow_loopback: bool,
) -> Result<()> {
    let parsed = validate_url(url, allow_loopback)?;
    validate_sources(&engine, source_ids)?;
    // Fail configuration errors before starting the worker. The request itself
    // is rebuilt for each connection; its credential is never persisted/logged.
    connection_request(&parsed, token_env)?;
    let relay_id = digest(parsed.as_str().as_bytes());
    let _lock = crate::cli::acquire_worker_lock(&engine.dir)?;
    // Recover exactly once per process, never on a transport reconnect: model
    // dispatch can continue locally while the cloud connection is unavailable.
    engine.db.recover()?;
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
    let mut worker = AbortWorker(tokio::spawn(engine.clone().run()));
    let result = tokio::select! {
        outcome = reconnect(&engine, &relay_id, source_ids, || open_wire(&parsed, token_env)) => outcome,
        outcome = shutdown_signal() => outcome,
        _ = &mut worker.0 => anyhow::bail!("local relay worker stopped; inspect jobs before restarting"),
    };
    // Wait for cancellation before releasing the profile's exclusive worker lock.
    // Any uncertain dispatch remains durable for normal startup reconciliation.
    worker.0.abort();
    let _ = (&mut worker.0).await;
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{db::Db, model::ProviderKind};
    use tokio_tungstenite::tungstenite::{http::Response, protocol::Role, Error};

    fn fixture() -> (tempfile::TempDir, Engine) {
        let dir = tempfile::tempdir().unwrap();
        let cfg = Config {
            sources: vec![serde_json::from_value(json!({
                "id":"source","kind":"linear","tenant_id":"tenant","secret_env":"UNUSED_TEST_SECRET"
            }))
            .unwrap()],
            ..Config::default()
        };
        cfg.save(dir.path()).unwrap();
        let engine = Engine::new(dir.path().to_owned(), Db::open(dir.path()).unwrap());
        (dir, engine)
    }

    #[test]
    fn device_endpoints_and_retryable_failures_keep_credentials_out_of_errors() {
        for endpoint in [
            "wss://relay.example.test/relay/mac",
            "wss://relay.example.test:8443/relay/mac",
        ] {
            assert!(validate_url(endpoint, false).is_ok());
        }
        for endpoint in [
            "ws://relay.example.test/relay/mac",
            "ws://127.0.0.1:7433/relay/mac",
            "wss://token@relay.example.test/relay/mac",
            "wss://relay.example.test/relay/mac?token=secret",
            "wss://relay.example.test/relay/mac#secret",
            "wss://relay.example.test/",
            "wss://relay.example.test/relay/mac/another",
            "wss://relay.example.test/relay/",
        ] {
            assert!(validate_url(endpoint, false).is_err(), "{endpoint}");
        }
        assert!(validate_url("ws://127.0.0.1:7433/relay/mac", true).is_ok());
        assert!(validate_url("ws://[::1]:7433/relay/mac", true).is_ok());
        assert!(validate_url("ws://relay.example.test/relay/mac", true).is_err());
        for status in [401, 403, 404, 502, 429] {
            let response = Response::builder()
                .status(status)
                .header("X-Debug-Credential", "never-log-this")
                .body(Some(b"never-log-this".to_vec()))
                .unwrap();
            let error = websocket_error(Error::Http(Box::new(response)));
            assert!(!format!("{error:?}").contains("never-log-this"));
            assert_eq!(
                error.is::<RetryableTransport>(),
                matches!(status, 502 | 429)
            );
        }
        assert!(websocket_error(Error::Protocol(
            tokio_tungstenite::tungstenite::error::ProtocolError::ResetWithoutClosingHandshake,
        ))
        .is::<RetryableTransport>());
        for kind in [
            std::io::ErrorKind::InvalidData,
            std::io::ErrorKind::InvalidInput,
        ] {
            let error = websocket_error(Error::Io(std::io::Error::new(
                kind,
                "certificate rejected: never-log-this",
            )));
            assert!(!error.is::<RetryableTransport>());
            assert!(error.to_string().contains("relay_tls_or_protocol_rejected"));
            assert!(!format!("{error:?}").contains("never-log-this"));
        }
        let reset = websocket_error(Error::Io(std::io::Error::new(
            std::io::ErrorKind::ConnectionReset,
            "network reset: never-log-this",
        )));
        assert!(reset.is::<RetryableTransport>());
        assert!(!format!("{reset:?}").contains("never-log-this"));
        for failures in 0..20 {
            let ceiling = (1000_u64 << failures.min(5)).min(30_000);
            let delay = reconnect_delay(failures);
            assert!(delay >= Duration::from_millis(ceiling * 3 / 4));
            assert!(delay <= Duration::from_millis(ceiling));
        }
    }

    #[tokio::test(start_paused = true)]
    async fn offline_start_and_disconnect_reconnect_with_backoff_but_authorization_errors_stop() {
        let (_dir, engine) = fixture();
        let (server, client) = tokio::io::duplex(64);
        let mut server = WebSocketStream::from_raw_socket(server, Role::Server, None).await;
        let client = WebSocketStream::from_raw_socket(client, Role::Client, None).await;
        let attempts = Arc::new(Mutex::new(Vec::new()));
        let observed = attempts.clone();
        let local = engine.clone();
        let mut connections = VecDeque::from([
            Err(retryable("relay network connection unavailable")),
            Err(retryable("relay connection deadline exceeded")),
            Ok(RelayWire::websocket(client)),
            Err(anyhow::anyhow!(
                "relay_authorization_rejected: test fixture"
            )),
        ]);
        let task = tokio::spawn(async move {
            reconnect(&local, "test-relay", &["source".into()], move || {
                observed.lock().unwrap().push(Instant::now());
                future::ready(
                    connections
                        .pop_front()
                        .expect("fatal errors must not retry"),
                )
            })
            .await
        });
        let event = NormalizedEvent {
            event_id: "offline-event".into(),
            delivery_id: "original-delivery".into(),
            source_id: "source".into(),
            provider: ProviderKind::Linear,
            tenant_id: "tenant".into(),
            resource_key: "linear:https://linear.app:tenant:issue".into(),
            conversation_key: None,
            actor_id: "person".into(),
            action: "comment.create".into(),
            occurred_at_ms: 1,
            received_at_ms: 1,
            source_url: None,
            content: "@right-wake continue".into(),
            revision: "original-revision".into(),
            actionable: true,
            explicit_trigger: true,
            skip_reason: None,
            reply_target: None,
            linear: None,
        };
        server
            .send(ClientMessage::Text(
                json!({"type":"event","schema_version":1,"event":event})
                    .to_string()
                    .into(),
            ))
            .await
            .unwrap();
        let ClientMessage::Text(ack) = server.next().await.unwrap().unwrap() else {
            panic!("expected durable received ACK");
        };
        assert_eq!(
            serde_json::from_str::<Value>(&ack).unwrap()["phase"],
            "received"
        );
        assert!(engine
            .db
            .relay_received_from("test-relay", "offline-event")
            .unwrap());
        server.send(ClientMessage::Close(None)).await.unwrap();
        let error = task.await.unwrap().unwrap_err();
        assert!(format!("{error:#}").contains("relay_authorization_rejected"));
        let attempts = attempts.lock().unwrap();
        assert_eq!(attempts.len(), 4);
        for (index, pair) in attempts.windows(2).enumerate() {
            let delay = pair[1] - pair[0];
            let ceiling = 1000_u64 << index;
            assert!(delay >= Duration::from_millis(ceiling * 3 / 4));
            assert!(delay <= Duration::from_millis(ceiling));
        }
        assert!(engine.db.jobs(None, 10).unwrap().is_empty());
        assert_eq!(engine.db.events(None, 10).unwrap().len(), 1);
    }

    #[tokio::test(start_paused = true)]
    async fn a_silent_idle_peer_is_detected_without_creating_model_work() {
        let (_dir, engine) = fixture();
        let (client, _silent_peer) = tokio::io::duplex(128);
        let client = WebSocketStream::from_raw_socket(client, Role::Client, None).await;
        let started = Instant::now();
        let error = receive_wire(
            RelayWire::websocket(client),
            engine.clone(),
            "test-relay".into(),
            vec!["source".into()],
            engine.completed.subscribe(),
        )
        .await
        .unwrap_err();
        assert!(error.is::<RetryableTransport>());
        assert!(error.to_string().contains("heartbeat deadline"));
        assert_eq!(started.elapsed(), HEARTBEAT_INTERVAL + HEARTBEAT_DEADLINE);
        assert!(engine.db.jobs(None, 10).unwrap().is_empty());
        assert!(engine.db.events(None, 10).unwrap().is_empty());
    }

    #[tokio::test]
    async fn an_unsupported_envelope_stops_without_retry_or_receipt() {
        let (_dir, engine) = fixture();
        let (server, client) = tokio::io::duplex(1024);
        let mut server = WebSocketStream::from_raw_socket(server, Role::Server, None).await;
        let client = WebSocketStream::from_raw_socket(client, Role::Client, None).await;
        let local = engine.clone();
        let mut connection = Some(RelayWire::websocket(client));
        let task = tokio::spawn(async move {
            reconnect(&local, "test-relay", &["source".into()], move || {
                future::ready(Ok(connection
                    .take()
                    .expect("protocol errors must not retry")))
            })
            .await
        });
        server
            .send(ClientMessage::Text(
                json!({"type":"event","schema_version":999,"event":{}})
                    .to_string()
                    .into(),
            ))
            .await
            .unwrap();
        let error = timeout(Duration::from_secs(2), task)
            .await
            .unwrap()
            .unwrap()
            .unwrap_err();
        assert!(!error.is::<RetryableTransport>());
        assert!(format!("{error:#}").contains("unsupported relay envelope"));
        assert!(engine.db.events(None, 10).unwrap().is_empty());
    }

    #[tokio::test]
    async fn fatal_handshake_categories_survive_the_outer_cli_error_without_secret_details() {
        let (_dir, engine) = fixture();
        let response = Response::builder()
            .status(403)
            .header("X-Debug-Credential", "never-log-this")
            .body(Some(b"never-log-this".to_vec()))
            .unwrap();
        let failures = [
            (
                "relay_authorization_rejected",
                Error::Http(Box::new(response)),
            ),
            (
                "relay_tls_or_protocol_rejected",
                Error::Io(std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    "certificate rejected: never-log-this",
                )),
            ),
        ];
        for (category, failure) in failures {
            let mut failure = Some(websocket_error(failure));
            let error = reconnect(&engine, "test-relay", &["source".into()], || {
                future::ready(Err(failure.take().expect("fatal handshake must not retry")))
            })
            .await
            .unwrap_err();
            // main.rs emits error.to_string(), not the complete anyhow chain.
            let message = error.to_string();
            assert!(message.starts_with(category), "{message}");
            assert!(!message.contains("never-log-this"));
            assert!(error.is::<PermanentRelayError>());
        }
    }
}
