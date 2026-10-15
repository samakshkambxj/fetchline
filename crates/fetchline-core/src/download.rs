// Copyright 2026 Samaksh Kamboj - SPDX-License-Identifier: Apache-2.0
//! Segmented downloader: HEAD probe, parallel Range fetch, resume.

use std::fs::{self, File, OpenOptions};
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::Duration;

use sha2::{Digest, Sha256};

use std::sync::Arc;

use crate::limit::Limiter;
use crate::probe::{ProbeResult, filename_from_disposition, filename_from_url};
use crate::segments::plan;
use crate::state::DownloadState;

const MAX_RETRIES: u32 = 3;

/// Blocking download engine. One instance per process is enough;
/// the underlying client pools connections.
pub struct Engine {
    client: reqwest::blocking::Client,
    pub(crate) sync_interval: Duration,
    limiter: Option<Arc<Limiter>>,
    control: Option<Arc<Control>>,
}

impl Engine {
    pub fn new() -> Result<Self, String> {
        Self::with_sync_interval(Duration::from_millis(500))
    }

    /// Cap total throughput (all segments combined) at `bytes_per_sec`.
    /// `0` means unlimited.
    pub fn with_speed_limit(bytes_per_sec: u64) -> Result<Self, String> {
        let mut engine = Self::new()?;
        if bytes_per_sec > 0 {
            engine.limiter = Some(Arc::new(Limiter::new(bytes_per_sec)));
        }
        Ok(engine)
    }

    pub fn with_sync_interval(sync_interval: Duration) -> Result<Self, String> {
        let client = reqwest::blocking::Client::builder()
            .timeout(Duration::from_secs(30))
            .user_agent(concat!("fetchline/", env!("CARGO_PKG_VERSION")))
            .build()
            .map_err(|e| e.to_string())?;
        Ok(Self { client, sync_interval, limiter: None, control: None })
    }

    /// Cooperative pause/cancel shared with the caller (the daemon).
    pub fn with_control(&mut self, control: Arc<Control>) {
        self.control = Some(control);
    }

    /// HEAD probe: length, Range support, ETag, filename.
    pub fn probe(&self, url: &str) -> Result<ProbeResult, String> {
        let resp = self.client.head(url).send().map_err(|e| e.to_string())?;
        if !resp.status().is_success() {
            return Err(format!("HEAD {} -> {}", url, resp.status()));
        }
        let total_bytes = resp
            .headers()
            .get(reqwest::header::CONTENT_LENGTH)
            .and_then(|v| v.to_str().ok())
            .and_then(|s| s.parse::<u64>().ok());
        let supports_ranges = resp
            .headers()
            .get(reqwest::header::ACCEPT_RANGES)
            .and_then(|v| v.to_str().ok())
            .map(|s| s.eq_ignore_ascii_case("bytes"))
            .unwrap_or(false);
        let etag = resp
            .headers()
            .get(reqwest::header::ETAG)
            .and_then(|v| v.to_str().ok())
            .map(|s| s.to_string());
        let filename = resp
            .headers()
            .get(reqwest::header::CONTENT_DISPOSITION)
            .and_then(|v| v.to_str().ok())
            .and_then(filename_from_disposition)
            .unwrap_or_else(|| filename_from_url(url));
        Ok(ProbeResult { url: url.to_string(), total_bytes, supports_ranges, etag, filename })
    }

    /// Fresh download. Writes `<output>.state.json` atomically during
    /// transfer and deletes it on verified completion.
    pub fn download(
        &self,
        url: &str,
        output: &Path,
        connections: u64,
        sha256: Option<&str>,
    ) -> Result<DownloadState, String> {
        let probe = self.probe(url)?;
        let segments = plan(probe.total_bytes, connections, probe.supports_ranges);
        let mut state = DownloadState::fresh(
            url,
            &output.to_string_lossy(),
            probe.total_bytes,
            connections,
            &segments,
            probe.etag,
            probe.supports_ranges,
        );
        self.run_segments(url, output, &mut state)?;
        self.finish(output, &state, sha256)?;
        Ok(state)
    }

    /// Resume from a previously saved state file.
    pub fn resume(&self, state_path: &Path) -> Result<DownloadState, String> {
        let raw = fs::read_to_string(state_path).map_err(|e| e.to_string())?;
        let mut state: DownloadState = serde_json::from_str(&raw).map_err(|e| e.to_string())?;
        state.validate()?;
        // Revalidate freshness: restart instead of stitching stale bytes.
        let probe = self.probe(&state.url)?;
        if probe.total_bytes != state.total_bytes || probe.etag != state.etag {
            let segments = plan(probe.total_bytes, state.connections, probe.supports_ranges);
            state = DownloadState::fresh(
                &state.url.clone(),
                &state.output.clone(),
                probe.total_bytes,
                state.connections,
                &segments,
                probe.etag,
                probe.supports_ranges,
            );
        }
        let output = PathBuf::from(state.output.clone());
        self.run_segments(&state.url.clone(), &output, &mut state)?;
        let sha = state.sha256.clone();
        self.finish(&output, &state, sha.as_deref())?;
        Ok(state)
    }

    fn run_segments(
        &self,
        url: &str,
        output: &Path,
        state: &mut DownloadState,
    ) -> Result<(), String> {
        if let Some(total) = state.total_bytes {
            preallocate(output, total)?;
        } else if !output.exists() {
            File::create(output).map_err(|e| e.to_string())?;
        }
        let state_path = state_path_for(output);
        // Per-segment byte counters shared with worker threads.
        // `AtomicU64` is `Sync`, so scoped threads may borrow them.
        let dones: Vec<AtomicU64> =
            state.segments.iter().map(|s| AtomicU64::new(s.done)).collect();
        // Crash-pause support: snapshot progress to state.json every
        // `sync_interval` so killing the process mid-transfer still resumes.
        let sync_stop = AtomicBool::new(false);
        let snapshot = state.clone();
        std::thread::scope(|scope| {
            let sync_handle = scope.spawn(|| {
                while !sync_stop.load(Ordering::Relaxed) {
                    std::thread::sleep(self.sync_interval);
                    let mut snap = snapshot.clone();
                    for (seg, done) in snap.segments.iter_mut().zip(&dones) {
                        seg.done = done.load(Ordering::Relaxed);
                    }
                    let _ = write_state_file(&state_path, &snap);
                }
            });
            let mut handles = Vec::new();
            for (i, seg) in state.segments.iter().enumerate() {
                let from = seg.start + seg.done;
                if from > seg.end {
                    continue; // already complete (resume case)
                }
                let client = self.client.clone();
                let limiter = self.limiter.clone();
                let control = self.control.clone();
                let url = url.to_string();
                let output = output.to_path_buf();
                let end = seg.end;
                let done = &dones[i];
                handles.push(scope.spawn(move || {
                    fetch_segment(
                        &client,
                        &url,
                        &output,
                        from,
                        end,
                        done,
                        limiter.as_deref(),
                        control.as_deref(),
                    )
                }));
            }
            let mut first_err: Option<String> = None;
            for h in handles {
                if let Err(e) = h.join().map_err(|_| "segment thread panicked".to_string())? {
                    if first_err.is_none() {
                        first_err = Some(e);
                    }
                }
            }
            sync_stop.store(true, Ordering::Relaxed);
            let _ = sync_handle.join();
            if let Some(e) = first_err {
                return Err(e);
            }
            Ok(())
        })?;
        for (seg, done) in state.segments.iter_mut().zip(&dones) {
            seg.done = done.load(Ordering::Relaxed);
        }
        // Persist resume state atomically for the next run (or crash).
        write_state_file(&state_path, state)?;
        if !state.is_complete() {
            return Err("transfer incomplete after all segments".into());
        }
        Ok(())
    }

    fn finish(
        &self,
        output: &Path,
        state: &DownloadState,
        sha256: Option<&str>,
    ) -> Result<(), String> {
        if let Some(expected) = sha256 {
            let actual = sha256_of(output)?;
            if !actual.eq_ignore_ascii_case(expected) {
                return Err(format!("sha256 mismatch: got {actual}, want {expected}"));
            }
        }
        let _ = fs::remove_file(state_path_for(output));
        let _ = state;
        Ok(())
    }
}

impl Default for Engine {
    fn default() -> Self {
        Self::new().expect("engine client builds")
    }
}

/// Cooperative transfer control. The daemon flips these flags;
/// workers check them after every 64 KiB chunk.
#[derive(Debug, Default)]
pub struct Control {
    paused: AtomicBool,
    cancelled: AtomicBool,
}

/// Sentinel errors (matched with `==`, never shown raw to users).
pub const PAUSED: &str = "transfer paused";
pub const CANCELLED: &str = "transfer cancelled";

impl Control {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn pause(&self) {
        self.paused.store(true, Ordering::Relaxed);
    }

    pub fn cancel(&self) {
        self.cancelled.store(true, Ordering::Relaxed);
    }

    fn check(&self) -> Result<(), String> {
        if self.cancelled.load(Ordering::Relaxed) {
            return Err(CANCELLED.into());
        }
        if self.paused.load(Ordering::Relaxed) {
            return Err(PAUSED.into());
        }
        Ok(())
    }
}

pub fn state_path_for(output: &Path) -> PathBuf {
    let mut name = output.as_os_str().to_owned();
    name.push(".state.json");
    PathBuf::from(name)
}

/// Atomic state write (tmp + rename) so a crash never leaves half JSON.
pub(crate) fn write_state_file(path: &Path, state: &DownloadState) -> Result<(), String> {
    let tmp = path.with_extension("json.tmp");
    let raw = serde_json::to_string(state).map_err(|e| e.to_string())?;
    fs::write(&tmp, raw).map_err(|e| e.to_string())?;
    fs::rename(&tmp, path).map_err(|e| e.to_string())?;
    Ok(())
}

fn preallocate(path: &Path, total: u64) -> Result<(), String> {
    let file = OpenOptions::new()
        .create(true)
        .truncate(true)
        .write(true)
        .open(path)
        .map_err(|e| e.to_string())?;
    file.set_len(total).map_err(|e| e.to_string())?;
    Ok(())
}

#[allow(clippy::too_many_arguments)] // one internal caller; a struct adds noise
fn fetch_segment(
    client: &reqwest::blocking::Client,
    url: &str,
    output: &Path,
    start: u64,
    end: u64,
    done: &AtomicU64,
    limiter: Option<&Limiter>,
    control: Option<&Control>,
) -> Result<(), String> {
    let mut attempt = 0;
    let mut offset = start;
    while offset <= end {
        let range = format!("bytes={offset}-{end}");
        let resp = client.get(url).header(reqwest::header::RANGE, range).send();
        match resp {
            Ok(r) if r.status().is_success() || r.status().as_u16() == 206 => {
                let mut file =
                    OpenOptions::new().write(true).open(output).map_err(|e| e.to_string())?;
                file.seek(SeekFrom::Start(offset)).map_err(|e| e.to_string())?;
                let mut stream = r;
                let mut buf = [0u8; 65536];
                loop {
                    let n = stream.read(&mut buf).map_err(|e| e.to_string())?;
                    if n == 0 {
                        break;
                    }
                    file.write_all(&buf[..n]).map_err(|e| e.to_string())?;
                    offset += n as u64;
                    done.fetch_add(n as u64, Ordering::Relaxed);
                    if let Some(lim) = limiter {
                        lim.consume(n as u64);
                    }
                    if let Some(ctrl) = control {
                        ctrl.check()?;
                    }
                }
                if offset > end + 1 {
                    return Err("server sent more than requested".into());
                }
                return Ok(());
            }
            Ok(r) => {
                attempt += 1;
                if attempt > MAX_RETRIES {
                    return Err(format!("segment {start}-{end}: HTTP {}", r.status()));
                }
            }
            Err(e) => {
                attempt += 1;
                if attempt > MAX_RETRIES {
                    return Err(format!("segment {start}-{end}: {e}"));
                }
                std::thread::sleep(Duration::from_millis(200 * attempt as u64));
            }
        }
    }
    Ok(())
}

fn sha256_of(path: &Path) -> Result<String, String> {
    let mut file = File::open(path).map_err(|e| e.to_string())?;
    let mut hasher = Sha256::new();
    let mut buf = [0u8; 65536];
    loop {
        let n = file.read(&mut buf).map_err(|e| e.to_string())?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    Ok(format!("{:x}", hasher.finalize()))
}

#[cfg(test)]
mod integration {
    use super::*;
    use std::process::{Child, Command};
    use std::sync::Arc;

    fn start_server(port: u16, size: usize) -> Child {
        let script = concat!(env!("CARGO_MANIFEST_DIR"), "/../../scripts/range_server.py");
        Command::new("python3")
            .arg(script)
            .arg("--port")
            .arg(port.to_string())
            .arg("--size")
            .arg(size.to_string())
            .spawn()
            .expect("range_server starts")
    }

    fn wait_ready(port: u16) {
        let client = reqwest::blocking::Client::builder()
            .timeout(Duration::from_secs(2))
            .build()
            .unwrap();
        for _ in 0..50 {
            if client.head(format!("http://127.0.0.1:{port}/file.bin")).send().is_ok() {
                return;
            }
            std::thread::sleep(Duration::from_millis(100));
        }
        panic!("test server never came up");
    }

    #[test]
    fn segmented_download_byte_identical() {
        let port = 18766;
        let size = 1_048_576usize;
        let mut server = start_server(port, size);
        wait_ready(port);
        let dir = std::env::temp_dir().join("fetchline-m1");
        let _ = fs::create_dir_all(&dir);
        let out = dir.join("seg.bin");
        let _ = fs::remove_file(&out);
        let _ = fs::remove_file(state_path_for(&out));
        let engine = Engine::new().unwrap();
        let state = engine
            .download(&format!("http://127.0.0.1:{port}/file.bin"), &out, 8, None)
            .unwrap();
        assert!(state.is_complete());
        let data = fs::read(&out).unwrap();
        assert_eq!(data.len(), size);
        assert!(data.iter().enumerate().all(|(i, b)| *b == (i % 256) as u8));
        assert!(!state_path_for(&out).exists(), "state cleaned up");
        let _ = fs::remove_file(&out);
        server.kill().ok();
        let _ = server.wait();
    }

    #[test]
    fn state_snapshot_round_trips() {
        use crate::segments::plan;
        let segs = plan(Some(1000), 4, true);
        let mut st = DownloadState::fresh("u", "o", Some(1000), 4, &segs, None, true);
        st.segments[0].done = 250;
        let path = std::env::temp_dir().join("fetchline-snap.state.json");
        write_state_file(&path, &st).unwrap();
        let back: DownloadState =
            serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
        assert!(back.validate().is_ok());
        assert_eq!(back.segments[0].done, 250);
        assert!(!back.is_complete());
        let _ = fs::remove_file(&path);
    }

    #[test]
    fn resume_completes_partial_state() {
        let port = 18767;
        let size = 262_144usize;
        let mut server = start_server(port, size);
        wait_ready(port);
        let dir = std::env::temp_dir().join("fetchline-m1");
        let _ = fs::create_dir_all(&dir);
        let out = dir.join("resume.bin");
        let _ = fs::remove_file(&out);
        // Simulate an interrupted 2-connection download: seg0 done, seg1 empty.
        let engine = Engine::new().unwrap();
        let probe = engine.probe(&format!("http://127.0.0.1:{port}/file.bin")).unwrap();
        let segments = plan(probe.total_bytes, 2, probe.supports_ranges);
        let mut state = DownloadState::fresh(
            &probe.url,
            &out.to_string_lossy(),
            probe.total_bytes,
            2,
            &segments,
            probe.etag,
            probe.supports_ranges,
        );
        preallocate(&out, size as u64).unwrap();
        // Fetch only seg0 manually via Range, record progress.
        let s0 = &segments[0];
        let done0 = AtomicU64::new(0);
        fetch_segment(
            &engine.client,
            &probe.url,
            &out,
            s0.start,
            s0.end,
            &done0,
            None,
            None,
        )
        .unwrap();
        state.segments[0].done = done0.load(Ordering::Relaxed);
        let raw = serde_json::to_string(&state).unwrap();
        fs::write(state_path_for(&out), raw).unwrap();
        // Resume must finish seg1 and verify the whole file.
        let resumed = engine.resume(&state_path_for(&out)).unwrap();
        assert!(resumed.is_complete());
        let data = fs::read(&out).unwrap();
        assert!(data.iter().enumerate().all(|(i, b)| *b == (i % 256) as u8));
        let _ = fs::remove_file(&out);
        server.kill().ok();
        let _ = server.wait();
    }

    #[test]
    fn pause_then_resume() {
        let port = 18769;
        let size = 1_048_576usize; // 1 MiB throttled to 256 KiB/s: ~4s window to pause
        let mut server = start_server(port, size);
        wait_ready(port);
        let dir = std::env::temp_dir().join("fetchline-m1");
        let _ = fs::create_dir_all(&dir);
        let out = dir.join("paused.bin");
        let _ = fs::remove_file(&out);
        let _ = fs::remove_file(state_path_for(&out));
        let mut engine = Engine::with_speed_limit(256 * 1024).unwrap();
        let ctrl = Arc::new(Control::new());
        engine.with_control(ctrl.clone());
        let url = format!("http://127.0.0.1:{port}/file.bin");
        let handle = std::thread::spawn(move || engine.download(&url, &out, 1, None));
        std::thread::sleep(Duration::from_millis(300));
        ctrl.pause();
        let err = handle.join().unwrap().unwrap_err();
        assert_eq!(err, PAUSED);
        let sp = state_path_for(&dir.join("paused.bin"));
        assert!(sp.exists(), "partial state must survive pause");
        let partial: DownloadState =
            serde_json::from_str(&fs::read_to_string(&sp).unwrap()).unwrap();
        assert!(partial.validate().is_ok() && !partial.is_complete());
        let done = Engine::new().unwrap().resume(&sp).unwrap();
        assert!(done.is_complete());
        let data = fs::read(dir.join("paused.bin")).unwrap();
        assert!(data.iter().enumerate().all(|(i, b)| *b == (i % 256) as u8));
        let _ = fs::remove_file(dir.join("paused.bin"));
        server.kill().ok();
        let _ = server.wait();
    }

    #[test]
    fn capped_download_stays_correct() {
        let port = 18768;
        let size = 262_144usize; // 256 KiB at 1 MiB/s -> fast, exercises wiring
        let mut server = start_server(port, size);
        wait_ready(port);
        let dir = std::env::temp_dir().join("fetchline-m1");
        let _ = fs::create_dir_all(&dir);
        let out = dir.join("capped.bin");
        let _ = fs::remove_file(&out);
        let _ = fs::remove_file(state_path_for(&out));
        let engine = Engine::with_speed_limit(1024 * 1024).unwrap();
        let t = std::time::Instant::now();
        let state = engine
            .download(&format!("http://127.0.0.1:{port}/file.bin"), &out, 4, None)
            .unwrap();
        assert!(state.is_complete());
        assert!(t.elapsed() < Duration::from_secs(20));
        let data = fs::read(&out).unwrap();
        assert!(data.iter().enumerate().all(|(i, b)| *b == (i % 256) as u8));
        let _ = fs::remove_file(&out);
        server.kill().ok();
        let _ = server.wait();
    }
}
