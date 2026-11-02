//! Schema definitions and forward-only migrations.
//!
//! Version 1 covers the phase-2 persistence needs: server profiles,
//! the single persisted auth cookie per profile (a0-auth §2.3 parity),
//! per-`(profile, chat)` drafts with staged attachment metadata, and a
//! bounded message cache for instant chat-list rendering.

use rusqlite::Connection;

use crate::error::{Result, StorageError};

/// Current schema version. Bump and add a `migrate_v<n>` step when evolving.
pub const SCHEMA_VERSION: i64 = 1;

/// Applies all pending migrations to bring `conn` to [`SCHEMA_VERSION`].
pub fn migrate(conn: &Connection) -> Result<()> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS meta (
            key TEXT PRIMARY KEY,
            value TEXT NOT NULL
        ) WITHOUT ROWID;",
    )?;
    let version: i64 = conn
        .query_row(
            "SELECT value FROM meta WHERE key = 'schema_version'",
            [],
            |row| {
                let text: String = row.get(0)?;
                text.parse::<i64>().map_err(|_| {
                    rusqlite::Error::InvalidColumnType(0, "value".into(), rusqlite::types::Type::Text)
                })
            },
        )
        .or_else(|e| match e {
            rusqlite::Error::QueryReturnedNoRows => Ok(0),
            other => Err(other),
        })?;
    if version > SCHEMA_VERSION {
        return Err(StorageError::InvalidInput(format!(
            "database schema v{version} is newer than supported v{SCHEMA_VERSION}"
        )));
    }
    if version < 1 {
        migrate_v1(conn)?;
    }
    Ok(())
}

fn migrate_v1(conn: &Connection) -> Result<()> {
    conn.execute_batch(
        "BEGIN;
        CREATE TABLE server_profiles (
            id TEXT PRIMARY KEY,
            origin TEXT NOT NULL UNIQUE,
            label TEXT NOT NULL,
            created_at_ms INTEGER NOT NULL,
            last_used_at_ms INTEGER NOT NULL
        );
        CREATE TABLE auth_cookies (
            profile_id TEXT PRIMARY KEY REFERENCES server_profiles(id) ON DELETE CASCADE,
            cookie_name TEXT NOT NULL,
            cookie_value TEXT NOT NULL,
            expires_unix INTEGER,
            updated_at_ms INTEGER NOT NULL
        );
        CREATE TABLE drafts (
            profile_id TEXT NOT NULL REFERENCES server_profiles(id) ON DELETE CASCADE,
            chat_id TEXT NOT NULL,
            text TEXT NOT NULL,
            updated_at_ms INTEGER NOT NULL,
            PRIMARY KEY (profile_id, chat_id)
        );
        CREATE TABLE draft_attachments (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            profile_id TEXT NOT NULL,
            chat_id TEXT NOT NULL,
            local_path TEXT NOT NULL,
            mime TEXT NOT NULL,
            size_bytes INTEGER NOT NULL,
            position INTEGER NOT NULL,
            FOREIGN KEY (profile_id, chat_id)
                REFERENCES drafts(profile_id, chat_id) ON DELETE CASCADE
        );
        CREATE INDEX idx_draft_attachments_chat
            ON draft_attachments(profile_id, chat_id, position);
        CREATE TABLE message_cache (
            profile_id TEXT NOT NULL REFERENCES server_profiles(id) ON DELETE CASCADE,
            chat_id TEXT NOT NULL,
            message_no INTEGER NOT NULL,
            payload_json TEXT NOT NULL,
            cached_at_ms INTEGER NOT NULL,
            PRIMARY KEY (profile_id, chat_id, message_no)
        );
        CREATE TABLE kv (
            key TEXT PRIMARY KEY,
            value TEXT NOT NULL
        ) WITHOUT ROWID;
        INSERT INTO meta(key, value) VALUES ('schema_version', '1');
        COMMIT;",
    )?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fresh_db_migrates_to_v1() {
        let conn = Connection::open_in_memory().unwrap();
        migrate(&conn).unwrap();
        let v: String = conn
            .query_row("SELECT value FROM meta WHERE key='schema_version'", [], |r| r.get(0))
            .unwrap();
        let v: i64 = v.parse().unwrap();
        assert_eq!(v, SCHEMA_VERSION);
        let tables: i64 = conn.query_row(
            "SELECT count(*) FROM sqlite_master WHERE type='table'
             AND name IN ('server_profiles','auth_cookies','drafts',
                          'draft_attachments','message_cache','kv','meta')",
            [],
            |r| r.get(0),
        ).unwrap();
        assert_eq!(tables, 7);
    }

    #[test]
    fn migrate_is_idempotent() {
        let conn = Connection::open_in_memory().unwrap();
        migrate(&conn).unwrap();
        migrate(&conn).unwrap();
    }
}
