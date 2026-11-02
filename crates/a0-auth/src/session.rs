//! Session object — inventory §2.5 (`SocketSession {origin, cookieHeader,
//! csrfToken, runtimeID}`, `HTTPClient.swift:122-127,256-259`) plus the task
//! brief's `username` field. Cookie header serialization per §2.4
//! (`HTTPClient.swift:289-294`): all jar cookies sorted by name joined as
//! `name=value; ...`, then the synthetic `csrf_token_<runtime_id>` cookie
//! appended LAST (verified `HTTPTests.swift:33-34`).

/// §2.2/§2.3 persisted-cookie name: `session_<runtime_id>`.
pub fn session_cookie_name(runtime_id: &str) -> String {
    format!("session_{runtime_id}")
}

/// §2.4 synthetic cookie name: `csrf_token_<runtime_id>`.
pub fn csrf_cookie_name(runtime_id: &str) -> String {
    format!("csrf_token_{runtime_id}")
}

/// Authenticated session held after a validated §2.2 bootstrap.
#[derive(Clone, PartialEq, Eq)]
pub struct Session {
    /// Base origin string (§2.5 `origin`).
    pub origin: String,
    /// §2.5 `csrfToken`; sent as `X-CSRF-Token` and the synthetic cookie.
    pub csrf_token: String,
    /// §2.5 `runtimeID`; names the session cookie and csrf cookie.
    pub runtime_id: String,
    /// Set on credential login; `None` after a credential-free restore (§2.3:
    /// passwords are never persisted or reconstructed).
    pub username: Option<String>,
    /// Jar cookie snapshot `(name, value)` in §2.4 order (name-sorted),
    /// WITHOUT the synthetic csrf cookie.
    pub cookies: Vec<(String, String)>,
}

impl Session {
    /// §2.4 serialization: jar cookies name-sorted joined as `name=value;
    /// ...`, then `csrf_token_<runtime_id>=<csrf>` appended LAST.
    pub fn cookie_header(&self) -> String {
        compose_cookie_header(&self.cookies, Some((&self.runtime_id, &self.csrf_token)))
    }
}

impl std::fmt::Debug for Session {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // §5.5 log redaction: csrf token and cookie values never rendered.
        f.debug_struct("Session")
            .field("origin", &self.origin)
            .field("csrf_token", &"«redacted»")
            .field("runtime_id", &self.runtime_id)
            .field("username", &self.username)
            .field(
                "cookies",
                &self.cookies.iter().map(|(n, _)| n).collect::<Vec<_>>(),
            )
            .finish()
    }
}

/// §2.4 composer shared by [`Session`] and the HTTP layer.
pub(crate) fn compose_cookie_header(
    cookies: &[(String, String)],
    csrf: Option<(&str, &str)>,
) -> String {
    let mut header = String::new();
    for (name, value) in cookies {
        if !header.is_empty() {
            header.push_str("; ");
        }
        header.push_str(name);
        header.push('=');
        header.push_str(value);
    }
    if let Some((runtime_id, token)) = csrf {
        if !header.is_empty() {
            header.push_str("; ");
        }
        header.push_str(&csrf_cookie_name(runtime_id));
        header.push('=');
        header.push_str(token);
    }
    header
}
