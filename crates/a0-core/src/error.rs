//! Unified error type for the `a0-core` shared domain logic.
//!
//! Every variant maps onto a documented protocol or domain failure and cites
//! its source section in `docs/phase1/protocol-inventory.md`.

use thiserror::Error;

/// Errors produced by `a0-core` domain logic.
///
/// The enum is [`non_exhaustive`]: new protocol failures may be added as the
/// phase documents grow; downstream crates must match with a wildcard arm.
#[derive(Debug, Error)]
#[non_exhaustive]
pub enum CoreError {
    /// Handshake ack failed validation (protocol inventory sec 5.3, Swift error
    /// `invalidHandshake`, `Protocol.swift:105-113`).
    #[error("invalid handshake: {0}")]
    InvalidHandshake(String),

    /// A snapshot could not be applied; a full resync is required
    /// (protocol inventory sec 5.4, `SyncReducer.swift:51-84`).
    #[error("snapshot rejected ({reason:?}): {detail}")]
    SnapshotRejected {
        /// Machine-readable rejection cause derived from sec 5.4.
        reason: crate::reducer::SnapshotRejectReason,
        /// Human-readable detail for logging.
        detail: String,
    },

    /// A state push was stale, duplicated, out of order or epoch-fenced
    /// (protocol inventory sec 5.4, `SyncReducer.swift:42-50`).
    #[error("push rejected ({reason:?}): {detail}")]
    PushRejected {
        /// Machine-readable rejection cause derived from sec 5.4.
        reason: crate::reducer::PushRejectReason,
        /// Human-readable detail for logging.
        detail: String,
    },

    /// Illegal delivery state-machine transition. There is deliberately no
    /// auto-resend edge: commands are never auto-retried (protocol inventory
    /// sec 6, `Projects.swift:169` et al.).
    #[error("illegal delivery transition from {from} to {to}")]
    IllegalTransition {
        /// Source status rendering.
        from: String,
        /// Attempted target status rendering.
        to: String,
    },

    /// Attachment staging limits violated (protocol inventory sec 7:
    /// max 5 files, 10 MiB per file, 20 MiB total, `Attachments.swift:22,51-52`).
    #[error("attachment limit violated: {0}")]
    AttachmentLimit(String),

    /// Pre-handshake push buffer overflowed (protocol inventory sec 4, failure
    /// stage `payload-or-handshake`, `Realtime.swift:53-56`).
    #[error("pending push buffer overflow: {0}")]
    BufferOverflow(String),

    /// Wrapper for `serde_json` codec failures; corresponds to the Swift
    /// `incompatiblePayload` mapping for undecodable JSON (protocol inventory sec 1,
    /// `HTTPClient.swift:307-310`).
    #[error("json codec error: {0}")]
    Json(#[from] serde_json::Error),
}
