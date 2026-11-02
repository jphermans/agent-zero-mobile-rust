//! Plugins-layer error type.

use thiserror::Error;

/// Errors produced by the plugins feature module.
#[derive(Debug, Error)]
pub enum PluginsError {
    /// HTTP/API call failed.
    #[error("api: {0}")]
    Api(#[from] a0_api::ApiError),

    /// A destructive operation ended in an uncertain state (timeout, broken
    /// response). The caller must reconcile manually; the service never
    /// auto-repeats it (protocol inventory §6, project instruction §9).
    #[error("uncertain result after {operation}; manual reconciliation required")]
    UncertainResult { operation: String },

    /// The plugin is protected and rejects the requested operation.
    #[error("plugin '{0}' is protected")]
    Protected(String),
}

/// Convenience alias.
pub type Result<T> = std::result::Result<T, PluginsError>;
