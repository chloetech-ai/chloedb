// A card's story: every event on it, in order, as one vertical timeline —
// the persona that hit it (its words and ease), the reports folded in, your
// picks, the repro agent (its steps folded, the runs on the base as dots),
// the plan, the fix agent (its steps, its checks), the persona's replay, the
// pull request and CI, the merge and the guard. Built from the card's events
// (GET /api/converge/findings/:id), its runs and its agents' events.
import { h, put, toMs, ago, enc, firstName, firstSentences, doingWords, secsWords, agentOn, STAGE_NAME } from "/goals/converge/util.js";
import { useStyle } from "/levels/common.js";

const CSS = `
.st-list { display: flex; flex-direction: column; }
.st-ev { display: flex; gap: 12px; min-width: 0; }
.st-rail { width: 26px; display: flex; flex-direction: column; align-items: center; flex: none; }
.st-node { width: 24px; height: 24px; border-radius: 50%; display: grid; place-items: center; font-size: 12px; font-weight: var(--w-title); color: #fff; flex: none; background: var(--muted); }
.st-node.p { background: var(--lane-4); } .st-node.y { background: var(--warn); color: var(--on-warn, #fff); } .st-node.a { background: var(--accent); color: var(--accent-ink); }
.st-node.g { background: var(--good); } .st-node.x { background: var(--crit); } .st-node.c { background: var(--muted); }
.st-node.live { animation: cv-pulse 1.4s ease-in-out infinite; }
.st-line { flex: 1; width: 2px; background: var(--line); min-height: 12px; }
.st-ev:last-child .st-line { background: transparent; }
.st-body { flex: 1; min-width: 0; padding-bottom: 14px; display: flex; flex-direction: column; gap: 4px; }
.st-h { display: flex; justify-content: space-between; gap: 10px; align-items: baseline; min-width: 0; }
.st-h .w { min-width: 0; overflow-wrap: anywhere; font-size: var(--fs-sm); line-height: 1.45; }
.st-h .tm { font-family: var(--mono); font-size: var(--fs-xs); color: var(--muted); white-space: nowrap; flex: none; }
.st-who { font-size: 11px; padding: 2px 6px; border-radius: 4px; font-weight: var(--w-title); margin-right: 4px; white-space: nowrap; background: var(--surface-3); color: var(--text-2); }
.st-who.p { background: color-mix(in srgb, var(--lane-4) 14%, var(--surface)); color: var(--lane-4); }
.st-who.y { background: var(--warn-soft); color: var(--warn-ink); } .st-who.a { background: var(--accent-soft); color: var(--accent-text); }
.st-who.g { background: var(--good-soft); color: var(--good-ink); } .st-who.x { background: var(--crit-soft); color: var(--crit-ink); }
.st-q { font-size: var(--fs-sm); color: var(--text-2); font-style: italic; overflow-wrap: anywhere; }
.st-sub { font-size: var(--fs-xs); color: var(--text-2); overflow-wrap: anywhere; }
.st-sub .good { color: var(--good-ink); } .st-sub .bad { color: var(--crit-ink); }
.st-fold { all: unset; cursor: pointer; color: var(--accent-text); font-size: var(--fs-xs); min-height: 24px; display: inline-flex; align-items: center; align-self: flex-start; }
.st-fold:focus-visible { outline: 2px solid var(--accent); border-radius: 4px; }
.st-steps { border-left: 2px solid var(--line); padding-left: 12px; margin-top: 4px; display: flex; flex-direction: column; gap: 3px; font-size: var(--fs-xs); color: var(--text-2); }
.st-steps .st-step { font-weight: var(--w-mid); color: var(--text); }
.st-steps code { font-family: var(--mono); }
.st-dots { display: inline-flex; gap: 4px; vertical-align: -1px; margin: 0 4px; }
.st-dots i { width: 10px; height: 10px; border-radius: 50%; display: inline-block; background: var(--surface-3); }
.st-dots i.f { background: var(--crit); } .st-dots i.p { background: var(--good); }
.st-mono { font-family: var(--mono); font-size: .95em; overflow-wrap: anywhere; }
@media (max-width: 640px) { .st-fold { min-height: 44px; } .st-h .w { font-size: var(--fs-sm); } }
`;

const clock = (t) => { const ms = toMs(t); return ms ? new Date(ms).toLocaleTimeString([], { hour: "2-digit", minute: "2-digit", hour12: false }) : ""; };
const prNo = (u) => ((/\/pull\/(\d+)/.exec(u || "") || [])[1] || null);
const exits = (runs) => (Array.isArray(runs) ? runs.map((r) => Number(r && typeof r === "object" ? r.exit : r)).filter(Number.isFinite) : []);
const dots = (xs) => h("span", { class: "st-dots", "aria-hidden": "true" }, xs.map((x) => h("i", { class: x === 0 ? "p" : "f" })));

/** An agent's steps, from its events: what it did, in a few words each. */
function agentSteps(C, id, live, repaint) {
  const M = (C.S.agSteps = C.S.agSteps || new Map());
  const c = M.get(id);
  if (!c || (!c.busy && live && Date.now() - c.at > 5000)) {
    M.set(id, { ...(c || {}), busy: true, at: Date.now() });
    C.api(`/api/agents/${enc(id)}/events?limit=1000`, { quiet: true }).then((x) => {
      const ev = x && Array.isArray(x.events) ? x.events : [];
      const lines = [];
      let commits = 0;
      for (const e of ev) {
        if (e.kind !== "PreToolUse") continue;
        if (/git (add[^&]*&&\s*)?git commit|^Bash: git commit|git commit/.test(e.summary || "")) commits++;
        const w = doingWords(e.summary);
        if (w && lines.at(-1) !== w) lines.push(w);
      }
      // Its working time: from its start to its last action (not the idle wait after).
      const ts = ev.filter((e) => e.kind !== "Notification" && e.kind !== "SessionEnd" && e.kind !== "exited").map((e) => toMs(e.ts)).filter(Boolean);
      M.set(id, { lines, commits, first: ts[0], last: ts.at(-1), at: Date.now() });
      repaint();
    }, () => { M.set(id, { lines: [], at: Date.now(), err: true }); });
  }
  return c && c.lines ? c : null;
}

/** The story's events, merged into the moments a person reads. */
export function storyItems(C, x) {
  const f = x.finding || {};
  const evs = Array.isArray(x.events) ? x.events : [];
  const ps = C.S.d.personas || [];
  const pn = (id) => { const p = ps.find((y) => y.id === id || y.name === id) || (x.persona && x.persona.id === id ? x.persona : null); return p ? firstName(p.name) : id && !/^cp[0-9a-f]{6,}$/.test(id) ? firstName(id) : "A persona"; };
  const out = [];
  const add = (o) => { out.push(o); return o; };
  let repro = null, fix = null, gh = null;
  for (const e of evs) {
    const ev = e.evidence && typeof e.evidence === "object" ? e.evidence : {};
    const ts = e.ts_ms || toMs(e.ts);
    switch (e.gate) {
      case "reported": {
        const persona = /^cp/.test(e.by || "");
        const run = x.found_run && x.found_run.id === ev.run ? x.found_run : null;
        add(persona
          ? { kind: "p", who: "Persona", ts, title: [h("b", {}, pn(e.by)), ` hit it${run && x.persona && x.persona.journey ? "" : ""}${ev.step ? ` at “${String(ev.step).replace(/^Step \d+( of \d+)?:\s*/i, "")}”` : ""}`], quote: firstSentences(f.detail, 2), sub: [ev.step && /^Step \d+ of \d+/.test(ev.step) ? ev.step.match(/^Step \d+ of \d+/)[0].toLowerCase() : null, run && run.ease != null ? `ease ${run.ease}/5` : f.ease_before != null ? `ease ${f.ease_before}/5` : null].filter(Boolean).join(" · "), run: ev.run, runSteps: run && Array.isArray(run.steps) ? run.steps : null }
          : { kind: e.by === "you" ? "y" : "c", who: e.by === "you" ? "You" : "Converge", ts, title: e.by === "you" ? "wrote the card" : e.summary || "Reported" });
        break;
      }
      case "dedup":
        if (e.passed && ev.merged) add({ kind: "c", node: "=", who: "Converge", ts, title: `${ev.report && ev.report.by ? pn(ev.report.by) : "Another report"} hit the same thing · folded in${ev.similarity != null ? ` (likeness ${Number(ev.similarity).toFixed(2)}${ev.by ? `, by ${ev.by === "model" ? "the model" : ev.by}` : ""})` : ev.by === "model" ? " (the model says it is the same)" : ""}`, quote: ev.report && ev.report.title ? `“${ev.report.title}”` : null });
        break;
      case "triage": case "asked":
        add({ kind: "y", node: "✋", who: "You", ts, title: [`chose `, h("b", {}, "Reproduce")] });
        break;
      case "repro_agent":
        repro = add({ kind: "a", node: "A", who: "Repro agent", ts, agent: ev.agent || null, title: "is writing the repro", end: null });
        break;
      case "repro_fails": case "repro_broken": case "cant_reproduce": {
        const xs = exits(ev.runs);
        const fails = xs.filter((v) => v !== 0).length;
        const line = [`On ${ev.base || C.settings().base || "the base"}${ev.commit ? ` (${String(ev.commit).slice(0, 8)})` : ""}:`, dots(xs), e.passed ? `fails ${fails} of ${xs.length}: locked` : e.summary || "did not reproduce"];
        if (repro && !repro.done) { repro.done = true; repro.end = ts; repro.title = e.passed ? ["wrote ", h("span", { class: "st-mono" }, "repro.sh")] : "could not show it"; repro.sub = line; }
        else add({ kind: e.passed ? "c" : "x", node: "R", who: "Converge", ts, title: e.passed ? "ran the repro on the base" : "could not reproduce it", sub: line });
        break;
      }
      case "scope": case "scope_talk": {
        const sc = ev.scope && typeof ev.scope === "object" ? ev.scope : f.scope || {};
        add({ kind: "c", node: "S", who: "Converge", ts, title: ["scoped: ", sc.area ? h("span", { class: "st-mono" }, sc.area) : "the plan", sc.size ? ` · size ${sc.size}` : ""], sub: [sc.root_cause ? `Root cause: ${firstSentences(sc.root_cause, 1)}` : null, sc.acceptance ? `Fixed when: ${firstSentences(sc.acceptance, 1)}` : null].filter(Boolean).join(" · "), more: Array.isArray(sc.files) && sc.files.length ? sc.files : null, door: ev.door || null });
        break;
      }
      case "queued":
        add({ kind: "c", node: "⏸", who: "Converge", ts, title: e.summary || "Queued" });
        break;
      case "slot": case "start": {
        if (e.by === "you") add({ kind: "y", node: "✋", who: "You", ts, title: ["chose ", h("b", {}, "Build")] });
        fix = add({ kind: "a", node: "A", who: "Fix agent", ts, agent: ev.agent || null, branch: ev.branch || f.branch, title: ["is working on ", h("span", { class: "st-mono" }, ev.branch || f.branch || "its branch")], rounds: [] });
        break;
      }
      case "develop": {
        const xs = exits(ev.repro);
        const t = ev.tests && typeof ev.tests === "object" ? ev.tests : null;
        const bits = [xs.length ? h("span", { class: xs.every((v) => v === 0) ? "good" : "bad" }, `repro passes ${xs.filter((v) => v === 0).length} of ${xs.length}`) : null,
          t ? h("span", { class: t.exit === 0 ? "good" : t.exit != null ? "bad" : "" }, t.exit === 0 ? "tests pass" : t.exit != null ? "tests fail" : "tests not run") : null,
          ev.diff && ev.diff.files != null ? `${ev.diff.files} file${ev.diff.files === 1 ? "" : "s"} +${ev.diff.add ?? 0} −${ev.diff.del ?? 0}` : Array.isArray(ev.changed) ? `${ev.changed.length} files` : null].filter(Boolean);
        const sub = e.passed ? ["Checks: ", bits.flatMap((b, i) => (i ? [" · ", b] : [b]))] : [h("span", { class: "bad" }, `Round ${ev.round || (fix ? fix.rounds.length + 1 : 1)} did not pass: `), firstSentences(e.summary, 1)];
        if (fix) {
          // The same pass again (a base merge, a recheck) counts, it does not repeat.
          const prev = fix.rounds.at(-1);
          if (prev && prev.passed && e.passed) { prev.n = (prev.n || 1) + 1; prev.sub = [...sub, ` · checked ${prev.n} times`]; } else fix.rounds.push({ ts, passed: e.passed, sub });
          fix.end = ts; if (e.passed) fix.title = ["fixed it on ", h("span", { class: "st-mono" }, fix.branch || "its branch")]; }
        else add({ kind: e.passed ? "a" : "x", node: "✓", who: "Checks", ts, title: e.passed ? "passed" : "did not pass", sub });
        break;
      }
      case "verify": {
        const by = /^cp/.test(e.by || "") ? e.by : f.persona;
        const eb = ev.ease_before ?? f.ease_before, ea = ev.ease_after ?? f.ease_after;
        add({ kind: e.passed ? "p" : "x", node: "P", who: "Persona", ts, title: [h("b", {}, pn(by)), ` replayed it on the branch${eb != null && ea != null ? ` · ease ${eb} → ${ea}` : ""}`], quote: ev.answer ? `“${firstSentences(ev.answer, 2)}”` : null, sub: e.passed ? null : h("span", { class: "bad" }, firstSentences(e.summary, 1)) });
        break;
      }
      case "pr": case "ci": case "conflict": {
        if (e.gate === "conflict") { add({ kind: "x", node: "!", who: "GitHub", ts, title: firstSentences(e.summary, 1) }); break; }
        const no = prNo(ev.url || f.pr_url);
        if (!gh || e.gate === "pr") gh = add({ kind: "g", node: "✓", who: "GitHub", ts, url: ev.url || f.pr_url, title: no ? `PR #${no} opened` : e.summary || "The pull request" });
        if (e.gate === "ci") { gh.ci = ev.ci; gh.end = ts; gh.title = `${no ? `PR #${no}` : "The pull request"} · CI ${ev.ci === "pass" ? "green" : ev.ci === "fail" ? "failed" : ev.ci || "running"}${ev.ci === "pass" && !f.conflict ? " · mergeable" : ""}`; if (ev.ci === "fail") gh.kind = "x"; }
        break;
      }
      case "review":
        add({ kind: "y", node: "✋", who: "You", ts, title: e.by === "you" ? "sent it back" : e.summary || "Review", quote: e.by === "you" && e.summary ? `“${e.summary}”` : null });
        break;
      case "approved": case "merge_local":
        add({ kind: "g", node: "✓", who: "You", ts, title: [h("b", {}, "approved and merged"), prNo(ev.url) ? ` PR #${prNo(ev.url)}` : ""] });
        break;
      case "guard":
        add({ kind: e.passed ? "g" : "x", node: e.passed ? "✓" : "!", who: "Guard", ts, title: e.passed ? `the repro passes on ${C.settings().base || "the base"}` : "the problem is back on the base", sub: e.passed ? null : e.summary });
        break;
      case "blocked":
        add({ kind: "x", node: "!", who: "Blocked", ts, title: firstSentences(e.summary, 2) });
        break;
      case "unblocked": case "parked": case "unparked": case "dismissed": case "fold": case "pin": case "unpin":
        add({ kind: e.by === "you" ? "y" : "c", node: e.by === "you" ? "✋" : "·", who: e.by === "you" ? "You" : "Converge", ts, title: e.summary || e.gate });
        break;
      default:
        if (e.summary) add({ kind: e.passed === false ? "x" : e.by === "you" ? "y" : "c", node: e.by === "you" ? "✋" : "·", who: e.by === "you" ? "You" : /^cp/.test(e.by || "") ? pn(e.by) : "Converge", ts, title: firstSentences(e.summary, 2) });
    }
  }
  // What is happening right now, at the end.
  if (f.state === "active" && (agentOn(f) || f.busy)) {
    const who = f.stage === "reproduce" ? "Repro agent" : f.stage === "scope" || f.stage === "prioritize" ? "Planning agent" : "Fix agent";
    add({ kind: "a", node: "●", live: true, who: agentOn(f) ? who : "Converge", ts: Date.now(), title: agentOn(f) ? (f.agent_state === "waiting" ? "waits for an answer" : doingWords(f.agent_doing) || "working") : String(f.busy), now: true });
  } else if (f.state === "active" && f.stage !== "merged" && (f.needs_now || f.needs_you)) {
    add({ kind: "y", node: "✋", who: "You", ts: null, title: firstSentences(f.needs_now || f.needs_you, 1), now: true, next: true });
  }
  return out;
}

/** The timeline. `ui.open` keeps what is unfolded; `repaint` redraws the sheet. */
export function storyTimeline(C, x, ui, repaint) {
  useStyle("converge-story", CSS);
  const f = x.finding || {};
  const items = storyItems(C, x);
  const fold = (key, label, body) => {
    const open = ui.open.has(key);
    return [h("button", { class: "st-fold", type: "button", "aria-expanded": String(open), onclick: () => { if (open) ui.open.delete(key); else ui.open.add(key); repaint(); } }, `${open ? "▾" : "▸"} ${label}`), open ? body() : null];
  };
  const rows = items.map((it, i) => {
    const time = it.now && !it.next ? "now" : it.end && clock(it.end) !== clock(it.ts) ? `${clock(it.ts)}–${clock(it.end)}` : clock(it.ts);
    const extra = [];
    if (it.agent) {
      const live = agentOn(f) && f.agent === it.agent;
      const s = agentSteps(C, it.agent, live, repaint);
      const meta = s ? [s.lines.length ? `${s.lines.length} step${s.lines.length > 1 ? "s" : ""}` : null, s.commits ? `${s.commits} commit${s.commits > 1 ? "s" : ""}` : null, s.first && s.last ? secsWords(s.last - s.first) : null].filter(Boolean).join(" · ") : "";
      extra.push(fold(`ag:${it.agent}`, meta ? `Its steps · ${meta}` : "Its steps", () => h("div", { class: "st-steps" }, s ? (s.lines.length ? s.lines.slice(-40).map((l) => h("span", {}, `✓ ${l}`)) : h("span", {}, s.err ? "Its steps did not load." : "No steps kept.")) : h("span", {}, "Reading its steps…"),
        h("span", {}, h("a", { href: `#/a/${enc(it.agent)}/terminal` }, "Open its terminal")))));
    }
    if (it.rounds && it.rounds.length) for (const r of it.rounds) extra.push(h("span", { class: "st-sub" }, r.sub));
    if (it.runSteps && it.runSteps.length) {
      const steps = it.runSteps.filter((s) => s.kind === "step");
      extra.push(fold(`run:${it.run}`, `Their run · ${steps.length} step${steps.length === 1 ? "" : "s"}`, () => h("div", { class: "st-steps" },
        it.runSteps.filter((s) => s.kind === "step" || s.kind === "say").slice(0, 40).map((s) => h("span", { class: s.kind === "step" ? "st-step" : null }, s.kind === "step" ? s.text : firstSentences(s.text, 1))),
        h("span", {}, h("a", { href: C.here({ run: it.run }) }, "Open the run")))));
    }
    if (it.more) extra.push(fold(`files:${i}`, `${it.more.length} file${it.more.length > 1 ? "s" : ""} in the plan`, () => h("div", { class: "st-steps" }, it.more.map((p) => h("code", {}, p)))));
    if (it.url) extra.push(h("span", { class: "st-sub" }, h("a", { href: it.url, target: "_blank", rel: "noopener" }, "Open on GitHub ↗")));
    return h("div", { class: "st-ev" },
      h("div", { class: "st-rail" }, h("span", { class: `st-node ${it.kind}${it.live ? " live" : ""}`, "aria-hidden": "true" }, it.node || { p: "P", y: "✋", a: "A", g: "✓", x: "!", c: "·" }[it.kind]), h("span", { class: "st-line" })),
      h("div", { class: "st-body" },
        h("div", { class: "st-h" }, h("span", { class: "w" }, h("span", { class: `st-who ${it.kind}` }, it.who), " ", it.title), time ? h("span", { class: "tm" }, time) : null),
        it.quote ? h("span", { class: "st-q" }, it.kind === "p" && !String(it.quote).startsWith("“") ? `“${it.quote}”` : it.quote) : null,
        it.sub ? h("span", { class: "st-sub" }, it.sub) : null,
        extra));
  });
  return { el: h("div", { class: "st-list" }, rows.length ? rows : h("p", { class: "cv-empty" }, "No events yet.")), count: items.filter((x) => !x.next).length, items };
}

/** The stage, its number of eight, and the bar, for the card's head. */
export const STAGE_ORDER = ["found", "reproduce", "scope", "prioritize", "develop", "verify", "review", "merged"];
export function stageWords(f) {
  const i = Math.max(0, STAGE_ORDER.indexOf(f.stage));
  return { n: i + 1, name: STAGE_NAME[f.stage] || f.stage };
}
export { ago };
