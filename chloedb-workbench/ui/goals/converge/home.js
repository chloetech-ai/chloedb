// The inbox (#/converge/inbox; the overview is home now). The project and its status line on top (sticky on a
// phone), the five steps' counts as one compact strip, then the three
// decisions that are yours, in this order: "Merge this?" (green and
// mergeable), "Build these?" (planned), "Worth fixing?" (new problems).
// Each row has one primary move and one secondary. Everything moving
// without you (reproducing, planning, building, verifying, blocked) is one
// "In progress" strip, a bar and a live line a card; all of it is
// #/converge/progress. Keys (desktop): j/k, Enter, r, b, d, m, ? for help.
import { h, put, toast, enc, usd, ago, toMs, SEV, STAGE_NAME, statusLine, flagLine, thinBar, blockedOf, queuedOf, agentOn, happening, seenOf, similarOf, evidenceOf, inboxOf, deferVerb, firstName, firstSentences, fmtScore } from "/goals/converge/util.js";

/** The project's name as the title; a native select over it switches. */
function projectTitle(C) {
  const ps = C.projects();
  const name = C.projName();
  const sel = h("select", { class: "cv-psel", "aria-label": "Project", onchange: () => { location.hash = sel.value ? `#/converge?project=${enc(sel.value)}` : "#/converge"; } },
    ps.map((p) => h("option", { value: p.project, selected: p.project === name ? true : null }, p.project)),
    ps.some((p) => p.project === name) ? null : h("option", { value: name, selected: true }, name),
    h("option", { value: "" }, "All projects…"));
  return h("h1", { class: "cv-ptitle" }, h("span", { class: "cv-brand" }, "Converge"), h("span", { class: "cv-pname" }, name, h("span", { class: "car", "aria-hidden": "true" }, "▾")), sel);
}

/** One tap: Stop everything (the autopilot and your picks pause, every
 * agent and run stops), and Resume while stopped. */
export function stopButton(C, always) {
  const st = C.settings();
  const stopped = !!st.stopped;
  const busy = (C.S.d.findings || []).some((f) => agentOn(f) || f.busy) || (C.S.d.runs || []).some((r) => r.state === "running");
  if (!always && !stopped && !busy && !st.enabled && !st.follow_picks) return null;
  const b = h("button", { class: `cv-btn ${stopped ? "pri" : "crit"}`, type: "button", onclick: async () => {
    b.disabled = true;
    try {
      const r = await C.api("/api/converge/stop", { method: "POST", body: { repo: C.repo(), resume: stopped } });
      toast(stopped ? `Resumed: the autopilot is ${r.enabled ? "on" : "off"}${r.follow_picks ? ", your picks move by themselves" : ""}` : `Stopped ${r.agents} agent${r.agents === 1 ? "" : "s"}${r.runs ? ` and ${r.runs} run${r.runs === 1 ? "" : "s"}` : ""}. Nothing moves until you resume.`);
    } catch (err) { toast(err.message || String(err)); }
    C.S.sig = ""; await C.load();
  } }, stopped ? "Resume" : [h("span", { class: "w-lg" }, "Stop everything"), h("span", { class: "w-sm" }, "Stop all")]);
  return b;
}

export function topBar(C) {
  return h("div", { class: "cv-top" },
    h("div", { class: "cv-top-r" },
      projectTitle(C),
      h("div", { class: "cv-top-a" },
        stopButton(C),
        h("button", { class: "cv-btn icon quiet cv-keyhelp", type: "button", "aria-label": "Keyboard shortcuts (?)", title: "Keyboard shortcuts (?)", onclick: () => keyHelp() }, "?"),
        h("a", { class: "cv-btn pri", href: C.href("personas") }, "Run personas"))),
    autoLine(C));
}

/** One calm line: the autopilot, what it last did, the spend. */
function autoLine(C) {
  const st = C.settings(), m = C.S.d.metrics || {};
  const lt = C.S.d.last_tick;
  const word = (x) => (typeof x === "string" ? x : x && (x.summary || x.text || x.why || x.decision)) || "";
  const last = lt ? ((lt.decisions || []).map(word).filter(Boolean)[0] || lt.summary || "") : "";
  if (st.stopped) return h("a", { class: "cv-auto", href: C.href("activity") }, h("span", { class: "dot", "aria-hidden": "true" }), h("span", { class: "s" }, `Stopped by you${ago(st.stopped.at) && ago(st.stopped.at) !== "now" ? ` ${ago(st.stopped.at)} ago` : ""}: tap Resume to carry on`));
  const parts = [st.enabled ? `Autopilot on${st.tick_s ? ` · every ${Math.max(1, Math.round(st.tick_s / 60))} min` : ""}` : "Autopilot off", `${usd(m.spent_today)} of ${usd(st.budget_usd_day)} today`];
  if (last) parts.push(`${ago(lt.ts) || "now"}: ${last}`);
  return h("a", { class: "cv-auto", href: C.href("activity") }, h("span", { class: `dot${st.enabled ? " on" : ""}`, "aria-hidden": "true" }), h("span", { class: "s" }, parts.join(" · ")));
}


const SHORT = { problems: "Found", reproducing: "Repro", ready: "Ready", building: "Building", merge: "Merge" };
/** The five steps as one compact strip: a count each, a link to its list. */
function pipeStrip(C) {
  const st = C.steps();
  return h("nav", { class: "cv-strip", "aria-label": "The steps" }, st.map((s) =>
    h("a", { class: `cv-sp5${s.count ? " has" : ""}${s.blocked ? " blk" : ""}`, href: C.href(s.step), "aria-label": `${s.name}: ${s.count}${s.moving ? `, ${s.moving} moving` : ""}` },
      h("span", { class: "n" }, String(s.count)), h("span", { class: "l" }, h("span", { class: "w-lg" }, s.name), h("span", { class: "w-sm" }, SHORT[s.step] || s.name)), s.moving ? h("i", { class: "cv-pulse", "aria-hidden": "true" }) : null)));
}

/** Why a merge waits for you, from its evidence when the server sends it. */
function mergeWhy(C, f) {
  const e = evidenceOf(f);
  const bits = [];
  bits.push(f.ci === "pass" ? "CI green" : !f.pr_url ? "checks passed, no pull request" : "");
  const after = Array.isArray(e.repro_after) ? e.repro_after : [];
  if (after.length) bits.push(`repro passes ${after.filter((x) => Number(x) === 0).length} of ${after.length}`);
  const t = e.tests && typeof e.tests === "object" ? e.tests : null;
  if (t && t.pass != null) bits.push(t.pass ? "tests pass" : "tests fail");
  const rp = e.replay && typeof e.replay === "object" ? e.replay : null;
  const eb = rp?.ease_before ?? f.ease_before, ea = rp?.ease_after ?? f.ease_after;
  if (eb != null && ea != null) bits.push(`ease ${eb} → ${ea}`);
  const d = e.diff && typeof e.diff === "object" ? e.diff : null;
  if (d && d.files != null) bits.push(`${d.files} file${d.files === 1 ? "" : "s"} +${d.add ?? 0} −${d.del ?? 0}`);
  return bits.filter(Boolean).join(" · ");
}
function buildWhy(C, f) {
  const sc = f.scope && typeof f.scope === "object" ? f.scope : {};
  return [`score ${fmtScore(f.score)}`, sc.size ? `size ${sc.size}` : null, sc.area || null, f.door ? `one-way door: ${f.door}` : null].filter(Boolean).join(" · ");
}
function fixWhy(C, f) {
  const hits = f.personas_hit && f.personas_hit.length ? f.personas_hit : f.persona ? [f.persona] : [];
  const pn = (id) => firstName(((C.S.d.personas || []).find((p) => p.id === id) || {}).name || id);
  const who = hits.length ? `${hits.slice(0, 2).map(pn).join(", ")}${hits.length > 2 ? ` +${hits.length - 2}` : ""} hit it` : f.by ? `reported by ${f.by}` : "";
  return [who, firstSentences(f.detail, 1)].filter(Boolean).join(": ");
}

/** "Looks like: <title>" for each similar card, and "Same problem — fold" when the server folds. */
function similarLine(C, f) {
  const ids = similarOf(f).filter((x) => x !== f.id);
  if (!ids.length) return null;
  const all = C.S.d.findings || [];
  const items = ids.map((id) => all.find((x) => x.id === id)).filter(Boolean).slice(0, 2);
  if (!items.length) return null;
  return h("span", { class: "ib-like" }, items.map((o) => h("span", { class: "lk" },
    h("span", { class: "w" }, "Looks like: "),
    h("a", { href: C.here({ f: o.id }), onclick: (e) => e.stopPropagation() }, o.title),
    C.S.noFold || o.state !== "active" ? null : h("button", { class: "cv-btn quiet sm", type: "button", title: `Fold this card into “${o.title}”`, onclick: (e) => { e.stopPropagation(); fold(C, f, o); } }, "Same problem — fold"))));
}
export async function fold(C, f, into) {
  try {
    await C.post(f.id, "fold", { into: into.id });
    toast(`Folded into “${into.title}”`);
  } catch (err) {
    if (err.status === 404 || err.status === 405) { C.S.noFold = true; toast("This server cannot fold cards: dismiss one instead"); }
    else toast(err.message || String(err));
  }
  C.S.sig = ""; await C.load();
}

/** The moves of an inbox row: [secondary, primary]. */
function moves(C, kind, f) {
  const btn = (label, cls, fn, key) => h("button", { class: `cv-btn ${cls}`, type: "button", title: key ? `${label} (${key})` : label, onclick: (e) => { e.stopPropagation(); fn(); } }, label);
  if (kind === "merge") return [btn("Send back", "", () => { C.S.sendBack = true; C.open(f.id); }), btn("Review and merge", "good", () => C.open(f.id), "m")];
  if (kind === "build") return [btn("Dismiss", "", () => askDismiss(C, f), "d"), btn(f.door ? "Build (one-way door)" : "Build", "pri", () => build(C, [f]), "b")];
  return [btn("Dismiss", "", () => askDismiss(C, f), "d"), btn("Reproduce", "pri", () => reproduce(C, [f.id]), "r")];
}
export function reproduce(C, ids) { deferVerb(C, ids, "reproduce", {}, ids.length > 1 ? `${ids.length} on to Reproducing` : "On to Reproducing: an agent writes the repro"); }
export function dismiss(C, ids, reason) { deferVerb(C, ids, "dismiss", { reason: reason || "Not a problem" }, ids.length > 1 ? `${ids.length} dismissed` : "Dismissed"); }
export async function build(C, list) {
  const doors = list.filter((f) => f.door);
  if (doors.length && !confirm(`${doors.length === 1 ? `“${doors[0].title}” touches` : `${doors.length} of these touch`} a one-way door. Build anyway?`)) return;
  const r = await C.act(() => C.bulk(list.map((f) => f.id), "start"), null);
  if (r) toast(r.failed && r.failed.length ? `${r.done.length} started; ${r.failed.length} did not: ${r.failed[0].error}` : `${r.done.length > 1 ? `${r.done.length} fix agents` : "A fix agent"} on it`);
}
export function askDismiss(C, f) { C.S.dismissRow = f.id; C.S.cur = f.id; C.draw(); }

export function dismissBox(C, f) {
  const why = h("input", { class: "cv-in", type: "text", placeholder: "Why is it not worth fixing?", "aria-label": `Why dismiss “${f.title}”?`, enterkeyhint: "done" });
  const cancel = () => { C.S.dismissRow = null; C.draw(); };
  const go = () => { const r = why.value.trim(); if (!r) { why.focus(); toast("A reason, first: it teaches the personas"); return; } C.S.dismissRow = null; dismiss(C, [f.id], r); };
  why.addEventListener("keydown", (e) => { if (e.key === "Enter") { e.preventDefault(); go(); } else if (e.key === "Escape") { e.preventDefault(); e.stopPropagation(); cancel(); } });
  setTimeout(() => why.focus(), 0);
  return h("div", { class: "ib-dis" }, why, h("button", { class: "cv-btn", type: "button", onclick: cancel }, "Cancel"), h("button", { class: "cv-btn crit", type: "button", onclick: go }, "Dismiss"));
}

/** One row of a decision. */
function inboxRow(C, kind, f) {
  const S = C.S;
  const seen = seenOf(f);
  const why = kind === "merge" ? mergeWhy(C, f) || f.needs_now || "Ready for your approval" : kind === "build" ? buildWhy(C, f) : fixWhy(C, f);
  const ck = kind === "fix" ? h("input", { type: "checkbox", "aria-label": `Select “${f.title}”`, checked: S.sel.has(f.id) ? true : null, onchange: (e) => { if (e.target.checked) S.sel.set(f.id, true); else S.sel.delete(f.id); C.keep(); C.draw(); } }) : null;
  const main = h("button", { class: "ib-main", type: "button", onclick: () => { S.cur = f.id; C.open(f.id); } },
    h("span", { class: "ib-t" }, h("span", { class: "cv-sev", "data-sev": String(f.severity), "aria-hidden": "true" }), h("span", { class: "ttl" }, f.title)),
    h("span", { class: "ib-why" }, why),
    h("span", { class: "ib-meta" },
      h("span", { class: `sv s${f.severity}` }, SEV[f.severity] || ""),
      seen > 1 ? h("span", { class: "cv-tag seen", title: `${seen} reports were folded into this card` }, `seen ${seen}×`) : null,
      f.component ? h("span", {}, f.component) : null,
      h("span", { class: "tm" }, ago(f.stage_since || f.created))));
  return h("li", { class: `cv-ib${S.cur === f.id ? " cur" : ""}${S.sel.has(f.id) ? " sel" : ""}`, "data-id": f.id, "data-kind": kind },
    ck ? h("label", { class: "ib-ck" }, ck) : null,
    h("div", { class: "ib-body" }, main, similarLine(C, f)),
    h("div", { class: "ib-acts" }, moves(C, kind, f)),
    S.dismissRow === f.id ? dismissBox(C, f) : null);
}

function section(C, kind, title, hint, items, extra) {
  if (!items.length) return null;
  const N = C.S.showAll && C.S.showAll[kind] ? items.length : 8;
  return h("section", { class: `cv-ibs k-${kind}`, "aria-label": title },
    h("div", { class: "cv-ibh" }, h("h2", {}, title, h("span", { class: "c" }, String(items.length))), h("span", { class: "hint" }, hint), extra || null),
    h("ul", { class: "cv-iblist" }, items.slice(0, N).map((f) => inboxRow(C, kind, f))),
    items.length > N ? h("button", { class: "cv-btn quiet", type: "button", onclick: () => { C.S.showAll = { ...(C.S.showAll || {}), [kind]: true }; C.draw(); } }, `Show all ${items.length}`) : null);
}

/** A card moving without you: title, its live line, its bar. */
export function progressRow(C, f) {
  return h("li", { class: `cv-ip${blockedOf(f) ? " blocked" : ""}${C.S.cur === f.id ? " cur" : ""}`, "data-id": f.id },
    h("button", { class: "ip-main", type: "button", onclick: () => { C.S.cur = f.id; C.open(f.id); } },
      h("span", { class: "ip-t" }, h("span", { class: "cv-sev", "data-sev": String(f.severity), "aria-hidden": "true" }), h("span", { class: "ttl" }, f.title)),
      statusLine(f, C.settings()),
      flagLine(f),
      thinBar(f, C.settings())));
}

function progressStrip(C, moving) {
  if (!moving.length) return null;
  const N = 4;
  const live = moving.filter((f) => agentOn(f) || f.busy).length;
  const blk = moving.filter((f) => blockedOf(f)).length;
  return h("section", { class: "cv-ibs k-moving", "aria-label": "In progress" },
    h("div", { class: "cv-ibh" }, h("h2", {}, "In progress", h("span", { class: "c" }, String(moving.length))),
      h("span", { class: "hint" }, [live ? `${live} agent${live > 1 ? "s" : ""} at work` : "waiting for a free agent", blk ? `${blk} blocked on you` : null].filter(Boolean).join(" · ")),
      h("a", { class: "cv-btn quiet", href: C.href("progress") }, moving.length > N ? `All ${moving.length} ›` : "Open ›")),
    h("ul", { class: "cv-iplist" }, moving.slice(0, N).map((f) => progressRow(C, f))));
}

/** #/converge/progress: everything moving without you. */
export function progressScreen(C) {
  const { moving } = inboxOf(C.findings());
  return h("div", {},
    h("header", { class: "cv-sh" }, h("a", { class: "cv-back", href: C.href("home") }, "‹ Overview"),
      h("div", { class: "row" }, h("h1", {}, "In progress", h("span", { class: "c" }, String(moving.length)))),
      h("p", { class: "hint" }, "Reproducing, planning, building and verifying: agents move these without you. Blocked ones wait for a step only you can take.")),
    moving.length ? h("ul", { class: "cv-iplist full" }, moving.map((f) => progressRow(C, f))) : h("div", { class: "cv-none" }, h("p", {}, "Nothing is moving. Pick something in the inbox."), h("a", { class: "cv-btn pri", href: C.href("home") }, "Back to the inbox")));
}

export function homeScreen(C) {
  const d = C.S.d;
  if (!(d.findings || []).length && !(d.personas || []).length) {
    return h("div", {},
      h("div", { class: "cv-none" },
        h("p", {}, "Nothing here yet. Personas play the product and what they hit shows up as problems. Start by writing personas from the code, or load ChloeDB's six evaluation personas and what they reported."),
        h("a", { class: "cv-btn pri", href: C.href("personas") }, "Set up personas"),
        h("button", { class: "cv-btn", type: "button", onclick: () => C.act(() => C.api("/api/converge/seed", { method: "POST", body: { repo: C.repo() } }), "Loaded the seed personas and their problems") }, "Load the seed personas")));
  }
  const S = C.S;
  const ib = inboxOf(C.findings());
  // The selection (Worth fixing?) keeps only what is still in it.
  for (const id of [...S.sel.keys()]) if (!ib.fix.some((f) => f.id === id)) S.sel.delete(id);
  const week = Date.now() - 7 * 86400000;
  const merged = Number.isFinite(Number(d.merged_week)) && d.merged_week != null ? Number(d.merged_week)
    : (d.findings || []).filter((f) => f.stage === "merged" && (toMs(f.stage_since) || 0) >= week).length;
  const allSel = ib.fix.length && ib.fix.every((f) => S.sel.has(f.id));
  const selAll = ib.fix.length > 1 ? h("label", { class: "ib-all" }, h("input", { type: "checkbox", checked: allSel ? true : null, "aria-label": "Select all new problems", onchange: (e) => { ib.fix.forEach((f) => (e.target.checked ? S.sel.set(f.id, true) : S.sel.delete(f.id))); C.keep(); C.draw(); } }), "All") : null;
  let bar = null;
  const n = S.sel.size;
  if (n) {
    const ids = [...S.sel.keys()];
    if (S.dismissing) {
      const why = h("input", { class: "cv-in", type: "text", placeholder: "Why are these not worth fixing?", "aria-label": "Why are these not worth fixing?" });
      why.addEventListener("keydown", (e) => { if (e.key === "Enter") go.click(); });
      const go = h("button", { class: "cv-btn crit", type: "button", onclick: () => { if (!why.value.trim()) { why.focus(); toast("A reason, first: it teaches the personas"); return; } S.dismissing = false; dismiss(C, ids, why.value.trim()); } }, "Dismiss");
      bar = h("div", { class: "cv-bulk", role: "toolbar" }, why, h("button", { class: "cv-btn", type: "button", onclick: () => { S.dismissing = false; C.draw(); } }, "Cancel"), go);
      setTimeout(() => why.focus(), 0);
    } else {
      bar = h("div", { class: "cv-bulk", role: "toolbar", "aria-label": "Selected" }, h("span", { class: "n" }, `${n} selected`),
        h("button", { class: "cv-btn", type: "button", onclick: () => { S.sel.clear(); C.draw(); } }, "Clear"),
        h("button", { class: "cv-btn", type: "button", onclick: () => { S.dismissing = true; C.draw(); } }, "Dismiss"),
        h("button", { class: "cv-btn pri", type: "button", onclick: () => reproduce(C, ids) }, n > 1 ? `Reproduce ${n}` : "Reproduce"));
    }
  } else S.dismissing = false;
  return h("div", { class: "cv-inbox" },
    pipeStrip(C),
    ib.count ? null : h("div", { class: "cv-none cv-zero" }, h("p", {}, h("b", {}, "Nothing needs you."), " ", ib.moving.length ? "The agents are on the rest." : "Play a persona to find problems, or write one down."), ib.moving.length ? null : h("a", { class: "cv-btn pri", href: C.href("personas") }, "Run personas")),
    section(C, "merge", "Merge this?", "Green and mergeable: the evidence, then your yes", ib.merge),
    section(C, "build", "Build these?", "Planned and scored: a fix agent takes each on its own branch", ib.build),
    section(C, "fix", "Worth fixing?", "New problems the personas hit: reproduce or dismiss", ib.fix, selAll),
    progressStrip(C, ib.moving),
    h("a", { class: "cv-merged", href: C.href("merge", { show: "merged" }) }, h("span", {}, merged ? `${merged} merged this week` : "Nothing merged this week yet"), h("span", { "aria-hidden": "true" }, "›")),
    bar);
}

// ------------------------------------------------------------ keys
const KEYS = [["j / k", "Next / previous card"], ["Enter", "Open the card"], ["r", "Reproduce (a card in Found)"], ["b", "Build (a card ready to build)"], ["d", "Dismiss, with a reason"], ["m", "Open the merge pack"], ["x", "Select (Needs you: Worth fixing?)"], ["?", "This help"], ["Esc", "Close"]];

export function keyHelp() {
  document.querySelectorAll(".cv-help, .cv-helpscrim").forEach((e) => e.remove());
  const done = () => { box.remove(); scrim.remove(); };
  const scrim = h("div", { class: "cv-popscrim cv-helpscrim", onclick: done });
  const box = h("div", { class: "cv-help cv-pop", role: "dialog", "aria-label": "Keyboard shortcuts", tabindex: "-1" },
    h("h3", {}, "Keyboard shortcuts"),
    h("dl", {}, KEYS.map(([k, w]) => [h("dt", {}, h("kbd", {}, k)), h("dd", {}, w)])),
    h("p", { class: "cv-meta", style: { margin: "0 0 12px" } }, "Workbench's own keys still work: / searches, . opens Needs you, g s goes home."),
    h("button", { class: "cv-btn", type: "button", onclick: done }, "Close"));
  document.body.append(scrim, box);
  box.focus();
}

/** The inbox's keys; returns whether it used the key. */
export function inboxKey(C, e, host) {
  if (e.metaKey || e.ctrlKey || e.altKey) return false;
  const t = e.target;
  if (t && t.closest && t.closest("input, textarea, select, [contenteditable]")) return false;
  if (document.querySelector(".cv-sheet, .cv-menu, .cv-pop:not(.cv-help)")) return false;
  if (e.key === "?") { e.preventDefault(); keyHelp(); return true; }
  const rows = [...host.querySelectorAll(".cv-ib, .cv-ip")];
  if (!rows.length) return false;
  const S = C.S;
  let i = rows.findIndex((r) => r.dataset.id === S.cur);
  const find = (id) => C.findings().find((f) => f.id === id);
  const mark = (j) => {
    rows.forEach((r) => r.classList.remove("cur"));
    const r = rows[Math.max(0, Math.min(rows.length - 1, j))];
    S.cur = r.dataset.id; r.classList.add("cur");
    r.querySelector(".ib-main, .ip-main")?.focus({ preventScroll: true });
    r.scrollIntoView({ block: "nearest" });
  };
  const k = e.key;
  if (k === "j" || k === "ArrowDown" && i >= 0) { e.preventDefault(); mark(i < 0 ? 0 : i + 1); return true; }
  if (k === "k" || k === "ArrowUp" && i >= 0) { e.preventDefault(); mark(i < 0 ? 0 : i - 1); return true; }
  const row = i >= 0 ? rows[i] : null;
  const f = row ? find(row.dataset.id) : null;
  const kind = row ? row.dataset.kind : null;
  if (k === "Enter" && f && !(t && t.closest && t.closest("button, a"))) { e.preventDefault(); C.open(f.id); return true; }
  if (k === "m") {
    const target = kind === "merge" ? f : find((rows.find((r) => r.dataset.kind === "merge") || {}).dataset?.id);
    if (!target) { toast("Nothing waits to be merged"); return true; }
    e.preventDefault(); S.cur = target.id; C.open(target.id); return true;
  }
  if (!f) { if ("rbdx".includes(k)) toast("Pick a card first: j or k"); return "rbdx".includes(k); }
  const next = () => { const r = rows[i + 1] || rows[i - 1]; S.cur = r ? r.dataset.id : null; };
  if (k === "r") { e.preventDefault(); if (kind === "fix") { next(); reproduce(C, [f.id]); } else toast("r reproduces a new problem (Worth fixing?)"); return true; }
  if (k === "b") { e.preventDefault(); if (kind === "build") build(C, [f]); else toast("b builds a planned card (Build these?)"); return true; }
  if (k === "d") { e.preventDefault(); if (kind === "fix" || kind === "build") askDismiss(C, f); else toast("d dismisses a new problem or a planned card"); return true; }
  if (k === "x" && kind === "fix") { e.preventDefault(); if (S.sel.has(f.id)) S.sel.delete(f.id); else S.sel.set(f.id, true); C.keep(); C.draw(); return true; }
  return false;
}
