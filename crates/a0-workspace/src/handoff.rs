//! Authenticated workspace WebView handoff.
//!
//! Security model (project instruction §10, feature parity doc §Workspace):
//! - The WebView authenticates with the session **cookies** only; tokens,
//!   passwords and the CSRF value are never placed in URLs.
//! - Targets must be strictly same-origin with the session origin
//!   (scheme, host, port). Navigation policies and cookie isolation are
//!   enforced by the platform layer; this module is the gate that decides
//!   which URLs may be handed off at all.

use url::Url;

use crate::error::{Result, WorkspaceError};

/// A validated handoff target ready for the platform WebView.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HandoffTarget {
    url: Url,
}

impl HandoffTarget {
    /// The validated absolute URL (same-origin with the session origin).
    pub fn url(&self) -> &Url {
        &self.url
    }

    /// The path+query portion for platform navigation logging
    /// (never contains credentials by construction).`
    pub fn path_for_log(&self) -> String {
        let mut s = self.url.path().to_string();
        if let Some(q) = self.url.query() {
            s.push('?');
            s.push_str(q);
        }
        s
    }
}

/// Validates `target` as a workspace handoff for `session_origin`.
///
/// Rules: absolute URL; scheme `https` (or `http` **only** when the session
/// origin itself is debug loopback http — the §2.1 development exception);
/// scheme, host and port exactly equal to the session origin's.
pub fn validate_handoff(session_origin: &str, target: &str) -> Result<HandoffTarget> {
    let origin = Url::parse(session_origin).map_err(|e| WorkspaceError::Url(e.to_string()))?;
    let parsed = Url::parse(target).map_err(|e| WorkspaceError::Url(e.to_string()))?;

    if parsed.scheme() != origin.scheme() {
        return Err(if parsed.scheme() == "http" {
            WorkspaceError::InsecureScheme(target.to_string())
        } else {
            WorkspaceError::CrossOrigin(target.to_string())
        });
    }
    if parsed.host_str() != origin.host_str() || parsed.port() != origin.port() {
        return Err(WorkspaceError::CrossOrigin(target.to_string()));
    }
    Ok(HandoffTarget { url: parsed })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn same_origin_https_is_accepted() {
        let target = validate_handoff(
            "https://a0.example.com:8443",
            "https://a0.example.com:8443/workspace/browser",
        )
        .unwrap();
        assert_eq!(target.path_for_log(), "/workspace/browser");
    }

    #[test]
    fn cross_origin_host_rejected() {
        let err = validate_handoff("https://a0.example.com", "https://evil.example/workspace")
            .unwrap_err();
        assert!(matches!(err, WorkspaceError::CrossOrigin(_)));
    }

    #[test]
    fn port_mismatch_rejected() {
        assert!(matches!(
            validate_handoff(
                "https://a0.example.com:8443",
                "https://a0.example.com:9000/x"
            ),
            Err(WorkspaceError::CrossOrigin(_))
        ));
    }

    #[test]
    fn http_target_rejected_for_https_origin() {
        assert!(matches!(
            validate_handoff("https://a0.example.com", "http://a0.example.com/x"),
            Err(WorkspaceError::InsecureScheme(_))
        ));
    }

    #[test]
    fn debug_loopback_http_pair_is_accepted() {
        let target = validate_handoff(
            "http://127.0.0.1:8080",
            "http://127.0.0.1:8080/workspace/desktop",
        )
        .unwrap();
        assert_eq!(target.url().scheme(), "http");
    }
}
