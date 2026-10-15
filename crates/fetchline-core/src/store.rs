// Copyright 2026 Samaksh Kamboj - SPDX-License-Identifier: Apache-2.0
//! SQLite persistence: downloads queue, settings.

use rusqlite::{Connection, OptionalExtension, params};

/// Download lifecycle states.
pub const QUEUED: &str = "queued";
pub const ACTIVE: &str = "active";
pub const PAUSED: &str = "paused";
pub const COMPLETED: &str = "completed";
pub const FAILED: &str = "failed";
pub const CANCELLED: &str = "cancelled";

#[derive(Debug, Clone)]
pub struct Download {
    pub id: i64,
    pub url: String,
    pub output: String,
    pub filename: String,
    pub status: String,
    pub total_bytes: Option<u64>,
    pub downloaded_bytes: u64,
    pub connections: u64,
    pub error: Option<String>,
    pub scheduled_at: Option<i64>,
}

pub struct Store {
    conn: Connection,
}

impl Store {
    /// Open (creating parents, tables) at `path`.
    pub fn open(path: &std::path::Path) -> Result<Self, String> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
        let conn = Connection::open(path).map_err(|e| e.to_string())?;
        conn.busy_timeout(std::time::Duration::from_secs(5)).map_err(|e| e.to_string())?;
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS downloads (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                url TEXT NOT NULL,
                output TEXT NOT NULL,
                filename TEXT NOT NULL,
                status TEXT NOT NULL DEFAULT 'queued',
                total_bytes INTEGER,
                downloaded_bytes INTEGER NOT NULL DEFAULT 0,
                connections INTEGER NOT NULL DEFAULT 8,
                error TEXT,
                scheduled_at INTEGER,
                created_at INTEGER NOT NULL,
                updated_at INTEGER NOT NULL
            );
            CREATE INDEX IF NOT EXISTS idx_downloads_status ON downloads(status);
            CREATE TABLE IF NOT EXISTS settings (
                key TEXT PRIMARY KEY,
                value TEXT NOT NULL
            );",
        )
        .map_err(|e| e.to_string())?;
        Ok(Self { conn })
    }

    fn now() -> i64 {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs() as i64)
            .unwrap_or(0)
    }

    /// Insert a queued download. Returns the row id.
    pub fn add(
        &self,
        url: &str,
        output: &str,
        filename: &str,
        connections: u64,
        scheduled_at: Option<i64>,
    ) -> Result<i64, String> {
        let now = Self::now();
        self.conn
            .execute(
                "INSERT INTO downloads (url, output, filename, status, connections, scheduled_at, created_at, updated_at)
                 VALUES (?1, ?2, ?3, 'queued', ?4, ?5, ?6, ?6)",
                params![url, output, filename, connections as i64, scheduled_at, now],
            )
            .map_err(|e| e.to_string())?;
        Ok(self.conn.last_insert_rowid())
    }

    /// True when `url` already has a queued/active/paused entry.
    pub fn exists_live(&self, url: &str) -> Result<bool, String> {
        let n: i64 = self
            .conn
            .query_row(
                "SELECT COUNT(*) FROM downloads WHERE url = ?1 AND status IN ('queued','active','paused')",
                params![url],
                |r| r.get(0),
            )
            .map_err(|e| e.to_string())?;
        Ok(n > 0)
    }

    pub fn get(&self, id: i64) -> Result<Option<Download>, String> {
        self.conn
            .query_row("SELECT id, url, output, filename, status, total_bytes, downloaded_bytes, connections, error, scheduled_at FROM downloads WHERE id = ?1", params![id], row_to_download)
            .optional()
            .map_err(|e| e.to_string())
    }

    pub fn list(&self, status: Option<&str>) -> Result<Vec<Download>, String> {
        let mut out = Vec::new();
        if let Some(s) = status {
            let mut stmt = self.conn.prepare("SELECT id, url, output, filename, status, total_bytes, downloaded_bytes, connections, error, scheduled_at FROM downloads WHERE status = ?1 ORDER BY id").map_err(|e| e.to_string())?;
            let rows = stmt.query_map(params![s], row_to_download).map_err(|e| e.to_string())?;
            for r in rows {
                out.push(r.map_err(|e| e.to_string())?);
            }
        } else {
            let mut stmt = self.conn.prepare("SELECT id, url, output, filename, status, total_bytes, downloaded_bytes, connections, error, scheduled_at FROM downloads ORDER BY id").map_err(|e| e.to_string())?;
            let rows = stmt.query_map([], row_to_download).map_err(|e| e.to_string())?;
            for r in rows {
                out.push(r.map_err(|e| e.to_string())?);
            }
        }
        Ok(out)
    }

    /// Downloads due to run: queued with no future schedule.
    pub fn due(&self) -> Result<Vec<Download>, String> {
        let now = Self::now();
        let mut stmt = self.conn.prepare("SELECT id, url, output, filename, status, total_bytes, downloaded_bytes, connections, error, scheduled_at FROM downloads WHERE status = 'queued' AND (scheduled_at IS NULL OR scheduled_at <= ?1) ORDER BY id").map_err(|e| e.to_string())?;
        let rows = stmt.query_map(params![now], row_to_download).map_err(|e| e.to_string())?;
        let mut out = Vec::new();
        for r in rows {
            out.push(r.map_err(|e| e.to_string())?);
        }
        Ok(out)
    }

    pub fn set_status(&self, id: i64, status: &str, error: Option<&str>) -> Result<(), String> {
        self.conn
            .execute(
                "UPDATE downloads SET status = ?1, error = ?2, updated_at = ?3 WHERE id = ?4",
                params![status, error, Self::now(), id],
            )
            .map_err(|e| e.to_string())?;
        Ok(())
    }

    pub fn set_progress(
        &self,
        id: i64,
        downloaded: u64,
        total: Option<u64>,
    ) -> Result<(), String> {
        self.conn
            .execute(
                "UPDATE downloads SET downloaded_bytes = ?1, total_bytes = ?2, updated_at = ?3 WHERE id = ?4",
                params![downloaded as i64, total.map(|t| t as i64), Self::now(), id],
            )
            .map_err(|e| e.to_string())?;
        Ok(())
    }

    /// Requeue a failed/cancelled download. Returns false when the row
    /// is not in a retryable state.
    pub fn retry(&self, id: i64) -> Result<bool, String> {
        let n = self
            .conn
            .execute(
                "UPDATE downloads SET status = 'queued', error = NULL, updated_at = ?1 WHERE id = ?2 AND status IN ('failed','cancelled')",
                params![Self::now(), id],
            )
            .map_err(|e| e.to_string())?;
        Ok(n == 1)
    }

    /// Delete one row. Returns the removed row for file cleanup.
    pub fn remove(&self, id: i64) -> Result<Option<Download>, String> {
        let row = self.get(id)?;
        self.conn
            .execute("DELETE FROM downloads WHERE id = ?1", params![id])
            .map_err(|e| e.to_string())?;
        Ok(row)
    }

    /// Delete completed rows. Returns the count removed.
    pub fn clear_completed(&self) -> Result<usize, String> {
        let n = self
            .conn
            .execute("DELETE FROM downloads WHERE status = 'completed'", [])
            .map_err(|e| e.to_string())?;
        Ok(n)
    }

    pub fn setting_get(&self, key: &str) -> Result<Option<String>, String> {
        self.conn
            .query_row("SELECT value FROM settings WHERE key = ?1", params![key], |r| {
                r.get(0)
            })
            .optional()
            .map_err(|e| e.to_string())
    }

    pub fn setting_set(&self, key: &str, value: &str) -> Result<(), String> {
        self.conn
            .execute(
                "INSERT INTO settings (key, value) VALUES (?1, ?2)
                 ON CONFLICT(key) DO UPDATE SET value = excluded.value",
                params![key, value],
            )
            .map_err(|e| e.to_string())?;
        Ok(())
    }
}

fn row_to_download(r: &rusqlite::Row) -> rusqlite::Result<Download> {
    Ok(Download {
        id: r.get(0)?,
        url: r.get(1)?,
        output: r.get(2)?,
        filename: r.get(3)?,
        status: r.get(4)?,
        total_bytes: r.get::<_, Option<i64>>(5)?.map(|v| v as u64),
        downloaded_bytes: r.get::<_, i64>(6)? as u64,
        connections: r.get::<_, i64>(7)? as u64,
        error: r.get(8)?,
        scheduled_at: r.get(9)?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp_db(name: &str) -> Store {
        let path = std::env::temp_dir().join(format!("fetchline-store-{name}.sqlite3"));
        let _ = std::fs::remove_file(&path);
        Store::open(&path).unwrap()
    }

    #[test]
    fn lifecycle() {
        let s = tmp_db("lifecycle");
        let id = s.add("http://x/f.zip", "/tmp/f.zip", "f.zip", 8, None).unwrap();
        assert!(s.exists_live("http://x/f.zip").unwrap());
        s.set_status(id, ACTIVE, None).unwrap();
        s.set_progress(id, 50, Some(100)).unwrap();
        let d = s.get(id).unwrap().unwrap();
        assert_eq!(d.status, ACTIVE);
        assert_eq!(d.downloaded_bytes, 50);
        assert_eq!(d.total_bytes, Some(100));
        s.set_status(id, FAILED, Some("boom")).unwrap();
        assert!(s.retry(id).unwrap());
        assert_eq!(s.get(id).unwrap().unwrap().status, QUEUED);
        assert!(!s.retry(id).unwrap(), "queued is not retryable");
        s.set_status(id, COMPLETED, None).unwrap();
        assert_eq!(s.clear_completed().unwrap(), 1);
        assert!(s.get(id).unwrap().is_none());
    }

    #[test]
    fn due_and_remove() {
        let s = tmp_db("due");
        let future = Store::now() + 3600;
        s.add("http://x/a", "/tmp/a", "a", 8, Some(future)).unwrap();
        let id = s.add("http://x/b", "/tmp/b", "b", 8, None).unwrap();
        let due = s.due().unwrap();
        assert_eq!(due.len(), 1);
        assert_eq!(due[0].url, "http://x/b");
        let removed = s.remove(id).unwrap().unwrap();
        assert_eq!(removed.filename, "b");
        assert_eq!(s.list(None).unwrap().len(), 1);
    }

    #[test]
    fn settings_round_trip() {
        let s = tmp_db("settings");
        assert!(s.setting_get("k").unwrap().is_none());
        s.setting_set("k", "v").unwrap();
        assert_eq!(s.setting_get("k").unwrap().as_deref(), Some("v"));
        s.setting_set("k", "v2").unwrap();
        assert_eq!(s.setting_get("k").unwrap().as_deref(), Some("v2"));
    }
}
