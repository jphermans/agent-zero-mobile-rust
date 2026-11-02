//! Realtime client lifecycle (reference `Sources/A0Realtime/Realtime.swift`):
//! WebSocket-only engine.io v4 transport, namespace `/ws`, no auto-reconnect.
//!
//! Mirrored semantics: overall connect timeout (10 s default) covering engine
//! open + namespace connect; ack-based `state_request` with its own timeout
//! (10 s default) and stage `"payload-or-handshake"` on any ack/decode
//! failure; a single queued request replacing any earlier queued one; a
//! bounded pre-ack push buffer (32) whose overflow emits `Failed` while the
//! connection stays open (reference guard `count < 32`); handshake emission
//! followed by an in-order buffer drain; engine-level pong replies to server
//! pings; and a silence watchdog at `pingInterval + pingTimeout + grace`.

use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tokio::time::Instant;

use a0_core::protocol::{AckData, HandshakeAck, StatePush, StatePushData, StateRequest};
use futures_util::{SinkExt, StreamExt};
use serde_json::{json, Value as JSONValue};
use tokio::net::TcpStream;
use tokio::sync::mpsc::{self, UnboundedReceiver, UnboundedSender};
use tokio::task::JoinHandle;
use tokio_tungstenite::tungstenite::client::IntoClientRequest;
use tokio_tungstenite::tungstenite::http::HeaderValue;
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::{connect_async, MaybeTlsStream, WebSocketStream};
use url::Url;

use crate::codec::{self, EngineFrame, OpenPacket, SocketPacket};
use crate::error::{RealtimeError, Result};

/// Socket.IO namespace used by Agent Zero (reference `Realtime.swift:37`).
const NAMESPACE: &str = "/ws";

type WsStream = WebSocketStream<MaybeTlsStream<TcpStream>>;
type WsWriter = futures_util::stream::SplitSink<WsStream, Message>;
type WsReader = futures_util::stream::SplitStream<WsStream>;

/// Tunables; defaults mirror the reference (`timeoutAfter: 10`,
/// `timingOut(after: 10)`, 32-push buffer).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RealtimeConfig {
    /// Overall timeout for engine open + namespace connect.
    pub connect_timeout: Duration,
    /// `state_request` ack timeout.
    pub ack_timeout: Duration,
    /// Pre-ack push buffer capacity (reference guard `count < 32`).
    pub push_buffer_capacity: usize,
    /// Extra inbound-silence allowance beyond `pingInterval + pingTimeout`.
    pub ping_grace: Duration,
}

impl Default for RealtimeConfig {
    fn default() -> Self {
        Self {
            connect_timeout: Duration::from_secs(10),
            ack_timeout: Duration::from_secs(10),
            push_buffer_capacity: 32,
            ping_grace: Duration::from_secs(2),
        }
    }
}

/// Security context for one realtime connection. Built by the coordinator
/// from an authenticated session (a0-auth `Session::cookie_header`).
#[derive(Clone)]
pub struct ConnectContext {
    /// Base origin, e.g. `https://a0.example.com:8443`.
    pub origin: String,
    /// Full `Cookie` header value including the synthetic csrf cookie.
    pub cookie_header: String,
    /// CSRF token for the namespace-connect auth payload.
    pub csrf_token: String,
}

impl std::fmt::Debug for ConnectContext {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // Never render credential material (a0-auth redaction policy).
        f.debug_struct("ConnectContext")
            .field("origin", &self.origin)
            .field("cookie_header", &"«redacted»")
            .field("csrf_token", &"«redacted»")
            .finish()
    }
}

/// Events delivered to the consumer; stages mirror the reference strings.
#[derive(Debug)]
pub enum RealtimeEvent {
    /// Handshake accepted (`RealtimeEvent.handshake`).
    Handshake { epoch: String, seq_base: u64 },
    /// `state_push` payload, buffered or live.
    Push(Box<StatePushData>),
    /// Stage failure; the connection may stay open (ack/decode failures).
    Failed { stage: String },
    /// Transport ended; `None` on clean local disconnect.
    Closed(Option<RealtimeError>),
}

enum Command {
    RequestState(StateRequest),
    Disconnect,
}

/// In-flight `state_request` awaiting its ack.
struct PendingAck {
    id: u64,
    correlation: String,
    deadline: Instant,
}

/// Control handle for a running realtime client.
#[derive(Debug)]
pub struct RealtimeHandle {
    command_tx: UnboundedSender<Command>,
    task: JoinHandle<()>,
}

impl RealtimeHandle {
    /// Emits `state_request`; while a handshake is in flight the request is
    /// queued (replacing any queued one) and re-issued after the ack —
    /// reference `requestState`/`queuedRequest`.
    pub fn request_state(&self, request: StateRequest) {
        let _ = self.command_tx.send(Command::RequestState(request));
    }

    /// Clean disconnect: engine close frame, state teardown, `Closed(None)`.
    pub fn disconnect(&self) {
        let _ = self.command_tx.send(Command::Disconnect);
    }

    /// Disconnects and waits for the task to finish.
    pub async fn shutdown(self) {
        self.disconnect();
        let _ = self.task.await;
    }

    /// Whether the client task is still running.
    pub fn is_running(&self) -> bool {
        !self.task.is_finished()
    }
}

/// Connects and spawns the client task. Connection failures arrive as
/// `Failed`/`Closed` events (the reference reports them the same way).
pub async fn connect(
    config: RealtimeConfig,
    context: ConnectContext,
    initial_request: StateRequest,
) -> Result<(RealtimeHandle, UnboundedReceiver<RealtimeEvent>)> {
    let (command_tx, command_rx) = mpsc::unbounded_channel();
    let (event_tx, event_rx) = mpsc::unbounded_channel();
    let task = tokio::spawn(run_loop(
        config,
        context,
        initial_request,
        command_rx,
        event_tx,
    ));
    Ok((RealtimeHandle { command_tx, task }, event_rx))
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

fn build_ws_url(origin: &str) -> Result<Url> {
    let mut url = Url::parse(origin)?;
    let ws_scheme = match url.scheme() {
        "https" => "wss",
        "http" => "ws",
        other => {
            return Err(RealtimeError::Connect(format!(
                "unsupported origin scheme {other:?}"
            )))
        }
    };
    url.set_scheme(ws_scheme)
        .map_err(|_| RealtimeError::Connect("scheme replacement failed".into()))?;
    url.set_path("/socket.io/");
    url.set_query(Some("EIO=4&transport=websocket"));
    Ok(url)
}

async fn establish(context: &ConnectContext) -> Result<(WsWriter, WsReader, OpenPacket)> {
    let url = build_ws_url(&context.origin)?;
    let mut request = url
        .as_str()
        .into_client_request()
        .map_err(|e| RealtimeError::Connect(e.to_string()))?;
    let headers = request.headers_mut();
    headers.insert(
        "Origin",
        HeaderValue::from_str(&context.origin)
            .map_err(|e| RealtimeError::Connect(format!("origin header: {e}")))?,
    );
    headers.insert(
        "Cookie",
        HeaderValue::from_str(&context.cookie_header)
            .map_err(|e| RealtimeError::Connect(format!("cookie header: {e}")))?,
    );

    let (ws, _response) = connect_async(request)
        .await
        .map_err(|e| RealtimeError::Connect(e.to_string()))?;
    let (mut write, mut read) = ws.split();

    // Engine open: first meaningful text frame must be `0{...}` (server pings
    // may arrive first under EIO=4; answer them while waiting).
    let open = loop {
        match read.next().await {
            Some(Ok(Message::Text(text))) => match codec::parse_frame(&text) {
                Ok(EngineFrame::Open(open)) => break open,
                Ok(EngineFrame::Ping) => {
                    let _ = write.send(Message::Text(codec::encode_pong())).await;
                }
                _ => {
                    return Err(RealtimeError::EngineOpen(
                        "expected engine open frame".into(),
                    ))
                }
            },
            Some(Ok(_)) => {
                return Err(RealtimeError::EngineOpen(
                    "non-text frame during engine open".into(),
                ))
            }
            Some(Err(e)) => return Err(RealtimeError::Connect(e.to_string())),
            None => {
                return Err(RealtimeError::TransportClosed(
                    "closed during engine open".into(),
                ))
            }
        }
    };

    // Namespace connect with the auth payload object: `40/ws,{...}`.
    let auth = json!({"csrf_token": context.csrf_token, "handlers": ["ws_webui"]});
    write
        .send(Message::Text(codec::encode_connect(NAMESPACE, &auth)))
        .await
        .map_err(|e| RealtimeError::Connect(e.to_string()))?;

    loop {
        match read.next().await {
            Some(Ok(Message::Text(text))) => match codec::parse_frame(&text) {
                Ok(EngineFrame::Ping) => {
                    write
                        .send(Message::Text(codec::encode_pong()))
                        .await
                        .map_err(|e| RealtimeError::Connect(e.to_string()))?;
                }
                Ok(EngineFrame::Message(SocketPacket::Connect { nsp, .. })) if nsp == NAMESPACE => {
                    return Ok((write, read, open));
                }
                Ok(EngineFrame::Message(SocketPacket::ConnectError { message, .. })) => {
                    return Err(RealtimeError::NamespaceConnectTimeout(message));
                }
                Ok(_) => {}
                Err(_) => {
                    return Err(RealtimeError::EngineOpen(
                        "invalid frame during namespace connect".into(),
                    ))
                }
            },
            Some(Ok(_)) => {
                return Err(RealtimeError::EngineOpen(
                    "non-text frame during namespace connect".into(),
                ))
            }
            Some(Err(e)) => return Err(RealtimeError::Connect(e.to_string())),
            None => {
                return Err(RealtimeError::TransportClosed(
                    "closed during namespace connect".into(),
                ))
            }
        }
    }
}

fn resolve_ack(data: &[JSONValue], correlation: &str) -> std::result::Result<AckData, ()> {
    let Some(first) = data.first() else {
        return Err(());
    };
    let ack: HandshakeAck = serde_json::from_value(first.clone()).map_err(|_| ())?;
    ack.validated(correlation).map_err(|_| ())
}

/// Sends one `state_request` emit and arms the ack slot; clears the buffer
/// first (reference: `handshakeID = correlation; pendingPushes = []`).
async fn send_issue(
    write: &mut WsWriter,
    event_tx: &UnboundedSender<RealtimeEvent>,
    config: &RealtimeConfig,
    next_ack_id: &mut u64,
    pending: &mut Option<PendingAck>,
    buffer: &mut Vec<StatePushData>,
    request: &StateRequest,
) {
    let correlation = uuid::Uuid::new_v4().to_string();
    let payload = match serde_json::to_value(request) {
        Ok(v) => v,
        Err(_) => {
            let _ = event_tx.send(RealtimeEvent::Failed {
                stage: "encoding".into(),
            });
            return;
        }
    };
    let id = *next_ack_id;
    *next_ack_id += 1;
    buffer.clear();
    let frame = match codec::encode_event_ack(
        NAMESPACE,
        id,
        &[json!({
            "ts": now_ms(),
            "data": payload,
            "correlationId": correlation,
        })],
    ) {
        Ok(frame) => frame,
        Err(_) => {
            let _ = event_tx.send(RealtimeEvent::Failed {
                stage: "encoding".into(),
            });
            return;
        }
    };
    if write.send(Message::Text(frame)).await.is_err() {
        let _ = event_tx.send(RealtimeEvent::Failed {
            stage: "socket-error".into(),
        });
        return;
    }
    *pending = Some(PendingAck {
        id,
        correlation,
        deadline: Instant::now() + config.ack_timeout,
    });
}

async fn run_loop(
    config: RealtimeConfig,
    context: ConnectContext,
    initial_request: StateRequest,
    mut command_rx: UnboundedReceiver<Command>,
    event_tx: UnboundedSender<RealtimeEvent>,
) {
    let established = tokio::time::timeout(config.connect_timeout, establish(&context)).await;
    let (mut write, mut read, open) = match established {
        Ok(Ok(ok)) => ok,
        Ok(Err(err)) => {
            let _ = event_tx.send(RealtimeEvent::Failed {
                stage: "socket-error".into(),
            });
            let _ = event_tx.send(RealtimeEvent::Closed(Some(err)));
            return;
        }
        Err(_) => {
            let _ = event_tx.send(RealtimeEvent::Failed {
                stage: "connect-timeout".into(),
            });
            let _ = event_tx.send(RealtimeEvent::Closed(Some(
                RealtimeError::NamespaceConnectTimeout("connect timed out".into()),
            )));
            return;
        }
    };
    tracing::debug!(
        sid = %open.sid,
        ping_interval_ms = open.ping_interval_ms,
        ping_timeout_ms = open.ping_timeout_ms,
        "realtime connected"
    );

    let mut next_ack_id: u64 = 0;
    let mut pending: Option<PendingAck> = None;
    let mut queued: Option<StateRequest> = None;
    let mut buffer: Vec<StatePushData> = Vec::new();
    let mut last_inbound = Instant::now();

    send_issue(
        &mut write,
        &event_tx,
        &config,
        &mut next_ack_id,
        &mut pending,
        &mut buffer,
        &initial_request,
    )
    .await;

    loop {
        let ping_deadline = last_inbound
            + Duration::from_millis(open.ping_interval_ms)
            + Duration::from_millis(open.ping_timeout_ms)
            + config.ping_grace;
        let ack_armed = pending.is_some();
        let ack_deadline = pending
            .as_ref()
            .map(|p| p.deadline)
            .unwrap_or_else(Instant::now);

        tokio::select! {
                    cmd = command_rx.recv() => {
                        match cmd {
                            Some(Command::RequestState(request)) => {
                                if pending.is_some() {
                                    // Reference: queuedRequest replaces any queued one.
                                    queued = Some(request);
                                } else {
                                    send_issue(
                                        &mut write,
                                        &event_tx,
                                        &config,
                                        &mut next_ack_id,
                                        &mut pending,
                                        &mut buffer,
                                        &request,
                                    )
                                    .await;
                                }
                            }
                            Some(Command::Disconnect) | None => {
                                let _ = write.send(Message::Text(codec::encode_close())).await;
                                let _ = write.close().await;
                                let _ = event_tx.send(RealtimeEvent::Closed(None));
                                return;
                            }
                        }
                    }
                    inbound = read.next() => {
                        last_inbound = Instant::now();
                        match inbound {
                            Some(Ok(Message::Text(text))) => {
                                match codec::parse_frame(&text) {
                                    Ok(EngineFrame::Ping) => {
                                        if write.send(Message::Text(codec::encode_pong())).await.is_err() {
                                            let _ = event_tx.send(RealtimeEvent::Failed {
                                                stage: "socket-error".into(),
                                            });
                                            let _ = event_tx.send(RealtimeEvent::Closed(Some(
                                                RealtimeError::TransportClosed("write failed".into()),
                                            )));
                                            return;
                                        }
                                    }
                                    Ok(EngineFrame::Pong) | Ok(EngineFrame::Open(_)) => {}
                                    Ok(EngineFrame::Close) => {
                                        let _ = event_tx.send(RealtimeEvent::Failed {
                                            stage: "socket-disconnect".into(),
                                        });
                                        let _ = event_tx.send(RealtimeEvent::Closed(Some(
                                            RealtimeError::TransportClosed("server close frame".into()),
                                        )));
                                        return;
                                    }
                                    Ok(EngineFrame::Message(SocketPacket::Event { nsp, data }))
                                        if nsp == NAMESPACE
                                            && data.first().and_then(|v| v.as_str())
                                                == Some("state_push") =>
                                    {
                                        let decoded = data
                                            .get(1)
                                            .and_then(|raw| {
                                                serde_json::from_value::<StatePush>(raw.clone()).ok()
                                            });
                                        match decoded {
                                            Some(push) => {
                                                if pending.is_some() {
                                                    // Reference guard (Realtime.swift:53-56):
                                                    // overflow only fails the stage; the
                                                    // connection and ack slot stay armed.
                                                    if buffer.len() >= config.push_buffer_capacity {
                                                        let _ = event_tx.send(RealtimeEvent::Failed {
                                                            stage: "payload-or-handshake".into(),
                                                        });
                                                    } else {
                                                        buffer.push(push.data);
                                                    }
                                                } else {
                                                    let _ = event_tx.send(RealtimeEvent::Push(Box::new(push.data)));
                                                }
                                            }
                                            None => {
                                                let _ = event_tx.send(RealtimeEvent::Failed {
                                                    stage: "payload-or-handshake".into(),
                                                });
                                            }
                                        }
                                    }
                                    Ok(EngineFrame::Message(SocketPacket::Event { .. })) => {
                                        // Reference registers no other event handlers.
                                    }
                                    Ok(EngineFrame::Message(SocketPacket::Ack { nsp, id, data }))
                                        if nsp == NAMESPACE
                                            && pending.as_ref().is_some_and(|p| p.id == id) =>
                                    {
                                        let pending_ack = pending.take().expect("checked above");
                                        match resolve_ack(&data, &pending_ack.correlation) {
                                            Ok(accepted) => {
                                                if let Some(q) = queued.take() {
                                                    // Reference: re-issue the queued
                                                    // request instead of finishing the
                                                    // handshake (buffer already cleared
        // by send_issue).
                                                    buffer.clear();
                                                    send_issue(
                                                        &mut write,
                                                        &event_tx,
                                                        &config,
                                                        &mut next_ack_id,
                                                        &mut pending,
                                                        &mut buffer,
                                                        &q,
                                                    )
                                                    .await;
                                                } else {
                                                    let _ = event_tx.send(RealtimeEvent::Handshake {
                                                        epoch: accepted.runtime_epoch,
                                                        seq_base: accepted.seq_base,
                                                    });
                                                    for push in buffer.drain(..) {
                                                        let _ =
                                                            event_tx.send(RealtimeEvent::Push(Box::new(push)));
                                                    }
                                                }
                                            }
                                            Err(()) => {
                                                buffer.clear();
                                                let _ = event_tx.send(RealtimeEvent::Failed {
                                                    stage: "payload-or-handshake".into(),
                                                });
                                            }
                                        }
                                    }
                                    Ok(EngineFrame::Message(SocketPacket::Ack { .. })) => {
                                        // Stale or unknown ack id: ignored.
                                    }
                                    Ok(EngineFrame::Message(SocketPacket::Disconnect { .. })) => {
                                        let _ = event_tx.send(RealtimeEvent::Failed {
                                            stage: "socket-disconnect".into(),
                                        });
                                        let _ = event_tx.send(RealtimeEvent::Closed(Some(
                                            RealtimeError::TransportClosed(
                                                "namespace disconnected by server".into(),
                                            ),
                                        )));
                                        return;
                                    }
                                    Ok(EngineFrame::Message(SocketPacket::Connect { .. })) => {}
                                    Ok(EngineFrame::Message(SocketPacket::ConnectError {
                                        message,
                                        ..
                                    })) => {
                                        let _ = event_tx.send(RealtimeEvent::Failed {
                                            stage: "socket-error".into(),
                                        });
                                        let _ = event_tx.send(RealtimeEvent::Closed(Some(
                                            RealtimeError::TransportClosed(format!(
                                                "namespace connect error: {message}"
                                            )),
                                        )));
                                        return;
                                    }
                                    Err(_) => {
                                        // Decode failure keeps the connection (reference:
                                        // invalid JSON only emits the failure stage).
                                        let _ = event_tx.send(RealtimeEvent::Failed {
                                            stage: "payload-or-handshake".into(),
                                        });
                                    }
                                }
                            }
                            Some(Ok(Message::Binary(_))) => {
                                // Binary socket.io packets are outside the JSON subset.
                                let _ = event_tx.send(RealtimeEvent::Failed {
                                    stage: "payload-or-handshake".into(),
                                });
                            }
                            Some(Ok(Message::Close(_))) | None => {
                                let _ = event_tx.send(RealtimeEvent::Failed {
                                    stage: "socket-disconnect".into(),
                                });
                                let _ = event_tx.send(RealtimeEvent::Closed(Some(
                                    RealtimeError::TransportClosed("connection closed".into()),
                                )));
                                return;
                            }
                            Some(Err(err)) => {
                                let _ = event_tx.send(RealtimeEvent::Failed {
                                    stage: "socket-error".into(),
                                });
                                let _ = event_tx.send(RealtimeEvent::Closed(Some(
                                    RealtimeError::TransportClosed(err.to_string()),
                                )));
                                return;
                            }
                            Some(Ok(_)) => {
                                // WS-level ping/pong/frames: handled by tungstenite.
                            }
                        }
                    }
                    _ = tokio::time::sleep_until(ack_deadline), if ack_armed => {
                        // Reference: ack timeout surfaces as the empty-ack failure
                        // stage and keeps the connection open for a later request.
                        pending = None;
                        buffer.clear();
                        let _ = event_tx.send(RealtimeEvent::Failed {
                            stage: "payload-or-handshake".into(),
                        });
                    }
                    _ = tokio::time::sleep_until(ping_deadline) => {
                        let _ = event_tx.send(RealtimeEvent::Failed {
                            stage: "socket-disconnect".into(),
                        });
                        let _ = event_tx.send(RealtimeEvent::Closed(Some(RealtimeError::PingTimeout)));
                        return;
                    }
                }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builds_ws_url_from_origin() {
        let url = build_ws_url("https://a0.example.com:8443").unwrap();
        assert_eq!(url.scheme(), "wss");
        assert_eq!(url.host_str(), Some("a0.example.com"));
        assert_eq!(url.port(), Some(8443));
        assert_eq!(url.path(), "/socket.io/");
        assert_eq!(url.query(), Some("EIO=4&transport=websocket"));

        let url = build_ws_url("http://127.0.0.1:8080").unwrap();
        assert_eq!(url.scheme(), "ws");
        assert!(build_ws_url("ftp://example.com").is_err());
    }

    #[test]
    fn connect_context_debug_redacts_secrets() {
        let ctx = ConnectContext {
            origin: "https://x.example".into(),
            cookie_header: "session=secret-cookie-value".into(),
            csrf_token: "secret-token".into(),
        };
        let rendered = format!("{ctx:?}");
        assert!(!rendered.contains("secret-cookie-value"));
        assert!(!rendered.contains("secret-token"));
        assert!(rendered.contains("redacted"));
    }

    #[test]
    fn config_defaults_match_reference() {
        let config = RealtimeConfig::default();
        assert_eq!(config.connect_timeout, Duration::from_secs(10));
        assert_eq!(config.ack_timeout, Duration::from_secs(10));
        assert_eq!(config.push_buffer_capacity, 32);
    }

    #[test]
    fn resolve_ack_validates_correlation() {
        let ack = json!({
            "correlationId": "c1",
            "results": [{
                "handlerId": "ws_webui.WsWebui",
                "ok": true,
                "data": {"runtime_epoch": "e1", "seq_base": 3},
            }],
        });
        let accepted = resolve_ack(&[ack], "c1").unwrap();
        assert_eq!(accepted.runtime_epoch, "e1");
        assert_eq!(accepted.seq_base, 3);
        assert!(resolve_ack(&[], "c1").is_err());
    }
}
