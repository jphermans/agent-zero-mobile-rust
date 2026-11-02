//! Storage-layer error type.

use thiserror::Error;

/// Errors produced by the encrypted persistence layer.
#[derive(Debug, Error)]
pub enum StorageError {
    /// SQLite/SQLCipher operation failed (including wrong-key decryption).
    #[error("sqlite: {0}")]
    Sqlite(#[from] rusqlite::Error),

    /// Key provider failed.
    #[error("key provider: {0}")]
    Key(String),

    /// Filesystem I/O failed.
    #[error("io: {0}")]
    Io(#[from] std::io::Error),

    /// JSON (de)serialization of stored values failed.
    #[error("json: {0}")]
    Json(#[from] serde_json::Error),

    /// Core invariant violated (e.g. a persisted attachment fails
    /// re-validation on load).
    #[error("core: {0}")]
    Core(#[from] a0_core::CoreError),

    /// Requested row does not exist.
    #[error("not found")]
    NotFound,

    /// Caller violated a storage invariant (e.g. empty identifier).
    #[error("invalid input: {0}")]
    InvalidInput(String),
}

/// Convenience alias.
pub type Result<T> = std::result::Result<T, StorageError>;
