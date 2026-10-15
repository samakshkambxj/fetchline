// Copyright 2026 Samaksh Kamboj - SPDX-License-Identifier: Apache-2.0
// Backend bridge. MockBackend serves the static preview; the Tauri build
// swaps in TauriBackend (same method surface) without touching app.js.
class MockBackend {
  constructor() {
    this.items = [
      { id: 1, filename: "cachyos-desktop.iso", status: "active", downloadedBytes: 1308622848, totalBytes: 2147483648, connections: 8 },
      { id: 2, filename: "paper.pdf", status: "paused", downloadedBytes: 524288, totalBytes: 2097152, connections: 4 },
      { id: 3, filename: "talk.webm", status: "queued", downloadedBytes: 0, totalBytes: null, connections: 8, scheduledAt: Date.now() / 1000 + 3600 },
      { id: 4, filename: "album.zip", status: "completed", downloadedBytes: 104857600, totalBytes: 104857600, connections: 8 },
      { id: 5, filename: "mirror.tar.gz", status: "failed", downloadedBytes: 12345678, totalBytes: 987654321, connections: 8, error: "connection reset" },
    ];
    // Demo liveliness: active item creeps forward until full.
    setInterval(() => {
      const a = this.items.find((d) => d.status === "active");
      if (a && a.totalBytes) {
        a.downloadedBytes = Math.min(a.totalBytes, a.downloadedBytes + a.totalBytes / 120);
        document.dispatchEvent(new CustomEvent("fetchline:update"));
      }
    }, 500);
  }
  async list() { return this.items.map((d) => ({ ...d })); }
  async add(urls, connections) {
    for (const url of urls.slice(0, 100)) {
      const name = url.split(/[?#]/)[0].split("/").pop() || "download.bin";
      this.items.unshift({ id: Math.max(...this.items.map((d) => d.id)) + 1, filename: name, status: "queued", downloadedBytes: 0, totalBytes: null, connections });
    }
    document.dispatchEvent(new CustomEvent("fetchline:update"));
  }
  async act(id, action) {
    const d = this.items.find((x) => x.id === id);
    if (!d) return;
    if (action === "pause" && d.status === "active") d.status = "paused";
    if (action === "resume" && d.status === "paused") d.status = "queued";
    if (action === "cancel") d.status = "cancelled";
    if (action === "retry" && (d.status === "failed" || d.status === "cancelled")) d.status = "queued";
    if (action === "remove") this.items = this.items.filter((x) => x.id !== id);
    document.dispatchEvent(new CustomEvent("fetchline:update"));
  }
}
window.fetchlineBackend = new MockBackend();
