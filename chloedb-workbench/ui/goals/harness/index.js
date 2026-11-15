// Optimize the harness (HARNESS.md): the goal's pages under #/harness.
//   mount(root, ctx) -> { update(state), unmount() }
// ctx = { api(path, opts), params: { repo }, go(hash), state } as the shell
// gives every goal page. The pages are:
//   #/harness                    the week's scorecard, findings, agents by version
//   #/harness/fixes              every problem as one ranked list of fixes
//   #/harness/edit/permissions   the permissions editor
//   #/harness/edit/memory        the memory editor
//   #/harness/sees[/<agent>]     what the model saw at a turn
//   #/harness/versions           versions, promote and roll back
//   #/harness/evals              tests, replay A against B, the runs
//   #/harness/evals/new/<agent>  a session's turns: Make this a test, Optimize this
//   #/harness/runs/<id>          a run: what plays, the scorecard, Enough
import { h } from "/levels/common.js";
import * as overview from "./overview.js";
import * as fixes from "./fixes.js";
import * as permissions from "./permissions.js";
import * as memory from "./memory.js";
import * as sees from "./sees.js";
import * as versions from "./versions.js";
import * as evals from "./evals.js";
import * as timeline from "./timeline.js";
import * as run from "./run.js";

const ROUTES = [
  [/^#\/harness\/fixes/, fixes],
  [/^#\/harness\/(edit\/)?permissions/, permissions],
  [/^#\/harness\/edit\/memory/, memory],
  [/^#\/harness\/sees/, sees],
  [/^#\/harness\/versions/, versions],
  [/^#\/harness\/evals\/new\//, timeline],
  [/^#\/harness\/evals/, evals],
  [/^#\/harness\/runs\//, run],
  [/^#\/harness/, overview],
];

const STEPS = [["#/harness/fixes", "Fix the harness"], ["#/harness/edit/permissions", "Permissions"], ["#/harness/edit/memory", "Memory"], ["#/harness/sees", "What the model sees"], ["#/harness/versions", "Versions"], ["#/harness/evals", "Tests"], ["#/harness/runs", "Tests"]];

export function mount(root, ctx) {
  const host = h("div", { class: "hx" });
  root.replaceChildren(host);
  let view = null;
  let key = null;
  function render(force) {
    const hash = location.hash || "#/harness";
    const path = hash.split("?")[0];
    // The whole hash: the pages read their query (a test's turns and
    // ?optimize=1, the turn in What the model sees, the version to compare).
    if (!force && hash === key) return;
    key = hash;
    // The shell's crumbs (R1's goalCtx), when it gives them.
    const step = STEPS.find(([p]) => path.startsWith(p));
    if (ctx.crumbs) ctx.crumbs([{ label: "Start", href: "#/" }, { label: "Optimize the harness", href: "#/harness" }, ...(step ? [{ label: step[1], href: path }] : [])]);
    const mod = (ROUTES.find(([re]) => re.test(path)) || ROUTES[ROUTES.length - 1])[1];
    try { view && view.unmount && view.unmount(); } catch (err) { console.error(err); }
    host.replaceChildren();
    view = mod.render(host, ctx, path);
    if (typeof root.scrollTo === "function") root.scrollTo(0, 0);
  }
  const onHash = () => render(false);
  window.addEventListener("hashchange", onHash);
  render(true);
  return {
    update(state) { if (view && view.update) view.update(state); },
    unmount() {
      window.removeEventListener("hashchange", onHash);
      try { view && view.unmount && view.unmount(); } catch (err) { console.error(err); }
    },
  };
}
