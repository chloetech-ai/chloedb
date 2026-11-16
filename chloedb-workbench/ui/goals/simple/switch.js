// Simple or Advanced: the switch in the top bar (and the phone's tab bar),
// remembered in this browser as the pref `simple`. Advanced is today's
// Workbench, unchanged; Simple is the board of requests (#/s). The pref
// only says which one #/ opens: every page stays reachable from both.
import { useStyle } from "/levels/common.js";

let PREFS = null;

/** Once, from app.js's start: the styles and the clicks of every
 *  `[data-view-switch]` (the top bar's switch, the board's "Advanced view"). */
export function init(prefs) {
  PREFS = prefs;
  useStyle("vw-switch", CSS);
  document.addEventListener("click", (e) => {
    const t = e.target && e.target.closest ? e.target.closest("[data-view-switch]") : null;
    if (t && PREFS) PREFS.set("simple", t.dataset.viewSwitch === "simple");
  }, true);
}

/** Whether #/ opens the board. */
export function simpleHome() { return !!(PREFS && PREFS.get("simple")); }

/** After each route: which side of the switch is lit, and which tabs the
 *  phone shows. */
export function paint(page) {
  const simple = page === "simple";
  document.body.dataset.view = simple || simpleHome() ? "simple" : "advanced";
  for (const a of document.querySelectorAll("#vw-sw [data-view-switch]")) {
    if ((a.dataset.viewSwitch === "simple") === simple) a.setAttribute("aria-current", "page");
    else a.removeAttribute("aria-current");
  }
}

const CSS = `
.vw-sw{display:inline-flex;align-items:center;gap:2px;padding:2px;border-radius:var(--r-sm);background:var(--surface-2);border:var(--hair) solid var(--line);margin-right:var(--s-2);flex:none}
.vw-sw a{display:inline-flex;align-items:center;height:24px;padding:0 10px;border-radius:calc(var(--r-sm) - 2px);font:var(--w-mid) var(--fs-sm)/1 var(--sans);color:var(--text-2);text-decoration:none;white-space:nowrap}
.vw-sw a:hover{color:var(--text)}
.vw-sw a[aria-current="page"]{background:var(--surface);color:var(--text);box-shadow:var(--shadow-1)}
.tabbar .vw-s{display:none}
body[data-view="simple"] .tabbar .vw-s{display:flex}
body[data-view="simple"] .tabbar .vw-a{display:none}
/* A phone has room for one word: the view you are not in. */
@media (max-width:640px){
  .vw-sw{padding:0;margin:0 2px 0 0;background:none;border:0}
  .vw-sw a[aria-current="page"]{display:none}
  .vw-sw a{height:30px;padding:0 10px;border:var(--hair) solid var(--line-2);border-radius:var(--r-pill);font-size:var(--fs-xs);color:var(--text-2)}
}
`;
