// Copyright 2026 Samaksh Kamboj - SPDX-License-Identifier: Apache-2.0
//! fetchlined: queue daemon. Owns the SQLite store, runs up to
//! `MAX_ACTIVE` transfers, serves the Unix-socket JSON protocol.

use std::collections::HashMap;
use std::io::{Read, Write};
use std::os::unix::fs::PermissionsExt;
use std::os::unix::net::UnixListener;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use fetchline_core::download::{CANCELLED, Control, Engine, PAUSED, state_path_for};
use fetchline_core::ipc::{self, MAX_MESSAGE};
use fetchline_core::library;
use fetchline_core::store::{self, Store};

const MAX_ACTIVE: usize = 3;

struct Daemon {
    db: PathBuf,
    controls: Mutex<HashMap<i64, Arc<Control>>>,
    shutdown: Mutex<bool>,
}

impl Daemon {
    fn store(&self) -> Result<Store, String> {
        Store::open(&self.db)
    }

    fn active_count(&self) -> usize {
        self.controls.lock().unwrap().len()
    }

    fn handle(&self, req: serde_json::Value) -> serde_json::Value {
        let action = req.get("action").and_then(|v| v.as_str()).unwrap_or("");
        match action {
            "add" => self.add(&req),
            "list" => self.list(&req),
            "status" => self.status(&req),
            "pause" => self.pause(&req),
            "resume" => self.resume(&req),
            "cancel" => self.cancel(&req),
            "retry" => self.retry(&req),
            "remove" => self.remove(&req),
            "clear" => self.clear(),
            "shutdown" => {
                *self.shutdown.lock().unwrap() = true;
                ok()
            }
            _ => err("unknown action"),
        }
    }

    fn add(&self, req: &serde_json::Value) -> serde_json::Value {
        let url = match req.get("url").and_then(|v| v.as_str()) {
            Some(u) if u.starts_with("http://") || u.starts_with("https://") => u,
            _ => return err("url must start with http:// or https://"),
        };
        let connections = req
            .get("connections")
            .and_then(|v| v.as_u64())
            .unwrap_or(8)
            .clamp(1, 16);
        let scheduled_at = req.get("scheduled_at").and_then(|v| v.as_i64());
        let store = match self.store() {
            Ok(s) => s,
            Err(e) => return err(&e),
        };
        match store.exists_live(url) {
            Ok(true) => return err("url is already queued or running"),
            Err(e) => return err(&e),
            _ => {}
        }
        // Probe for filename/total before queueing.
        let engine = match Engine::new() {
            Ok(e) => e,
            Err(e) => return err(&e),
        };
        let probe = match engine.probe(url) {
            Ok(p) => p,
            Err(e) => return err(&e),
        };
        let home = match ipc::home_dir() {
            Ok(h) => h,
            Err(e) => return err(&e),
        };
        let base = match req.get("dir").and_then(|v| v.as_str()) {
            Some(d) => PathBuf::from(d),
            None => library::download_base(&home),
        };
        let output = library::resolve_output(&base, &probe.filename);
        if let Some(parent) = output.parent() {
            if std::fs::create_dir_all(parent).is_err() {
                return err("cannot create download directory");
            }
        }
        match store.add(
            url,
            &output.to_string_lossy(),
            &probe.filename,
            connections,
            scheduled_at,
        ) {
            Ok(id) => serde_json::json!({"ok": true, "id": id}),
            Err(e) => err(&e),
        }
    }

    fn list(&self, req: &serde_json::Value) -> serde_json::Value {
        let status = req.get("status").and_then(|v| v.as_str());
        match self.store().and_then(|s| s.list(status)) {
            Ok(rows) => serde_json::json!({"ok": true, "downloads": rows.iter().map(row_json).collect::<Vec<_>>()}),
            Err(e) => err(&e),
        }
    }

    fn status(&self, req: &serde_json::Value) -> serde_json::Value {
        let id = match req.get("id").and_then(|v| v.as_i64()) {
            Some(i) => i,
            None => return err("missing id"),
        };
        match self.store().and_then(|s| s.get(id)) {
            Ok(Some(d)) => serde_json::json!({"ok": true, "download": row_json(&d)}),
            Ok(None) => err("unknown download id"),
            Err(e) => err(&e),
        }
    }

    fn pause(&self, req: &serde_json::Value) -> serde_json::Value {
        let id = match req.get("id").and_then(|v| v.as_i64()) {
            Some(i) => i,
            None => return err("missing id"),
        };
        if let Some(ctrl) = self.controls.lock().unwrap().get(&id) {
            ctrl.pause();
            return serde_json::json!({"ok": true});
        }
        // Not running: mark a queued row paused directly.
        match self.store().and_then(|s| {
            let row = s.get(id)?;
            if row.map(|d| d.status.as_str() == store::QUEUED).unwrap_or(false) {
                s.set_status(id, store::PAUSED, None)?;
                Ok(true)
            } else {
                Ok(false)
            }
        }) {
            Ok(true) => serde_json::json!({"ok": true}),
            Ok(false) => err("download is not running or queued"),
            Err(e) => err(&e),
        }
    }

    fn resume(&self, req: &serde_json::Value) -> serde_json::Value {
        let id = match req.get("id").and_then(|v| v.as_i64()) {
            Some(i) => i,
            None => return err("missing id"),
        };
        match self.store().and_then(|s| {
            let row = s.get(id)?;
            match row.map(|d| d.status) {
                Some(st) if st == store::PAUSED => {
                    s.set_status(id, store::QUEUED, None)?;
                    Ok(true)
                }
                _ => Ok(false),
            }
        }) {
            Ok(true) => serde_json::json!({"ok": true}),
            Ok(false) => err("only paused downloads can resume (use retry for failed)"),
            Err(e) => err(&e),
        }
    }

    fn cancel(&self, req: &serde_json::Value) -> serde_json::Value {
        let id = match req.get("id").and_then(|v| v.as_i64()) {
            Some(i) => i,
            None => return err("missing id"),
        };
        if let Some(ctrl) = self.controls.lock().unwrap().get(&id) {
            ctrl.cancel();
            return serde_json::json!({"ok": true});
        }
        match self.store().and_then(|s| {
            let row = s.get(id)?;
            match row {
                Some(d) if d.status != store::COMPLETED => {
                    cleanup_files(&d.output);
                    s.set_status(id, store::CANCELLED, None)?;
                    Ok(true)
                }
                _ => Ok(false),
            }
        }) {
            Ok(true) => serde_json::json!({"ok": true}),
            Ok(false) => err("nothing to cancel"),
            Err(e) => err(&e),
        }
    }

    fn retry(&self, req: &serde_json::Value) -> serde_json::Value {
        let id = match req.get("id").and_then(|v| v.as_i64()) {
            Some(i) => i,
            None => return err("missing id"),
        };
        match self.store().and_then(|s| s.retry(id)) {
            Ok(true) => serde_json::json!({"ok": true}),
            Ok(false) => err("only failed or cancelled downloads can retry"),
            Err(e) => err(&e),
        }
    }

    fn remove(&self, req: &serde_json::Value) -> serde_json::Value {
        let id = match req.get("id").and_then(|v| v.as_i64()) {
            Some(i) => i,
            None => return err("missing id"),
        };
        if self.controls.lock().unwrap().contains_key(&id) {
            return err("download is running; cancel first");
        }
        let delete_files = req.get("delete_files").and_then(|v| v.as_bool()).unwrap_or(false);
        match self.store().and_then(|s| s.remove(id)) {
            Ok(Some(d)) => {
                if delete_files {
                    cleanup_files(&d.output);
                }
                serde_json::json!({"ok": true})
            }
            Ok(None) => err("unknown download id"),
            Err(e) => err(&e),
        }
    }

    fn clear(&self) -> serde_json::Value {
        match self.store().and_then(|s| s.clear_completed()) {
            Ok(n) => serde_json::json!({"ok": true, "removed": n}),
            Err(e) => err(&e),
        }
    }

    /// Spawn workers for due downloads and sync progress. Runs every tick.
    fn tick(daemon: &Arc<Daemon>) {
        // Progress: read each active transfer's state.json into the store.
        let ids: Vec<i64> = daemon.controls.lock().unwrap().keys().copied().collect();
        if let Ok(store) = daemon.store() {
            for id in ids {
                if let Ok(Some(row)) = store.get(id) {
                    let sp = state_path_for(Path::new(&row.output));
                    if let Ok(raw) = std::fs::read_to_string(&sp) {
                        if let Ok(st) = serde_json::from_str::<fetchline_core::state::DownloadState>(&raw) {
                            let done: u64 = st.segments.iter().map(|s| s.done).sum();
                            let _ = store.set_progress(id, done, st.total_bytes);
                        }
                    }
                }
            }
        }
        // Schedule: fill free worker slots with due rows.
        while daemon.active_count() < MAX_ACTIVE {
            let next = daemon.store().and_then(|s| Ok(s.due()?.into_iter().next()));
            match next {
                Ok(Some(row)) => {
                    if daemon.store().and_then(|s| s.set_status(row.id, store::ACTIVE, None)).is_err() {
                        break;
                    }
                    let ctrl = Arc::new(Control::new());
                    daemon.controls.lock().unwrap().insert(row.id, ctrl.clone());
                    let daemon = daemon.clone();
                    std::thread::spawn(move || worker(&daemon, row.id, ctrl));
                }
                _ => break,
            }
        }
    }
}

fn worker(daemon: &Arc<Daemon>, id: i64, ctrl: Arc<Control>) {
    let result = run_transfer(daemon, id, &ctrl);
    daemon.controls.lock().unwrap().remove(&id);
    let Ok(store) = daemon.store() else { return };
    match result {
        Ok(()) => {
            let _ = store.set_status(id, store::COMPLETED, None);
        }
        Err(e) if e == PAUSED => {
            let _ = store.set_status(id, store::PAUSED, None);
        }
        Err(e) if e == CANCELLED => {
            if let Ok(Some(row)) = store.get(id) {
                cleanup_files(&row.output);
            }
            let _ = store.set_status(id, store::CANCELLED, None);
        }
        Err(e) => {
            let _ = store.set_status(id, store::FAILED, Some(&e));
        }
    }
}

fn run_transfer(daemon: &Arc<Daemon>, id: i64, ctrl: &Arc<Control>) -> Result<(), String> {
    let store = daemon.store()?;
    let row = store.get(id)?.ok_or("download vanished")?;
    let output = PathBuf::from(&row.output);
    let mut engine = Engine::new()?;
    engine.with_control(ctrl.clone());
    let sp = state_path_for(&output);
    if sp.exists() {
        engine.resume(&sp)?;
    } else {
        engine.download(&row.url, &output, row.connections, None)?;
    }
    // Final progress sync so completed rows show full totals.
    if let Ok(meta) = std::fs::metadata(&output) {
        let _ = store.set_progress(id, meta.len(), Some(meta.len()));
    }
    Ok(())
}

fn cleanup_files(output: &str) {
    let out = Path::new(output);
    let _ = std::fs::remove_file(out);
    let _ = std::fs::remove_file(state_path_for(out));
}

fn row_json(d: &store::Download) -> serde_json::Value {
    serde_json::json!({
        "id": d.id,
        "url": d.url,
        "output": d.output,
        "filename": d.filename,
        "status": d.status,
        "totalBytes": d.total_bytes,
        "downloadedBytes": d.downloaded_bytes,
        "connections": d.connections,
        "error": d.error,
        "scheduledAt": d.scheduled_at,
    })
}

fn ok() -> serde_json::Value {
    serde_json::json!({"ok": true})
}

fn err(msg: &str) -> serde_json::Value {
    serde_json::json!({"ok": false, "error": msg})
}

fn main() {
    if let Err(e) = run() {
        eprintln!("fetchlined: {e}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), String> {
    let data_dir = ipc::data_dir()?;
    std::fs::create_dir_all(&data_dir).map_err(|e| e.to_string())?;
    // Touch the store early so a corrupt DB fails fast, not mid-transfer.
    Store::open(&ipc::db_path(&data_dir))?;
    let sock = ipc::socket_path(&data_dir);
    if sock.exists() {
        std::fs::remove_file(&sock).map_err(|e| e.to_string())?;
    }
    let listener = UnixListener::bind(&sock).map_err(|e| e.to_string())?;
    std::fs::set_permissions(&sock, std::fs::Permissions::from_mode(0o600))
        .map_err(|e| e.to_string())?;
    // Non-blocking accept so the scheduler tick runs without connections.
    listener.set_nonblocking(true).map_err(|e| e.to_string())?;

    let daemon = Arc::new(Daemon {
        db: ipc::db_path(&data_dir),
        controls: Mutex::new(HashMap::new()),
        shutdown: Mutex::new(false),
    });

    loop {
        if *daemon.shutdown.lock().unwrap() {
            break;
        }
        match listener.accept() {
            Ok((mut stream, _)) => {
                let daemon = daemon.clone();
                std::thread::spawn(move || {
                    let reply = read_request(&mut stream)
                        .map(|req| daemon.handle(req))
                        .unwrap_or_else(|e| err(&e));
                    if let Ok(bytes) = serde_json::to_vec(&reply) {
                        let _ = stream.write_all(&bytes);
                    }
                });
            }
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {}
            Err(e) => eprintln!("fetchlined: accept: {e}"),
        }
        Daemon::tick(&daemon);
        std::thread::sleep(Duration::from_millis(200));
    }
    let _ = std::fs::remove_file(&sock);
    Ok(())
}

fn read_request(stream: &mut std::os::unix::net::UnixStream) -> Result<serde_json::Value, String> {
    let mut buf = Vec::new();
    stream
        .take(MAX_MESSAGE + 1)
        .read_to_end(&mut buf)
        .map_err(|e| e.to_string())?;
    if buf.len() as u64 > MAX_MESSAGE {
        return Err("request too large".into());
    }
    serde_json::from_slice(&buf).map_err(|e| e.to_string())
}
