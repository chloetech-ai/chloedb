// Problems: what the personas hit, as a triage list. One row of filter
// chips, rows you can select, "Reproduce" and "Dismiss" in a sticky bar;
// on a phone a swipe right reproduces and a swipe left dismisses (with an
// undo). "+ New problem" writes a card from a sentence, or by hand.
import { h, put, toast, SEV, THEME_WORDS, KINDS, toMs, ago, firstName, flagLine, blockedOf } from "/goals/converge/util.js";
import { chipFilter, sheetShell } from "/goals/converge/kit.js";
import { listRow, stepHead, none, bulkBar } from "/goals/converge/rows.js";

const UNDO_MS = 5000;

export function problemsScreen(C) {
  const S = C.S, F = S.pf;
  const all = C.findings().filter((f) => f.state === "active" && f.stage === "found");
  const from = C.query().get("from");
  const run = from ? (S.d.runs || []).find((r) => r.id === from) : null;
  const persona = (id) => (S.d.personas || []).find((p) => p.id === id);
  const personaName = (id) => firstName(persona(id)?.name || id);
  const isNew = (f) => S.seenAt && (toMs(f.created) || 0) > S.seenAt;
  const inRun = (f) => !from || f.run === from || (S.runSum && (S.runSum.get(from) || []).includes(f.id)) || (run && (run.findings || []).some((x) => x && (x.id === f.id || x.finding === f.id)));
  const match = (f, skip) => inRun(f)
    && (skip === "sev" || !F.sev.size || F.sev.has(f.severity))
    && (skip === "kind" || !F.kind.size || F.kind.has(f.theme))
    && (skip === "comp" || !F.comp.size || F.comp.has(f.component || ""))
    && (skip === "persona" || !F.persona.size || (f.personas_hit || [f.persona]).some((p) => F.persona.has(p)))
    && (skip === "fresh" || !F.fresh || isNew(f));
  const shown = all.filter((f) => match(f)).sort((a, b) => (blockedOf(b) ? 1 : 0) - (blockedOf(a) ? 1 : 0) || (b.pinned ? 1 : 0) - (a.pinned ? 1 : 0) || b.severity - a.severity || (b.occurrences || 1) - (a.occurrences || 1) || (toMs(b.created) || 0) - (toMs(a.created) || 0));
  const count = (skip, pred) => all.filter((f) => match(f, skip) && pred(f)).length;
  const redraw = () => { C.keep(); C.draw(); };
  // The selection keeps only what is still on screen.
  for (const id of [...S.sel.keys()]) if (!shown.some((f) => f.id === id)) S.sel.delete(id);

  const comps = [...new Set(all.map((f) => f.component).filter(Boolean))].sort();
  const pids = [...new Set(all.flatMap((f) => f.personas_hit && f.personas_hit.length ? f.personas_hit : f.persona ? [f.persona] : []))];
  const freshN = count("fresh", isNew);
  const chips = h("div", { class: "cv-chips", role: "group", "aria-label": "Filters" },
    run ? h("button", { class: "cv-chip on", type: "button", onclick: () => { location.hash = C.href("problems"); } }, `From ${personaName(run.persona)}'s run`, h("span", { "aria-hidden": "true" }, "×")) : null,
    S.seenAt ? h("button", { class: `cv-chip${F.fresh ? " on" : ""}`, type: "button", "aria-pressed": String(F.fresh), onclick: () => { F.fresh = !F.fresh; redraw(); } }, "New since last visit", h("span", { class: "n" }, String(freshN))) : null,
    chipFilter({ label: "Severity", chosen: F.sev, onChange: redraw, options: [4, 3, 2, 1].map((k) => ({ value: k, label: SEV[k], dot: k, n: count("sev", (f) => f.severity === k) })) }),
    chipFilter({ label: "Kind", chosen: F.kind, onChange: redraw, options: (S.d.themes || Object.keys(THEME_WORDS)).map((t) => ({ value: t, label: THEME_WORDS[t] || t, n: count("kind", (f) => f.theme === t) })).filter((o) => o.n || F.kind.has(o.value)) }),
    comps.length ? chipFilter({ label: "Component", chosen: F.comp, onChange: redraw, options: comps.map((c) => ({ value: c, label: c, n: count("comp", (f) => f.component === c) })) }) : null,
    pids.length ? chipFilter({ label: "Persona", chosen: F.persona, onChange: redraw, options: pids.map((p) => ({ value: p, label: personaName(p), n: count("persona", (f) => (f.personas_hit || [f.persona]).includes(p)) })) }) : null,
    F.sev.size + F.kind.size + F.comp.size + F.persona.size || F.fresh ? h("button", { class: "cv-chip", type: "button", onclick: () => { F.sev.clear(); F.kind.clear(); F.comp.clear(); F.persona.clear(); F.fresh = false; redraw(); } }, "Clear") : null);

  /** Hide the rows now, send the verb once the undo has passed. */
  function later(ids, verb, body, words) {
    ids.forEach((id) => { S.gone.add(id); S.sel.delete(id); });
    redraw();
    let undone = false;
    const timer = setTimeout(async () => {
      if (undone) return;
      try { await C.bulk(ids, verb, body); } catch (err) { toast(err.message || String(err)); }
      ids.forEach((id) => S.gone.delete(id));
      S.sig = ""; await C.load();
    }, UNDO_MS);
    toast(words, { undo: () => { undone = true; clearTimeout(timer); ids.forEach((id) => S.gone.delete(id)); redraw(); }, ms: UNDO_MS });
  }
  const reproduce = (ids) => later(ids, "reproduce", {}, ids.length > 1 ? `${ids.length} on to Reproducing` : "On to Reproducing: an agent writes the repro");
  const dismiss = (ids, reason) => later(ids, "dismiss", { reason: reason || "Not a problem" }, ids.length > 1 ? `${ids.length} dismissed` : "Dismissed");

  const rows = shown.map((f) => {
    const hits = f.personas_hit && f.personas_hit.length ? f.personas_hit : f.persona ? [f.persona] : [];
    return listRow({
      id: f.id, title: f.title, S, selectable: true, selected: S.sel.has(f.id), cls: blockedOf(f) ? "blocked" : null,
      onSelect: (on) => { if (on) S.sel.set(f.id, true); else S.sel.delete(f.id); redraw(); },
      onOpen: () => C.open(f.id),
      lines: [
        h("span", { class: "r-t1" }, h("span", { class: "cv-sev", "data-sev": String(f.severity), title: SEV[f.severity] }), h("span", { class: "r-ttl" }, f.title)),
        h("span", { class: "r-meta" },
          isNew(f) ? h("span", { class: "r-new" }, "new") : null,
          f.component ? h("span", {}, f.component) : null,
          hits.length ? h("span", {}, hits.slice(0, 2).map(personaName).join(", ") + (hits.length > 2 ? ` +${hits.length - 2}` : "")) : f.by ? h("span", {}, f.by === "you" ? "you" : f.by) : null,
          f.occurrences > 1 ? h("span", {}, `seen ${f.occurrences}×`) : null),
        flagLine(f)],
      side: h("span", {}, ago(f.created || f.stage_since)),
      swipe: { right: ["Reproduce", () => reproduce([f.id])], left: ["Dismiss", () => dismiss([f.id], "Dismissed from the list")] },
    });
  });

  const n = S.sel.size;
  let bar = null;
  if (n) {
    const ids = [...S.sel.keys()];
    if (S.dismissing) {
      const why = h("input", { class: "cv-in", type: "text", placeholder: "Why is it not a problem?", "aria-label": "Why is it not a problem?" });
      bar = h("div", { class: "cv-bulk", role: "toolbar" }, why,
        h("button", { class: "cv-btn", type: "button", onclick: () => { S.dismissing = false; redraw(); } }, "Cancel"),
        h("button", { class: "cv-btn crit", type: "button", onclick: () => { if (!why.value.trim()) { why.focus(); toast("A reason, first: it teaches the personas"); return; } S.dismissing = false; dismiss(ids, why.value.trim()); } }, "Dismiss"));
      setTimeout(() => why.focus(), 0);
    } else {
      bar = bulkBar(n,
        h("button", { class: "cv-btn", type: "button", onclick: () => { S.dismissing = true; redraw(); } }, "Dismiss"),
        h("button", { class: "cv-btn pri", type: "button", onclick: () => reproduce(ids) }, "Reproduce"));
    }
  } else S.dismissing = false;

  const allSel = shown.length && shown.every((f) => S.sel.has(f.id));
  return h("div", { class: "cv-step-problems" },
    stepHead(C, "Problems", all.length, "Pick what to reproduce; dismiss what is not a problem.",
      h("button", { class: "cv-btn", type: "button", onclick: () => newProblem(C) }, "+ New problem")),
    all.length ? chips : null,
    shown.length ? [
      h("div", { class: "cv-sec" },
        h("label", { class: "r-ck", style: { display: "inline-flex", alignItems: "center", gap: "10px", minHeight: "44px", cursor: "pointer" } },
          h("input", { type: "checkbox", style: { width: "20px", height: "20px", accentColor: "var(--accent)" }, checked: allSel ? true : null, onchange: (e) => { shown.forEach((f) => (e.target.checked ? S.sel.set(f.id, true) : S.sel.delete(f.id))); redraw(); } }),
          h("span", { class: "cv-label" }, shown.length === all.length ? `All ${shown.length}` : `${shown.length} of ${all.length}`)),
        h("span", { class: "cv-sp" }),
        window.matchMedia && window.matchMedia("(pointer: coarse)").matches ? h("span", { class: "cv-meta" }, "Swipe right to reproduce, left to dismiss") : null),
      h("ul", { class: "cv-list" }, rows)]
      : all.length ? none("Nothing matches these filters.", h("button", { class: "cv-btn", type: "button", onclick: () => { F.sev.clear(); F.kind.clear(); F.comp.clear(); F.persona.clear(); F.fresh = false; if (from) location.hash = C.href("problems"); else redraw(); } }, "Clear the filters"))
        : none(from ? "That run found nothing new to triage." : "No problems wait for you. Play a persona to find some, or write one down.",
          h("a", { class: "cv-btn pri", href: C.href("personas") }, "Run personas"),
          h("button", { class: "cv-btn", type: "button", onclick: () => newProblem(C) }, "+ New problem")),
    bar);
}

/** "+ New problem": a sentence becomes a card (/ask), or a card by hand. */
export function newProblem(C) {
  const sh = sheetShell({ label: "New problem", data: { kind: "new" } });
  sh.setHead(h("span", { class: "cv-label" }, "Problems"), h("h2", {}, "New problem"));
  const ta = h("textarea", { class: "cv-ta", rows: "4", placeholder: "What goes wrong, and where? E.g. “Search misses accented words in the Python client”", "aria-label": "Describe the problem", style: { fontFamily: "var(--sans)", fontSize: "16px", minHeight: "120px" } });
  const out = h("div", { "aria-live": "polite" });
  const title = h("input", { type: "text", id: "cv-ah-t" }); // No maxlength: pasted text is not cut silently; the server says when it is too long.
  const detail = h("textarea", { class: "cv-ta", id: "cv-ah-d", style: { minHeight: "80px" } });
  const kind = h("select", { id: "cv-ah-k" }, KINDS.map((k) => h("option", { value: k }, k)));
  const sev = h("select", { id: "cv-ah-s" }, ["blocker", "major", "minor", "papercut"].map((k) => h("option", { value: k, selected: k === "major" ? true : null }, k)));
  const comps = (C.S.d.components || []).map((c) => (typeof c === "string" ? c : c && c.name)).filter(Boolean);
  const comp = h("select", { id: "cv-ah-c" }, h("option", { value: "" }, "Let it decide"), comps.map((c) => h("option", { value: c }, c)));
  const hand = h("details", { class: "cv-fold" }, h("summary", {}, "Write it by hand", h("span", { class: "s" }, "title, kind, severity")),
    h("div", { class: "cv-f" }, h("label", { for: "cv-ah-t" }, "Title"), title),
    h("div", { class: "cv-f" }, h("label", { for: "cv-ah-d" }, "What happens, what you expected"), detail),
    h("div", { class: "cv-edit" }, h("div", { class: "cv-f" }, h("label", { for: "cv-ah-k" }, "Kind"), kind), h("div", { class: "cv-f" }, h("label", { for: "cv-ah-s" }, "Severity"), sev), comps.length ? h("div", { class: "cv-f" }, h("label", { for: "cv-ah-c" }, "Component"), comp) : null));
  put(sh.body, h("section", {}, h("p", { class: "cv-intro", style: { margin: "0 0 10px" } }, "Say it in a sentence: an agent reads the project and writes the card, which goes on to Reproducing."), ta), out, hand);
  const go = h("button", { class: "cv-btn pri", type: "button" }, "Write the card");
  put(sh.foot, h("div", { class: "acts" }, go));
  go.addEventListener("click", async () => {
    go.disabled = true;
    try {
      let r;
      if (hand.open && title.value.trim()) {
        const body = { repo: C.repo(), title: title.value.trim(), detail: detail.value.trim(), kind: kind.value, severity: sev.value, pick: true };
        if (comp.value) body.component = comp.value;
        r = await C.api("/api/converge/findings", { method: "POST", body });
      } else {
        const text = ta.value.trim();
        if (!text) { ta.focus(); go.disabled = false; return; }
        put(out, h("p", { class: "cv-note" }, h("span", { class: "cv-live" }, "Writing the card: it reads the project first, up to a minute")));
        r = await C.api("/api/converge/ask", { method: "POST", body: { repo: C.repo(), text } });
      }
      const f = (r && r.finding) || {};
      const like = r && r.similar && r.similar.id ? r.similar : null;
      toast(like ? `${f.title || "The card"}: on to Reproducing. It looks like “${like.title}” (${like.stage}): dismiss one if they are the same` : `${f.title || "The card"}: on to Reproducing`);
      sh.close();
      C.S.sig = ""; await C.load();
      if (f.id) C.open(f.id);
    } catch (err) {
      if (err.status === 404 || err.status === 405) { put(out, h("p", { class: "cv-note" }, "This server cannot write cards from a sentence; write it by hand below.")); title.value = ta.value.split(/[.\n]/)[0].slice(0, 200); detail.value = ta.value; hand.open = true; }
      else put(out, h("p", { class: "cv-note" }, `The card was not written: ${err.message || err}`));
    }
    go.disabled = false;
  });
  setTimeout(() => ta.focus(), 50);
}
