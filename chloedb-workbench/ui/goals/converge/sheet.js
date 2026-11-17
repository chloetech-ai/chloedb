// A card's sheet: only what its step needs, its moves in a sticky foot, and
// always "Talk to the agent" at the bottom. It refreshes in place: what you
// typed, what you unfolded and where you scrolled stay.
//   Problems       what happened, who hit it; Reproduce or Dismiss
//   Reproducing    the agent's progress, the script (folded), the runs as dots
//   Ready to build the plan in plain words, the score and why; Build
//   Building       the checks as a checklist with the real reason, the change, the replay
//   Ready to merge a one-line verdict, the pull request, CI; Approve and merge
import { h, put, toast, enc, SEV, THEME_WORDS, STAGE_NAME, stepOf, blockedOf, blockStep, BLOCK_KIND, looksLikeCmd, cmdOfStep, queuedOf, agentOn, doingWords, ago, toMs, statusLine, thinBar, firstName, fmtScore, firstSentences } from "/goals/converge/util.js";
import { sheetShell, popMenu } from "/goals/converge/kit.js";
import { talk } from "/goals/converge/talk.js";

const PLACEHOLDER = {
  problems: "Ask about this problem",
  reproducing: "Give the repro agent a hint",
  ready: "Change the plan: say what to do differently",
  building: "Tell the fix agent something",
  merge: "Send the fix agent a note",
};
const INTRO = {
  problems: "Ask anything about it: an agent reads the code (it changes nothing) and answers here.",
  reproducing: "A hint goes to the repro agent, or starts one with it.",
  ready: "Say what to change; the planning agent revises the plan and the score.",
  building: "A note steers the fix agent, or restarts it on its branch with it.",
  merge: "A note goes to the fix agent on its branch.",
};

export async function openSheet(C, id) {
  const S = C.S;
  let d = null, sig = "", timer = null, ui = { edit: false, events: false, dismiss: false, back: false, open: new Set(), dot: null };
  const f0 = C.findings().find((x) => x.id === id);
  const sh = sheetShell({
    label: f0 ? f0.title : "A card", data: { id },
    onClose: (quiet) => { clearInterval(timer); tk && tk.stop(); if (!quiet) history.replaceState(history.state, "", C.here()); },
  });
  let tk = null;
  const acts = h("div", { class: "acts" });
  const reload = async () => { sig = ""; await fetchIt(); C.S.sig = ""; C.load(); };
  // One call at a time: a second tap while one is pending sends nothing, and
  // the footer's buttons are disabled until it answers.
  let pending = false;
  const post = async (verb, body, msg) => {
    if (pending) return;
    pending = true;
    acts.querySelectorAll("button").forEach((b) => { b.disabled = true; });
    try { await C.post(id, verb, body); if (msg) toast(msg); } catch (err) { toast(err.message || String(err)); } finally { pending = false; }
    await reload();
  };
  const putF = async (body, msg) => { try { await C.api(`/api/converge/findings/${enc(id)}`, { method: "PUT", body }); toast(msg); } catch (err) { toast(err.message || String(err)); } await reload(); };

  // Draw at once from the overview's copy, then from the card's own data.
  if (f0) paint({ finding: f0, events: [] });
  else put(sh.body, h("p", { class: "cv-empty" }, "Opening the card…"));

  async function fetchIt() {
    let x;
    try { x = await C.api(`/api/converge/findings/${enc(id)}`); } catch (err) {
      if (d) return;
      clearInterval(timer);
      sh.setHead(h("h2", {}, err.status === 404 ? "No such card" : "The card did not open"));
      put(sh.body, h("p", { class: "cv-empty" }, err.status === 404 ? "This card does not exist (it may have been removed). Close this and pick one from the list." : `It did not load: ${err.message || err}`));
      return;
    }
    if (!sh.sheet.isConnected) return;
    // Typing in one of its fields: the next refresh paints it.
    const a = document.activeElement;
    if (a && sh.body.contains(a) && a.matches("input, select, textarea")) return;
    const s = JSON.stringify(x).replace(/"(now|updated)":\d+/g, "");
    if (s === sig) return;
    sig = s;
    paint(x);
  }

  function paint(x) {
    d = x;
    const f = x.finding || {};
    const step = stepOf(f.stage);
    const sk = step ? step.step : f.stage === "merged" ? "merge" : "problems";
    sh.sheet.setAttribute("aria-label", f.title || "A card");
    if (!tk) {
      tk = talk({ api: C.api, base: `/api/converge/findings/${enc(id)}`, placeholder: PLACEHOLDER[sk], intro: INTRO[sk], onSent: () => setTimeout(reload, 800) });
      put(sh.foot, acts, tk.box);
    }
    const more = h("button", { class: "cv-btn icon quiet", type: "button", "aria-label": "More", "aria-haspopup": "menu", onclick: () => menu(f, more) }, "⋯");
    sh.setTools([more]);
    sh.setHead(
      h("span", { class: "cv-meta" }, h("span", { class: "cv-sev", "data-sev": String(f.severity) }), h("span", {}, SEV[f.severity] || ""), h("span", {}, step ? step.name : STAGE_NAME[f.stage] || f.stage), f.state && f.state !== "active" ? h("span", { class: "cv-tag bad" }, f.state.replace(/_/g, " ")) : null, f.pinned ? h("span", { class: "cv-tag" }, "pinned") : null),
      h("h2", {}, f.title || ""),
      statusLine(f, C.settings()),
      thinBar(f, C.settings()));
    const keepTop = sh.body.scrollTop;
    const parts = [
      ui.dismiss ? dismissBox(f) : null,
      ui.back ? backBox(f) : null,
      ui.edit ? editBox(f) : null,
      blockedBox(f),
      scopeAskBox(f),
      queuedOf(f) ? h("p", { class: "cv-note" }, `Queued: ${queuedOf(f)}. It starts by itself when a slot frees.`) : null,
      ...stageParts(f, x),
      ui.events ? eventsBox(x) : null,
      tk.section,
    ];
    put(sh.body, parts);
    sh.body.scrollTop = keepTop;
    put(acts, actions(f));
  }

  // ------------------------------------------------------------ moves
  function actions(f) {
    const b = blockedOf(f);
    const out = [];
    const btn = (label, cls, fn) => h("button", { class: `cv-btn ${cls || ""}`, type: "button", onclick: fn }, label);
    if (f.state === "parked") return [btn("Back in the flow", "pri", () => post("unpark", {}, "Back in the flow"))];
    if (f.state !== "active") return [];
    if (b) {
      out.push(btn("Done, carry on", "pri", () => {
        const note = sh.body.querySelector(".cv-bsec input")?.value.trim();
        post("unblock", note ? { note } : {}, "Thanks: it carries on");
      }));
      return out;
    }
    switch (f.stage) {
      case "found":
        out.push(btn("Dismiss", "", () => { ui.dismiss = true; paint(d); sh.body.scrollTop = 0; }));
        out.push(btn("Reproduce", "pri", () => post("reproduce", {}, "On to Reproducing: an agent writes the repro")));
        break;
      case "reproduce":
        if (f.needs_you || (f.repro_attempts || 0) >= 2) {
          out.push(btn("Dismiss", "", () => { ui.dismiss = true; paint(d); sh.body.scrollTop = 0; }));
          out.push(btn("Give a hint", "pri", () => tk.focus()));
        }
        break;
      case "prioritize":
        out.push(btn(f.door ? "Build (one-way door)" : "Build", "pri", () => {
          if (f.door && !confirm(`This touches a one-way door (${f.door}). Start building it?`)) return;
          post("start", {}, "A fix agent is on it");
        }));
        break;
      case "develop": case "verify": {
        const failing = f.last_check && Array.isArray(f.last_check.problems) && f.last_check.problems.length || f.needs_you;
        if (failing || f.agent_state === "idle" || !agentOn(f)) {
          if (f.agent) out.push(h("a", { class: "cv-btn", href: `#/a/${enc(f.agent)}/terminal` }, "Take over"));
          // In develop a note restarts or steers the fix agent; in verify, "back" moves it back.
          out.push(f.stage === "verify" ? btn("Send back with a note", "pri", () => { ui.back = true; paint(d); sh.body.scrollTop = 0; }) : btn("Send a note", "pri", () => tk.focus()));
        }
        break;
      }
      case "review":
        out.push(btn("Send back", "", () => { ui.back = true; paint(d); sh.body.scrollTop = 0; }));
        // A pull request that conflicts with the base is never offered for the merge.
        if (f.ci === "pass" && !f.conflict) out.push(btn("Approve and merge", "good", () => { if (confirm(`Merge the pull request of “${f.title}”?`)) post("approve", {}, "Approved: merging"); }));
        break;
      default: break;
    }
    return out;
  }

  function menu(f, anchor) {
    const items = [
      ["Edit the card", () => { ui.edit = !ui.edit; paint(d); sh.body.scrollTop = 0; }],
      [f.pinned ? "Unpin" : "Pin to the top", () => post(f.pinned ? "unpin" : "pin", {}, f.pinned ? "Unpinned" : "Pinned to the top")],
      f.state === "parked" ? ["Back in the flow", () => post("unpark", {}, "Back in the flow")] : f.state === "active" ? ["Park it", () => post("park", {}, "Parked: the autopilot leaves it alone")] : null,
      f.state === "active" ? ["Dismiss…", () => { ui.dismiss = true; paint(d); sh.body.scrollTop = 0; }, { danger: true }] : null,
      "-",
      f.agent ? ["Open the agent's terminal", `#/a/${enc(f.agent)}/terminal`] : null,
      f.run ? ["The run that found it", C.here({ run: f.run })] : null,
      [ui.events ? "Hide the history" : "Show the history", () => { ui.events = !ui.events; paint(d); }],
    ];
    popMenu(anchor, items);
  }

  function dismissBox(f) {
    const why = h("input", { class: "cv-in", type: "text", placeholder: "Why is it not a problem?", "aria-label": "Why is it not a problem?" });
    setTimeout(() => why.focus(), 0);
    return h("section", { class: "cv-note", style: { display: "flex", flexDirection: "column", gap: "8px" } },
      h("span", { class: "cv-label" }, "Dismiss"), why,
      h("div", { style: { display: "flex", gap: "8px" } },
        h("button", { class: "cv-btn", type: "button", onclick: () => { ui.dismiss = false; paint(d); } }, "Cancel"),
        h("button", { class: "cv-btn crit", type: "button", onclick: async () => { if (!why.value.trim()) { why.focus(); toast("Say why first: it teaches the personas"); return; } await post("dismiss", { reason: why.value.trim() }, "Dismissed"); sh.close(); } }, "Dismiss it")));
  }

  /** "Send back with a note": the `back` verb with {text}; talking alone does not move a card back. */
  function backBox(f) {
    const ta = h("textarea", { class: "cv-ta", rows: "3", placeholder: "What should the fix agent change?", "aria-label": "What should the fix agent change?", style: { fontFamily: "var(--sans)", fontSize: "16px", minHeight: "88px" } });
    setTimeout(() => ta.focus(), 0);
    return h("section", { class: "cv-note", style: { display: "flex", flexDirection: "column", gap: "8px" } },
      h("span", { class: "cv-label" }, "Send back to Building"), ta,
      h("div", { style: { display: "flex", gap: "8px" } },
        h("button", { class: "cv-btn", type: "button", onclick: () => { ui.back = false; paint(d); } }, "Cancel"),
        h("button", { class: "cv-btn pri", type: "button", onclick: async () => { if (!ta.value.trim()) { ta.focus(); toast("Say what to change first: the fix agent gets only your note"); return; } ui.back = false; await post("back", { text: ta.value.trim() }, "Sent back: the fix agent has your note"); } }, "Send back")));
  }

  function editBox(f) {
    const themes = C.S.d.themes || Object.keys(THEME_WORDS);
    const kind = h("select", { id: "cv-e-k", "aria-label": C.KIND_LABEL, onchange: () => putF({ theme: kind.value }, "Kind changed") }, themes.map((t) => h("option", { value: t, selected: t === f.theme ? true : null }, THEME_WORDS[t] || t)));
    const names = [...new Set([...(C.S.d.components || []).map((c) => (typeof c === "string" ? c : c && c.name)), f.component].filter(Boolean))];
    const comp = h("select", { id: "cv-e-c", "aria-label": "Component", onchange: () => putF({ component: comp.value }, "Component changed") }, h("option", { value: "", selected: f.component ? null : true }, "No component"), names.map((c) => h("option", { value: c, selected: c === f.component ? true : null }, c)));
    const sev = h("select", { id: "cv-e-s", "aria-label": "Severity", onchange: () => putF({ severity: sev.value }, "Severity changed") }, [4, 3, 2, 1].map((k) => h("option", { value: SEV[k], selected: k === f.severity ? true : null }, SEV[k])));
    // The card's context: what the agents work from. Kept while the sheet
    // refreshes (ui.draft), saved together, told to the card's agent.
    const dr = ui.draft || (ui.draft = { title: f.title || "", detail: f.detail || "", ...(f.scope && typeof f.scope === "object" ? { area: f.scope.area || "", parts: (f.scope.parts || []).map((x) => ({ ...x })), files: (f.scope.files || []).join("\n"), root_cause: f.scope.root_cause || "", acceptance: f.scope.acceptance || "", size: f.scope.size || "M" } : {}) });
    const hasPlan = "files" in dr;
    const field = (k, label, el) => { el.id = `cv-e-${k}`; el.value = dr[k]; el.addEventListener("input", () => { dr[k] = el.value; }); return h("div", { class: "cv-f", style: { gridColumn: "1 / -1" } }, h("label", { for: el.id }, label), el); };
    const ta = (rows) => h("textarea", { class: "cv-ta", rows: String(rows), style: { fontFamily: "var(--sans)", fontSize: "16px" } });
    const inp = () => h("input", { class: "cv-in", type: "text", style: { fontSize: "16px" } });
    const sizeSel = hasPlan ? h("select", { id: "cv-e-size", onchange: () => { dr.size = sizeSel.value; } }, ["S", "M", "L"].map((x) => h("option", { value: x, selected: x === (dr.size || "M").toUpperCase() ? true : null }, { S: "Small", M: "Medium", L: "Large" }[x]))) : null;
    const save = h("button", { class: "cv-btn pri", type: "button", onclick: async () => {
      const body = { title: dr.title.trim(), detail: dr.detail };
      if (hasPlan) body.scope = { area: dr.area.trim(), parts: dr.parts || [], files: dr.files.split(/[\n,]+/).map((x) => x.trim()).filter(Boolean), root_cause: dr.root_cause.trim(), acceptance: dr.acceptance.trim(), size: dr.size || "M" };
      save.disabled = true;
      try {
        const r = await C.api(`/api/converge/findings/${enc(id)}`, { method: "PUT", body });
        const what = (r && r.edited) || [];
        toast(!what.length ? "Nothing changed" : r.recheck ? `Saved: ${what.join(", ")}. The branch is checked again.` : f.agent ? `Saved: ${what.join(", ")}. The agent has the new version.` : `Saved: ${what.join(", ")}`);
        ui.edit = false; ui.draft = null;
      } catch (err) { toast(err.message || String(err)); save.disabled = false; }
      await reload();
    } }, "Save");
    return h("section", { class: "cv-note" },
      h("span", { class: "cv-label" }, "Edit the card"),
      h("p", { class: "cv-meta" }, "The agents work from this. Saving tells the card's agent; a plan change while it is building checks the branch again."),
      h("div", { class: "cv-edit" },
        field("title", "Title", inp()),
        field("detail", "What happened (and anything the agent should know)", ta(5)),
        hasPlan ? partsEditor(dr) : null,
        hasPlan ? h("details", { class: "cv-fold", style: { gridColumn: "1 / -1" }, open: dr.files ? true : null }, h("summary", {}, "Exact paths", h("span", { class: "s" }, "advanced")),
          field("files", "One path per line; a folder ends with /", Object.assign(ta(3), { style: "font-family: var(--mono); font-size: 16px" }))) : null,
        hasPlan ? field("area", "Where", inp()) : null,
        hasPlan ? field("root_cause", "Root cause", ta(2)) : null,
        hasPlan ? field("acceptance", "Fixed when", ta(2)) : null,
        hasPlan ? h("div", { class: "cv-f" }, h("label", { for: "cv-e-size" }, "Size"), sizeSel) : null,
        h("div", { class: "cv-f" }, h("label", { for: "cv-e-k" }, C.KIND_LABEL), kind),
        h("div", { class: "cv-f" }, h("label", { for: "cv-e-c" }, "Component"), comp),
        h("div", { class: "cv-f" }, h("label", { for: "cv-e-s" }, "Severity"), sev)),
      h("div", { style: { display: "flex", gap: "8px", marginTop: "8px" } },
        h("button", { class: "cv-btn", type: "button", onclick: () => { ui.edit = false; ui.draft = null; paint(d); } }, "Cancel"), save));
  }

  // A plan's parts of the code as chips: the name, and the files it stands for.
  function partChips(sc) {
    const parts = Array.isArray(sc.parts) ? sc.parts : [];
    const files = Array.isArray(sc.files) ? sc.files : [];
    if (!parts.length && !files.length) return "–";
    return h("div", { class: "cv-parts" },
      parts.map((x) => h("span", { class: "cv-part", title: (x.paths || []).join("\n") }, h("b", {}, x.name || (x.paths || [])[0] || "?"), (x.paths || []).length ? h("span", {}, (x.paths || []).join(", ")) : h("span", { class: "warn" }, "not found in the code"))),
      files.map((p) => h("span", { class: "cv-part" }, h("b", {}, p))));
  }

  // Add parts by what they do: search as you type over the code index.
  function partsEditor(dr) {
    dr.parts = dr.parts || [];
    const list = h("div", { class: "cv-parts" });
    const sugg = h("div", { class: "cv-parts sugg" });
    const drawList = () => put(list, dr.parts.length ? dr.parts.map((x, i) => h("span", { class: "cv-part" }, h("b", {}, x.name || (x.paths || [])[0]), (x.paths || []).length ? h("span", {}, x.paths.join(", ")) : h("span", { class: "warn" }, "found on save"),
      h("button", { type: "button", class: "x", "aria-label": `Remove ${x.name}`, onclick: () => { dr.parts.splice(i, 1); drawList(); } }, "×"))) : h("span", { class: "cv-meta" }, "None yet: the fix may change anything."));
    let t = null, seq = 0;
    const q = h("input", { class: "cv-in", type: "search", id: "cv-e-part", placeholder: "e.g. the crawler's template learning", style: { fontSize: "16px" }, autocomplete: "off" });
    const add = (part) => { if (!dr.parts.some((x) => (x.paths || []).join() === (part.paths || []).join() && x.name === part.name)) dr.parts.push(part); q.value = ""; put(sugg); drawList(); };
    q.addEventListener("input", () => {
      clearTimeout(t);
      const text = q.value.trim();
      if (text.length < 3) { put(sugg); return; }
      t = setTimeout(async () => {
        const my = ++seq;
        let r = null;
        try { r = await C.api(`/api/converge/parts?repo=${enc(C.repo())}&q=${enc(text)}`); } catch { r = null; }
        if (my !== seq) return;
        const found = (r && r.parts) || [];
        put(sugg, found.map((x) => h("button", { type: "button", class: "cv-part pick", onclick: () => add(x) }, h("b", {}, x.name), h("span", {}, (x.paths || []).join(", ")))),
          h("button", { type: "button", class: "cv-part pick", onclick: () => add({ name: text, paths: [], symbols: [] }) }, h("b", {}, `“${text}”`), h("span", {}, found.length ? "as written: matched on save" : "nothing in the code index matches: matched on save")));
      }, 250);
    });
    q.addEventListener("keydown", (e) => { if (e.key === "Enter") { e.preventDefault(); const first = sugg.querySelector("button"); if (first) first.click(); } });
    drawList();
    return h("div", { class: "cv-f", style: { gridColumn: "1 / -1" } },
      h("label", { for: "cv-e-part" }, "Parts of the code it touches"), list, q, sugg);
  }

  // The fix agent asks to change the plan: one tap to approve.
  function scopeAskBox(f) {
    const a = f.scope_ask && typeof f.scope_ask === "object" ? f.scope_ask : null;
    if (!a) return null;
    const add = Array.isArray(a.add) ? a.add : [], rm = Array.isArray(a.remove) ? a.remove : [];
    return h("section", { class: "cv-bsec" },
      h("span", { class: "hd" }, `${a.by || "The fix agent"} asks to change the plan`),
      add.length ? h("p", {}, h("b", {}, "Add: "), h("code", {}, add.join(", "))) : null,
      rm.length ? h("p", {}, h("b", {}, "Remove: "), h("code", {}, rm.join(", "))) : null,
      a.why ? h("p", {}, a.why) : null,
      h("div", { style: { display: "flex", gap: "8px", flexWrap: "wrap" } },
        h("button", { class: "cv-btn", type: "button", onclick: () => post("scope-no", {}, "Kept the plan: the agent stays inside it") }, "Keep the plan"),
        h("button", { class: "cv-btn pri", type: "button", onclick: () => post("scope-ok", {}, "Plan changed: the branch is checked again") }, "Approve")));
  }

  // "changed outside the plan's files: a, b (…)": the files, to add with one tap.
  function outsideFiles(p) {
    const m = /outside the (?:plan's files|scope): ([^(]+)/.exec(p || "");
    return m ? m[1].split(",").map((x) => x.trim()).filter(Boolean) : [];
  }

  function blockedBox(f) {
    const b = blockedOf(f);
    if (!b) return null;
    const stepTxt = blockStep(f, b);
    const cmd = looksLikeCmd(stepTxt) ? cmdOfStep(stepTxt) : null;
    return h("section", { class: "cv-bsec" },
      h("span", { class: "hd" }, `Blocked on you · ${BLOCK_KIND[b.kind] || BLOCK_KIND.other}`),
      b.what ? h("p", {}, b.what) : null,
      cmd ? h("div", { class: "do" }, h("code", {}, cmd), h("button", { class: "cv-btn", type: "button", onclick: () => { navigator.clipboard?.writeText(cmd).then(() => toast("Copied"), () => toast("Copy did not work: select it")); } }, "Copy"))
        : h("p", {}, h("b", {}, "Your step: "), stepTxt),
      h("input", { class: "cv-in", type: "text", placeholder: "A note for the agent, if any (optional)", "aria-label": "A note for the agent" }),
      h("span", { class: "by" }, `Asked by ${b.by || "an agent"}${b.at ? ` · ${ago(b.at)} ago` : ""}. When it is done, tap “Done, carry on”.`));
  }

  // ------------------------------------------------------------ by step
  function stageParts(f, x) {
    const evs = Array.isArray(x.events) ? x.events : [];
    const lastEv = (...gates) => [...evs].reverse().find((e) => gates.includes(e.gate));
    const who = () => {
      const hits = f.personas_hit || [];
      const pn = x.persona ? firstName(x.persona.name) : f.persona ? firstName(((C.S.d.personas || []).find((p) => p.id === f.persona) || {}).name || f.persona) : null;
      return [pn ? `Found by ${pn}${f.step ? ` at “${f.step}”` : ""}` : `Reported by ${f.by || "someone"}`, hits.length > 1 ? `${hits.length} personas` : null, f.occurrences > 1 ? `seen ${f.occurrences}×` : null, f.component || null].filter(Boolean).join(" · ");
    };
    const what = h("section", {}, h("span", { class: "cv-label" }, "What happened"), h("p", {}, f.detail || "No detail was written."), h("p", { class: "cv-meta", style: { marginTop: "6px" } }, who()));
    switch (f.stage) {
      case "found": return [what, f.repro ? fold("Their repro script", null, h("pre", { class: "cv-pre" }, f.repro)) : null];
      case "reproduce": return [reproProgress(f, lastEv("repro_fails", "repro_broken")), what];
      case "scope": case "prioritize": return [plan(f), what];
      case "develop": case "verify": return [checks(f, evs), change(f), verify(f, lastEv("verify")), what];
      case "review": case "merged": return [verdict(f, evs), change(f), verify(f, lastEv("verify")), what];
      default: return [what];
    }
  }

  function fold(title, sub, ...kids) {
    const key = title;
    const det = h("details", { class: "cv-fold", open: ui.open.has(key) ? true : null }, h("summary", {}, title, sub ? h("span", { class: "s" }, sub) : null), ...kids);
    det.addEventListener("toggle", () => { if (det.open) ui.open.add(key); else ui.open.delete(key); });
    return det;
  }

  function reproProgress(f, ev) {
    const runs = (ev && ev.evidence && Array.isArray(ev.evidence.runs)) ? ev.evidence.runs : [];
    const showed = runs.filter((r) => r.exit === 1).length;
    const agent = agentOn(f) ? `${f.agent_state === "starting" ? "starting" : doingWords(f.agent_doing) || "writing the repro"}` : null;
    const head = blockedOf(f) ? null : f.state === "cant_reproduce" ? h("div", { class: "cv-verdict bad" }, "Could not reproduce it. Give a hint below, or dismiss it.")
      : f.needs_you ? h("div", { class: "cv-verdict bad" }, f.needs_you)
        : agent ? h("div", { class: "cv-verdict wait" }, h("span", { class: "cv-live" }), `Repro agent: ${agent}`)
          : f.repro && !f.repro_hash ? h("div", { class: "cv-verdict wait" }, "Running the repro at the base branch")
            : h("div", { class: "cv-verdict wait" }, queuedOf(f) ? "Waits for a free agent to write the repro" : "An agent writes the repro next");
    const dots = runs.length ? h("section", {}, h("span", { class: "cv-label" }, `Runs · ${showed} of ${runs.length} showed the problem`),
      h("div", { class: "cv-dots" }, runs.map((r, i) => h("button", { type: "button", class: r.exit === 1 ? "p" : "f", "aria-pressed": String(ui.dot === i), "aria-label": `Run ${i + 1}: exit ${r.exit}`, onclick: () => { ui.dot = ui.dot === i ? null : i; paint(d); } }, h("i")))),
      ui.dot != null && runs[ui.dot] ? h("pre", { class: "cv-pre", style: { marginTop: "6px", whiteSpace: "pre-wrap" } }, `exit ${runs[ui.dot].exit}\n${runs[ui.dot].output || "(no output)"}`) : h("p", { class: "cv-meta" }, "Tap a dot for its output. Green: the script showed the problem (exit 1).")) : null;
    return [head, dots, f.repro ? fold("The script", f.repro_hash ? `locked ${f.repro_hash.slice(0, 8)}` : `${f.repro.split("\n").length} lines`, h("pre", { class: "cv-pre" }, f.repro)) : null, ev && ev.summary ? h("p", { class: "cv-meta" }, `${ev.summary} · ${ago(ev.ts_ms)}`) : null];
  }

  function plan(f) {
    const sc = f.scope && typeof f.scope === "object" ? f.scope : null;
    if (!sc) return h("div", { class: "cv-verdict wait" }, agentOn(f) || f.busy ? h("span", { class: "cv-live" }) : null, f.busy ? String(f.busy) : "Being planned: an agent reads the code and writes the plan");
    const sevW = { 4: 8, 3: 4, 2: 2, 1: 1 }[f.severity] || 1;
    const hits = Math.max(1, (f.personas_hit || []).length);
    const why = [`${SEV[f.severity]} (×${sevW})`, `${hits} persona${hits > 1 ? "s" : ""}${f.occurrences > hits ? `, seen ${f.occurrences}×` : ""}`, f.ease_before != null ? `ease ${f.ease_before} of 5` : null, `size ${sc.size || "M"}`].filter(Boolean).join(" · ");
    const wip = Number(C.settings().wip) || 0;
    const building = C.findings().filter((y) => y.state === "active" && (y.stage === "develop" || y.stage === "verify")).length;
    return [
      f.stage === "prioritize" ? h("div", { class: "cv-verdict wait", style: { justifyContent: "space-between" } }, h("span", {}, `Score ${fmtScore(f.score)}`), h("span", { style: { fontWeight: "var(--w-text)", fontSize: "var(--fs-sm)", color: "var(--text-2)" } }, why)) : null,
      h("section", {}, h("span", { class: "cv-label" }, "The plan"), h("dl", { class: "cv-kv" },
        h("dt", {}, "Where"), h("dd", {}, sc.area || "–"),
        h("dt", {}, "Root cause"), h("dd", {}, sc.root_cause || "–"),
        h("dt", {}, "Fixed when"), h("dd", {}, sc.acceptance || "–"),
        h("dt", {}, "Size"), h("dd", {}, { S: "Small", M: "Medium", L: "Large" }[sc.size] || sc.size || "–"),
        h("dt", {}, "One-way door"), h("dd", {}, f.door ? `Yes: ${f.door}. Only you start it.` : "No"),
        h("dt", {}, "Parts of the code"), h("dd", {}, partChips(sc)))),
      f.stage === "prioritize" && wip ? h("p", { class: "cv-meta" }, `${Math.min(building, wip)} of ${wip} building slots in use${building >= wip ? ": it waits for one after you pick it" : ""}`) : null,
    ];
  }

  function checks(f, evs) {
    const ev = [...evs].reverse().find((e) => e.gate === "develop" && e.evidence);
    const e = ev ? ev.evidence : null;
    const lc = f.last_check && typeof f.last_check === "object" ? f.last_check : null;
    const probs = lc && Array.isArray(lc.problems) ? lc.problems : [];
    const has = (re) => probs.find((p) => re.test(p));
    const item = (state, text, why) => h("li", {}, h("span", { class: `mk ${state}`, "aria-label": state }, state === "done" ? "✓" : state === "fail" ? "✕" : state === "run" ? h("span", { class: "cv-live" }) : ""), h("span", {}, text, why ? h("span", { class: "why" }, why) : null), h("span"));
    const rounds = Math.max(1, C.settings().max_rounds || 3);
    const fresh = !e && !lc;
    const repro = e && Array.isArray(e.repro) ? e.repro : [];
    const green = repro.filter((r) => r.exit === 0).length;
    const st = (failMatch, okIf) => fresh ? "todo" : failMatch ? "fail" : okIf ? "done" : "todo";
    const changed = e && Array.isArray(e.changed) ? e.changed : [];
    const list = h("ul", { class: "cv-checks" },
      item(agentOn(f) ? "run" : f.rounds || !fresh ? "done" : "todo", agentOn(f) ? `Fix agent: ${doingWords(f.agent_doing) || "working"}` : "The fix agent made its change", null),
      item(st(has(/repro/), repro.length && green === repro.length), repro.length ? `Repro passes ${green} of ${repro.length}` : "Repro passes", has(/repro/)),
      item(fresh || !e || !e.tests || e.tests.note ? (has(/tests failed/i) ? "fail" : "todo") : e.tests.exit === 0 ? "done" : "fail", e && e.tests && e.tests.cmd ? `Tests pass (${e.tests.cmd})` : "Tests pass", has(/tests failed|test command/i) || (e && e.tests && e.tests.note) || null),
      item(fresh ? "todo" : has(/no test file/) || (e && !changed.some((p) => /test/i.test(p)) && !e.docs_only) ? "fail" : e ? "done" : "todo", "A test changed", has(/no test file/) || (e && !changed.some((p) => /test/i.test(p)) && !e.docs_only ? "no test file changed: add a regression test" : null)),
      item(st(has(/outside the (scope|plan)|no commits/), changed.length), "Inside the plan's files", has(/outside the (scope|plan)|no commits/)),
      f.stage === "verify" ? item("run", "The persona replays it on the branch", null) : null);
    return [
      lc && probs.length ? h("div", { class: "cv-verdict bad" }, `Round ${lc.round || f.rounds || 1} of ${rounds} did not pass: ${probs[0]}`) : null,
      (() => {
        const out = outsideFiles(has(/outside the (scope|plan)/));
        if (!out.length || f.scope_ask) return null;
        const sc = f.scope && typeof f.scope === "object" ? f.scope : {};
        return h("div", { style: { display: "flex", gap: "8px", flexWrap: "wrap", alignItems: "center" } },
          h("button", { class: "cv-btn pri", type: "button", onclick: async () => {
            const parts = [];
            for (const file of out) {
              let r = null;
              try { r = await C.api(`/api/converge/parts?repo=${enc(C.repo())}&q=${enc(file)}`); } catch { r = null; }
              parts.push(((r && r.parts) || [])[0] || { name: file, paths: [file], symbols: [] });
            }
            await putF({ scope: { ...sc, parts: [...(sc.parts || []), ...parts] } }, `Added ${parts.map((x) => x.name).join(", ")} to the plan: checking the branch again`);
          } }, `Add ${out.length > 1 ? `these ${out.length} parts` : out[0].split("/").slice(-2).join("/")} to the plan and check again`),
          h("button", { class: "cv-btn quiet", type: "button", onclick: () => { ui.edit = true; paint(d); sh.body.scrollTop = 0; } }, "Edit the plan"));
      })(),
      h("section", {}, h("span", { class: "cv-label" }, f.rounds ? `Checks · round ${Math.min(f.rounds + 1, rounds)} of ${rounds}` : "Checks"), list),
      lc && lc.agent_said ? h("section", {}, h("span", { class: "cv-label" }, "The fix agent said"), h("p", { class: "cv-note" }, lc.agent_said)) : null,
    ];
  }

  function change(f) {
    if (!f.agent) return null;
    const k = `diff:${f.agent}`;
    const c = (S.diffs = S.diffs || new Map()).get(k);
    if (!c || Date.now() - c.at > 15000) {
      S.diffs.set(k, { ...(c || {}), at: Date.now() });
      C.api(`/api/agents/${enc(f.agent)}/diff`).then((x) => { S.diffs.set(k, { x, at: Date.now() }); if (sh.sheet.isConnected && d) paint(d); }, () => {});
    }
    const files = c && c.x && Array.isArray(c.x.files) ? c.x.files : null;
    if (!files) return null;
    // Workbench's diff gives each file's hunks: its lines are counted (+ added, - removed).
    const lines = (o, k) => (Array.isArray(o.hunks) ? o.hunks : []).reduce((a, hk) => a + (Array.isArray(hk.lines) ? hk.lines.filter((l) => l && l.kind === k).length : 0), 0);
    const num = (o, ...ks) => { for (const q of ks) if (o[q] != null && Number.isFinite(Number(o[q]))) return Number(o[q]); return lines(o, ks[0] === "added" ? "+" : "-"); };
    const add = files.reduce((a, o) => a + num(o, "added", "adds", "insertions", "plus"), 0);
    const del = files.reduce((a, o) => a + num(o, "removed", "deleted", "dels", "deletions", "minus"), 0);
    return fold("The change", `${files.length} file${files.length === 1 ? "" : "s"} · +${add} −${del}`,
      h("ul", { class: "cv-checks" }, files.slice(0, 40).map((o) => h("li", {}, h("span"), h("span", { style: { fontFamily: "var(--mono)", fontSize: "var(--fs-xs)", overflowWrap: "anywhere" } }, o.path || o.file || o.name || String(o)), h("span", { class: "du" }, `+${num(o, "added", "adds", "insertions", "plus")} −${num(o, "removed", "deleted", "dels", "deletions", "minus")}`)))),
      h("a", { class: "cv-btn quiet", href: `#/a/${enc(f.agent)}/changes` }, "See the diff"));
  }

  function verify(f, ev) {
    const e = ev && ev.evidence ? ev.evidence : null;
    if (!e && f.ease_after == null) return null;
    const before = e?.ease_before ?? f.ease_before, after = e?.ease_after ?? f.ease_after;
    const nf = e && Array.isArray(e.new_findings) ? e.new_findings : [];
    const pn = x0 => firstName(((C.S.d.personas || []).find((p) => p.id === x0) || {}).name || x0 || "The persona");
    const words = [`ease ${before ?? "–"} → ${after ?? "–"}`, e?.fixed === true || ev?.passed ? "problem gone" : e?.fixed === false ? "problem still there" : null, nf.length ? `${nf.length} new problem${nf.length > 1 ? "s" : ""}` : "nothing new"].filter(Boolean).join(", ");
    return h("section", {}, h("span", { class: "cv-label" }, `${pn(ev?.by || f.persona)} replayed it`), h("p", {}, words), e && e.answer ? h("p", { class: "cv-note", style: { marginTop: "6px" } }, firstSentences(e.answer, 2)) : null);
  }

  function verdict(f, evs) {
    if (f.stage === "merged") return h("div", { class: `cv-verdict${f.guard_ok === false ? " bad" : ""}` }, f.guard_ok === false ? "Merged, but the nightly guard says the problem is back." : "Merged. The nightly guard keeps running its repro.");
    const ver = [...evs].reverse().find((e) => e.gate === "verify");
    const dev = [...evs].reverse().find((e) => e.gate === "develop" && e.passed);
    const bits = [f.ci === "pass" ? "CI green" : f.ci === "fail" ? "CI failed" : "CI running", dev ? "checks passed, in scope" : null, ver && ver.passed ? "the persona succeeded" : null].filter(Boolean);
    return [
      f.conflict ? h("div", { class: "cv-verdict bad" }, `${f.conflict}. Not ready to merge.`)
        : h("div", { class: `cv-verdict${f.ci === "pass" ? "" : f.ci === "fail" ? " bad" : " wait"}` }, f.ci === "pass" ? `Ready: ${bits.join(" · ")}` : bits.join(" · ")),
      f.pr_url ? h("section", {}, h("span", { class: "cv-label" }, "Pull request"), h("a", { href: f.pr_url, target: "_blank", rel: "noopener", style: { overflowWrap: "anywhere" } }, f.pr_url), f.branch ? h("p", { class: "cv-meta" }, `Branch ${f.branch}`) : null) : null,
    ];
  }

  function eventsBox(x) {
    const evs = Array.isArray(x.events) ? x.events : [];
    return h("section", {}, h("span", { class: "cv-label" }, "History"), h("ul", { class: "cv-checks" }, evs.slice(-30).reverse().map((e) =>
      h("li", { style: { overflowWrap: "anywhere" } }, h("span", { class: `mk ${e.passed || e.gate === "reported" ? "done" : "fail"}` }, e.passed || e.gate === "reported" ? "✓" : "!"), h("span", {}, h("b", {}, e.gate), e.summary ? `: ${e.summary}` : ""), h("span", { class: "du" }, `${e.by || ""} · ${ago(e.ts_ms)}`)))));
  }

  await fetchIt();
  timer = setInterval(() => { if (!document.hidden && sh.sheet.isConnected) { fetchIt(); if (tk) tk.refresh(); } }, 4000);
}
