//! Database encryption-key providers.
//!
//! SQLCipher needs a per-database key. The mobile hosts derive it in the
//! platform keystore (iOS Keychain / Android Keystore via a0-platform
//! bridges); this crate only consumes key bytes through [`KeyProvider`].
//! Key material is held in [`zeroize::Zeroizing`] and **never** rendered by
//! `Debug` implementations or `tracing` calls (architecture §5.5).

use zeroize::Zeroizing;

use a0_core::ServerProfileId;

use crate::error::{Result, StorageError};

/// Supplies the SQLCipher key for one profile database.
pub trait KeyProvider: Send + Sync {
    /// Returns raw key bytes (32 bytes recommended; any length accepted by
    /// SQLCipher's PBKDF2 path works). Implementations must not log them.
    fn database_key(&self, profile: &ServerProfileId) -> Result<Zeroizing<Vec<u8>>>;
}

/// Deterministic provider wrapping one fixed key: tests, and hosts that
/// already resolved the key before constructing the storage layer.
pub struct StaticKeyProvider {
    key: Zeroizing<Vec<u8>>,
}

impl StaticKeyProvider {
    /// Wraps `key` bytes.
    pub fn new(key: Vec<u8>) -> Self {
        Self {
            key: Zeroizing::new(key),
        }
    }
}

// Manual Debug: the key must never render, even in accidental log macros.
impl std::fmt::Debug for StaticKeyProvider {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("StaticKeyProvider").finish_non_exhaustive()
    }
}

impl KeyProvider for StaticKeyProvider {
    fn database_key(&self, _profile: &ServerProfileId) -> Result<Zeroizing<Vec<u8>>> {
        Ok(Zeroizing::new(self.key.to_vec()))
    }
}

/// Provider that always fails: documents a keystore miss (locked device,
/// wiped keychain) without leaking whether the profile exists.
#[derive(Debug, Clone, Copy, Default)]
pub struct UnavailableKeyProvider;

impl KeyProvider for UnavailableKeyProvider {
    fn database_key(&self, _profile: &ServerProfileId) -> Result<Zeroizing<Vec<u8>>> {
        Err(StorageError::Key("keystore unavailable".into()))
    }
}

/// Formats raw key bytes into SQLCipher's raw-key literal: `x'<hex>'`.
/// Returns an owned zeroized buffer holding the literal (still secret).
pub fn sqlcipher_key_literal(key: &[u8]) -> Zeroizing<String> {
    let mut hex = String::with_capacity(key.len() * 2 + 3);
    hex.push_str("x'");
    for byte in key.iter() {
        use std::fmt::Write;
        let _ = write!(hex, "{byte:02x}");
    }
    hex.push('\'');
    Zeroizing::new(hex)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn static_provider_roundtrips_and_debug_redacts() {
        let provider = StaticKeyProvider::new(vec![0xABu8; 32]);
        let key = provider.database_key(&ServerProfileId::new("p1")).unwrap();
        assert_eq!(key.len(), 32);
        let rendered = format!("{provider:?}");
        assert!(!rendered.contains("abab"));
    }

    #[test]
    fn unavailable_provider_errors() {
        let provider = UnavailableKeyProvider;
        assert!(provider.database_key(&ServerProfileId::new("p")).is_err());
    }

    #[test]
    fn literal_format_is_sqlcipher_raw_key() {
        let literal = sqlcipher_key_literal(&[0x01, 0x02, 0xFF]);
        assert_eq!(literal.as_str(), "x'0102ff'");
    }
}
