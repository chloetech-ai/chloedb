// Personas: who plays your product. On top, three ways to make one (written
// from the code, described by you, copied from another project); then a card
// each, ranked by how hard they find it (lowest ease first): live progress
// while playing (watch live), ease, runs, found, reproduced, what probation
// means for it, its journey and past runs folded, and Play now / Stop / Edit
// / Retire. A run opens in its own sheet (?run=).
import { h, toast, enc, firstName, toMs, usd } from "/goals/converge/util.js";
import { useStyle } from "/levels/common.js";

const CSS = `
.pe-head { display: flex; flex-direction: column; gap: 4px; margin-bottom: 16px; }
.pe-head a { font-size: var(--fs-sm); color: var(--accent-text); text-decoration: none; min-height: 32px; display: inline-flex; align-items: center; align-self: flex-start; }
.pe-head h1 { margin: 0; font-size: var(--fs-xl, 24px); line-height: 1.25; font-weight: var(--w-title); }
.pe-k { font-family: var(--mono); font-size: 11px; letter-spacing: .06em; text-transform: uppercase; color: var(--muted); font-weight: var(--w-mid); }
.pe-card { background: var(--surface); border: var(--hair) solid var(--line); border-radius: 10px; padding: 14px; min-width: 0; }
.pe-make { display: grid; grid-template-columns: repeat(3, minmax(0, 1fr)); gap: 16px; margin-bottom: 18px; }
.pe-way { display: flex; flex-direction: column; gap: 8px; min-width: 0; }
.pe-way + .pe-way { border-left: var(--hair) solid var(--line); padding-left: 16px; }
.pe-way p { margin: 0; font-size: var(--fs-sm); color: var(--text); }
.pe-way .row { display: flex; gap: 8px; align-items: center; flex-wrap: wrap; }
.pe-way select, .pe-way input, .pe-way textarea { font: inherit; font-size: 16px; border: var(--hair) solid var(--line-2); border-radius: 8px; background: var(--surface); color: var(--text); padding: 6px 10px; min-height: 40px; box-sizing: border-box; max-width: 100%; }
.pe-way textarea { min-height: 64px; resize: vertical; width: 100%; }
.pe-way input[type=text] { width: 100%; }
.pe-way .cv-btn { align-self: flex-start; }
.pe-way .row select { width: auto; flex: 0 1 auto; }
.pe-copy { display: flex; flex-direction: column; gap: 4px; max-height: 220px; overflow: auto; }
.pe-copy label { display: flex; gap: 8px; align-items: flex-start; font-size: var(--fs-sm); min-height: 32px; }
.pe-copy label span small { display: block; color: var(--text-2); font-size: var(--fs-xs); }
.pe-sm { font-size: var(--fs-xs); color: var(--text-2); }
.pe-bar { display: flex; align-items: center; gap: 8px; margin-bottom: 10px; flex-wrap: wrap; }
.pe-grid { display: grid; grid-template-columns: repeat(3, minmax(0, 1fr)); gap: 16px; align-items: start; }
.pe-p { display: flex; flex-direction: column; gap: 10px; }
.pe-p.live { border-color: var(--accent-line); }
.pe-p .pe-top { display: flex; justify-content: space-between; gap: 8px; align-items: flex-start; }
.pe-p .pe-top b { font-size: var(--fs-md); line-height: 1.3; overflow-wrap: anywhere; }
.pe-tag { font-size: 11px; padding: 2px 7px; border-radius: 4px; background: var(--surface-3); color: var(--text-2); white-space: nowrap; flex: none; }
.pe-tag.run { background: var(--accent-soft); color: var(--accent-text); } .pe-tag.ok { background: var(--good-soft); color: var(--good-ink); } .pe-tag.prob { background: var(--warn-soft); color: var(--warn-ink); }
.pe-role { font-size: var(--fs-sm); color: var(--text-2); display: -webkit-box; -webkit-line-clamp: 3; -webkit-box-orient: vertical; overflow: hidden; margin: 0; }
.pe-seg { display: flex; gap: 3px; } .pe-seg i { flex: 1; height: 6px; border-radius: 2px; background: var(--surface-3); }
.pe-seg i.d { background: var(--accent); } .pe-seg i.n { background: color-mix(in srgb, var(--accent) 50%, var(--surface)); animation: cv-pulse 1.4s ease-in-out infinite; }
.pe-now { font-size: var(--fs-xs); color: var(--accent-text); overflow-wrap: anywhere; }
.pe-now a { color: var(--accent-text); font-weight: var(--w-mid); }
.pe-stats { display: grid; grid-template-columns: repeat(4, minmax(0, 1fr)); gap: 8px; }
.pe-stats b { display: block; font-size: 20px; font-weight: var(--w-title); font-variant-numeric: tabular-nums; }
.pe-stats span { font-size: var(--fs-xs); color: var(--text-2); }
.pe-fold { border-top: var(--hair) solid var(--line); padding-top: 8px; display: flex; flex-direction: column; gap: 4px; }
.pe-fold > button { all: unset; cursor: pointer; font-size: var(--fs-xs); color: var(--text-2); min-height: 28px; display: inline-flex; align-items: center; }
.pe-fold > button:focus-visible { outline: 2px solid var(--accent); border-radius: 4px; }
.pe-fold .body { font-size: var(--fs-xs); color: var(--text-2); display: flex; flex-direction: column; gap: 4px; }
.pe-fold .body a { color: var(--text); text-decoration: none; } .pe-fold .body a:hover { text-decoration: underline; }
.pe-acts { display: flex; gap: 8px; flex-wrap: wrap; align-items: center; }
.pe-acts .cv-btn { min-height: 36px; }
.pe-edit { display: flex; flex-direction: column; gap: 6px; }
.pe-edit label { font-size: var(--fs-xs); color: var(--text-2); display: flex; flex-direction: column; gap: 3px; }
.pe-edit input, .pe-edit textarea { font: inherit; font-size: 16px; border: var(--hair) solid var(--line-2); border-radius: 8px; padding: 6px 10px; background: var(--surface); color: var(--text); }
.pe-edit textarea { min-height: 90px; }
.pe-mem { display: inline-flex; align-items: center; gap: 6px; font-size: var(--fs-xs); color: var(--text-2); min-height: 36px; cursor: pointer; margin-left: auto; }
.pe-retired { margin-top: 18px; }
@media (max-width: 1100px) { .pe-grid { grid-template-columns: repeat(2, minmax(0, 1fr)); } }
@media (max-width: 760px) {
  .pe-make { grid-template-columns: minmax(0, 1fr); }
  .pe-way + .pe-way { border-left: 0; padding-left: 0; border-top: var(--hair) solid var(--line); padding-top: 14px; }
  .pe-grid { grid-template-columns: minmax(0, 1fr); }
}
@media (max-width: 640px) {
  .pe-head h1 { font-size: var(--fs-lg); }
  .pe-acts .cv-btn, .pe-way .cv-btn { min-height: 44px; }
  .pe-fold > button, .pe-mem, .pe-head a, .pe-copy label { min-height: 44px; }
  .pe-copy label { align-items: center; }
}
`;

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

async function stop(C, p) {
  if (!confirm(`Stop ${firstName(p.name)}'s run? What it found so far is kept.`)) return;
  try {
    await C.api(`/api/converge/personas/${enc(p.id)}/stop`, { method: "POST", body: {} });
    toast(`${firstName(p.name)} stopped`);
  } catch (err) { toast(err.status === 404 ? "This server cannot stop one run: use Stop everything" : err.message || String(err)); }
  C.S.sig = ""; await C.load();
}

async function edit(C, p, body, msg) {
  try {
    try { await C.api(`/api/converge/personas/${enc(p.id)}`, { method: "PUT", body }); }
    catch (err) { if (err.status === 404 || err.status === 405) await C.api(`/api/converge/personas/${enc(p.id)}/update`, { method: "POST", body }); else throw err; }
    toast(msg);
    return true;
  } catch (err) { toast(err.message || String(err)); return false; }
  finally { C.S.sig = ""; await C.load(); }
}

/** "Name, role" → [name, role]. */
const split = (n) => { const s = String(n || ""); const i = s.indexOf(","); return i < 0 ? [s, ""] : [s.slice(0, i).trim(), s.slice(i + 1).trim()]; };
const clock = (t) => { const ms = toMs(t); if (!ms) return ""; const d = new Date(ms); const today = new Date().toDateString() === d.toDateString(); return today ? d.toLocaleTimeString([], { hour: "2-digit", minute: "2-digit", hour12: false }) : d.toLocaleDateString([], { month: "short", day: "numeric" }); };

/** The three ways to make personas. */
function makePanel(C) {
  const S = C.S;
  const busy = (C.S.d.runs || []).find((r) => r.purpose === "generate" && r.state === "running");
  const n = h("select", { "aria-label": "How many personas" }, [1, 2, 3, 4, 5, 6].map((k) => h("option", { value: String(k), selected: k === (S.genN || 3) ? true : null }, `how many: ${k}`)));
  n.addEventListener("change", () => { S.genN = Number(n.value); gen.textContent = `Write ${S.genN} persona${S.genN > 1 ? "s" : ""}`; });
  const gen = h("button", { class: "cv-btn pri", type: "button", onclick: async () => {
    if (busy) { C.openRun(busy.id); return; }
    gen.disabled = true;
    try {
      const count = Number(n.value) || 3;
      const r = await C.api("/api/converge/personas/generate", { method: "POST", body: { repo: C.repo(), from: "codebase", count } });
      const id = typeof r.run === "string" ? r.run : r.run && r.run.id;
      C.S.sig = ""; await C.load();
      if (id) C.openRun(id); else toast("A new persona is on probation");
    } catch (err) { toast(err.message || String(err)); }
    gen.disabled = false;
  } }, busy ? "Writing now: watch" : `Write ${S.genN || 3} persona${(S.genN || 3) > 1 ? "s" : ""}`);

  // Describe one yourself: a name, and who they are and what they try.
  const dr = (S.peDraft = S.peDraft || { name: "", text: "" });
  const name = h("input", { type: "text", placeholder: "Name and role: Omar, high-school teacher", "aria-label": "The persona's name and role", value: dr.name });
  const text = h("textarea", { placeholder: "A teacher who pastes student essays and wants reading time per paragraph…", "aria-label": "Who they are and what they try, end to end" }, dr.text);
  name.addEventListener("input", () => { dr.name = name.value; });
  text.addEventListener("input", () => { dr.text = text.value; });
  const add = h("button", { class: "cv-btn", type: "button", onclick: async () => {
    const [nm, role] = split(name.value.trim());
    if (!nm) { name.focus(); toast("A name first"); return; }
    if (!text.value.trim()) { text.focus(); toast("Say who they are and what they try"); return; }
    add.disabled = true;
    try {
      await C.api("/api/converge/personas", { method: "POST", body: { repo: C.repo(), name: name.value.trim(), role: role || "", journey: text.value.trim() } });
      S.peDraft = null; toast(`${nm} added`);
    } catch (err) { toast(err.message || String(err)); }
    add.disabled = false;
    C.S.sig = ""; await C.load();
  } }, "Add persona");

  return h("section", { class: "pe-card pe-make", "aria-label": "Make personas" },
    h("div", { class: "pe-way" }, h("span", { class: "pe-k" }, "1 · From the code"), h("p", {}, "Reads the README, the docs and the code, and writes personas aimed at the riskiest parts. About a minute, a few cents."), h("div", { class: "row" }, gen, busy ? null : n)),
    h("div", { class: "pe-way" }, h("span", { class: "pe-k" }, "2 · Describe one yourself"), name, text, add),
    copyWay(C));
}

/** Copy from another project: its personas, picked, made here with their journeys (not their history). */
function copyWay(C) {
  const S = C.S;
  const others = C.projects().filter((p) => p.path !== C.repo());
  if (!others.length) return h("div", { class: "pe-way" }, h("span", { class: "pe-k" }, "3 · Copy from another project"), h("p", {}, "Open another project first: its personas can then be copied here."));
  const cp = S.peCopy || (S.peCopy = { from: "", list: null, pick: new Set(), busy: false });
  const sel = h("select", { "aria-label": "The project to copy from" }, h("option", { value: "" }, "Pick a project"), others.map((p) => h("option", { value: p.path, selected: p.path === cp.from ? true : null }, p.project)));
  sel.addEventListener("change", async () => {
    cp.from = sel.value; cp.list = null; cp.pick = new Set();
    if (!cp.from) { C.draw(); return; }
    cp.busy = true; C.draw();
    try {
      const r = await C.api(`/api/converge/personas?repo=${enc(cp.from)}`);
      const here = new Set((C.S.d.personas || []).map((p) => p.name.toLowerCase()));
      cp.list = (r.personas || []).filter((p) => p.state !== "retired" && !here.has(String(p.name).toLowerCase()));
    } catch (err) { toast(err.message || String(err)); cp.list = []; }
    cp.busy = false; C.draw();
  });
  const go = h("button", { class: "cv-btn", type: "button", disabled: cp.pick.size ? null : true, onclick: async () => {
    go.disabled = true;
    let made = 0;
    for (const p of (cp.list || []).filter((x) => cp.pick.has(x.id))) {
      try { await C.api("/api/converge/personas", { method: "POST", body: { repo: C.repo(), name: p.name, role: p.role || "", traits: p.traits || [], journey: p.journey, surfaces: p.surfaces || [] } }); made++; }
      catch (err) { toast(`${firstName(p.name)}: ${err.message || err}`); }
    }
    if (made) toast(`Copied ${made} persona${made > 1 ? "s" : ""}: they keep their journeys`);
    S.peCopy = null; C.S.sig = ""; await C.load();
  } }, cp.pick.size ? `Copy ${cp.pick.size}` : "Copy");
  return h("div", { class: "pe-way" }, h("span", { class: "pe-k" }, "3 · Copy from another project"),
    h("p", {}, "Reuse personas that found real problems elsewhere: they keep their role and journey; their runs start fresh here."),
    sel,
    cp.busy ? h("span", { class: "pe-sm" }, "Reading its personas…") : null,
    cp.list ? (cp.list.length ? h("div", { class: "pe-copy" }, cp.list.map((p) => {
      const ck = h("input", { type: "checkbox", checked: cp.pick.has(p.id) ? true : null });
      ck.addEventListener("change", () => { if (ck.checked) cp.pick.add(p.id); else cp.pick.delete(p.id); go.disabled = !cp.pick.size; go.textContent = cp.pick.size ? `Copy ${cp.pick.size}` : "Copy"; });
      return h("label", {}, ck, h("span", {}, p.name, h("small", {}, [p.ease != null ? `ease ${Number(p.ease).toFixed(1)}` : null, `${p.found ?? 0} found`, p.reproduced ? `${p.reproduced} reproduced` : null].filter(Boolean).join(" · "))));
    })) : h("span", { class: "pe-sm" }, "None to copy: it has none, or this project has them all.")) : null,
    cp.list && cp.list.length ? go : null);
}

/** One persona's card. */
function personaCard(C, p) {
  const S = C.S;
  const d = C.S.d;
  const [nm, role0] = split(p.name);
  const live = !!p.live_run;
  const at = p.live_at && typeof p.live_at === "object" ? p.live_at : null;
  const runs = (d.runs || []).filter((r) => r.persona === p.id).sort((a, b) => (toMs(b.created) || 0) - (toMs(a.created) || 0));
  const last = runs.find((r) => r.state !== "running");
  const waiting = C.findings().filter((f) => f.state === "active" && f.stage === "found" && (f.persona === p.id || (f.personas_hit || []).includes(p.id))).length;
  const open = (S.peOpen = S.peOpen || new Set());
  const fold = (key, label, body) => {
    const on = open.has(`${p.id}:${key}`);
    return [h("button", { type: "button", "aria-expanded": String(on), onclick: () => { if (on) open.delete(`${p.id}:${key}`); else open.add(`${p.id}:${key}`); C.draw(); } }, `${on ? "▾" : "▸"} ${label}`), on ? h("div", { class: "body" }, body()) : null];
  };
  const tag = live ? h("span", { class: "pe-tag run" }, `● playing${at && at.of ? ` · step ${at.n}/${at.of}` : ""}`)
    : p.state === "probation" ? h("span", { class: "pe-tag prob" }, `on probation · ${p.runs || 0} run${p.runs === 1 ? "" : "s"}`)
      : p.state === "retired" ? h("span", { class: "pe-tag" }, "retired") : h("span", { class: "pe-tag ok" }, "active");
  const runLine = (r) => {
    const sum = r.purpose === "verify" ? `replay · ${r.fixed === true ? "fixed ✓" : r.fixed === false ? "not fixed" : r.outcome || r.state}` : r.purpose === "explore" ? `explore · ${Array.isArray(r.findings) ? r.findings.length : 0} problem${Array.isArray(r.findings) && r.findings.length === 1 ? "" : "s"}` : r.purpose;
    return h("a", { href: C.here({ run: r.id }) }, [clock(r.created), sum, r.ease != null ? `ease ${r.ease}` : null, r.state === "failed" ? "failed" : null, r.state === "running" ? "now" : null].filter(Boolean).join(" · "));
  };
  const probation = p.state === "probation"
    ? h("span", { class: "pe-sm" }, `Plays like the others, and joins once one of its problems reproduces${waiting ? ` · ${waiting} wait${waiting === 1 ? "s" : ""} for your pick` : p.found ? "" : " · it has found nothing yet"}.`)
    : null;
  const memId = `pe-mem-${p.id}`;
  const mem = h("input", { type: "checkbox", id: memId, checked: p.use_memory ? true : null });
  mem.addEventListener("change", () => edit(C, p, { use_memory: mem.checked }, mem.checked ? `${nm} remembers earlier visits from now on` : `${nm} comes in fresh from now on`));
  const editing = S.peEdit === p.id;
  let editBox = null;
  if (editing) {
    const iname = h("input", { type: "text", value: p.name, "aria-label": "Name, role" });
    const irole = h("textarea", { "aria-label": "Who they are" }, p.role || "");
    const ij = h("textarea", { "aria-label": "Their journey" }, p.journey || "");
    editBox = h("div", { class: "pe-edit" },
      h("label", {}, "Name, role", iname), h("label", {}, "Who they are", irole), h("label", {}, "Their journey, end to end", ij),
      h("div", { class: "pe-acts" }, h("button", { class: "cv-btn", type: "button", onclick: () => { S.peEdit = null; C.draw(); } }, "Cancel"),
        h("button", { class: "cv-btn pri", type: "button", onclick: async () => { if (await edit(C, p, { name: iname.value.trim(), role: irole.value.trim(), journey: ij.value.trim() }, "Saved: the next run plays the new version")) { S.peEdit = null; C.draw(); } } }, "Save")));
  }
  const steps = Math.max(1, (at && at.of) || 0);
  return h("article", { class: `pe-card pe-p${live ? " live" : ""}`, "data-id": p.id },
    h("div", { class: "pe-top" }, h("b", {}, role0 ? `${nm} · ${role0}` : nm), tag),
    h("p", { class: "pe-role" }, p.role || p.journey || ""),
    live ? h("span", { class: "pe-seg", "aria-hidden": "true" }, Array.from({ length: steps }, (_, i) => h("i", { class: at && i < at.n - 1 ? "d" : at && i === at.n - 1 ? "n" : null }))) : null,
    live ? h("span", { class: "pe-now" }, at && at.name ? `Now: “${String(at.name).slice(0, 90)}” · ` : "Opening a fresh checkout · ", h("a", { href: C.here({ run: p.live_run }) }, "watch live"))
      : last ? h("span", { class: "pe-sm" }, `Last run ${clock(last.created)} · ${last.purpose === "verify" ? "replayed a fix" : "explored"}${last.ease != null ? ` · ease ${last.ease}` : ""}${last.cost_usd ? ` · ${usd(last.cost_usd)}` : ""}`) : h("span", { class: "pe-sm" }, "Has not played yet."),
    h("div", { class: "pe-stats" },
      h("div", {}, h("b", {}, p.ease == null ? "–" : Number(p.ease).toFixed(1)), h("span", {}, "ease")),
      h("div", {}, h("b", {}, String(p.runs || 0)), h("span", {}, "runs")),
      h("div", {}, h("b", {}, String(p.found ?? p.reported ?? 0)), h("span", {}, "found")),
      h("div", { title: "Reproduced of what it found" }, h("b", {}, (p.found ?? p.reported) ? `${p.reproduced || 0}/${p.found ?? p.reported}` : "–"), h("span", {}, "reproduced"))),
    probation,
    editBox,
    h("div", { class: "pe-fold" },
      fold("journey", "Journey", () => h("span", {}, p.journey || "No journey written.")),
      fold("runs", `Past runs (${runs.length})`, () => runs.length ? runs.slice(0, 12).map(runLine) : h("span", {}, "None yet."))),
    h("div", { class: "pe-acts" },
      p.state === "retired" ? h("button", { class: "cv-btn", type: "button", onclick: () => C.act(() => C.api(`/api/converge/personas/${enc(p.id)}/activate`, { method: "POST", body: {} }), `${nm} is back`) }, "Bring back")
        : live ? h("button", { class: "cv-btn", type: "button", onclick: () => stop(C, p) }, "Stop")
          : h("button", { class: "cv-btn pri", type: "button", disabled: p.busy ? true : null, onclick: () => play(C, p) }, p.busy ? String(p.busy) : "Play now"),
      p.state !== "retired" && !editing ? h("button", { class: "cv-btn", type: "button", onclick: () => { S.peEdit = p.id; C.draw(); } }, "Edit") : null,
      p.state !== "retired" && !live ? h("button", { class: "cv-btn quiet", type: "button", onclick: () => { if (confirm(`Retire ${nm}? It stops playing; you can bring it back.`)) C.act(() => C.api(`/api/converge/personas/${enc(p.id)}/retire`, { method: "POST", body: {} }), `${nm} retired`); } }, "Retire") : null,
      p.state !== "retired" ? h("label", { class: "pe-mem", for: memId, title: "Remembers its own earlier visits (its own ChloeCtx store, not yours). Off: comes in fresh like a first-time user." }, mem, "Past memory") : null));
}

export function personasScreen(C) {
  useStyle("converge-personas2", CSS);
  const d = C.S.d;
  const ps = d.personas || [];
  // Ranked by how hard they find it: the lowest ease first; who has not played, last.
  const rank = (a, b) => (b.live_run ? 1 : 0) - (a.live_run ? 1 : 0) || (a.ease == null ? 1 : 0) - (b.ease == null ? 1 : 0) || (a.ease ?? 9) - (b.ease ?? 9) || (b.found || 0) - (a.found || 0);
  const live = ps.filter((p) => p.state !== "retired").sort(rank);
  const retired = ps.filter((p) => p.state === "retired");
  const st = C.settings();
  return h("div", {},
    h("header", { class: "pe-head" }, h("a", { href: C.href("home") }, `‹ ${C.projName()}`), h("h1", {}, "Personas: who plays your product")),
    makePanel(C),
    h("div", { class: "pe-bar" }, h("span", { class: "pe-k" }, `${live.length} persona${live.length === 1 ? "" : "s"} · ranked by how hard they find it`), h("span", { class: "cv-sp" }),
      h("span", { class: "pe-sm" }, st.enabled ? `The autopilot replays each every ${st.replay_every_h || 6} h, and replays a fix before it merges` : "The autopilot is off: personas play when you press Play now")),
    live.length ? h("div", { class: "pe-grid" }, live.map((p) => personaCard(C, p))) : h("div", { class: "cv-none" }, h("p", {}, "No personas yet. Write some from the code above, describe one, or copy them from another project.")),
    retired.length ? h("section", { class: "pe-retired" }, h("div", { class: "pe-bar" }, h("span", { class: "pe-k" }, `Retired · ${retired.length}`)), h("div", { class: "pe-grid" }, retired.map((p) => personaCard(C, p)))) : null);
}
