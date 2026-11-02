//! Realtime transport error types.

use thiserror::Error;

/// Errors produced by the realtime transport.
#[derive(Debug, Error)]
pub enum RealtimeError {
    /// WebSocket connection failed.
    #[error("websocket connect: {0}")]
    Connect(String),

    /// Engine.io open handshake failed or timed out.
    #[error("engine open: {0}")]
    EngineOpen(String),

    /// Namespace connect ack not received within timeout.
    #[error("namespace connect timeout: {0}")]
    NamespaceConnectTimeout(String),

    /// state_request ack timed out (10 s per reference).
    #[error("state_request ack timeout")]
    AckTimeout,

    /// Handshake validation failed (a0-core::CoreError).
    #[error("handshake: {0}")]
    Handshake(#[from] a0_core::CoreError),

    /// Packet codec error.
    #[error("codec: {0}")]
    Codec(String),

    /// Pre-ack push buffer overflow (>32).
    #[error("pre-ack push buffer overflow")]
    PushBufferOverflow,

    /// Ping timeout (server stopped responding).
    #[error("ping timeout")]
    PingTimeout,

    /// Transport closed unexpectedly.
    #[error("transport closed: {0}")]
    TransportClosed(String),

    /// URL parse failure.
    #[error("url: {0}")]
    Url(#[from] url::ParseError),

    /// JSON serialization/deserialization failure.
    #[error("json: {0}")]
    Json(#[from] serde_json::Error),

    /// The client is disconnected; operation rejected.
    #[error("disconnected")]
    Disconnected,
}

/// Convenience alias.
pub type Result<T> = std::result::Result<T, RealtimeError>;
