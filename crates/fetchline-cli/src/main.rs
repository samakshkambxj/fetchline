// Copyright 2026 Samaksh Kamboj - SPDX-License-Identifier: Apache-2.0
//! fetchline: CLI front-end. Talks to fetchlined over the Unix socket,
//! spawning the daemon on demand.

use std::time::Duration;

use fetchline_core::ipc;

fn main() {
    if let Err(e) = run() {
        eprintln!("fetchline: {e}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), String> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.first().map(|s| s.as_str()) {
        Some("add") => cmd_add(&args[1..]),
        Some("list") => cmd_list(&args[1..]),
        Some("info") => cmd_one(&args[1..], "status", "info"),
        Some("pause") => cmd_one(&args[1..], "pause", "pause"),
        Some("resume") => cmd_one(&args[1..], "resume", "resume"),
        Some("cancel") => cmd_one(&args[1..], "cancel", "cancel"),
        Some("retry") => cmd_one(&args[1..], "retry", "retry"),
        Some("rm") => cmd_rm(&args[1..]),
        Some("clear") => cmd_clear(),
        Some("status") => cmd_status(),
        Some("shutdown") => cmd_shutdown(),
        _ => usage(),
    }
}

fn usage() -> Result<(), String> {
    println!(
        "fetchline — queue downloads through fetchlined\n\
         \n\
         usage:\n  \
         fetchline add <url>... [--connections N] [--dir PATH] [--at EPOCH]\n  \
         fetchline list [--status S]\n  \
         fetchline info <id>\n  \
         fetchline pause|resume|cancel|retry <id>\n  \
         fetchline rm <id> [--delete-files]\n  \
         fetchline clear | status | shutdown"
    );
    Ok(())
}

/// Call the daemon, spawning it first when the socket is absent.
fn call(req: &serde_json::Value) -> Result<serde_json::Value, String> {
    let dir = ipc::data_dir()?;
    let sock = ipc::socket_path(&dir);
    match ipc::call(&sock, req) {
        Ok(v) => Ok(v),
        Err(_) => {
            spawn_daemon()?;
            ipc::call(&sock, req)
        }
    }
}

fn spawn_daemon() -> Result<(), String> {
    // Prefer a daemon next to this binary (dev target/ + installs),
    // then ~/.local/bin.
    let mut candidates = Vec::new();
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            candidates.push(dir.join("fetchlined"));
        }
    }
    if let Ok(home) = ipc::home_dir() {
        candidates.push(home.join(".local").join("bin").join("fetchlined"));
    }
    let bin = candidates.into_iter().find(|p| p.is_file()).ok_or(
        "fetchlined not found next to fetchline or in ~/.local/bin",
    )?;
    std::process::Command::new(&bin)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .map_err(|e| format!("cannot start {}: {e}", bin.display()))?;
    let sock = ipc::socket_path(&ipc::data_dir()?);
    for _ in 0..50 {
        if sock.exists() {
            // Give accept() a moment to come up.
            std::thread::sleep(Duration::from_millis(50));
            return Ok(());
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    Err("fetchlined did not come up".into())
}

fn check(resp: serde_json::Value) -> Result<serde_json::Value, String> {
    if resp.get("ok").and_then(|v| v.as_bool()).unwrap_or(false) {
        Ok(resp)
    } else {
        Err(resp
            .get("error")
            .and_then(|v| v.as_str())
            .unwrap_or("daemon error")
            .to_string())
    }
}

fn cmd_add(args: &[String]) -> Result<(), String> {
    let mut urls = Vec::new();
    let mut connections = 8u64;
    let mut dir: Option<String> = None;
    let mut at: Option<i64> = None;
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--connections" => {
                i += 1;
                connections = args.get(i).and_then(|v| v.parse().ok()).unwrap_or(8).clamp(1, 16);
            }
            "--dir" => {
                i += 1;
                dir = args.get(i).cloned();
            }
            "--at" => {
                i += 1;
                at = args.get(i).and_then(|v| v.parse().ok());
            }
            u => urls.push(u.to_string()),
        }
        i += 1;
    }
    if urls.is_empty() {
        return Err("add needs at least one url".into());
    }
    if urls.len() > 100 {
        return Err("batch limit is 100 urls".into());
    }
    for url in urls {
        let mut req = serde_json::json!({"action": "add", "url": url, "connections": connections});
        if let Some(d) = &dir {
            req["dir"] = d.as_str().into();
        }
        if let Some(t) = at {
            req["scheduled_at"] = t.into();
        }
        match check(call(&req)?) {
            Ok(r) => println!("queued {} {}", r["id"], url),
            Err(e) => println!("skip {url}: {e}"),
        }
    }
    Ok(())
}

fn cmd_list(args: &[String]) -> Result<(), String> {
    let mut status: Option<String> = None;
    let mut i = 0;
    while i < args.len() {
        if args[i] == "--status" {
            i += 1;
            status = args.get(i).cloned();
        }
        i += 1;
    }
    let mut req = serde_json::json!({"action": "list"});
    if let Some(s) = status {
        req["status"] = s.into();
    }
    let resp = check(call(&req)?)?;
    let items = resp["downloads"].as_array().cloned().unwrap_or_default();
    if items.is_empty() {
        println!("empty");
        return Ok(());
    }
    println!("{:<6} {:<10} {:<21} NAME", "ID", "STATUS", "PROGRESS");
    for d in items {
        let total = d["totalBytes"].as_u64();
        let done = d["downloadedBytes"].as_u64().unwrap_or(0);
        let prog = match total {
            Some(t) => format!("{}/{}", human(done), human(t)),
            None => human(done),
        };
        println!(
            "{:<6} {:<10} {:<21} {}",
            d["id"].as_i64().unwrap_or(-1),
            d["status"].as_str().unwrap_or("?"),
            prog,
            d["filename"].as_str().unwrap_or("?")
        );
    }
    Ok(())
}

/// Generic single-id command: info/pause/resume/cancel/retry.
fn cmd_one(args: &[String], action: &str, verb: &str) -> Result<(), String> {
    let id: i64 = args.first().and_then(|v| v.parse().ok()).ok_or(format!("{verb} needs an id"))?;
    let resp = check(call(&serde_json::json!({"action": action, "id": id}))?)?;
    if action == "status" {
        let d = &resp["download"];
        let str_field = |k: &str| d[k].as_str().unwrap_or("?");
        println!("id:         {}", d["id"].as_i64().unwrap_or(-1));
        println!("status:     {}", str_field("status"));
        println!("filename:   {}", str_field("filename"));
        println!("url:        {}", str_field("url"));
        println!("output:     {}", str_field("output"));
        println!(
            "progress:   {}/{}",
            human(d["downloadedBytes"].as_u64().unwrap_or(0)),
            d["totalBytes"].as_u64().map(human).unwrap_or_else(|| "?".into())
        );
        println!("connections: {}", d["connections"].as_u64().unwrap_or(0));
        if let Some(e) = d["error"].as_str() {
            println!("error:      {e}");
        }
    } else {
        println!("{verb} {id}: ok");
    }
    Ok(())
}

fn cmd_rm(args: &[String]) -> Result<(), String> {
    let id: i64 = args.first().and_then(|v| v.parse().ok()).ok_or("rm needs an id")?;
    let delete_files = args.iter().any(|a| a == "--delete-files");
    check(call(&serde_json::json!({"action": "remove", "id": id, "delete_files": delete_files}))?)?;
    println!("removed {id}");
    Ok(())
}

fn cmd_clear() -> Result<(), String> {
    let resp = check(call(&serde_json::json!({"action": "clear"}))?)?;
    println!("cleared {}", resp["removed"]);
    Ok(())
}

fn cmd_status() -> Result<(), String> {
    let resp = check(call(&serde_json::json!({"action": "list"}))?)?;
    let items = resp["downloads"].as_array().cloned().unwrap_or_default();
    let mut counts: std::collections::HashMap<String, usize> = Default::default();
    for d in &items {
        *counts.entry(d["status"].as_str().unwrap_or("?").to_string()).or_default() += 1;
    }
    println!("daemon: up ({} tracked)", items.len());
    let mut keys: Vec<_> = counts.keys().collect();
    keys.sort();
    for k in keys {
        println!("  {k}: {}", counts[k]);
    }
    Ok(())
}

fn cmd_shutdown() -> Result<(), String> {
    check(call(&serde_json::json!({"action": "shutdown"}))?)?;
    println!("daemon shutting down");
    Ok(())
}

fn human(n: u64) -> String {
    const UNITS: &[&str] = &["B", "KiB", "MiB", "GiB"];
    let mut v = n as f64;
    let mut u = 0;
    while v >= 1024.0 && u + 1 < UNITS.len() {
        v /= 1024.0;
        u += 1;
    }
    if u == 0 {
        format!("{n} B")
    } else {
        format!("{v:.1} {}", UNITS[u])
    }
}