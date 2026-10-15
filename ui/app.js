// Copyright 2026 Samaksh Kamboj - SPDX-License-Identifier: Apache-2.0
// fetchline main window: views, cards, sheet, settings. Backend-agnostic
// (talks only to window.fetchlineBackend).
(function () {
  "use strict";
  const $ = (s) => document.querySelector(s);
  const backend = () => window.fetchlineBackend;
  let items = [];
  let view = "downloads";

  const VIEWS = { downloads: "Downloads", scheduled: "Scheduled", finished: "Finished", settings: "Settings" };

  function human(n) {
    if (n == null) return "?";
    const units = ["B", "KiB", "MiB", "GiB"];
    let v = n, u = 0;
    while (v >= 1024 && u + 1 < units.length) { v /= 1024; u++; }
    return u === 0 ? `${n} B` : `${v.toFixed(1)} ${units[u]}`;
  }

  function iconFor(name) {
    const ext = (name.split(".").pop() || "").toLowerCase();
    if (["mp4", "mkv", "webm", "avi", "mov"].includes(ext)) return "▶";
    if (["mp3", "m4a", "flac", "wav", "ogg"].includes(ext)) return "♪";
    if (["png", "jpg", "jpeg", "gif", "webp", "svg"].includes(ext)) return "◈";
    if (["iso", "img"].includes(ext)) return "◉";
    if (["zip", "rar", "tar", "gz", "7z"].includes(ext)) return "▤";
    return "⭳";
  }

  function actionsFor(d) {
    const b = (act, label, cls) => `<button class="btn ${cls}" data-act="${act}" data-id="${d.id}">${label}</button>`;
    switch (d.status) {
      case "active": return b("pause", "Pause", "tonal");
      case "paused": return b("resume", "Resume", "filled") + b("cancel", "Cancel", "text");
      case "queued": return b("cancel", "Cancel", "text");
      case "failed":
      case "cancelled": return b("retry", "Retry", "filled") + b("remove", "Remove", "text");
      case "completed": return b("remove", "Remove", "text");
      default: return "";
    }
  }

  function card(d) {
    const pct = d.totalBytes ? Math.min(100, (d.downloadedBytes / d.totalBytes) * 100) : 0;
    const sub = d.totalBytes
      ? `${human(d.downloadedBytes)} of ${human(d.totalBytes)} · ${d.connections} connections`
      : d.scheduledAt
        ? `starts ${new Date(d.scheduledAt * 1000).toLocaleTimeString()}`
        : `${d.connections} connections · size unknown`;
    const err = d.error ? `<div class="dl-sub">Error: ${d.error}</div>` : "";
    return `<article class="dl-card" data-id="${d.id}">
      <div class="dl-top">
        <div class="dl-icon" aria-hidden="true">${iconFor(d.filename)}</div>
        <div style="min-width:0"><div class="dl-name">${d.filename}</div><div class="dl-sub">${sub}</div></div>
        <span class="chip st-${d.status}">${d.status}</span>
      </div>
      <div class="progress" role="progressbar" aria-valuenow="${Math.round(pct)}" aria-valuemin="0" aria-valuemax="100">
        <i style="width:${pct}%"></i><b style="left:calc(${pct}% - 2px)"></b>
      </div>
      ${err}
      <div class="dl-actions">${actionsFor(d)}</div>
    </article>`;
  }

  function render() {
    const q = ($("#search").value || "").toLowerCase();
    const match = (d) => d.filename.toLowerCase().includes(q);
    const active = items.filter((d) => ["active", "paused", "queued"].includes(d.status) && !d.scheduledAt && match(d));
    const sched = items.filter((d) => d.scheduledAt && d.status === "queued" && match(d));
    const fin = items.filter((d) => ["completed", "failed", "cancelled"].includes(d.status) && match(d));
    $("#card-list").innerHTML = active.map(card).join("");
    $("#card-scheduled").innerHTML = sched.map(card).join("");
    $("#card-finished").innerHTML = fin.map(card).join("");
    $("#empty-downloads").hidden = active.length > 0;
    $("#empty-scheduled").hidden = sched.length > 0;
    $("#empty-finished").hidden = fin.length > 0;
    $("#stat-active").textContent = items.filter((d) => d.status === "active").length;
    $("#stat-queued").textContent = items.filter((d) => d.status === "queued").length;
  }

  async function refresh() {
    try {
      items = await backend().list();
    } catch (e) {
      items = [];
      snack("Daemon unreachable — start fetchlined");
    }
    render();
  }

  function snack(msg) {
    const el = $("#snackbar");
    el.textContent = msg;
    el.hidden = false;
    clearTimeout(snack.t);
    snack.t = setTimeout(() => { el.hidden = true; }, 2600);
  }

  // View switching
  document.querySelectorAll(".rail-item").forEach((btn) => {
    btn.addEventListener("click", () => {
      document.querySelectorAll(".rail-item").forEach((b) => b.classList.remove("is-active"));
      btn.classList.add("is-active");
      view = btn.dataset.view;
      $("#view-title").textContent = VIEWS[view];
      document.querySelectorAll(".view").forEach((v) => v.classList.remove("is-visible"));
      $("#view-" + view).classList.add("is-visible");
    });
  });

  // Search + live updates
  $("#search").addEventListener("input", render);
  document.addEventListener("fetchline:update", refresh);
  document.addEventListener("click", async (e) => {
    const btn = e.target.closest("[data-act]");
    if (!btn) return;
    await backend().act(Number(btn.dataset.id), btn.dataset.act);
    if (btn.dataset.act === "remove") snack("Removed from list");
  });

  // Add sheet
  const sheet = $("#sheet-add"), scrim = $("#scrim");
  function openSheet() { sheet.hidden = false; scrim.hidden = false; $("#add-urls").focus(); }
  function closeSheet() { sheet.hidden = true; scrim.hidden = true; }
  $("#fab-add").addEventListener("click", openSheet);
  $("#add-cancel").addEventListener("click", closeSheet);
  scrim.addEventListener("click", closeSheet);
  document.addEventListener("keydown", (e) => { if (e.key === "Escape" && !sheet.hidden) closeSheet(); });
  $("#add-confirm").addEventListener("click", async () => {
    const urls = $("#add-urls").value.split("\n").map((s) => s.trim()).filter(Boolean);
    if (!urls.length) { snack("Paste at least one URL"); return; }
    const conn = Number(document.querySelector("#seg-add-conn .is-on").dataset.v);
    await backend().add(urls, conn);
    $("#add-urls").value = "";
    closeSheet();
    snack(urls.length === 1 ? "Download queued" : `${urls.length} downloads queued`);
  });

  // Segmented controls + switches (persist to localStorage for now)
  document.querySelectorAll(".segmented").forEach((seg) => {
    const saved = localStorage.getItem("fetchline:" + seg.id);
    if (saved) seg.querySelectorAll("button").forEach((b) => b.classList.toggle("is-on", b.dataset.v === saved));
    seg.addEventListener("click", (e) => {
      const btn = e.target.closest("button");
      if (!btn) return;
      seg.querySelectorAll("button").forEach((b) => b.classList.remove("is-on"));
      btn.classList.add("is-on");
      localStorage.setItem("fetchline:" + seg.id, btn.dataset.v);
      if (seg.id === "seg-theme") setTheme(btn.dataset.v);
      snack("Preference saved");
    });
  });
  document.querySelectorAll(".switch").forEach((sw) => {
    const saved = localStorage.getItem("fetchline:" + sw.id);
    if (saved !== null) sw.setAttribute("aria-checked", saved);
    sw.addEventListener("click", () => {
      const on = sw.getAttribute("aria-checked") !== "true";
      sw.setAttribute("aria-checked", String(on));
      localStorage.setItem("fetchline:" + sw.id, String(on));
    });
  });

  // Theme: system/light/dark
  function setTheme(mode) {
    document.documentElement.dataset.theme = mode;
    $("#theme-toggle").title = "Theme: " + mode;
  }
  (function initTheme() {
    const saved = localStorage.getItem("fetchline:seg-theme") || "system";
    document.querySelectorAll("#seg-theme button").forEach((b) => b.classList.toggle("is-on", b.dataset.v === saved));
    setTheme(saved);
  })();
  $("#theme-toggle").addEventListener("click", () => {
    const order = ["system", "light", "dark"];
    const cur = document.documentElement.dataset.theme || "system";
    const next = order[(order.indexOf(cur) + 1) % order.length];
    document.querySelectorAll("#seg-theme button").forEach((b) => b.classList.toggle("is-on", b.dataset.v === next));
    localStorage.setItem("fetchline:seg-theme", next);
    setTheme(next);
  });

  refresh();
})();
