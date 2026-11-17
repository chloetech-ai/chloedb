// Home: the pipeline. The project, "Needs you", "Run personas" and "⋯" on
// top; then the five steps with their counts and what is moving in each (a
// list of rows on a phone, five columns on a desktop); merged as a link.
import { h, toast, enc, usd, ago, toMs, statusLine, flagLine, thinBar, blockedOf, queuedOf, agentOn, happening } from "/goals/converge/util.js";

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
function stopButton(C) {
  const st = C.settings();
  const stopped = !!st.stopped;
  const busy = (C.S.d.findings || []).some((f) => agentOn(f) || f.busy) || (C.S.d.runs || []).some((r) => r.state === "running");
  if (!stopped && !busy && !st.enabled && !st.follow_picks) return null;
  const b = h("button", { class: `cv-btn ${stopped ? "pri" : "crit"}`, type: "button", onclick: async () => {
    b.disabled = true;
    try {
      const r = await C.api("/api/converge/stop", { method: "POST", body: { repo: C.repo(), resume: stopped } });
      toast(stopped ? `Resumed: the autopilot is ${r.enabled ? "on" : "off"}${r.follow_picks ? ", your picks move by themselves" : ""}` : `Stopped ${r.agents} agent${r.agents === 1 ? "" : "s"}${r.runs ? ` and ${r.runs} run${r.runs === 1 ? "" : "s"}` : ""}. Nothing moves until you resume.`);
    } catch (err) { toast(err.message || String(err)); }
    C.S.sig = ""; await C.load();
  } }, stopped ? "Resume" : "Stop everything");
  return b;
}

export function topBar(C) {
  return h("div", { class: "cv-top" },
    projectTitle(C),
    h("div", { style: { display: "flex", gap: "8px", flexWrap: "wrap", justifyContent: "flex-end" } },
      stopButton(C),
      h("a", { class: "cv-btn pri", href: C.href("personas") }, "Run personas")));
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

/** What is moving in a step, in one line. */
function movingLine(C, s) {
  if (!s.count) return { 0: "Nothing waits for you here", 1: "Nothing being reproduced", 2: "Nothing planned yet", 3: "Nothing being built", 4: "No pull request waits" }[["problems", "reproducing", "ready", "building", "merge"].indexOf(s.step)];
  const live = s.items.filter((f) => agentOn(f) || f.busy);
  const bits = [];
  if (s.step === "problems") {
    const fresh = s.items.filter((f) => (toMs(f.created) || 0) > (C.S.seenAt || 0)).length;
    if (fresh && C.S.seenAt) bits.push(`${fresh} new`);
    const blockers = s.items.filter((f) => f.severity >= 4).length;
    if (blockers) bits.push(`${blockers} blocker${blockers > 1 ? "s" : ""}`);
    if (!bits.length) bits.push("waiting for your pick");
  } else if (s.step === "ready") {
    const top = [...s.items].filter((f) => f.stage === "prioritize").sort((a, b) => (b.score || 0) - (a.score || 0))[0];
    const plan = s.items.filter((f) => f.stage === "scope").length;
    if (plan) bits.push(`${plan} being planned`);
    if (top) bits.push(`top: ${top.title}`);
  } else if (s.step === "merge") {
    const ready = s.items.filter((f) => f.ci === "pass" && !f.conflict).length;
    const clash = s.items.filter((f) => f.conflict).length;
    bits.push(ready ? `${ready} ready for your approval` : clash ? "" : "CI running");
    if (clash) bits.push(`${clash} conflict${clash > 1 ? "s" : ""} with the base`);
    if (!bits[0]) bits.shift();
  } else {
    if (live.length) bits.push(`${live.length} moving`);
    const f = live[0] || s.items[0];
    if (f) bits.push(happening(f, C.settings()));
  }
  if (s.queued) bits.push(`${s.queued} queued`);
  return bits.join(" · ");
}

function stepRow(C, s, i) {
  const b = s.items.filter((f) => blockedOf(f));
  const you = s.step === "problems" && s.count || s.step === "merge" && s.items.some((f) => (f.ci === "pass" && !f.conflict) || f.needs_now);
  return h("a", { class: `cv-step${s.count ? " has" : ""}${you ? " you" : ""}`, href: C.href(s.step) },
    h("span", { class: "s-no", "aria-hidden": "true" }, String(i + 1)),
    h("span", { class: "s-tx" }, h("span", { class: "s-nm" }, s.name), h("span", { class: "s-mv" }, s.moving ? h("i", { class: "cv-pulse", style: { color: "var(--accent)" }, "aria-hidden": "true" }) : null, movingLine(C, s))),
    h("span", { class: "s-ct", "aria-label": `${s.count} cards` }, String(s.count)),
    h("span", { class: "s-car", "aria-hidden": "true" }, "›"),
    b.length ? h("span", { class: "s-fl" }, b.slice(0, 2).map((f) => flagLine(f))) : null);
}

/** A compact card for a desktop column. */
export function miniCard(C, f) {
  return h("button", { class: `cv-card${blockedOf(f) ? " blocked" : ""}`, type: "button", "data-id": f.id, onclick: () => C.open(f.id) },
    h("span", { class: "ttl" }, f.title),
    statusLine(f, C.settings()),
    flagLine(f),
    thinBar(f, C.settings()));
}

function column(C, s) {
  const order = (a, b) => (blockedOf(b) ? 1 : 0) - (blockedOf(a) ? 1 : 0) || ((agentOn(b) || b.busy ? 1 : 0) - (agentOn(a) || a.busy ? 1 : 0)) || (b.pinned ? 1 : 0) - (a.pinned ? 1 : 0) || (b.score || 0) - (a.score || 0) || b.severity - a.severity;
  const items = [...s.items].sort(order);
  const N = 6;
  return h("section", { class: "cv-col", "aria-label": s.name },
    h("a", { class: "cv-col-h", href: C.href(s.step) }, h("span", { class: "t" }, h("span", {}, s.name), h("span", { class: "c" }, String(s.count))), h("span", { class: "g" }, movingLine(C, s))),
    h("div", { class: "cards" }, items.length ? items.slice(0, N).map((f) => miniCard(C, f)) : h("p", { class: "cv-empty", style: { padding: "4px" } }, s.hint),
      items.length > N ? h("a", { class: "more", href: C.href(s.step) }, `All ${items.length}`) : null));
}

export function homeScreen(C) {
  const d = C.S.d;
  if (!(d.findings || []).length && !(d.personas || []).length) {
    return h("div", {}, autoLine(C),
      h("div", { class: "cv-none" },
        h("p", {}, "Nothing here yet. Personas play the product and what they hit shows up as problems. Start by writing personas from the code, or load ChloeDB's six evaluation personas and what they reported."),
        h("a", { class: "cv-btn pri", href: C.href("personas") }, "Set up personas"),
        h("button", { class: "cv-btn", type: "button", onclick: () => C.act(() => C.api("/api/converge/seed", { method: "POST", body: { repo: C.repo() } }), "Loaded the seed personas and their problems") }, "Load the seed personas")));
  }
  const st = C.steps();
  const week = Date.now() - 7 * 86400000;
  const merged = Number.isFinite(Number(d.merged_week)) && d.merged_week != null ? Number(d.merged_week)
    : (d.findings || []).filter((f) => f.stage === "merged" && (toMs(f.stage_since) || 0) >= week).length;
  return h("div", {},
    autoLine(C),
    h("nav", { class: "cv-pipe5", "aria-label": "The steps" }, st.map((s, i) => stepRow(C, s, i))),
    h("div", { class: "cv-cols" }, st.map((s) => column(C, s))),
    h("a", { class: "cv-merged", href: C.href("merge", { show: "merged" }) }, h("span", {}, merged ? `${merged} merged this week` : "Nothing merged this week yet"), h("span", { "aria-hidden": "true" }, "›")));
}
