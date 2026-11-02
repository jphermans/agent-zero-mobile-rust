//! Core HTTP client — inventory §1 conventions, §2.1/2.2 auth primitives,
//! §6 retry policy and §9 size caps.
//!
//! Contract enforcement here:
//! * `redirect(Policy::none())` — redirects are never followed (§1,
//!   `HTTPClient.swift:76-82`); 3xx responses are classified manually.
//! * Cookies are managed MANUALLY in [`crate::cookies::CookieJar`] per §1
//!   (`httpCookieStorage = nil`, `httpShouldSetCookies = false`,
//!   `HTTPClient.swift:86-93`). reqwest's automatic cookie store is
//!   therefore NOT enabled even though the `cookies` feature is compiled
//!   in: reqwest's store cannot reproduce the §2.4 header ordering
//!   (name-sorted jar with the synthetic csrf cookie appended LAST).
//! * Headers on every request: `Accept`, `Origin`, body `Content-Type`,
//!   `X-CSRF-Token` once a token is held, `Cookie` when non-empty
//!   (§1, `HTTPClient.swift:260-268`).
//! * Epoch fencing (§1, `HTTPClient.swift:138-142,269-272`): every request
//!   captures a generation counter; [`A0Client::disconnect`] bumps it and
//!   any in-flight request resolves to [`ApiError::Cancelled`].
//! * Mutations are never retried (§6); idempotent reads (GETs and the
//!   foreground poll) retry at most 4 times on transient transport/5xx
//!   failures with the constant 1/2/4/8 s table scaled by `1 + jitter*0.25`
//!   (`PollRecovery.swift:12-19`), jitter seeded from a caller-supplied
//!   `u64` (deterministic, no rng dependency).

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde::de::DeserializeOwned;
use serde_json::Value;
use url::Url;

use crate::cookies::CookieJar;
use crate::error::{ApiError, AuthErrorKind, TransportKind};
use crate::util::{new_uuid_v4, query_encode, SeededJitter};

/// Constant retry backoff table per §6 (`PollRecovery.swift:12-19`):
/// delay `2^attempts` seconds (1, 2, 4, 8), max 4 transient retries.
pub(crate) const RETRY_TABLE: [Duration; 4] = [
    Duration::from_secs(1),
    Duration::from_secs(2),
    Duration::from_secs(4),
    Duration::from_secs(8),
];

/// Max transient retries per §6: "max 4 transient retries; the 5th failure
/// ... transitions to `paused`".
pub(crate) const MAX_RETRIES: usize = 4;

/// Validated base origin (§1, `HTTPClient.swift:22-50`): HTTPS-only, no
/// userinfo/query/fragment, path stripped, host lower-cased. Loopback `http`
/// is accepted only through the explicit debug flag (`.loopbackDevelopment`
/// policy); production callers must pass `false`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Origin {
    url: Url,
    host: String,
    debug_loopback: bool,
}

impl Origin {
    pub fn new(raw: &str, allow_debug_loopback_http: bool) -> Result<Origin, ApiError> {
        let invalid = || ApiError::Protocol(format!("invalidOrigin: {raw}"));
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
        let is_loopback = host == "localhost"
            || host.starts_with("127.")
            || matches!(url.host(), Some(url::Host::Ipv6(ip)) if ip == std::net::Ipv6Addr::LOCALHOST);
        match url.scheme() {
            "https" => {}
            "http" if allow_debug_loopback_http && is_loopback => {}
            _ => return Err(invalid()),
        }
        url.set_path(""); // host lower-cased, path stripped (§1, `HTTPClient.swift:44`)
        Ok(Origin {
            url,
            host,
            debug_loopback: allow_debug_loopback_http,
        })
    }

    pub fn url(&self) -> &Url {
        &self.url
    }

    pub fn host(&self) -> &str {
        &self.host
    }

    /// Whether this origin was created with the debug-loopback-http flag
    /// (§2.1 step 1 gates the login-disabled exception on it).
    pub fn allows_debug_loopback(&self) -> bool {
        self.debug_loopback
    }

    /// True when `other` resolves to exactly this origin (scheme + host +
    /// port). Used for §1 login-redirect detection and §2.1 login checks.
    pub fn same_origin(&self, other: &Url) -> bool {
        other.scheme() == self.url.scheme()
            && other.host_str().map(|h| h.to_ascii_lowercase()).as_deref()
                == Some(self.host.as_str())
            && other.port_or_known_default() == self.url.port_or_known_default()
    }
}

impl std::fmt::Display for Origin {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.url.as_str().trim_end_matches('/'))
    }
}

/// §2.5 `SocketSession` view (`HTTPClient.swift:122-127,256-259`):
/// `{ origin, cookieHeader, csrfToken, runtimeID }`. Feature methods call
/// [`A0Client::socket_session`] as an authentication guard before mutating
/// endpoints.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SocketSession {
    pub origin: String,
    pub cookie_header: String,
    pub csrf_token: String,
    pub runtime_id: String,
}

#[derive(Debug, Clone)]
pub(crate) struct AuthState {
    pub(crate) csrf: String,
    pub(crate) runtime_id: String,
}

/// Request body variants. `Json` is the default for API calls; `Raw` serves
/// the form-encoded login (§2.1) and the hand-built multipart upload (§7).
#[derive(Clone)]
pub(crate) enum Body {
    None,
    Json(Value),
    Raw(Vec<u8>, String),
}

/// §6: only idempotent reads (GETs, the foreground poll) may retry transient
/// failures; mutations (every POST) never retry.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RetryPolicy {
    Never,
    TransientRead,
}

pub(crate) struct RequestSpec {
    pub(crate) method: reqwest::Method,
    pub(crate) path_and_query: String,
    pub(crate) body: Body,
    /// §9 response size cap for this endpoint.
    pub(crate) response_cap: usize,
    pub(crate) retry: RetryPolicy,
    /// `Accept` override (image transfer uses image MIMEs, §3.9).
    pub(crate) accept: Option<&'static str>,
}

/// Builder for [`A0Client`]: timeouts, retry seed and the rustls platform
/// verifier hook (architecture §5.1) — accepts a full rustls client-config
/// override for tests.
pub struct A0ClientBuilder {
    origin: Origin,
    connect_timeout: Duration,
    total_timeout: Duration,
    mutation_timeout: Duration,
    retry_seed: u64,
    tls_adjuster:
        Option<Box<dyn Fn(reqwest::ClientBuilder) -> reqwest::ClientBuilder + Send + Sync>>,
}

impl A0ClientBuilder {
    pub(crate) fn new(origin: Origin) -> Self {
        Self {
            origin,
            connect_timeout: Duration::from_secs(5),
            total_timeout: Duration::from_secs(30),
            mutation_timeout: Duration::from_secs(60),
            retry_seed: 0,
            tls_adjuster: None,
        }
    }

    /// Connect timeout, 5 s default (task contract).
    pub fn connect_timeout(mut self, d: Duration) -> Self {
        self.connect_timeout = d;
        self
    }

    /// Total request timeout, 30 s default (task contract).
    pub fn total_timeout(mut self, d: Duration) -> Self {
        self.total_timeout = d;
        self
    }

    /// Total timeout for mutations, 60 s default (task contract).
    pub fn mutation_timeout(mut self, d: Duration) -> Self {
        self.mutation_timeout = d;
        self
    }

    /// Deterministic jitter seed for the §6 backoff (task: "seeded from a
    /// passed u64").
    pub fn retry_seed(mut self, seed: u64) -> Self {
        self.retry_seed = seed;
        self
    }

    /// rustls platform-verifier hook (architecture §5.1): receives the
    /// reqwest builder after the contract-critical options are applied and
    /// returns it with TLS customized (platform verifier in production, a
    /// custom rustls config in tests).
    pub fn tls_adjuster(
        mut self,
        f: impl Fn(reqwest::ClientBuilder) -> reqwest::ClientBuilder + Send + Sync + 'static,
    ) -> Self {
        self.tls_adjuster = Some(Box::new(f));
        self
    }

    pub fn build(self) -> Result<A0Client, ApiError> {
        let mut builder = reqwest::Client::builder()
            // §1: redirects are never followed (`HTTPClient.swift:76-82`).
            .redirect(reqwest::redirect::Policy::none())
            .connect_timeout(self.connect_timeout)
            // Cookies are managed manually (§1); reqwest's store stays off so
            // the §2.4 ordering is guaranteed by [`CookieJar`].
            .cookie_store(false);
        if let Some(adjust) = &self.tls_adjuster {
            builder = adjust(builder);
        }
        let http = builder.build().map_err(|e| ApiError::Transport {
            kind: TransportKind::Connect,
            source: e,
        })?;
        Ok(A0Client {
            http,
            origin: self.origin,
            jar: Mutex::new(CookieJar::new()),
            state: Mutex::new(None),
            epoch: Arc::new(AtomicU64::new(0)),
            total_timeout: self.total_timeout,
            mutation_timeout: self.mutation_timeout,
            retry_seed: self.retry_seed,
        })
    }
}

/// Agent Zero HTTP client bound to one [`Origin`]. Cheap to clone-free share
/// via `Arc` by the caller; interior state is mutex-guarded.
pub struct A0Client {
    pub(crate) http: reqwest::Client,
    pub(crate) origin: Origin,
    pub(crate) jar: Mutex<CookieJar>,
    pub(crate) state: Mutex<Option<AuthState>>,
    /// §1 epoch fencing: bumped by [`A0Client::disconnect`]; in-flight
    /// requests capture the value before sending and fail with
    /// [`ApiError::Cancelled`] when it changed afterwards.
    pub(crate) epoch: Arc<AtomicU64>,
    pub(crate) total_timeout: Duration,
    pub(crate) mutation_timeout: Duration,
    pub(crate) retry_seed: u64,
}

impl A0Client {
    pub fn builder(origin: Origin) -> A0ClientBuilder {
        A0ClientBuilder::new(origin)
    }

    pub fn origin(&self) -> &Origin {
        &self.origin
    }

    fn lock<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
        // A poisoned lock still contains consistent data for our usage
        // (single-field swaps); recover instead of panicking.
        mutex.lock().unwrap_or_else(|p| p.into_inner())
    }

    /// §2.4 header assembly: jar cookies (name-sorted, non-expired) plus the
    /// synthetic `csrf_token_<runtime_id>` cookie appended LAST.
    pub(crate) fn cookie_header_locked(&self) -> String {
        let st = Self::lock(&self.state);
        let jar = Self::lock(&self.jar);
        jar.cookie_header(
            st.as_ref().map(|s| s.runtime_id.as_str()),
            st.as_ref().map(|s| s.csrf.as_str()),
        )
    }

    /// §2.5 `SocketSession` guard value; `None` until a bootstrap succeeded.
    /// Feature modules call this before mutating endpoints.
    pub fn socket_session(&self) -> Option<SocketSession> {
        let st = Self::lock(&self.state).clone()?;
        let (csrf, runtime_id) = (st.csrf.clone(), st.runtime_id.clone());
        // Lock only the jar here; cookie_header_locked() would re-lock
        // self.state (std Mutex is not reentrant -> deadlock).
        let cookie_header = {
            let jar = Self::lock(&self.jar);
            jar.cookie_header(Some(runtime_id.as_str()), Some(csrf.as_str()))
        };
        Some(SocketSession {
            origin: self.origin.to_string(),
            cookie_header,
            csrf_token: csrf,
            runtime_id,
        })
    }

    /// Like [`A0Client::socket_session`] but as the contract-style guard
    /// error: `requiresLogin` when no session is held (§2.5 feature-method
    /// guard semantics).
    pub fn require_socket_session(&self) -> Result<SocketSession, ApiError> {
        self.socket_session().ok_or(ApiError::Auth {
            kind: AuthErrorKind::RequiresLogin,
        })
    }

    /// §2.3: insert a restored session cookie (validation lives in `a0-auth`).
    pub fn insert_session_cookie(&self, name: &str, value: &str) {
        Self::lock(&self.jar).set(crate::cookies::Cookie {
            name: name.to_string(),
            value: value.to_string(),
            expires: None,
        });
    }

    /// §2.3: the single persisted cookie `session_<runtime_id>` (name, value)
    /// — the ONLY material ever persisted (`SavedAuthentication.swift:5-13`).
    pub fn session_cookie(&self) -> Option<(String, String)> {
        let jar = Self::lock(&self.jar);
        let found = jar.iter().find_map(|c| {
            if c.name.starts_with("session_") {
                Some((c.name.clone(), c.value.clone()))
            } else {
                None
            }
        });
        found
    }

    /// §1: clear the session on `requiresLogin` / `csrfRejected`.
    pub(crate) fn clear_session(&self) {
        Self::lock(&self.jar).clear();
        *Self::lock(&self.state) = None;
    }

    /// §2.3 logout: local state destruction ONLY — no server endpoint exists
    /// in the reference source (`HTTPClient.swift:142`; §3.10 grep). Also
    /// bumps the epoch so in-flight requests resolve to [`ApiError::Cancelled`]
    /// (§1 epoch fencing).
    pub fn disconnect(&self) {
        self.epoch.fetch_add(1, Ordering::SeqCst);
        self.clear_session();
    }

    /// Client-generated uuid for `message_id` / `item_id` / boundaries.
    pub fn new_request_id(&self) -> String {
        new_uuid_v4()
    }

    /// §1 status classification (`HTTPClient.swift:295-306`): 2xx ok; 401 or
    /// a same-origin `/login` redirect → `requiresLogin`; 403 →
    /// `csrfRejected`; every other non-2xx (including generic 3xx, which are
    /// never followed) → [`ApiError::Status`].
    pub(crate) fn classify_status(&self, status: u16, location: Option<&str>) -> Option<ApiError> {
        match status {
            200..=299 => None,
            401 => Some(ApiError::Auth {
                kind: AuthErrorKind::RequiresLogin,
            }),
            403 => Some(ApiError::Auth {
                kind: AuthErrorKind::CsrfRejected,
            }),
            300..=399 => {
                let login_redirect = location
                    .and_then(|loc| self.origin.url.join(loc).ok())
                    .map(|resolved| {
                        self.origin.same_origin(&resolved) && resolved.path() == "/login"
                    })
                    .unwrap_or(false);
                Some(if login_redirect {
                    ApiError::Auth {
                        kind: AuthErrorKind::RequiresLogin,
                    }
                } else {
                    ApiError::Status { code: status }
                })
            }
            _ => Some(ApiError::Status { code: status }),
        }
    }

    fn transport_kind(err: &reqwest::Error) -> TransportKind {
        if err.is_timeout() {
            TransportKind::Timeout
        } else if err.is_connect() {
            TransportKind::Connect
        } else {
            TransportKind::Network
        }
    }

    /// One send attempt: epoch capture, header assembly (§1), body encoding,
    /// total timeout (30 s reads / 60 s mutations per task contract), then
    /// Set-Cookie absorption (§1 `HTTPClient.swift:273-278`) and the §1
    /// post-transfer epoch re-check (`HTTPClient.swift:269-272`).
    pub(crate) async fn send_once(
        &self,
        spec: &RequestSpec,
        mutation: bool,
    ) -> Result<reqwest::Response, ApiError> {
        let epoch_before = self.epoch.load(Ordering::SeqCst);
        let method = spec.method.clone();
        let url = format!("{}{}", self.origin, spec.path_and_query);
        let mut rb = self.http.request(method, &url);
        rb = rb.header(
            reqwest::header::ACCEPT,
            spec.accept.unwrap_or("application/json"),
        );
        rb = rb.header(reqwest::header::ORIGIN, self.origin.to_string());
        match &spec.body {
            Body::None => {}
            Body::Json(v) => {
                rb = rb.header(reqwest::header::CONTENT_TYPE, "application/json");
                rb = rb.body(
                    serde_json::to_vec(v)
                        .map_err(|e| ApiError::Protocol(format!("encode: {e}")))?,
                );
            }
            Body::Raw(bytes, content_type) => {
                rb = rb.header(reqwest::header::CONTENT_TYPE, content_type.clone());
                rb = rb.body(bytes.clone());
            }
        }
        {
            let st = Self::lock(&self.state);
            if let Some(s) = st.as_ref() {
                rb = rb.header("X-CSRF-Token", s.csrf.clone());
            }
        }
        let cookie = self.cookie_header_locked();
        if !cookie.is_empty() {
            rb = rb.header(reqwest::header::COOKIE, cookie);
        }
        let timeout = if mutation {
            self.mutation_timeout
        } else {
            self.total_timeout
        };
        rb = rb.timeout(timeout);
        let result = rb.send().await;
        if self.epoch.load(Ordering::SeqCst) != epoch_before {
            return Err(ApiError::Cancelled);
        }
        let response = result.map_err(|e| ApiError::Transport {
            kind: Self::transport_kind(&e),
            source: e,
        })?;
        {
            let mut jar = Self::lock(&self.jar);
            jar.absorb(&response, &self.origin.host);
        }
        Ok(response)
    }
}

impl A0Client {
    /// Stream-read the body enforcing a §9 size cap (content-length fast path
    /// plus chunk-wise enforcement for chunked transfer encoding).
    pub(crate) async fn read_body_capped(
        response: reqwest::Response,
        cap: usize,
    ) -> Result<Vec<u8>, ApiError> {
        if let Some(len) = response.content_length() {
            if len as usize > cap {
                return Err(ApiError::Protocol("payloadTooLarge".to_string()));
            }
        }
        let mut out = Vec::with_capacity(4096.min(cap));
        let mut stream = response;
        loop {
            let chunk = match stream.chunk().await {
                Ok(Some(c)) => c,
                Ok(None) => break,
                Err(e) => {
                    return Err(ApiError::Transport {
                        kind: TransportKind::Network,
                        source: e,
                    });
                }
            };
            if out.len() + chunk.len() > cap {
                return Err(ApiError::Protocol("payloadTooLarge".to_string()));
            }
            out.extend_from_slice(&chunk);
        }
        Ok(out)
    }

    /// §1 `validate` + decode (`HTTPClient.swift:301-310`): status via
    /// [`A0Client::classify_status`], `Content-Type` must contain
    /// `application/json` (else `unexpectedResponse`), decode failure maps to
    /// `incompatiblePayload`. Enforces the §6 retry policy: retries only when
    /// `retry == TransientRead` and the failure is transient (5xx of
    /// transport) — at most [`MAX_RETRIES`] times with the constant
    /// [`RETRY_TABLE`] delays scaled by `1 + jitter*0.25`.
    pub(crate) async fn execute_json<T: DeserializeOwned>(
        &self,
        spec: RequestSpec,
    ) -> Result<T, ApiError> {
        let body = self.execute_json_raw(spec).await?;
        serde_json::from_slice(&body)
            .map_err(|_| ApiError::Protocol("incompatiblePayload".to_string()))
    }

    /// Like [`A0Client::execute_json`] but returns raw bytes (callers that
    /// apply their own structural validation, e.g. §3.1 chat responses that
    /// must echo exact ids).
    pub(crate) async fn execute_json_raw(&self, spec: RequestSpec) -> Result<Vec<u8>, ApiError> {
        let mut jitter = SeededJitter::new(self.retry_seed);
        let mut attempt: usize = 0;
        loop {
            // POST bodies are mutations (60 s, never retried) EXCEPT the
            // foreground poll, which is a read per §6 (30 s, retryable).
            let mutation = !matches!(spec.body, Body::None) && spec.retry == RetryPolicy::Never;
            // §6: transient TRANSPORT failures also retry on idempotent reads
            // (same budget as the 5xx retry below); mutations fail fast.
            let response = match self.send_once(&spec, mutation).await {
                Ok(response) => response,
                Err(e) => {
                    if spec.retry == RetryPolicy::TransientRead
                        && e.is_transient()
                        && attempt < MAX_RETRIES
                    {
                        let delay = jitter.backoff(attempt, &RETRY_TABLE);
                        attempt += 1;
                        tracing::warn!(attempt, ?delay, error = %e, "transient transport failure on read; retrying");
                        tokio::time::sleep(delay).await;
                        continue;
                    }
                    return Err(e);
                }
            };
            let status = response.status().as_u16();
            let location = response
                .headers()
                .get(reqwest::header::LOCATION)
                .and_then(|v| v.to_str().ok())
                .map(|s| s.to_string());
            if let Some(err) = self.classify_status(status, location.as_deref()) {
                // §1: 401/403 always clear the session. Retries only apply to
                // transient 5xx on idempotent reads (§6); auth errors never retry.
                if matches!(err, ApiError::Auth { .. }) {
                    self.clear_session();
                    return Err(err);
                }
                let transient_5xx = matches!(status, 500 | 502 | 503 | 504);
                if spec.retry == RetryPolicy::TransientRead
                    && transient_5xx
                    && attempt < MAX_RETRIES
                {
                    let delay = jitter.backoff(attempt, &RETRY_TABLE);
                    attempt += 1;
                    tracing::warn!(attempt, ?delay, status, "transient 5xx on read; retrying");
                    tokio::time::sleep(delay).await;
                    continue;
                }
                return Err(err);
            }
            let ctype = response
                .headers()
                .get(reqwest::header::CONTENT_TYPE)
                .and_then(|v| v.to_str().ok())
                .unwrap_or("")
                .to_string();
            if !ctype.contains("application/json") {
                return Err(ApiError::Protocol("unexpectedResponse".to_string()));
            }
            return Self::read_body_capped(response, spec.response_cap).await;
        }
    }

    /// POST a JSON body and decode the JSON response (the standard shape of
    /// every §3 endpoint; POST = mutation → never retried, §6).
    pub(crate) async fn post_json<T: DeserializeOwned>(
        &self,
        path_and_query: &str,
        payload: &Value,
        response_cap: usize,
    ) -> Result<T, ApiError> {
        self.execute_json(RequestSpec {
            method: reqwest::Method::POST,
            path_and_query: path_and_query.to_string(),
            body: Body::Json(payload.clone()),
            response_cap,
            retry: RetryPolicy::Never,
            accept: None,
        })
        .await
    }

    /// POST returning raw bytes with the status preserved — used by the §3.6
    /// `model_override` 403-exception (body exactly `Per-chat override is
    /// disabled` → `overrideDisabled`, session kept) and by login.
    pub(crate) async fn execute_preserving_status(
        &self,
        spec: RequestSpec,
    ) -> Result<(u16, Option<String>, Vec<u8>), ApiError> {
        // Same read/mutation split as execute_json_raw: POST is a mutation
        // (60 s, never retried) unless marked TransientRead (§6 poll exception).
        let mutation = !matches!(spec.body, Body::None) && spec.retry == RetryPolicy::Never;
        let response = self.send_once(&spec, mutation).await?;
        let status = response.status().as_u16();
        let location = response
            .headers()
            .get(reqwest::header::LOCATION)
            .and_then(|v| v.to_str().ok())
            .map(|s| s.to_string());
        // 401 still clears per §1; 403 classification is left to the caller.
        if status == 401 {
            self.clear_session();
            return Err(ApiError::Auth {
                kind: AuthErrorKind::RequiresLogin,
            });
        }
        let body = Self::read_body_capped(response, spec.response_cap).await?;
        Ok((status, location, body))
    }
}

impl A0Client {
    fn csrf_request_spec() -> RequestSpec {
        RequestSpec {
            method: reqwest::Method::GET,
            path_and_query: "/api/csrf_token".to_string(),
            body: Body::None,
            response_cap: 64 * 1024,
            retry: RetryPolicy::Never,
            accept: None,
        }
    }

    /// §2.2 `acceptBootstrap` (`HTTPClient.swift:197-206`): `ok:true`; `token`
    /// and `runtime_id` non-empty ASCII `[A-Za-z0-9_-]`; the jar must hold a
    /// cookie named `session_<runtime_id>`; afterwards the jar is pruned to
    /// exactly that cookie (`HTTPClient.swift:203-205`). Returns
    /// `(csrf, runtime_id)` and installs the [`AuthState`].
    fn accept_bootstrap(&self, payload: Value) -> Result<(String, String), ApiError> {
        let ok = payload.get("ok").and_then(Value::as_bool).unwrap_or(false);
        let token = payload.get("token").and_then(Value::as_str).unwrap_or("");
        let runtime_id = payload
            .get("runtime_id")
            .and_then(Value::as_str)
            .unwrap_or("");
        let valid = |s: &str| {
            !s.is_empty()
                && s.len() <= 256
                && s.bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
        };
        if !ok || !valid(token) || !valid(runtime_id) {
            return Err(ApiError::Protocol("incompatiblePayload".to_string()));
        }
        let session_name = format!("session_{runtime_id}");
        {
            let mut jar = Self::lock(&self.jar);
            if jar.get(&session_name).is_none() {
                return Err(ApiError::Protocol("incompatiblePayload".to_string()));
            }
            jar.prune_to(&session_name);
        }
        {
            let mut st = Self::lock(&self.state);
            *st = Some(AuthState {
                csrf: token.to_string(),
                runtime_id: runtime_id.to_string(),
            });
        }
        Ok((token.to_string(), runtime_id.to_string()))
    }

    /// §2.1 step 1 / §2.3 restore / §2.3 refresh: `GET /api/csrf_token` and
    /// validate the §2.2 bootstrap when it returns 200.
    ///
    /// * 200 + valid bootstrap → `Ok((csrf, runtime_id))` (allowed on an
    ///   empty jar only when login is disabled — callers enforce the §2.1
    ///   `unauthenticatedServer` guard for production origins).
    /// * Login redirect or 401 → `Err(Auth { RequiresLogin })` (callers
    ///   continue the login flow).
    /// * Anything else → the classified error (`unexpectedResponse` etc.).
    pub async fn csrf_token(&self) -> Result<(String, String), ApiError> {
        let spec = Self::csrf_request_spec();
        let response = self.send_once(&spec, false).await?;
        let status = response.status().as_u16();
        let location = response
            .headers()
            .get(reqwest::header::LOCATION)
            .and_then(|v| v.to_str().ok())
            .map(|s| s.to_string());
        match self.classify_status(status, location.as_deref()) {
            None => {
                let ctype = response
                    .headers()
                    .get(reqwest::header::CONTENT_TYPE)
                    .and_then(|v| v.to_str().ok())
                    .unwrap_or("")
                    .to_string();
                if !ctype.contains("application/json") {
                    return Err(ApiError::Protocol("unexpectedResponse".to_string()));
                }
                let body = Self::read_body_capped(response, spec.response_cap).await?;
                let payload: Value = serde_json::from_slice(&body)
                    .map_err(|_| ApiError::Protocol("incompatiblePayload".to_string()))?;
                self.accept_bootstrap(payload)
            }
            Some(err) => Err(err),
        }
    }

    /// §2.3 restore path (`HTTPClient.swift:178-194`): the saved cookie has
    /// been inserted via [`A0Client::insert_session_cookie`]; the anonymous
    /// csrf GET must still require login (200 → login-disabled server), then
    /// the authenticated bootstrap is validated. No credentials are involved.
    pub async fn restore_session(&self) -> Result<(String, String), ApiError> {
        // Step 1: anonymous probe — a 200 here means login is disabled.
        // Because our jar already holds the restored cookie, a plain GET
        // returning 200 with a valid bootstrap IS the authenticated success
        // path; a login redirect fails the restore (§2.3: 3xx/401 on the
        // authenticated bootstrap → requiresLogin).
        match self.csrf_token().await {
            Ok(session) => Ok(session),
            Err(
                e @ ApiError::Auth {
                    kind: AuthErrorKind::RequiresLogin,
                },
            ) => {
                self.clear_session();
                Err(e)
            }
            Err(e) => Err(e),
        }
    }

    /// §2.3 session refresh without credentials (`refreshSocketSession`,
    /// `HTTPClient.swift:245-255`): re-runs `GET /api/csrf_token` with the
    /// existing cookie and re-validates §2.2. A bootstrap validation failure
    /// disconnects; a transient failure keeps the session for the caller.
    pub async fn refresh_session(&self) -> Result<(String, String), ApiError> {
        match self.csrf_token().await {
            Ok(session) => Ok(session),
            Err(e) => {
                if matches!(
                    e,
                    ApiError::Auth {
                        kind: AuthErrorKind::RequiresLogin
                    }
                ) {
                    self.clear_session();
                }
                Err(e)
            }
        }
    }

    /// §2.1 first login (`HTTPClient.swift:144-171`):
    /// 1. `POST /login`, `application/x-www-form-urlencoded`, body
    ///    `username=<enc>&password=<enc>` with RFC 3986 unreserved-character
    ///    percent-encoding (verified wire example `HTTPTests.swift:31`:
    ///    `username=user%2Bname&password=a%26b%3D%20c`).
    /// 2. Success = HTTP 3xx whose `Location` resolves to the SAME origin
    ///    with path NOT `/login`; anything else (including a 200 HTML error
    ///    page) → `invalidCredentials`. Cross-origin redirects are rejected
    ///    WITHOUT resending credentials.
    /// 3. `GET /api/csrf_token` again (now with the session cookie); a login
    ///    redirect here → `invalidCredentials` (§2.1 step 4).
    pub async fn login(&self, username: &str, password: &str) -> Result<(), ApiError> {
        let invalid = || ApiError::Auth {
            kind: AuthErrorKind::InvalidCredentials,
        };
        // §2.1 step 1: anonymous probe with an empty jar. HTTP 200 + valid
        // bootstrap means login is DISABLED — accepted ONLY for
        // unauthenticated loopback dev origins (`HTTPClient.swift:148-154`);
        // otherwise `unauthenticatedServer`. Login redirect or 401 → continue.
        match self.csrf_token().await {
            Ok(_) if self.origin.allows_debug_loopback() => return Ok(()),
            Ok(_) => {
                self.clear_session();
                return Err(ApiError::Auth {
                    kind: AuthErrorKind::UnauthenticatedServer,
                });
            }
            Err(ApiError::Auth {
                kind: AuthErrorKind::RequiresLogin,
            }) => {}
            Err(e) => return Err(e),
        }
        let form = format!(
            "username={}&password={}",
            query_encode(username),
            query_encode(password)
        );
        let spec = RequestSpec {
            method: reqwest::Method::POST,
            path_and_query: "/login".to_string(),
            body: Body::Raw(
                form.into_bytes(),
                "application/x-www-form-urlencoded".to_string(),
            ),
            response_cap: 64 * 1024,
            retry: RetryPolicy::Never,
            accept: None,
        };
        let (status, location, _body) = self.execute_preserving_status(spec).await?;
        if !(300..400).contains(&status) {
            // Includes a 200 HTML error page (`HTTPTests.swift:37-43`).
            return Err(invalid());
        }
        let Some(loc) = location.as_deref() else {
            return Err(invalid());
        };
        let resolved = self.origin.url.join(loc).map_err(|_| invalid())?;
        if !self.origin.same_origin(&resolved) || resolved.path() == "/login" {
            return Err(invalid());
        }
        // Step 4: authenticated bootstrap; RequiresLogin here is a bad login.
        match self.csrf_token().await {
            Ok(_) => Ok(()),
            Err(ApiError::Auth {
                kind: AuthErrorKind::RequiresLogin,
            }) => Err(invalid()),
            Err(e) => Err(e),
        }
    }
}

impl A0Client {
    /// Binary transfer path for `GET /api/image_get` (§3.9,
    /// `BrowserScreenshot.swift:58-74` + bounded transport
    /// `HTTPClient.swift:104-118,281-288`): status classification per §1,
    /// but the response is validated against `allowed_content` instead of
    /// the JSON content-type rule. The §6 read-retry policy applies (image
    /// transfer is an idempotent GET); the §1 post-transfer epoch re-check
    /// runs inside `send_once`.
    pub(crate) async fn execute_binary(
        &self,
        spec: RequestSpec,
        allowed_content: impl Fn(&str) -> bool,
    ) -> Result<(Vec<u8>, String), ApiError> {
        let mut jitter = SeededJitter::new(self.retry_seed);
        let mut attempt: usize = 0;
        loop {
            // §6: transient transport failures retry on this idempotent read.
            let response = match self.send_once(&spec, false).await {
                Ok(response) => response,
                Err(e) => {
                    if spec.retry == RetryPolicy::TransientRead
                        && e.is_transient()
                        && attempt < MAX_RETRIES
                    {
                        let delay = jitter.backoff(attempt, &RETRY_TABLE);
                        attempt += 1;
                        tokio::time::sleep(delay).await;
                        continue;
                    }
                    return Err(e);
                }
            };
            let status = response.status().as_u16();
            let location = response
                .headers()
                .get(reqwest::header::LOCATION)
                .and_then(|v| v.to_str().ok())
                .map(|s| s.to_string());
            if let Some(err) = self.classify_status(status, location.as_deref()) {
                if matches!(err, ApiError::Auth { .. }) {
                    self.clear_session();
                    return Err(err);
                }
                let transient_5xx = matches!(status, 500 | 502 | 503 | 504);
                if spec.retry == RetryPolicy::TransientRead
                    && transient_5xx
                    && attempt < MAX_RETRIES
                {
                    let delay = jitter.backoff(attempt, &RETRY_TABLE);
                    attempt += 1;
                    tokio::time::sleep(delay).await;
                    continue;
                }
                return Err(err);
            }
            let ctype = response
                .headers()
                .get(reqwest::header::CONTENT_TYPE)
                .and_then(|v| v.to_str().ok())
                .unwrap_or("")
                .to_string();
            if !allowed_content(&ctype) {
                return Err(ApiError::Protocol("unexpectedResponse".to_string()));
            }
            let body = Self::read_body_capped(response, spec.response_cap).await?;
            return Ok((body, ctype));
        }
    }
}
