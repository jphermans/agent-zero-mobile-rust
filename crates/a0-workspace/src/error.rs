//! Workspace-layer error type.

use thiserror::Error;

/// Errors produced by workspace handoff construction.
#[derive(Debug, Error)]
pub enum WorkspaceError {
    /// The requested target is not same-origin with the session origin.
    #[error("cross-origin workspace target rejected: {0}")]
    CrossOrigin(String),

    /// The origin or target URL failed to parse.
    #[error("invalid url: {0}")]
    Url(String),

    /// Insecure scheme rejected (https required; debug loopback http is
    /// an explicit platform-level exception and never built here).
    #[error("insecure scheme: {0}")]
    InsecureScheme(String),

    /// API call failed (workspace tool discovery).
    #[error("api: {0}")]
    Api(#[from] a0_api::ApiError),
}

/// Convenience alias.
pub type Result<T> = std::result::Result<T, WorkspaceError>;
