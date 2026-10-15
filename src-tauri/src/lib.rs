// Copyright 2026 Samaksh Kamboj - SPDX-License-Identifier: Apache-2.0
//! App-shell command surface. Thin wrappers over [`proxy`]; see it for logic.

use fetchline_core::proxy;
use serde_json::Value;
use std::os::unix::net::UnixStream;
use std::process::Stdio;
use std::time::{Duration, Instant};
use tauri::Manager;

#[tauri::command]
fn dl_list(status: Option<String>) -> Result<Value, String> {
    proxy::cmd_list(status)
}

#[tauri::command]
fn dl_add(urls: Vec<String>, connections: u64) -> Result<Value, String> {
    proxy::cmd_add(urls, connections)
}

#[tauri::command]
fn dl_act(action: String, id: i64) -> Result<Value, String> {
    proxy::cmd_act(&action, id)
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .setup(|app| {
            ensure_daemon(&app.handle().clone());
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![dl_list, dl_add, dl_act])
        .run(tauri::generate_context!())
        .expect("error while running fetchline");
}

/// Start fetchlined when no daemon answers the socket.
/// Prefers the bundled sidecar, then a sibling binary, then ~/.local/bin.
/// Never crashes the app: worst case the UI shows "daemon down".
fn ensure_daemon(app: &tauri::AppHandle) {
    let data_dir = match fetchline_core::ipc::data_dir() {
        Ok(d) => d,
        Err(e) => {
            eprintln!("fetchline: {e}");
            return;
        }
    };
    let sock = fetchline_core::ipc::socket_path(&data_dir);
    if UnixStream::connect(&sock).is_ok() {
        return;
    }
    let mut candidates = Vec::new();
    if let Ok(res) = app.path().resource_dir() {
        candidates.push(res.join("fetchlined"));
    }
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            candidates.push(dir.join("fetchlined"));
        }
    }
    if let Ok(home) = fetchline_core::ipc::home_dir() {
        candidates.push(home.join(".local").join("bin").join("fetchlined"));
    }
    let Some(bin) = candidates.into_iter().find(|p| p.is_file()) else {
        eprintln!("fetchline: fetchlined not found");
        return;
    };
    if std::process::Command::new(&bin)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .is_err()
    {
        eprintln!("fetchline: cannot start {}", bin.display());
        return;
    }
    let start = Instant::now();
    while start.elapsed() < Duration::from_secs(10) {
        if UnixStream::connect(&sock).is_ok() {
            return;
        }
        std::thread::sleep(Duration::from_millis(200));
    }
    eprintln!("fetchline: fetchlined did not come up");
}
