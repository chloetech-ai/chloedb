// Make this a test, and Optimize this (HARNESS.md, "Evals as rows" and
// "Reproduce and replay"), at #/harness/evals/new/<agent>[?from=&to=]: a
// session's turns as a strip. Tap where a stretch starts, then where it
// ends, and the page shows the case it makes: where its code starts, the
// conversation before it, what the scripted user plays, the checks that
// judge a replay (ticked as proposed), and what it cannot reproduce.
// Then make it a test, or Optimize this: its replays under changed
// harnesses, one change at a time, until every target you tick is met,
// the budget is spent or you press Enough. The price is shown first.
import { h, useStyle, page, label, btn, icon } from "/levels/common.js";
import { tabs, fmtMs, fmtTok, rich, toast, failed, loading, seg, repoName } from "./parts.js";

/** The targets of Optimize this: key, label, what it counts. */
export const TARGETS = [
  ["interventions", "Interventions", "times you would have stepped in: prompts answered, questions, stuck"],
  ["wall_s", "Wall time", "from its first prompt to its last stop"],
  ["cost_usd", "Cost per run", "dollars a replay, from its usage"],
  ["pass_rate", "Pass rate", "replays whose checks all pass, in points"],
];

const CONTEXT_WORDS = {
  fresh: "None: it starts at the session's first prompt.",
  transcript: "The conversation before it, from a copy of the session's transcript, resumed.",
  code_only: "Not on disk: a replay starts from the code only.",
};

const CHECK_WORDS = {
  fail_to_pass: "failed, then passed",
  pass_to_pass: "passed, and still passes",
  command: "command",
  never: "never runs",
  limit: "at most",
};

const enc = encodeURIComponent;

export function render(host, ctx, path) {
  const agent = decodeURIComponent((path.match(/^#\/harness\/evals\/new\/([^/?]+)/) || [])[1] || "");
  const q = new URLSearchParams((location.hash.split("?")[1]) || "");
  const S = {
    alive: true, agent, tl: null, from: Number(q.get("from")) || 1, to: Number(q.get("to")) || 0, pick: "from",
    prop: null, propErr: null, on: {}, name: "", set: null, extra: "", busy: false,
    opt: { open: q.get("optimize") === "1", targets: Object.fromEntries(TARGETS.map(([k]) => [k, { on: k === "interventions", pct: 30 }])), n: 3, rounds: 4, cap: 8, est: null, estErr: null },
    seq: 0,
  };
  host.replaceChildren(loading("Reading the session's turns…"));
  (async () => {
    try {
      S.tl = await ctx.api(`/api/harness/timeline?agent=${enc(agent)}`);
      const last = S.tl.turns.length;
      S.to = Math.min(S.to || last, last);
      S.from = Math.max(1, Math.min(S.from, S.to || 1));
      if (S.alive) { draw(host, ctx, S); propose(host, ctx, S); }
    } catch (err) { if (S.alive) host.replaceChildren(failed(err)); }
  })();
  return { unmount() { S.alive = false; } };
}

async function propose(host, ctx, S) {
  if (!S.tl.turns.length) return;
  const seq = ++S.seq;
  S.prop = null; S.propErr = null; S.opt.est = null; S.opt.estErr = null;
  draw(host, ctx, S);
  try {
    const p = await ctx.api(`/api/harness/cases/propose?agent=${enc(S.agent)}&from=${S.from}&to=${S.to}`);
    if (seq !== S.seq || !S.alive) return;
    S.prop = p;
    S.on = Object.fromEntries(p.checks.map((c) => [c.id, !!c.on]));
    S.set = p.set;
    S.name = p.name;
  } catch (err) { if (seq === S.seq) S.propErr = err; }
  if (S.alive && seq === S.seq) draw(host, ctx, S);
}

function setRange(host, ctx, S, n) {
  if (S.pick === "from") { S.from = n; S.to = Math.max(n, S.to); S.pick = "to"; }
  else { if (n < S.from) { S.to = S.from; S.from = n; } else S.to = n; S.pick = "from"; }
  history.replaceState(null, "", `#/harness/evals/new/${enc(S.agent)}?from=${S.from}&to=${S.to}`);
  propose(host, ctx, S);
}

function strip(host, ctx, S) {
  const turns = S.tl.turns;
  const most = Math.max(1, ...turns.map((t) => t.ms || 0));
  return h("div", { class: "hxe-strip" },
    h("div", { class: "hxe-bars", role: "group", "aria-label": "Turns" }, turns.map((t) => {
      const inside = t.n >= S.from && t.n <= S.to;
      return h("button", {
        type: "button", class: `hxe-bar${inside ? " in" : ""}${t.tests.failed ? " fail" : ""}${t.n === S.from ? " a" : ""}${t.n === S.to ? " z" : ""}`,
        style: { "--hh": `${Math.max(18, Math.round(((t.ms || 0) / most) * 100))}%` },
        title: `Turn ${t.n}: ${t.prompt}`, "aria-pressed": String(inside), "aria-label": `Turn ${t.n}`,
        onclick: () => setRange(host, ctx, S, t.n),
      }, h("i"));
    })),
    h("div", { class: "hxe-axis hx-num" }, h("span", {}, "turn 1"), h("span", {}, `turn ${turns.length}`)),
    h("p", { class: "hxe-pick" }, S.pick === "from" ? "Tap the turn where the stretch starts." : `Starts at turn ${S.from}: tap where it ends.`));
}

function turnRows(host, ctx, S) {
  return h("div", { class: "pane hxe-turns" }, S.tl.turns.map((t) => {
    const inside = t.n >= S.from && t.n <= S.to;
    const bits = [
      t.tools ? `${t.tools} tool${t.tools === 1 ? "" : "s"}` : null,
      t.edits ? `${t.edits} edit${t.edits === 1 ? "" : "s"}` : null,
      t.tests.ran ? `${t.tests.ran} test${t.tests.ran === 1 ? "" : "s"}${t.tests.failed ? `, ${t.tests.failed} failed` : ""}` : null,
      t.prompts ? `${t.prompts} permission prompt${t.prompts === 1 ? "" : "s"}` : null,
      t.asks ? `${t.asks} question${t.asks === 1 ? "" : "s"}` : null,
    ].filter(Boolean).join(" · ");
    return h("button", { type: "button", class: `hxe-turn${inside ? " in" : ""}`, onclick: () => setRange(host, ctx, S, t.n) },
      h("span", { class: "hxe-tn hx-num" }, String(t.n)),
      h("span", { class: "hxe-tt" }, h("span", { class: "hxe-tp" }, t.prompt || "(no words)"), h("span", { class: "hxe-tb hx-mut" }, [bits || "no tools", fmtMs(t.ms), t.cost_usd ? `$${t.cost_usd.toFixed(2)}` : null].filter(Boolean).join(" · "))),
      t.code ? null : h("span", { class: "hxe-nocode", title: "Workbench has no record of the code at this turn's start" }, "no code"));
  }));
}

const LIMIT_WORDS = { prompts: "permission prompt", interventions: "intervention", turns: "turn" };

function checkLine(c, S, onChange) {
  const box = h("input", { type: "checkbox", checked: !!S.on[c.id] || null });
  box.addEventListener("change", () => { S.on[c.id] = box.checked; onChange(); });
  const what = c.kind === "fail_to_pass" || c.kind === "pass_to_pass" ? [h("code", {}, c.test), ` ${CHECK_WORDS[c.kind]}`]
    : c.kind === "command" ? [h("code", {}, c.run), " exits 0"]
    : c.kind === "never" ? ["No command contains ", h("code", {}, c.pattern)]
    : c.kind === "limit" ? [`At most ${c.max} ${LIMIT_WORDS[c.metric] || c.metric}${c.max === 1 ? "" : "s"}`] : [c.kind];
  return h("label", { class: "hxe-chk" }, box, h("span", {}, h("span", {}, what), c.why ? h("span", { class: "hx-mut" }, ` · ${c.why}`) : null));
}

function chosenChecks(S) {
  const out = S.prop.checks.filter((c) => S.on[c.id]).map((c) => ({ ...c }));
  const extra = S.extra.trim();
  if (extra) out.push({ kind: "command", run: extra });
  return out;
}

async function makeTest(host, ctx, S) {
  const checks = chosenChecks(S);
  if (!checks.length) { toast("Tick a check, or add a command: something has to say whether a replay worked.", "bad"); return; }
  S.busy = true; draw(host, ctx, S);
  try {
    const r = await ctx.api("/api/harness/cases", { method: "POST", body: { agent: S.agent, from: S.from, to: S.to, name: S.name, set: S.set, checks } });
    toast(`Kept “${r.case.name}” as a test, in ${r.case.set === "held_out" ? "the held-out set" : "the train set"}.`);
    ctx.go ? ctx.go("#/harness/evals") : (location.hash = "#/harness/evals");
  } catch (err) { toast(err.message, "bad"); }
  S.busy = false; if (S.alive) draw(host, ctx, S);
}

function optBody(S) {
  const targets = TARGETS.filter(([k]) => S.opt.targets[k].on).map(([k]) => ({ metric: k, pct: Number(S.opt.targets[k].pct) || 0 }));
  // The checks on this page judge its replays, as they would a test's.
  return { kind: "optimize", agent: S.agent, from: S.from, to: S.to, n: S.opt.n, rounds: S.opt.rounds, cap_usd: S.opt.cap, targets, checks: chosenChecks(S), name: S.name || undefined };
}

async function estimate(host, ctx, S) {
  S.opt.est = null; S.opt.estErr = null; draw(host, ctx, S);
  try { S.opt.est = await ctx.api("/api/harness/runs/estimate", { method: "POST", body: optBody(S) }); }
  catch (err) { S.opt.estErr = err; }
  if (S.alive) draw(host, ctx, S);
}

async function startOptimize(host, ctx, S) {
  const body = { ...optBody(S), confirm_usd: S.opt.est && S.opt.est.confirm_usd };
  if (!body.targets.length) { toast("Tick at least one target.", "bad"); return; }
  S.busy = true; draw(host, ctx, S);
  try {
    const r = await ctx.api("/api/harness/runs", { method: "POST", body });
    location.hash = `#/harness/runs/${enc(r.run.id)}`;
  } catch (err) {
    toast(err.message, "bad");
    if (err.code === "confirm_cost") estimate(host, ctx, S);
  }
  S.busy = false; if (S.alive) draw(host, ctx, S);
}

function optimizePanel(host, ctx, S) {
  const o = S.opt;
  const redraw = () => { o.est = null; o.estErr = null; draw(host, ctx, S); };
  const rows = TARGETS.map(([k, l, what]) => {
    const t = o.targets[k];
    const box = h("input", { type: "checkbox", checked: t.on || null, "aria-label": l });
    box.addEventListener("change", () => { t.on = box.checked; redraw(); });
    const pct = h("input", { type: "number", class: "field hxe-pct hx-num", min: "1", max: "100", step: "5", value: String(t.pct), disabled: !t.on || null, "aria-label": `${l}: how much better` });
    pct.addEventListener("change", () => { t.pct = Math.max(1, Math.min(100, Number(pct.value) || 30)); redraw(); });
    return h("div", { class: `hxe-tg${t.on ? " on" : ""}` },
      h("label", { class: "hxe-tgl" }, box, h("span", {}, h("b", {}, l), h("span", { class: "hx-mut" }, ` ${what}`))),
      h("span", { class: "hxe-tgv" }, k === "pass_rate" ? "+" : "−", pct, k === "pass_rate" ? " pts" : " %"));
  });
  const num = (key, min, max, step, aria) => {
    const i = h("input", { type: "number", class: "field hxe-n hx-num", min: String(min), max: String(max), step: String(step), value: String(o[key]), "aria-label": aria });
    i.addEventListener("change", () => { o[key] = Math.max(min, Math.min(max, Number(i.value) || min)); redraw(); });
    return i;
  };
  const anyOn = TARGETS.some(([k]) => o.targets[k].on);
  const est = o.est;
  return h("div", { class: "hxe-opt" },
    label("Optimize this"),
    h("p", { class: "hx-note" }, "It replays this stretch under your current harness first, to measure its noise, then tries one change at a time (an allow rule for a command its replays were asked about, a memory pin the week's misses call for), keeping a change only when it is better beyond the noise and passes everything the last one passed. It stops when every target you tick is met, the budget is spent, or you press Enough."),
    h("div", { class: "hxe-tgs" }, rows),
    h("p", { class: "hxe-small hx-mut" }, "Each target is a share better than your current harness, and beyond the noise of its own replays."),
    h("div", { class: "hxe-ns" },
      h("label", {}, h("span", {}, "Replays a version"), num("n", 2, 10, 1, "Replays a version")),
      h("label", {}, h("span", {}, "Changes, at most"), num("rounds", 1, 8, 1, "Changes, at most")),
      h("label", {}, h("span", {}, "Budget, $"), num("cap", 1, 100, 1, "Budget in dollars"))),
    est ? h("div", { class: "hxe-est" },
      h("div", { class: "hxe-price" }, h("b", { class: "hx-num" }, `up to $${est.most_usd.toFixed(2)}`), h("span", { class: "hx-mut" }, ` · ${est.replays} replays at about $${est.per_replay_usd.toFixed(2)} each, from ${est.estimated_from}`)),
      est.over_cap ? h("p", { class: "hxe-warn" }, icon("warn", "i sm"), ` This stretch cost more than the cap of one replay ($${est.cap_run_usd.toFixed(2)}): pick fewer turns.`) : null,
      h("p", { class: "hxe-small hx-mut" }, est.honest)) : null,
    o.estErr ? h("p", { class: "hxe-warn" }, icon("warn", "i sm"), " ", o.estErr.message) : null,
    h("div", { class: "hx-acts" },
      est ? btn(S.busy ? "Starting…" : `Start, up to $${est.most_usd.toFixed(2)}`, { primary: true, disabled: S.busy || est.over_cap || !anyOn, onclick: () => startOptimize(host, ctx, S) })
        : btn("Show the price", { primary: true, disabled: !anyOn || !S.prop || !S.prop.replayable, onclick: () => estimate(host, ctx, S) }),
      btn("Not now", { onclick: () => { o.open = false; draw(host, ctx, S); } })));
}

function draw(host, ctx, S) {
  const tl = S.tl;
  const a = tl.agent;
  const last = tl.turns.length;
  if (!last) {
    host.replaceChildren(page({ kicker: "Optimize the harness · make this a test", title: `${a.name} has no prompt yet`, sub: "A test is made from turns a session played. Come back once it has run.", cls: "hx-page" }, tabs("evals")));
    return;
  }
  const p = S.prop;
  const sel = tl.turns.filter((t) => t.n >= S.from && t.n <= S.to);
  const sum = (k) => sel.reduce((x, t) => x + (t[k] || 0), 0);
  const nameIn = h("input", { class: "field", value: S.name || "", "aria-label": "The test's name", maxlength: "80" });
  nameIn.addEventListener("input", () => { S.name = nameIn.value; });
  const extraIn = h("input", { class: "field hx-num", value: S.extra, placeholder: "A command that exits 0 when it worked, e.g. grep -q fixed notes.txt", "aria-label": "Add a command check" });
  extraIn.addEventListener("input", () => { S.extra = extraIn.value; });
  const sections = p ? [
    h("section", { class: "hx-sec" }, label("Where it starts"),
      h("div", { class: "pane hxe-facts" },
        h("dl", { class: "hxe-dl" },
          h("dt", {}, "The code"), h("dd", {}, p.start_from, p.commit ? h("code", { class: "hxe-sha" }, String(p.commit).slice(0, 8)) : null),
          h("dt", {}, "Before it"), h("dd", {}, CONTEXT_WORDS[p.context] || p.context),
          h("dt", {}, "It plays"), h("dd", {}, `${p.prompts} prompt${p.prompts === 1 ? "" : "s"}, and the answers to ${p.permissions} permission prompt${p.permissions === 1 ? "" : "s"} and ${p.script.filter((x) => x.kind === "answer").length} question${p.script.filter((x) => x.kind === "answer").length === 1 ? "" : "s"}, as you gave them`),
          h("dt", {}, "It took"), h("dd", { class: "hx-num" }, `${fmtMs(p.wall_ms)} · ${fmtTok(p.tokens)} tokens · $${(p.cost_usd || 0).toFixed(2)}`)))),
    p.warnings.length ? h("section", { class: "hx-sec" }, label("What it cannot reproduce", h("span", { class: "hx-num" }, String(p.warnings.length))),
      h("ul", { class: "hxe-warns" }, p.warnings.map((w) => h("li", {}, icon("warn", "i sm"), h("span", {}, rich(w)))))) : null,
    h("section", { class: "hx-sec" }, label("How a replay is judged", h("span", { class: "hx-mut" }, "ticked as proposed")),
      h("div", { class: "pane hxe-chks" }, p.checks.map((c) => checkLine(c, S, () => {})),
        h("div", { class: "hxe-extra" }, extraIn))),
    h("section", { class: "hx-sec" }, label("The user's side", h("span", { class: "hx-mut" }, "played back in order, never by a model")),
      h("ol", { class: "pane hxe-script" }, p.script.slice(0, 40).map((x) => h("li", {},
        h("span", { class: "hxe-sk" }, x.kind === "prompt" ? `turn ${x.turn}` : x.kind === "permission" ? "asked" : "question"),
        h("span", { class: "hxe-sv" }, x.kind === "prompt" ? x.text : x.kind === "permission" ? [x.what, h("span", { class: "hx-mut" }, ` → ${x.label || x.answer}`)] : [x.question, h("span", { class: "hx-mut" }, ` → ${x.answer}`)]))),
        p.script.length > 40 ? h("li", { class: "hx-mut" }, `and ${p.script.length - 40} more`) : null)),
  ] : [S.propErr ? h("div", { class: "hx-sec" }, failed(S.propErr)) : loading("Reading the stretch…")];
  const ready = p && p.replayable;
  const insp = [
    h("div", {}, label(`Turns ${S.from}–${S.to} of ${last}`),
      h("dl", { class: "kv hxe-kv" },
        h("dt", {}, "Tools"), h("dd", { class: "hx-num" }, String(sum("tools"))),
        h("dt", {}, "Prompts"), h("dd", { class: "hx-num" }, String(sum("prompts"))),
        h("dt", {}, "Cost"), h("dd", { class: "hx-num" }, `$${sum("cost_usd").toFixed(2)}`))),
    p ? h("div", {}, label("Keep it as a test"),
      h("div", { class: "hxe-form" }, h("label", {}, h("span", {}, "Name"), nameIn),
        h("div", {}, h("span", { class: "hxe-fl" }, "Set"), seg([["train", "Train"], ["held_out", "Held out"]], S.set, (k) => { S.set = k; draw(host, ctx, S); }, { label: "Set" })),
        h("p", { class: "hxe-small hx-mut" }, "One case in three is held out, by session, so a change is judged on sessions it was not made from.")),
      h("div", { class: "hx-acts" }, btn(S.busy ? "Keeping…" : "Make this a test", { primary: !S.opt.open, disabled: !ready || S.busy, onclick: () => makeTest(host, ctx, S) }),
        S.opt.open ? null : btn("Optimize this…", { disabled: !ready, onclick: () => { S.opt.open = true; draw(host, ctx, S); } }))) : null,
    p && S.opt.open ? optimizePanel(host, ctx, S) : null,
    h("div", { class: "hxe-sb" }, label("The sandbox"), h("ul", { class: "hxe-small hx-mut" },
      h("li", {}, "A clone at the stretch's start, with no remote: nothing can be pushed."),
      h("li", {}, "Its own empty memory, the distiller off: your ChloeCtx store is not read or written."),
      h("li", {}, "Things outside the repository (a database, a service) are not rewound."))),
  ];
  host.replaceChildren(page({
    kicker: `Optimize the harness · make this a test · ${repoName(a.repo)}`,
    title: `${a.name}, turns ${S.from}–${S.to}`,
    sub: "Pick a stretch of this session. Workbench can play it again from the same code, with your prompts and answers, and judge each replay by the checks you keep.",
    inspector: insp, cls: "hx-page hxe-page",
  }, tabs("evals"), strip(host, ctx, S), ...sections, h("section", { class: "hx-sec" }, label("Every turn"), turnRows(host, ctx, S))));
}

useStyle("harness-timeline", `
.hxe-strip{margin-top:var(--s-2)}
.hxe-bars{display:flex;gap:2px;height:72px;align-items:flex-end;overflow-x:auto;padding-bottom:2px}
.hxe-bar{flex:1 1 0;min-width:10px;max-width:40px;height:100%;padding:0;border:0;background:none;cursor:pointer;display:flex;align-items:flex-end;border-radius:2px}
.hxe-bar i{display:block;width:100%;height:var(--hh);background:var(--line-2);border-radius:2px}
.hxe-bar.in i{background:var(--accent)}
.hxe-bar.fail i{box-shadow:inset 0 3px 0 var(--crit)}
.hxe-bar.a i,.hxe-bar.z i{outline:1.5px solid var(--text);outline-offset:1px}
.hxe-bar:focus-visible{outline:2px solid var(--accent);outline-offset:2px}
.hxe-axis{display:flex;justify-content:space-between;font-size:var(--fs-xs);color:var(--text-3);margin-top:var(--s-1)}
.hxe-pick{display:flex;align-items:center;gap:4px;margin:var(--s-2) 0 0;font-size:var(--fs-sm);color:var(--text-2)}
.hxe-turns{display:flex;flex-direction:column}
.hxe-turn{display:flex;gap:var(--s-3);align-items:flex-start;width:100%;text-align:left;padding:var(--s-2) var(--s-4);border:0;border-top:var(--hair) solid var(--line);background:none;font:inherit;color:inherit;cursor:pointer;min-height:var(--touch)}
.hxe-turn:first-child{border-top:0}
.hxe-turn.in{background:var(--accent-soft)}
.hxe-turn:hover{background:var(--surface-3)}
.hxe-turn.in:hover{background:var(--accent-soft)}
.hxe-tn{flex:none;width:28px;color:var(--text-2);padding-top:1px}
.hxe-tt{flex:1;min-width:0;display:flex;flex-direction:column;gap:2px}
.hxe-tp{overflow:hidden;text-overflow:ellipsis;white-space:nowrap}
.hxe-tb{font-size:var(--fs-xs)}
.hxe-nocode{flex:none;font:var(--w-mid) var(--fs-xs)/16px var(--mono);color:var(--warn-ink);text-transform:uppercase;letter-spacing:.04em}
.hxe-facts{padding:var(--s-3) var(--s-4)}
.hxe-dl{display:grid;grid-template-columns:96px minmax(0,1fr);gap:var(--s-2) var(--s-3);margin:0;font-size:var(--fs-sm)}
.hxe-dl dt{color:var(--text-2)}.hxe-dl dd{margin:0;min-width:0;overflow-wrap:anywhere}
.hxe-sha{margin-left:6px;color:var(--text-2)}
.hxe-warns{list-style:none;margin:0;padding:0;display:flex;flex-direction:column;gap:var(--s-2)}
.hxe-warns li{display:flex;gap:var(--s-2);font-size:var(--fs-sm);color:var(--warn-ink)}
.hxe-warns svg{flex:none;margin-top:2px}
.hxe-chks{display:flex;flex-direction:column}
.hxe-chk{display:flex;gap:var(--s-3);align-items:flex-start;padding:var(--s-2) var(--s-4);border-top:var(--hair) solid var(--line);font-size:var(--fs-sm);min-height:var(--touch);cursor:pointer;overflow-wrap:anywhere}
.hxe-chk:first-child{border-top:0}
.hxe-chk input{margin-top:3px;flex:none;accent-color:var(--accent)}
.hxe-extra{padding:var(--s-2) var(--s-4) var(--s-3);border-top:var(--hair) solid var(--line)}
.hxe-extra input{width:100%}
.hxe-script{list-style:none;margin:0;padding:var(--s-2) 0}
.hxe-script li{display:flex;gap:var(--s-3);padding:var(--s-1) var(--s-4);font-size:var(--fs-sm);min-width:0}
.hxe-sk{flex:none;width:64px;font:var(--w-mid) var(--fs-xs)/20px var(--mono);color:var(--text-2);text-transform:uppercase;letter-spacing:.04em}
.hxe-sv{flex:1;min-width:0;overflow-wrap:anywhere}
.hxe-opt{display:flex;flex-direction:column;gap:var(--s-3)}
.hxe-opt .hx-note{margin:0}
.hxe-tgs{display:flex;flex-direction:column;border:var(--hair) solid var(--line);border-radius:var(--r-sm);background:var(--surface)}
.hxe-tg{display:flex;gap:var(--s-2);align-items:center;justify-content:space-between;padding:var(--s-2) var(--s-3);border-top:var(--hair) solid var(--line);flex-wrap:wrap}
.hxe-tg:first-child{border-top:0}
.hxe-tgl{display:flex;gap:var(--s-2);align-items:flex-start;font-size:var(--fs-sm);flex:1 1 180px;min-width:0;cursor:pointer}
.hxe-tgl input{margin-top:3px;flex:none;accent-color:var(--accent)}
.hxe-tgv{display:flex;align-items:center;gap:4px;font-size:var(--fs-sm);color:var(--text-2);flex:none}
@media (max-width:640px){.hxe-dl{grid-template-columns:minmax(0,1fr)}.hxe-dl dt{margin-top:var(--s-2)}.hxe-bars{height:56px}}
`);
