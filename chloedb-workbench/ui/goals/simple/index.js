// The Simple view (Roberto, 14 Nov: between Claude Code desktop and the
// goal pages, which stay as Advanced). Many requests in flight, each one
// agent in its own worktree, on a board by where each is in the process:
// Planning, Building, Review, Done (stage.js). A card's buttons change with
// its stage, and a card that needs you is ringed and answered in place,
// with Claude Code's own options. One box starts requests, one per line if
// you like. A request opens beside the board on a desktop (#/s/r/<id>) and
// full screen on a phone; #/s/new is the box on a phone.
import { h, useStyle, btn, avatar, since, toMs, setFleet } from "/levels/common.js";
import { STAGES, stageOf, replied, titleOf, modeWords } from "/goals/simple/stage.js";
import { composer } from "/goals/simple/compose.js";
import { requestPanel } from "/goals/simple/request.js";

const enc = encodeURIComponent;
const PKEY = "wb-simple-project";
const phone = () => window.matchMedia("(max-width: 640px)").matches;
const store = (k, v) => { try { if (v === undefined) return localStorage.getItem(k); if (v === null) localStorage.removeItem(k); else localStorage.setItem(k, v); } catch { /* a private window */ } return null; };

export function mount(root, ctx) {
  useStyle("g-simple", CSS);
  let alive = true;
  const S = {
    route: parse(location.hash),
    project: store(PKEY) || "", // "" is every project
    review: new Map(), // repo path -> /api/review (ready, not_ready), and when it was read
    reviewAt: new Map(),
    reviewKey: new Map(),
    tab: null, // the phone's stage tab
    boardKey: "",
    panel: null,
    merging: new Set(),
    // After an undo, Review for a few seconds: the projects' change counts
    // are cached for 8 s and say "no change" until they are read again.
    hint: new Map(),
  };
  const el = h("div", { class: "sv" });
  root.append(el);

  const comp = composer(ctx, {
    project: () => currentProject(),
    onStarted: (agents) => {
      refreshReviewSoon(true);
      if (phone() && S.route.view === "new") ctx.go(agents.length === 1 ? `#/s/r/${agents[0].id}` : "#/s");
      else drawBoard(true);
    },
  });
  const top = h("div", { class: "sv-top" });
  const bar = h("div", { class: "sv-bar" });
  const tabs = h("nav", { class: "sv-tabs", "aria-label": "Stages" });
  const board = h("div", { class: "sv-board" });
  const side = h("aside", { class: "sv-side", "aria-label": "Request" });
  // The box and the board on the left; a request opens beside both.
  const main = h("div", { class: "sv-main" }, bar, tabs, board);
  const left = h("div", { class: "sv-left" }, top, main);
  top.append(comp.el);
  el.append(left, side);

  // ------------------------------------------------------------ the data

  function projects() { return ctx.projects() || []; }
  function currentProject() {
    const list = projects();
    return list.find((p) => p.project === S.project) || ctx.project || list[0] || null;
  }
  function projectOfAgent(a) { return projects().find((p) => p.path === a.repo) || null; }
  function needOf(a) { return ctx.needs().items.find((i) => i.agent && i.agent.id === a.id && i.kind !== "alert") || null; }
  function fleet() {
    const all = ctx.fleet();
    setFleet(all); // the same colours as everywhere else
    return all.filter((a) => !S.project || (projectOfAgent(a) || {}).project === S.project);
  }
  /** One request: its agent, stage, title and what the board shows of it. */
  function requestOf(a) {
    const p = projectOfAgent(a);
    const rv = p ? S.review.get(p.path) : null;
    const ready = rv ? (rv.ready || []).find((r) => r.agent && r.agent.id === a.id) : null;
    const nr = rv ? (rv.not_ready || []).find((r) => r.agent && r.agent.id === a.id) : null;
    const merged = p && p.merged ? p.merged[a.id] || null : null;
    const group = p && p.groups ? p.groups[a.id] : null;
    const hint = S.hint.get(a.id);
    const stage = hint && hint.until > Date.now() && !merged ? hint.stage : stageOf(a, { group, merged, changed: !!(ready || nr) || group === "ready" });
    const need = needOf(a);
    const stat = ready ? { files: ready.files, added: ready.added, removed: ready.removed } : nr ? { files: nr.files, added: nr.added, removed: nr.removed }
      : (p && (p.ready || []).find((r) => r.id === a.id)) || null;
    return { a, p, stage, need, ready, stat, merged, replied: replied(a, stage), title: titleOf(a, { reviewTitle: ready && ready.title }) };
  }

  /** Review's list per project with agents on the board: what changed on
   *  each branch (Building's +/−) and the checks of what is ready. Read
   *  again when an agent's state changes, at most every 5 s a project. */
  async function refreshReview(force = false) {
    const byRepo = new Map();
    for (const a of ctx.fleet()) { if (!byRepo.has(a.repo)) byRepo.set(a.repo, []); byRepo.get(a.repo).push(a); }
    const now = Date.now();
    await Promise.all([...byRepo].map(async ([repo, list]) => {
      const key = list.map((a) => `${a.id}${a.state}${a.running ? 1 : 0}${toMs(a.state_since) || 0}`).join();
      const fresh = S.reviewKey.get(repo) === key && now - (S.reviewAt.get(repo) || 0) < 20000;
      if (!force && (fresh || now - (S.reviewAt.get(repo) || 0) < 5000)) return;
      S.reviewAt.set(repo, now);
      S.reviewKey.set(repo, key);
      try { S.review.set(repo, await ctx.api(`/api/review?repo=${enc(repo)}`)); } catch { /* the board still shows the stages it can */ }
    }));
    if (!alive) return;
    drawBoard(true);
    // The open request too: its checks and its title come from this list.
    const open = S.panel && ctx.state.agents.find((a) => a.id === S.panel.id);
    if (open) S.panel.update(requestOf(open));
  }
  // One read waits at a time: the redraws that follow a forced one (after
  // a merge, an undo, a start) neither postpone it nor make it lazy.
  let reviewT = 0, reviewForce = false;
  function refreshReviewSoon(force) {
    if (reviewT && (reviewForce || !force)) { reviewForce = reviewForce || !!force; return; }
    clearTimeout(reviewT);
    reviewForce = reviewForce || !!force;
    reviewT = setTimeout(() => { const f = reviewForce; reviewT = 0; reviewForce = false; refreshReview(f); }, reviewForce ? 300 : 1200);
  }

  // ------------------------------------------------------------ drawing

  function draw() {
    S.route = parse(location.hash);
    const r = S.route;
    el.classList.toggle("open", r.view === "request");
    el.classList.toggle("composing", r.view === "new");
    const req = r.view === "request" ? ctx.state.agents.find((a) => a.id === r.agent) : null;
    const name = req ? req.name : null;
    ctx.crumbs([{ label: "Requests", href: "#/s" }, ...(r.view === "new" ? [{ label: "New request", href: "#/s/new" }] : []), ...(req ? [{ label: name, href: `#/s/r/${req.id}` }] : [])]);
    drawBoard(true);
    drawSide(req);
    if (r.view === "new") requestAnimationFrame(() => comp.focus());
  }

  function drawSide(a) {
    if (!a) {
      if (S.panel) { S.panel.unmount(); S.panel = null; }
      side.replaceChildren();
      side.hidden = true;
      return;
    }
    side.hidden = false;
    if (S.panel && S.panel.id === a.id) { S.panel.update(requestOf(a)); return; }
    if (S.panel) S.panel.unmount();
    S.panel = requestPanel(ctx, requestOf(a), {
      close: () => ctx.go("#/s"),
      changed: () => { refreshReviewSoon(true); },
      merge: (x) => merge(x),
      undo: (x) => undo(x),
    });
    side.replaceChildren(S.panel.el);
  }

  /** The board, drawn again only when what it shows changed, and never
   *  under an answer being picked. */
  function drawBoard(force) {
    if (!alive) return;
    const act = document.activeElement;
    if (act && main.contains(act) && (act.closest(".qc, .wait-card") || act.matches("input, textarea, select"))) return;
    if (board.querySelector(".qc-opt.sel")) return;
    const list = fleet().map(requestOf);
    const N = ctx.needs();
    // The minute is in it, so "4 min" moves on with nothing else changing.
    const key = JSON.stringify([Math.floor(Date.now() / 60000), S.project, S.tab, S.route.agent, phone(), N.items.map((i) => i.id), list.map((q) => [q.a.id, q.stage, q.a.state, q.a.narration, q.stat && [q.stat.added, q.stat.removed], q.ready && q.ready.gate, q.merged && q.merged.commit]), projects().map((p) => p.project)]);
    if (!force && key === S.boardKey) return;
    S.boardKey = key;
    const by = Object.fromEntries(STAGES.map((s) => [s.key, []]));
    for (const q of list) by[q.stage].push(q);
    // Needs you first in its column, then the newest move first.
    const at = (q) => toMs(q.a.state_since) || 0;
    for (const k of Object.keys(by)) by[k].sort((x, y) => (y.need ? 1 : 0) - (x.need ? 1 : 0) || at(y) - at(x));
    const needN = list.filter((q) => q.need || (q.a.running && q.a.state === "waiting")).length;
    const alerts = N.items.filter((i) => i.kind === "alert").length;

    // The line over the board: how many, the one count, the project filter.
    const ps = projects();
    const sel = h("select", { class: "field sv-proj", "aria-label": "Project", onchange: (e) => { S.project = e.target.value; store(PKEY, S.project || null); drawBoard(true); } },
      h("option", { value: "" }, "All projects"), ps.map((p) => h("option", { value: p.project, selected: p.project === S.project ? true : null }, p.project)));
    const working = list.filter((q) => q.a.running && (q.a.state === "working" || q.a.state === "starting")).length;
    bar.replaceChildren(...[
      h("h1", { class: "sv-h" }, `${list.length} request${list.length === 1 ? "" : "s"}`),
      // The top bar's count is these plus the alerts that ask you.
      h("span", { class: "sv-sum num" }, [needN ? h("span", { class: "sv-need" }, `${needN} need${needN === 1 ? "s" : ""} you`) : null,
        alerts ? h("a", { href: "#/needs", class: "sv-need" }, `${alerts} alert${alerts === 1 ? "" : "s"}`) : null,
        working ? `${working} working` : null, by.review.length ? `${by.review.length} to review` : null, by.done.length ? `${by.done.length} done` : null].filter(Boolean).flatMap((x, i) => (i ? [" · ", x] : [x]))),
      h("span", { class: "sp" }),
      ps.length > 1 ? sel : null,
      h("a", { class: "lv-link sv-adv", href: "#/", "data-view-switch": "advanced", title: "Today's Workbench: projects, Review, Debug, every agent's page" }, "Advanced view")].filter(Boolean));

    // A phone shows one stage at a time, Needs you first when anything does.
    const ph = phone();
    if (ph) {
      const counts = Object.fromEntries(STAGES.map((s) => [s.key, by[s.key].length]));
      if (!S.tab || (S.tab === "needs" && !needN)) S.tab = needN ? "needs" : (STAGES.find((s) => counts[s.key] && s.key !== "done") || STAGES[0]).key;
      tabs.hidden = false;
      tabs.replaceChildren(...[needN ? ["needs", "Needs you", needN] : null, ...STAGES.map((s) => [s.key, s.title, counts[s.key]])].filter(Boolean).map(([k, t, n]) =>
        h("button", { type: "button", class: `sv-tab${k === "needs" ? " need" : ""}`, "aria-pressed": String(S.tab === k), onclick: () => { S.tab = k; drawBoard(true); } }, t, h("b", { class: "num" }, String(n)))));
    } else {
      tabs.hidden = true;
      tabs.replaceChildren();
    }

    const cols = STAGES.filter((s) => !ph || S.tab === s.key || S.tab === "needs").map((s) => {
      const items = ph && S.tab === "needs" ? by[s.key].filter((q) => q.need || q.a.state === "waiting") : by[s.key];
      if (ph && S.tab === "needs" && !items.length) return null;
      const n = items.filter((q) => q.need).length;
      const shown = s.key === "done" ? items.slice(0, 8) : items;
      return h("section", { class: `sv-col sv-${s.key}`, "aria-label": s.title },
        h("div", { class: "sv-colh" },
          h("div", { class: "sv-colt" }, h("span", { class: "lbl" }, s.title), h("span", { class: "num sv-dim" }, String(items.length)), h("span", { class: "sp" }),
            n ? h("span", { class: "sv-needn" }, `${n} need${n === 1 ? "s" : ""} you`) : null),
          ph && S.tab === "needs" ? null : h("p", {}, s.sub)),
        shown.length ? shown.map(card) : h("p", { class: "sv-empty" }, emptyWords(s.key)),
        items.length > shown.length ? h("p", { class: "sv-empty" }, `${items.length - shown.length} older in `, h("a", { href: "#/more/terminals" }, "All terminals")) : null);
    }).filter(Boolean);
    board.replaceChildren(...(list.length ? cols : [firstWords()]));
  }

  function emptyWords(k) {
    return { planning: "Nothing is starting.", building: "Nothing is being built.", review: "Nothing waits for review.", done: "Nothing finished in the last three days." }[k];
  }
  function firstWords() {
    return h("div", { class: "sv-first" }, h("b", {}, "No requests yet"),
      h("p", {}, projects().length ? "Say what an agent should do in the box above. Each request gets its own agent and git worktree; this board shows where each one is." : "Open a project first: Workbench needs a git checkout to start agents in."),
      projects().length ? null : btn("Open a project", { href: "#/p", primary: true }));
  }

  /** One card. Its body and buttons follow its stage. */
  function card(q) {
    const { a, stage, need } = q;
    const open = S.route.agent === a.id;
    const mode = modeWords(a.permission_mode);
    const meta = [a.name, S.project || projects().length < 2 ? null : q.p && q.p.project, a.read_only ? "read-only" : mode, timeIn(q)].filter(Boolean).join(" · ");
    const go = `#/s/r/${a.id}`;
    // Done's one action sits on the title line, as a link (the mock): Undo
    // a merge, Resume one that ended.
    const doneAct = stage !== "done" ? null : q.merged ? h("button", { type: "button", class: "lv-link sv-da", onclick: () => undo(q) }, "Undo")
      : !a.running ? h("a", { class: "lv-link sv-da", href: `${go}?tell=1` }, "Resume") : null;
    const head = h("div", { class: "sv-ch" }, avatar(a, 18), h("div", { class: "sv-cht" }, h("a", { class: "sv-ct", href: go }, q.title), stage === "done" ? null : h("span", { class: "sv-meta num" }, meta)), doneAct);
    const body = [];
    if (need && need.kind === "waiting" && need.asked) {
      body.push(h("div", { class: "sv-said" }, h("b", {}, `${a.name} asks you`), h("span", {}, need.text)));
    } else if (need) {
      body.push(h("div", { class: "sv-ask" }, ctx.needCard(need)));
    } else if (a.running && a.state === "waiting") {
      body.push(h("p", { class: "sv-asks" }, a.narration || "Claude Code is showing a prompt."));
    }
    if (stage === "planning") {
      if (!need) body.push(live(a, q.replied ? (a.narration || "Its turn ended. It waits for your reply.") : a.state === "starting" ? "Starting…" : a.narration || "Reading"));
      body.push(acts(q.replied ? btn("Reply", { sm: true, primary: true, href: `${go}?tell=1` }) : null, a.running && !need ? btn(`Tell ${a.name}…`, { sm: true, href: `${go}?tell=1` }) : null));
    } else if (stage === "building") {
      body.push(h("div", { class: "sv-chips" }, statChip(q.stat)));
      if (!need) body.push(live(a, a.narration || "Working"));
      body.push(acts(btn("Watch", { sm: true, href: `#/a/${a.id}/terminal` }), !need ? btn(`Tell ${a.name}…`, { sm: true, href: `${go}?tell=1` }) : null));
    } else if (stage === "review") {
      const r = q.ready;
      const g = (r && r.gate) || {};
      body.push(h("ul", { class: "sv-checks" },
        r ? h("li", { class: r.tests && r.tests.ok === true ? "ok" : r.tests && r.tests.failed ? "bad" : "" }, r.tests ? r.tests.line : "Tests not read yet") : h("li", {}, "Reading its checks…"),
        r ? h("li", { class: g.can_merge ? "ok" : "bad" }, g.can_merge ? `Merges into ${r.onto || "main"}` : g.label || "Not ready to merge") : null,
        r && g.unconfirmed ? h("li", { class: "dec" }, `${g.unconfirmed} decision${g.unconfirmed === 1 ? "" : "s"} to confirm`) : null));
      const canMerge = g.can_merge && !g.anyway && g.primary === "merge";
      body.push(acts(statChip(q.stat), h("span", { class: "sp" }), btn("Review", { sm: true, primary: !canMerge, href: go }), canMerge ? btn(S.merging.has(a.id) ? "Merging…" : "Merge", { sm: true, primary: true, disabled: S.merging.has(a.id), onclick: () => merge(q) }) : null));
    } else {
      body.push(h("p", { class: "sv-meta num" }, doneWords(q)));
    }
    return h("article", { class: `sv-card${need ? " need" : ""}${open ? " sel" : ""}${stage === "done" ? " done" : ""}`, "data-agent": a.id, "data-href": go, tabindex: "0",
      onclick: (e) => { if (!e.target.closest("a, button, input, textarea, select, .qc, .wait-card")) ctx.go(go); } }, head, body);
  }

  /** Merge from the card: only when every check passed (the server checks
   *  the gate again); Done's card then has Undo merge. */
  async function merge(q) {
    const a = q.a;
    if (S.merging.has(a.id)) return;
    S.merging.add(a.id);
    drawBoard(true);
    try {
      const r = await ctx.api("/api/review/merge", { method: "POST", body: { agents: [a.id] } });
      ctx.toast(r && r.message ? r.message : `Merged ${a.name}`);
      refreshTwice(ctx);
    } catch (e) { ctx.toast(`Not merged: ${e.message}`, true); }
    S.merging.delete(a.id);
    refreshReviewSoon(true);
    drawBoard(true);
  }

  async function undo(q) {
    try {
      await ctx.api("/api/review/undo", { method: "POST", body: { agent: q.a.id } });
      S.hint.set(q.a.id, { stage: "review", until: Date.now() + 10000 });
      ctx.toast(`Undid the merge of ${q.a.name}: its branch is back in Review.`);
      refreshTwice(ctx);
    } catch (e) { ctx.toast(`Not undone: ${e.message}`, true); }
    refreshReviewSoon(true);
  }

  const acts = (...kids) => { const k = kids.flat().filter(Boolean); return k.length ? h("div", { class: "sv-acts" }, k) : null; };
  function live(a, text) {
    return h("div", { class: "sv-live" }, h("i", { class: a.running && (a.state === "working" || a.state === "starting") ? "on" : "" }), h("span", {}, text));
  }
  function timeIn(q) {
    const a = q.a;
    if (q.stage === "done") return null;
    if (q.stage === "review") return `ready ${since(a.state_since)}`;
    return since(a.created);
  }
  function doneWords(q) {
    const a = q.a;
    if (q.merged) return `Merged into ${q.merged.onto || "main"}${q.merged.commit ? ` as ${String(q.merged.commit).slice(0, 7)}` : ""} · ${since(q.merged.ts || a.state_since)} ago`;
    const when = since(a.ended || a.state_since);
    return `Ended${a.exit_code != null && a.exit_code !== 0 ? ` (exit ${a.exit_code})` : ""} with nothing to merge · ${when} ago`;
  }

  // ------------------------------------------------------------ life

  const onHash = () => {
    if (!alive) return;
    if (!/^#\/s(\/|$|\?)/.test(location.hash)) return;
    draw();
  };
  window.addEventListener("hashchange", onHash);
  const onResize = () => { if (alive) drawBoard(true); };
  window.matchMedia("(max-width: 640px)").addEventListener("change", onResize);
  // Narration and the time in a stage change without an event the shell
  // redraws on: the board looks again every few seconds.
  const tick = setInterval(() => { drawBoard(false); refreshReview(false); if (S.panel) S.panel.tick(); }, 4000);
  draw();
  refreshReview(true);

  return {
    update() {
      if (!alive) return;
      drawBoard(false);
      refreshReviewSoon(false);
      const r = S.route;
      if (r.view === "request") drawSide(ctx.state.agents.find((a) => a.id === r.agent) || null);
      comp.update();
    },
    reload() { refreshReview(true); },
    onKey(e) {
      if (e.metaKey || e.ctrlKey || e.altKey) return S.panel ? S.panel.onKey(e) : false;
      if (e.key === "n") { if (phone()) ctx.go("#/s/new"); else comp.focus(); return true; }
      if (e.key === "j" || e.key === "k") return move(e.key === "j" ? 1 : -1);
      return S.panel ? S.panel.onKey(e) : false;
    },
    unmount() {
      alive = false;
      clearInterval(tick);
      clearTimeout(reviewT);
      window.removeEventListener("hashchange", onHash);
      window.matchMedia("(max-width: 640px)").removeEventListener("change", onResize);
      if (S.panel) S.panel.unmount();
      comp.unmount();
      el.remove();
    },
  };

  /** j and k: the next card on the board; with a request open, it opens. */
  function move(d) {
    const cards = [...board.querySelectorAll(".sv-card")].filter((c) => c.offsetParent !== null);
    if (!cards.length) return false;
    const cur = document.activeElement && document.activeElement.closest ? document.activeElement.closest(".sv-card") : board.querySelector(".sv-card.sel");
    const i = cards.indexOf(cur || board.querySelector(".sv-card.sel"));
    const next = cards[i < 0 ? (d > 0 ? 0 : cards.length - 1) : Math.max(0, Math.min(cards.length - 1, i + d))];
    next.focus({ preventScroll: true });
    next.scrollIntoView({ block: "nearest", behavior: "smooth" });
    if (S.route.view === "request" && next.dataset.agent !== S.route.agent) history.replaceState(history.state, "", `#/s/r/${next.dataset.agent}`), draw(), next.focus({ preventScroll: true });
    return true;
  }
}

/** #/s, #/s/new, #/s/r/<agent>[?tell=1|merge=1]. */
export function parse(hash) {
  const h0 = String(hash || "").replace(/^#/, "");
  const qi = h0.indexOf("?");
  const parts = (qi >= 0 ? h0.slice(0, qi) : h0).split("/").filter(Boolean);
  const query = new URLSearchParams(qi >= 0 ? h0.slice(qi + 1) : "");
  if (parts[1] === "new") return { view: "new", query };
  if (parts[1] === "r" && parts[2]) return { view: "request", agent: parts[2], query };
  return { view: "board", query };
}

/** A change's size as one chip: +12 −3 · 2 files. */
export function statChip(s) {
  if (!s || !(s.added || s.removed || s.files)) return null;
  return h("span", { class: "sv-chip num" }, h("span", { class: "add" }, `+${s.added || 0}`), " ", h("span", { class: "del" }, `−${s.removed || 0}`), s.files ? ` · ${s.files} file${s.files === 1 ? "" : "s"}` : "");
}

const CSS = `
.sv{display:flex;align-items:flex-start;min-height:100%;box-sizing:border-box}
.sv-left{flex:1;min-width:0;display:flex;flex-direction:column}
.sv-top{padding:var(--s-4) var(--s-6) 0;max-width:var(--page-w);width:100%;margin:0 auto;box-sizing:border-box}
.sv-main{min-width:0;padding:var(--s-4) var(--s-6) var(--s-6);box-sizing:border-box;max-width:var(--page-w);margin:0 auto;width:100%}
.sv.open .sv-main,.sv.open .sv-top{max-width:none;margin:0}
.sv-bar{display:flex;align-items:center;gap:var(--s-3);flex-wrap:wrap;margin-bottom:var(--s-3)}
.sv-h{margin:0;font:var(--w-title) var(--fs-md)/var(--lh-md) var(--sans);letter-spacing:var(--track-md)}
.sv-sum{font:var(--w-text) var(--fs-xs)/var(--lh-xs) var(--mono);color:var(--text-2)}
.sv-sum .sv-need{color:var(--accent-text);text-decoration:none;font-weight:var(--w-mid)}
.sv-bar .sp{flex:1}
.sv-proj{width:auto;height:28px;padding:0 var(--s-2);font-size:var(--fs-sm)}
.sv-adv{font-size:var(--fs-sm)}
.sv-board{display:grid;grid-template-columns:repeat(4,minmax(0,1fr));gap:var(--s-4);align-items:start}
.sv-col{display:flex;flex-direction:column;gap:var(--s-2);min-width:0}
.sv-colh{display:flex;flex-direction:column;gap:2px;padding:0 2px var(--s-1)}
.sv-colt{display:flex;align-items:center;gap:var(--s-2)}.sv-colt .lbl{color:var(--text)}.sv-colt .sp{flex:1}
.sv-colh p{margin:0;font-size:var(--fs-sm);line-height:var(--lh-sm);color:var(--text-2)}
.sv-dim{color:var(--text-3);font:var(--w-text) var(--fs-xs)/var(--lh-xs) var(--mono)}
.sv-needn{font:var(--w-mid) var(--fs-xs)/var(--lh-xs) var(--mono);color:var(--accent-text)}
.sv-card{background:var(--surface);border:var(--hair) solid var(--line);border-radius:var(--r);padding:var(--s-3);display:flex;flex-direction:column;gap:var(--s-2);cursor:pointer;min-width:0;outline:0}
.sv-card:hover{border-color:var(--line-2)}
.sv-card:focus-visible{box-shadow:0 0 0 2px var(--focus)}
.sv-card.need{border-color:var(--accent-line);box-shadow:0 0 0 3px var(--accent-soft)}
.sv-card.sel{border-color:transparent;box-shadow:0 0 0 2px var(--accent)}
.sv-card.done{gap:2px;padding:10px var(--s-3)}
.sv-ch{display:flex;gap:var(--s-2);align-items:flex-start;min-width:0}
.sv-ch .lv-av{margin-top:1px}
.sv-cht{display:flex;flex-direction:column;gap:1px;min-width:0}
.sv-ct{font-weight:var(--w-mid);color:var(--text);text-decoration:none;line-height:var(--lh-sm);overflow-wrap:anywhere}
.sv-ct:hover{text-decoration:underline}
.sv-meta{font:var(--w-text) var(--fs-xs)/var(--lh-xs) var(--mono);color:var(--text-3);overflow-wrap:anywhere}
.sv-card.done .sv-meta{padding-left:26px}
.sv-da{margin-left:auto;flex:none;font-size:var(--fs-sm);background:none;border:0;padding:0;cursor:pointer}
.sv-live{display:flex;align-items:baseline;gap:6px;font-size:var(--fs-sm);line-height:var(--lh-sm);color:var(--text-2);min-width:0}
.sv-live span{overflow:hidden;display:-webkit-box;-webkit-line-clamp:2;-webkit-box-orient:vertical;overflow-wrap:anywhere}
.sv-live i{width:7px;height:7px;border-radius:50%;background:var(--text-3);flex:none;transform:translateY(-1px)}
.sv-live i.on{background:var(--good);animation:pulse 1.6s ease-in-out infinite}
.sv-asks{margin:0;font-size:var(--fs-sm);color:var(--accent-text)}
.sv-said{display:flex;flex-direction:column;gap:2px;padding:var(--s-2) var(--s-3);border-radius:var(--r-sm);background:var(--accent-soft);font-size:var(--fs-sm);line-height:var(--lh-sm)}
.sv-said b{font-weight:var(--w-mid);color:var(--accent-text)}
.sv-said span{overflow:hidden;display:-webkit-box;-webkit-line-clamp:4;-webkit-box-orient:vertical;overflow-wrap:anywhere}
.sv-chips{display:flex;gap:6px;flex-wrap:wrap}
.sv-chip{display:inline-flex;align-items:center;gap:4px;height:20px;padding:0 6px;border-radius:var(--r-xs);background:var(--surface-2);border:var(--hair) solid var(--line);font:var(--w-mid) var(--fs-xs)/1 var(--mono);color:var(--text-2);white-space:nowrap}
.sv-chip .add{color:var(--add-ink)}.sv-chip .del{color:var(--del-ink)}
.sv-acts{display:flex;gap:6px;align-items:center;flex-wrap:wrap}.sv-acts .sp{flex:1}
.sv-checks{list-style:none;margin:0;padding:0;display:flex;flex-direction:column;gap:2px;font-size:var(--fs-sm);line-height:var(--lh-sm);color:var(--text-2)}
.sv-checks li{display:flex;gap:6px}
.sv-checks li::before{content:"·";width:12px;flex:none;text-align:center}
.sv-checks li.ok::before{content:"✓";color:var(--good-ink)}
.sv-checks li.bad::before{content:"!";color:var(--warn-ink);font-weight:var(--w-title)}
.sv-checks li.dec{color:var(--accent-text)}.sv-checks li.dec::before{content:"◆";color:var(--accent-text)}
.sv-ask .qc{border:0;box-shadow:none;background:var(--accent-soft);border-radius:var(--r-sm)}
.sv-ask .qc-hd,.sv-ask .qc-foot{display:none}
.sv-ask .qc-ask{font-size:var(--fs-sm);line-height:var(--lh-sm)}
.sv-ask .wait-card{border:0;background:var(--accent-soft);border-radius:var(--r-sm)}
.sv-empty{margin:0;padding:var(--s-3);border:var(--hair) dashed var(--line-2);border-radius:var(--r);color:var(--text-3);font-size:var(--fs-sm);text-align:center}
.sv-first{grid-column:1/-1;padding:var(--s-6);border:var(--hair) dashed var(--line-2);border-radius:var(--r);display:flex;flex-direction:column;gap:var(--s-2);align-items:flex-start}
.sv-first p{margin:0;color:var(--text-2);max-width:60ch}
.sv-tabs{display:flex;gap:6px;overflow-x:auto;padding:0 0 var(--s-3);scrollbar-width:none}
.sv-tabs::-webkit-scrollbar{display:none}
.sv-tab{display:inline-flex;align-items:center;gap:6px;height:34px;padding:0 var(--s-3);border-radius:var(--r-pill);border:var(--hair) solid var(--line-2);background:var(--surface);font:var(--w-mid) var(--fs-sm)/1 var(--sans);color:var(--text-2);white-space:nowrap;flex:none;cursor:pointer}
.sv-tab b{font-weight:var(--w-mid);color:var(--text)}
.sv-tab[aria-pressed="true"]{background:var(--text);color:var(--surface);border-color:transparent}.sv-tab[aria-pressed="true"] b{color:var(--surface)}
.sv-tab.need{border-color:var(--accent-line);color:var(--accent-text)}.sv-tab.need b{color:var(--accent-text)}
.sv-tab.need[aria-pressed="true"]{background:var(--accent);color:var(--accent-ink)}.sv-tab.need[aria-pressed="true"] b{color:var(--accent-ink)}
.sv-side{flex:0 0 min(500px,40vw);border-left:var(--hair) solid var(--line);background:var(--surface);min-width:0;position:sticky;top:0;height:calc(100vh - var(--top) - env(safe-area-inset-top));overflow:hidden;display:flex;flex-direction:column}
.sv-side[hidden]{display:none}
@media (max-width:1100px){
  .sv-board{grid-template-columns:repeat(4,minmax(220px,1fr));overflow-x:auto;padding-bottom:var(--s-2)}
  .sv-side{position:fixed;right:0;top:calc(var(--top) + env(safe-area-inset-top));bottom:0;height:auto;width:min(500px,100vw);z-index:15;box-shadow:var(--shadow-3)}
}
@media (max-width:640px){
  .sv{flex-direction:column;align-items:stretch}
  .sv-top{padding:var(--s-3) var(--s-4) 0}
  .sv:not(.composing) .sv-top{display:none}
  .sv.composing .sv-main{display:none}
  .sv-main{padding:var(--s-3) var(--s-4) var(--s-6)}
  .sv-board{display:flex;flex-direction:column;gap:var(--s-4);overflow:visible}
  .sv-adv{display:none}
  .sv.open .sv-left{display:none}
  .sv-side{position:static;flex:1;width:auto;height:auto;border:0;box-shadow:none;background:transparent;overflow:visible}
  .sv-card{padding:var(--s-3) 14px}
  .sv-ct{font-size:var(--fs-md);line-height:var(--lh-md)}
  .sv-acts .btn{height:var(--touch);flex:1 1 auto}
  .sv-acts .sv-chip{flex:none}
}
`;

/** The projects again after a merge or an undo: a read already on its way
 *  started before it, so one more after that one. */
function refreshTwice(ctx) {
  if (!ctx.refreshProjects) return;
  Promise.resolve(ctx.refreshProjects()).then(() => ctx.refreshProjects()).catch(() => {});
}
