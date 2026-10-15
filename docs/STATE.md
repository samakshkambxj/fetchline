<!-- Copyright 2026 Samaksh Kamboj - SPDX-License-Identifier: Apache-2.0 -->
# Download state (`state.json`)

One JSON sidecar per download: `<output>.state.json`.
Engine writes atomically (tmp + rename). Resume reads it.

```json
{
  "version": 1,
  "url": "https://example.com/file.iso",
  "output": "/home/user/Downloads/fetchline/ISO/file.iso",
  "total_bytes": 104857600,
  "connections": 8,
  "segments": [
    { "index": 0, "start": 0, "end": 13107199, "done": 13107200 },
    { "index": 1, "start": 13107200, "end": 26214399, "done": 0 }
  ],
  "etag": "\"abc123\"",
  "supports_ranges": true,
  "sha256": null
}
```

Rules:

- `version` must be `1`. Unknown versions refuse resume (fresh download).
- `segments[i].done` counts completed bytes within `[start, end]`.
  Complete when `done == end - start + 1` for all segments.
- Resume revalidates with `If-Range: <etag>` or `If-Unmodified-Since`.
  Mismatch discards state and restarts (never stitches stale bytes).
- Single-connection downloads use one segment covering `[0, total-1]`.
- Unknown total (`total_bytes: null`) is not resumable; state keeps
  `done` bytes only for display.
- State file is deleted on verified completion.
