// The step screens after Problems: Reproducing, Ready to build, Building
// and Ready to merge. Each is a list of rows with the step's own line; the
// one pick that is yours (Build) sits in a sticky bar.
import { h, toast, SEV, STEPS, statusLine, flagLine, thinBar, blockedOf, queuedOf, agentOn, doingWords, fmtScore, toMs, ago } from "/goals/converge/util.js";
import { listRow, stepHead, none, bulkBar } from "/goals/converge/rows.js";

const order = (a, b) => (blockedOf(b) ? 1 : 0) - (blockedOf(a) ? 1 : 0) || (b.pinned ? 1 : 0) - (a.pinned ? 1 : 0) || ((agentOn(b) || b.busy ? 1 : 0) - (agentOn(a) || a.busy ? 1 : 0)) || (b.score || 0) - (a.score || 0) || b.severity - a.severity;

/** The step's own line for a row. */
function detail(C, f) {
  const st = C.settings();
  const rounds = Math.max(1, st.max_rounds || 3);
  switch (f.stage) {
    case "reproduce": {
      if (f.needs_you) return h("span", { class: "cv-blkl" }, f.needs_you);
      if (agentOn(f)) return null;
      return f.repro ? h("span", { class: "r-meta" }, f.repro_hash ? "repro locked" : `repro written · ${f.repro_attempts ? `attempt ${f.repro_attempts + 1}` : "runs next"}`) : null;
    }
    case "scope": case "prioritize": {
      const sc = f.scope && typeof f.scope === "object" ? f.scope : null;
      if (!sc) return null;
      return h("span", { class: "r-meta" }, sc.area ? h("span", {}, sc.area) : null, sc.size ? h("span", {}, `size ${sc.size}`) : null);
    }
    case "develop": case "verify": {
      const lc = f.last_check && typeof f.last_check === "object" ? f.last_check : null;
      const p = lc && Array.isArray(lc.problems) && lc.problems[0];
      return p ? h("span", { class: "cv-blkl", style: { color: "var(--warn-ink)" } }, `Round ${lc.round || f.rounds || 1} of ${rounds}: ${p}`) : null;
    }
    case "review": {
      return h("span", { class: "r-meta" }, f.conflict ? h("span", { style: { color: "var(--crit-ink)" } }, "Conflicts with the base") : h("span", { style: { color: f.ci === "pass" ? "var(--good-ink)" : f.ci === "fail" ? "var(--crit-ink)" : null } }, f.ci === "pass" ? "CI green" : f.ci === "fail" ? "CI failed" : "CI running"),
        f.ease_before != null && f.ease_after != null ? h("span", {}, `ease ${f.ease_before} → ${f.ease_after}`) : null, f.pr_url ? h("span", {}, (f.pr_url.match(/\/pull\/(\d+)/) || [])[1] ? `PR #${f.pr_url.match(/\/pull\/(\d+)/)[1]}` : "PR open") : null);
    }
    default: return null;
  }
}

function row(C, f, o = {}) {
  return listRow({
    id: f.id, title: f.title, S: C.S, selectable: !!o.selectable, selected: C.S.sel.has(f.id), cls: blockedOf(f) ? "blocked" : null,
    onSelect: (on) => { if (on) C.S.sel.set(f.id, true); else C.S.sel.delete(f.id); C.keep(); C.draw(); },
    onOpen: () => C.open(f.id),
    lines: [
      h("span", { class: "r-t1" }, h("span", { class: "cv-sev", "data-sev": String(f.severity), title: SEV[f.severity] }), h("span", { class: "r-ttl" }, f.title)),
      statusLine(f, C.settings()),
      detail(C, f),
      flagLine(f),
      thinBar(f, C.settings())],
    side: o.side || null,
  });
}

export function stepScreen(C, step) {
  const s = STEPS.find((x) => x.step === step) || STEPS[1];
  const all = C.findings().filter((f) => f.state === "active" && s.stages.includes(f.stage)).sort(order);
  if (step === "ready") return ready(C, s, all);
  if (step === "merge") return merge(C, s, all);
  const head = stepHead(C, s.name, all.length, s.hint);
  if (!all.length) {
    return h("div", {}, head, step === "reproducing"
      ? none("Nothing is being reproduced. Pick problems to reproduce and an agent writes a script that shows each one.", h("a", { class: "cv-btn pri", href: C.href("problems") }, "Go to Problems"))
      : none("Nothing is being built. Pick a planned problem to build and a fix agent takes it on its own branch.", h("a", { class: "cv-btn pri", href: C.href("ready") }, "Go to Ready to build")));
  }
  if (step === "building") {
    const wip = Number(C.settings().wip) || 0;
    const dev = all.filter((f) => f.stage === "develop"), ver = all.filter((f) => f.stage === "verify");
    return h("div", {}, head, wip ? slots(all.length, wip) : null,
      dev.length ? [h("div", { class: "cv-sec" }, h("span", { class: "cv-label" }, "Fixing"), h("span", { class: "c" }, String(dev.length))), h("ul", { class: "cv-list" }, dev.map((f) => row(C, f)))] : null,
      ver.length ? [h("div", { class: "cv-sec" }, h("span", { class: "cv-label" }, "Verifying"), h("span", { class: "c" }, String(ver.length))), h("ul", { class: "cv-list" }, ver.map((f) => row(C, f)))] : null);
  }
  return h("div", {}, head, h("ul", { class: "cv-list" }, all.map((f) => row(C, f))));
}

function slots(used, wip) {
  return h("div", { class: "cv-slots" }, h("span", { class: "pips", "aria-hidden": "true" }, Array.from({ length: Math.min(wip, 12) }, (_, i) => h("i", { class: i < used ? "on" : null }))), `${Math.min(used, wip)} of ${wip} building slots in use`);
}

function ready(C, s, all) {
  const S = C.S;
  const planning = all.filter((f) => f.stage === "scope");
  const planned = all.filter((f) => f.stage === "prioritize").sort((a, b) => (blockedOf(b) ? 1 : 0) - (blockedOf(a) ? 1 : 0) || (b.pinned ? 1 : 0) - (a.pinned ? 1 : 0) || (b.score || 0) - (a.score || 0));
  for (const id of [...S.sel.keys()]) if (!planned.some((f) => f.id === id)) S.sel.delete(id);
  const wip = Number(C.settings().wip) || 0;
  const building = C.findings().filter((f) => f.state === "active" && (f.stage === "develop" || f.stage === "verify")).length;
  const head = stepHead(C, s.name, all.length, "Sorted by score. Pick what to build; the rest waits here.");
  if (!all.length) return h("div", {}, head, none("Nothing is planned yet. Once a problem reproduces, an agent plans its fix and it waits here for you.", h("a", { class: "cv-btn pri", href: C.href("reproducing") }, "See Reproducing")));
  const ids = [...S.sel.keys()];
  const build = async (list) => {
    const doors = planned.filter((f) => list.includes(f.id) && f.door);
    if (doors.length && !confirm(`${doors.length === 1 ? `“${doors[0].title}” touches` : `${doors.length} of these touch`} a one-way door. Build anyway?`)) return;
    const r = await C.act(() => C.bulk(list, "start"), null);
    if (r) { list.forEach((id) => S.sel.delete(id)); toast(r.failed && r.failed.length ? `${r.done.length} started; ${r.failed.length} did not: ${r.failed[0].error}` : `${r.done.length > 1 ? `${r.done.length} fix agents` : "A fix agent"} on it`); C.draw(); }
  };
  return h("div", {}, head,
    wip ? slots(building, wip) : null,
    planned.length ? [h("div", { class: "cv-sec" }, h("span", { class: "cv-label" }, "Planned"), h("span", { class: "c" }, String(planned.length))),
      h("ul", { class: "cv-list" }, planned.map((f) => row(C, f, { selectable: true, side: [h("span", { class: "r-score" }, fmtScore(f.score)), h("span", {}, "score")] })))] : null,
    planning.length ? [h("div", { class: "cv-sec" }, h("span", { class: "cv-label" }, "Being planned"), h("span", { class: "c" }, String(planning.length))), h("ul", { class: "cv-list" }, planning.map((f) => row(C, f)))] : null,
    ids.length ? bulkBar(ids.length, h("button", { class: "cv-btn", type: "button", onclick: () => { S.sel.clear(); C.draw(); } }, "Clear"), h("button", { class: "cv-btn pri", type: "button", onclick: () => build(ids) }, ids.length > 1 ? `Build ${ids.length}` : "Build"))
      : planned.length ? h("div", { class: "cv-bulk" }, h("span", { class: "n" }, `Top: ${planned[0].title}`), h("button", { class: "cv-btn pri", type: "button", onclick: () => build([planned[0].id]) }, "Build the top one")) : null);
}

function merge(C, s, all) {
  const head = stepHead(C, s.name, all.length, "Each has passed its checks and its persona's replay. Merging stays yours.");
  const showMerged = C.query().get("show") === "merged";
  const week = Date.now() - 7 * 86400000;
  const merged = C.findings().filter((f) => f.stage === "merged" && (toMs(f.stage_since) || 0) >= week).sort((a, b) => (toMs(b.stage_since) || 0) - (toMs(a.stage_since) || 0));
  const mergedList = merged.length ? [h("div", { class: "cv-sec", id: "cv-merged" }, h("span", { class: "cv-label" }, "Merged this week"), h("span", { class: "c" }, String(merged.length))),
    h("ul", { class: "cv-list" }, merged.map((f) => row(C, f, { side: h("span", {}, ago(f.stage_since)) })))] : null;
  if (showMerged) setTimeout(() => document.getElementById("cv-merged")?.scrollIntoView({ block: "start" }), 0);
  if (!all.length) return h("div", {}, head, none("No pull request waits for you. Fixes arrive here once their checks and their persona pass.", h("a", { class: "cv-btn", href: C.href("building") }, "See Building")), mergedList);
  return h("div", {}, head, h("ul", { class: "cv-list" }, all.map((f) => row(C, f))), mergedList);
}

export { doingWords, queuedOf };
