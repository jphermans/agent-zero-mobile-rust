//! Integration tests: encrypted persistence behavior.

use std::path::PathBuf;

use a0_core::draft::StagedAttachment;
use a0_core::{ChatId, Draft, DraftKey, ServerProfileId};
use a0_storage::{Database, StaticKeyProvider};

fn profile() -> ServerProfileId {
    ServerProfileId::new("profile-test")
}

#[test]
fn draft_roundtrip_with_attachments() {
    let db = Database::open_in_memory(&StaticKeyProvider::new(vec![7u8; 32]), &profile()).unwrap();
    db.upsert_profile(&profile(), "https://test.example", "t", 900)
        .unwrap();
    let chat = ChatId::new("ctx-1");
    let mut draft = Draft::new(DraftKey::new(profile(), chat.clone()), 1_000);
    draft.set_text("hello composer", 1_100);
    draft
        .stage_attachment(
            StagedAttachment::new(PathBuf::from("/tmp/x.png"), "image/png", 120).unwrap(),
            1_200,
        )
        .unwrap();

    db.save_draft(&profile(), &chat, &draft).unwrap();
    let (text, updated) = db.load_draft_text(&profile(), &chat).unwrap().unwrap();
    assert_eq!(text, "hello composer");
    // Staging the attachment at 1_200 restamped the draft (a0-core rule),
    // so the persisted updated_at must be 1_200, not the set_text stamp.
    assert_eq!(updated, 1_200);

    let attachments = db.load_draft_attachments(&profile(), &chat).unwrap();
    assert_eq!(attachments.len(), 1);
    assert_eq!(attachments[0].size_bytes, 120);

    db.delete_draft(&profile(), &chat).unwrap();
    assert!(db.load_draft_text(&profile(), &chat).unwrap().is_none());
    assert!(db
        .load_draft_attachments(&profile(), &chat)
        .unwrap()
        .is_empty());
}

#[test]
fn profiles_roundtrip() {
    let db = Database::open_in_memory(&StaticKeyProvider::new(vec![7u8; 32]), &profile()).unwrap();
    db.upsert_profile(&profile(), "https://a0.example.com", "Home", 1_000)
        .unwrap();
    db.upsert_profile(
        &ServerProfileId::new("p2"),
        "https://b.example.com",
        "Work",
        2_000,
    )
    .unwrap();
    assert_eq!(db.list_profiles().unwrap().len(), 2);
    db.delete_profile(&profile()).unwrap();
    assert_eq!(db.list_profiles().unwrap().len(), 1);
}

#[test]
fn message_cache_roundtrip_and_order() {
    let db = Database::open_in_memory(&StaticKeyProvider::new(vec![7u8; 32]), &profile()).unwrap();
    db.upsert_profile(&profile(), "https://test.example", "t", 900)
        .unwrap();
    let chat = ChatId::new("ctx-2");
    let payload3 = r#"{"no":3}"#;
    let payload1 = r#"{"no":1}"#;
    let payload3b = r#"{"no":3-updated}"#;
    db.cache_message(&profile(), &chat, 3, payload3, 1_000)
        .unwrap();
    db.cache_message(&profile(), &chat, 1, payload1, 1_000)
        .unwrap();
    db.cache_message(&profile(), &chat, 3, payload3b, 2_000)
        .unwrap();
    let rows = db.load_cached_messages(&profile(), &chat).unwrap();
    let nos: Vec<i64> = rows.iter().map(|(no, _)| *no).collect();
    assert_eq!(nos, vec![1, 3]);
    assert_eq!(rows[1].1, payload3b);
}

#[cfg(feature = "sqlcipher")]
#[test]
fn wrong_key_cannot_open_existing_file() {
    let dir = std::env::temp_dir().join(format!("a0-storage-test-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("profile.db");

    {
        let db = Database::open(&path, &StaticKeyProvider::new(vec![1u8; 32]), &profile()).unwrap();
        db.upsert_profile(&profile(), "https://x.example", "t", 1)
            .unwrap();
    }

    // Right key: opens and reads.
    let db = Database::open(&path, &StaticKeyProvider::new(vec![1u8; 32]), &profile());
    assert!(db.is_ok());

    // Wrong key: SQLCipher must reject (file is not a database).
    let wrong = Database::open(&path, &StaticKeyProvider::new(vec![2u8; 32]), &profile());
    assert!(wrong.is_err(), "wrong key must not open the database");

    std::fs::remove_dir_all(&dir).ok();
}

#[cfg(feature = "sqlcipher")]
#[test]
fn file_at_rest_is_ciphertext() {
    let dir = std::env::temp_dir().join(format!("a0-storage-plain-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("cipher.db");

    let db = Database::open(&path, &StaticKeyProvider::new(vec![9u8; 32]), &profile()).unwrap();
    db.upsert_profile(&profile(), "https://secret.example", "s", 1)
        .unwrap();
    drop(db);

    let bytes = std::fs::read(&path).unwrap();
    // Not an SQLite/SQLCipher plaintext header ("SQLite format 3\0") and the
    // marker string from our row must not appear in the clear.
    let header_ok = !bytes.starts_with(b"SQLite format 3");
    let content_ok = !bytes
        .windows("secret.example".len())
        .any(|w| w == b"secret.example");
    assert!(header_ok, "database file must not be plaintext sqlite");
    assert!(content_ok, "row content must not appear in the clear");

    std::fs::remove_dir_all(&dir).ok();
}
