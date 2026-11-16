// The activity log (under "⋯"): every step the autopilot takes, and why,
// newest first; a line an item, unfolded into its evidence. New items go
// on top and nothing else is redrawn.
import { h, put, toMs, ago, enc } from "/goals/converge/util.js";
import { useStyle } from "/levels/common.js";
import { stepHead } from "/goals/converge/rows.js";

const CSS = `
/* Activity */
.cv-evs { list-style: none; margin: 0; padding: 0; border-top: var(--hair) solid var(--line); }
.cv-ev { border-bottom: var(--hair) solid var(--line); }
.cv-ev[hidden] { display: none; }
.cv-ev-h { grid-template-columns: 76px minmax(0, 1fr) auto; padding: 6px 2px; }
.cv-ev-h .tx { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; min-width: 0; font-size: var(--fs-sm); }
.cv-ev.open .cv-ev-h .tx { white-space: normal; }
.cv-ev-h .tm { font-family: var(--mono); font-size: var(--fs-xs); color: var(--muted); white-space: nowrap; }
.cv-ev-b { padding: 0 2px 12px 88px; display: flex; flex-direction: column; gap: 8px; }
.cv-ev-b[hidden] { display: none; }
.cv-ev-b .s { margin: 0; font-size: var(--fs-sm); }
.cv-ev.cv-new { animation: cv-in 2.4s ease-out; }
@keyframes cv-in { from { background: var(--accent-soft); } to { background: transparent; } }
.cv-k { display: inline-flex; justify-content: center; min-width: 0; height: 20px; padding: 0 6px; align-items: center; border-radius: 4px; font-family: var(--mono); font-size: 10px; letter-spacing: .04em; text-transform: uppercase; background: var(--surface-3); color: var(--text-2); white-space: nowrap; overflow: hidden; }
.cv-k.k-tick, .cv-k.k-decide { background: var(--accent-soft); color: var(--accent-text); }
.cv-k.k-gate, .cv-k.k-pr { background: var(--good-soft); color: var(--good-ink); }
.cv-k.bad { background: var(--crit-soft); color: var(--crit-ink); }
.cv-k.k-you, .cv-k.k-ask { background: var(--warn-soft); color: var(--warn-ink); }
.cv-ev-h { width: 100%; min-height: 44px; display: grid; align-items: center; gap: 10px; border: 0; background: none; color: var(--text); font: inherit; text-align: left; cursor: pointer; }
.cv-chipf { min-height: 36px; padding: 0 12px; border-radius: var(--r-pill); border: var(--hair) solid var(--line-2); background: var(--surface); color: var(--text-2); font: inherit; font-size: var(--fs-sm); cursor: pointer; display: inline-flex; align-items: center; gap: 6px; flex: none; }
.cv-chipf.on { border-color: var(--accent-line); background: var(--accent-soft); color: var(--accent-text); }
.cv-chipf .n { font-family: var(--mono); font-size: var(--fs-xs); }
@media (max-width: 640px) { .cv-chipf { min-height: 44px; } .cv-ev-h { grid-template-columns: 64px minmax(0, 1fr) auto; gap: 8px; } .cv-ev-b { padding-left: 2px; } }
`;

export function activityScreen(C) {
  useStyle("converge-activity", CSS);
  const S = C.S;
  const repo = C.repo, api = C.api, keep = C.keep, view = C.view;
  const ACT = { tick: "Tick", decide: "Decision", gate: "Gate", persona: "Persona", agent: "Agent", pr: "PR", generate: "Generate", ask: "Ask", you: "You" };
  function activity() {
    if (!S.actUI) {
      const list = h("ol", { class: "cv-evs", "aria-live": "polite" });
      const chips = h("div", { class: "cv-bar kinds", role: "group", "aria-label": "Kinds of step" });
      const note = h("div", {});
      S.actUI = { el: h("div", {}, chips, note, list), list, chips, note, nodes: new Map() };
    }
    paintAct([]);
    return S.actUI.el;
  }
  async function fetchAct() {
    if (S.actBusy || !repo()) return;
    S.actBusy = true;
    try {
      const A = S.act || { items: [], since: null };
      const q = A.since ? `&since=${A.since}` : "&limit=200";
      const j = await api(`/api/converge/activity?repo=${enc(repo())}${q}`);
      S.actMissing = false;
      const ids = new Set(A.items.map((x) => x.id));
      const fresh = (j.items || []).filter((x) => x && x.id && !ids.has(x.id)).sort((a, b) => (toMs(b.ts) || 0) - (toMs(a.ts) || 0));
      const items = [...fresh, ...A.items].slice(0, 500);
      const newest = items.length ? Math.max(...items.slice(0, 20).map((x) => toMs(x.ts) || 0)) : null;
      S.act = { items, since: newest || toMs(j.now) || A.since };
      keep();
      if (S.alive && view() === "activity") paintAct(fresh);
    } catch (err) {
      if (err.status === 404 || err.status === 405) { S.actMissing = true; if (S.actUI) paintAct([]); }
    } finally { S.actBusy = false; }
  }
  function paintAct(fresh) {
    const U = S.actUI;
    if (!U) return;
    const chips = U.chips;
    const items = (S.act && S.act.items) || [];
    put(U.note, S.actMissing ? h("p", { class: "cv-needs" }, "This server does not keep the autopilot's log yet: it comes with the next build.") : !items.length ? h("p", { class: "cv-empty" }, S.act ? "Nothing logged yet. Run a tick, or turn the autopilot on." : "Reading the log…") : null);
    // Kinds: a chip each, with its count; several can be on.
    const counts = {};
    for (const x of items) counts[x.kind] = (counts[x.kind] || 0) + 1;
    const kinds = Object.keys(ACT).filter((k) => counts[k] || S.actKinds.has(k)).concat(Object.keys(counts).filter((k) => !ACT[k]));
    put(U.chips, !items.length ? null : [ 
      h("button", { class: `cv-chipf${S.actKinds.size ? "" : " on"}`, type: "button", onclick: () => { S.actKinds.clear(); keep(); paintAct([]); } }, "All", h("span", { class: "n" }, String(items.length))),
      kinds.map((k) => h("button", { class: `cv-chipf${S.actKinds.has(k) ? " on" : ""}`, type: "button", "aria-pressed": String(S.actKinds.has(k)), onclick: () => { if (S.actKinds.has(k)) S.actKinds.delete(k); else S.actKinds.add(k); keep(); paintAct([]); } }, h("span", { class: `cv-k k-${k}` }, ACT[k] || k), h("span", { class: "n" }, String(counts[k] || 0))))]);
    // Rows: the new ones on top; the page stays where you are reading.
    const scroller = scrollerOf(U.list);
    const anchor = scroller && scroller.scrollTop > 0 ? U.list.querySelector(".cv-ev:not([hidden])") : null;
    const top0 = anchor ? anchor.getBoundingClientRect().top : 0;
    if (!U.list.childNodes.length) for (const x of items) U.list.append(actRow(x));
    else { const at = U.list.firstChild; for (const x of fresh) if (!U.nodes.has(x.id)) U.list.insertBefore(actRow(x), at); }
    for (const [id, n] of U.nodes) {
      n.el.hidden = S.actKinds.size ? !S.actKinds.has(n.x.kind) : false;
      n.time.textContent = ago(n.x.ts) || "now";
      if (fresh.some((x) => x.id === id) && anchor == null && U.list.childNodes.length > fresh.length) { n.el.classList.add("cv-new"); setTimeout(() => n.el.classList.remove("cv-new"), 2500); }
    }
    if (anchor) scroller.scrollTop += anchor.getBoundingClientRect().top - top0;
  }
  function scrollerOf(el) {
    for (let e = el.parentElement; e; e = e.parentElement) { const o = getComputedStyle(e).overflowY; if ((o === "auto" || o === "scroll") && e.scrollHeight > e.clientHeight) return e; }
    return document.scrollingElement;
  }
  function actRow(x) {
    const time = h("span", { class: "tm" }, ago(x.ts) || "now");
    const failed = x.kind === "gate" && (x.passed === false || x.detail?.passed === false);
    const head = h("button", { class: "cv-ev-h", type: "button", "aria-expanded": "false" },
      h("span", { class: `cv-k k-${x.kind}${failed ? " bad" : ""}` }, ACT[x.kind] || x.kind || "–"),
      // The server's summaries often start with the actor's name: said once.
      h("span", { class: "tx" }, x.actor ? h("b", {}, x.actor, " ") : null, x.actor && (x.summary || "").startsWith(x.actor) ? x.summary.slice(x.actor.length).replace(/^[\s:,]+/, "") : x.summary || ""),
      time);
    const body = h("div", { class: "cv-ev-b", hidden: true });
    const el = h("li", { class: "cv-ev", "data-kind": x.kind || "" }, head, body);
    head.addEventListener("click", () => {
      const o = body.hidden;
      body.hidden = !o; el.classList.toggle("open", o); head.setAttribute("aria-expanded", String(o));
      if (o && !body.childNodes.length) body.append(...actDetail(x));
    });
    S.actUI.nodes.set(x.id, { el, x, time });
    return el;
  }
  function actDetail(x) {
    const t = toMs(x.ts);
    const links = [];
    const fid0 = typeof x.finding === "object" ? x.finding?.id : x.finding;
    const run0 = typeof x.run === "object" ? x.run?.id : x.run;
    const ag = typeof x.agent === "object" ? x.agent?.id : x.agent;
    if (fid0) links.push(h("a", { class: "cv-btn", href: C.href("activity", { f: fid0 }) }, "The card"));
    if (run0) links.push(h("a", { class: "cv-btn", href: C.href("activity", { run: run0 }) }, "The interview"));
    if (ag) links.push(h("a", { class: "cv-btn", href: `#/a/${ag}/terminal` }, "The terminal"));
    if (x.persona) links.push(h("a", { class: "cv-btn", href: C.href("personas") }, "Personas"));
    return [
      h("p", { class: "s" }, x.summary || "", t ? h("span", { class: "cv-meta" }, ` · ${new Date(t).toLocaleString([], { month: "short", day: "numeric", hour: "2-digit", minute: "2-digit", second: "2-digit" })}`) : null),
      x.detail != null && !(typeof x.detail === "object" && !Object.keys(x.detail).length) ? kvOf(x.detail) : null,
      links.length ? h("div", { class: "cv-meta cv-btns" }, links) : null].filter(Boolean);
  }
  /** Evidence as key/values; nested values as pretty JSON. */
  function kvOf(v) {
    if (v == null || typeof v !== "object" || Array.isArray(v) && v.some((x) => x && typeof x === "object")) return h("pre", { class: "cv-pre" }, typeof v === "string" ? v : JSON.stringify(v, null, 2));
    if (Array.isArray(v)) return h("ul", { class: "cv-checks" }, v.map((x) => h("li", {}, String(x))));
    return h("dl", { class: "cv-kv" }, Object.entries(v).map(([k, x]) => [h("dt", {}, k.replace(/_/g, " ")),
      h("dd", {}, x == null ? "–" : typeof x !== "object" ? String(x) : Array.isArray(x) && x.every((y) => y == null || typeof y !== "object") ? (x.length ? (x.join(", ").length < 120 ? x.join(", ") : h("ul", { class: "cv-checks" }, x.map((y) => h("li", {}, String(y))))) : "none") : h("pre", { class: "cv-pre" }, JSON.stringify(x, null, 2)))]));
  }
  function actPoll() {
    if (S.actTimer) return;
    fetchAct();
    S.actTimer = setInterval(() => { if (view() !== "activity") { actStop(); return; } if (!document.hidden) fetchAct(); }, 4000);
  }
  function actStop() { clearInterval(S.actTimer); S.actTimer = null; }

  if (S.actUI) S.actUI.chips.className = "cv-chips";
  actPoll();
  (S.stops = S.stops || []).push(actStop);
  return h("div", {}, stepHead(C, "Activity log", null, "Every step the autopilot takes, and why. Tap a line for its evidence."), activity());
}
