// Copyright 2026 Samaksh Kamboj - SPDX-License-Identifier: Apache-2.0
//! Unix-socket JSON protocol shared by the daemon and the CLI.
//! Wire format is documented in `docs/IPC.md`.

use std::io::{Read, Write};
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::time::Duration;

/// Reject payloads larger than 1 MiB before parsing.
pub const MAX_MESSAGE: u64 = 1024 * 1024;

pub fn home_dir() -> Result<PathBuf, String> {
    std::env::var("HOME").map(PathBuf::from).map_err(|_| "HOME is not set".into())
}

/// `$XDG_DATA_HOME/fetchline` or `~/.local/share/fetchline`.
pub fn data_dir() -> Result<PathBuf, String> {
    if let Ok(xdg) = std::env::var("XDG_DATA_HOME") {
        let base = PathBuf::from(xdg);
        if base.is_absolute() {
            return Ok(base.join("fetchline"));
        }
    }
    Ok(home_dir()?.join(".local").join("share").join("fetchline"))
}

pub fn socket_path(data_dir: &Path) -> PathBuf {
    data_dir.join("control.sock")
}

pub fn db_path(data_dir: &Path) -> PathBuf {
    data_dir.join("downloads.sqlite3")
}

/// Send one JSON request, read one JSON response.
pub fn call(socket: &Path, req: &serde_json::Value) -> Result<serde_json::Value, String> {
    let mut stream = UnixStream::connect(socket)
        .map_err(|e| format!("daemon unreachable (is fetchlined running?): {e}"))?;
    stream.set_read_timeout(Some(Duration::from_secs(30))).map_err(|e| e.to_string())?;
    stream.set_write_timeout(Some(Duration::from_secs(5))).map_err(|e| e.to_string())?;
    let bytes = serde_json::to_vec(req).map_err(|e| e.to_string())?;
    if bytes.len() as u64 > MAX_MESSAGE {
        return Err("request too large".into());
    }
    stream.write_all(&bytes).map_err(|e| e.to_string())?;
    stream.shutdown(std::net::Shutdown::Write).map_err(|e| e.to_string())?;
    let mut buf = Vec::new();
    stream
        .take(MAX_MESSAGE + 1)
        .read_to_end(&mut buf)
        .map_err(|e| e.to_string())?;
    if buf.len() as u64 > MAX_MESSAGE {
        return Err("response too large".into());
    }
    serde_json::from_slice(&buf).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn data_dir_respects_xdg() {
        std::env::set_var("XDG_DATA_HOME", "/tmp/xdg-test");
        assert_eq!(data_dir().unwrap(), PathBuf::from("/tmp/xdg-test/fetchline"));
        std::env::remove_var("XDG_DATA_HOME");
        let home = home_dir().unwrap();
        assert_eq!(data_dir().unwrap(), home.join(".local/share/fetchline"));
    }
}
