// The Simple view's one box: what an agent should do, in your words; the
// project, the permission mode (the same choice as Start a change, kept in
// this browser) and the model. "One request per line" starts one agent per
// line, each in its own worktree. A sentence that would delete, push or
// touch a guarded path is named before anything starts (Continue's check).
import { h, useStyle, btn, kc, permissionChoice, PERMISSION_MODES } from "/levels/common.js";
import { intentCheck } from "/goals/continue.js";
import { splitRequests } from "/goals/simple/stage.js";

const enc = encodeURIComponent;
const DRAFT = "wb-simple-draft";
const LINES = "wb-simple-per-line";
const MODEL = "wb-simple-model";
const kept = (k, v) => { try { if (v === undefined) return localStorage.getItem(k); if (v === null) localStorage.removeItem(k); else localStorage.setItem(k, v); } catch { /* a private window */ } return null; };

/** `o.project()`: the project in focus; `o.onStarted(agents)`. */
export function composer(ctx, o) {
  useStyle("g-simple-compose", CSS);
  const S = { sending: false, ok: null, repo: null, rules: [], guarded: [] };
  const box = h("textarea", { class: "field sc-box", rows: 1, placeholder: "What should an agent do? One request per line starts one agent each.", "aria-label": "New request" });
  box.value = kept(DRAFT) || "";
  const proj = h("select", { class: "field sc-sel", "aria-label": "Project" });
  const model = h("select", { class: "field sc-sel", "aria-label": "Model" }, ["", "haiku", "sonnet", "opus"].map((m) => h("option", { value: m }, m ? m[0].toUpperCase() + m.slice(1) : "Default model")));
  model.value = kept(MODEL) || "";
  const perm = permissionChoice();
  perm.select.classList.add("sc-sel");
  const lines = h("input", { type: "checkbox", checked: kept(LINES) !== "0" });
  const go = h("button", { class: "btn primary sc-go", type: "submit" }, "Start", h("span", { class: "sc-k" }, kc("⌘↵")));
  const note = h("div", { class: "sc-note", hidden: true, role: "status" });
  const count = h("span", { class: "sc-count num" });
  const hint = h("span", { class: "sc-hint" });

  const el = h("form", { class: "sc", onsubmit: (e) => { e.preventDefault(); start(); } },
    box,
    h("div", { class: "sc-row" },
      h("label", { class: "sc-f" }, h("span", { class: "sc-lb" }, "Project"), proj),
      h("label", { class: "sc-f", title: "What it may do without asking" }, h("span", { class: "sc-lb" }, "Mode"), perm.select),
      h("label", { class: "sc-f" }, h("span", { class: "sc-lb" }, "Model"), model),
      h("label", { class: "sc-tick" }, lines, h("span", {}, "One request per line")),
      h("span", { class: "sp" }),
      count,
      go),
    hint,
    note);

  const repoNow = () => { const p = ctx.projects().find((x) => x.path === proj.value); return p || o.project(); };
  function paintProjects() {
    const ps = ctx.projects();
    const want = proj.value || (o.project() || {}).path || "";
    proj.replaceChildren(...(ps.length ? ps.map((p) => h("option", { value: p.path }, p.project)) : [h("option", { value: "" }, "No project open")]));
    proj.value = ps.some((p) => p.path === want) ? want : ps[0] ? ps[0].path : "";
    proj.disabled = ps.length < 2;
    loadChecks();
  }
  function loadChecks() {
    const p = repoNow();
    if (!p || S.repo === p.path) return;
    S.repo = p.path;
    S.rules = []; S.guarded = [];
    ctx.api(`/api/rules?repo=${enc(p.path)}`).then((r) => { if (S.repo === p.path) S.rules = r.rules || []; }, () => {});
    ctx.api(`/api/guarded?repo=${enc(p.path)}`).then((r) => { if (S.repo === p.path) S.guarded = (r.rules || []).filter((x) => x.on); }, () => {});
  }
  function paint() {
    const n = splitRequests(box.value, lines.checked).length;
    count.textContent = n > 1 ? `${n} agents` : "";
    go.firstChild.textContent = n > 1 ? `Start ${n}` : "Start";
    go.disabled = !n || S.sending || !repoNow();
    // The mode's sentence, only when it is not your settings' own mode.
    const m = PERMISSION_MODES.find(([v]) => v === perm.select.value);
    hint.textContent = m && m[0] ? m[2] : "";
    hint.hidden = !hint.textContent;
    // The box grows with what is typed, to eight lines.
    box.style.height = "auto";
    box.style.height = `${Math.min(box.scrollHeight + 2, 8 * 22 + 20)}px`;
  }
  box.addEventListener("input", () => { kept(DRAFT, box.value || null); S.ok = null; note.hidden = true; paint(); });
  box.addEventListener("keydown", (e) => {
    if (e.key === "Enter" && (e.metaKey || e.ctrlKey)) { e.preventDefault(); start(); }
    if (e.key === "Escape") { e.stopPropagation(); box.blur(); }
  });
  lines.addEventListener("change", () => { kept(LINES, lines.checked ? "1" : "0"); paint(); });
  model.addEventListener("change", () => kept(MODEL, model.value || null));
  proj.addEventListener("change", () => { loadChecks(); paint(); });
  perm.select.addEventListener("change", paint);

  function warn(found) {
    note.hidden = false;
    note.className = "sc-note warn";
    note.replaceChildren(
      h("b", {}, found.length === 1 ? "One sentence asks for something hard to undo." : `${found.length} sentences ask for something hard to undo.`),
      h("ul", {}, found.slice(0, 4).map((f) => h("li", {}, h("q", {}, f.sentence), ` ${f.why}.`))),
      h("div", { class: "sc-acts" },
        btn("Start anyway", { sm: true, primary: true, onclick: () => { S.ok = box.value; start(); } }),
        btn("Edit it", { sm: true, onclick: () => { note.hidden = true; box.focus(); } })));
  }

  async function start() {
    const p = repoNow();
    const list = splitRequests(box.value, lines.checked);
    if (!list.length || S.sending || !p) { box.focus(); return; }
    if (S.ok !== box.value) {
      const found = list.flatMap((t) => intentCheck(t, { rules: S.rules, guarded: S.guarded }));
      if (found.length) { warn(found); return; }
    }
    S.sending = true;
    note.hidden = true;
    go.disabled = true;
    const started = [];
    let failed = null;
    for (const task of list) {
      const body = { repo: p.path, task, worktree: true, permissions: { allow: [], deny: ["Bash(git push:*)", "Bash(rm -rf:*)"] } };
      Object.assign(body, perm.body());
      if (model.value) body.model = model.value;
      try {
        const { agent } = await ctx.api("/api/agents", { method: "POST", body });
        started.push(agent);
      } catch (e) { failed = e.status === 429 ? `${e.message}. Stop one first.` : e.message; break; }
    }
    S.sending = false;
    if (started.length) {
      // What did not start stays in the box, so nothing typed is lost.
      const left = list.slice(started.length);
      box.value = left.join("\n");
      kept(DRAFT, box.value || null);
      ctx.toast(started.length === 1 ? `Started ${started[0].name}${started[0].branch ? ` on ${started[0].branch}` : ""}` : `Started ${started.length} agents: ${started.map((a) => a.name).join(", ")}`);
      o.onStarted(started);
    }
    if (failed) {
      note.hidden = false;
      note.className = "sc-note bad";
      note.replaceChildren(h("b", {}, started.length ? `${started.length} started; the next did not: ` : "Not started: "), failed);
    }
    paint();
  }

  paintProjects();
  paint();
  return {
    el,
    focus() { box.focus({ preventScroll: false }); },
    update() {
      const have = [...proj.options].map((x) => x.value).join();
      if (have !== ctx.projects().map((p) => p.path).join()) paintProjects();
      paint();
    },
    unmount() { el.remove(); },
  };
}

const CSS = `
.sc{display:flex;flex-direction:column;gap:var(--s-2);background:var(--surface);border:var(--hair) solid var(--line-2);border-radius:var(--r);padding:var(--s-3);box-shadow:var(--shadow-1)}
.sc:focus-within{border-color:var(--accent-line);box-shadow:0 0 0 3px var(--accent-soft)}
.sc .sc-box{border:0;padding:2px 0;min-height:24px;height:auto;resize:none;background:transparent;box-shadow:none;font:var(--w-text) var(--fs-body)/var(--lh-body) var(--sans)}
.sc .sc-box:focus{outline:0;box-shadow:none}
.sc-row{display:flex;align-items:center;gap:var(--s-2);flex-wrap:wrap}
.sc-row .sp{flex:1}
.sc-f{display:inline-flex;align-items:center;gap:6px;min-width:0}
.sc-lb{font:var(--w-mid) var(--fs-xs)/var(--lh-xs) var(--mono);letter-spacing:.06em;text-transform:uppercase;color:var(--text-3)}
.sc-sel{width:auto;max-width:200px;height:28px;padding:0 var(--s-2);font-size:var(--fs-sm)}
.sc-tick{display:inline-flex;align-items:center;gap:6px;font-size:var(--fs-sm);color:var(--text-2);cursor:pointer}
.sc-tick input{accent-color:var(--accent);margin:0}
.sc-count{font:var(--w-text) var(--fs-xs)/var(--lh-xs) var(--mono);color:var(--text-2)}
.sc-go .sc-k{margin-left:4px;opacity:.8}
.sc-go .sc-k .kc{background:transparent;border-color:currentColor;color:inherit}
.sc-hint{font-size:var(--fs-xs);line-height:var(--lh-xs);color:var(--text-3)}
.sc-note{display:flex;flex-direction:column;gap:var(--s-1);padding:var(--s-2) var(--s-3);border-radius:var(--r-sm);font-size:var(--fs-sm);line-height:var(--lh-sm)}
.sc-note.warn{background:var(--warn-soft);color:var(--text)}
.sc-note.bad{background:var(--crit-soft);color:var(--text)}
.sc-note ul{margin:0;padding-left:var(--s-4)}
.sc-note q{font-style:italic}
.sc-acts{display:flex;gap:6px}
@media (max-width:640px){
  .sc{padding:var(--s-3);border-radius:var(--r)}
  .sc .sc-box{min-height:140px}
  .sc-row{flex-direction:column;align-items:stretch;gap:var(--s-2)}
  .sc-f{display:grid;grid-template-columns:64px minmax(0,1fr);gap:var(--s-2)}
  .sc-sel{max-width:none;flex:1;height:var(--touch)}
  .sc-tick{min-height:var(--touch)}
  .sc-row .sp{display:none}
  .sc-go{height:var(--touch)}
  .sc-go .sc-k{display:none}
}
`;
