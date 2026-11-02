//! Login & session lifecycle flows — inventory §2.1 (first login,
//! `HTTPClient.swift:144-171`), §2.2 (bootstrap, `HTTPClient.swift:195-207`),
//! §2.3 (restore/refresh/logout, `HTTPClient.swift:178-194,245-255,142`)
//! and §8.3 (verified wire values).

use reqwest::Method;
use zeroize::Zeroizing;

use crate::error::AuthError;
use crate::http::{form_encode, parse_bootstrap, AuthHttpClient, AuthOrigin};
use crate::session::{session_cookie_name, Session};
use crate::store::{SecretStore, ServerProfileId};

/// Credential pair for [`AuthFlow::login`]. Held in [`Zeroizing`] buffers and
/// wiped on drop; NEVER persisted (§2.3, `SavedAuthentication.swift:3-4`).
pub struct LoginCredentials {
    pub username: String,
    pub password: Zeroizing<String>,
}

impl LoginCredentials {
    pub fn new(username: impl Into<String>, password: impl Into<String>) -> Self {
        Self {
            username: username.into(),
            password: Zeroizing::new(password.into()),
        }
    }
}

impl std::fmt::Debug for LoginCredentials {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // §5.5 redaction.
        f.debug_struct("LoginCredentials")
            .field("username", &self.username)
            .field("password", &"«redacted»")
            .finish()
    }
}

/// Auth flow driver for one origin + one profile.
pub struct AuthFlow {
    pub http: AuthHttpClient,
    pub profile: ServerProfileId,
}

impl AuthFlow {
    pub fn new(origin: AuthOrigin, profile: ServerProfileId) -> Result<AuthFlow, AuthError> {
        Ok(AuthFlow {
            http: AuthHttpClient::new(origin)?,
            profile,
        })
    }

    /// `GET /api/csrf_token` and, on 200 + JSON, validate the §2.2 bootstrap:
    /// `ok:true`, charset-checked `token`/`runtime_id`, the jar must hold
    /// `session_<runtime_id>` which is then the ONLY jar cookie
    /// (`HTTPClient.swift:203-205`). Returns the installed [`Session`].
    /// Login redirect / 401 → [`AuthError::RequiresLogin`] (caller continues
    /// the login or restore flow).
    async fn fetch_bootstrap(&self) -> Result<Session, AuthError> {
        let response = self
            .http
            .request(Method::GET, "/api/csrf_token", None)
            .await?;
        if !response.is_json() {
            return Err(AuthError::UnexpectedResponse);
        }
        let payload = response.json()?;
        let bootstrap = parse_bootstrap(&payload)?;
        let session_name = session_cookie_name(&bootstrap.runtime_id);
        if !self.http.prune_to_session(&session_name) {
            // §2.2: the jar must contain `session_<runtime_id>`.
            return Err(AuthError::IncompatiblePayload("missingSessionCookie".to_string()));
        }
        let session = Session {
            origin: self.http.origin.to_string(),
            csrf_token: bootstrap.token,
            runtime_id: bootstrap.runtime_id,
            username: None,
            cookies: self.http.cookies(),
        };
        self.http.set_session(session.clone());
        Ok(session)
    }

    /// §2.1 step 1 anonymous probe (`HTTPClient.swift:148-154`): a 200
    /// bootstrap on an EMPTY jar means login is disabled — accepted ONLY for
    /// unauthenticated loopback dev origins, otherwise
    /// [`AuthError::UnauthenticatedServer`]. Login redirect or 401 → `Ok(())`
    /// (continue). Anything else → [`AuthError::UnexpectedResponse`] via the
    /// §1 classification.
    /// `Ok(Some(session))` = server has login disabled and the origin is a
    /// debug-loopback origin: §2.1 accepts the bootstrap and the flow ENDS
    /// here (no POST /login). `Ok(None)` = login required, continue with
    /// credentials.
    async fn probe_login_required(
        &self,
        origin_is_debug_loopback: bool,
    ) -> Result<Option<Session>, AuthError> {
        match self.http.request(Method::GET, "/api/csrf_token", None).await {
            Err(AuthError::RequiresLogin) => Ok(None), // 3xx /login redirect or 401 → continue
            Err(e) => Err(e),
            Ok(response) => {
                if origin_is_debug_loopback && response.is_json() {
                    // Dev-loopback server without login: a valid bootstrap on
                    // an empty jar is the accepted path (§2.1 exception).
                    match self.fetch_bootstrap().await {
                        Ok(session) => Ok(Some(session)),
                        Err(e) => Err(e),
                    }
                } else {
                    Err(AuthError::UnauthenticatedServer)
                }
            }
        }
    }
}

impl AuthFlow {
    /// §2.1 first login (`HTTPClient.swift:144-171`):
    /// 1. `GET /api/csrf_token` with an EMPTY jar (probe, §2.1 step 1).
    /// 2. `POST /login`, `application/x-www-form-urlencoded`, body
    ///    `username=<enc>&password=<enc>` with RFC 3986 unreserved-character
    ///    percent-encoding (wire example `HTTPTests.swift:31`:
    ///    `username=user%2Bname&password=a%26b%3D%20c`).
    /// 3. Success = HTTP 3xx whose `Location` resolves to the SAME origin
    ///    with path NOT `/login`; anything else — including a 200 HTML error
    ///    page (`HTTPTests.swift:37-43`) — is
    ///    [`AuthError::InvalidCredentials`]. Cross-origin redirects are
    ///    rejected WITHOUT resending credentials (`HTTPTests.swift:44-49`).
    /// 4. `GET /api/csrf_token` again (now with the session cookie); a login
    ///    redirect here → [`AuthError::InvalidCredentials`] (§2.1 step 4).
    /// 5. Bootstrap validation (§2.2) and session installation.
    ///
    /// `origin_is_debug_loopback` gates the §2.1 exception for servers with
    /// login disabled on unauthenticated dev-loopback origins.
    /// On success the session cookie is persisted via [`SecretStore::put`]
    /// (exactly one cookie, §2.3).
    pub async fn login(
        &self,
        credentials: LoginCredentials,
        origin_is_debug_loopback: bool,
        store: &dyn SecretStore,
    ) -> Result<Session, AuthError> {
        // Step 1: anonymous probe on an empty jar; an accepted no-login
        // bootstrap ENDS the flow (§2.1 step 1 exception).
        if let Some(session) = self.probe_login_required(origin_is_debug_loopback).await? {
            return Ok(session);
        }
        // Step 2: form POST with RFC 3986 encoding (§2.1 step 2).
        let form = format!(
            "username={}&password={}",
            form_encode(&credentials.username),
            form_encode(&credentials.password)
        );
        let login_response = self
            .http
            .request_raw(
                Method::POST,
                "/login",
                Some((form.into_bytes(), "application/x-www-form-urlencoded")),
            )
            .await?;
        let invalid_credentials = || AuthError::InvalidCredentials;
        if !(300..400).contains(&login_response.status) {
            return Err(invalid_credentials());
        }
        let Some(location) = login_response.location.as_deref() else {
            return Err(invalid_credentials());
        };
        let resolved = self
            .http
            .origin
            .resolve(location)
            .ok_or_else(invalid_credentials)?;
        if !self.http.origin.same_origin(&resolved) || resolved.path() == "/login" {
            return Err(invalid_credentials());
        }
        // Step 4/5: authenticated bootstrap; RequiresLogin → invalid credentials.
        let mut session = self.fetch_bootstrap().await.map_err(|e| match e {
            AuthError::RequiresLogin => invalid_credentials(),
            other => other,
        })?;
        session.username = Some(credentials.username.clone());
        self.http.set_session(session.clone());
        // §2.3: persist EXACTLY the one session cookie.
        let (name, value) = self
            .http
            .session_cookie()
            .ok_or_else(|| AuthError::IncompatiblePayload("missingSessionCookie".to_string()))?;
        store
            .put(crate::store::SavedCookie::new(
                self.profile.clone(),
                name,
                value,
                None,
            )?)
            .await?;
        Ok(session)
    }

    /// §2.3 restore WITHOUT credentials (`HTTPClient.swift:178-194`):
    /// anonymous `GET /api/csrf_token` must still require login (200 →
    /// [`AuthError::UnauthenticatedServer`]; 3xx/401 → continue); then the
    /// saved cookie is inserted into the jar and the bootstrap is retried;
    /// 3xx/401 on the authenticated bootstrap → [`AuthError::RequiresLogin`].
    /// Any failure disconnects. The restored username is NOT reconstructed
    /// (§2.3: usernames are not persisted material).
    pub async fn restore(&self, store: &dyn SecretStore) -> Result<Session, AuthError> {
        let outcome = async {
            // Probe WITHOUT the restored cookie.
            match self.http.request(Method::GET, "/api/csrf_token", None).await {
                Err(AuthError::RequiresLogin) => {}
                Err(e) => return Err(e),
                Ok(_) => return Err(AuthError::UnauthenticatedServer),
            }
            // Load and validate the persisted cookie (§2.3 validation happens
            // in [`crate::store::SavedCookie`]; expiry re-checked here).
            let Some(saved) = store.get(&self.profile).await? else {
                return Err(AuthError::RequiresLogin);
            };
            if let Some(now) = crate::store::now_unix() {
                if saved.is_expired(now) {
                    return Err(AuthError::RequiresLogin);
                }
            }
            self.http.insert_session_cookie(&saved.name, saved.value.as_str());
            // Authenticated bootstrap.
            self.fetch_bootstrap().await
        }
        .await;
        match outcome {
            Ok(session) => Ok(session),
            Err(e) => {
                // §2.3: any failure disconnects.
                self.http.disconnect();
                Err(e)
            }
        }
    }

    /// §2.3 session refresh WITHOUT credentials (`refreshSocketSession`,
    /// `HTTPClient.swift:245-255`): re-runs `GET /api/csrf_token` with the
    /// existing cookie and re-validates §2.2. A transient HTTP failure keeps
    /// the session alive for the caller; a bootstrap validation failure
    /// disconnects.
    pub async fn refresh(&self) -> Result<Session, AuthError> {
        match self.fetch_bootstrap().await {
            Ok(session) => Ok(session),
            Err(e) => {
                if matches!(
                    e,
                    AuthError::RequiresLogin
                        | AuthError::IncompatiblePayload(_)
                        | AuthError::UnexpectedResponse
                ) {
                    self.http.disconnect();
                }
                Err(e)
            }
        }
    }

    /// §2.3 logout (`HTTPClient.swift:142`): client-side destruction of
    /// cookies, CSRF token and runtime id. NO server-side endpoint is called
    /// (§3.10 grep shows none). Also deletes the persisted cookie.
    pub async fn logout(&self, store: &dyn SecretStore) -> Result<(), AuthError> {
        store.delete(&self.profile).await?;
        self.http.disconnect();
        Ok(())
    }
}
