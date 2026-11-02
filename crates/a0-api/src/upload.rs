//! Uploads & attachments — inventory §7 (`Attachments.swift:22-99`),
//! multipart (NOT base64):
//!
//! * `Content-Type: multipart/form-data; boundary=A0-<UUID>`
//!   (`Attachments.swift:54-55`).
//! * Text parts: `Content-Disposition: form-data; name="<key>"` followed by
//!   the raw value; allowed names `text`, `context`, `message_id`
//!   (`Attachments.swift:53,58-60`).
//! * File parts: `Content-Disposition: form-data; name="<fileField>";
//!   filename="<uploadName>"` plus `Content-Type: <mime>`, then raw bytes;
//!   `fileField` ∈ {`file`, `attachments`} (`Attachments.swift:53,61-64`).
//! * Limits: max 5 files / 10 MiB per file / 20 MiB total
//!   (`Attachments.swift:22,51-52`).
//! * `uploadName` = `<UUID>-<basename>`; basename chars outside ASCII
//!   letters/digits/`.-_` map to `_`, truncated to 120 chars, trailing dots
//!   stripped, empty → `file` (`Attachments.swift:39-45`).
//!
//! Two flows (§7 `Attachments.swift:71-92`):
//! 1. **Queued:** `POST /api/upload` (file parts named `file`, no text
//!    fields) → `{"filenames":[…]}` must equal the client-computed upload
//!    names IN ORDER; then `POST /api/message_queue_add` JSON with the
//!    `attachments` array.
//! 2. **Immediate:** single `POST /api/message_async` multipart with text
//!    fields `context`, `text`, `message_id` and file parts named
//!    `attachments` → `{"context":"<ctx>"}`.
//!
//! Both flows run the standard 401/403 handling (`Attachments.swift:93-99`)
//! via the shared client; both are mutations → never retried (§6).

use serde_json::{json, Value};

use crate::client::{A0Client, Body, RequestSpec};
use crate::error::ApiError;

pub(crate) const MAX_FILES: usize = 5;
pub(crate) const MAX_FILE_BYTES: usize = 10 * 1024 * 1024;
pub(crate) const MAX_TOTAL_BYTES: usize = 20 * 1024 * 1024;

/// One attachment file to upload.
#[derive(Debug, Clone)]
pub struct UploadFile {
    pub bytes: Vec<u8>,
    pub mime: String,
    /// Original basename; the wire `filename` is computed per §7.
    pub basename: String,
}

/// §7 uploadName sanitizer: sanitize → truncate to 120 → strip trailing dots
/// → empty → `file`; prefixed with `<UUID>-`.
pub fn upload_name(basename: &str, uuid: &str) -> String {
    let mut sanitized: String = basename
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '.' || c == '-' || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect();
    sanitized.truncate(120);
    while sanitized.ends_with('.') {
        sanitized.pop();
    }
    if sanitized.is_empty() {
        sanitized = "file".to_string();
    }
    format!("{uuid}-{sanitized}")
}

/// Hand-built multipart body with the exact §7 wire format and the
/// `A0-<UUID>` boundary.
struct MultipartBody {
    boundary: String,
    body: Vec<u8>,
    total_file_bytes: usize,
}

impl MultipartBody {
    fn new(boundary: String) -> Self {
        Self {
            boundary,
            body: Vec::new(),
            total_file_bytes: 0,
        }
    }

    fn text(&mut self, name: &str, value: &str) {
        debug_assert!(matches!(name, "text" | "context" | "message_id"));
        self.body
            .extend_from_slice(format!("--{}\r\n", self.boundary).as_bytes());
        self.body.extend_from_slice(
            format!("Content-Disposition: form-data; name=\"{name}\"\r\n\r\n").as_bytes(),
        );
        self.body.extend_from_slice(value.as_bytes());
        self.body.extend_from_slice(b"\r\n");
    }

    fn file(&mut self, field: &str, name: &str, file: &UploadFile) -> Result<(), ApiError> {
        debug_assert!(matches!(field, "file" | "attachments"));
        if file.bytes.len() > MAX_FILE_BYTES {
            return Err(ApiError::Protocol("attachmentTooLarge".to_string()));
        }
        self.total_file_bytes += file.bytes.len();
        if self.total_file_bytes > MAX_TOTAL_BYTES {
            return Err(ApiError::Protocol("attachmentTotalTooLarge".to_string()));
        }
        self.body
            .extend_from_slice(format!("--{}\r\n", self.boundary).as_bytes());
        self.body.extend_from_slice(
            format!("Content-Disposition: form-data; name=\"{field}\"; filename=\"{name}\"\r\n")
                .as_bytes(),
        );
        self.body
            .extend_from_slice(format!("Content-Type: {}\r\n\r\n", file.mime).as_bytes());
        self.body.extend_from_slice(&file.bytes);
        self.body.extend_from_slice(b"\r\n");
        Ok(())
    }

    fn finish(mut self) -> (Vec<u8>, String) {
        self.body
            .extend_from_slice(format!("--{}--\r\n", self.boundary).as_bytes());
        let content_type = format!("multipart/form-data; boundary={}", self.boundary);
        (self.body, content_type)
    }
}

fn check_count(files: &[UploadFile]) -> Result<(), ApiError> {
    if files.is_empty() || files.len() > MAX_FILES {
        return Err(ApiError::Protocol("attachmentCount".to_string()));
    }
    Ok(())
}

/// Flow 1a (§7, `Attachments.swift:74-78`): `POST /api/upload` with file
/// parts named `file` and NO text fields. The response must decode
/// `{"filenames":[…]}` equal to the client-computed upload names, in order.
pub async fn upload_files(
    client: &A0Client,
    files: &[UploadFile],
) -> Result<Vec<String>, ApiError> {
    check_count(files)?;
    client.require_socket_session()?;
    let boundary = format!("A0-{}", client.new_request_id());
    let mut mp = MultipartBody::new(boundary);
    let mut names = Vec::with_capacity(files.len());
    for file in files {
        let name = upload_name(&file.basename, &client.new_request_id());
        mp.file("file", &name, file)?;
        names.push(name);
    }
    let (body, content_type) = mp.finish();
    let spec = RequestSpec {
        method: reqwest::Method::POST,
        path_and_query: "/api/upload".to_string(),
        body: Body::Raw(body, content_type),
        response_cap: 64 * 1024,
        retry: crate::client::RetryPolicy::Never,
        accept: None,
    };
    let payload: Value = client.execute_json(spec).await?;
    let filenames: Vec<Option<&str>> = payload
        .get("filenames")
        .and_then(Value::as_array)
        .map(|a| a.iter().map(Value::as_str).collect())
        .unwrap_or_default();
    // §7: response must decode {"filenames":[...]} equal to the client-computed
    // upload names, in order (no `ok` key on /api/upload).
    if !filenames_match(&names, &filenames) {
        return Err(ApiError::Protocol("incompatiblePayload".to_string()));
    }
    Ok(names)
}

/// Flow 2 (§7, `Attachments.swift:85-91`): single `POST /api/message_async`
/// multipart with text fields `context`, `text`, `message_id` and file parts
/// named `attachments`. The response must decode `{"context":"<ctx>"}` with
/// the matching context (§3.1).
pub async fn message_async_multipart(
    client: &A0Client,
    context: &str,
    text: &str,
    files: &[UploadFile],
) -> Result<String, ApiError> {
    check_count(files)?;
    client.require_socket_session()?;
    let message_id = client.new_request_id();
    let boundary = format!("A0-{}", client.new_request_id());
    let mut mp = MultipartBody::new(boundary);
    mp.text("context", context);
    mp.text("text", text);
    mp.text("message_id", &message_id);
    for file in files {
        let name = upload_name(&file.basename, &client.new_request_id());
        mp.file("attachments", &name, file)?;
    }
    let (body, content_type) = mp.finish();
    let spec = RequestSpec {
        method: reqwest::Method::POST,
        path_and_query: "/api/message_async".to_string(),
        body: Body::Raw(body, content_type),
        response_cap: 1024 * 1024,
        retry: crate::client::RetryPolicy::Never,
        accept: None,
    };
    let payload: Value = client.execute_json(spec).await?;
    let ctx = payload.get("context").and_then(Value::as_str).unwrap_or("");
    if ctx != context {
        return Err(ApiError::Protocol("incompatiblePayload".to_string()));
    }
    Ok(message_id)
}

/// §7 queued-flow completion helper (§3.1): after [`upload_files`], send
/// `POST /api/message_queue_add` JSON with the `attachments` array. Response
/// must be `{"ok":true,"item_id":"<id>"}` echoing the client id.
pub async fn message_queue_add_with_attachments(
    client: &A0Client,
    context: &str,
    text: &str,
    upload_names: &[String],
) -> Result<String, ApiError> {
    client.require_socket_session()?;
    let item_id = client.new_request_id();
    let payload = json!({
        "context": context,
        "text": text,
        "item_id": item_id,
        "attachments": upload_names,
    });
    let response: Value = client
        .post_json("/api/message_queue_add", &payload, 64 * 1024)
        .await?;
    let ok = response.get("ok").and_then(Value::as_bool).unwrap_or(false);
    let echoed = response
        .get("item_id")
        .and_then(Value::as_str)
        .unwrap_or("");
    if !ok || echoed != item_id {
        return Err(ApiError::Protocol("incompatiblePayload".to_string()));
    }
    Ok(item_id)
}

/// Pure §7 echo rule: the server filenames array must equal the
/// client-computed upload names, in order.
fn filenames_match(computed: &[String], echoed: &[Option<&str>]) -> bool {
    computed.len() == echoed.len()
        && echoed
            .iter()
            .zip(computed.iter())
            .all(|(wire, local)| wire.map(|w| w == local).unwrap_or(false))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn upload_name_sanitizes_per_s7() {
        // §7 (`Attachments.swift:39-45`): chars outside ASCII letters/digits/
        // `.-_` → `_`, truncate 120, strip trailing dots, empty → `file`.
        assert!(upload_name("a b/c.txt", "U").ends_with("a_b_c.txt"));
        assert!(upload_name("a b/c.txt", "U").starts_with("U-"));
        let long = "x".repeat(200);
        let name = upload_name(&long, "U");
        assert_eq!(name.len(), 2 + 120); // "U-" + 120 sanitized chars
        assert_eq!(upload_name("...", "U"), "U-file"); // trailing dots stripped → empty → file
        assert_eq!(upload_name("", "U"), "U-file");
        assert_eq!(upload_name("photo.jpeg", "U"), "U-photo.jpeg");
    }

    #[test]
    fn filenames_match_requires_exact_order() {
        let computed = vec!["A".to_string(), "B".to_string()];
        assert!(filenames_match(&computed, &[Some("A"), Some("B")]));
        assert!(!filenames_match(&computed, &[Some("B"), Some("A")])); // order matters
        assert!(!filenames_match(&computed, &[Some("A")])); // count matters
        assert!(!filenames_match(&computed, &[None, Some("B")]));
    }

    #[test]
    fn multipart_body_shape_matches_s7() {
        // §7: text parts carry no filename; file parts carry
        // `filename="<uploadName>"` + `Content-Type: <mime>` + raw bytes;
        // boundary A0-<UUID>; closing delimiter.
        let mut mp = MultipartBody::new("A0-test-boundary".to_string());
        mp.text("context", "ctx-1");
        mp.text("text", "hello");
        mp.text("message_id", "m-1");
        let file = UploadFile {
            bytes: b"PNGDATA".to_vec(),
            mime: "image/png".to_string(),
            basename: "shot.png".to_string(),
        };
        mp.file("attachments", "U-shot.png", &file).unwrap();
        let (body, content_type) = mp.finish();
        let text = String::from_utf8_lossy(&body);
        assert_eq!(
            content_type,
            "multipart/form-data; boundary=A0-test-boundary"
        );
        assert!(text.contains(
            "Content-Disposition: form-data; name=\"context\"\r\n\r\nctx-1\r
"
        ));
        assert!(text.contains(
            "Content-Disposition: form-data; name=\"text\"\r\n\r\nhello\r
"
        ));
        assert!(text.contains(
            "Content-Disposition: form-data; name=\"attachments\"; filename=\"U-shot.png\"\r\n"
        ));
        assert!(text.contains(
            "Content-Type: image/png\r\n\r\nPNGDATA\r
"
        ));
        assert!(text.ends_with("--A0-test-boundary--\r\n"));
        // Text parts must NOT carry a filename.
        assert!(!text.contains("name=\"context\"; filename"));
    }
}
