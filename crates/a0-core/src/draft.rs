//! Draft composition model for outbound chat messages.
//!
//! A [`Draft`] is a **plain value type**: an owned, `Clone`-able record keyed
//! by `(ServerProfileId, ChatId)` with no coupling to device state, UI
//! lifecycle or orientation. Preserve-on-rotation therefore falls out of the
//! type itself: the draft lives outside any view model, and rotation merely
//! re-renders it (architecture.md sec 2.2 lists the draft model as an
//! a0-core responsibility).
//!
//! Time is injected: `updated_at` is an epoch-millisecond value supplied by
//! the caller, per the documented clock-injection rule
//! (architecture.md sec 2.2, "clock injection for tests").
//!
//! Staging limits are transcribed from protocol inventory sec 7
//! (`Attachments.swift:22,51-52`): max **5** files per message, **10 MiB**
//! per file, **20 MiB** total per message.

use crate::error::CoreError;
use crate::ids::{ChatId, ServerProfileId};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// Maximum number of staged attachments per message
/// (protocol inventory sec 7, `Attachments.swift:22`).
pub const MAX_ATTACHMENTS: usize = 5;

/// Maximum size per staged file: 10 MiB
/// (protocol inventory sec 7, `Attachments.swift:51-52`).
pub const MAX_FILE_BYTES: u64 = 10 * 1024 * 1024;

/// Maximum total staged size per message: 20 MiB
/// (protocol inventory sec 7, `Attachments.swift:51-52`).
pub const MAX_TOTAL_BYTES: u64 = 20 * 1024 * 1024;

/// Identity of a draft: one draft per server profile and chat.
///
/// TODO(doc-gap): the phase-1 inventory defines no persistence format for
/// drafts; the serde encoding here is a client-side default, not wire data.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct DraftKey {
    /// Server profile the draft belongs to.
    pub profile: ServerProfileId,
    /// Chat the draft composes into (`context` on the wire).
    pub chat: ChatId,
}

impl DraftKey {
    /// Creates a draft key for the given profile and chat.
    pub fn new(profile: ServerProfileId, chat: ChatId) -> Self {
        Self { profile, chat }
    }
}

/// A staged attachment: a local file reference with its MIME type and size.
///
/// The file itself is **not** read or uploaded here; upload flows are a
/// transport concern (protocol inventory sec 7 describes the multipart
/// contract implemented by the API layer).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct StagedAttachment {
    /// Local file reference (never accessed by the core).
    pub local_path: PathBuf,
    /// MIME type (e.g. `image/png`).
    pub mime: String,
    /// Declared file size in bytes; validated against [`MAX_FILE_BYTES`].
    pub size_bytes: u64,
}

impl StagedAttachment {
    /// Creates a staged attachment after validating the per-file limit.
    pub fn new(
        local_path: PathBuf,
        mime: impl Into<String>,
        size_bytes: u64,
    ) -> Result<Self, CoreError> {
        if size_bytes > MAX_FILE_BYTES {
            return Err(CoreError::AttachmentLimit(format!(
                "file size {size_bytes} exceeds the per-file limit of {MAX_FILE_BYTES} bytes"
            )));
        }
        Ok(Self {
            local_path,
            mime: mime.into(),
            size_bytes,
        })
    }

    /// Computes the wire `uploadName` for this attachment per protocol
    /// inventory sec 7 (`Attachments.swift:39-45`): `<UUID>-<basename>` with
    /// the basename mapped, truncated and sanitized by
    /// [`StagedAttachment::compose_upload_name`]. The UUID is caller-supplied
    /// (the core performs no RNG).
    ///
    /// TODO(doc-gap): the inventory does not spell out path-separator
    /// handling; the basename is taken after the last `/`.
    pub fn upload_name(&self, uuid: &str) -> String {
        let path = self.local_path.to_string_lossy();
        Self::compose_upload_name(uuid, &path)
    }

    /// Pure `uploadName` composition from a raw path
    /// (protocol inventory sec 7, `Attachments.swift:39-45`).
    pub fn compose_upload_name(uuid: &str, file_name: &str) -> String {
        let basename = file_name.rsplit('/').next().unwrap_or(file_name);
        let mut mapped: String = basename
            .chars()
            .map(|c| {
                if c.is_ascii_alphanumeric() || c == '.' || c == '-' || c == '_' {
                    c
                } else {
                    '_'
                }
            })
            .collect();
        if mapped.chars().count() > 120 {
            mapped = mapped.chars().take(120).collect();
        }
        while mapped.ends_with('.') {
            mapped.pop();
        }
        if mapped.is_empty() {
            mapped = "file".to_string();
        }
        format!("{uuid}-{mapped}")
    }
}

/// An in-progress outbound message, keyed per profile and chat.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct Draft {
    /// Draft identity (profile + chat).
    pub key: DraftKey,
    /// Composed text body.
    pub body: String,
    /// Staged attachments in staging order.
    pub attachments: Vec<StagedAttachment>,
    /// Last modification time in epoch milliseconds (caller-injected).
    pub updated_at: u64,
}

impl Draft {
    /// Creates an empty draft for `key`, stamped with `now_ms`.
    pub fn new(key: DraftKey, now_ms: u64) -> Self {
        Self {
            key,
            body: String::new(),
            attachments: Vec::new(),
            updated_at: now_ms,
        }
    }

    /// Replaces the text body and restamps `updated_at`.
    pub fn set_text(&mut self, body: impl Into<String>, now_ms: u64) {
        self.body = body.into();
        self.updated_at = now_ms;
    }

    /// Stages an attachment, enforcing the documented per-message limits
    /// (protocol inventory sec 7): at most [`MAX_ATTACHMENTS`] files and at
    /// most [`MAX_TOTAL_BYTES`] total. Restamps `updated_at` on success.
    pub fn stage_attachment(
        &mut self,
        attachment: StagedAttachment,
        now_ms: u64,
    ) -> Result<(), CoreError> {
        if self.attachments.len() >= MAX_ATTACHMENTS {
            return Err(CoreError::AttachmentLimit(format!(
                "cannot stage more than {MAX_ATTACHMENTS} attachments"
            )));
        }
        let projected = self.total_staged_bytes() + attachment.size_bytes;
        if projected > MAX_TOTAL_BYTES {
            return Err(CoreError::AttachmentLimit(format!(
                "staged total {projected} exceeds the per-message limit of {MAX_TOTAL_BYTES} bytes"
            )));
        }
        self.attachments.push(attachment);
        self.updated_at = now_ms;
        Ok(())
    }

    /// Removes the attachment at `index`, restamping `updated_at`.
    pub fn remove_attachment(
        &mut self,
        index: usize,
        now_ms: u64,
    ) -> Result<StagedAttachment, CoreError> {
        if index >= self.attachments.len() {
            return Err(CoreError::AttachmentLimit(format!(
                "attachment index {index} out of range"
            )));
        }
        let removed = self.attachments.remove(index);
        self.updated_at = now_ms;
        Ok(removed)
    }

    /// Sum of staged file sizes in bytes.
    pub fn total_staged_bytes(&self) -> u64 {
        self.attachments.iter().map(|a| a.size_bytes).sum()
    }

    /// `true` when the draft carries neither text nor attachments.
    pub fn is_empty(&self) -> bool {
        self.body.is_empty() && self.attachments.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key() -> DraftKey {
        DraftKey::new(ServerProfileId::new("profile-1"), ChatId::new("ctx-1"))
    }

    fn attachment(name: &str, size: u64) -> StagedAttachment {
        StagedAttachment::new(PathBuf::from(name), "image/png", size).unwrap()
    }

    #[test]
    fn draft_is_a_plain_value_that_survives_rotation() {
        let original = Draft::new(key(), 1_000);
        let rotated_view = original.clone();
        drop(original);
        // After the "rotation" (original dropped, view kept as a plain
        // value), the composed content is intact: no UI/device coupling.
        assert_eq!(rotated_view.body, "");
        assert!(rotated_view.attachments.is_empty());
        assert_eq!(rotated_view.updated_at, 1_000);
        assert_eq!(rotated_view.key, key());
        assert!(rotated_view.is_empty());
    }

    #[test]
    fn text_edits_restamp_updated_at_with_injected_clock() {
        let mut draft = Draft::new(key(), 1_000);
        draft.set_text("hello", 1_500);
        assert_eq!(draft.body, "hello");
        assert_eq!(draft.updated_at, 1_500);
        assert!(!draft.is_empty());
    }

    #[test]
    fn per_file_limit_is_enforced_at_construction() {
        let err = StagedAttachment::new(
            PathBuf::from("/tmp/big.png"),
            "image/png",
            MAX_FILE_BYTES + 1,
        )
        .unwrap_err();
        assert!(matches!(err, CoreError::AttachmentLimit(_)));
        // Exactly at the limit is legal.
        assert!(
            StagedAttachment::new(PathBuf::from("/tmp/ok.png"), "image/png", MAX_FILE_BYTES)
                .is_ok()
        );
    }

    #[test]
    fn staging_enforces_the_documented_count_limit() {
        let mut draft = Draft::new(key(), 0);
        for index in 0..MAX_ATTACHMENTS {
            draft
                .stage_attachment(
                    attachment(&format!("/tmp/f{index}.png"), 1),
                    1_000 + index as u64,
                )
                .expect("staging up to the limit must succeed");
        }
        assert_eq!(draft.attachments.len(), MAX_ATTACHMENTS);
        let err = draft
            .stage_attachment(attachment("/tmp/f6.png", 1), 9_999)
            .unwrap_err();
        assert!(matches!(err, CoreError::AttachmentLimit(_)));
    }

    #[test]
    fn staging_enforces_the_total_size_limit() {
        let mut draft = Draft::new(key(), 0);
        // Two files at exactly the per-file limit are legal together
        // (10 MiB + 10 MiB == the 20 MiB per-message total).
        draft
            .stage_attachment(attachment("/tmp/a.png", MAX_FILE_BYTES), 1)
            .unwrap();
        draft
            .stage_attachment(attachment("/tmp/b.png", MAX_FILE_BYTES), 2)
            .unwrap();
        // Any further file breaches the 20 MiB total from here.
        let err = draft
            .stage_attachment(attachment("/tmp/c.png", 1), 3)
            .unwrap_err();
        assert!(matches!(err, CoreError::AttachmentLimit(_)));
        assert_eq!(
            draft.attachments.len(),
            2,
            "rejected file must not be staged"
        );
        assert_eq!(draft.updated_at, 2, "failed staging must not restamp");
    }

    #[test]
    fn remove_attachment_updates_staging_and_timestamp() {
        let mut draft = Draft::new(key(), 0);
        draft
            .stage_attachment(attachment("/tmp/a.png", 10), 1)
            .unwrap();
        draft
            .stage_attachment(attachment("/tmp/b.png", 20), 2)
            .unwrap();
        let removed = draft.remove_attachment(0, 3).unwrap();
        assert_eq!(removed.local_path, PathBuf::from("/tmp/a.png"));
        assert_eq!(draft.attachments.len(), 1);
        assert_eq!(draft.total_staged_bytes(), 20);
        assert_eq!(draft.updated_at, 3);
        assert!(draft.remove_attachment(5, 4).is_err());
    }

    #[test]
    fn upload_name_follows_the_documented_algorithm() {
        // Charset mapping: every char outside [A-Za-z0-9._-] becomes '_'.
        assert_eq!(
            StagedAttachment::compose_upload_name("uuid-1", "my photo+(1).txt"),
            "uuid-1-my_photo__1_.txt"
        );
        // Truncation to 120 characters of the MAPPED basename: the 130 a's
        // consume the budget, so the ".png" extension is cut. Truncation
        // happens before trailing-dot stripping and never preserves an
        // extension (Attachments.swift:39-45).
        let long = format!("{}.png", "a".repeat(130));
        let name = StagedAttachment::compose_upload_name("u", &long);
        let base = name.strip_prefix("u-").unwrap();
        assert_eq!(base.chars().count(), 120);
        assert_eq!(base, "a".repeat(120));
        // Trailing dots only are stripped.
        assert_eq!(
            StagedAttachment::compose_upload_name("u", "report.v2.."),
            "u-report.v2"
        );
        // Empty basename falls back to "file".
        assert_eq!(StagedAttachment::compose_upload_name("u", ""), "u-file");
        assert_eq!(StagedAttachment::compose_upload_name("u", "dir/"), "u-file");
        // Basename is taken after the last separator.
        assert_eq!(
            StagedAttachment::compose_upload_name("u", "/tmp/x/report.pdf"),
            "u-report.pdf"
        );
    }

    #[test]
    fn draft_serializes_round_trip() {
        let mut draft = Draft::new(key(), 42);
        draft.set_text("body", 43);
        draft
            .stage_attachment(attachment("/tmp/a.png", 7), 44)
            .unwrap();
        let json = serde_json::to_string(&draft).unwrap();
        let back: Draft = serde_json::from_str(&json).unwrap();
        assert_eq!(back, draft);
    }
}
