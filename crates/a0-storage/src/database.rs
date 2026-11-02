//! SQLCipher-backed application database.
//!
//! One encrypted file per server profile. The key is supplied by a
//! [`KeyProvider`] and applied with `PRAGMA key` as the very first statement
//! on the connection; a wrong key surfaces at first use as
//! [`StorageError::Sqlite`] (`file is not a database` from SQLCipher).

use std::path::{Path, PathBuf};
use std::sync::Mutex;

use rusqlite::Connection;
use zeroize::Zeroizing;

use a0_core::draft::StagedAttachment;
use a0_core::{ChatId, Draft, ServerProfileId};

use crate::error::{Result, StorageError};
#[cfg(feature = "sqlcipher")]
use crate::key::sqlcipher_key_literal;
use crate::key::KeyProvider;
use crate::schema;

/// Encrypted per-profile database.
pub struct Database {
    conn: Mutex<Connection>,
    path: PathBuf,
}

impl Database {
    /// Opens (creating if needed) the encrypted database at `path` and runs
    /// pending migrations.
    pub fn open(path: impl AsRef<Path>, key_provider: &dyn KeyProvider, profile: &ServerProfileId) -> Result<Self> {
        let key = key_provider.database_key(profile)?;
        let conn = Self::open_connection(path.as_ref(), &key)?;
        schema::migrate(&conn)?;
        Ok(Self {
            conn: Mutex::new(conn),
            path: path.as_ref().to_path_buf(),
        })
    }

    /// In-memory database with the same key/migration semantics as
    /// [`Database::open`]: tests, previews and embedders that keep data
    /// process-local.
    pub fn open_in_memory(key_provider: &dyn KeyProvider, profile: &ServerProfileId) -> Result<Self> {
        let key = key_provider.database_key(profile)?;
        let conn = Self::apply_key(Connection::open_in_memory()?, &key)?;
        schema::migrate(&conn)?;
        Ok(Self {
            conn: Mutex::new(conn),
            path: PathBuf::from(":memory:"),
        })
    }

    fn open_connection(path: &Path, key: &Zeroizing<Vec<u8>>) -> Result<Connection> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let conn = Connection::open(path)?;
        Self::apply_key(conn, key)
    }

    #[cfg(feature = "sqlcipher")]
    fn apply_key(conn: Connection, key: &Zeroizing<Vec<u8>>) -> Result<Connection> {
        // PRAGMA key must be the first statement and the x'' literal must be
        // parsed by SQLCipher itself (a bound parameter would be read as a
        // passphrase string instead of raw key bytes). The literal contains
        // hex characters only, so statement composition is injection-safe;
        // it stays in a zeroized buffer and is never logged.
        let literal = sqlcipher_key_literal(key);
        conn.execute_batch(&format!("PRAGMA key = {};", literal.as_str()))?;
        Ok(conn)
    }

    #[cfg(not(feature = "sqlcipher"))]
    fn apply_key(conn: Connection, _key: &Zeroizing<Vec<u8>>) -> Result<Connection> {
        // Without the sqlcipher feature the file is a plain SQLite database;
        // at-rest protection is delegated to the OS (iOS NSFileProtection,
        // Android file-based encryption + Keystore). See lib.rs notes.
        Ok(conn)
    }

    /// File path backing this database.
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Persists a draft (upsert).
    pub fn save_draft(&self, profile: &ServerProfileId, chat: &ChatId, draft: &Draft) -> Result<()> {
        let conn = self.conn.lock().expect("database mutex");
        conn.execute(
            "INSERT INTO drafts(profile_id, chat_id, text, updated_at_ms)
             VALUES (?1, ?2, ?3, ?4)
             ON CONFLICT(profile_id, chat_id)
             DO UPDATE SET text = excluded.text, updated_at_ms = excluded.updated_at_ms",
            rusqlite::params![profile.as_str(), chat.as_str(), draft.body, draft.updated_at],
        )?;
        conn.execute(
            "DELETE FROM draft_attachments WHERE profile_id = ?1 AND chat_id = ?2",
            rusqlite::params![profile.as_str(), chat.as_str()],
        )?;
        for (position, att) in draft.attachments.iter().enumerate() {
            conn.execute(
                "INSERT INTO draft_attachments
                     (profile_id, chat_id, local_path, mime, size_bytes, position)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                rusqlite::params![
                    profile.as_str(),
                    chat.as_str(),
                    att.local_path.display().to_string(),
                    att.mime,
                    att.size_bytes,
                    position as i64,
                ],
            )?;
        }
        Ok(())
    }

    /// Loads a draft row if present; returns `None` for an empty draft slot.
    pub fn load_draft_text(
        &self,
        profile: &ServerProfileId,
        chat: &ChatId,
    ) -> Result<Option<(String, u64)>> {
        let conn = self.conn.lock().expect("database mutex");
        let mut stmt = conn.prepare(
            "SELECT text, updated_at_ms FROM drafts
             WHERE profile_id = ?1 AND chat_id = ?2",
        )?;
        let mut rows = stmt.query(rusqlite::params![profile.as_str(), chat.as_str()])?;
        match rows.next()? {
            Some(row) => Ok(Some((row.get(0)?, row.get(1)?))),
            None => Ok(None),
        }
    }

    /// Lists staged attachments of one draft, ordered by position.
    pub fn load_draft_attachments(
        &self,
        profile: &ServerProfileId,
        chat: &ChatId,
    ) -> Result<Vec<StagedAttachment>> {
        let conn = self.conn.lock().expect("database mutex");
        let mut stmt = conn.prepare(
            "SELECT local_path, mime, size_bytes FROM draft_attachments
             WHERE profile_id = ?1 AND chat_id = ?2 ORDER BY position",
        )?;
        let rows = stmt.query_map(
            rusqlite::params![profile.as_str(), chat.as_str()],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, i64>(2)?,
                ))
            },
        )?;
        let mut out = Vec::new();
        for row in rows {
            let (path, mime, size): (String, String, i64) = row?;
            out.push(
                StagedAttachment::new(PathBuf::from(path), mime, size as u64)
                    .map_err(StorageError::from)?,
            );
        }
        Ok(out)
    }

    /// Deletes a draft and its attachments (sent or cleared composer).
    pub fn delete_draft(&self, profile: &ServerProfileId, chat: &ChatId) -> Result<()> {
        let conn = self.conn.lock().expect("database mutex");
        conn.execute(
            "DELETE FROM drafts WHERE profile_id = ?1 AND chat_id = ?2",
            rusqlite::params![profile.as_str(), chat.as_str()],
        )?;
        Ok(())
    }

    /// Caches one message payload (JSON text) for instant list rendering.
    pub fn cache_message(
        &self,
        profile: &ServerProfileId,
        chat: &ChatId,
        message_no: i64,
        payload_json: &str,
        now_ms: u64,
    ) -> Result<()> {
        let conn = self.conn.lock().expect("database mutex");
        conn.execute(
            "INSERT INTO message_cache(profile_id, chat_id, message_no, payload_json, cached_at_ms)
             VALUES (?1, ?2, ?3, ?4, ?5)
             ON CONFLICT(profile_id, chat_id, message_no)
             DO UPDATE SET payload_json = excluded.payload_json,
                           cached_at_ms = excluded.cached_at_ms",
            rusqlite::params![profile.as_str(), chat.as_str(), message_no, payload_json, now_ms],
        )?;
        Ok(())
    }

    /// Loads cached messages for one chat ordered by message number.
    pub fn load_cached_messages(
        &self,
        profile: &ServerProfileId,
        chat: &ChatId,
    ) -> Result<Vec<(i64, String)>> {
        let conn = self.conn.lock().expect("database mutex");
        let mut stmt = conn.prepare(
            "SELECT message_no, payload_json FROM message_cache
             WHERE profile_id = ?1 AND chat_id = ?2 ORDER BY message_no",
        )?;
        let rows = stmt.query_map(
            rusqlite::params![profile.as_str(), chat.as_str()],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )?;
        let mut out = Vec::new();
        for row in rows {
            out.push(row?);
        }
        Ok(out)
    }

    /// Registers a server profile (upsert by id).
    pub fn upsert_profile(
        &self,
        profile: &ServerProfileId,
        origin: &str,
        label: &str,
        now_ms: u64,
    ) -> Result<()> {
        if origin.is_empty() {
            return Err(StorageError::InvalidInput("origin is empty".into()));
        }
        let conn = self.conn.lock().expect("database mutex");
        conn.execute(
            "INSERT INTO server_profiles(id, origin, label, created_at_ms, last_used_at_ms)
             VALUES (?1, ?2, ?3, ?4, ?4)
             ON CONFLICT(id) DO UPDATE SET origin = excluded.origin,
                 label = excluded.label, last_used_at_ms = excluded.last_used_at_ms",
            rusqlite::params![profile.as_str(), origin, label, now_ms],
        )?;
        Ok(())
    }

    /// Lists all profiles ordered by most recent use.
    pub fn list_profiles(&self) -> Result<Vec<(String, String, String)>> {
        let conn = self.conn.lock().expect("database mutex");
        let mut stmt = conn.prepare(
            "SELECT id, origin, label FROM server_profiles ORDER BY last_used_at_ms DESC",
        )?;
        let rows = stmt.query_map([], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)))?;
        let mut out = Vec::new();
        for row in rows {
            out.push(row?);
        }
        Ok(out)
    }

    /// Removes a profile and every dependent row (explicit account removal).
    pub fn delete_profile(&self, profile: &ServerProfileId) -> Result<()> {
        let conn = self.conn.lock().expect("database mutex");
        conn.execute("DELETE FROM server_profiles WHERE id = ?1", [profile.as_str()])?;
        Ok(())
    }
}
