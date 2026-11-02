//! Projects-layer error type.

use thiserror::Error;

/// Errors produced by the projects feature module.
#[derive(Debug, Error)]
pub enum ProjectsError {
    /// HTTP/API call failed.
    #[error("api: {0}")]
    Api(#[from] a0_api::ApiError),

    /// Deletion attempted without the explicit typed confirmation.
    #[error("project deletion requires explicit confirmation")]
    DeleteNotConfirmed,

    /// Project name invalid (empty or whitespace-only).
    #[error("invalid project name")]
    InvalidName,
}

/// Convenience alias.
pub type Result<T> = std::result::Result<T, ProjectsError>;
