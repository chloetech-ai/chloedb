// The pieces the screens share: a sheet (one at a time), a menu under a
// button, and a filter chip that opens its choices.
import { h, put } from "/goals/converge/util.js";

/** Closes whatever sheet is open (it runs its own clean-up). */
export function closeSheets() {
  document.querySelectorAll(".cv-sheet").forEach((s) => { if (s._close) s._close(true); else s.remove(); });
  document.querySelectorAll(".cv-scrim").forEach((s) => s.remove());
}

/** A sheet from the right (the full screen on a phone): a head, a body that
 *  scrolls and a sticky foot. `onClose(quiet)` runs once when it closes. */
export function sheetShell({ label, data = {}, onClose }) {
  closeSheets();
  let closed = false;
  const close = (quiet) => {
    if (closed) return;
    closed = true;
    sheet.remove(); scrim.remove();
    if (onClose) onClose(quiet);
  };
  const scrim = h("div", { class: "cv-scrim", onclick: () => close() });
  const head = h("div", { class: "cv-sheet-h" });
  const tools = h("div", { class: "tools" });
  const body = h("div", { class: "cv-sheet-b" });
  const foot = h("div", { class: "cv-sheet-f" });
  const sheet = h("aside", { class: "cv-sheet", role: "dialog", "aria-modal": "true", "aria-label": label || "Details" }, head, body, foot);
  for (const [k, v] of Object.entries(data)) sheet.dataset[k] = v;
  sheet._close = close;
  const x = h("button", { class: "cv-btn icon quiet cv-x", type: "button", "aria-label": "Close", onclick: () => close() }, "×");
  const setHead = (...kids) => { put(tools, ...(sheet._tools || []), x); put(head, ...kids, tools); };
  sheet._tools = [];
  setHead();
  document.body.append(scrim, sheet);
  return { sheet, head, body, foot, close, setHead, setTools: (t) => { sheet._tools = t; } };
}

/** A menu under `anchor`: items are [label, onclick | href, {danger}] or "-". */
export function popMenu(anchor, items) {
  document.querySelectorAll(".cv-menu, .cv-popscrim").forEach((e) => e.remove());
  const done = () => { m.remove(); scrim.remove(); };
  const scrim = h("div", { class: "cv-popscrim", onclick: done });
  const m = h("div", { class: "cv-menu", role: "menu" }, items.filter(Boolean).map((it) => {
    if (it === "-") return h("hr");
    const [label, go, o = {}] = it;
    return typeof go === "string"
      ? h("a", { href: go, role: "menuitem", class: o.danger ? "danger" : null, onclick: done }, label)
      : h("button", { type: "button", role: "menuitem", class: o.danger ? "danger" : null, onclick: () => { done(); go(); } }, label);
  }));
  document.body.append(scrim, m);
  place(m, anchor);
  m.querySelector("a, button")?.focus();
  return done;
}

function place(el, anchor) {
  const r = anchor.getBoundingClientRect();
  const w = el.offsetWidth, vh = window.innerHeight, vw = window.innerWidth;
  const left = Math.max(8, Math.min(vw - w - 8, r.right - w > 8 && r.left + w > vw - 8 ? r.right - w : r.left));
  let top = r.bottom + 4;
  if (top + el.offsetHeight > vh - 8) top = Math.max(8, r.top - el.offsetHeight - 4);
  el.style.left = `${left}px`; el.style.top = `${top}px`;
}

/** A filter chip: shows its choice, opens a list of checkboxes (a bottom sheet on a phone). */
export function chipFilter({ label, options, chosen, onChange }) {
  const on = chosen.size > 0;
  const names = options.filter((o) => chosen.has(o.value)).map((o) => o.label);
  const text = !on ? label : names.length === 1 ? names[0] : `${label} · ${names.length}`;
  const btn = h("button", { class: `cv-chip${on ? " on" : ""}`, type: "button", "aria-haspopup": "true", "aria-pressed": String(on) },
    text, h("span", { class: "car", "aria-hidden": "true" }, "▾"));
  btn.addEventListener("click", () => {
    document.querySelectorAll(".cv-pop, .cv-popscrim").forEach((e) => e.remove());
    const done = () => { pop.remove(); scrim.remove(); onChange(); };
    const scrim = h("div", { class: "cv-popscrim", onclick: done });
    const pop = h("div", { class: "cv-pop", role: "group", "aria-label": label },
      options.length ? options.map((o) => {
        const inp = h("input", { type: "checkbox", checked: chosen.has(o.value) ? true : null });
        inp.addEventListener("change", () => { if (inp.checked) chosen.add(o.value); else chosen.delete(o.value); });
        return h("label", {}, inp, o.dot != null ? h("span", { class: "cv-sev", "data-sev": String(o.dot) }) : null, h("span", {}, o.label), h("span", { class: "n" }, String(o.n ?? "")));
      }) : h("p", { class: "cv-empty", style: { padding: "8px 10px" } }, "Nothing to choose yet."),
      h("button", { class: "clr", type: "button", onclick: () => { chosen.clear(); done(); } }, "Clear"),
      h("button", { class: "cv-btn pri", type: "button", onclick: done }, "Done"));
    document.body.append(scrim, pop);
    if (window.innerWidth > 640) place(pop, btn);
  });
  return btn;
}

/** A textarea that grows with what is typed, up to its max-height. */
export function autoGrow(ta) {
  const fit = () => { ta.style.height = "auto"; ta.style.height = `${Math.min(140, ta.scrollHeight + 2)}px`; };
  ta.addEventListener("input", fit);
  return fit;
}
