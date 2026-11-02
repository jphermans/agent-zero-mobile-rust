//! Chat-layer error type.

use thiserror::Error;

/// Errors produced by the chat feature module.
#[derive(Debug, Error)]
pub enum ChatError {
    /// HTTP/API call failed.
    #[error("api: {0}")]
    Api(#[from] a0_api::ApiError),

    /// Core invariant violated (delivery FSM, draft limits).
    #[error("core: {0}")]
    Core(#[from] a0_core::CoreError),

    /// Persistence failed.
    #[error("storage: {0}")]
    Storage(#[from] a0_storage::StorageError),

    /// The delivery FSM rejected an explicit transition.
    #[error("invalid delivery transition to {0:?}")]
    InvalidTransition(a0_core::DeliveryStatus),

    /// The chat is not connected; sends are refused (never auto-retried).
    #[error("not connected")]
    NotConnected,

    /// The referenced outbox message does not exist.
    #[error("unknown message")]
    UnknownMessage,
}

/// Convenience alias.
pub type Result<T> = std::result::Result<T, ChatError>;
