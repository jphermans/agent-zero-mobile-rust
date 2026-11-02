//! Minimal standalone HTTP layer for the auth flows — inventory §1
//! conventions (`HTTPClient.swift:22-50,76-93,260-310`) and §2.1/§2.2
//! wire shapes. Deliberately small: auth needs exactly `GET
//! /api/csrf_token` and form `POST /login`.
//!
//! Contract rules mirrored from §1:
//! * `redirect(Policy::none())` — redirects never followed; 3xx inspected
//!   manually (login success IS a 3xx, §2.1 step 3).
//! * Manual cookie jar (reqwest cookie store off) with §2.4 serialization:
//!   jar cookies name-sorted, synthetic `csrf_token_<runtime_id>` LAST.
//! * Headers per request: `Accept: application/json`, `Origin`, body
//!   `Content-Type`, `X-CSRF-Token` once held, `Cookie` when non-empty.
//! * Response validation (`HTTPClient.swift:301-310`): 2xx required for
//!   JSON endpoints, `application/json` content type, decode failure →
//!   incompatible-payload error.

use std::collections::BTreeMap;
use std::sync::Mutex;
use std::time::Duration;

use reqwest::Url;
use serde_json::Value;

use crate::error::AuthError;
use crate::session::Session;

/// §2.2 bootstrap (`HTTPClient.swift:197-206`; fixture §2.2/§8.3):
/// `{"ok":true,"token":"…","runtime_id":"…"}` with strict charset rules.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Bootstrap {
    pub token: String,
    pub runtime_id: String,
}

fn ascii_ident(s: &str) -> bool {
    !s.is_empty()
        && s.bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
}

/// Parse + validate §2.2 (charset `[A-Za-z0-9_-]`, non-empty, `ok:true`).
/// The `session_<runtime_id>` cookie presence is checked by the caller
/// against its jar (§2.2 post-condition).
pub fn parse_bootstrap(payload: &Value) -> Result<Bootstrap, AuthError> {
    let ok = payload.get("ok").and_then(Value::as_bool).unwrap_or(false);
    let token = payload.get("token").and_then(Value::as_str).unwrap_or("");
    let runtime_id = payload
        .get("runtime_id")
        .and_then(Value::as_str)
        .unwrap_or("");
    if !ok || !ascii_ident(token) || !ascii_ident(runtime_id) {
        return Err(AuthError::IncompatiblePayload("bootstrap".to_string()));
    }
    Ok(Bootstrap {
        token: token.to_string(),
        runtime_id: runtime_id.to_string(),
    })
}

/// Validated origin (§1, `HTTPClient.swift:22-50`): HTTPS-only (loopback
/// `http` in debug only), no userinfo/query/fragment, host lower-cased.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuthOrigin {
    url: Url,
    host: String,
}

impl AuthOrigin {
    pub fn new(raw: &str, allow_debug_loopback_http: bool) -> Result<AuthOrigin, AuthError> {
        let invalid = || AuthError::IncompatiblePayload(format!("invalidOrigin: {raw}"));
        let mut url = Url::parse(raw).map_err(|_| invalid())?;
        if !url.username().is_empty() || url.password().is_some() {
            return Err(invalid());
        }
        if url.query().is_some() || url.fragment().is_some() {
            return Err(invalid());
        }
        let host = url.host_str().unwrap_or_default().to_ascii_lowercase();
        if host.is_empty() {
            return Err(invalid());
        }
        let is_loopback = host == "localhost" || host.starts_with("127.");
        match url.scheme() {
            "https" => {}
            "http" if allow_debug_loopback_http && is_loopback => {}
            _ => return Err(invalid()),
        }
        url.set_path("");
        Ok(AuthOrigin { url, host })
    }

    pub fn host(&self) -> &str {
        &self.host
    }

    /// Resolve a Location header value against this origin (login-redirect
    /// checks, §2.1 step 3).
    pub fn resolve(&self, location: &str) -> Option<Url> {
        self.url.join(location).ok()
    }

    /// True when `other` resolves to exactly this origin (scheme + host +
    /// port); §1 login-redirect detection and §2.1 cross-origin rejection.
    pub fn same_origin(&self, other: &Url) -> bool {
        other.scheme() == self.url.scheme()
            && other.host_str().map(|h| h.to_ascii_lowercase()).as_deref()
                == Some(self.host.as_str())
            && other.port_or_known_default() == self.url.port_or_known_default()
    }
}

impl std::fmt::Display for AuthOrigin {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.url.as_str().trim_end_matches('/'))
    }
}

/// RFC 3986 unreserved-character percent-encoding (§2.1 step 2,
/// `HTTPClient.swift:311-314`).
pub(crate) fn form_encode(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for byte in value.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' => {
                out.push(byte as char);
            }
            _ => out.push_str(&format!("%{:02X}", byte)),
        }
    }
    out
}

/// Outcome of one raw request: status, Location header and body bytes.
pub(crate) struct RawResponse {
    pub status: u16,
    pub location: Option<String>,
    pub content_type: String,
    pub body: Vec<u8>,
}

impl RawResponse {
    pub(crate) fn is_json(&self) -> bool {
        self.content_type.contains("application/json")
    }

    pub(crate) fn json(&self) -> Result<Value, AuthError> {
        serde_json::from_slice(&self.body)
            .map_err(|_| AuthError::IncompatiblePayload("jsonDecode".to_string()))
    }
}

/// One stored cookie (compact jar for the auth flows only).
#[derive(Debug, Clone)]
struct Cookie {
    name: String,
    value: String,
}

/// Auth HTTP client for one origin. Manual cookie jar per §1
/// (`httpCookieStorage = nil`, `httpShouldSetCookies = false`);
/// reqwest's own store is disabled.
pub struct AuthHttpClient {
    http: reqwest::Client,
    pub origin: AuthOrigin,
    cookies: Mutex<BTreeMap<String, Cookie>>,
    /// Held after a successful bootstrap (§2.5 `SocketSession` material).
    session: Mutex<Option<Session>>, // below via crate::session
}

impl AuthHttpClient {
    pub fn new(origin: AuthOrigin) -> Result<AuthHttpClient, AuthError> {
        let http = reqwest::Client::builder()
            // §1: redirects are never followed (`HTTPClient.swift:76-82`).
            .redirect(reqwest::redirect::Policy::none())
            .connect_timeout(Duration::from_secs(5))
            .timeout(Duration::from_secs(30))
            .cookie_store(false)
            .build()
            .map_err(AuthError::Transport)?;
        Ok(AuthHttpClient {
            http,
            origin,
            cookies: Mutex::new(BTreeMap::new()),
            session: Mutex::new(None),
        })
    }

    /// §2.4 cookie header: jar cookies sorted by name, then the synthetic
    /// `csrf_token_<runtime_id>` cookie appended LAST.
    pub fn cookie_header(&self) -> String {
        let jar = self.cookies.lock().unwrap_or_else(|p| p.into_inner());
        let session = self.session.lock().unwrap_or_else(|p| p.into_inner());
        let mut header = String::new();
        for cookie in jar.values() {
            if !header.is_empty() {
                header.push_str("; ");
            }
            header.push_str(&cookie.name);
            header.push('=');
            header.push_str(&cookie.value);
        }
        if let Some(session) = session.as_ref() {
            if !header.is_empty() {
                header.push_str("; ");
            }
            header.push_str("csrf_token_");
            header.push_str(&session.runtime_id);
            header.push('=');
            header.push_str(&session.csrf_token);
        }
        header
    }

    pub fn held_session(&self) -> Option<Session> {
        self.session
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .clone()
    }

    pub(crate) fn set_session(&self, session: Session) {
        *self.session.lock().unwrap_or_else(|p| p.into_inner()) = Some(session);
    }

    /// Insert the restored cookie into the jar (§2.3 restore). Any previous
    /// `session_*` cookie is replaced (one cookie per profile).
    pub(crate) fn insert_session_cookie(&self, name: &str, value: &str) {
        self.cookies
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .insert(
                name.to_string(),
                Cookie {
                    name: name.to_string(),
                    value: value.to_string(),
                },
            );
    }

    pub(crate) fn session_cookie(&self) -> Option<(String, String)> {
        let jar = self.cookies.lock().unwrap_or_else(|p| p.into_inner());
        jar.values()
            .find(|c| c.name.starts_with("session_"))
            .map(|c| (c.name.clone(), c.value.clone()))
    }

    /// §2.2 post-condition: the jar must hold `session_<runtime_id>` and is
    /// then pruned to EXACTLY that cookie (`HTTPClient.swift:203-205`).
    pub(crate) fn prune_to_session(&self, session_name: &str) -> bool {
        let mut jar = self.cookies.lock().unwrap_or_else(|p| p.into_inner());
        if !jar.contains_key(session_name) {
            return false;
        }
        jar.retain(|name, _| name == session_name);
        true
    }

    /// §2.3 logout: client-side destruction only — no server endpoint exists.
    pub fn disconnect(&self) {
        self.cookies
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .clear();
        *self.session.lock().unwrap_or_else(|p| p.into_inner()) = None;
    }

    fn classify_status(
        status: u16,
        location: Option<&str>,
        origin: &AuthOrigin,
    ) -> Result<(), AuthError> {
        match status {
            200..=299 => Ok(()),
            401 => Err(AuthError::RequiresLogin),
            300..=399 => {
                // §1 login redirect: same-origin Location with path /login.
                let login_redirect = location
                    .and_then(|loc| origin.url.join(loc).ok())
                    .map(|resolved| origin.same_origin(&resolved) && resolved.path() == "/login")
                    .unwrap_or(false);
                if login_redirect {
                    Err(AuthError::RequiresLogin)
                } else {
                    Err(AuthError::Status(status))
                }
            }
            _ => Err(AuthError::Status(status)),
        }
    }

    /// One request with §1 headers; captures Set-Cookie into the jar.
    pub(crate) async fn request_raw(
        &self,
        method: reqwest::Method,
        path_and_query: &str,
        body: Option<(Vec<u8>, &str)>,
    ) -> Result<RawResponse, AuthError> {
        let url = format!("{}{}", self.origin, path_and_query);
        let mut rb = self.http.request(method, &url);
        rb = rb.header(reqwest::header::ACCEPT, "application/json");
        rb = rb.header(reqwest::header::ORIGIN, self.origin.to_string());
        if let Some((bytes, content_type)) = &body {
            rb = rb.header(reqwest::header::CONTENT_TYPE, *content_type);
            rb = rb.body(bytes.clone());
        }
        if let Some(session) = self.held_session() {
            rb = rb.header("X-CSRF-Token", session.csrf_token);
        }
        let cookie = self.cookie_header();
        if !cookie.is_empty() {
            rb = rb.header(reqwest::header::COOKIE, cookie);
        }
        let response = rb.send().await?;
        {
            // §1 (`HTTPClient.swift:273-278`): parse Set-Cookie headers of
            // every response; keep only ours (single-origin client).
            let mut jar = self.cookies.lock().unwrap_or_else(|p| p.into_inner());
            for value in response
                .headers()
                .get_all(reqwest::header::SET_COOKIE)
                .iter()
            {
                if let Ok(v) = value.to_str() {
                    if let Some(nv) = v.split(';').next() {
                        if let Some((name, val)) = nv.split_once('=') {
                            let name = name.trim();
                            if !name.is_empty() {
                                jar.insert(
                                    name.to_string(),
                                    Cookie {
                                        name: name.to_string(),
                                        value: val.trim().to_string(),
                                    },
                                );
                            }
                        }
                    }
                }
            }
        }
        let status = response.status().as_u16();
        let location = response
            .headers()
            .get(reqwest::header::LOCATION)
            .and_then(|v| v.to_str().ok())
            .map(|s| s.to_string());
        let content_type = response
            .headers()
            .get(reqwest::header::CONTENT_TYPE)
            .and_then(|v| v.to_str().ok())
            .unwrap_or("")
            .to_string();
        let bytes = response.bytes().await?.to_vec();
        Ok(RawResponse {
            status,
            location,
            content_type,
            body: bytes,
        })
    }
}

impl AuthHttpClient {
    /// [`AuthHttpClient::request_raw`] plus §1 status classification
    /// (2xx ok; 401/login-redirect → [`AuthError::RequiresLogin`]; other
    /// non-2xx → [`AuthError::Status`]). Flows that need raw statuses (the
    /// §2.1 anonymous probe and the login 3xx success) use `request_raw`.
    pub(crate) async fn request(
        &self,
        method: reqwest::Method,
        path_and_query: &str,
        body: Option<(Vec<u8>, &str)>,
    ) -> Result<RawResponse, AuthError> {
        let response = self.request_raw(method, path_and_query, body).await?;
        Self::classify_status(response.status, response.location.as_deref(), &self.origin)?;
        Ok(response)
    }

    /// Snapshot of the stored jar cookies in §2.4 order (name-sorted,
    /// without the synthetic csrf cookie).
    pub fn cookies(&self) -> Vec<(String, String)> {
        let jar = self.cookies.lock().unwrap_or_else(|p| p.into_inner());
        jar.values()
            .map(|c| (c.name.clone(), c.value.clone()))
            .collect()
    }
}
