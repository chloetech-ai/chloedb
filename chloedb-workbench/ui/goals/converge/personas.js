// Run a persona: the personas ranked (the Scores table: who finds the most,
// whose journey is hardest), each with Play and its memory switch. Playing
// opens the run's live sheet; a run that ends links to what it found.
import { h, toast, enc, firstName } from "/goals/converge/util.js";
import { useStyle } from "/levels/common.js";
import { stepHead, none } from "/goals/converge/rows.js";

const CSS = `
.cv-sc { list-style: none; margin: 0; padding: 0; border-top: var(--hair) solid var(--line); }
.cv-sc-h, .cv-sc-r { display: grid; grid-template-columns: 28px minmax(0, 1.6fr) 120px 56px 70px 56px 150px 112px; gap: 12px; align-items: center; }
.cv-sc-h { font-family: var(--mono); font-size: var(--fs-xs); letter-spacing: var(--track-label); text-transform: uppercase; color: var(--muted); padding: 8px 0; border-bottom: var(--hair) solid var(--line); }
.cv-sc-h .n, .cv-sc-r .n { text-align: right; }
.cv-sc-r { min-height: 64px; padding: 8px 0; border-bottom: var(--hair) solid var(--line); }
.cv-sc-r .rk { font-family: var(--mono); color: var(--muted); text-align: center; }
.cv-sc-r .who { display: flex; flex-direction: column; min-width: 0; }
.cv-sc-r .who b { font-weight: var(--w-title); overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
.cv-sc-r .who span { font-size: var(--fs-xs); color: var(--text-2); overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
.cv-sc-r .n { font-family: var(--mono); font-variant-numeric: tabular-nums; }
.cv-sc-r .ez { display: flex; align-items: center; gap: 8px; }
.cv-sc-r .ez svg { flex: 1; height: 24px; min-width: 0; }
.cv-sc-r .ez .n { width: 28px; }
.cv-sc-r .lbl-m { display: none; }
.cv-sc-r .go { display: flex; justify-content: flex-end; }
.cv-sc-r .go .cv-btn { width: 100%; }
.cv-sc-r.live { background: var(--accent-soft); }
.cv-mem label { position: relative; display: inline-flex; align-items: center; gap: 8px; min-height: 44px; cursor: pointer; font-size: var(--fs-sm); color: var(--text-2); }
.cv-mem input { position: absolute; opacity: 0; width: 1px; height: 1px; }
.cv-mem .sw { width: 38px; height: 22px; border-radius: 11px; background: var(--surface-3); border: var(--hair) solid var(--line-2); position: relative; flex: none; transition: background .15s; }
.cv-mem .sw::after { content: ""; position: absolute; top: 2px; left: 2px; width: 16px; height: 16px; border-radius: 50%; background: var(--surface); box-shadow: var(--shadow-2); transition: transform .15s; }
.cv-mem input:checked + .sw { background: var(--accent); border-color: var(--accent); }
.cv-mem input:checked + .sw::after { transform: translateX(16px); }
.cv-mem input:focus-visible + .sw { outline: 2px solid var(--accent); outline-offset: 2px; }
.cv-gen { display: flex; gap: 8px; align-items: center; flex-wrap: wrap; }
.cv-gen input { width: 72px; }
@media (max-width: 900px) {
  .cv-sc-h { display: none; }
  .cv-sc-r { grid-template-columns: 24px minmax(0, 1fr) auto; grid-template-areas: "rk who who" ". st st" ". mem go"; gap: 2px 10px; padding: 12px 0 8px; }
  .cv-sc-r .rk { grid-area: rk; align-self: start; padding-top: 2px; }
  .cv-sc-r .who { grid-area: who; }
  .cv-sc-r .go { grid-area: go; }
  .cv-sc-r .go .cv-btn { width: auto; }
  .cv-sc-r .ez, .cv-sc-r .c-runs, .cv-sc-r .c-rr, .cv-sc-r .c-open { display: none; }
  .cv-sc-r .lbl-m { display: block; grid-area: st; font-size: var(--fs-sm); color: var(--text-2); }
  .cv-sc-r .cv-mem { grid-area: mem; }
}
`;

function spark(vals) {
  const v = (vals || []).filter((x) => Number.isFinite(x));
  const svg = document.createElementNS("http://www.w3.org/2000/svg", "svg");
  svg.setAttribute("viewBox", "0 0 100 24"); svg.setAttribute("preserveAspectRatio", "none"); svg.setAttribute("aria-hidden", "true");
  if (v.length < 2) return svg;
  const Y = (x) => 22 - ((x - 1) / 4) * 20, X = (i) => (i / (v.length - 1)) * 100;
  svg.innerHTML = `<polyline points="${v.map((x, i) => `${X(i)},${Y(x)}`).join(" ")}" fill="none" style="stroke:var(--accent)" stroke-width="1.5" vector-effect="non-scaling-stroke"/>`;
  return svg;
}

/** "Use past memory": the persona keeps its own ChloeCtx store between visits. */
function memSwitch(C, p) {
  const id = `cv-mem-${p.id}`;
  const inp = h("input", { type: "checkbox", id, role: "switch", checked: p.use_memory ? true : null, "aria-label": `${firstName(p.name)} uses past memory` });
  inp.addEventListener("change", async () => {
    const v = inp.checked;
    const body = { use_memory: v };
    try {
      try { await C.api(`/api/converge/personas/${enc(p.id)}`, { method: "PUT", body }); }
      catch (err) { if (err.status === 404 || err.status === 405) await C.api(`/api/converge/personas/${enc(p.id)}/update`, { method: "POST", body }); else throw err; }
      toast(v ? `${firstName(p.name)} remembers earlier visits from now on` : `${firstName(p.name)} comes in fresh from now on`);
    } catch (err) { inp.checked = !v; toast(err.status === 404 || err.status === 405 ? "This server cannot keep that setting yet" : err.message || String(err)); }
    C.S.sig = ""; await C.load();
  });
  return h("div", { class: "cv-mem" }, h("label", { for: id, title: "Remembers its own earlier visits (its own ChloeCtx store, separate from yours). Off: comes in fresh like a first-time user." }, inp, h("span", { class: "sw", "aria-hidden": "true" }), "Past memory"));
}

async function play(C, p) {
  try {
    const r = await C.api(`/api/converge/personas/${enc(p.id)}/play`, { method: "POST", body: {} });
    const run = typeof r?.run === "string" ? r.run : r?.run?.id;
    toast(`${firstName(p.name)} is playing${p.use_memory ? ", with their memory" : ""}${r?.over_limit ? `. ${r.over_limit}` : ""}`);
    C.S.sig = ""; await C.load();
    if (run) { C.openRun(run); return; }
    // The run starts a moment later: open it once the persona names it.
    for (let i = 0; i < 12 && C.S.alive; i++) {
      await new Promise((ok) => setTimeout(ok, 800));
      C.S.sig = ""; await C.load();
      const q = (C.S.d.personas || []).find((x) => x.id === p.id);
      if (q && q.live_run) { C.openRun(q.live_run); return; }
    }
  } catch (err) { toast(err.message || String(err)); }
}

// What the ranking sorts by (the persona Scores); kept in the address as ?sort=.
const SORTS = [
  { k: "open", label: "Open problems", v: (p) => p.open },
  { k: "found", label: "Problems found", v: (p) => p.found ?? p.reported },
  { k: "reproduced", label: "Reproduced", v: (p) => p.reproduced },
  { k: "fixed", label: "Fixed", v: (p) => p.fixed },
  { k: "blockers", label: "Blockers", v: (p) => p.blockers },
  { k: "ease", label: "Ease (last run)", v: (p) => p.ease },
  { k: "ease_avg", label: "Ease (average)", v: (p) => p.ease_avg },
  { k: "ease_trend", label: "Ease trend", v: (p) => p.ease_trend },
  { k: "runs", label: "Runs", v: (p) => p.runs },
  { k: "cost_usd", label: "Cost", v: (p) => p.cost_usd },
];
function sortKey() {
  const q = new URLSearchParams(location.hash.split("?")[1] || "");
  const v = q.get("sort");
  return v && SORTS.some((c) => c.k === v.replace(/^-/, "")) ? v : "-open";
}
function setSort(C, v) {
  const [path, qs] = location.hash.split("?");
  const q = new URLSearchParams(qs || "");
  if (v === "-open") q.delete("sort"); else q.set("sort", v);
  history.replaceState(history.state, "", `${path}?${q}`);
  C.S.sig = ""; C.load();
}

export function personasScreen(C) {
  useStyle("converge-personas", CSS);
  const d = C.S.d;
  const ps = d.personas || [];
  const sort = sortKey();
  const col = SORTS.find((c) => c.k === sort.replace(/^-/, "")) || SORTS[0], desc = sort.startsWith("-");
  const rank = (a, b) => {
    if (!a.runs !== !b.runs) return a.runs ? -1 : 1;
    const x = col.v(a), y = col.v(b);
    if (x == null && y != null) return 1;
    if (y == null && x != null) return -1;
    return (x == null ? 0 : desc ? y - x : x - y) || (b.open || 0) - (a.open || 0) || (a.ease ?? 9) - (b.ease ?? 9);
  };
  const sel = h("select", { class: "cv-in", "aria-label": "Rank by", style: { minHeight: "44px", marginBottom: "12px" }, onchange: (e) => setSort(C, e.target.value) },
    SORTS.flatMap((c) => [["-" + c.k, `${c.label}, highest first`], [c.k, `${c.label}, lowest first`]]).map(([v, l]) => h("option", { value: v, selected: v === sort ? true : null }, l)));
  const row = (p, i) => {
    const rr = p.repro_rate == null ? "–" : `${Math.round(p.repro_rate * 100)}%`;
    const last = (d.runs || []).find((r) => r.persona === p.id && r.state !== "running");
    return h("li", { class: `cv-sc-r${p.live_run ? " live" : ""}` },
      h("span", { class: "rk" }, String(i + 1)),
      h("span", { class: "who" }, h("b", {}, p.name), h("span", {}, p.role || "")),
      h("span", { class: "ez" }, spark(p.ease_hist), h("span", { class: "n" }, p.ease == null ? "–" : Number(p.ease).toFixed(1))),
      h("span", { class: "n c-runs", title: `${p.runs || 0} runs` }, String(p.found ?? p.reported ?? 0)),
      h("span", { class: "n c-rr", title: `${p.reproduced ?? "–"} reproduced` }, rr),
      h("span", { class: "n c-open", title: p.cost_usd == null ? "" : `cost $${Number(p.cost_usd).toFixed(2)}` }, p.fixed == null ? "–" : String(p.fixed)),
      h("span", { class: "lbl-m" }, [`ease ${p.ease == null ? "–" : Number(p.ease).toFixed(1)}${p.ease_avg == null ? "" : ` (avg ${Number(p.ease_avg).toFixed(1)})`}`, `${p.found ?? 0} found`, p.repro_rate == null ? null : `${rr} reproduced`, p.fixed == null ? null : `${p.fixed} fixed`, `${p.runs || 0} run${p.runs === 1 ? "" : "s"}`, p.cost_usd == null ? null : `$${Number(p.cost_usd).toFixed(2)}`].filter(Boolean).join(" · "), last ? [" · ", h("a", { href: C.here({ run: last.id }) }, "last run")] : null),
      memSwitch(C, p),
      h("span", { class: "go" }, p.live_run
        ? h("button", { class: "cv-btn", type: "button", onclick: () => C.openRun(p.live_run) }, h("i", { class: "cv-pulse", style: { color: "var(--accent)" } }), "Watch")
        : h("button", { class: `cv-btn${i === 0 ? " pri" : ""}`, type: "button", disabled: p.busy ? true : null, onclick: () => play(C, p) }, p.busy ? String(p.busy) : "Play")));
  };
  const table = (list) => h("div", {}, h("div", { class: "cv-sc-h", "aria-hidden": "true" }, h("span", {}, "#"), h("span", {}, "Persona"), h("span", {}, "Ease"), h("span", { class: "n" }, "Found"), h("span", { class: "n" }, "Repro"), h("span", { class: "n" }, "Fixed"), h("span", {}, "Memory"), h("span", {})),
    h("ol", { class: "cv-sc" }, list.map(row)));
  const active = ps.filter((p) => p.state === "active").sort(rank);
  const prob = ps.filter((p) => p.state === "probation").sort(rank);
  const retired = ps.filter((p) => p.state === "retired");
  const gens = (d.runs || []).filter((r) => r.purpose === "generate" && r.state === "running");

  const n = h("input", { type: "number", min: "1", max: "6", value: "3", class: "cv-in", "aria-label": "How many personas" });
  const gen = h("button", { class: "cv-btn", type: "button", onclick: async () => {
    gen.disabled = true;
    try {
      const count = Math.max(1, Math.min(6, Number(n.value) || 3));
      const r = await C.api("/api/converge/personas/generate", { method: "POST", body: { repo: C.repo(), from: "codebase", count } });
      const id = typeof r.run === "string" ? r.run : r.run && r.run.id;
      C.S.sig = ""; await C.load();
      if (id) C.openRun(id); else toast("A new persona is on probation");
    } catch (err) { toast(err.message || String(err)); }
    gen.disabled = false;
  } }, "Write personas from the code");
  const name = h("input", { type: "text", id: "cv-pn", placeholder: "Omar, data engineer" });
  const role = h("input", { type: "text", id: "cv-pr", placeholder: "Who they are and what they want" });
  const journey = h("textarea", { class: "cv-ta", id: "cv-pj", placeholder: "The task they try, end to end", style: { fontFamily: "var(--sans)", fontSize: "16px", minHeight: "80px" } });

  return h("div", {},
    stepHead(C, "Run a persona", ps.length || null, "Each persona plays its journey on a fresh checkout and reports what it hit. Ranked by what you pick below, open problems first by default."),
    gens.map((r) => h("button", { class: "cv-btn", type: "button", style: { width: "100%", marginBottom: "12px" }, onclick: () => C.openRun(r.id) }, h("i", { class: "cv-pulse", style: { color: "var(--accent)" } }), "Writing personas from the code: watch")),
    ps.length > 1 ? sel : null,
    active.length ? table(active) : none("No active personas yet. Write some from the code, or add one by hand below.", h("button", { class: "cv-btn pri", type: "button", onclick: () => gen.click() }, "Write personas from the code")),
    prob.length ? h("details", { class: "cv-fold", open: true, style: { marginTop: "16px" } }, h("summary", {}, "On probation", h("span", { class: "s" }, `${prob.length} · join once a problem of theirs reproduces`)), table(prob)) : null,
    retired.length ? h("details", { class: "cv-fold", style: { marginTop: "8px" } }, h("summary", {}, "Retired", h("span", { class: "s" }, String(retired.length))),
      h("ul", { class: "cv-checks" }, retired.map((p) => h("li", {}, h("span"), h("span", {}, p.name), h("button", { class: "cv-btn quiet", type: "button", onclick: () => C.act(() => C.api(`/api/converge/personas/${enc(p.id)}/activate`, { method: "POST", body: {} }), "Active again") }, "Bring back"))))) : null,
    h("details", { class: "cv-fold", style: { marginTop: "16px" } }, h("summary", {}, "More personas", h("span", { class: "s" }, "from the code, or by hand")),
      h("div", { class: "cv-gen", style: { marginBottom: "16px" } }, h("label", { class: "cv-label" }, "How many"), n, gen),
      h("div", { class: "cv-f" }, h("label", { for: "cv-pn" }, "Name"), name), h("div", { class: "cv-f" }, h("label", { for: "cv-pr" }, "Role"), role), h("div", { class: "cv-f" }, h("label", { for: "cv-pj" }, "Journey"), journey),
      h("button", { class: "cv-btn", type: "button", onclick: () => C.act(() => C.api("/api/converge/personas", { method: "POST", body: { repo: C.repo(), name: name.value, role: role.value, journey: journey.value } }), "Persona added") }, "Add the persona")));
}
