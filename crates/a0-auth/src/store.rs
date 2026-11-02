//! Secret storage — architecture §5.3 (`SecretStore` trait outcome) carrying
//! the ONLY persistable auth material of §2.3 (`SavedAuthentication.swift:5-21`):
//! exactly one cookie `{version:1, profile, name, value, expires}` per
//! profile. Passwords and CSRF tokens are deliberately NOT persistable
//! (`SavedAuthentication.swift:3-4`) — the store API simply has no type for
//! them.

use async_trait::async_trait;
use zeroize::{Zeroize, Zeroizing};

use crate::error::AuthError;

/// Stable identifier of one server profile (opaque string key on the wire).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ServerProfileId(pub String);

impl std::fmt::Display for ServerProfileId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

/// §2.3 persisted material (`SavedAuthentication.swift:5-13`):
/// `{version:1, profile, name, value, expires}`.
///
/// Validation (`SavedAuthentication.swift:14-21`):
/// * `name` starts with `session_`, max 256 chars, charset `[A-Za-z0-9_-]`.
/// * `value` non-empty, max 16384 bytes, printable ASCII excluding `;` and `,`.
/// * not expired (`expires` = unix seconds; `None` = session cookie).
#[derive(Clone)]
pub struct SavedCookie {
    pub version: u32,
    pub profile: ServerProfileId,
    pub name: String,
    /// Zeroized on drop (architecture §5.3).
    pub value: Zeroizing<String>,
    /// Unix-epoch seconds; `None` = session cookie.
    pub expires: Option<u64>,
}

impl std::fmt::Debug for SavedCookie {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // Value NEVER rendered (§5.5 log redaction).
        f.debug_struct("SavedCookie")
            .field("version", &self.version)
            .field("profile", &self.profile.0)
            .field("name", &self.name)
            .field("value", &"«redacted»")
            .field("expires", &self.expires)
            .finish()
    }
}

impl SavedCookie {
    pub fn new(
        profile: ServerProfileId,
        name: impl Into<String>,
        value: impl Into<String>,
        expires: Option<u64>,
    ) -> Result<SavedCookie, AuthError> {
        let name = name.into();
        let value = Zeroizing::new(value.into());
        Self::validate(&name, &value, expires)?;
        Ok(SavedCookie {
            version: 1,
            profile,
            name,
            value,
            expires,
        })
    }

    /// §2.3 validation rules (`SavedAuthentication.swift:14-21`).
    pub fn validate(name: &str, value: &str, expires: Option<u64>) -> Result<(), AuthError> {
        let name_valid = name.starts_with("session_")
            && name.len() <= 256
            && name.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-');
        if !name_valid {
            return Err(AuthError::IncompatiblePayload("invalidSavedCookieName".to_string()));
        }
        let value_valid = !value.is_empty()
            && value.len() <= 16_384
            && value.bytes().all(|b| (0x21..=0x7e).contains(&b) && b != b';' && b != b',');
        if !value_valid {
            return Err(AuthError::IncompatiblePayload("invalidSavedCookieValue".to_string()));
        }
        // A record that is already expired at store time is invalid to keep
        // as restorable material; expiry is re-checked on restore.
        if let Some(exp) = expires {
            if let Some(now) = now_unix() {
                if exp <= now {
                    return Err(AuthError::IncompatiblePayload("expiredSavedCookie".to_string()));
                }
            }
        }
        Ok(())
    }

    /// §2.3: "not expired" — session cookies (`expires: None`) never expire.
    pub fn is_expired(&self, now_unix: u64) -> bool {
        self.expires.map(|exp| exp <= now_unix).unwrap_or(false)
    }

    /// Serialize to the §2.3 JSON record (for platform stores persisting blobs).
    pub fn to_json(&self) -> String {
        serde_json::json!({
            "version": self.version,
            "profile": self.profile.0,
            "name": self.name,
            "value": self.value.as_str(),
            "expires": self.expires,
        })
        .to_string()
    }

    /// Parse and validate a §2.3 JSON record.
    pub fn from_json(text: &str) -> Result<SavedCookie, AuthError> {
        let v: serde_json::Value = serde_json::from_str(text)
            .map_err(|e| AuthError::IncompatiblePayload(format!("savedCookieJson: {e}")))?;
        let version = v.get("version").and_then(serde_json::Value::as_u64).unwrap_or(0) as u32;
        if version != 1 {
            return Err(AuthError::IncompatiblePayload("unsupportedSavedCookieVersion".to_string()));
        }
        let profile = ServerProfileId(
            v.get("profile")
                .and_then(serde_json::Value::as_str)
                .ok_or_else(|| AuthError::IncompatiblePayload("missingProfile".to_string()))?
                .to_string(),
        );
        let name = v
            .get("name")
            .and_then(serde_json::Value::as_str)
            .ok_or_else(|| AuthError::IncompatiblePayload("missingName".to_string()))?
            .to_string();
        let value = v
            .get("value")
            .and_then(serde_json::Value::as_str)
            .ok_or_else(|| AuthError::IncompatiblePayload("missingValue".to_string()))?;
        let expires = v.get("expires").and_then(serde_json::Value::as_u64);
        SavedCookie::new(profile, name, value, expires)
    }
}

impl Drop for SavedCookie {
    fn drop(&mut self) {
        // Zeroizing<String> wipes the value; wipe the name buffer too so the
        // whole struct tears down uniformly.
        self.name.zeroize();
    }
}

pub(crate) fn now_unix() -> Option<u64> {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .ok()
        .map(|d| d.as_secs())
}

/// Async secret store keyed by [`ServerProfileId`] — architecture §5.3.
/// Exactly ONE cookie slot per profile (§2.3: persisted material is exactly
/// one cookie; a new `put` replaces the previous record). Platform impls
/// (iOS Keychain wrapper, Android Keystore bridge) implement this trait.
#[async_trait]
pub trait SecretStore: Send + Sync {
    /// Load the saved cookie for a profile; `Ok(None)` when absent.
    async fn get(&self, profile: &ServerProfileId) -> Result<Option<SavedCookie>, AuthError>;

    /// Store (or replace) the saved cookie for a profile. Implementations
    /// MUST store only this one record per profile (§2.3).
    async fn put(&self, cookie: SavedCookie) -> Result<(), AuthError>;

    /// Remove the saved cookie (logout §2.3). Absent profile → `Ok(())`.
    async fn delete(&self, profile: &ServerProfileId) -> Result<(), AuthError>;
}

/// In-process reference implementation of [`SecretStore`] (tests and
/// non-persistent hosts). Values are held in [`Zeroizing`] buffers.
#[derive(Default)]
pub struct InMemorySecretStore {
    slots: tokio::sync::Mutex<std::collections::HashMap<ServerProfileId, SavedCookie>>,
}

impl InMemorySecretStore {
    pub fn new() -> Self {
        Self::default()
    }
}

#[async_trait]
impl SecretStore for InMemorySecretStore {
    async fn get(&self, profile: &ServerProfileId) -> Result<Option<SavedCookie>, AuthError> {
        Ok(self.slots.lock().await.get(profile).cloned())
    }

    async fn put(&self, cookie: SavedCookie) -> Result<(), AuthError> {
        self.slots.lock().await.insert(cookie.profile.clone(), cookie);
        Ok(())
    }

    async fn delete(&self, profile: &ServerProfileId) -> Result<(), AuthError> {
        if let Some(mut cookie) = self.slots.lock().await.remove(profile) {
            cookie.value.zeroize();
        }
        Ok(())
    }
}
