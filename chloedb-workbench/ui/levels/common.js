// Shared pieces of the level screens (SPEC.md, "Levels (third build)").
// Each screen is an ES module in this folder exporting
//   mount(root, ctx) -> { update(state), unmount() }
// where ctx = { api(path, opts), state: {agents, events}, go(hash),
// openWhy(args), openRewind(args), zoom(agentId) }.
// No build step and no dependencies: plain DOM, styles injected once per
// module with useStyle(), colours only from the theme's tokens (style.css).

export function h(tag, attrs, ...kids) {
  const el = document.createElement(tag);
  for (const [k, v] of Object.entries(attrs || {})) {
    if (v == null || v === false) continue;
    if (k === "class") el.className = v;
    else if (k === "style" && typeof v === "object") for (const [p, x] of Object.entries(v)) { if (p.startsWith("--")) el.style.setProperty(p, x); else el.style[p] = x; }
    else if (k.startsWith("on") && typeof v === "function") el.addEventListener(k.slice(2), v);
    else if (k === "html") el.innerHTML = v;
    else el.setAttribute(k, v === true ? "" : v);
  }
  for (const kid of kids.flat(Infinity)) {
    if (kid == null || kid === false) continue;
    el.append(kid instanceof Node ? kid : document.createTextNode(String(kid)));
  }
  return el;
}

const styled = new Set();
/** Adds a module's CSS to the page once. */
export function useStyle(id, css) {
  if (styled.has(id)) return;
  styled.add(id);
  document.head.append(h("style", { "data-level": id }, css));
}

export const toMs = (t) => (t == null ? null : typeof t === "number" ? t : Date.parse(t));

export function fmtDur(ms) {
  if (ms == null || !isFinite(ms)) return "";
  const s = Math.max(0, Math.round(ms / 1000));
  if (s < 60) return `${s} s`;
  const m = Math.round(s / 60);
  if (m < 60) return `${m} min`;
  if (m < 48 * 60) return `${Math.floor(m / 60)} h ${m % 60} min`;
  return `${Math.round(m / 1440)} days`;
}

export function fmtClock(t) {
  const ms = toMs(t);
  return ms ? new Date(ms).toLocaleTimeString([], { hour: "numeric", minute: "2-digit" }) : "";
}

export function fmtAgo(t) {
  const ms = toMs(t);
  return ms ? fmtDur(Date.now() - ms) + " ago" : "";
}

/** A stable hue per agent name, so an agent keeps its colour on every screen. */
const HUES = [222, 266, 176, 330, 40, 150, 12, 200];
function hashHue(a) {
  const key = (a && (a.name || a.id)) || "?";
  let x = 0;
  for (const c of key) x = (x * 31 + c.charCodeAt(0)) >>> 0;
  return HUES[x % HUES.length];
}
const fleetHues = new Map();
export function agentHue(a) {
  return (a && fleetHues.get(a.id)) ?? hashHue(a);
}
/** Makes the agents seen together tell apart: oldest first, each keeps its
 *  own hue unless an older one has one within 20°, and then takes the middle
 *  of the widest free gap (never the reds, which mean trouble). Any screen
 *  may call it with the fleet; the colours stay the same everywhere. */
export function setFleet(agents) {
  const list = [...(agents || [])].sort((x, y) => (toMs(x.created) || 0) - (toMs(y.created) || 0) || String(x.id).localeCompare(String(y.id)));
  const dist = (p, q) => { const d = Math.abs(p - q) % 360; return Math.min(d, 360 - d); };
  const taken = [];
  fleetHues.clear();
  for (const a of list) {
    let hue = hashHue(a);
    if (taken.some((t) => dist(t, hue) < 20)) {
      let best = hue, gap = -1;
      for (let c = 30; c <= 340; c += 5) {
        const g = Math.min(...taken.map((t) => dist(t, c)));
        if (g > gap) { gap = g; best = c; }
      }
      hue = best;
    }
    taken.push(hue);
    fleetHues.set(a.id, hue);
  }
}
export function agentColor(a) { return `hsl(${agentHue(a)} 80% 68%)`; }

/** The square avatar with the agent's initial; `you` for the user. */
export function avatar(a, size = 24) {
  if (a === "you") return h("span", { class: "lv-av lv-you", style: { width: size + "px", height: size + "px" } }, "you");
  const name = (a && (a.name || a.id)) || "?";
  return h("span", { class: "lv-av", title: name, style: { width: size + "px", height: size + "px", background: agentColor(a) } }, name[0].toUpperCase());
}

/** Avatar plus name, clickable to the agent's story. */
export function agentChip(a, ctx) {
  const name = (a && (a.name || a.id)) || "?";
  return h("button", { class: "lv-agent", onclick: (e) => { e.stopPropagation(); ctx && ctx.go(`#/story/${a.id}`); } }, avatar(a, 18), name);
}

/** The ◆ chip that marks something ChloeCtx remembered or decided. */
export function ctxChip(text, title) { return h("span", { class: "lv-ctx", title: title || "from ChloeCtx" }, text); }

export function pill(text, tone = "mut") { return h("span", { class: `lv-pill lv-${tone}` }, text); }

export function bar(pct, tone = "accent") {
  return h("div", { class: "lv-bar" }, h("b", { style: { width: Math.max(0, Math.min(100, pct || 0)) + "%", background: `var(--${tone})` } }));
}

export function empty(text) { return h("div", { class: "lv-empty" }, text); }

useStyle("common", `
.lv-av{display:inline-grid;place-items:center;border-radius:7px;font-weight:750;font-size:11px;color:#0b0d10;flex:none}
.lv-you{background:var(--text);color:var(--bg);font-size:9px}
.lv-agent{display:inline-flex;align-items:center;gap:6px;border:0;background:none;color:var(--text-2);font:inherit;cursor:pointer;padding:0}
.lv-agent:hover{color:var(--text)}
.lv-ctx{display:inline-flex;align-items:center;gap:5px;font-size:11px;font-weight:600;color:var(--ctx, #8b5cf6)}
.lv-ctx::before{content:"";width:7px;height:7px;border-radius:2px;background:currentColor;transform:rotate(45deg);flex:none}
.lv-pill{display:inline-flex;align-items:center;border-radius:999px;padding:2px 9px;font-size:11px;font-weight:600;white-space:nowrap}
.lv-mut{background:var(--surface-3);color:var(--text-2)} .lv-good{background:var(--good-soft);color:var(--good-ink)}
.lv-warn{background:var(--warn-soft);color:var(--warn-ink)} .lv-crit{background:var(--crit-soft);color:var(--crit-ink)}
.lv-acc{background:var(--accent-soft);color:var(--accent)}
.lv-bar{height:6px;border-radius:999px;background:var(--surface-3);overflow:hidden}.lv-bar>b{display:block;height:100%;border-radius:999px}
.lv-empty{color:var(--muted);padding:24px;text-align:center}
:root{--ctx:#7c3aed}
@media (prefers-color-scheme: dark){:root:not([data-theme="light"]){--ctx:#b48cff}}
:root[data-theme="dark"]{--ctx:#b48cff}
`);

// ---------------------------------------------------------------- shared helpers
// (added with Mission, Story and Decide; generic, for every level screen)

/** A route's parameter: ctx.params[name] when the shell gives it, else the
 *  hash's second segment (`#/story/<agent>` → agent). */
export function routeParam(ctx, name) {
  if (ctx && ctx.params && ctx.params[name]) return ctx.params[name];
  const parts = (location.hash || "").replace(/^#\/?/, "").split(/[/?]/);
  return parts[1] ? decodeURIComponent(parts[1]) : null;
}

/** The repository a screen is about: ctx.params.repo, else the one most of
 *  the running agents work in. */
export function repoOf(ctx) {
  if (ctx && ctx.params && ctx.params.repo) return ctx.params.repo;
  const n = new Map();
  for (const a of (ctx && ctx.state && ctx.state.agents) || []) if (a.repo && a.running !== false) n.set(a.repo, (n.get(a.repo) || 0) + 1);
  let best = null, max = 0;
  for (const [r, c] of n) if (c > max) [best, max] = [r, c];
  return best || "";
}

/** Agents by id from several lists (fetched rows, the stream's): the row
 *  whose state or narration is newer wins; on a tie, the later list. */
export function mergeAgents(...lists) {
  const stamp = (a) => Math.max(toMs(a.state_since) || 0, toMs(a.narration_ts) || 0);
  const out = new Map();
  for (const list of lists) for (const a of list || []) {
    const had = out.get(a.id);
    if (!had || stamp(a) >= stamp(had)) out.set(a.id, a);
  }
  return out;
}

/** Text with `code` spans, as nodes. */
export function inlineCode(text) {
  return String(text ?? "").split(/(`[^`]+`)/).filter(Boolean).map((p) => (p.length > 2 && p[0] === "`" && p.at(-1) === "`" ? h("code", { class: "lv-code" }, p.slice(1, -1)) : p));
}

/** JSON a row keeps as TEXT (options, precedent), or already parsed. */
export function parseJSON(v, fallback = null) {
  if (v == null || v === "") return fallback;
  if (typeof v !== "string") return v;
  try { return JSON.parse(v); } catch { return fallback; }
}

/** A question's precedent as records to show. The server keeps what
 *  `chloectx ask --json` printed ({text, ids, agents: {id: Agent}, missing,
 *  rules: [{id, text, option}]}): the notes are the text's "- <date> <kind>:
 *  <text>" lines, in the order of `ids`. Rules of the repository that speak
 *  for an option come first, as yours. Each record is {id, kind, text, ts,
 *  agent, by}; `by` is "you", the agent's name, or null (a session before
 *  Workbench). */
export function precedentOf(q) {
  const p = parseJSON(q && q.precedent, null);
  if (!p || typeof p !== "object") return { records: [], ids: [], text: "", missing: [], pending: true };
  if (Array.isArray(p.records)) return { ids: [], missing: [], ...p };
  const ids = Array.isArray(p.ids) ? p.ids : [];
  const agents = p.agents || {};
  const records = [];
  const seen = new Set();
  for (const r of p.rules || []) {
    if (seen.has(r.id)) continue;
    seen.add(r.id);
    records.push({ id: r.id, kind: "rule", text: r.text, ts: null, agent: null, by: "you", option: r.option });
  }
  let i = 0;
  for (const line of String(p.text || "").split("\n")) {
    const m = line.match(/^- (\d{4}-\d{2}-\d{2}) ([\w-]+): (.*)$/);
    if (!m) continue;
    const id = ids[i++] || null;
    const agent = (id && agents[id]) || null;
    records.push({ id, kind: m[2], text: m[3].trim(), ts: m[1], agent, by: agent ? agent.name : null });
  }
  return { records, ids, text: p.text || "", missing: p.missing || [], pending: false };
}

/** 950, 41k, 1.2M. */
export function fmtTokens(n) {
  if (n == null || !isFinite(n)) return "";
  if (n < 1000) return String(n);
  if (n < 1e6) return `${n < 1e4 ? (n / 1000).toFixed(1).replace(/\.0$/, "") : Math.round(n / 1000)}k`;
  return `${(n / 1e6).toFixed(1).replace(/\.0$/, "")}M`;
}

/** A short span: 0.4 s, 11 s, 2 m 40 s, 1 h 5 m. */
export function fmtSpan(ms) {
  if (ms == null || !isFinite(ms)) return "";
  if (ms < 1000) return `${(ms / 1000).toFixed(1)} s`;
  const s = Math.round(ms / 1000);
  if (s < 60) return `${s} s`;
  if (s < 3600) return s % 60 ? `${Math.floor(s / 60)} m ${s % 60} s` : `${s / 60} min`;
  return `${Math.floor(s / 3600)} h ${Math.round((s % 3600) / 60)} m`;
}

/** A day as people say it: today, yesterday, 9 Nov. */
export function fmtDay(t) {
  const ms = toMs(t);
  if (!ms) return "";
  const d = new Date(ms);
  const days = Math.round((new Date(new Date().toDateString()) - new Date(d.toDateString())) / 864e5);
  if (days === 0) return "today";
  if (days === 1) return "yesterday";
  return d.toLocaleDateString([], { day: "numeric", month: "short" });
}

/** Vertical bars for a series (a pulse, activity per minute); `tone(v, i)`
 *  names a theme token per bar, `title(v, i)` its tooltip. */
export function barChart(values, { height = 64, tone = () => "accent", title = () => null } = {}) {
  const max = Math.max(1, ...values);
  return h("div", { class: "lv-bars", style: { height: height + "px" } }, values.map((v, i) =>
    h("i", { title: title(v, i), style: { height: (v ? Math.max(6, (v / max) * 100) : 3) + "%", background: `var(--${tone(v, i)})`, opacity: v ? 1 : 0.4 } })));
}

const typing = (el) => el && (el.isContentEditable || /^(INPUT|TEXTAREA|SELECT)$/.test(el.tagName));

/** A screen's keyboard: `j`/`k` move the focus through the visible elements
 *  matching `items`, in document order; Enter or Space on one that is not a
 *  button clicks it. `onKey(e)` sees other keys first and returns true when
 *  it used one. Keys typed into a field, or with a modifier, are left alone.
 *  Returns the function that removes it. */
export function keyNav(root, { items = "[data-nav]", onKey } = {}) {
  const handler = (e) => {
    if (!root.isConnected || e.defaultPrevented || e.metaKey || e.ctrlKey || e.altKey || typing(e.target)) return;
    if (onKey && onKey(e)) { e.preventDefault(); return; }
    if (e.key === "j" || e.key === "k") {
      const list = [...root.querySelectorAll(items)].filter((x) => x.offsetParent !== null);
      if (!list.length) return;
      e.preventDefault();
      const cur = document.activeElement && document.activeElement.closest ? document.activeElement.closest(items) : null;
      const at = list.indexOf(cur);
      const next = at < 0 ? list[0] : list[Math.max(0, Math.min(list.length - 1, at + (e.key === "j" ? 1 : -1)))];
      next.focus({ preventScroll: true });
      next.scrollIntoView({ block: "nearest" });
    } else if ((e.key === "Enter" || e.key === " ") && root.contains(e.target) && e.target.matches(items) && !/^(BUTTON|A|LABEL)$/.test(e.target.tagName)) {
      e.preventDefault();
      e.target.click();
    }
  };
  document.addEventListener("keydown", handler);
  return () => document.removeEventListener("keydown", handler);
}

/** Replaces `host`'s children with what `fn` builds, keeping the focus on
 *  the element with the same data-key. */
export function rerender(host, fn) {
  const had = document.activeElement && host.contains(document.activeElement) ? document.activeElement.dataset.key : null;
  host.replaceChildren(...[fn()].flat(Infinity).filter((x) => x != null && x !== false));
  if (had) { const el = host.querySelector(`[data-key="${CSS.escape(had)}"]`); if (el) el.focus({ preventScroll: true }); }
}

/** fn, called once `ms` after the last call. */
export function later(fn, ms) {
  let t = null;
  const f = (...a) => { clearTimeout(t); t = setTimeout(() => fn(...a), ms); };
  f.cancel = () => clearTimeout(t);
  return f;
}

useStyle("common-helpers", `
.lv-screen{height:100%;overflow:auto;overscroll-behavior:contain}
.lv-code{font-family:var(--mono);font-size:.88em;background:var(--surface-3);border-radius:4px;padding:1px 4px}
.lv-h{margin:0 0 10px;font-size:11.5px;font-weight:700;letter-spacing:.06em;text-transform:uppercase;color:var(--muted)}
.lv-h .n{font-weight:500;margin-left:3px}
.lv-card{background:var(--surface);border:1px solid var(--line);border-radius:12px;padding:14px;box-shadow:var(--shadow-1)}
.lv-bars{display:flex;align-items:flex-end;gap:3px}.lv-bars>i{flex:1;min-width:2px;border-radius:2px 2px 0 0}
.lv-vio{background:color-mix(in srgb,var(--ctx) 14%,transparent);color:var(--ctx)}
`);

// ---------------------------------------------------------------------------
// More shared pieces: the page frame, cards, a tooltip, Esc handling, short
// inline formatting, numbers.

/** The token the app keeps (for URLs that cannot carry a header). */
export function token(ctx) {
  if (ctx && ctx.token) return ctx.token;
  try { return sessionStorage.getItem("chloedb-workbench-token") || "dev"; } catch { return "dev"; }
}

/** An agent's colour at an alpha, for tints. */
export function agentTint(a, alpha) { return `hsl(${agentHue(a)} 80% 68% / ${alpha})`; }

/** `**bold**` and `code` in a sentence, as nodes; everything else is text. */
export function md(text) {
  const out = [];
  const re = /\*\*([^*]+)\*\*|`([^`]+)`/g;
  const s = String(text ?? "");
  let at = 0, m;
  while ((m = re.exec(s))) {
    if (m.index > at) out.push(s.slice(at, m.index));
    out.push(m[1] != null ? h("b", null, m[1]) : h("code", { class: "lv-code" }, m[2]));
    at = re.lastIndex;
  }
  if (at < s.length) out.push(s.slice(at));
  return out;
}

/** 1.2M, 312k, 940. */
export function fmtNum(n) {
  if (n == null || !isFinite(n)) return "";
  const a = Math.abs(n);
  if (a >= 1e6) return (n / 1e6).toFixed(a >= 1e7 ? 0 : 1).replace(/\.0$/, "") + "M";
  if (a >= 1e3) return Math.round(n / 1e3) + "k";
  return String(Math.round(n));
}

/** Calls fn at most once per `wait` ms, the last call's arguments winning. */
export function throttle(fn, wait) {
  let t = null, last = 0, args;
  const run = () => { t = null; last = Date.now(); fn(...args); };
  const call = (...a) => { args = a; if (t) return; t = setTimeout(run, Math.max(0, wait - (Date.now() - last))); };
  call.cancel = () => { clearTimeout(t); t = null; };
  return call;
}

/** Esc closes the newest open panel or bubble first. Returns its remover. */
const escStack = [];
document.addEventListener("keydown", (e) => {
  if (e.key !== "Escape" || !escStack.length) return;
  e.preventDefault();
  e.stopPropagation();
  escStack[escStack.length - 1]();
}, true);
export function onEscape(close) {
  escStack.push(close);
  return () => { const i = escStack.lastIndexOf(close); if (i >= 0) escStack.splice(i, 1); };
}

/** One floating tooltip: show(content, x, y) near a point, hide(), remove(). */
export function tooltip() {
  const el = h("div", { class: "lv-tip", role: "tooltip", hidden: true });
  document.body.append(el);
  return {
    show(content, x, y) {
      el.replaceChildren(...[content].flat());
      el.hidden = false;
      const r = el.getBoundingClientRect();
      const left = Math.min(window.innerWidth - r.width - 8, Math.max(8, x + 14));
      const top = y + 18 + r.height > window.innerHeight - 8 ? y - r.height - 12 : y + 18;
      el.style.left = left + "px";
      el.style.top = Math.max(8, top) + "px";
    },
    hide() { el.hidden = true; },
    remove() { el.remove(); },
  };
}

/** A card with an optional small uppercase heading and a right-hand aside. */
export function card(title, aside, ...kids) {
  return h("section", { class: "lv-card" },
    title || aside ? h("div", { class: "lv-card-head" }, title ? h("h2", { class: "lv-h2" }, title) : null, aside) : null,
    ...kids);
}

useStyle("common-2", `
.lv-screen{position:absolute;inset:0;color:var(--text);font-size:13px;line-height:1.45}
.lv-screen.aw,.lv-screen.fl,.lv-screen.mp,.lv-screen.pv{padding:20px 24px 28px}
.lv-head{display:flex;align-items:baseline;gap:6px 14px;flex-wrap:wrap}
.lv-h1{margin:0;font-size:22px;font-weight:680;letter-spacing:-.01em}
.lv-sub{color:var(--muted)}
.lv-h2{margin:0;font-size:11.5px;font-weight:650;color:var(--muted);text-transform:uppercase;letter-spacing:.06em}
.lv-card{background:var(--surface);border:1px solid var(--line);border-radius:12px;padding:14px 16px;box-shadow:var(--shadow-1);min-width:0}
.lv-card-head{display:flex;align-items:center;justify-content:space-between;gap:10px;margin-bottom:10px}
.lv-code{font-family:var(--mono);font-size:.92em;background:var(--surface-3);border-radius:4px;padding:0 4px}
.lv-tip{position:fixed;z-index:80;pointer-events:none;max-width:340px;background:var(--surface);color:var(--text);border:1px solid var(--line-2);border-radius:9px;padding:8px 10px;font-size:12px;line-height:1.45;box-shadow:var(--shadow-2)}
.lv-tip[hidden]{display:none}
.lv-link{border:0;background:none;padding:0;color:var(--accent);cursor:pointer;font:inherit;font-weight:600}
.lv-link:hover{text-decoration:underline}
button.lv-ctx{border:0;background:none;padding:0;cursor:pointer;font-family:inherit}
@media (max-width:640px){.lv-screen.aw,.lv-screen.fl,.lv-screen.mp,.lv-screen.pv{padding:14px}.lv-h1{font-size:19px}}
`);
