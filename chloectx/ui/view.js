// ChloeCtx's page in ChloeDB's command center (#/app/chloectx). The command
// center loads this file through chloedb-server, which forwards
// /v1/apps/chloectx/... to `chloectx app`, and runs it with its own helpers:
// render(el, ctx) gets ctx.core (h, card, table, fmt, ...), ctx.api (the
// app's API) and ctx.core.api (the server's). It imports nothing, so the app
// serves it as one file.
//
//   #/app/chloectx              Context: the spaces, and how an agent uses them
//   #/app/chloectx/space/<name> one space: ask it (what went in and why), its notes
//   #/app/chloectx/seen         Seen before: firing alerts beside past incidents

export default { render };

const CSS = `
.cx-top { display: flex; gap: 12px; align-items: center; flex-wrap: wrap; margin-bottom: 16px; }
.cx-top .grow { min-width: 0; }
.cx-top h2 { margin: 0; font-size: 18px; }
.cx-lede { color: var(--text-2); font-size: 13px; margin: 2px 0 0; max-width: 760px; }
.cx-crumbs { font-size: 12.5px; color: var(--muted); margin-bottom: 4px; }
.cx-crumbs a { color: var(--accent); text-decoration: none; }
.cx-status { display: flex; gap: 6px 16px; flex-wrap: wrap; font-size: 12px; color: var(--muted); margin: -6px 0 14px; }
.cx-status b { color: var(--text-2); font-weight: 600; }
.cx-spaces { display: grid; grid-template-columns: repeat(auto-fill, minmax(260px, 1fr)); gap: 12px; margin-bottom: 16px; }
.cx-space { display: block; padding: 14px 16px; text-decoration: none; color: inherit; }
.cx-space:hover { border-color: var(--line-2); box-shadow: var(--shadow-2); }
.cx-space h4, .cx-alert h4, .cx-inc h4, .cx-match h5 { text-transform: none; letter-spacing: 0; }
.cx-space h4 { margin: 0; font-size: 14.5px; }
.cx-ask textarea.input, .cx-form textarea.input, .cx-form input.input { font-family: var(--sans); }
.cx-space p { margin: 6px 0 10px; color: var(--text-2); font-size: 12.5px; line-height: 1.45; min-height: 18px; }
.cx-counts { display: flex; gap: 4px 12px; flex-wrap: wrap; font-size: 12px; color: var(--muted); }
.cx-counts b { color: var(--text); font-weight: 600; }
.cx-split { display: grid; grid-template-columns: minmax(0, 1.45fr) minmax(0, 1fr); gap: 16px; align-items: start; }
.cx-stack { display: grid; gap: 16px; }
.cx-ask { display: grid; gap: 10px; }
.cx-ask textarea.input { min-height: 64px; resize: vertical; font-size: 14px; }
.cx-askrow { display: flex; gap: 12px; align-items: center; flex-wrap: wrap; }
.cx-askrow input[type=range] { flex: 1 1 160px; min-width: 120px; accent-color: var(--accent); }
.cx-askrow .cx-bud { font-variant-numeric: tabular-nums; font-size: 12.5px; color: var(--text-2); min-width: 120px; }
.cx-meter { display: flex; height: 14px; border-radius: 7px; overflow: hidden; background: var(--surface-3); margin: 4px 0 6px; }
.cx-meter > i { display: block; height: 100%; border-right: 1px solid var(--surface); min-width: 2px; }
.cx-meter > i:last-child { border-right: 0; }
.cx-legend { display: flex; gap: 4px 14px; flex-wrap: wrap; font-size: 12px; color: var(--muted); }
.cx-legend span { display: inline-flex; align-items: center; gap: 6px; }
.cx-legend i { width: 10px; height: 10px; border-radius: 3px; display: inline-block; }
.cx-facts { display: flex; gap: 4px 18px; flex-wrap: wrap; margin: 12px 0 4px; font-size: 12.5px; color: var(--text-2); }
.cx-facts b { font-size: 15px; color: var(--text); font-variant-numeric: tabular-nums; margin-right: 4px; }
.cx-sub { margin: 16px 0 6px; font-size: 12px; font-weight: 600; color: var(--text-2); text-transform: uppercase; letter-spacing: .04em; }
.cx-list { list-style: none; margin: 0; padding: 0; }
.cx-list > li { display: grid; grid-template-columns: 8px minmax(0, 1fr) auto; gap: 10px; padding: 9px 0; border-top: 1px solid var(--line); align-items: start; }
.cx-list > li:first-child { border-top: 0; }
.cx-list > li.plain { grid-template-columns: minmax(0, 1fr) auto; }
.cx-sw { width: 8px; height: 100%; min-height: 32px; border-radius: 4px; }
.cx-text { font-size: 13.5px; line-height: 1.45; overflow-wrap: anywhere; }
.cx-text.old { color: var(--muted); text-decoration: line-through; text-decoration-color: var(--line-2); }
.cx-why { display: flex; gap: 4px 10px; flex-wrap: wrap; margin-top: 4px; font-size: 11.5px; color: var(--muted); }
.cx-why .was { color: var(--text-2); }
.cx-why .was s { color: var(--muted); }
.cx-tok { font-size: 12px; color: var(--muted); font-variant-numeric: tabular-nums; white-space: nowrap; text-align: right; }
.cx-acts { display: flex; gap: 4px; }
.cx-acts .btn { opacity: .75; }
.cx-acts .btn:hover { opacity: 1; }
.cx-context { margin-top: 12px; }
.cx-context summary { cursor: pointer; font-size: 12.5px; color: var(--text-2); }
.cx-context pre { white-space: pre-wrap; overflow-wrap: anywhere; max-height: 360px; overflow: auto; margin-top: 8px; font-size: 12px; }
.cx-form { display: grid; gap: 10px; }
.cx-form label { display: grid; gap: 4px; font-size: 12px; color: var(--text-2); }
.cx-form textarea.input { min-height: 60px; resize: vertical; }
.cx-form .cx-two { display: grid; grid-template-columns: 1fr 1fr; gap: 10px; }
.cx-hint { font-size: 12px; color: var(--muted); }
.cx-code { white-space: pre-wrap; overflow-wrap: anywhere; font-size: 12px; margin: 6px 0 12px; }
.cx-alert { display: grid; grid-template-columns: minmax(0, 1fr) minmax(0, 1.25fr); gap: 18px; padding: 16px 0; border-top: 1px solid var(--line); }
.cx-alert:first-child { border-top: 0; padding-top: 4px; }
.cx-alert h4 { margin: 6px 0 2px; font-size: 15px; }
.cx-alert .cx-desc { font-size: 12.5px; color: var(--text-2); line-height: 1.45; margin: 6px 0; }
.cx-val { font-size: 12.5px; font-variant-numeric: tabular-nums; }
.cx-val b { font-size: 16px; }
.cx-where { font-size: 12px; color: var(--muted); margin-top: 4px; }
.cx-match { padding: 10px 12px; border: 1px solid var(--line); border-radius: var(--r-sm); background: var(--surface-2); margin-bottom: 8px; }
.cx-match.best { border-color: var(--accent); background: var(--accent-soft); }
.cx-match h5 { margin: 0; font-size: 13.5px; display: flex; gap: 8px; align-items: center; flex-wrap: wrap; }
.cx-match dl { display: grid; grid-template-columns: 62px minmax(0, 1fr); gap: 3px 8px; margin: 8px 0 0; font-size: 12.5px; line-height: 1.4; }
.cx-match dt { color: var(--muted); }
.cx-match dd { margin: 0; overflow-wrap: anywhere; }
.cx-match dd.fix { font-weight: 600; }
.cx-pct { display: inline-flex; align-items: center; gap: 6px; font-size: 11.5px; color: var(--text-2); font-variant-numeric: tabular-nums; }
.cx-pct > span { width: 46px; height: 5px; border-radius: 3px; background: var(--surface-3); overflow: hidden; display: inline-block; }
.cx-pct > span > i { display: block; height: 100%; background: var(--accent); }
.cx-hand { display: flex; gap: 8px; align-items: center; flex-wrap: wrap; margin-top: 10px; }
.cx-inc { padding: 12px 0; border-top: 1px solid var(--line); }
.cx-inc:first-child { border-top: 0; padding-top: 0; }
.cx-inc h4 { margin: 0 0 4px; font-size: 14px; display: flex; gap: 8px; align-items: center; flex-wrap: wrap; }
.cx-inc dl { display: grid; grid-template-columns: 62px minmax(0, 1fr); gap: 3px 8px; margin: 6px 0 0; font-size: 12.5px; line-height: 1.4; }
.cx-inc dt { color: var(--muted); }
.cx-inc dd { margin: 0; overflow-wrap: anywhere; }
.cx-inc dd s { color: var(--muted); }
.cx-drafts { display: flex; gap: 6px; flex-wrap: wrap; }
@media (max-width: 900px) {
  .cx-split, .cx-alert { grid-template-columns: minmax(0, 1fr); }
  .cx-form .cx-two { grid-template-columns: minmax(0, 1fr); }
}`;

const SECTION = {
  rule: ["var(--s7)", "rule", "Rules go in first, whatever the question"],
  note: ["var(--s1)", "note", "A note the searches found for the question"],
  timeline: ["var(--s3)", "timeline", "Every note, oldest first: the question asks about the whole history"],
  reply: ["var(--s4)", "earlier reply", "A past exchange whose reply answers something like this"],
  excerpt: ["var(--s5)", "excerpt", "A passage of a past exchange the searches found"],
};
const KINDS = [["decision", "Decision"], ["rule", "Rule"], ["fact", "Fact"], ["todo", "To do"], ["note", "Note"]];
const KIND_LABEL = Object.fromEntries(KINDS.map(([k, l]) => [k, l]));
const LEFT = { superseded: ["Replaced", "A later note replaced it; the context shows the change on the note that replaced it"], uncertain: ["Uncertain", "Marked uncertain: kept out unless asked for"], budget: ["No room", "Found, but the budget was spent on closer matches"] };
const day = (secs) => (secs ? new Date(secs * 1000).toLocaleDateString(undefined, { year: "numeric", month: "short", day: "numeric" }) : "–");

function render(el, ctx) {
  const { h } = ctx.core;
  el.append(h("style", {}, CSS));
  const root = h("div");
  el.append(root);
  let stop = null;
  const show = (path) => {
    try { stop?.(); } catch (e) { console.warn(e); }
    root.replaceChildren();
    const main = el.closest("main");
    if (main) main.scrollTop = 0;
    stop = path[0] === "seen" ? seenPage(root, ctx) : path[0] === "space" && path[1] ? spacePage(root, ctx, path[1]) : homePage(root, ctx);
  };
  ctx.onPath(show);
  show(ctx.path());
  return () => stop?.();
}

// ---------------------------------------------------------------- shared bits

function tabs(ctx, on) {
  const { seg } = ctx.core;
  return seg([["context", "Context"], ["seen", "Seen before"]], on, (v) => { location.hash = v === "seen" ? ctx.href("seen") : ctx.href(); });
}

function header(ctx, on, title, lede, tools, crumbs) {
  const { h } = ctx.core;
  return h("div", { class: "cx-top" },
    h("div", { class: "grow" }, crumbs ? h("div", { class: "cx-crumbs" }, crumbs) : null, h("h2", {}, title), lede ? h("p", { class: "cx-lede" }, lede) : null),
    tools, tabs(ctx, on));
}

function kindBadge(ctx, kind) {
  return ctx.core.h("span", { class: "badge" + (kind === "rule" ? " accent" : "") }, KIND_LABEL[kind] || kind);
}

function modelLine(core, info) {
  const m = info?.model || {};
  if (m.how === "api") return `distilled by ${m.model} through Anthropic's API`;
  if (m.how === "cli") return `distilled by the claude CLI (${m.model})`;
  return "not distilled: start chloectx app with ANTHROPIC_API_KEY, or with the claude CLI on its PATH";
}

/** A note's history (what it replaced and what replaced it), in a drawer. */
async function historyDrawer(ctx, space, id) {
  const { h, drawer, toast } = ctx.core;
  let j;
  try { j = await ctx.api(`/v1/spaces/${encodeURIComponent(space)}/notes/${encodeURIComponent(id)}`); }
  catch (e) { toast(e.message, "bad"); return; }
  const row = (n, now) => h("li", { class: "plain" },
    h("div", {}, h("div", { class: "cx-text" + (n.tag === "superseded" && !now ? " old" : "") }, n.text),
      h("div", { class: "cx-why" }, kindBadge(ctx, n.kind), h("span", {}, day(n.ts)), h("span", { class: "mono" }, n.id), n.tag === "superseded" ? h("span", {}, n.superseded_by?.length ? "replaced" : "retired") : h("span", {}, "current"))),
    h("span"));
  drawer({
    title: "A note's history",
    sub: "Each version is kept; a context shows only the current one, with what it changed",
    body: h("div", {},
      j.later.length ? [h("div", { class: "cx-sub" }, "Replaced by"), h("ul", { class: "cx-list" }, j.later.map((n) => row(n, n.tag !== "superseded")))] : null,
      h("div", { class: "cx-sub" }, "This note"), h("ul", { class: "cx-list" }, row(j.note, true)),
      j.earlier.length ? [h("div", { class: "cx-sub" }, "It replaced"), h("ul", { class: "cx-list" }, j.earlier.map((n) => row(n, false)))] : null),
  });
}

// ---------------------------------------------------------------- Context: the spaces

function homePage(root, ctx) {
  const { h, empty, failure, fmt, sqlBlock } = ctx.core;
  const newBtn = h("button", { class: "btn primary", onclick: () => newSpace(ctx) }, ctx.core.icon("plus"), "New space");
  root.append(header(ctx, "context", "Context",
    "Memory spaces any agent writes notes to and asks for context. An answer is found by search alone, within the token budget the agent gives, and says why each note is in it. Each space is a table of this cluster.", newBtn));
  const status = h("div", { class: "cx-status" });
  const list = h("div", { class: "cx-spaces" }, h("div", { class: "muted small" }, "Loading the spaces…"));
  const how = h("div");
  root.append(status, list, how);
  (async () => {
    let info, spaces;
    try { [info, spaces] = await Promise.all([ctx.api("/v1/info"), ctx.api("/v1/spaces")]); }
    catch (e) { list.replaceChildren(failure(e, "The spaces")); return; }
    status.replaceChildren(
      h("span", {}, h("b", {}, "Search "), `BM25 and ${info.embedder} vectors (${info.dim} dimensions)`),
      h("span", {}, h("b", {}, "Conversations "), modelLine(ctx.core, info)),
      ...(info.distill?.running ? [h("span", {}, `${info.distill.running} distilling now`)] : []));
    const ss = spaces.spaces || [];
    if (!ss.length) {
      list.replaceChildren(empty("layers", "No space yet", "A space holds one team's or one agent's notes: make one, then write to it from an agent.", h("button", { class: "btn sm primary", onclick: () => newSpace(ctx) }, "New space")));
      list.style.display = "block";
    } else {
      list.replaceChildren(...ss.map((s) => {
        const by = s.by_kind || {};
        const notes = ["decision", "preference", "fact", "todo", "note"].reduce((a, k) => a + (by[k] || 0), 0);
        const said = (by.prompt || 0) + (by.answer || 0);
        const href = s.kind === "incidents" ? ctx.href("seen") : ctx.href("space", s.name);
        return h("a", { class: "card cx-space", href },
          h("div", { class: "row", style: { justifyContent: "space-between", gap: "8px" } }, h("h4", {}, s.title || s.name), h("span", { class: "badge mono" }, s.table)),
          h("p", {}, s.description || ""),
          h("div", { class: "cx-counts" },
            s.kind === "incidents" ? h("span", {}, h("b", {}, fmt.n(by.note || 0)), " incidents") : h("span", {}, h("b", {}, fmt.n(notes)), " notes"),
            said ? h("span", {}, h("b", {}, fmt.n(said)), " messages") : null,
            by.decision ? h("span", {}, fmt.plural(by.decision, "decision")) : null,
            by.preference ? h("span", {}, fmt.plural(by.preference, "rule")) : null,
            s.created ? h("span", {}, "since " + day(s.created)) : null));
      }));
    }
    const first = ss.find((s) => s.kind !== "incidents")?.name || "billing";
    const base = `${location.origin}/v1/apps/chloectx/v1/spaces/${first}`;
    how.replaceChildren(ctx.core.card({
      title: "From an agent",
      sub: "through this server, with a key that may write (or straight to chloectx app)",
      body: h("div", {},
        h("div", { class: "cx-hint" }, "Write down what it learned (replaces names the note it updates):"),
        h("pre", { class: "code cx-code" }, `curl -s ${base}/notes -H "authorization: Bearer $KEY" \\\n  -d '{"kind": "decision", "text": "Refunds over 1000 EUR need a second approver.", "entities": ["refunds"]}'`),
        h("div", { class: "cx-hint" }, "Hand over a conversation; a model writes its notes down when one is set:"),
        h("pre", { class: "code cx-code" }, `curl -s ${base}/exchanges -H "authorization: Bearer $KEY" \\\n  -d '{"user": "...", "assistant": "..."}'`),
        h("div", { class: "cx-hint" }, "Ask for context before a task, within a budget in tokens:"),
        h("pre", { class: "code cx-code" }, `curl -s ${base}/ask -H "authorization: Bearer $KEY" \\\n  -d '{"question": "Who approves a 700 EUR refund?", "budget": 2000}'`),
        h("div", { class: "cx-hint" }, "The records are rows of the space's table, so SQL reads them too:"),
        sqlBlock(`SELECT kind, text, ts FROM ctx_${first} WHERE kind IN ('decision', 'preference') AND tag != 'superseded' ORDER BY ts DESC LIMIT 20`)),
    }));
  })();
  return null;
}

function newSpace(ctx) {
  const { h, modal, toast } = ctx.core;
  const name = h("input", { class: "input", placeholder: "billing", autocomplete: "off", spellcheck: "false" });
  const title = h("input", { class: "input", placeholder: "Billing service" });
  const desc = h("textarea", { class: "input", rows: 2, placeholder: "What the agents of the billing service know" });
  const go = h("button", { class: "btn primary" }, "Make it");
  const m = modal({
    title: "New space",
    body: h("div", { class: "cx-form" },
      h("label", {}, "Name (lowercase letters, digits and _; its table is ctx_<name>)", name),
      h("label", {}, "Title", title), h("label", {}, "What it holds", desc)),
    foot: [h("button", { class: "btn", onclick: () => m.close() }, "Cancel"), go],
  });
  go.onclick = async () => {
    go.disabled = true;
    try {
      const s = await ctx.api("/v1/spaces", { body: { name: name.value.trim(), title: title.value.trim(), description: desc.value.trim() } });
      m.close();
      location.hash = ctx.href("space", s.name);
    } catch (e) { toast(e.message, "bad"); go.disabled = false; }
  };
}

// ---------------------------------------------------------------- one space

function spacePage(root, ctx, name) {
  const { h, card, fmt, toast, empty, failure, icon } = ctx.core;
  const sp = `/v1/spaces/${encodeURIComponent(name)}`;
  const head = h("div");
  root.append(head);
  head.append(header(ctx, "context", name, null, null, [h("a", { href: ctx.href() }, "Context"), " / ", name]));
  ctx.api("/v1/spaces").then((j) => {
    const s = (j.spaces || []).find((x) => x.name === name);
    if (s) head.replaceChildren(header(ctx, "context", s.title || name, s.description || null, null, [h("a", { href: ctx.href() }, "Context"), " / ", h("span", { class: "mono" }, s.table)]));
  }).catch(() => {});

  // The playground.
  const q = h("textarea", { class: "input", placeholder: "What an agent would ask before a task: \"Who approves a 700 EUR refund?\"" });
  const budget = h("input", { type: "range", min: 500, max: 16000, step: 500, value: 4000, "aria-label": "Token budget" });
  const budLabel = h("span", { class: "cx-bud" });
  const setBud = () => { budLabel.textContent = `budget ${fmt.n(+budget.value)} tokens`; };
  setBud();
  const askBtn = h("button", { class: "btn primary" }, "Ask");
  const out = h("div", {}, h("div", { class: "cx-hint", style: { marginTop: "6px" } }, "The answer shows what went into the context, why, and what was left out."));
  let last = "";
  const ask = async () => {
    const question = q.value.trim();
    if (!question) { q.focus(); return; }
    askBtn.disabled = true;
    try { last = question; drawAnswer(ctx, out, await ctx.api(`${sp}/ask`, { body: { question, budget: +budget.value } }), name); }
    catch (e) { out.replaceChildren(failure(e, "The context")); }
    askBtn.disabled = false;
  };
  askBtn.onclick = ask;
  q.onkeydown = (e) => { if (e.key === "Enter" && (e.metaKey || e.ctrlKey)) { e.preventDefault(); ask(); } };
  budget.oninput = setBud;
  budget.onchange = () => { if (last && q.value.trim() === last) ask(); };
  const play = card({ title: "Ask for context", sub: "what an agent gets, by search alone", body: h("div", { class: "cx-ask" }, q, h("div", { class: "cx-askrow" }, budget, budLabel, askBtn), out) });

  // The notes.
  let all = false;
  const notesBody = h("div", {}, h("div", { class: "muted small" }, "Loading…"));
  const loadNotes = async () => {
    let j;
    try { j = await ctx.api(`${sp}/notes?limit=200${all ? "&all=1" : ""}`); }
    catch (e) { notesBody.replaceChildren(e.status === 404 ? empty("layers", "No such space", e.message) : failure(e, "The notes")); return; }
    const ns = j.notes || [];
    if (!ns.length) { notesBody.replaceChildren(empty("note", "No notes yet", "Add one here, or write to the space from an agent.")); return; }
    notesBody.replaceChildren(h("ul", { class: "cx-list" }, ns.map((n) => h("li", { class: "plain" },
      h("div", {},
        h("div", { class: "cx-text" + (n.tag === "superseded" ? " old" : "") }, n.text),
        h("div", { class: "cx-why" }, kindBadge(ctx, n.kind), h("span", {}, day(n.ts)),
          n.supersedes?.length ? h("span", { class: "was" }, "updates an earlier note") : null,
          n.tag === "superseded" ? h("span", {}, n.superseded_by?.length ? "replaced" : "retired") : n.tag === "done" ? h("span", {}, "done") : null)),
      h("div", { class: "cx-acts" },
        h("button", { class: "btn ghost icon sm", title: "History", "aria-label": "History", onclick: () => historyDrawer(ctx, name, n.id) }, icon("clock")),
        n.tag === "superseded" ? null : h("button", { class: "btn ghost icon sm", title: "Replace with a new version", "aria-label": "Replace", onclick: () => noteForm(ctx, name, n, loadNotes) }, icon("edit")),
        n.tag === "superseded" ? null : h("button", { class: "btn ghost icon sm", title: "Retire: leave it out of every context", "aria-label": "Retire", onclick: async () => {
          if (!(await ctx.core.confirm({ title: "Retire this note?", body: `“${n.text}” stays in the space's history but no context shows it again.`, ok: "Retire", danger: true }))) return;
          try { await ctx.api(`${sp}/notes/${encodeURIComponent(n.id)}`, { method: "DELETE" }); loadNotes(); } catch (e) { toast(e.message, "bad"); }
        } }, icon("x")))))));
  };
  const which = ctx.core.seg([["live", "Current"], ["all", "With history"]], "live", (v) => { all = v === "all"; loadNotes(); });
  const notes = card({ title: "Notes", tools: [which, h("button", { class: "btn sm", onclick: () => noteForm(ctx, name, null, loadNotes) }, icon("plus"), "Add")], body: notesBody });

  // A conversation, handed over.
  const user = h("textarea", { class: "input", rows: 3, placeholder: "What the user said" });
  const asst = h("textarea", { class: "input", rows: 3, placeholder: "What the agent answered" });
  const send = h("button", { class: "btn" }, "Hand it over");
  const said = h("div", { class: "cx-hint" });
  ctx.api("/v1/info").then((i) => { said.textContent = `Kept as two messages, and ${modelLine(ctx.core, i)}.`; }).catch(() => {});
  send.onclick = async () => {
    if (!user.value.trim() && !asst.value.trim()) return;
    send.disabled = true;
    try {
      const j = await ctx.api(`${sp}/exchanges`, { body: { user: user.value, assistant: asst.value } });
      user.value = ""; asst.value = "";
      toast(j.distilling ? "Kept; its notes follow in a few seconds" : "Kept as messages", "ok");
      if (j.distilling) waitDistill(ctx, loadNotes);
    } catch (e) { toast(e.message, "bad"); }
    send.disabled = false;
  };
  const conv = card({ title: "Hand over a conversation", sub: "a model writes down its decisions, rules and facts", body: h("div", { class: "cx-form" }, user, asst, h("div", { class: "row", style: { justifyContent: "space-between", gap: "10px" } }, said, send)) });

  root.append(h("div", { class: "cx-split" }, play, h("div", { class: "cx-stack" }, notes, conv)));
  loadNotes();
  setTimeout(() => q.focus(), 0);
  return null;
}

/** Waits for background distillations to end (up to a minute), then reloads. */
async function waitDistill(ctx, then) {
  for (let i = 0; i < 30; i++) {
    await new Promise((r) => setTimeout(r, 2000));
    try {
      const j = await ctx.api("/v1/info");
      if (!j.distill?.running) {
        if (j.distill?.last_error && j.distill.failed) ctx.core.toast("Distilling failed: " + j.distill.last_error, "bad", 8000);
        then();
        return;
      }
    } catch { return; }
  }
}

function drawAnswer(ctx, out, a, space) {
  const { h, fmt, empty } = ctx.core;
  const items = a.items || [];
  const used = a.tokens || 0;
  const meter = h("div", { class: "cx-meter", title: `${fmt.n(used)} of ${fmt.n(a.budget)} tokens` },
    items.filter((it) => it.tokens).map((it) => h("i", { style: { width: Math.max(0.4, (it.tokens / Math.max(a.budget, used)) * 100) + "%", background: (SECTION[it.section] || SECTION.note)[0] }, title: `${(SECTION[it.section] || SECTION.note)[1]}: ${fmt.n(it.tokens)} tokens` })));
  const bySection = {};
  for (const it of items) bySection[it.section] = (bySection[it.section] || 0) + 1;
  const legend = h("div", { class: "cx-legend" },
    Object.entries(bySection).map(([s, n]) => h("span", { title: (SECTION[s] || SECTION.note)[2] }, h("i", { style: { background: (SECTION[s] || SECTION.note)[0] } }), `${n} ${(SECTION[s] || SECTION.note)[1]}${n === 1 ? "" : "s"}`)),
    h("span", {}, h("i", { style: { background: "var(--surface-3)" } }), `${fmt.n(Math.max(0, a.budget - used))} tokens left`));
  const facts = h("div", { class: "cx-facts" },
    h("span", {}, h("b", {}, fmt.n(used)), `of ${fmt.n(a.budget)} tokens`),
    h("span", {}, h("b", {}, fmt.n(items.length)), items.length === 1 ? "record" : "records"),
    h("span", {}, h("b", {}, fmt.ms(a.ms)), "to build"),
    h("span", {}, h("b", {}, "0"), "model calls"));
  const picks = items.length ? h("ul", { class: "cx-list" }, items.map((it) => {
    const [color, label, why] = SECTION[it.section] || SECTION.note;
    return h("li", {},
      h("span", { class: "cx-sw", style: { background: color }, title: why }),
      h("div", {},
        h("div", { class: "cx-text" }, it.kind === "user" || it.kind === "assistant" ? h("span", { class: "muted" }, it.kind === "user" ? "User: " : "Reply: ") : null, it.text),
        h("div", { class: "cx-why" },
          h("span", { title: why }, label), it.kind && it.kind !== "user" && it.kind !== "assistant" ? kindBadge(ctx, it.kind) : null,
          it.bm25 != null ? h("span", { title: "Its BM25 score for the question's words" }, `words ${it.bm25.toFixed(2)}`) : null,
          it.cos != null ? h("span", { title: "Cosine similarity of its vector to the question's" }, `meaning ${it.cos.toFixed(2)}`) : null,
          it.bm25 == null && it.cos == null && it.section !== "rule" ? h("span", {}, it.section === "timeline" ? "every note" : "in the space's few notes") : null,
          it.replaced ? h("span", { class: "was" }, "shows the change: ", h("s", {}, it.replaced.replace(/^changed [0-9-]+; was: /, ""))) : null,
          h("span", { class: "mono" }, day(it.ts)),
          h("a", { href: "javascript:void 0", onclick: () => historyDrawer(ctx, space, it.id) }, "history"))),
      h("span", { class: "cx-tok" }, it.tokens ? `${fmt.n(it.tokens)} tok` : ""));
  })) : empty("search", "Nothing found", "The space has nothing that matches: the agent gets an empty context.");
  const left = (a.left_out || []).length ? [h("div", { class: "cx-sub" }, "Left out"), h("ul", { class: "cx-list" }, a.left_out.map((r) => {
    const [label, why] = LEFT[r.why] || [r.why, ""];
    return h("li", { class: "plain" },
      h("div", {}, h("div", { class: "cx-text old" }, r.text), h("div", { class: "cx-why" }, h("span", { title: why }, label + ": " + why.toLowerCase()), h("span", {}, day(r.ts)))),
      h("span"));
  }))] : null;
  out.replaceChildren(h("div", { style: { marginTop: "8px" } }, meter, legend), facts,
    h("div", { class: "cx-sub" }, "In the context"), h("div", { class: "cx-hint", style: { marginBottom: "4px" } }, "Rules first, then the notes the searches found, dated and in order, until the budget is spent; a small space fits whole."),
    picks, h("div", {}, left),
    h("details", { class: "cx-context" }, h("summary", {}, `The context as the agent gets it (${fmt.n(a.chars)} characters)`), h("pre", { class: "code" }, a.context || "(empty)")));
}

function noteForm(ctx, space, old, done) {
  const { h, modal, toast } = ctx.core;
  const kind = h("select", { class: "select" }, KINDS.map(([k, l]) => h("option", { value: k, selected: old ? old.kind === k : k === "decision" }, l)));
  const text = h("textarea", { class: "input", rows: 3, placeholder: "One sentence that stands on its own, with its reason" }, old ? old.text : "");
  const ents = h("input", { class: "input", placeholder: "refunds, invoices (what it is about; optional)" });
  const go = h("button", { class: "btn primary" }, old ? "Replace" : "Add");
  const m = modal({
    title: old ? "A new version of the note" : "Add a note",
    body: h("div", { class: "cx-form" },
      old ? h("div", { class: "cx-hint" }, "The old version stays in the history; contexts show this one, with what changed.") : null,
      h("label", {}, "Kind", kind), h("label", {}, "Note", text), h("label", {}, "About", ents)),
    foot: [h("button", { class: "btn", onclick: () => m.close() }, "Cancel"), go],
  });
  go.onclick = async () => {
    go.disabled = true;
    try {
      const entities = ents.value.split(",").map((s) => s.trim()).filter(Boolean);
      await ctx.api(`/v1/spaces/${encodeURIComponent(space)}/notes`, { body: { kind: kind.value, text: text.value, entities, replaces: old?.id } });
      m.close();
      done();
    } catch (e) { toast(e.message, "bad"); go.disabled = false; }
  };
}

// ---------------------------------------------------------------- Seen before

function pct(ctx, v) {
  const { h } = ctx.core;
  const word = v >= 0.5 ? "close" : v >= 0.3 ? "similar" : "related";
  return h("span", { class: "cx-pct", title: `How close it is in meaning: cosine similarity ${v.toFixed(2)}` }, h("span", {}, h("i", { style: { width: Math.round(Math.max(0, Math.min(1, v)) * 100) + "%" } })), word);
}

function incidentFields(ctx, i) {
  const { h } = ctx.core;
  return h("dl", {},
    h("dt", {}, "Symptom"), h("dd", {}, i.symptom || "–"),
    h("dt", {}, "Cause"), h("dd", {}, i.cause || "–"),
    h("dt", {}, "Fix"), h("dd", { class: "fix" }, i.fix || "–"),
    (i.replaced || []).map((e) => [h("dt", {}, "Before"), h("dd", {}, h("s", {}, e.fix), h("span", { class: "muted" }, ` (until ${day(e.ts)})`))]));
}

function seenPage(root, ctx) {
  const { h, card, fmt, toast, empty, failure, every, icon } = ctx.core;
  const rec = h("button", { class: "btn primary", onclick: () => incidentForm(ctx, null, load) }, icon("plus"), "Record an incident");
  root.append(header(ctx, "seen", "Seen before",
    "The cluster's firing alerts, each beside the past incidents it looks like and the fix that worked. Hand one to the on-call agent with those incidents as its context, and keep what it found as a new incident.", rec));
  const kpis = h("div", { class: "grid g3", style: { marginBottom: "16px" } });
  const firing = h("div", {}, h("div", { class: "muted small" }, "Reading the alerts…"));
  const incidents = h("div", {}, h("div", { class: "muted small" }, "Loading…"));
  const probe = h("input", { class: "input", style: { width: "100%" }, placeholder: "Or describe what you see, and press Enter: \"p99 doubled on one shard after a deploy\"" });
  const probeOut = h("div");
  probe.onkeydown = async (e) => {
    if (e.key !== "Enter" || !probe.value.trim()) return;
    try {
      const j = await ctx.api(`/v1/incidents/match?q=${encodeURIComponent(probe.value.trim())}&n=3`);
      probeOut.replaceChildren(j.matches.length ? h("div", { style: { marginTop: "10px" } }, j.matches.map((m, i) => matchCard(ctx, m, i === 0))) : empty("search", "Nothing like it", "No recorded incident looks like this."));
    } catch (err) { probeOut.replaceChildren(failure(err, "The match")); }
  };
  root.append(kpis,
    card({ title: "Firing now", sub: "with the closest past incidents", body: h("div", {}, firing, h("div", { style: { marginTop: "12px" } }, probe, probeOut)) }),
    h("div", { style: { height: "16px" } }),
    card({ title: "Incidents", sub: "kept in the space incidents (table ctx_incidents)", body: incidents }));
  const kpi = (label, val, sub) => h("div", { class: "card kpi" }, h("div", { class: "lbl" }, label), h("div", { class: "val" }, val), h("div", { class: "delta" }, sub || ""));
  let incs = [];
  const open = new Set(), handed = new Map();
  async function load() {
    let f, l;
    try { [f, l] = await Promise.all([ctx.api("/v1/incidents/firing"), ctx.api("/v1/incidents")]); }
    catch (e) { firing.replaceChildren(failure(e, "The alerts")); return; }
    incs = l.incidents || [];
    const alerts = f.alerts || [];
    const nFiring = alerts.filter((a) => a.state === "firing").length;
    const matched = alerts.filter((a) => a.matches?.length).length;
    kpis.replaceChildren(
      kpi("Firing", fmt.n(nFiring), alerts.length > nFiring ? `${alerts.length - nFiring} pending` : "alerts of this cluster"),
      kpi("Seen before", alerts.length ? `${matched} of ${alerts.length}` : "–", `alerts with a past incident like them · matched in ${fmt.ms(f.ms)}`),
      kpi("Incidents kept", fmt.n(incs.length), fmt.plural(incs.reduce((a, i) => a + (i.replaced?.length || 0), 0), "fix", "fixes") + " replaced by a better one"));
    firing.replaceChildren(alerts.length ? h("div", {}, alerts.map((a) => alertBlock(ctx, a, open, handed))) : empty("okc", "No alert is firing", "When one fires, it shows here with the incidents it looks like."));
    incidents.replaceChildren(incs.length ? h("div", {}, incs.map((i) => h("div", { class: "cx-inc" },
      h("h4", {}, i.title, i.alert ? h("span", { class: "badge mono" }, i.alert) : null, h("span", { class: "small muted", style: { fontWeight: 400 } }, day(i.ts)),
        h("span", { class: "grow" }),
        h("button", { class: "btn xs", onclick: () => incidentForm(ctx, i, load), title: "A better fix, or a correction: the old one stays as history" }, "Better fix")),
      incidentFields(ctx, i)))) : empty("note", "No incident recorded yet", "Record one by hand, paste a postmortem for a model to draft it, or draft it from an on-call run.", h("button", { class: "btn sm primary", onclick: () => incidentForm(ctx, null, load) }, "Record an incident")));
  }
  return every(15000, load);
}

function matchCard(ctx, m, best) {
  const { h } = ctx.core;
  return h("div", { class: "cx-match" + (best ? " best" : "") },
    h("h5", {}, m.title, pct(ctx, m.match || 0), m.same_alert ? h("span", { class: "badge accent", title: "Recorded for this same alert rule" }, "same alert") : null, h("span", { class: "small muted", style: { fontWeight: 400 } }, day(m.ts))),
    incidentFields(ctx, m));
}

function alertBlock(ctx, a, open, handed) {
  const { h, fmt, pill, toast } = ctx.core;
  const since = a.since_ms ? fmt.ago(a.since_ms).replace(" ago", "") : null;
  const where = (a.where || []).slice(0, 3).map((w) => w.short || w.name || w.url || "").filter(Boolean);
  const hand = h("button", { class: "btn primary sm" }, ctx.core.icon("bolt"), "Hand to the on-call agent");
  const handOut = h("span", { class: "small muted" });
  const shown = (id) => {
    hand.disabled = true;
    handOut.replaceChildren("The agent is looking into it: ", h("a", { href: "#/oncall?run=" + encodeURIComponent(id) }, "follow the run"), " · when it ends, ", h("a", { href: "javascript:void 0", onclick: () => incidentForm(ctx, null, null, { run: id, alert: a.rule }) }, "keep what it found"));
  };
  if (handed.has(a.rule)) shown(handed.get(a.rule));
  hand.onclick = async () => {
    hand.disabled = true;
    try {
      const j = await ctx.api("/v1/incidents/handoff", { body: { alert: a.rule } });
      handed.set(a.rule, j.run?.run);
      shown(j.run?.run);
    } catch (e) { toast(e.message, "bad", 8000); hand.disabled = false; }
  };
  const ctxBox = h("details", { class: "cx-context", open: open.has(a.rule), ontoggle: (e) => { if (e.target.open) open.add(a.rule); else open.delete(a.rule); } },
    h("summary", {}, `What the agent gets with it (${fmt.n(a.context_tokens)} tokens)`),
    h("pre", { class: "code" }, a.context || "Nothing: no incident looks like it, so the agent starts from the alert alone."));
  return h("div", { class: "cx-alert" },
    h("div", {},
      h("div", { class: "row", style: { gap: "8px" } }, pill(a.state === "firing" ? "firing" : "pending", a.state), a.severity ? h("span", { class: "badge" }, a.severity) : null, since ? h("span", { class: "small muted" }, "for " + since) : null),
      h("h4", {}, a.name || a.rule),
      h("div", { class: "cx-val" }, h("b", {}, a.value == null ? "–" : fmt.si(a.value)), h("span", { class: "muted" }, ` ${a.metric} ${a.op || ""} ${a.threshold ?? ""}`)),
      a.description ? h("p", { class: "cx-desc" }, a.description) : null,
      where.length ? h("div", { class: "cx-where" }, "on " + where.join(", ")) : null,
      h("div", { class: "cx-hand" }, hand, handOut), ctxBox),
    h("div", {},
      h("div", { class: "cx-sub", style: { marginTop: 0 } }, "Seen before"),
      a.matches?.length ? a.matches.map((m, i) => matchCard(ctx, m, i === 0)) : h("div", { class: "cx-hint" }, "No recorded incident looks like this one. When it is over, keep what was found so that the next one is.")));
}

function incidentForm(ctx, old, done, from) {
  const { h, modal, toast, icon } = ctx.core;
  const f = (ph, rows) => rows ? h("textarea", { class: "input", rows, placeholder: ph }) : h("input", { class: "input", placeholder: ph });
  const title = f("Search p99 jumped during a large merge"), alert = f("p99 (the alert rule, if one fired)");
  const symptom = f("What was seen first: the metric, the shard, the numbers", 2), cause = f("Why it happened", 2), fix = f("What made it stop, and what changed so it does not come back", 2);
  const set = (i) => { title.value = i.title || ""; alert.value = i.alert || ""; symptom.value = i.symptom || ""; cause.value = i.cause || ""; fix.value = i.fix || ""; };
  if (old) set(old);
  if (from?.alert) alert.value = from.alert;
  const src = h("textarea", { class: "input", rows: 5, placeholder: "Paste a postmortem, an incident thread or notes" });
  const run = h("input", { class: "input", placeholder: "an on-call run's id" }, );
  if (from?.run) run.value = from.run;
  const drafts = h("div", { class: "cx-drafts" });
  const draftBtn = h("button", { class: "btn sm" }, icon("sparkle"), "Draft it");
  draftBtn.onclick = async () => {
    const body = src.value.trim() ? { text: src.value } : run.value.trim() ? { run: run.value.trim() } : null;
    if (!body) { src.focus(); return; }
    draftBtn.disabled = true;
    try {
      const j = await ctx.api("/v1/incidents/draft", { body, timeout: 150000 });
      const ds = j.drafts || [];
      if (!ds.length) toast("The model found no incident in it", "info");
      else set({ ...ds[0], alert: ds[0].alert || alert.value });
      drafts.replaceChildren(...(ds.length > 1 ? ds.map((d, i) => h("button", { class: "chip", onclick: () => set(d) }, `${i + 1}. ${d.title}`)) : []));
    } catch (e) { toast(e.message, "bad", 8000); }
    draftBtn.disabled = false;
  };
  const go = h("button", { class: "btn primary" }, old ? "Keep the better fix" : "Record it");
  const m = modal({
    title: old ? "A better fix for: " + old.title : "Record an incident",
    wide: true,
    body: h("div", { class: "cx-form" },
      old ? h("div", { class: "cx-hint" }, "The old fix stays as history: a match shows it struck through, under this one.") :
        h("details", { open: !!from?.run }, h("summary", { class: "small", style: { cursor: "pointer" } }, "Draft it with a model, from a postmortem or an on-call run"),
          h("div", { class: "cx-form", style: { marginTop: "8px" } }, src, h("div", { class: "row", style: { gap: "8px" } }, run, draftBtn), drafts,
            h("div", { class: "cx-hint" }, "Nothing is kept until you record it; check what the model wrote first."))),
      h("div", { class: "cx-two" }, h("label", {}, "Title", title), h("label", {}, "Alert", alert)),
      h("label", {}, "Symptom", symptom), h("label", {}, "Cause", cause), h("label", {}, "Fix", fix)),
    foot: [h("button", { class: "btn", onclick: () => m.close() }, "Cancel"), go],
  });
  go.onclick = async () => {
    go.disabled = true;
    try {
      await ctx.api("/v1/incidents", { body: { title: title.value, alert: alert.value, symptom: symptom.value, cause: cause.value, fix: fix.value, replaces: old?.id } });
      m.close();
      toast(old ? "Kept; the old fix is history now" : "Recorded", "ok");
      done?.();
    } catch (e) { toast(e.message, "bad"); go.disabled = false; }
  };
}
