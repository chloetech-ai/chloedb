// Tests (HARNESS.md, "Evals as rows"), at #/harness/evals[?b=<version>]:
// the repository's tests, each a stretch of a session Workbench can play
// again, with its latest result under each version it ran; the sessions a
// test can be made from (Make this a test); replay A against B on the
// tests, its price shown first and a widening confirmed by name; and the
// runs so far, each opening its scorecard.
import { h, useStyle, page, label, btn, icon } from "/levels/common.js";
import { tabs, fmtWhen, rich, repoQ, repoName, toast, failed, loading, seg, confirmWidening, OPTIMIZE_THIS } from "./parts.js";

const enc = encodeURIComponent;
const STATE_WORDS = { queued: "queued", running: "playing", needs_you: "needs you", done: "done", stopped: "stopped", failed: "failed" };

export function render(host, ctx) {
  const q = new URLSearchParams((location.hash.split("?")[1]) || "");
  const S = { alive: true, d: null, runs: [], vers: null, timer: null, cmp: { a: null, b: q.get("b"), suite: "", n: 3, cap: 8, est: null, err: null, busy: false }, sure: null };
  host.replaceChildren(loading("Reading your tests…"));
  const load = async () => {
    try {
      const rq = repoQ(ctx);
      const [d, runs, vers] = await Promise.all([
        ctx.api(`/api/harness/suites?${rq}`),
        ctx.api(`/api/harness/runs?${rq}`),
        ctx.api(`/api/harness/versions?${rq}`),
      ]);
      if (!S.alive) return;
      S.d = d; S.runs = runs.runs; S.vers = vers;
      const cfg = vers.versions.filter((v) => v.kind !== "run");
      if (!S.cmp.a) S.cmp.a = (vers.current && vers.current.id) || (cfg.find((v) => v.state === "current") || {}).id || null;
      if (!S.cmp.b) S.cmp.b = (cfg.find((v) => v.state === "draft") || cfg.find((v) => v.state === "candidate") || {}).id || null;
      draw(host, ctx, S, load);
      clearTimeout(S.timer);
      if (S.runs.some((r) => ["queued", "running"].includes(r.state))) S.timer = setTimeout(load, 4000);
    } catch (err) { if (S.alive) host.replaceChildren(failed(err)); }
  };
  load();
  return { unmount() { S.alive = false; clearTimeout(S.timer); } };
}

function cmpBody(S) {
  const c = S.cmp;
  return { kind: "compare", repo: S.d.repo, a: c.a, b: c.b, suite: c.suite || undefined, n: c.n, cap_usd: c.cap };
}

async function estimate(host, ctx, S, reload) {
  S.cmp.est = null; S.cmp.err = null; draw(host, ctx, S, reload);
  try { S.cmp.est = await ctx.api("/api/harness/runs/estimate", { method: "POST", body: cmpBody(S) }); }
  catch (err) { S.cmp.err = err; }
  if (S.alive) draw(host, ctx, S, reload);
}

async function startCompare(host, ctx, S, reload) {
  const c = S.cmp;
  let body = { ...cmpBody(S), confirm_usd: c.est && c.est.confirm_usd };
  c.busy = true; draw(host, ctx, S, reload);
  try {
    let r;
    try { r = await ctx.api("/api/harness/runs", { method: "POST", body }); }
    catch (err) {
      if (err.code !== "widens") throw err;
      let pats = [];
      try { pats = JSON.parse((String(err.message).match(/approve_widening: (\[.*\])$/) || [])[1] || "[]"); } catch { pats = []; }
      const named = await confirmWidening(pats, err.message.replace(/ To test it.*$/, ""));
      if (!named) { toast("Not started: the widening was not confirmed."); c.busy = false; draw(host, ctx, S, reload); return; }
      body = { ...body, approve_widening: named };
      r = await ctx.api("/api/harness/runs", { method: "POST", body });
    }
    location.hash = `#/harness/runs/${enc(r.run.id)}`;
    return;
  } catch (err) {
    toast(err.message, "bad");
    if (err.code === "confirm_cost") estimate(host, ctx, S, reload);
  }
  c.busy = false; if (S.alive) draw(host, ctx, S, reload);
}

async function setOf(ctx, id, set, reload) {
  try { await ctx.api(`/api/harness/cases/${enc(id)}`, { method: "POST", body: { set } }); reload(); }
  catch (err) { toast(err.message, "bad"); }
}

async function del(ctx, S, id, reload) {
  if (S.sure !== id) { S.sure = id; reload(); return; }
  try { await ctx.api(`/api/harness/cases/${enc(id)}`, { method: "DELETE" }); toast("Deleted the test; its runs stay as history."); }
  catch (err) { toast(err.message, "bad"); }
  S.sure = null; reload();
}

function caseRow(ctx, S, c, reload) {
  const res = c.results || [];
  const kinds = [...new Set((c.checks || []).map((k) => k.kind))];
  return h("div", { class: "hxe-case" },
    h("div", { class: "hxe-ch" },
      h("b", {}, c.name),
      h("span", { class: "hx-mut hx-num" }, ` · turns ${c.from}–${c.to} of ${c.turns_all} · ${c.prompts} prompt${c.prompts === 1 ? "" : "s"} · ${c.checks.length} check${c.checks.length === 1 ? "" : "s"}${kinds.length ? ` (${kinds.map((k) => k.replace(/_/g, " ")).join(", ")})` : ""}`)),
    h("div", { class: "hxe-cr" },
      res.length ? res.map((r) => h("span", { class: `hxe-res ${r.pass_k ? "ok" : "bad"}`, title: `${r.name}: passed ${r.passed} of ${r.runs} runs, ${fmtWhen(r.when)}` }, h("i"), h("span", { class: "hx-num" }, `${r.name} ${r.passed}/${r.runs}`)))
        : h("span", { class: "hx-mut hxe-small" }, "not replayed yet"),
      h("span", { class: "sp" }),
      seg([["train", "Train"], ["held_out", "Held out"]], c.set, (k) => setOf(ctx, c.id, k, reload), { label: `${c.name}: set` }),
      c.agent && OPTIMIZE_THIS ? btn("Optimize", { sm: true, href: `#/harness/evals/new/${enc(c.agent)}?from=${c.from}&to=${c.to}&optimize=1` }) : null,
      btn(S.sure === c.id ? "Delete, sure?" : "Delete", { sm: true, ghost: S.sure !== c.id, danger: S.sure === c.id, onclick: () => del(ctx, S, c.id, reload) })));
}

function compareBox(host, ctx, S, reload) {
  const c = S.cmp;
  const cfg = S.vers.versions.filter((v) => v.kind !== "run" && v.state !== "retired");
  const opts = (on) => cfg.map((v) => h("option", { value: v.id, selected: v.id === on || null }, `${v.name} · ${v.state}`));
  const selA = h("select", { class: "field", "aria-label": "A, the version to beat" }, opts(c.a));
  const selB = h("select", { class: "field", "aria-label": "B, the version to try" }, opts(c.b));
  const suites = S.d.suites.filter((s) => s.cases.length);
  const selS = h("select", { class: "field", "aria-label": "Which tests" }, h("option", { value: "" }, `Every test (${S.d.cases})`), suites.map((s) => h("option", { value: s.id, selected: s.id === c.suite || null }, `${s.name} (${s.cases.length})`)));
  const changed = () => { c.est = null; c.err = null; draw(host, ctx, S, reload); };
  selA.addEventListener("change", () => { c.a = selA.value; changed(); });
  selB.addEventListener("change", () => { c.b = selB.value; changed(); });
  selS.addEventListener("change", () => { c.suite = selS.value; changed(); });
  const num = (key, min, max, aria) => {
    const i = h("input", { type: "number", class: "field hxe-n hx-num", min: String(min), max: String(max), value: String(c[key]), "aria-label": aria });
    i.addEventListener("change", () => { c[key] = Math.max(min, Math.min(max, Number(i.value) || min)); changed(); });
    return i;
  };
  const same = c.a && c.a === c.b;
  const est = c.est;
  return h("div", { class: "hxe-cmp" },
    label("Replay A against B"),
    h("p", { class: "hx-note" }, "Each test played under both versions, in its own sandbox, the order turning each time. A against itself measures the noise."),
    h("div", { class: "hxe-form" },
      h("label", {}, h("span", {}, "A, to beat"), selA),
      h("label", {}, h("span", {}, "B, to try"), selB),
      h("label", {}, h("span", {}, "Tests"), selS),
      h("div", { class: "hxe-ns" }, h("label", {}, h("span", {}, "Runs a side"), num("n", same ? 2 : 1, 10, "Runs a side")), h("label", {}, h("span", {}, "Budget, $"), num("cap", 1, 500, "Budget in dollars")))),
    est ? h("div", { class: "hxe-est" },
      h("div", { class: "hxe-price" }, h("b", { class: "hx-num" }, `up to $${est.most_usd.toFixed(2)}`), h("span", { class: "hx-mut" }, ` · ${est.replays} replays of ${est.cases} test${est.cases === 1 ? "" : "s"}, from ${est.estimated_from}`)),
      (est.skipped || []).length ? h("p", { class: "hxe-warn" }, icon("warn", "i sm"), ` ${est.skipped.length} test${est.skipped.length === 1 ? "" : "s"} left out: ${est.skipped[0].why}`) : null,
      est.over_cap ? h("p", { class: "hxe-warn" }, icon("warn", "i sm"), " Over the budget: fewer runs or tests, or a higher budget.") : null,
      h("p", { class: "hxe-small hx-mut" }, est.honest)) : null,
    c.err ? h("p", { class: "hxe-warn" }, icon("warn", "i sm"), " ", c.err.message) : null,
    h("div", { class: "hx-acts" }, est
      ? btn(c.busy ? "Starting…" : `Start, up to $${est.most_usd.toFixed(2)}`, { primary: true, disabled: c.busy || est.over_cap || !est.cases, onclick: () => startCompare(host, ctx, S, reload) })
      : btn("Show the price", { primary: true, disabled: !S.d.cases || !c.a || !c.b, onclick: () => estimate(host, ctx, S, reload) })));
}

function draw(host, ctx, S, reload) {
  const d = S.d;
  const all = d.suites.flatMap((s) => s.cases);
  const held = all.filter((c) => c.set === "held_out").length;
  const sessions = d.sessions.filter((s) => s.turns > 0);
  const tests = d.suites.filter((s) => s.cases.length).map((s) => h("section", { class: "hx-sec" },
    label(s.name, h("span", { class: "hx-num" }, String(s.cases.length))),
    h("div", { class: "pane hxe-cases" }, s.cases.map((c) => caseRow(ctx, S, c, reload)))));
  const made = h("section", { class: "hx-sec" }, label("Make this a test", h("span", { class: "hx-mut" }, "pick a session, then the turns")),
    sessions.length ? h("div", { class: "pane hxe-sess" }, sessions.slice(0, 20).map((s) => h("a", { class: "hxe-ses", href: `#/harness/evals/new/${enc(s.id)}` },
      h("span", { class: "hxe-sn" }, h("b", {}, s.name), h("span", { class: "hx-mut" }, ` ${s.task || ""}`)),
      h("span", { class: "hx-mut hx-num hxe-sm" }, `${s.turns} turn${s.turns === 1 ? "" : "s"}${s.cases ? ` · ${s.cases} test${s.cases === 1 ? "" : "s"}` : ""}${s.running ? " · running" : ""} · ${fmtWhen(s.created)}`),
      icon("chev", "i sm"))))
      : h("p", { class: "hx-note" }, "No session of this repository has a prompt yet. Start an agent, and its turns show here."));
  const runs = S.runs.length ? h("section", { class: "hx-sec" }, label("Runs", h("span", { class: "hx-num" }, String(S.runs.length))),
    h("div", { class: "pane hxe-runs" }, S.runs.slice(0, 12).map((r) => h("a", { class: "hxe-run", href: `#/harness/runs/${enc(r.id)}` },
      h("span", { class: `hxr-st ${r.state}` }, STATE_WORDS[r.state] || r.state),
      h("span", { class: "hxe-sn" }, r.kind === "optimize" ? `Optimize this${r.best ? `: kept ${r.best.name}` : ""}` : `${r.a.name} against ${r.b ? r.b.name : "?"}`, r.why ? h("span", { class: "hx-mut" }, ` · ${r.why}`) : null),
      h("span", { class: "hx-mut hx-num hxe-sm" }, `$${r.spent_usd.toFixed(2)} · ${fmtWhen(r.created)}`), icon("chev", "i sm"))))) : null;
  const insp = [
    h("div", {}, label("Your tests"),
      h("dl", { class: "kv hxe-kv" },
        h("dt", {}, "Tests"), h("dd", { class: "hx-num" }, String(all.length)),
        h("dt", {}, "Held out"), h("dd", { class: "hx-num" }, `${held}`),
        h("dt", {}, "Enough"), h("dd", {}, all.length >= 6 ? "to tell a difference" : `${6 - all.length} more to tell a difference`))),
    all.length ? compareBox(host, ctx, S, reload) : h("div", {}, label("Replay A against B"), h("p", { class: "hx-note" }, "Make a test first: then two versions of the harness can be compared on it.")),
  ];
  host.replaceChildren(page({
    kicker: `Optimize the harness · tests · ${repoName(d.repo)}`,
    title: all.length ? `${all.length} test${all.length === 1 ? "" : "s"} to judge a change by` : "Make a session into a test",
    sub: "A test is a stretch of a session, played again from the same code with your prompts and answers, and judged by checks you keep. Versions of the harness are compared on them before you promote one.",
    inspector: insp, cls: "hx-page hxe-page",
  }, tabs("evals"), ...tests, made, runs));
}

useStyle("harness-evals", `
.hxe-cases,.hxe-sess,.hxe-runs{display:flex;flex-direction:column}
.hxe-case{padding:var(--s-3) var(--s-4);border-top:var(--hair) solid var(--line)}
.hxe-case:first-child{border-top:0}
.hxe-ch{font-size:var(--fs-sm);overflow-wrap:anywhere}
.hxe-cr{display:flex;gap:var(--s-2);align-items:center;flex-wrap:wrap;margin-top:var(--s-2)}
.hxe-cr .sp{flex:1}
.hxe-res{display:inline-flex;align-items:center;gap:4px;height:22px;padding:0 6px;border-radius:var(--r-xs);border:var(--hair) solid var(--line-2);font-size:var(--fs-xs)}
.hxe-res i{width:8px;height:8px;border-radius:2px;background:var(--crit)}
.hxe-res.ok i{background:var(--good)}
.hxe-ses,.hxe-run{display:flex;gap:var(--s-3);align-items:center;padding:var(--s-2) var(--s-4);border-top:var(--hair) solid var(--line);color:inherit;text-decoration:none;min-height:var(--touch);font-size:var(--fs-sm)}
.hxe-ses:first-child,.hxe-run:first-child{border-top:0}
.hxe-ses:hover,.hxe-run:hover{background:var(--surface-3)}
.hxe-sn{flex:1;min-width:0;overflow:hidden;text-overflow:ellipsis;white-space:nowrap}
.hxe-sm{flex:none;font-size:var(--fs-xs)}
.hxe-cmp{display:flex;flex-direction:column;gap:var(--s-3)}
.hxe-cmp .hx-note{margin:0}
.hxe-cmp select{width:100%}
@media (max-width:640px){.hxe-ses,.hxe-run{flex-wrap:wrap}.hxe-sn{flex-basis:100%;white-space:normal}}
`);
