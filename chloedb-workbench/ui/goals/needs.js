// Needs you (power users, 1): everything that needs you, across projects.
// Converge's "Blocked on you" groups come first, each finding with its step,
// what you do and "Done, carry on" (the shell's card); then the questions,
// the alerts and Converge's other picks, in the one question card. With
// more than a few, filters by project, agent and kind, sorted by the oldest
// or by the most work an answer unblocks. `#/needs?project=&agent=&kind=&
// sort=` keeps the filters in the address.
import { h, useStyle, page, btn, kc, label, parseJSON, toMs } from "/levels/common.js";
import { isBlocked } from "/goals/shell.js";

const KINDS = [["", "All"], ["permission", "Permission"], ["question", "Question"], ["trust", "Trust"], ["alert", "Alert"], ["waiting", "Waiting"], ["converge", "Converge"]];

export function kindOf(it) {
  if (it.kind === "alert" || it.kind === "trust" || it.kind === "waiting" || it.kind === "converge") return it.kind;
  return it.question && it.question.kind === "ask" ? "question" : "permission";
}

const cmdOf = (it) => (it.question && (parseJSON(it.question.detail, {}) || {}).command) || null;

/** How much work an answer unblocks: the agents asking the identical
 *  command (one answer each, batchable), plus the agents it spawned. */
export function blocked(it, all) {
  const c = cmdOf(it);
  const same = c ? all.filter((x) => cmdOf(x) === c).length : 1;
  const kids = (it.agent && Array.isArray(it.agent.children) ? it.agent.children.length : 0);
  return same + kids;
}

export function mount(root, ctx) {
  useStyle("g-needs", CSS);
  const q = ctx.params.query || new URLSearchParams();
  const F = { project: q.get("project") || "", agent: q.get("agent") || "", kind: q.get("kind") || "", sort: q.get("sort") === "blocked" ? "blocked" : "oldest" };
  const el = h("div", { class: "g-needs" });
  root.append(el);
  ctx.crumbs([{ label: "Start", href: "#/" }, { label: "Needs you", href: "#/needs" }]);

  const keep = () => {
    const p = new URLSearchParams();
    for (const k of ["project", "agent", "kind"]) if (F[k]) p.set(k, F[k]);
    if (F.sort !== "oldest") p.set("sort", F.sort);
    const hash = `#/needs${p.toString() ? "?" + p : ""}`;
    if (location.hash !== hash) history.replaceState(history.state, "", hash);
  };

  function draw() {
    const N = ctx.needs();
    const names = ctx.projectNames();
    const all = N.items;
    const projects = [...new Set(all.map((it) => names[it.repo] || it.repo).filter(Boolean))];
    const agents = [...new Map(all.filter((it) => it.agent).map((it) => [it.agent.id, it.agent])).values()];
    let list = all.filter((it) => (!F.project || (names[it.repo] || it.repo) === F.project) && (!F.agent || (it.agent && it.agent.id === F.agent)) && (!F.kind || kindOf(it) === F.kind));
    if (F.sort === "blocked") list = [...list].sort((a, b) => blocked(b, all) - blocked(a, all) || (toMs(a.since_ms) || 0) - (toMs(b.since_ms) || 0));
    // What only you can do first: Converge's "Blocked on you".
    const blk = list.filter(isBlocked);
    const rest = list.filter((it) => !isBlocked(it));
    const blkN = blk.reduce((n, it) => n + (it.count || (it.findings || []).length || 1), 0);
    // The filters earn their room only with more than a few cards (or one set).
    const showF = all.length > 4 || F.project || F.agent || F.kind || F.sort !== "oldest";
    const sel = (key, opts, labelText) => h("label", { class: "g-f" }, h("span", {}, labelText),
      h("select", { class: "field", onchange: (e) => { F[key] = e.target.value; keep(); draw(); } },
        opts.map(([v, t]) => h("option", { value: v, selected: F[key] === v ? true : null }, t))));
    const filtered = list.length !== all.length;
    const sub = all.length
      ? [blkN ? `${blkN} blocked on you: do it, then Done, carry on.` : null,
        rest.length ? `${rest.length} more wait${rest.length === 1 ? "s" : ""} for your answer or pick${projects.length > 1 ? `, across ${projects.length} projects` : ""}, ${F.sort === "blocked" ? "the ones that unblock the most work first" : "oldest first"}.` : null].filter(Boolean).join(" ")
      : "Nothing needs you: nothing is blocked on you, no agent is waiting on you, and no finding waits for your pick. Converge moves the rest by itself.";
    const item = (it) => {
      const card = ctx.needCard(it, { big: true });
      const proj = names[it.repo];
      return h("div", { class: "g-item" }, proj && projects.length > 1 ? h("div", { class: "g-iproj" }, h("a", { href: `#/converge?project=${encodeURIComponent(proj)}` }, proj), F.sort === "blocked" && blocked(it, all) > 1 ? h("span", {}, ` · unblocks ${blocked(it, all)}`) : null) : null, card);
    };
    el.replaceChildren(page({ kicker: "Converge", title: "Needs you", sub, wide: true },
      showF ? h("div", { class: "g-fbar" },
        sel("project", [["", "Every project"], ...projects.map((p) => [p, p])], "Project"),
        sel("agent", [["", "Every agent"], ...agents.map((a) => [a.id, a.name])], "Agent"),
        h("div", { class: "g-kinds", role: "group", "aria-label": "Kind" }, KINDS.map(([v, t]) => {
          const n = v ? all.filter((it) => kindOf(it) === v).length : all.length;
          return v && !n ? null : h("button", { type: "button", class: `chip${F.kind === v ? " on" : ""}`, "aria-pressed": String(F.kind === v), onclick: () => { F.kind = v; keep(); draw(); } }, t, h("span", { class: "num" }, ` ${n}`));
        })),
        h("span", { class: "sp" }),
        h("div", { class: "g-sort", role: "group", "aria-label": "Order" },
          btn("Oldest", { sm: true, ghost: F.sort !== "oldest", onclick: () => { F.sort = "oldest"; keep(); draw(); } }),
          btn("Most work blocked", { sm: true, ghost: F.sort !== "blocked", onclick: () => { F.sort = "blocked"; keep(); draw(); } }))) : null,
      blk.length ? h("section", { class: "g-sec g-blk", "aria-label": "Blocked on you" },
        label("Blocked on you", h("span", { class: "num" }, String(blkN))),
        h("div", { class: "g-list" }, blk.map(item))) : null,
      rest.length ? h("section", { class: "g-sec", "aria-label": "Waiting for you" },
        blk.length ? label("Waiting for you", h("span", { class: "num" }, String(rest.length))) : null,
        h("div", { class: "g-list" }, rest.map(item))) : null,
      !list.length && all.length ? h("p", { class: "g-dim" }, "Nothing matches these filters. ", filtered ? h("a", { href: "#/needs", onclick: () => { F.project = F.agent = F.kind = ""; } }, "Show all") : null) : null,
      all.length ? h("p", { class: "g-keys" }, kc("j"), kc("k"), " move · a digit then ", kc("↵"), " answers · ", kc("x"), " selects · ", kc("⇧1"), " answers the selected alike") : null));
  }

  draw();
  return { update: draw, unmount: () => el.remove() };
}

const CSS = `
.g-fbar{display:flex;gap:var(--s-3);align-items:flex-end;flex-wrap:wrap;margin-bottom:var(--s-4)}
.g-fbar .sp{flex:1}
.g-f{display:flex;flex-direction:column;gap:4px;font:var(--w-mid) var(--fs-xs)/var(--lh-xs) var(--mono);letter-spacing:var(--track-label);text-transform:uppercase;color:var(--text-2)}
.g-f select{min-width:160px;text-transform:none;letter-spacing:0}
.g-kinds{display:flex;gap:var(--s-1);flex-wrap:wrap}
.g-kinds .chip{cursor:pointer;border:0;height:var(--ctl-h);padding:0 var(--s-3);font-size:var(--fs-sm);line-height:1;border-radius:var(--r-sm)}
.g-kinds .chip.on{background:var(--text);color:var(--bg);border-color:var(--text)}
.g-sort{display:flex;gap:var(--s-1)}
.g-list{display:flex;flex-direction:column;gap:var(--s-4);max-width:900px}
.g-sec{margin-bottom:var(--s-6);max-width:900px}
.g-sec>.lbl{margin-bottom:var(--s-2)}
.g-blk>.lbl{color:var(--crit-ink)}
.g-item{display:flex;flex-direction:column;gap:var(--s-1)}
/* Not .g-proj: that is Continue's page, which turned monospace once this style loaded (Priya). */
.g-iproj{font:var(--w-mid) var(--fs-xs)/var(--lh-xs) var(--mono);color:var(--text-2)}
.g-iproj a{color:var(--text-2)}
.g-needs .g-dim{color:var(--text-2)}
.g-needs .g-keys{margin-top:var(--s-5);font-size:var(--fs-sm);color:var(--text-2)}
@media (max-width:640px){.g-f{flex:1 1 140px}.g-f select{min-width:0;width:100%}.g-fbar .sp{display:none}.g-needs .g-keys{display:none}}
`;
