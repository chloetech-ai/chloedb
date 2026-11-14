// ChloeDB Workbench: the page. One module, no build step. State comes from
// /api/stream (server-sent events); terminals are WebSockets to
// /api/agents/:id/pty drawn with xterm.js (window.Terminal, loaded by
// index.html from the pinned CDN URL). SPEC.md is the contract.

const TOKEN_KEY = "chloedb-workbench-token";
const REPO_KEY = "chloedb-workbench-repo";
const REPOS_KEY = "chloedb-workbench-repos";
const MODEL_KEY = "chloedb-workbench-model";
const PREVIEW_LIMIT = 6; // past this many running agents, tiles show their last event instead of a terminal
const DEFAULT_SIZE = [120, 32]; // the PTY's size at spawn (SPEC.md)
const PREVIEW_RATIO = 0.5; // a tile's terminal box: height over width (120 x 32 at 11px fits)
const MONO = 'ui-monospace, SFMono-Regular, Menlo, Consolas, "Liberation Mono", monospace';
const ENC = new TextEncoder();

const $ = (id) => document.getElementById(id);

const S = {
  token: null,
  agents: new Map(), // id -> Agent
  tiles: new Map(), // id -> Tile
  order: [], // running agents' ids, in grid order
  health: null,
  zoomed: null,
  selected: null,
  prefix: false,
  sizes: new Map(), // id -> [cols, rows], the PTY's size as this page last set it
  pendingZoom: null,
  conn: "connecting",
};

// ------------------------------------------------------------- helpers

function h(tag, attrs, ...kids) {
  const el = document.createElement(tag);
  if (attrs) {
    for (const [k, v] of Object.entries(attrs)) {
      if (v == null || v === false) continue;
      if (k === "class") el.className = v;
      else if (k === "text") el.textContent = v;
      else if (k.startsWith("on")) el.addEventListener(k.slice(2), v);
      else el.setAttribute(k, v === true ? "" : v);
    }
  }
  for (const kid of kids.flat()) if (kid != null && kid !== false) el.append(kid);
  return el;
}

function icon(name, cls = "i sm") {
  const svg = document.createElementNS("http://www.w3.org/2000/svg", "svg");
  svg.setAttribute("class", cls);
  svg.setAttribute("aria-hidden", "true");
  const use = document.createElementNS("http://www.w3.org/2000/svg", "use");
  use.setAttribute("href", `#i-${name}`);
  svg.append(use);
  return svg;
}

function parse(text) {
  try { return JSON.parse(text); } catch { return null; }
}

function store(kind, key, value) {
  try {
    const s = kind === "session" ? sessionStorage : localStorage;
    if (value === undefined) return s.getItem(key);
    if (value === null) s.removeItem(key);
    else s.setItem(key, value);
  } catch { /* storage blocked: keep going without it */ }
  return null;
}

function toMs(t) {
  if (t == null || t === "") return null;
  if (typeof t === "number") return t < 1e12 ? t * 1000 : t;
  const ms = Date.parse(t);
  return Number.isNaN(ms) ? null : ms;
}

function fmtDur(ms) {
  if (ms == null || ms < 0) ms = 0;
  const s = Math.floor(ms / 1000);
  if (s < 60) return `${s}s`;
  const m = Math.floor(s / 60);
  if (m < 60) return `${m}m`;
  const hr = Math.floor(m / 60);
  if (hr < 24) return m % 60 ? `${hr}h ${m % 60}m` : `${hr}h`;
  const d = Math.floor(hr / 24);
  return hr % 24 ? `${d}d ${hr % 24}h` : `${d}d`;
}

function fmtClock(t) {
  const ms = toMs(t);
  if (ms == null) return "";
  return new Date(ms).toLocaleTimeString([], { hour: "2-digit", minute: "2-digit", second: "2-digit", hour12: false });
}

function fmtWhen(t) {
  const ms = toMs(t);
  if (ms == null) return "";
  const d = new Date(ms);
  const today = new Date().toDateString() === d.toDateString();
  return today ? fmtClock(ms) : d.toLocaleString([], { month: "short", day: "numeric", hour: "2-digit", minute: "2-digit", hour12: false });
}

function since(el, t) {
  const ms = toMs(t);
  if (ms == null) { el.removeAttribute("data-since"); el.textContent = ""; return; }
  el.dataset.since = String(ms);
  el.textContent = fmtDur(Date.now() - ms);
}

function tickTimes() {
  const now = Date.now();
  for (const el of document.querySelectorAll("[data-since]")) {
    const t = fmtDur(now - Number(el.dataset.since));
    if (el.textContent !== t) el.textContent = t;
  }
}

function cap(s) { return s ? s[0].toUpperCase() + s.slice(1) : s; }

function nameOf(a) { return (a && (a.name || a.id)) || "?"; }

function shortPath(p) {
  if (!p) return "";
  return p.replace(/^\/(home|Users)\/[^/]+/, "~");
}

const STATE_LABEL = { starting: "starting", working: "working", waiting: "waiting for you", idle: "idle", exited: "exited" };

function chip(el, a) {
  el.className = "chip";
  el.dataset.state = a.state || "starting";
  let label = STATE_LABEL[a.state] || a.state || "starting";
  if (a.state === "exited" && a.exit_code != null && a.exit_code !== 0) {
    label = `exited ${a.exit_code}`;
    el.classList.add("bad");
  }
  el.textContent = label;
}

function toast(text, bad = false) {
  const t = h("div", { class: bad ? "toast bad" : "toast", role: bad ? "alert" : "status", text });
  $("toasts").append(t);
  setTimeout(() => t.remove(), bad ? 6000 : 3500);
}

function isDark() {
  const forced = document.documentElement.dataset.theme;
  if (forced) return forced === "dark";
  return matchMedia("(prefers-color-scheme: dark)").matches;
}

function termTheme() {
  const cs = getComputedStyle(document.documentElement);
  const bg = cs.getPropertyValue("--term-bg").trim();
  const fg = cs.getPropertyValue("--term-fg").trim();
  if (isDark()) {
    return {
      background: bg || "#0f1114", foreground: fg || "#d8dde4", cursor: "#d8dde4", cursorAccent: bg || "#0f1114",
      selectionBackground: "rgba(122,162,255,.32)",
      black: "#1d2128", red: "#f07178", green: "#8fd18b", yellow: "#e6c07b", blue: "#7aa2ff", magenta: "#c3a6ff", cyan: "#76d0e0", white: "#c7ccd4",
      brightBlack: "#5c6370", brightRed: "#ff8b92", brightGreen: "#a9e6a5", brightYellow: "#f5d58f", brightBlue: "#9ab8ff", brightMagenta: "#d6c1ff", brightCyan: "#94e2ef", brightWhite: "#f2f4f7",
    };
  }
  return {
    background: bg || "#fbfbfc", foreground: fg || "#1f2328", cursor: "#1f2328", cursorAccent: bg || "#fbfbfc",
    selectionBackground: "rgba(47,91,211,.22)",
    black: "#24292f", red: "#c62f37", green: "#1a7f37", yellow: "#8a5a00", blue: "#0b5cd5", magenta: "#8250df", cyan: "#1b7c83", white: "#6e7781",
    brightBlack: "#57606a", brightRed: "#a40e26", brightGreen: "#116329", brightYellow: "#6c4400", brightBlue: "#218bff", brightMagenta: "#a475f9", brightCyan: "#3192aa", brightWhite: "#8c959f",
  };
}

// ----------------------------------------------------------------- token

function readToken() {
  const url = new URL(location.href);
  const t = url.searchParams.get("token");
  if (t) {
    store("session", TOKEN_KEY, t);
    url.searchParams.delete("token");
    history.replaceState(history.state, "", url.pathname + url.search + url.hash);
    return t;
  }
  return store("session", TOKEN_KEY);
}

function gate(title, text) {
  $("gate-title").textContent = title;
  if (text) $("gate-text").textContent = text;
  $("gate").hidden = false;
  setTimeout(() => $("gate-input").focus(), 0);
}

$("gate-form").addEventListener("submit", (e) => {
  e.preventDefault();
  let v = $("gate-input").value.trim();
  if (!v) return;
  try {
    const u = new URL(v);
    v = u.searchParams.get("token") || v;
  } catch { /* a bare token */ }
  S.token = v;
  store("session", TOKEN_KEY, v);
  $("gate").hidden = true;
  $("gate-input").value = "";
  start();
});

// ------------------------------------------------------------------- api

async function api(path, { method = "GET", body } = {}) {
  const headers = { Authorization: `Bearer ${S.token}` };
  if (body !== undefined) headers["Content-Type"] = "application/json";
  let r;
  try {
    r = await fetch(path, { method, headers, body: body === undefined ? undefined : JSON.stringify(body) });
  } catch {
    throw Object.assign(new Error("The Workbench server did not answer."), { status: 0 });
  }
  const j = parse(await r.text()) || {};
  if (!r.ok) {
    if (r.status === 401) gate("The token was refused", "This page's token is not the server's. Open the URL that chloedb-workbench serve printed, or paste it here.");
    throw Object.assign(new Error(j.error || `${r.status} ${r.statusText}`), { status: r.status, code: j.error_code });
  }
  return j;
}

// ---------------------------------------------------------------- agents

function norm(a) {
  if (a.running == null) a.running = a.state !== "exited";
  if (!Array.isArray(a.children)) a.children = [];
  return a;
}

function upsert(a) {
  if (!a || !a.id) return;
  norm(a);
  const prev = S.agents.get(a.id);
  S.agents.set(a.id, a);
  if (prev && prev.state !== a.state && a.state === "waiting" && S.zoomed !== a.id) {
    S.tiles.get(a.id)?.flash();
  }
  schedule();
}

async function loadAll() {
  try {
    const { agents = [] } = await api("/api/agents?all=1");
    // The stream is newer than this answer for the agents it already sent.
    for (const a of agents) if (!S.agents.has(a.id) || !norm(a).running) upsert(a);
  } catch (e) {
    if (e.status !== 401) console.warn("agents:", e.message);
  }
  if (S.pendingZoom && S.agents.has(S.pendingZoom)) {
    const id = S.pendingZoom;
    S.pendingZoom = null;
    zoom(id, { history: "replace" });
  }
}

// ---------------------------------------------------------------- stream

let es = null;
let esTries = 0;
let esTimer = 0;

function setConn(state) {
  S.conn = state;
  $("conn").dataset.state = state;
  $("conn-text").textContent = { live: "live", connecting: "connecting", reconnecting: "reconnecting", offline: "offline" }[state];
}

function connectStream() {
  clearTimeout(esTimer);
  es?.close();
  setConn(esTries ? "reconnecting" : "connecting");
  es = new EventSource(`/api/stream?token=${encodeURIComponent(S.token)}`);
  es.addEventListener("open", () => { esTries = 0; setConn("live"); });
  es.addEventListener("snapshot", (e) => {
    const d = parse(e.data) || {};
    const list = d.agents || [];
    const ids = new Set(list.map((a) => a.id));
    // Running agents the snapshot leaves out ended while we were away; drop
    // them, and loadAll brings them back as exited.
    for (const [id, a] of S.agents) if (a.running && !ids.has(id)) S.agents.delete(id);
    for (const a of list) upsert(a);
    setConn("live");
    loadAll();
  });
  es.addEventListener("agent", (e) => {
    const d = parse(e.data);
    upsert(d && d.agent ? d.agent : d);
  });
  es.addEventListener("event", (e) => {
    const d = parse(e.data);
    const ev = d && d.event ? d.event : d;
    if (ev) onEvent(ev);
  });
  es.onerror = () => {
    if (es.readyState === EventSource.CLOSED) {
      setConn("offline");
      esTries++;
      // Find out why before trying again: a refused token opens the gate.
      api("/api/agents").then(
        () => { esTimer = setTimeout(connectStream, Math.min(1000 * 2 ** esTries, 15000)); },
        (err) => { if (err.status !== 401) esTimer = setTimeout(connectStream, Math.min(1000 * 2 ** esTries, 15000)); },
      );
    } else {
      setConn("reconnecting");
    }
  };
}

// ------------------------------------------------------------ pty socket

class PtySocket {
  constructor(id, handlers) {
    this.id = id;
    this.h = handlers;
    this.closed = false;
    this.tries = 0;
    this.ws = null;
    this.connect();
  }

  connect() {
    const proto = location.protocol === "https:" ? "wss" : "ws";
    const ws = new WebSocket(`${proto}://${location.host}/api/agents/${encodeURIComponent(this.id)}/pty?token=${encodeURIComponent(S.token)}`);
    ws.binaryType = "arraybuffer";
    let first = true;
    ws.onopen = () => { this.tries = 0; this.h.onOpen?.(this); };
    ws.onmessage = (e) => {
      if (first) { first = false; this.h.onReset?.(); } // the scrollback is replayed on every attach
      this.h.onData(typeof e.data === "string" ? e.data : new Uint8Array(e.data));
    };
    ws.onclose = () => {
      if (this.ws === ws) this.ws = null;
      if (this.closed) return;
      const a = S.agents.get(this.id);
      if (!a || !a.running) { this.h.onClose?.("ended"); return; }
      this.tries++;
      this.h.onClose?.("retry");
      this.timer = setTimeout(() => { if (!this.closed) this.connect(); }, Math.min(400 * 2 ** this.tries, 10000));
    };
    this.ws = ws;
  }

  get open() { return this.ws?.readyState === WebSocket.OPEN; }

  send(data) {
    if (!this.open) return false;
    this.ws.send(typeof data === "string" ? ENC.encode(data) : data);
    return true;
  }

  resize(cols, rows) {
    if (this.open) this.ws.send(JSON.stringify({ resize: [cols, rows] }));
  }

  close() {
    this.closed = true;
    clearTimeout(this.timer);
    try { this.ws?.close(); } catch { /* already closed */ }
  }
}

function sizeOf(id) { return S.sizes.get(id) || DEFAULT_SIZE; }

function setSize(id, cols, rows) {
  const [c, r] = sizeOf(id);
  if (c === cols && r === rows) return;
  S.sizes.set(id, [cols, rows]);
  S.tiles.get(id)?.preview?.resize(cols, rows);
}

// ---------------------------------------------------------------- tiles

class Preview {
  constructor(id, host) {
    this.id = id;
    this.host = host;
    this.inner = h("div", { class: "pv" });
    host.replaceChildren(this.inner);
    const [cols, rows] = sizeOf(id);
    this.term = new window.Terminal({
      cols, rows, fontSize: 11, lineHeight: 1.0, fontFamily: MONO, disableStdin: true, cursorBlink: false,
      cursorStyle: "block", scrollback: 0, theme: termTheme(), allowTransparency: false,
    });
    this.term.open(this.inner);
    if (this.term.textarea) { this.term.textarea.tabIndex = -1; this.term.textarea.setAttribute("aria-hidden", "true"); }
    this.ro = new ResizeObserver(() => this.queue());
    this.ro.observe(host);
    this.term.onCursorMove(() => this.queue());
    this.sock = new PtySocket(id, {
      onReset: () => this.term.reset(),
      onData: (d) => this.term.write(d),
    });
    requestAnimationFrame(() => this.scale());
  }

  resize(cols, rows) {
    try { this.term.resize(cols, rows); } catch { /* disposed */ }
    requestAnimationFrame(() => this.scale());
  }

  // The tile's box keeps one shape whatever the PTY's size; when the screen
  // is taller than the box (after a zoom made the PTY bigger), the box
  // follows the cursor, where the agent is writing.
  scale() {
    const scr = this.inner.querySelector(".xterm-screen");
    if (!scr) return;
    const w = scr.offsetWidth, ht = scr.offsetHeight;
    const room = this.host.clientWidth - 12;
    if (!w || !ht || room <= 0) return;
    const s = Math.min(room / w, 1);
    const box = Math.round(room * PREVIEW_RATIO);
    const cell = ht / this.term.rows;
    const cursor = this.term.buffer.active.cursorY;
    const shift = Math.max(0, Math.min(ht * s - box, (cursor + 2) * cell * s - box));
    this.inner.style.transform = `translateY(${-Math.round(shift)}px) scale(${s})`;
    this.host.style.aspectRatio = "auto";
    this.host.style.height = `${box + 10}px`;
  }

  queue() {
    if (this.raf) return;
    this.raf = requestAnimationFrame(() => { this.raf = 0; this.scale(); });
  }

  retheme() { this.term.options.theme = termTheme(); }

  dispose() {
    cancelAnimationFrame(this.raf);
    this.sock.close();
    this.ro.disconnect();
    this.term.dispose();
    this.host.replaceChildren();
    this.host.style.height = "";
    this.host.style.aspectRatio = "";
  }
}

class Tile {
  constructor(id) {
    this.id = id;
    this.preview = null;
    this.idx = h("span", { class: "idx" });
    this.nm = h("span", { class: "nm" });
    this.chip = h("span", { class: "chip" });
    this.br = h("span", { class: "br" }, icon("branch"), h("span"));
    this.model = h("span", { class: "model" });
    this.kids = h("span", { class: "kids" });
    this.since = h("span", { class: "since" });
    this.task = h("p", { class: "task" });
    this.pv = h("div", { class: "preview" });
    this.footIcon = h("span");
    this.last = h("span", { class: "last" });
    this.foot = h("div", { class: "tile-foot" }, this.footIcon, this.last);
    this.el = h("article", { class: "tile", tabindex: "0", "data-id": id },
      h("div", { class: "tile-head" }, this.idx, this.nm, h("span", { class: "sep" }), this.chip),
      h("div", { class: "meta" }, this.br, this.model, this.kids, this.since),
      this.task, this.pv, this.foot);
    this.el.addEventListener("click", () => zoom(this.id));
    this.el.addEventListener("focus", () => select(this.id, { focus: false }));
    this.key = "";
  }

  update(a, n, withPreview) {
    const key = [a.name, a.state, a.state_since, a.branch, a.task, a.last_event, a.waiting_for, a.model, a.children.length, n, withPreview].join("\u0001");
    if (key === this.key) return;
    this.key = key;
    this.el.dataset.state = a.state || "starting";
    this.el.setAttribute("aria-label", `${nameOf(a)}, ${STATE_LABEL[a.state] || a.state}`);
    this.idx.textContent = n <= 9 ? String(n) : "";
    this.nm.textContent = nameOf(a);
    this.nm.title = a.cwd || "";
    chip(this.chip, a);
    this.br.lastChild.textContent = a.branch || "no branch";
    this.br.title = a.cwd ? `${a.branch || ""} in ${a.cwd}` : a.branch || "";
    this.model.textContent = a.model || "";
    this.model.hidden = !a.model;
    this.kids.textContent = a.children.length ? `${a.children.length} spawned` : "";
    this.kids.hidden = !a.children.length;
    since(this.since, a.state_since);
    this.task.textContent = a.task || "No task: started at a prompt";
    this.task.classList.toggle("none", !a.task);
    this.task.title = a.task || "";
    const waiting = a.state === "waiting";
    this.foot.classList.toggle("ask", waiting);
    this.footIcon.replaceChildren(icon(waiting ? "bell" : a.state === "idle" ? "turn" : "act"));
    this.last.textContent = cap(waiting ? a.waiting_for || "Waiting for you" : a.last_event || (a.state === "starting" ? "Starting Claude Code…" : "No events yet"));
    this.last.title = this.last.textContent;
    this.setPreview(withPreview);
  }

  setPreview(on) {
    on = on && !!window.Terminal;
    if (on && !this.preview) this.preview = new Preview(this.id, this.pv);
    if (!on && this.preview) { this.preview.dispose(); this.preview = null; }
    this.pv.hidden = !on;
  }

  flash() {
    this.el.classList.remove("flash");
    void this.el.offsetWidth;
    this.el.classList.add("flash");
  }

  dispose() {
    this.setPreview(false);
    this.el.remove();
  }
}

// --------------------------------------------------------------- render

let raf = 0;
function schedule() {
  if (!raf) raf = requestAnimationFrame(() => { raf = 0; render(); });
}

const byCreated = (a, b) => (toMs(a.created) || 0) - (toMs(b.created) || 0) || a.id.localeCompare(b.id);
const bySince = (a, b) => (toMs(a.state_since) || 0) - (toMs(b.state_since) || 0);

function render() {
  const all = [...S.agents.values()];
  const running = all.filter((a) => a.running).sort(byCreated);
  const exited = all.filter((a) => !a.running).sort((a, b) => (toMs(b.ended || b.state_since) || 0) - (toMs(a.ended || a.state_since) || 0));
  const attention = running.filter((a) => a.state === "waiting" || a.state === "idle").sort(bySince);
  S.order = running.map((a) => a.id);
  if (S.selected && !S.agents.has(S.selected)) S.selected = null;

  // top bar
  $("run-n").textContent = String(running.length);
  $("run-max").textContent = S.health?.max_agents ? ` of ${S.health.max_agents}` : "";
  const waiting = attention.filter((a) => a.state === "waiting").length;
  $("need-pill").hidden = !attention.length;
  $("need-n").textContent = String(attention.length);
  $("need-pill").classList.toggle("warn", waiting > 0);
  document.title = waiting ? `(${waiting}) Workbench` : "Workbench";

  renderRail(attention);
  renderGrid(running);
  renderExited(exited);
  if (S.zoomed) renderZoom();
}

function renderRail(list) {
  const ol = $("rail-list");
  $("rail-empty").hidden = list.length > 0;
  const n = $("rail-n");
  n.textContent = list.length ? String(list.length) : "";
  n.classList.toggle("warn", list.some((a) => a.state === "waiting"));
  const items = list.map((a) => {
    const ago = h("span", { class: "ago" });
    since(ago, a.state_since);
    const last = a.last_event && !/^turn (finished|over)/i.test(a.last_event) ? `: ${a.last_event}` : "";
    const why = a.state === "waiting" ? cap(a.waiting_for) || "Waiting for you" : `Turn finished${last}`;
    return h("li", null, h("button", {
      class: `ritem ${a.state}${S.zoomed === a.id ? " sel" : ""}`,
      title: `Zoom ${nameOf(a)}`,
      onclick: () => zoom(a.id),
    }, h("span", { class: "dot" }), h("span", { class: "nm", text: nameOf(a) }), ago, h("span", { class: "why", text: why })));
  });
  ol.replaceChildren(...items);
}

function renderGrid(running) {
  const grid = $("grid");
  const previews = running.length <= PREVIEW_LIMIT;
  grid.classList.toggle("compact", !previews);
  const live = new Set(running.map((a) => a.id));
  for (const [id, t] of S.tiles) if (!live.has(id)) { t.dispose(); S.tiles.delete(id); }
  running.forEach((a, i) => {
    let t = S.tiles.get(a.id);
    if (!t) { t = new Tile(a.id); S.tiles.set(a.id, t); }
    if (grid.children[i] !== t.el) grid.insertBefore(t.el, grid.children[i] || null);
    t.el.classList.toggle("sel", S.selected === a.id);
    t.update(a, i + 1, previews);
  });
  $("empty").hidden = running.length > 0;
}

function renderExited(list) {
  $("fold").hidden = !list.length;
  $("fold-n").textContent = list.length ? `(${list.length})` : "";
  const rows = list.slice(0, 200).map((a) => {
    const c = h("span");
    chip(c, a);
    return h("button", { class: "xrow", title: `Open ${nameOf(a)}'s terminal and events`, onclick: () => zoom(a.id) },
      h("span", { class: "nm", text: nameOf(a) }), c,
      h("span", { class: "t", text: a.task || "No task" }),
      h("span", { class: "br", text: a.branch || "" }),
      h("span", { class: "when", text: fmtWhen(a.ended || a.state_since) }));
  });
  $("exited").replaceChildren(...rows);
}

// ---------------------------------------------------------------- select

function select(id, { focus = true } = {}) {
  if (!id) return;
  if (S.selected !== id) {
    S.selected = id;
    for (const [tid, t] of S.tiles) t.el.classList.toggle("sel", tid === id);
  }
  if (focus && !S.zoomed) {
    const t = S.tiles.get(id);
    if (t && document.activeElement !== t.el) t.el.focus({ preventScroll: true });
    t?.el.scrollIntoView({ block: "nearest", behavior: "smooth" });
  }
}

function move(dir) {
  const ids = S.order;
  if (!ids.length) return;
  if (S.zoomed) {
    const i = ids.indexOf(S.zoomed);
    const step = dir === "left" || dir === "up" ? -1 : 1;
    const next = ids[((i < 0 ? 0 : i + step) + ids.length) % ids.length];
    zoom(next, { history: "replace" });
    return;
  }
  const cur = S.tiles.get(S.selected);
  if (!cur) { select(ids[0]); return; }
  const r0 = cur.el.getBoundingClientRect();
  const cx = r0.left + r0.width / 2, cy = r0.top + r0.height / 2;
  let best = null, bestD = Infinity;
  for (const id of ids) {
    if (id === S.selected) continue;
    const r = S.tiles.get(id).el.getBoundingClientRect();
    const x = r.left + r.width / 2, y = r.top + r.height / 2;
    const dx = x - cx, dy = y - cy;
    const ok = { left: dx < -4 && Math.abs(dy) < r0.height / 2, right: dx > 4 && Math.abs(dy) < r0.height / 2, up: dy < -4, down: dy > 4 }[dir];
    if (!ok) continue;
    const d = dir === "up" || dir === "down" ? Math.abs(dy) * 1 + Math.abs(dx) * 2 : Math.abs(dx);
    if (d < bestD) { bestD = d; best = id; }
  }
  // Left and right run on into the row before or after, as reading order does.
  if (!best && (dir === "left" || dir === "right")) {
    const i = ids.indexOf(S.selected) + (dir === "left" ? -1 : 1);
    if (i >= 0 && i < ids.length) best = ids[i];
  }
  if (best) select(best);
}

// ------------------------------------------------------------------ zoom

const Z = { id: null, term: null, fit: null, sock: null, ro: null, events: [], live: [], loaded: false, factsKey: "" };

function zoom(id, { history: how = "push" } = {}) {
  const a = S.agents.get(id);
  if (!a) return;
  S.selected = id;
  if (Z.id === id) { Z.term?.focus(); return; }
  const wasZoomed = !!S.zoomed;
  closeZoomTerm();
  S.zoomed = id;
  if (how === "push" && !wasZoomed) history.pushState({ zoom: id }, "", `#${id}`);
  else if (how !== "none") history.replaceState({ zoom: id, root: !wasZoomed && how === "replace" ? true : history.state?.root }, "", `#${id}`);
  $("grid-view").hidden = true;
  $("zoom-view").hidden = false;
  setTab("term");
  Z.factsKey = "";
  openZoomTerm(id);
  loadEvents(id);
  render();
}

function unzoom({ fromHistory = false } = {}) {
  if (!S.zoomed) return;
  if (!fromHistory) {
    if (history.state?.zoom && !history.state.root) { history.back(); return; } // popstate comes back here
    history.replaceState(null, "", location.pathname + location.search);
  }
  const id = S.zoomed;
  closeZoomTerm();
  S.zoomed = null;
  $("zoom-view").hidden = true;
  $("grid-view").hidden = false;
  render();
  requestAnimationFrame(() => {
    const t = S.tiles.get(id);
    if (t) { select(id); t.preview?.scale(); }
    for (const tile of S.tiles.values()) tile.preview?.scale();
  });
}

window.addEventListener("popstate", () => {
  const id = location.hash.slice(1);
  if (id && S.agents.has(id)) zoom(id, { history: "none" });
  else unzoom({ fromHistory: true });
});

function overlay(text) {
  const o = $("z-overlay");
  o.hidden = !text;
  o.textContent = text || "";
}

function openZoomTerm(id) {
  const host = $("z-term");
  host.replaceChildren();
  overlay(null);
  Z.id = id;
  Z.events = [];
  Z.live = [];
  Z.loaded = false;
  if (!window.Terminal) {
    overlay("The terminal library did not load (cdn.jsdelivr.net).");
    return;
  }
  const a = S.agents.get(id);
  const term = new window.Terminal({
    fontFamily: MONO, fontSize: 13, lineHeight: 1.0, cursorBlink: true, scrollback: 10000,
    theme: termTheme(), macOptionIsMeta: true, allowProposedApi: false, disableStdin: !a.running,
  });
  const fit = new window.FitAddon.FitAddon();
  term.loadAddon(fit);
  term.open(host);
  Z.term = term;
  Z.fit = fit;
  const doFit = () => { if (Z.term === term && host.clientWidth > 0) { try { fit.fit(); } catch { /* not laid out yet */ } } };
  doFit();
  Z.sock = new PtySocket(id, {
    onOpen: (s) => {
      overlay(null);
      if (S.agents.get(id)?.running) { s.resize(term.cols, term.rows); setSize(id, term.cols, term.rows); }
    },
    onReset: () => term.reset(),
    onData: (d) => term.write(d),
    onClose: (why) => {
      if (why === "ended") {
        term.options.disableStdin = true;
        term.write("\x1b[?25l"); // no cursor in a terminal nobody can type into
        overlay("The session has ended. Its scrollback is above.");
      }
      else overlay("Reconnecting to the terminal…");
    },
  });
  term.onData((d) => Z.sock?.send(d));
  term.onBinary((d) => Z.sock?.send(Uint8Array.from(d, (c) => c.charCodeAt(0) & 255)));
  term.onResize(({ cols, rows }) => {
    if (!S.agents.get(id)?.running) return;
    Z.sock?.resize(cols, rows);
    setSize(id, cols, rows);
  });
  let fitRaf = 0;
  Z.ro = new ResizeObserver(() => { cancelAnimationFrame(fitRaf); fitRaf = requestAnimationFrame(doFit); });
  Z.ro.observe(host);
  if (a.running) setTimeout(() => { if (Z.term === term) term.focus(); }, 0);
}

function closeZoomTerm() {
  Z.ro?.disconnect();
  Z.sock?.close();
  Z.term?.dispose();
  Z.id = Z.term = Z.fit = Z.sock = Z.ro = null;
  $("z-term").replaceChildren();
}

function renderZoom() {
  const a = S.agents.get(S.zoomed);
  if (!a) return;
  $("z-name").textContent = nameOf(a);
  chip($("z-chip"), a);
  $("z-branch").replaceChildren(icon("branch"), document.createTextNode(a.branch || "no branch"));
  $("z-branch").title = a.cwd || "";
  since($("z-since"), a.state_since);
  $("z-since").title = "Time in this state";
  const ask = a.state === "waiting";
  $("z-ask").hidden = !ask;
  $("z-ask-text").textContent = cap(a.waiting_for) || "Waiting for you";
  $("z-stop").disabled = !a.running;
  if (Z.term) Z.term.options.disableStdin = !a.running;

  const key = JSON.stringify([a.task, a.repo, a.cwd, a.branch, a.model, a.parent, a.children, a.pid, a.session_id, a.created, a.ended, a.exit_code, a.depth]);
  if (key === Z.factsKey) return;
  Z.factsKey = key;
  const link = (id) => {
    const o = S.agents.get(id);
    return h("a", { onclick: () => zoom(id, { history: "replace" }), title: o?.task || id, text: o ? nameOf(o) : id });
  };
  const rows = [];
  const fact = (k, v, title) => { if (v == null || v === "" || (Array.isArray(v) && !v.length)) return; rows.push(h("dt", { text: k }), h("dd", { title: title || (typeof v === "string" ? v : null) }, v)); };
  fact("repo", shortPath(a.repo), a.repo);
  if (a.cwd && a.cwd !== a.repo) fact("worktree", shortPath(a.cwd), a.cwd);
  fact("branch", a.branch);
  fact("model", a.model);
  if (a.parent) fact("spawned by", link(a.parent));
  if (a.children.length) fact("spawned", a.children.flatMap((id, i) => (i ? [", ", link(id)] : [link(id)])));
  fact("pid", a.pid != null ? String(a.pid) : null);
  fact("session", a.session_id);
  fact("started", fmtWhen(a.created));
  if (!a.running) fact("ended", fmtWhen(a.ended) + (a.exit_code != null ? `, exit ${a.exit_code}` : ""));
  $("z-facts").replaceChildren(
    h("p", { class: "task-full", text: a.task || "No task: started at a prompt." }),
    h("dl", null, rows));
}

// ---------------------------------------------------------------- events

const KIND = {
  spawned: ["spawned", "c-sys"],
  SessionStart: ["session", "c-sys"],
  UserPromptSubmit: ["prompt", "c-prompt"],
  PreToolUse: ["tool", "c-tool"],
  PostToolUse: ["result", "c-tool"],
  Notification: ["asks", "c-ask"],
  Stop: ["done", "c-stop"],
  SubagentStop: ["subagent", "c-sys"],
  SessionEnd: ["ended", "c-end"],
  input: ["input", "c-prompt"],
  exited: ["exited", "c-end"],
};

function evRow(ev) {
  const [label, cls] = KIND[ev.kind] || [ev.kind || "event", "c-sys"];
  const s = h("span", { class: "s" });
  let text = (ev.summary || "").replace(/\/(home|Users)\/[^/\s]+\//g, "~/");
  // The server's summaries read "Bash: cargo test", "Bash done: cargo test",
  // "prompt: …": the label and the tool's chip already say the first part.
  if (ev.tool) {
    s.append(h("code", { text: ev.tool }));
    if (text.startsWith(ev.tool)) text = text.slice(ev.tool.length).replace(/^\s*(done\b)?\s*:?\s*/, "");
  } else if (ev.kind === "UserPromptSubmit") {
    text = text.replace(/^prompt:\s*/, "");
  }
  s.append(document.createTextNode(ev.tool ? text : cap(text)));
  return h("li", { class: `ev ${cls}`, title: `${ev.kind}${ev.tool ? ` ${ev.tool}` : ""} at ${fmtWhen(ev.ts)} (seq ${ev.seq})` },
    h("span", { class: "ts", text: fmtClock(ev.ts) }), h("span", { class: "k", text: label }), s);
}

function renderEvents() {
  const ol = $("z-events");
  $("ev-n").textContent = Z.events.length ? String(Z.events.length) : "";
  if (!Z.events.length) {
    ol.replaceChildren(h("li", { class: "ev-empty", text: Z.loaded ? "No events yet." : "Loading…" }));
    return;
  }
  ol.replaceChildren(...Z.events.map(evRow));
  ol.scrollTop = ol.scrollHeight;
}

async function loadEvents(id) {
  renderEvents();
  const got = [];
  let after = 0;
  try {
    for (let page = 0; page < 25; page++) {
      const { events = [] } = await api(`/api/agents/${encodeURIComponent(id)}/events?after=${after}&limit=200`);
      if (Z.id !== id) return;
      got.push(...events);
      if (events.length < 200) break;
      after = events[events.length - 1].seq;
    }
  } catch (e) {
    if (Z.id !== id) return;
    toast(`Events: ${e.message}`, true);
  }
  const seen = new Set(got.map((e) => e.seq));
  for (const e of Z.live) if (!seen.has(e.seq)) got.push(e);
  got.sort((a, b) => a.seq - b.seq);
  Z.events = got.slice(-1500);
  Z.live = [];
  Z.loaded = true;
  renderEvents();
}

function onEvent(ev) {
  if (!Z.id || ev.agent !== Z.id) return;
  if (!Z.loaded) { Z.live.push(ev); return; }
  if (Z.events.length && Z.events[Z.events.length - 1].seq >= ev.seq && Z.events.some((e) => e.seq === ev.seq)) return;
  const ol = $("z-events");
  const atBottom = ol.scrollHeight - ol.scrollTop - ol.clientHeight < 40;
  if (!Z.events.length) ol.replaceChildren();
  Z.events.push(ev);
  ol.append(evRow(ev));
  $("ev-n").textContent = String(Z.events.length);
  if (atBottom) ol.scrollTop = ol.scrollHeight;
}

function setTab(tab) {
  $("z-body").dataset.tab = tab;
  $("tab-term").setAttribute("aria-selected", String(tab === "term"));
  $("tab-events").setAttribute("aria-selected", String(tab === "events"));
  if (tab === "term") requestAnimationFrame(() => { try { Z.fit?.fit(); } catch { /* hidden */ } });
}
$("tab-term").addEventListener("click", () => setTab("term"));
$("tab-events").addEventListener("click", () => setTab("events"));

const KEYBAR = { esc: "\x1b", tab: "\t", stab: "\x1b[Z", up: "\x1b[A", down: "\x1b[B", cc: "\x03", enter: "\r" };
$("keybar").addEventListener("pointerdown", (e) => e.preventDefault()); // keep the terminal's focus
$("keybar").addEventListener("click", (e) => {
  const k = e.target.closest("button")?.dataset.k;
  if (k && Z.sock) Z.sock.send(KEYBAR[k]);
});

$("z-back").addEventListener("click", () => unzoom());
$("z-stop").addEventListener("click", () => S.zoomed && confirmStop(S.zoomed));

// --------------------------------------------------------------- dialogs

const openDialog = () => document.querySelector("dialog[open]");

for (const d of document.querySelectorAll("dialog")) {
  d.addEventListener("click", (e) => {
    if (e.target.closest("[data-close]") || e.target === d) d.close();
  });
  d.addEventListener("close", () => {
    if (S.zoomed && S.agents.get(S.zoomed)?.running) Z.term?.focus();
    else if (S.selected) S.tiles.get(S.selected)?.el.focus({ preventScroll: true });
  });
}

function showHelp() {
  if (openDialog()) return;
  const v = S.health?.version;
  $("help-version").textContent = v ? `chloedb-workbench ${v}` : "";
  $("help-dialog").showModal();
}

function recentRepos() {
  const list = parse(store("local", REPOS_KEY) || "[]");
  return Array.isArray(list) ? list.filter((x) => typeof x === "string") : [];
}

function branchHelp() {
  const name = $("f-name").value || "<name>";
  const help = $("wt-help");
  help.replaceChildren();
  if ($("f-wt").checked) {
    help.append("A new branch ", h("code", { text: `wb/${name}` }), " from the repository's HEAD, so agents don't step on each other.");
  } else {
    help.append("Off: the session runs in the repository's own checkout, on its current branch.");
  }
}

function openNew() {
  if (openDialog()) return;
  const repo = store("local", REPO_KEY) || "";
  $("f-repo").value = repo;
  $("f-repo").removeAttribute("aria-invalid");
  $("f-task").value = "";
  $("f-name").value = "";
  $("f-model").value = store("local", MODEL_KEY) || "";
  $("f-wt").checked = true;
  $("new-err").hidden = true;
  $("new-go").disabled = false;
  $("repo-list").replaceChildren(...recentRepos().map((r) => h("option", { value: r })));
  branchHelp();
  $("new-dialog").showModal();
  (repo ? $("f-task") : $("f-repo")).focus();
}

$("f-name").addEventListener("input", (e) => {
  const el = e.target;
  const clean = el.value.toLowerCase().replace(/[\s_]+/g, "-").replace(/[^a-z0-9-]/g, "").slice(0, 32);
  if (clean !== el.value) el.value = clean;
  branchHelp();
});
$("f-wt").addEventListener("change", branchHelp);
$("f-task").addEventListener("keydown", (e) => {
  if (e.key === "Enter" && (e.ctrlKey || e.metaKey)) { e.preventDefault(); $("new-form").requestSubmit(); }
});

function newError(text) {
  $("new-err").textContent = text;
  $("new-err").hidden = false;
}

$("new-form").addEventListener("submit", async (e) => {
  e.preventDefault();
  const repo = $("f-repo").value.trim();
  const task = $("f-task").value.trim();
  const name = $("f-name").value.trim();
  const model = $("f-model").value.trim();
  const worktree = $("f-wt").checked;
  $("new-err").hidden = true;
  if (!repo || !(repo.startsWith("/") || /^[A-Za-z]:[\\/]/.test(repo))) {
    $("f-repo").setAttribute("aria-invalid", "true");
    $("f-repo").focus();
    newError("The repository is an absolute path, such as /home/you/src/project.");
    return;
  }
  $("f-repo").removeAttribute("aria-invalid");
  if (name && !/^[a-z0-9-]{1,32}$/.test(name)) { newError("A name is lowercase letters, digits and hyphens, at most 32."); return; }
  const body = { repo, worktree };
  if (task) body.task = task;
  if (name) body.name = name;
  if (model) body.model = model;
  $("new-go").disabled = true;
  try {
    const { agent } = await api("/api/agents", { method: "POST", body });
    store("local", REPO_KEY, repo);
    store("local", REPOS_KEY, JSON.stringify([repo, ...recentRepos().filter((r) => r !== repo)].slice(0, 8)));
    store("local", MODEL_KEY, model || null);
    $("new-dialog").close();
    if (agent) {
      upsert(agent);
      toast(`Started ${nameOf(agent)}${agent.branch ? ` on ${agent.branch}` : ""}`);
      if (!task) zoom(agent.id); // it waits at a prompt: take the user there
      else requestAnimationFrame(() => { select(agent.id); S.tiles.get(agent.id)?.flash(); });
    }
  } catch (err) {
    const max = S.health?.max_agents;
    if (err.status === 429) newError(`${err.message}${max ? ` (at most ${max} run at once)` : ""}. Stop one first.`);
    else newError(err.message);
  } finally {
    $("new-go").disabled = false;
  }
});

let confirmFor = null;
function confirmStop(id) {
  const a = S.agents.get(id);
  if (!a || !a.running || openDialog()) return;
  confirmFor = id;
  $("confirm-title").textContent = `Stop ${nameOf(a)}?`;
  $("confirm-text").textContent = `Its Claude Code session ends: Workbench sends it SIGHUP, then SIGKILL after 5 s. ${a.cwd && a.cwd !== a.repo ? "Its worktree and branch are left as they are." : ""}`.trim();
  $("confirm-dialog").showModal();
  $("confirm-yes").focus();
}

async function doStop() {
  const id = confirmFor;
  confirmFor = null;
  $("confirm-dialog").close();
  if (!id) return;
  try {
    await api(`/api/agents/${encodeURIComponent(id)}/stop`, { method: "POST" });
    toast(`Stopping ${nameOf(S.agents.get(id))}`);
  } catch (e) {
    toast(`Could not stop it: ${e.message}`, true);
  }
}
$("confirm-yes").addEventListener("click", doStop);
$("confirm-no").addEventListener("click", () => { confirmFor = null; $("confirm-dialog").close(); });
$("confirm-dialog").addEventListener("keydown", (e) => {
  if (e.key === "y" || e.key === "Y") { e.preventDefault(); doStop(); }
  else if (e.key === "n" || e.key === "N") { e.preventDefault(); confirmFor = null; $("confirm-dialog").close(); }
});

$("new-btn").addEventListener("click", openNew);
$("empty-new").addEventListener("click", openNew);
$("help-btn").addEventListener("click", showHelp);

// ------------------------------------------------------------------ keys

function setPrefix(on) {
  S.prefix = on;
  $("prefix-ind").hidden = !on;
}

function inField(t) {
  if (!t || t.classList?.contains("xterm-helper-textarea")) return false;
  return t.isContentEditable || /^(INPUT|TEXTAREA|SELECT)$/.test(t.tagName);
}

const ARROWS = { ArrowLeft: "left", ArrowRight: "right", ArrowUp: "up", ArrowDown: "down" };

function target() { return S.zoomed || S.selected || S.order[0] || null; }

function runCommand(e) {
  const k = e.key;
  if (k === "n") return openNew(), true;
  if (k === "?" || (k === "/" && e.shiftKey)) return showHelp(), true;
  if (k === "z") {
    if (S.zoomed) unzoom();
    else if (target()) zoom(target());
    return true;
  }
  if (k === "x") { const id = target(); if (id) confirmStop(id); return true; }
  if (ARROWS[k]) return move(ARROWS[k]), true;
  if (/^[1-9]$/.test(k)) { const id = S.order[Number(k) - 1]; if (id) zoom(id, { history: S.zoomed ? "replace" : "push" }); return true; }
  return false;
}

window.addEventListener("keydown", (e) => {
  if (e.isComposing || !$("gate").hidden) return;
  if (S.prefix) {
    if (["Shift", "Control", "Alt", "Meta", "CapsLock"].includes(e.key)) return;
    e.preventDefault();
    e.stopPropagation();
    setPrefix(false);
    if (e.ctrlKey && (e.key === "b" || e.key === "B")) { Z.sock?.send("\x02"); return; } // C-b C-b: a literal C-b
    if (e.key === "Escape") { if (S.zoomed) unzoom(); return; }
    runCommand(e);
    return;
  }
  if (e.ctrlKey && !e.altKey && !e.metaKey && (e.key === "b" || e.key === "B")) {
    if (openDialog()) return;
    e.preventDefault();
    e.stopPropagation();
    setPrefix(true);
    return;
  }
  if (openDialog() || inField(e.target) || e.ctrlKey || e.metaKey || e.altKey) return;
  const inTerm = e.target.classList?.contains("xterm-helper-textarea");
  // Every other key belongs to the agent, Esc included, while it runs.
  if (inTerm && S.agents.get(S.zoomed)?.running) return;
  if (e.key === "Escape" && S.zoomed) { e.preventDefault(); unzoom(); return; }
  if (e.key === "Enter" && !S.zoomed && e.target.closest?.(".tile")) { e.preventDefault(); zoom(e.target.closest(".tile").dataset.id); return; }
  if (!S.zoomed && ARROWS[e.key] && (e.target.closest?.(".tile") || e.target === document.body)) { e.preventDefault(); move(ARROWS[e.key]); return; }
  // Without the prefix only n, z and ? act: stop, digits and (in zoom) arrows
  // need it, so a stray key never stops or switches an agent.
  if ((e.key === "n" || e.key === "z" || e.key === "?") && runCommand(e)) e.preventDefault();
}, true);

// A click anywhere drops an armed prefix, as a key would.
window.addEventListener("pointerdown", () => { if (S.prefix) setPrefix(false); }, true);

// ----------------------------------------------------------------- theme

matchMedia("(prefers-color-scheme: dark)").addEventListener("change", () => {
  const theme = termTheme();
  for (const t of S.tiles.values()) t.preview?.retheme();
  if (Z.term) Z.term.options.theme = theme;
});

// ----------------------------------------------------------------- start

let started = false;
function start() {
  if (!started) {
    started = true;
    setInterval(tickTimes, 1000);
    fetch("/api/health").then((r) => r.json()).then((j) => { S.health = j; schedule(); }).catch(() => {});
  }
  const id = location.hash.slice(1);
  if (id) S.pendingZoom = id;
  connectStream();
  render();
}

S.token = readToken();
if (!S.token) gate("Open the link the server printed");
else start();
