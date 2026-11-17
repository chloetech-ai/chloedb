// Converge (src/converge.rs, SPEC.md "Seventh build: Converge"): personas
// play the product, what they hit becomes problems, and each problem walks
// the flow to a pull request you approve. One screen a step:
//   #/converge                 Home: the pipeline, five steps with counts
//   #/converge/personas        Run a persona: the ranked list, Play, live run
//   #/converge/problems        Problems: triage, filter, Reproduce or Dismiss
//   #/converge/reproducing     Reproducing: each repro agent's progress
//   #/converge/ready           Ready to build: the plans, by score; Build
//   #/converge/building        Building: the fix agents, their checks, verify
//   #/converge/merge           Ready to merge: the verdict; Approve and merge
//   #/converge/metrics | settings | activity   under the "⋯" menu
// A card opens in a sheet (?f=<id>), a persona's run in one too (?run=<id>);
// every card's sheet ends with "Talk to the agent".
//   mount(root, ctx) -> { update(state), unmount() }
// The modules are under goals/converge/ (each listed in src/server.rs).
import { h, put, toast, enc, STEPS, blockedOf, queuedOf, agentOn, local } from "/goals/converge/util.js";
import { styles } from "/goals/converge/style.js";
import { closeSheets } from "/goals/converge/kit.js";
import { homeScreen, topBar } from "/goals/converge/home.js";
import { problemsScreen } from "/goals/converge/problems.js";
import { stepScreen } from "/goals/converge/steps.js";
import { openSheet } from "/goals/converge/sheet.js";
import { openRun } from "/goals/converge/run.js";
import { personasScreen } from "/goals/converge/personas.js";
import { metricsScreen } from "/goals/converge/metrics.js";
import { settingsScreen } from "/goals/converge/settings.js";
import { activityScreen } from "/goals/converge/activity.js";

export { THEME_WORDS, doingWords } from "/goals/converge/util.js";

/** The label of the sheet's kind select (a test reads it here). */
export const KIND_LABEL = "Kind of problem";

const VIEWS = new Set(["home", "problems", "reproducing", "ready", "building", "merge", "personas", "metrics", "settings", "activity"]);
/** Old addresses: the board and the Agents tab are Home now. */
const OLD = { board: "home", work: "home" };

/** The last data and view settings of each repository, kept across visits:
 *  coming back draws at once, then refreshes in place. */
const KEEP = new Map();

export function mount(root, ctx) {
  styles();
  const host = h("div", { class: "cv" });
  put(root, host);
  const proj = () => (ctx.params && ctx.params.project) || "";
  const projects = () => { try { return (typeof ctx.projects === "function" ? ctx.projects() : []) || []; } catch { return []; } };
  const projRow = () => { const n = proj(); return n ? projects().find((p) => p.project === n || p.path === n) || null : null; };
  const repo = () => { const p = projRow(); return p ? p.path : proj() && !projects().length ? (ctx.params.repo || "") : ""; };
  const projName = () => (projRow() || {}).project || proj();
  const kept = KEEP.get(repo()) || {};
  const S = {
    d: kept.d || null, sig: kept.sig, alive: true, timer: null,
    // The Problems screen's filters, its selection; the metrics' range and hidden series.
    pf: kept.pf || { sev: new Set(), kind: new Set(), comp: new Set(), persona: new Set(), fresh: false },
    sel: new Map(kept.sel || []), range: kept.range, hide: new Set(kept.hide || []), act: kept.act || null, actKinds: new Set(kept.actKinds || []),
    gone: new Set(), // rows swiped away, waiting for their undo to pass
  };
  const keep = () => { if (repo()) KEEP.set(repo(), { d: S.d, sig: S.sig, pf: S.pf, sel: [...S.sel], range: S.range, hide: [...S.hide], act: S.act, actKinds: [...S.actKinds] }); };
  const api = (p, o) => ctx.api(p, o);
  const query = () => new URLSearchParams(location.hash.split("?")[1] || "");
  const view = () => { const v = location.hash.split("?")[0].split("/")[2] || "home"; const x = OLD[v] || v; return VIEWS.has(x) ? x : "home"; };
  /** A Converge address that keeps the project. */
  const href = (v, extra = {}) => {
    const q = new URLSearchParams();
    if (proj()) q.set("project", proj());
    for (const [k, x] of Object.entries(extra)) if (x != null && x !== "") q.set(k, x);
    const s = q.toString();
    return `#/converge${v && v !== "home" ? "/" + v : ""}${s ? "?" + s : ""}`;
  };
  /** This screen's address, with `extra` (and the `from` filter kept). */
  const here = (extra = {}) => href(view(), { from: query().get("from"), ...extra });

  // When the viewer last looked at Problems, for "new since last visit".
  const seenKey = () => `cv-seen:${repo()}`;
  S.seenAt = Number(local.get(seenKey())) || 0;

  async function load() {
    if (!repo()) { picker(); return; }
    try {
      const d = await api(`/api/converge?repo=${enc(repo())}`);
      // Nothing changed: keep the page as it is (its scroll, a row being swiped).
      const sig = JSON.stringify({ ...d, metrics: { ...d.metrics, history: (d.metrics?.history || []).length } }).replace(/"(stage_since|updated|ts_ms|last_run|now)":\d+/g, "");
      if (S.d && sig === S.sig) return;
      S.d = d;
      S.sig = sig;
    } catch (err) {
      if (!S.d) put(host, h("div", { class: "cv-none" }, h("p", {}, `Converge did not load: ${err.message || err}`), h("button", { class: "cv-btn", type: "button", onclick: () => load() }, "Try again")));
      return;
    }
    if (S.alive) draw();
  }

  async function act(fn, msg) {
    try { const r = await fn(); if (msg) toast(msg); S.sig = ""; await load(); return r; } catch (err) { toast(err.message || String(err)); S.sig = ""; await load(); return null; }
  }
  const post = (id, verb, body) => api(`/api/converge/findings/${enc(id)}/${verb}`, { method: "POST", body: body || {} });

  /** Several cards, one verb: the bulk route, or one call each on an older server. */
  async function bulk(ids, verb, extra = {}) {
    if (!ids.length) return { done: [], failed: [] };
    try {
      const r = await api("/api/converge/findings/bulk", { method: "POST", body: { repo: repo(), ids, verb, ...extra } });
      return { done: Array.isArray(r?.done) ? r.done : ids, failed: Array.isArray(r?.failed) ? r.failed : [] };
    } catch (err) {
      if (err.status !== 404 && err.status !== 405) throw err;
      const done = [], failed = [];
      for (const id of ids) { try { await post(id, verb, extra); done.push(id); } catch (e) { failed.push({ id, error: e.message || String(e) }); } }
      return { done, failed };
    }
  }

  const findings = () => (S.d?.findings || []).filter((f) => !S.gone.has(f.id));
  const settings = () => S.d?.settings || {};
  /** What needs you: blocked first, then the picks and approvals that are yours. Queued waits for a slot, not you. */
  function needs() {
    const blocked = [], rest = [];
    for (const f of findings()) {
      if (f.state !== "active") continue;
      if (blockedOf(f)) { blocked.push(f); continue; }
      if (queuedOf(f)) continue;
      // The server's one rule (needs_now) when it sends it, so every count agrees.
      const why = "needs_now" in f ? f.needs_now : f.needs_you ? f.needs_you : f.stage === "review" && f.ci === "pass" ? "CI passed: approve the merge." : f.stage === "prioritize" && f.door ? `Touches a one-way door (${f.door}): only you start it.` : null;
      if (why) rest.push({ f, why });
    }
    return { blocked, rest, count: blocked.length + rest.length };
  }
  /** The five steps' counts: the server's `steps`, else counted here. */
  function steps() {
    const srv = Array.isArray(S.d?.steps) ? S.d.steps : [];
    return STEPS.map((s) => {
      const items = findings().filter((f) => f.state === "active" && s.stages.includes(f.stage));
      const x = srv.find((y) => y && y.step === s.step) || {};
      const num = (v, dflt) => (Number.isFinite(Number(v)) && v != null ? Number(v) : dflt);
      return {
        ...s, items,
        count: num(x.count, items.length),
        moving: num(x.moving, items.filter((f) => agentOn(f) || f.busy).length),
        blocked: num(x.blocked, items.filter((f) => blockedOf(f)).length),
        queued: num(x.queued, items.filter((f) => queuedOf(f)).length),
      };
    });
  }

  const C = {
    S, ctx, api, repo, proj, projName, projects, href, here, view, query, load, act, post, bulk, keep,
    findings, settings, needs, steps, draw: () => draw(), KIND_LABEL,
    open: (id) => { location.hash = here({ f: id }); },
    openRun: (id) => { location.hash = here({ run: id }); },
    markSeen: () => { const now = Date.now(); local.set(seenKey(), String(now)); },
  };

  // Where the page is scrolled, and what has the keyboard's focus, kept
  // across a redraw. Parts that fill in a moment later (the activity list,
  // charts) leave the page short at first, so the height is held and the
  // scroll set again once they have, unless you scrolled in between.
  function scrollState() {
    const els = [];
    for (let e = host; e; e = e.parentElement) if (e.scrollTop) els.push([e, e.scrollTop]);
    if (document.scrollingElement && document.scrollingElement.scrollTop && !els.some(([e]) => e === document.scrollingElement)) els.push([document.scrollingElement, document.scrollingElement.scrollTop]);
    const strips = [...host.querySelectorAll(".cv-chips")].map((w) => w.scrollLeft);
    const a = document.activeElement;
    const focus = a && a !== document.body && host.contains(a) ? focusKey(a) : null;
    const tall = host.offsetHeight;
    const moved = S.userMoved || 0;
    return () => {
      if (tall) host.style.minHeight = `${tall}px`;
      const apply = () => {
        if ((S.userMoved || 0) !== moved) return;
        for (const [e, t] of els) if (Math.abs(e.scrollTop - t) > 1) e.scrollTop = t;
      };
      apply();
      host.querySelectorAll(".cv-chips").forEach((w, i) => { if (strips[i] != null) w.scrollLeft = strips[i]; });
      if (focus && (!document.activeElement || document.activeElement === document.body)) refocus(focus);
      requestAnimationFrame(apply);
      setTimeout(apply, 300);
      clearTimeout(S.heldTimer);
      S.heldTimer = setTimeout(() => { apply(); host.style.minHeight = ""; }, 1500);
    };
  }
  /** How to find an element again after a redraw: its id, its row's id, or its place among the focusable ones. */
  function focusKey(el) {
    const row = el.closest("[data-id], [data-fid]");
    const all = [...host.querySelectorAll("a[href], button, input, select, textarea, [tabindex]")];
    return { id: el.id || null, row: row ? row.dataset.id || row.dataset.fid : null, tag: el.tagName, label: el.getAttribute("aria-label") || el.textContent.trim().slice(0, 40), i: all.indexOf(el) };
  }
  function refocus(k) {
    const all = [...host.querySelectorAll("a[href], button, input, select, textarea, [tabindex]")];
    let el = k.id ? host.querySelector(`#${CSS.escape(k.id)}`) : null;
    if (!el && k.row) {
      const row = host.querySelector(`[data-id="${CSS.escape(k.row)}"], [data-fid="${CSS.escape(k.row)}"]`);
      el = row && ([...row.querySelectorAll(k.tag)].find((e) => (e.getAttribute("aria-label") || e.textContent.trim().slice(0, 40)) === k.label) || (row.matches(k.tag) ? row : null));
    }
    if (!el) el = all.find((e) => e.tagName === k.tag && (e.getAttribute("aria-label") || e.textContent.trim().slice(0, 40)) === k.label) || (k.i >= 0 ? all[k.i] : null);
    if (el && typeof el.focus === "function") el.focus({ preventScroll: true });
  }

  /** No project: each one as a large card. */
  function picker() {
    if (ctx.crumbs) ctx.crumbs([{ label: "Converge", href: "#/converge" }]);
    const ps = projects();
    const unknown = proj() && !projRow();
    put(host,
      h("div", { class: "cv-sh" }, h("div", { class: "row" }, h("h1", {}, "Converge"))),
      h("p", { class: "cv-intro" }, unknown ? `There is no open project named “${proj()}”. Pick one, or open it first.` : "Pick a project. Personas play it, and what they hit is reproduced, fixed and sent to you as a pull request."),
      h("div", { class: "cv-pjs" }, ps.map((p) => {
        const m = KEEP.get(p.path)?.d?.metrics;
        return h("a", { class: "cv-pj", href: `#/converge?project=${enc(p.project)}` }, h("span", { class: "n" }, p.project), h("span", { class: "p" }, p.path),
          m ? h("span", { class: "cv-meta" }, `${m.open || 0} open`) : null);
      }), h("a", { class: "cv-pj add", href: "#/p" }, h("span", { class: "n" }, "+ Open a project"), h("span", { class: "p" }, "A git checkout on this machine, or from GitHub"))));
    if (!ps.length && ctx.refreshProjects) ctx.refreshProjects().then(() => { if (S.alive && projects().length) load(); }, () => {});
    // No project may only mean the server refused this page's token: say that, not "no project".
    if (!ps.length || unknown) {
      api("/api/needs").then(() => {}, (err) => {
        if (!S.alive || !(err && (err.status === 401 || err.status === 403)) || repo()) return;
        if (ctx.crumbs) ctx.crumbs([{ label: "Converge", href: "#/converge" }]);
        put(host, h("div", { class: "cv-none" },
          h("h1", {}, "Not signed in"),
          h("p", {}, `Workbench refused this page's access token (${err.status}), so it cannot list your projects${proj() ? ` or open “${proj()}”` : ""}. Open Workbench again from the address \`chloedb-workbench serve\` printed (it carries the token), then come back.`),
          h("button", { class: "cv-btn", type: "button", onclick: () => location.reload() }, "Try again")));
      });
    }
  }

  function draw() {
    if (!S.d) return;
    keep();
    const v = view();
    // Home's crumbs are the project alone, so a phone gets no Back arrow to the picker.
    const crumbs = [{ label: projName(), href: href("home") }];
    const step = STEPS.find((s) => s.step === v);
    const extra = { personas: "Personas", metrics: "Metrics", settings: "Autopilot", activity: "Activity log" };
    if (step || extra[v]) crumbs.push({ label: step ? step.name : extra[v], href: href(v) });
    if (ctx.crumbs) ctx.crumbs(crumbs);
    const body = v === "home" ? homeScreen(C)
      : v === "problems" ? problemsScreen(C)
        : v === "personas" ? personasScreen(C)
          : v === "metrics" ? metricsScreen(C)
            : v === "settings" ? settingsScreen(C)
              : v === "activity" ? activityScreen(C)
                : stepScreen(C, v);
    const restore = scrollState();
    put(host, v === "home" ? topBar(C) : null, body);
    restore();
    syncSheet();
  }

  /** The sheet the address names: open it, or close what it no longer names. */
  function syncSheet() {
    const q = query();
    const f = q.get("f"), r = q.get("run");
    const open = document.querySelector(".cv-sheet");
    if (f) { if (open?.dataset.id !== f) openSheet(C, f); }
    else if (r) { if (open?.dataset.run !== r) openRun(C, r); }
    else if (open && (open.dataset.id || open.dataset.run)) closeSheets();
  }

  let lastView = view();
  const onHash = () => {
    if (!location.hash.startsWith("#/converge") || !S.d) return;
    if (view() !== lastView) {
      if (lastView === "problems") C.markSeen();
      lastView = view();
      closeSheets();
      const sc = host.parentElement; if (sc) sc.scrollTop = 0;
      draw();
      return;
    }
    syncSheet();
  };
  const touched = () => { S.touched = Date.now(); };
  host.addEventListener("pointerdown", touched, { passive: true });
  host.addEventListener("scroll", touched, { passive: true, capture: true });
  // Your own scrolling: a redraw's late restore must not undo it.
  const moved = () => { S.userMoved = (S.userMoved || 0) + 1; };
  for (const ev of ["wheel", "touchmove", "keydown"]) window.addEventListener(ev, moved, { passive: true });
  const onKey = (e) => {
    if (e.key !== "Escape") return;
    // Esc in a box you are typing in leaves the box, not the sheet (the draft stays).
    const t = document.activeElement;
    if (t && t.matches && t.matches("textarea, input[type=text], input:not([type])") && t.value) { t.blur(); e.preventDefault(); return; }
    const pop = document.querySelector(".cv-popscrim"); if (pop) { pop.click(); return; }
    const x = document.querySelector(".cv-sheet .cv-x"); if (x) x.click();
  };
  window.addEventListener("hashchange", onHash);
  document.addEventListener("keydown", onKey);
  if (S.d && repo()) draw();
  load();
  S.timer = setInterval(() => {
    if (!repo()) { if (!document.hidden) load(); return; }
    if (document.hidden || document.querySelector(".cv-sheet, .cv-pop, .cv-menu") || document.querySelector(".cv input:focus, .cv textarea:focus, .cv select:focus")) return;
    if (S.swiping || Date.now() - (S.touched || 0) < 4000) return;
    // Settings being edited are not redrawn under you; Save reloads them.
    if (view() === "settings") return;
    load();
  }, 8000);
  return {
    update() {},
    unmount() {
      S.alive = false;
      if (view() === "problems") C.markSeen();
      clearInterval(S.timer);
      (S.stops || []).forEach((f) => { try { f(); } catch { /* gone */ } });
      window.removeEventListener("hashchange", onHash);
      document.removeEventListener("keydown", onKey);
      for (const ev of ["wheel", "touchmove", "keydown"]) window.removeEventListener(ev, moved);
      closeSheets();
      document.querySelectorAll(".cv-menu, .cv-pop, .cv-popscrim").forEach((e) => e.remove());
    },
  };
}
