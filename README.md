<!-- Copyright 2026 Samaksh Kamboj - SPDX-License-Identifier: Apache-2.0 -->
# fetchline

Linux-first download manager. IDM-style segmented downloads, browser capture, and video extraction — without the Windows dependency.

Status: scaffold. See `docs/ROADMAP.md` for v1 plan.

## Layout

- `src-tauri/` — Rust core: engine, queue, IPC, native host
- `ui/` — Tauri frontend (plain HTML/CSS/JS)
- `browser/` — Chromium + Firefox extensions
- `scripts/` — packaging, tests, dev tooling
- `docs/` — roadmap, install guides
