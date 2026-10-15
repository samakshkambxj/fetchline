<!-- Copyright 2026 Samaksh Kamboj - SPDX-License-Identifier: Apache-2.0 -->
# fetchline — Roadmap

Goal: Linux-native IDM alternative. Segmented downloads, browser capture,
video extraction. No TCP listener, no Windows dependency.

Stack: Rust core + Tauri 2 + SQLite + plain HTML/CSS/JS UI.
Extensions: Chromium (MV3) + Firefox (shared codebase).

Commit style: Conventional Commits (`feat:`, `fix:`, `docs:`, `chore:`,
`test:`, `build:`). One logical change per commit.

---

## M0 — Scaffold (done)

- [x] Repo `samakshkambxj/fetchline`, Apache-2.0, `.gitignore`
- [x] Layout: `src-tauri/ ui/ browser/ scripts/ docs/`

## M1 — Core engine (v0.1.0) — done

`crates/fetchline-core/` — 15 tests pass (`cargo test -p fetchline-core`).

- [x] HEAD probe: length, `Accept-Ranges`, filename from `Content-Disposition`
- [x] Segmented GET 1–16 parts, `Range` requests, preallocated file + offset writes
- [x] Single-connection fallback when Range unsupported
- [x] Pause/resume via `.state.json` per download (500ms sync thread)
- [x] Retry with exponential backoff, per-segment error isolation
- [x] Global + per-download speed limiter (token bucket)
- [x] SHA-256 verify opt-in
- [x] Unit tests: local Range-capable test server, resume, fallback

Accept: 100MB file from local server downloads in N segments,
byte-identical, pause at 50% resumes correctly.

## M2 — Queue + persistence + CLI (v0.2.0) — done

`fetchlined` + `fetchline`, verified end-to-end (isolated HOME):
scheduled add → pause → resume → 2 MiB completed byte-identical,
status/clear/list/shutdown. Socket `0600`, 1 MiB cap.

- [x] SQLite schema: downloads, resume jobs, settings (`rusqlite` bundled)
- [x] States: queued → active → paused → completed / failed / cancelled
- [x] Batch add (100 URLs), schedule-at, retry-failed
- [x] Categories: Video/Music/Images/Docs/Apps/Archives/ISO/Other under
      `~/Downloads/fetchline/`, respect `XDG_DOWNLOAD_DIR`
- [x] CLI: `fetchline add/list/pause/resume/cancel/clear/info`
- [x] Unix-socket IPC `~/.local/share/fetchline/control.sock` (`0600`),
      1MB cap, action envelope `{action, id, ...}`

- [ ] SQLite schema: downloads, resume jobs, settings (`rusqlite` bundled)
- [ ] States: queued → active → paused → completed / failed / cancelled
- [ ] Batch add (100 URLs), schedule-at, retry-failed
- [ ] Categories: Video/Music/Images/Docs/Apps/Archives/ISO/Other under
      `~/Downloads/fetchline/`, respect `XDG_DOWNLOAD_DIR`
- [ ] CLI: `fetchline add/list/pause/resume/cancel/clear/info`
- [ ] Unix-socket IPC `~/.local/share/fetchline/control.sock` (`0600`),
      1MB cap, action envelope `{action, id, ...}`

Accept: reboot survives queue, CLI controls daemon, socket perms `0600`.

## M3 — Desktop UI (v0.3.0)

`ui/` — plain HTML/CSS/JS, three windows only.

- [ ] `main`: library, search/sort/filter, progress, speed/ETA, tray
- [ ] `add-download`: single + batch paste
- [ ] `detail`: per-connection bars, log, verify status
- [ ] `settings`: connections, speed limit, categories, autostart
- [ ] Theme follows system, notifications, start-at-login, hide-to-tray

Accept: all CLI ops doable in GUI, close hides to tray, quit from tray only.

## M4 — Browser integration (v0.4.0)

- [ ] `browser_native_host` stdio binary + manifests (Chrome/Brave/Edge/Vivaldi + Firefox)
- [ ] Shared extension core: context menu, link grabber, video-button injection
- [ ] Capture rules: extension allowlist + excluded sites (validated)
- [ ] Popup: recent transfers + pause/resume/cancel via socket
- [ ] Files under `~/.local/share/fetchline/extension/` (never `~/Documents/`)

Accept: click video → appears in app queue; popup controls work;
Firefox signed build installs persistently.

## M5 — Video (v0.5.0)

- [ ] yt-dlp wrapper: pinned version, checksum-verified fetch to `~/.local/bin`
- [ ] Quality menu (resolution/container/audio-only), ffmpeg mux
- [ ] HLS fragment concurrency + incomplete-output detection
- [ ] DoH fallback opt-in only (default off), documented privacy note

Accept: YouTube 1080p + audio-only both produce correct duration files.

## M6 — Packaging + hardening (v0.6.0)

- [ ] `install.sh` per-user (`~/.local/bin`), build-from-source + prebuilt modes
- [ ] AppImage + tar.gz on Ubuntu 22.04, `SHA256SUMS`, atomic install + backup
- [ ] Runtime-dep check (GTK3/WebKitGTK 4.1), `--check` dry run
- [ ] `scripts/`: installer test, extension settings test, transfer regression
- [ ] Security pass: socket perms, 1MB caps, header validation, no creds in logs

Accept: fresh Ubuntu 22.04 + Fedora 40 install from AppImage and from source.

## M7 — v1.0

- [ ] Flatpak manifest, aarch64 build
- [ ] Store-signed Firefox add-on, Chrome Web Store (optional)
- [ ] Docs: install guide per distro, troubleshooting, diagnostics export
- [ ] `uninstall.sh` preserves downloads/history by default

Post-1.0 (not v1): torrents, FTP/SFTP, mirror search, plugins API,
bandwidth scheduler v2, KDE/GNOME Search integration.

---

## Milestone order

M1 → M2 → M3 → M4 → M5 → M6 → M7. No skipping: each milestone's
Accept line must pass before the next starts.
