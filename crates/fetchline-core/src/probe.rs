// Copyright 2026 Samaksh Kamboj - SPDX-License-Identifier: Apache-2.0
//! HEAD probe results and filename inference.

use serde::{Deserialize, Serialize};

/// Outcome of probing a URL before downloading.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProbeResult {
    pub url: String,
    pub total_bytes: Option<u64>,
    pub supports_ranges: bool,
    pub etag: Option<String>,
    pub filename: String,
}

/// Infer a filename from a URL path. Falls back to `download.bin`.
pub fn filename_from_url(url: &str) -> String {
    let path = url.split(['?', '#']).next().unwrap_or(url);
    let last = path.rsplit('/').next().unwrap_or("").trim();
    if last.is_empty() {
        return "download.bin".to_string();
    }
    percent_decode(last)
}

/// Parse `Content-Disposition: attachment; filename="a.zip"` style headers.
pub fn filename_from_disposition(header: &str) -> Option<String> {
    for part in header.split(';') {
        let part = part.trim();
        if let Some(rest) = part.strip_prefix("filename*=") {
            // RFC 5987: UTF-8''name%20here
            let name = rest.split('\'').next_back().unwrap_or(rest);
            return Some(percent_decode(name.trim_matches('"')));
        }
        if let Some(rest) = part.strip_prefix("filename=") {
            let name = rest.trim().trim_matches('"');
            if !name.is_empty() {
                return Some(percent_decode(name));
            }
        }
    }
    None
}

fn percent_decode(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let bytes = s.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            if let (Some(h), Some(l)) = (hex(bytes[i + 1]), hex(bytes[i + 2])) {
                out.push((h << 4 | l) as char);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i] as char);
        i += 1;
    }
    // Sanitize path separators.
    out.replace(['/', '\\'], "_")
}

fn hex(b: u8) -> Option<u8> {
    match b {
        b'0'..=b'9' => Some(b - b'0'),
        b'a'..=b'f' => Some(b - b'a' + 10),
        b'A'..=b'F' => Some(b - b'A' + 10),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn url_filename_basics() {
        assert_eq!(filename_from_url("https://x.com/f.zip"), "f.zip");
        assert_eq!(filename_from_url("https://x.com/dir/"), "download.bin");
        assert_eq!(filename_from_url("https://x.com/a%20b.mp4?v=1"), "a b.mp4");
        assert_eq!(filename_from_url("https://x.com/a/b/../c.iso"), "c.iso");
    }

    #[test]
    fn disposition_parsing() {
        assert_eq!(
            filename_from_disposition("attachment; filename=\"a.zip\""),
            Some("a.zip".into())
        );
        assert_eq!(
            filename_from_disposition("attachment; filename*=UTF-8''a%20b.mp4"),
            Some("a b.mp4".into())
        );
        assert_eq!(filename_from_disposition("inline"), None);
    }

    #[test]
    fn path_traversal_sanitized() {
        assert_eq!(filename_from_url("https://x.com/../../etc/passwd"), "passwd");
    }
}
