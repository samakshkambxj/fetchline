# Copyright 2026 Samaksh Kamboj - SPDX-License-Identifier: Apache-2.0
"""Local Range-capable test server for M1 accept tests. Stdlib only.

Usage:
    python3 scripts/range_server.py --size 1048576 --port 8766

Serves one deterministic payload at /file.bin:
- HEAD: Content-Length, Accept-Ranges: bytes, ETag
- GET: 200 full body, or 206 Partial Content for a single Range request
- Deterministic bytes: byte i == i % 256 (verifiable without fixtures)
"""

import argparse
import hashlib
import threading
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer


class Handler(BaseHTTPRequestHandler):
    payload: bytes = b""
    etag: str = ""

    def log_message(self, *args):  # quiet during tests
        pass

    def _headers(self, length, extra=None):
        self.send_response(200)
        self.send_header("Content-Length", str(length))
        self.send_header("Accept-Ranges", "bytes")
        self.send_header("ETag", self.etag)
        self.send_header("Content-Type", "application/octet-stream")
        for k, v in (extra or {}).items():
            self.send_header(k, v)
        self.end_headers()

    def do_HEAD(self):
        if self.path != "/file.bin":
            self.send_error(404)
            return
        self._headers(len(self.payload))

    def do_GET(self):
        if self.path != "/file.bin":
            self.send_error(404)
            return
        total = len(self.payload)
        rng = self.headers.get("Range")
        if not rng:
            self._headers(total)
            self.wfile.write(self.payload)
            return
        try:
            unit, spec = rng.split("=", 1)
            assert unit == "bytes"
            start_s, end_s = spec.split("-", 1)
            start = int(start_s) if start_s else max(0, total - int(end_s))
            end = int(end_s) if end_s else total - 1
            end = min(end, total - 1)
            assert 0 <= start <= end
        except (ValueError, AssertionError):
            self.send_error(416)
            return
        chunk = self.payload[start : end + 1]
        self.send_response(206)
        self.send_header("Content-Length", str(len(chunk)))
        self.send_header("Accept-Ranges", "bytes")
        self.send_header("ETag", self.etag)
        self.send_header("Content-Type", "application/octet-stream")
        self.send_header("Content-Range", f"bytes {start}-{end}/{total}")
        self.end_headers()
        self.wfile.write(chunk)


def run(port, size):
    Handler.payload = bytes(i % 256 for i in range(size))
    Handler.etag = f'"{hashlib.sha256(Handler.payload).hexdigest()[:16]}"'
    server = ThreadingHTTPServer(("127.0.0.1", port), Handler)
    thread = threading.Thread(target=server.serve_forever, daemon=True)
    thread.start()
    return server


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--port", type=int, default=8766)
    parser.add_argument("--size", type=int, default=1048576)
    args = parser.parse_args()
    server = run(args.port, args.size)
    print(f"serving {args.size} bytes at http://127.0.0.1:{args.port}/file.bin")
    try:
        server.serve_forever()
    except KeyboardInterrupt:
        pass


if __name__ == "__main__":
    main()
