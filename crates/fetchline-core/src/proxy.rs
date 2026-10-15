// Copyright 2026 Samaksh Kamboj - SPDX-License-Identifier: Apache-2.0
//! fetchline desktop shell: Tauri window plus thin invoke handlers.
//! Every command builds a protocol request (docs/IPC.md) and forwards it
//! to fetchlined over the Unix socket. No business logic lives here, so
//! this layer stays testable without a display or WebKit.

use crate::ipc;
use serde_json::Value;

/// Send one request to the daemon and return its raw response.
/// Transport errors (daemon down) surface as Err; protocol-level
/// `{ok:false}` passes through for the UI to display.
pub fn forward(req: &Value) -> Result<Value, String> {
    let sock = ipc::socket_path(&ipc::data_dir()?);
    ipc::call(&sock, req)
}

pub fn cmd_list(status: Option<String>) -> Result<Value, String> {
    let mut req = serde_json::json!({"action": "list"});
    if let Some(s) = status {
        req["status"] = s.into();
    }
    forward(&req)
}

pub fn cmd_add(urls: Vec<String>, connections: u64) -> Result<Value, String> {
    if urls.is_empty() || urls.len() > 100 {
        return Err("add needs 1-100 urls".into());
    }
    let mut results = Vec::new();
    for url in urls {
        let req = serde_json::json!({"action": "add", "url": url, "connections": connections.clamp(1, 16)});
        results.push(forward(&req)?);
    }
    Ok(Value::Array(results))
}

pub fn cmd_act(action: &str, id: i64) -> Result<Value, String> {
    match action {
        "pause" | "resume" | "cancel" | "retry" | "remove" | "status" => {}
        _ => return Err("unknown action".into()),
    }
    forward(&serde_json::json!({"action": action, "id": id}))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn add_validates_batch_size() {
        assert!(cmd_add(vec![], 8).is_err());
        assert!(cmd_add(vec!["x".into(); 101], 8).is_err());
    }

    #[test]
    fn act_rejects_unknown() {
        assert!(cmd_act("explode", 1).is_err());
    }
}
