//! # a0-core: the shared domain core of the Agent Zero mobile Rust port
//!
//! `a0-core` is the base of the crate dependency graph: every feature module
//! and platform layer depends on it, and it depends on nothing but the serde
//! family (architecture.md sec 2.1: "a0-core depends on nothing but
//! serde/thiserror"; dependency direction: `a0-platform -> a0-ui -> feature
//! modules -> a0-core / a0-api / a0-auth / a0-realtime / a0-storage`).
//!
//! It holds the platform-agnostic domain logic shared by all mobile builds:
//!
//! - domain identifiers ([`ids`]) and the thin JSON wrapper ([`json`]),
//! - the state-sync wire DTOs ([`protocol`]), transcribed from the phase-1
//!   protocol inventory (`docs/phase1/protocol-inventory.md`),
//! - the pure synchronization reducer ([`reducer`]) implementing the
//!   documented `(State, Action) -> (State, Effects)` style, including the
//!   bounded pre-handshake buffer and stale/out-of-order push rejection,
//! - the delivery state machine for outbound commands ([`delivery`]) with
//!   explicit transitions only and no auto-resend edge,
//! - the draft composition model ([`draft`]) as a plain value type keyed by
//!   `(ServerProfileId, ChatId)` (preserve-on-rotation semantics),
//! - the unified [`error::CoreError`].
//!
//! ## Design rules (architecture.md sec 2.2 and sec 4)
//!
//! - **No I/O types.** No networking, storage, clock or RNG access; hosts
//!   inject time and identifiers. This keeps the crate portable and the
//!   reducer testable on any target.
//! - **Pure reducer style.** Sync operations take `&mut self` and return a
//!   report plus a list of [`reducer::Effect`]s; hosts execute the effects.
//! - **Single writer.** The reducer is the single writer of app state and
//!   UIs render from immutable committed snapshots (architecture.md sec 4.4
//!   invariant), so suspend/resume cannot produce divergent UI state.
//! - **Wire fidelity.** Every DTO field cites its protocol-inventory
//!   section. Details the inventory leaves open are modelled as
//!   `#[non_exhaustive]` types with `TODO(doc-gap)` comments; no wire field
//!   is ever invented.

#![forbid(unsafe_code)]

pub mod delivery;
pub mod draft;
pub mod error;
pub mod ids;
pub mod json;
pub mod protocol;
pub mod reducer;

pub use delivery::{Delivery, DeliveryReason, DeliveryStatus};
pub use draft::{
    Draft, DraftKey, StagedAttachment, MAX_ATTACHMENTS, MAX_FILE_BYTES, MAX_TOTAL_BYTES,
};
pub use error::CoreError;
pub use ids::{ChatId, MessageId, ProjectId, ServerProfileId};
pub use json::JSONValue;
pub use protocol::{
    AckData, AckResult, ContextSummary, HandshakeAck, LogEntry, NotificationEntry, Snapshot,
    StatePush, StatePushData, StateRequest, TaskSummary, ACCEPTED_HANDLER_IDS,
};
pub use reducer::{
    ApplyReport, Effect, PushRejectReason, SnapshotRejectReason, SyncReducer,
    PENDING_BUFFER_CAPACITY,
};
