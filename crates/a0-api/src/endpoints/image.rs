//! Media — inventory §3.9 (`BrowserScreenshot.swift:12,42-87`):
//! `GET /api/image_get?path=<path>` with client-side path validation BEFORE
//! requesting, image-only Accept/MIME rules and an 8 MiB cap (§9).

use crate::client::{A0Client, Body, RequestSpec, RetryPolicy};
use crate::error::ApiError;
use crate::util::query_encode;

/// §9 (`BrowserScreenshot.swift:12`): image body cap 8 MiB.
const CAP_8MIB: usize = 8 * 1024 * 1024;

/// §3.9 accepted image extensions (`BrowserScreenshot.swift:50-57`).
const ALLOWED_EXTENSIONS: [&str; 6] = ["png", "jpg", "jpeg", "webp", "gif", "bmp"];

/// §3.9 Accept header (`BrowserScreenshot.swift:62-66`).
const IMAGE_ACCEPT: &str = "image/png, image/jpeg, image/webp, image/gif, image/bmp";

/// §3.9 client-side path validation BEFORE requesting
/// (`BrowserScreenshot.swift:50-57`): non-empty, ≤2048 bytes, no leading
/// `//`, no `://`, no `\\`, no control chars, none of `%?#&`, no `.`/`..`
/// path segments, extension ∈ {png, jpg, jpeg, webp, gif, bmp}.
pub fn validate_image_path(path: &str) -> Result<(), ApiError> {
    let invalid = || ApiError::Protocol("invalidImagePath".to_string());
    if path.is_empty() || path.len() > 2048 {
        return Err(invalid());
    }
    if path.starts_with("//") || path.contains("://") || path.contains("\\") {
        return Err(invalid());
    }
    if path.chars().any(|c| c.is_control()) {
        return Err(invalid());
    }
    if path.chars().any(|c| matches!(c, '%' | '?' | '#' | '&')) {
        return Err(invalid());
    }
    let mut segments = path.split('/');
    let last = segments.next_back().unwrap_or("");
    for segment in segments {
        if segment == "." || segment == ".." {
            return Err(invalid());
        }
    }
    if last == "." || last == ".." {
        return Err(invalid());
    }
    let Some((_, ext)) = last.rsplit_once('.') else {
        return Err(invalid());
    };
    if !ALLOWED_EXTENSIONS.contains(&ext.to_ascii_lowercase().as_str()) {
        return Err(invalid());
    }
    Ok(())
}

/// §3.9 `GET /api/image_get?path=<path>` (`BrowserScreenshot.swift:58-74`):
/// image MIME content types only, non-empty body, ≤8 MiB. Idempotent GET →
/// the §6 transient-read retry policy applies; full cookie header (including
/// the synthetic csrf cookie) and `X-CSRF-Token` are added by the shared
/// client (§3.9 header list `BrowserScreenshot.swift:62-66`).
pub async fn image_get(client: &A0Client, path: &str) -> Result<(Vec<u8>, String), ApiError> {
    validate_image_path(path)?;
    let spec = RequestSpec {
        method: reqwest::Method::GET,
        path_and_query: format!("/api/image_get?path={}", query_encode(path)),
        body: Body::None,
        response_cap: CAP_8MIB,
        retry: RetryPolicy::TransientRead,
        accept: Some(IMAGE_ACCEPT),
    };
    let (body, content_type) = client
        .execute_binary(spec, |ctype| {
            [
                "image/png",
                "image/jpeg",
                "image/webp",
                "image/gif",
                "image/bmp",
            ]
            .iter()
            .any(|allowed| ctype.starts_with(allowed))
        })
        .await?;
    if body.is_empty() {
        return Err(ApiError::Protocol("unexpectedResponse".to_string()));
    }
    Ok((body, content_type))
}
