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
// TauriBackend speaks to fetchlined through the app shell's invoke
// handlers (src-tauri/src/proxy.rs). Same method surface as MockBackend.
// Detection covers every known Tauri webview surface:
// - Tauri v2: window.__TAURI_INTERNALS__.invoke
// - Tauri v1 compat: window.__TAURI_INVOKE__
// - Namespace object, if populated: window.__TAURI__.core.invoke
function tauriInvokeFn() {
  if (typeof window.__TAURI_INVOKE__ === "function") {
    return (cmd, args) => window.__TAURI_INVOKE__(cmd, args);
  }
  const internals = window.__TAURI_INTERNALS__;
  if (internals && typeof internals.invoke === "function") {
    return (cmd, args) => internals.invoke(cmd, args);
  }
  const ns = window.__TAURI__;
  if (ns && ns.core && typeof ns.core.invoke === "function") {
    return (cmd, args) => ns.core.invoke(cmd, args);
  }
  return null;
}
class TauriBackend {
  constructor(invoke) {
    this.invoke = invoke;
    // No push channel from the daemon yet; poll for progress.
    setInterval(() => document.dispatchEvent(new CustomEvent("fetchline:update")), 1000);
  }
  async list() {
    const r = await this.invoke("dl_list", { status: null });
    return (r.downloads || []).map((d) => ({
      id: d.id,
      filename: d.filename,
      status: d.status,
      downloadedBytes: d.downloadedBytes || 0,
      totalBytes: d.totalBytes,
      connections: d.connections,
      scheduledAt: d.scheduledAt,
      error: d.error,
    }));
  }
  async add(urls, connections) {
    await this.invoke("dl_add", { urls: urls.slice(0, 100), connections });
    document.dispatchEvent(new CustomEvent("fetchline:update"));
  }
  async act(id, action) {
    await this.invoke("dl_act", { action, id });
    document.dispatchEvent(new CustomEvent("fetchline:update"));
  }
}
const tauriInvoke = tauriInvokeFn();
if (tauriInvoke) {
  window.fetchlineBackend = new TauriBackend(tauriInvoke);
} else {
  window.fetchlineBackend = new MockBackend();
}
