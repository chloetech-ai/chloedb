// A persona's run, live: where in its journey it is, what it did as compact
// tasks, and a box to talk to it. When it ends: "Found 4 problems (2 new)",
// linking to Problems filtered to that run.
//   GET /api/converge/runs/:id -> { run, persona, live, now, summary? }
import { h, put, toast, toMs, usd, firstName, enc } from "/goals/converge/util.js";
import { sheetShell } from "/goals/converge/kit.js";
import { talk } from "/goals/converge/talk.js";
import { taskList, taskStyles } from "/goals/converge/tasks.js";

export function openRun(C, id) {
  taskStyles();
  let timer = null, tl = null, tk = null, stick = true, lastSig = "";
  const sh = sheetShell({ label: "A persona's run", data: { run: id }, onClose: (quiet) => { clearInterval(timer); tk && tk.stop(); if (!quiet) history.replaceState(history.state, "", C.here()); } });
  const openMap = new Map();
  const note = h("div", {});
  const tail = h("div", { style: { display: "flex", flexDirection: "column", gap: "16px" } });
  const tasksBox = h("div", {});
  const acts = h("div", { class: "acts" });
  sh.body.addEventListener("scroll", () => { stick = sh.body.scrollTop + sh.body.clientHeight >= sh.body.scrollHeight - 40; }, { passive: true });
  put(sh.body, h("p", { class: "cv-empty" }, "Joining the run…"));

  const paint = async () => {
    let d;
    try { d = await C.api(`/api/converge/runs/${enc(id)}`); } catch (err) { put(sh.body, h("p", { class: "cv-empty" }, err.message || String(err))); clearInterval(timer); return; }
    if (!sh.sheet.isConnected) { clearInterval(timer); return; }
    const r = d.run || {}, pe = d.persona || {};
    const gen = r.purpose === "generate" || (!d.persona && !r.persona);
    const name = gen ? "Writing personas from the code" : pe.name ? firstName(pe.name) : "A persona";
    const c0 = toMs(r.created), e0 = toMs(r.ended) || toMs(d.now) || Date.now();
    const secs = c0 ? Math.max(0, Math.round((e0 - c0) / 1000)) : null;
    const dur = secs == null ? "" : secs >= 60 ? `${Math.floor(secs / 60)} min` : `${secs} s`;
    const at = r.at && typeof r.at === "object" ? r.at : null;
    const pct = !d.live ? 100 : at && at.of ? Math.min(100, Math.round(((at.n - (d.live ? 0.5 : 0)) / at.of) * 100)) : d.live ? 5 : 100;
    sh.sheet.setAttribute("aria-label", name);
    sh.setHead(
      h("span", { class: "cv-meta" }, d.live ? h("span", { class: "cv-tag run" }, h("i", { class: "cv-pulse", "aria-hidden": "true" }), gen ? "reading now" : "playing now") : h("span", { class: `cv-tag ${r.state === "failed" ? "bad" : "ok"}` }, r.state === "failed" ? "failed" : "finished"),
        h("span", {}, dur), r.cost_usd ? h("span", {}, usd(r.cost_usd)) : null, r.ease != null ? h("span", {}, `ease ${r.ease} of 5`) : null, r.use_memory ? h("span", { class: "cv-tag" }, "with memory") : null),
      h("h2", {}, gen ? name : `${name}${r.purpose === "verify" ? " checks a fix" : " plays their journey"}`),
      h("span", { class: "cv-st" }, h("span", { class: "sg" }, at ? `Step ${at.n} of ${at.of}` : d.live ? "Starting" : "Done"), at && at.name ? h("span", { class: "wh" }, at.name) : null),
      h("span", { class: "cv-thin" }, h("i", { style: { width: `${pct}%` } })));
    if (!tl) {
      tl = taskList(openMap, gen ? "C" : (pe.name || "?").trim()[0] || "?");
      put(tasksBox, tl.el);
      if (!gen) {
        tk = talk({ api: C.api, base: `/api/converge/runs/${enc(id)}`, placeholder: `Ask ${name} something`, intro: `${name} answers between steps, in the first person.`, agentWord: name, missingText: `Talking to ${name} during a run is not available on this server yet.` });
        put(sh.foot, acts, tk.box);
      } else put(sh.foot, acts);
      put(sh.body, !gen && pe.journey ? h("details", { class: "cv-fold" }, h("summary", {}, "Their journey"), h("p", { style: { color: "var(--text-2)" } }, pe.journey)) : null, note, tasksBox, tail, tk ? tk.section : null);
    }
    const steps = Array.isArray(r.steps) ? r.steps : [];
    tl.paint(steps, { live: d.live, failed: r.state === "failed", ended: toMs(r.ended), now: toMs(d.now) || Date.now() });
    put(note, steps.length ? null : h("p", { class: "cv-empty" }, d.live ? (gen ? "Opening the project…" : `${name} is opening a fresh checkout…`) : "This run kept no steps."));
    // The end: what it found, as a link to Problems filtered to this run.
    const sum = d.summary && typeof d.summary === "object" ? d.summary : null;
    if (sum && Array.isArray(sum.findings)) (C.S.runSum = C.S.runSum || new Map()).set(id, sum.findings.map((x) => x && x.id).filter(Boolean));
    const found = sum ? Number(sum.found) || 0 : Array.isArray(r.findings) ? r.findings.length : 0;
    const fresh = sum && sum.new != null ? Number(sum.new) : null;
    const sig = JSON.stringify([d.live, sum, r.answer, r.error, found]);
    if (sig !== lastSig) {
      lastSig = sig;
      const made = gen ? (r.findings || []).filter((x) => x && (x.persona || x.name)) : [];
      put(tail,
        !d.live && !gen ? h("div", { class: `cv-verdict${found ? "" : " wait"}` }, found ? `Found ${found} problem${found > 1 ? "s" : ""}${fresh != null ? ` (${fresh} new)` : ""}` : "Found nothing new this time") : null,
        !d.live && r.answer && !gen ? h("section", {}, h("span", { class: "cv-label" }, "What they achieved"), h("p", {}, r.answer)) : null,
        sum && Array.isArray(sum.findings) && sum.findings.length ? h("ul", { class: "cv-checks" }, sum.findings.map((x) => h("li", { style: { cursor: "pointer" }, onclick: () => { location.hash = C.href("problems", { f: x.id }); } }, h("span", { class: "cv-sev", "data-sev": "2", style: { marginTop: "6px" } }), h("span", {}, x.title, x.new ? h("span", { class: "cv-tag run", style: { marginLeft: "6px" } }, "new") : x.dup_of ? h("span", { class: "cv-tag", style: { marginLeft: "6px" } }, "known") : null), h("span", { class: "du" }, "›")))) : null,
        made.length ? h("section", {}, h("span", { class: "cv-label" }, `Personas written (${made.length}), on probation`), h("ul", {}, made.map((x) => h("li", {}, x.name || x.persona)))) : null,
        r.error ? h("p", { class: "cv-note" }, r.error) : null);
      put(acts, !d.live && !gen && found ? h("a", { class: "cv-btn pri", href: C.href("problems", { from: id }) }, fresh != null ? `See the ${fresh || found} problem${(fresh || found) > 1 ? "s" : ""}` : "See the problems")
        : !d.live && gen && made.length ? h("a", { class: "cv-btn pri", href: C.href("personas") }, "See them") : null);
    }
    if (!d.live) { clearInterval(timer); if (gen) { C.S.sig = ""; C.load(); } }
    if (tk) tk.refresh();
    if (stick && d.live) sh.body.scrollTop = sh.body.scrollHeight;
  };
  timer = setInterval(paint, 1500);
  paint().catch((err) => toast(err.message || String(err)));
}
