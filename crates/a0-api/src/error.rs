//! Error model — inventory §1 (uniform auth handling), §2.1 (login errors)
//! and §6 (transient classes for the retry policy).

use thiserror::Error;

/// Auth failure kinds mirroring the inventory's named errors (§1, §2.1, §3.6).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub enum AuthErrorKind {
    /// HTTP 401 or a login redirect (`requiresLogin`) — the session is
    /// cleared (§1, `HTTPClient.swift:295-306`).
    #[error("requires login")]
    RequiresLogin,
    /// HTTP 403 (`csrfRejected`) — the session is cleared (§1). The
    /// `model_override` 403 exception (§3.6) maps to
    /// [`AuthErrorKind::OverrideDisabled`] instead and keeps the session.
    #[error("csrf rejected")]
    CsrfRejected,
    /// §3.6 `model_override/set_preset`: HTTP 403 with body exactly
    /// `Per-chat override is disabled` → `overrideDisabled` without clearing
    /// the session (`ModelPresets.swift:191-195`).
    #[error("override disabled")]
    OverrideDisabled,
    /// §2.1 login failure (`invalidCredentials`): non-redirect POST /login
    /// result (including a 200 HTML error page), cross-origin login redirect,
    /// or a login redirect on the post-login bootstrap
    /// (`HTTPClient.swift:161-169`).
    #[error("invalid credentials")]
    InvalidCredentials,
    /// §2.1 step 1: a 200 bootstrap on an empty jar means login is disabled —
    /// `unauthenticatedServer` unless the origin is an unauthenticated
    /// loopback dev origin (`HTTPClient.swift:148-154`).
    #[error("unauthenticated server")]
    UnauthenticatedServer,
}

/// Transport failure buckets used by the retry policy (§6, `PollRecovery.swift:20-29`:
/// `notConnectedToInternet`, `networkConnectionLost`, `timedOut`,
/// `cannotConnectToHost`, `cannotFindHost`, `dnsLookupFailed`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TransportKind {
    /// Connection/DNS/TLS establishment failure.
    Connect,
    /// Timed out.
    Timeout,
    /// Connection dropped mid-exchange.
    Network,
}

#[derive(Debug, Error)]
pub enum ApiError {
    #[error("transport failure ({kind:?}): {source}")]
    Transport {
        kind: TransportKind,
        #[source]
        source: reqwest::Error,
    },
    #[error("unexpected http status {code}")]
    Status { code: u16 },
    #[error("auth failure: {kind}")]
    Auth { kind: AuthErrorKind },
    #[error("protocol violation: {0}")]
    Protocol(String),
    #[error("cancelled: authentication epoch changed mid-request (§1 epoch fencing)")]
    Cancelled,
}

impl ApiError {
    /// Transient classification per §6: transport failures plus HTTP
    /// 500/502/503/504. Drives the retry policy for idempotent reads ONLY —
    /// mutations are never retried (§6, `Projects.swift:169`,
    /// `Plugins.swift:149`, `Attachments.swift:70`).
    pub fn is_transient(&self) -> bool {
        match self {
            ApiError::Transport { .. } => true,
            ApiError::Status { code } => matches!(code, 500 | 502 | 503 | 504),
            _ => false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn is_transient_matches_s6() {
        // §6: only 500/502/503/504 (plus transport failures) are transient.
        for code in [500u16, 502, 503, 504] {
            assert!(ApiError::Status { code }.is_transient(), "{code}");
        }
        for code in [401u16, 403, 404, 302, 499, 505] {
            assert!(!ApiError::Status { code }.is_transient(), "{code}");
        }
        assert!(!ApiError::Auth {
            kind: AuthErrorKind::RequiresLogin
        }
        .is_transient());
        assert!(!ApiError::Protocol("x".to_string()).is_transient());
        assert!(!ApiError::Cancelled.is_transient());
    }
}
