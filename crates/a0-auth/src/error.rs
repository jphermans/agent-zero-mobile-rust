//! Error model for the auth crate — named errors from §2.1/§2.2/§2.3.

use thiserror::Error;

#[derive(Debug, Error)]
pub enum AuthError {
    #[error("transport failure: {0}")]
    Transport(#[from] reqwest::Error),
    #[error("unexpected http status {0}")]
    Status(u16),
    /// §2.1: credentials wrong — non-redirect login result (including a 200
    /// HTML error page), cross-origin login redirect, or a login redirect on
    /// the post-login bootstrap (`HTTPClient.swift:161-169`).
    #[error("invalid credentials")]
    InvalidCredentials,
    /// §2.1 step 1: 200 bootstrap on an empty jar = login disabled, only
    /// acceptable for unauthenticated loopback dev origins
    /// (`HTTPClient.swift:148-154`).
    #[error("unauthenticated server")]
    UnauthenticatedServer,
    /// §2.3 restore: saved cookie present but the authenticated bootstrap
    /// still redirects → `requiresLogin` (`HTTPClient.swift:178-194`).
    #[error("requires login")]
    RequiresLogin,
    /// §2.2/§2.3 bootstrap or persisted-cookie validation failure.
    #[error("incompatible payload: {0}")]
    IncompatiblePayload(String),
    /// §1: non-JSON content type on a JSON endpoint.
    #[error("unexpected response")]
    UnexpectedResponse,
    /// SecretStore backend failure.
    #[error("secret store failure: {0}")]
    Store(String),
    /// Explicit Connect semantics (task contract): the flow refuses to
    /// continue on auth/cert errors — no automatic login retry.
    #[error("connect aborted: {0}")]
    ConnectAborted(String),
}
