// Copyright 2026 Samaksh Kamboj - SPDX-License-Identifier: Apache-2.0
//! App-shell command surface. Thin wrappers over [`proxy`]; see it for logic.

use fetchline_core::proxy;
use serde_json::Value;

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
        .invoke_handler(tauri::generate_handler![dl_list, dl_add, dl_act])
        .run(tauri::generate_context!())
        .expect("error while running fetchline");
}
