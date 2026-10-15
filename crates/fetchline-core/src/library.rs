// Copyright 2026 Samaksh Kamboj - SPDX-License-Identifier: Apache-2.0
//! Download categories and XDG-aware directories.
//!
//! Files land under `<downloads>/fetchline/<Category>/`. All functions take
//! an explicit `home` dir so tests never touch the real `$HOME`.

use std::path::{Path, PathBuf};

pub const APP_DIR: &str = "fetchline";

/// Read `XDG_DOWNLOAD_DIR` from `~/.config/user-dirs.dirs`.
/// Falls back to `~/Downloads`.
pub fn download_base(home: &Path) -> PathBuf {
    let dirs = home.join(".config").join("user-dirs.dirs");
    if let Ok(text) = std::fs::read_to_string(dirs) {
        for line in text.lines() {
            let line = line.trim();
            if let Some(raw) = line.strip_prefix("XDG_DOWNLOAD_DIR=") {
                let raw = raw.trim().trim_matches('"');
                if raw.is_empty() {
                    continue;
                }
                if let Some(rest) = raw.strip_prefix("$HOME/") {
                    return home.join(rest);
                }
                if raw == "$HOME" {
                    return home.to_path_buf();
                }
                let abs = PathBuf::from(raw);
                if abs.is_absolute() {
                    return abs;
                }
            }
        }
    }
    home.join("Downloads")
}

/// Category directory name for a filename.
pub fn category_for(filename: &str) -> &'static str {
    let ext = filename.rsplit('.').next().unwrap_or("").to_ascii_lowercase();
    match ext.as_str() {
        "mp4" | "mkv" | "webm" | "avi" | "mov" | "m4v" | "m3u8" | "mpd" => "Video",
        "mp3" | "m4a" | "flac" | "wav" | "ogg" | "opus" | "aac" => "Music",
        "png" | "jpg" | "jpeg" | "gif" | "webp" | "svg" | "bmp" | "avif" => "Images",
        "pdf" | "epub" | "csv" | "doc" | "docx" | "xls" | "xlsx" | "ppt" | "pptx"
        | "odt" | "txt" | "md" => "Documents",
        "apk" | "appimage" | "bin" | "deb" | "dmg" | "exe" | "msi" | "pkg" | "rpm" => {
            "Applications"
        }
        "zip" | "rar" | "tar" | "gz" | "tgz" | "bz2" | "xz" | "zst" | "7z" => "Archives",
        "iso" | "img" => "ISO",
        _ => "Other",
    }
}

/// Resolve `<base>/fetchline/<Category>/<filename>`, unique-ifying with
/// `name (1).ext` when the path already exists.
pub fn resolve_output(base: &Path, filename: &str) -> PathBuf {
    let dir = base.join(APP_DIR).join(category_for(filename));
    let mut candidate = dir.join(filename);
    if !candidate.exists() {
        return candidate;
    }
    let stem = filename.rsplit_once('.').map(|(s, _)| s).unwrap_or(filename);
    let ext = filename.rsplit_once('.').map(|(_, e)| e).unwrap_or("");
    for i in 1..1000 {
        candidate = if ext.is_empty() {
            dir.join(format!("{stem} ({i})"))
        } else {
            dir.join(format!("{stem} ({i}).{ext}"))
        };
        if !candidate.exists() {
            return candidate;
        }
    }
    candidate
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn categories() {
        assert_eq!(category_for("a.mp4"), "Video");
        assert_eq!(category_for("a.MP3"), "Music");
        assert_eq!(category_for("a.pdf"), "Documents");
        assert_eq!(category_for("a.deb"), "Applications");
        assert_eq!(category_for("a.tar.gz"), "Archives");
        assert_eq!(category_for("a.iso"), "ISO");
        assert_eq!(category_for("a.png"), "Images");
        assert_eq!(category_for("noext"), "Other");
    }

    #[test]
    fn xdg_and_unique() {
        let home = std::env::temp_dir().join("fetchline-lib-test");
        let _ = std::fs::remove_dir_all(&home);
        std::fs::create_dir_all(home.join(".config")).unwrap();
        std::fs::write(home.join(".config").join("user-dirs.dirs"), "XDG_DOWNLOAD_DIR=\"$HOME/DLS\"\n")
            .unwrap();
        assert_eq!(download_base(&home), home.join("DLS"));
        let _ = std::fs::remove_file(home.join(".config").join("user-dirs.dirs"));
        assert_eq!(download_base(&home), home.join("Downloads"));

        let base = home.join("base");
        let dir = base.join(APP_DIR).join("Video");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("a.mp4"), b"x").unwrap();
        assert_eq!(resolve_output(&base, "a.mp4"), dir.join("a (1).mp4"));
        assert_eq!(resolve_output(&base, "b.mp4"), dir.join("b.mp4"));
        let _ = std::fs::remove_dir_all(&home);
    }
}
