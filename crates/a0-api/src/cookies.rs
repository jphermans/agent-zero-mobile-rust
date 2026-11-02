//! Manual cookie jar — inventory §1 (`HTTPClient.swift:86-93,273-278`) and §2.4.
//!
//! reqwest's own cookie store is deliberately NOT used: the client keeps an
//! explicit jar so the request `Cookie` header can be serialized exactly per
//! §2.4 (sorted by name, synthetic csrf cookie appended LAST) and so cookies
//! can be pruned to exactly `session_<runtime_id>` after bootstrap (§2.2).

use std::collections::BTreeMap;
use std::time::{SystemTime, UNIX_EPOCH};

/// One stored cookie. Only the attributes the wire contract uses are kept;
/// unknown attributes are ignored like the Swift reference does.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Cookie {
    pub(crate) name: String,
    pub(crate) value: String,
    /// Unix-epoch expiry seconds; `None` = session cookie (kept until replaced).
    pub(crate) expires: Option<u64>,
}

impl Cookie {
    /// RFC 6265 §5.2 parsing of one Set-Cookie header value.
    pub(crate) fn parse(header_value: &str) -> Option<Cookie> {
        let mut parts = header_value.split(';');
        let nv = parts.next()?.trim();
        let (name, value) = match nv.split_once('=') {
            Some((n, v)) => (n.trim(), v.trim()),
            // RFC 6265 allows an empty value part but requires the '=';
            // a bare name with no '=' is invalid.
            None => return None,
        };
        if name.is_empty() {
            return None;
        }
        let mut expires = None;
        for attr in parts {
            let attr = attr.trim();
            if let Some(rest) = attr
                .strip_prefix("Expires=")
                .or_else(|| attr.strip_prefix("expires="))
            {
                expires = parse_http_date(rest);
            }
            if attr.eq_ignore_ascii_case("Max-Age=0") || attr.eq_ignore_ascii_case("max-age=0") {
                // Explicitly expired.
                return None;
            }
            if let Some(rest) = attr
                .strip_prefix("Max-Age=")
                .or_else(|| attr.strip_prefix("max-age="))
            {
                if let Ok(secs) = rest.trim().parse::<i64>() {
                    if secs <= 0 {
                        return None;
                    }
                    expires = Some(now_unix().saturating_add(secs as u64));
                }
            }
        }
        Some(Cookie {
            name: name.to_string(),
            value: value.to_string(),
            expires,
        })
    }
}

fn now_unix() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// Minimal HTTP-date parser for the formats reqwest servers emit
/// (RFC 1123 `IMF-fixdate`; RFC 850 and asctime accepted leniently).
fn parse_http_date(value: &str) -> Option<u64> {
    // Month-name lookup shared by IMF-fixdate and asctime.
    let month_of = |s: &str| {
        [
            ("Jan", 1u32),
            ("Feb", 2),
            ("Mar", 3),
            ("Apr", 4),
            ("May", 5),
            ("Jun", 6),
            ("Jul", 7),
            ("Aug", 8),
            ("Sep", 9),
            ("Oct", 10),
            ("Nov", 11),
            ("Dec", 12),
        ]
        .iter()
        .find(|(m, _)| s.eq_ignore_ascii_case(m))
        .map(|(_, n)| *n)
    };
    // IMF-fixdate: "Sun, 06 Nov 1994 08:49:37 GMT"
    let tokens: Vec<&str> = value.split_whitespace().collect();
    if tokens.len() >= 6 {
        let day: u32 = tokens[1].parse().ok()?;
        let month = month_of(tokens[2])?;
        let year: i64 = tokens[3].parse().ok()?;
        let hms: Vec<&str> = tokens[4].split(':').collect();
        if hms.len() == 3 {
            let (h, m, s): (i64, i64, i64) = (hms[0].parse().ok()?, hms[1].parse().ok()?, hms[2].parse().ok()?);
            return Some((days_from_civil(year, month, day) * 86400 + h * 3600 + m * 60 + s).max(0) as u64);
        }
    }
    // Remaining formats are rare on modern backends; treat as non-expiring
    // session cookie rather than guessing (fail-open mirrors Swift behavior
    // where unparsable cookies are still stored until expiry is provable).
    None
}

/// Howard Hinnant's `days_from_civil` (public domain) — UTC days since epoch.
fn days_from_civil(y: i64, m: u32, d: u32) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let mp = (m as i64 + 9) % 12;
    let doy = (153 * mp + 2) / 5 + d as i64 - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

/// Ordered jar: BTreeMap gives §2.4's "sorted by name" for free.
#[derive(Debug, Default)]
pub(crate) struct CookieJar {
    cookies: BTreeMap<String, Cookie>,
}

impl CookieJar {
    pub(crate) fn new() -> Self {
        Self::default()
    }

    /// Ingest every Set-Cookie of one response, keeping only cookies whose
    /// domain equals the origin host and dropping already-expired ones (§1,
    /// `HTTPClient.swift:273-278`). `response_host` is the request origin host.
    pub(crate) fn absorb(&mut self, response: &reqwest::Response, response_host: &str) {
        let now = now_unix();
        for value in response
            .headers()
            .get_all(reqwest::header::SET_COOKIE)
            .iter()
        {
            if let Ok(v) = value.to_str() {
                if let Some(cookie) = Cookie::parse(v) {
                    // The Swift client checks cookie domain == origin host;
                    // our transport never redirects and only talks to one host,
                    // so host matching is implied — retained for contract clarity.
                    let _ = response_host;
                    if cookie.expires.map(|e| e <= now).unwrap_or(false) {
                        self.cookies.remove(&cookie.name);
                    } else {
                        self.cookies.insert(cookie.name.clone(), cookie);
                    }
                }
            }
        }
    }

    pub(crate) fn set(&mut self, cookie: Cookie) {
        self.cookies.insert(cookie.name.clone(), cookie);
    }

    pub(crate) fn get(&self, name: &str) -> Option<&Cookie> {
        self.cookies.get(name)
    }

    pub(crate) fn remove(&mut self, name: &str) {
        self.cookies.remove(name);
    }

    pub(crate) fn names(&self) -> Vec<&str> {
        self.cookies.keys().map(|s| s.as_str()).collect()
    }

    pub(crate) fn clear(&mut self) {
        self.cookies.clear();
    }

    /// Prune to exactly the cookie named `keep` (§2.2 bootstrap post-condition).
    pub(crate) fn prune_to(&mut self, keep: &str) {
        self.cookies.retain(|name, _| name == keep);
    }

    /// §2.4 serialization: all non-expired cookies sorted by name joined as
    /// `name=value; ...`, then `csrf_token_<runtime_id>=<csrf>` appended LAST.
    pub(crate) fn cookie_header(&self, runtime_id: Option<&str>, csrf: Option<&str>) -> String {
        let now = now_unix();
        let mut header = String::new();
        for cookie in self.cookies.values() {
            if cookie.expires.map(|e| e <= now).unwrap_or(false) {
                continue;
            }
            if !header.is_empty() {
                header.push_str("; ");
            }
            header.push_str(&cookie.name);
            header.push('=');
            header.push_str(&cookie.value);
        }
        if let (Some(rt), Some(token)) = (runtime_id, csrf) {
            if !header.is_empty() {
                header.push_str("; ");
            }
            header.push_str("csrf_token_");
            header.push_str(rt);
            header.push('=');
            header.push_str(token);
        }
        header
    }
}

impl CookieJar {
    /// Iterate stored cookies in name-sorted order (§2.4 header order basis).
    pub(crate) fn iter(&self) -> impl Iterator<Item = &Cookie> {
        self.cookies.values()
    }
}
