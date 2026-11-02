//! Agent Zero realtime transport: a minimal Socket.IO v3 (engine.io EIO=4)
//! client implementing exactly the subset Agent Zero uses.
//!
//! Contract: `docs/phase1/protocol-inventory.md` section 4 and the reference
//! `Sources/A0Realtime/Realtime.swift`. WebSocket-only transport
//! (`.forceWebsockets(true)`), namespace `/ws`, connect payload
//! `{csrf_token, handlers:["ws_webui"]}`, ack-based `state_request` with a
//! 10 s timeout, bounded 32-push pre-ack buffer, queued request re-issue,
//! generation-safe disconnect, and **no library auto-reconnect** (the
//! coordinator reconnects with a fresh security context).
//!
//! EIO=4 direction: the server sends engine pings (`2`) and the client
//! answers pongs (`3`); inbound silence beyond
//! `pingInterval + pingTimeout + grace` fails the connection.

pub mod client;
pub mod codec;
pub mod error;

pub use client::{ConnectContext, RealtimeConfig, RealtimeEvent, RealtimeHandle};
pub use error::{RealtimeError, Result};
