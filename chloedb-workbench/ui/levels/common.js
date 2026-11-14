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
    else if (k === "style" && typeof v === "object") Object.assign(el.style, v);
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
  return `${Math.floor(m / 60)} h ${m % 60} min`;
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
export function agentHue(a) {
  const key = (a && (a.name || a.id)) || "?";
  let x = 0;
  for (const c of key) x = (x * 31 + c.charCodeAt(0)) >>> 0;
  return HUES[x % HUES.length];
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
@media (prefers-color-scheme: dark){:root:not([data-theme="light"]){--ctx:#b48cff}}
:root[data-theme="dark"]{--ctx:#b48cff}
`);
