// A run of replays (HARNESS.md, "Reproduce and replay" and "The
// scorecard"), at #/harness/runs/<id>: A against B on cases, or Optimize
// this on one stretch. What is playing now, the decision and why, each
// metric against the noise of A's own runs, the targets you ticked, each
// change Optimize this tried and what came of it, and every replay with
// what it met that the log did not have. Enough stops it (Optimize this
// keeps its best so far); a change that widens what agents may do waits
// here for you to name its patterns, or to skip it.
import { h, useStyle, page, label, btn, icon } from "/levels/common.js";
import { tabs, fmtMs, fmtWhen, rich, toast, failed, loading, repoName } from "./parts.js";
import { TARGETS } from "./timeline.js";

const enc = encodeURIComponent;
const STATE_WORDS = { queued: "queued", running: "playing", needs_you: "needs you", done: "done", stopped: "stopped", failed: "failed" };
const RESULT_WORDS = { keep: "keep", reject: "reject", no_detectable_difference: "no detectable difference", too_few: "too few to tell", running: "still playing" };
const VERDICT_WORDS = { better: "better", worse: "worse", noise: "inside the noise", too_few: "too few" };
const ROUND_WORDS = { kept: "kept", not_kept: "not kept", stopped: "stopped", skipped: "skipped", running: "playing" };

export function render(host, ctx, path) {
  const id = decodeURIComponent((path.match(/^#\/harness\/runs\/([^/?]+)/) || [])[1] || "");
  const S = { alive: true, id, d: null, timer: null, busy: false, named: new Set() };
  host.replaceChildren(loading("Reading the run…"));
  const load = async () => {
    try {
      S.d = await ctx.api(`/api/harness/runs/${enc(id)}`);
      if (!S.alive) return;
      draw(host, ctx, S, load);
      const live = ["queued", "running"].includes(S.d.run.state);
      clearTimeout(S.timer);
      if (live) S.timer = setTimeout(load, 2500);
    } catch (err) { if (S.alive) host.replaceChildren(failed(err)); }
  };
  load();
  return { unmount() { S.alive = false; clearTimeout(S.timer); } };
}

const money = (x) => `$${(Number(x) || 0).toFixed(2)}`;

function metricVal(key, v) {
  if (v == null) return "–";
  if (key === "wall_s") return fmtMs(v * 1000);
  if (key === "cost_usd") return money(v);
  if (key === "tokens") return Math.round(v).toLocaleString("en");
  return String(Math.round(v * 10) / 10);
}

async function enough(ctx, S, reload) {
  S.busy = true;
  try {
    await ctx.api(`/api/harness/runs/${enc(S.id)}/stop`, { method: "POST", body: {} });
    toast(S.d.run.kind === "optimize" ? "Enough: it stops after this replay, and keeps its best so far." : "Stopping after this replay.");
  } catch (err) { toast(err.message, "bad"); }
  S.busy = false;
  reload();
}

async function decide(ctx, S, reload, body) {
  S.busy = true;
  try {
    await ctx.api(`/api/harness/runs/${enc(S.id)}/decide`, { method: "POST", body });
    toast(body.skip ? "Skipped: it carries on with the next change." : "Approved: it tries the change now.");
  } catch (err) { toast(err.message, "bad"); }
  S.busy = false;
  reload();
}

function waitingCard(ctx, S, reload) {
  const ch = S.d.run.waiting;
  if (!ch) return null;
  const pats = ch.patterns || [];
  const boxes = pats.map((p) => {
    const b = h("input", { type: "checkbox", value: p, checked: S.named.has(p) || null });
    b.addEventListener("change", () => { if (b.checked) S.named.add(p); else S.named.delete(p); go.disabled = !pats.every((x) => S.named.has(x)); });
    return b;
  });
  const go = btn("Try it: it widens", { primary: true, disabled: !pats.every((x) => S.named.has(x)), onclick: () => decide(ctx, S, reload, { approve_widening: pats }) });
  return h("section", { class: "pane hxr-wait" },
    h("div", { class: "hx-flag" }, icon("warn"), "waits for you"),
    h("h2", {}, rich(ch.title || "A change")),
    h("p", { class: "hx-note" }, rich(ch.why || ""), ". A replay runs its commands for real, in its sandbox: it widens what the agent may do without asking. Tick each pattern to try it, or skip it."),
    h("div", { class: "hx-pats" }, pats.map((p, i) => h("label", {}, boxes[i], h("code", {}, p)))),
    h("div", { class: "hx-acts" }, go, btn("Skip it", { onclick: () => decide(ctx, S, reload, { skip: true }) })));
}

function targetsBlock(sc, run) {
  const ts = (sc && sc.targets) || run.targets.map((t) => ({ ...t, met: false, now: null }));
  if (!ts.length) return null;
  const word = (k) => (TARGETS.find(([x]) => x === k) || [k, k])[1];
  return h("section", { class: "hx-sec" }, label("Targets", h("span", { class: "hx-mut" }, sc && sc.targets_met ? "every one met" : "each against your current harness")),
    h("div", { class: "pane hxr-tgs" }, ts.map((t) => h("div", { class: `hxr-tg${t.met ? " met" : ""}` },
      h("span", { class: "hxr-tgm" }, t.met ? icon("check", "i sm") : h("i", { class: "hxr-dot" })),
      h("b", {}, word(t.metric)),
      h("span", { class: "hx-mut hx-num" }, t.metric === "pass_rate" || t.metric === "success" ? `+${t.pct} pts wanted` : `${t.pct}% better wanted`),
      h("span", { class: "sp" }),
      h("span", { class: "hx-num" }, t.now == null ? "not measured yet" : typeof t.now === "number" ? (t.metric === "pass_rate" ? `${t.now >= 0 ? "+" : ""}${t.now} pts now` : `${t.now}% better now`) : String(t.now))))));
}

function decisionBlock(sc) {
  if (!sc || !sc.decision) return null;
  const r = sc.decision.result;
  return h("section", { class: "hx-sec" }, label("Decision"),
    h("div", { class: `pane hxr-dec ${r}` },
      h("div", { class: "hxr-dw" }, RESULT_WORDS[r] || r),
      h("ul", {}, (sc.decision.why || []).map((w) => h("li", {}, rich(w)))),
      sc.success ? h("p", { class: "hx-note hx-num" }, sc.unit === "runs"
        ? `Passed: A ${sc.success.a} of ${sc.success.a_runs} runs, B ${sc.success.b} of ${sc.success.b_runs}`
        : `Cases passing every run: A ${sc.success.a ?? "–"}, B ${sc.success.b ?? "–"} of ${sc.complete}`) : null,
      (sc.regressions || []).length ? h("p", { class: "hxr-bad" }, icon("warn", "i sm"), ` Passed under A and fails under B: ${sc.regressions.map((x) => x.name).join(", ")}. A regression always rejects.`) : null));
}

function metricsBlock(sc, d) {
  if (!sc || !(sc.metrics || []).length) return null;
  const wait = d && d.cases[0] && d.cases[0].case.your_wait_ms;
  return h("section", { class: "hx-sec" }, label("Each metric", h("span", { class: "hx-mut" }, "lower is better · B against A, beyond the noise of A's runs")),
    h("div", { class: "pane hxr-ms" }, sc.metrics.map((m) => h("div", { class: `hxr-m ${m.verdict}` },
      h("span", { class: "hxr-ml" }, m.label),
      h("span", { class: "hxr-mv hx-num" }, `${metricVal(m.key, m.a)} → ${metricVal(m.key, m.b)}`),
      h("span", { class: "hxr-mn hx-num hx-mut" }, m.noise != null ? `noise ±${metricVal(m.key, m.noise)}` : ""),
      h("span", { class: "hxr-mvd" }, VERDICT_WORDS[m.verdict] || m.verdict)))),
    wait ? h("p", { class: "hx-note hx-num" }, `Wall time counts each intervention at ${Math.round(wait / 1000)} s, the time you took to answer in the session (its median): a replay's scripted user answers at once.`) : null);
}

/** The words an instruction change gives the agent. */
function briefOf(ch) {
  const b = ch && ch.edit && ch.edit.agent && ch.edit.agent.brief;
  return b && b.length ? b.join(" ") : null;
}

function roundsBlock(run) {
  if (run.kind !== "optimize") return null;
  const log = run.log || [];
  return h("section", { class: "hx-sec" }, label("Changes tried", h("span", { class: "hx-num" }, String(log.filter((l) => l.version).length))),
    log.length ? h("ol", { class: "pane hxr-rounds" }, log.map((l) => h("li", { class: `hxr-r ${l.result}` },
      h("span", { class: "hxr-rn hx-num" }, l.round ? String(l.round) : "–"),
      h("div", { class: "hxr-rt" },
        h("div", {}, h("b", {}, rich((l.change && l.change.title) || "A change")), l.name ? h("span", { class: "hx-mut hx-num" }, ` · ${l.name}`) : null, l.change && l.change.widens ? [" ", h("span", { class: "hx-flag" }, icon("warn"), "widens")] : null),
        briefOf(l.change) ? h("blockquote", { class: "hxr-brief" }, briefOf(l.change)) : null,
        h("div", { class: "hx-mut hxr-why" }, rich([l.change && l.change.why, ...(Array.isArray(l.why) ? l.why : l.why ? [l.why] : [])].filter(Boolean).join(" · ")))),
      h("span", { class: "hxr-rw" }, ROUND_WORDS[l.result] || l.result))))
      : h("p", { class: "hx-note" }, "Nothing tried yet: it replays your current harness first, to measure its noise."));
}

function replayChip(r) {
  const tone = r.state === "passed" ? "ok" : r.state === "running" ? "run" : r.state === "error" ? "err" : "bad";
  const tip = [`run ${r.k}: ${r.state}`, r.why, r.model ? `on ${r.model}` : null, r.wall_s ? `${Math.round(r.wall_s)} s${r.wait_s ? ` (${Math.round(r.wait_s)} s of it your answers)` : ""}` : null, r.interventions ? `${r.interventions} intervention${r.interventions === 1 ? "" : "s"}` : null, r.not_in_log && r.not_in_log.length ? `${r.not_in_log.length} not in the log` : null, r.cost_usd ? money(r.cost_usd) : null].filter(Boolean).join(" · ");
  return h("span", { class: `hxr-chip ${tone}`, title: tip }, h("i"), h("span", { class: "hx-num" }, r.state === "running" ? "…" : `${r.interventions}`));
}

function casesBlock(d) {
  if (!(d.cases || []).length) return null;
  return h("section", { class: "hx-sec" }, label("Replays", h("span", { class: "hx-mut" }, "each square a replay · the number is its interventions")),
    h("div", { class: "pane hxr-cases" }, d.cases.map((c) => {
      const off = [...c.a, ...c.b].flatMap((r) => (r.not_in_log || []).map((x) => ({ ...x, side: r.side })));
      return h("div", { class: "hxr-case" },
        h("div", { class: "hxr-cn" }, h("b", {}, c.case.name), h("span", { class: "hx-mut hx-num" }, ` · turns ${c.case.from}–${c.case.to} · ${c.case.set === "held_out" ? "held out" : "train"}`)),
        h("div", { class: "hxr-sides" },
          h("div", {}, h("span", { class: "hxr-side" }, "A"), c.a.length ? c.a.map(replayChip) : h("span", { class: "hx-mut" }, "none yet")),
          h("div", {}, h("span", { class: "hxr-side" }, "B"), c.b.length ? c.b.map(replayChip) : h("span", { class: "hx-mut" }, "none yet"))),
        off.length ? h("details", { class: "hxr-off" }, h("summary", {}, `${off.length} thing${off.length === 1 ? "" : "s"} the log did not have`),
          h("ul", {}, off.slice(0, 20).map((x) => h("li", {}, h("span", { class: "hx-mut" }, `${x.kind} · `), rich(x.what || ""), x.answer ? h("span", { class: "hx-mut" }, ` → ${x.answer} (${x.by})`) : null)))) : null);
    })));
}

function nowBlock(d) {
  const n = d.now;
  if (!n) return null;
  return h("section", { class: "hx-sec" }, label("Playing now", h("span", { class: "hx-mut hx-num" }, `${n.side.toUpperCase()} · run ${n.k}`)),
    h("div", { class: "pane hxr-now" }, n.log.length ? n.log.map((e) => h("div", { class: "hxr-ev" },
      h("span", { class: "hx-mut hx-num" }, fmtWhen(e.ts)), h("span", { class: "hxr-evk" }, e.tool || e.kind), h("span", { class: "hxr-evs" }, e.summary || ""))) : h("p", { class: "hx-note" }, "Setting up its sandbox…")));
}

function draw(host, ctx, S, reload) {
  const d = S.d;
  const run = d.run;
  const sc = d.scores && d.scores.decision ? d.scores : null;
  const live = ["queued", "running", "needs_you"].includes(run.state);
  const isOpt = run.kind === "optimize";
  const title = isOpt
    ? (run.best ? `Optimize this: ${run.best.name} is the best so far` : `Optimize this on “${(d.cases[0] && d.cases[0].case.name) || "a stretch"}”`)
    : `${run.a.name} against ${run.b ? run.b.name : "?"}`;
  const pct = run.cap_usd ? Math.min(100, Math.round((run.spent_usd / run.cap_usd) * 100)) : 0;
  const insp = [
    h("div", {}, label("This run"),
      h("dl", { class: "kv hxr-kv" },
        h("dt", {}, "State"), h("dd", {}, h("span", { class: `hxr-st ${run.state}` }, STATE_WORDS[run.state] || run.state), run.why ? h("div", { class: "hx-mut" }, rich(run.why)) : null),
        h("dt", {}, "A"), h("dd", {}, run.a.name, isOpt ? h("span", { class: "hx-mut" }, " · your current harness") : null),
        run.b ? [h("dt", {}, "B"), h("dd", {}, run.b.name)] : null,
        run.best ? [h("dt", {}, "Best"), h("dd", {}, run.best.name, h("span", { class: "hx-mut" }, " · a candidate"))] : null,
        h("dt", {}, "Replays"), h("dd", { class: "hx-num" }, `${d.replays} counted · ${run.n} a version`),
        h("dt", {}, "Spent"), h("dd", { class: "hx-num" }, `${money(run.spent_usd)} of ${money(run.cap_usd)}`, h("div", { class: "hxr-bud" }, h("b", { style: { width: `${pct}%` } }))))),
    live ? h("div", { class: "hx-acts" }, btn(isOpt ? "Enough" : "Stop", { danger: true, icon: "stop", disabled: S.busy, onclick: () => enough(ctx, S, reload) })) : null,
    run.best && !live ? h("div", {}, label("What it kept"), h("p", { class: "hx-note" }, `${run.best.name} is a candidate version. One stretch says whether a change helps this stretch, not whether it generalizes: compare it on your tests before you promote it.`),
      h("div", { class: "hx-acts" }, btn("Versions", { primary: true, href: "#/harness/versions" }), btn("Compare on your tests", { href: `#/harness/evals?b=${enc(run.best.id)}` }))) : null,
    d.errors.length ? h("div", {}, label("Could not be played", h("span", { class: "hx-num" }, String(d.errors.length))), h("ul", { class: "hxe-small hx-mut" }, d.errors.slice(0, 5).map((e) => h("li", {}, rich(e.why || "")))), h("p", { class: "hxe-small hx-mut" }, "Not counted in the scorecard.")) : null,
    h("div", {}, label("The sandbox"), h("ul", { class: "hxe-small hx-mut" }, d.sandbox.map((s) => h("li", {}, s)))),
  ];
  host.replaceChildren(page({
    kicker: `Optimize the harness · ${isOpt ? "Optimize this" : "replay A against B"} · ${repoName(run.repo)}`,
    title,
    sub: isOpt ? "Your current harness first, to measure its noise; then one change at a time, each kept only when it is better beyond the noise and loses nothing A passed." : "Each case replayed under both versions, the order turning each time; a case passes only when every one of its runs does.",
    inspector: insp, cls: "hx-page hxr-page",
  }, tabs("evals"), waitingCard(ctx, S, reload), nowBlock(d), targetsBlock(sc, run), decisionBlock(sc), roundsBlock(run), metricsBlock(sc, d), casesBlock(d)));
}

useStyle("harness-run", `
.hxr-brief{margin:var(--s-1) 0;padding:var(--s-1) var(--s-3);border-left:2px solid var(--line-strong, var(--line));color:var(--text-2);font-size:var(--fs-sm)}
.hxr-wait{padding:var(--s-4);border-color:var(--warn-line);background:var(--warn-soft);margin-bottom:var(--s-4)}
.hxr-wait h2{margin:var(--s-2) 0 0;font:var(--w-title) var(--fs-lg)/var(--lh-lg) var(--sans)}
.hxr-tgs{display:flex;flex-direction:column}
.hxr-tg{display:flex;align-items:center;gap:var(--s-2);padding:var(--s-2) var(--s-4);border-top:var(--hair) solid var(--line);font-size:var(--fs-sm);flex-wrap:wrap;min-height:var(--touch)}
.hxr-tg:first-child{border-top:0}
.hxr-tg .sp{flex:1}
.hxr-tg.met b{color:var(--good-ink)}
.hxr-tgm{display:inline-flex;width:16px;color:var(--good-ink)}
.hxr-dot{display:inline-block;width:8px;height:8px;border-radius:50%;border:1.5px solid var(--text-3)}
.hxr-dec{padding:var(--s-3) var(--s-4)}
.hxr-dec ul{margin:var(--s-2) 0 0;padding-left:18px;font-size:var(--fs-sm)}
.hxr-dw{display:inline-block;font:var(--w-mid) var(--fs-xs)/var(--lh-xs) var(--mono);letter-spacing:.06em;text-transform:uppercase;padding:2px 8px;border-radius:var(--r-xs);background:var(--surface-3);color:var(--text-2)}
.hxr-dec.keep .hxr-dw{background:var(--good-soft);color:var(--good-ink)}
.hxr-dec.reject .hxr-dw{background:var(--crit-soft);color:var(--crit-ink)}
.hxr-bad{display:flex;gap:4px;align-items:flex-start;color:var(--crit-ink);font-size:var(--fs-sm);margin:var(--s-2) 0 0}
.hxr-ms{display:flex;flex-direction:column}
.hxr-m{display:grid;grid-template-columns:minmax(0,1.2fr) minmax(0,1fr) minmax(0,.9fr) auto;gap:var(--s-3);align-items:baseline;padding:var(--s-2) var(--s-4);border-top:var(--hair) solid var(--line);font-size:var(--fs-sm)}
.hxr-m:first-child{border-top:0}
.hxr-mvd{font:var(--w-mid) var(--fs-xs)/16px var(--mono);text-transform:uppercase;letter-spacing:.04em;color:var(--text-2);text-align:right}
.hxr-m.better .hxr-mvd{color:var(--good-ink)}.hxr-m.worse .hxr-mvd{color:var(--crit-ink)}
.hxr-rounds{list-style:none;margin:0;padding:0}
.hxr-r{display:flex;gap:var(--s-3);align-items:flex-start;padding:var(--s-3) var(--s-4);border-top:var(--hair) solid var(--line);font-size:var(--fs-sm)}
.hxr-r:first-child{border-top:0}
.hxr-rn{flex:none;width:20px;color:var(--text-2)}
.hxr-rt{flex:1;min-width:0;overflow-wrap:anywhere}
.hxr-why{font-size:var(--fs-xs);margin-top:2px}
.hxr-rw{flex:none;font:var(--w-mid) var(--fs-xs)/16px var(--mono);text-transform:uppercase;letter-spacing:.04em;color:var(--text-2)}
.hxr-r.kept .hxr-rw{color:var(--good-ink)}.hxr-r.running .hxr-rw{color:var(--accent-text)}
.hxr-cases{display:flex;flex-direction:column}
.hxr-case{padding:var(--s-3) var(--s-4);border-top:var(--hair) solid var(--line)}
.hxr-case:first-child{border-top:0}
.hxr-cn{font-size:var(--fs-sm);overflow-wrap:anywhere}
.hxr-sides{display:flex;flex-direction:column;gap:var(--s-1);margin-top:var(--s-2)}
.hxr-sides>div{display:flex;gap:4px;align-items:center;flex-wrap:wrap}
.hxr-side{width:18px;font:var(--w-mid) var(--fs-xs)/16px var(--mono);color:var(--text-2)}
.hxr-chip{display:inline-flex;align-items:center;gap:3px;height:22px;padding:0 6px;border-radius:var(--r-xs);border:var(--hair) solid var(--line-2);font-size:var(--fs-xs)}
.hxr-chip i{width:8px;height:8px;border-radius:2px;background:var(--text-3)}
.hxr-chip.ok i{background:var(--good)}.hxr-chip.bad i{background:var(--crit)}.hxr-chip.run i{background:var(--accent)}.hxr-chip.err i{background:var(--warn)}
.hxr-off{margin-top:var(--s-2);font-size:var(--fs-xs)}
.hxr-off summary{cursor:pointer;color:var(--text-2);min-height:28px;display:flex;align-items:center}
.hxr-off ul{margin:var(--s-1) 0 0;padding-left:18px;overflow-wrap:anywhere}
.hxr-now{padding:var(--s-2) var(--s-4);display:flex;flex-direction:column;gap:2px}
.hxr-ev{display:flex;gap:var(--s-2);font-size:var(--fs-xs);min-width:0}
.hxr-evk{flex:none;font-family:var(--mono);color:var(--text-2);min-width:80px}
.hxr-evs{flex:1;min-width:0;overflow:hidden;text-overflow:ellipsis;white-space:nowrap}
.hxr-kv{grid-template-columns:64px minmax(0,1fr)}
.hxr-bud{height:4px;border-radius:2px;background:var(--line);margin-top:4px;overflow:hidden}
.hxr-bud b{display:block;height:100%;background:var(--accent)}
@media (max-width:640px){.hxr-m{grid-template-columns:minmax(0,1fr) auto}.hxr-mn{display:none}.hxr-mv{grid-column:1}.hxr-mvd{grid-row:1;grid-column:2}}
`);
