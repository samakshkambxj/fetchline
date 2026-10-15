<!-- Copyright 2026 Samaksh Kamboj - SPDX-License-Identifier: Apache-2.0 -->
# IPC envelope (Unix socket)

Path: `~/.local/share/fetchline/control.sock` (mode `0600`).
One JSON object per connection, max 1 MiB. No TCP listener.

Request:

```json
{ "action": "add", "url": "https://example.com/f.zip", "connections": 8 }
{ "action": "pause", "id": 3 }
{ "action": "resume", "id": 3 }
{ "action": "cancel", "id": 3 }
{ "action": "status", "id": 3 }
{ "action": "list" }
```

Response:

```json
{ "ok": true, "id": 3 }
{ "ok": false, "error": "unknown download id" }
```

Rules:

- Unknown `action` returns `ok: false`, never panics the daemon.
- `connections` clamps to 1–16; omitted means auto (engine decides,
  default 8, single when server lacks Range support).
- Payloads over 1 MiB are rejected before parsing.
