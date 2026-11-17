// A row of a step's list: a checkbox (when the step has a pick), the title,
// one meta line, the status line, and on a phone a swipe for the two moves.
import { h, put } from "/goals/converge/util.js";

/** o: { selectable, selected, onSelect(bool), onOpen, lines: [nodes], side: node,
 *       swipe: { right: [label, fn], left: [label, fn] }, cls } */
export function listRow(o) {
  const ck = o.selectable ? h("input", { type: "checkbox", "aria-label": `Select “${o.title}”`, checked: o.selected ? true : null }) : null;
  if (ck) ck.addEventListener("change", () => { li.classList.toggle("sel", ck.checked); o.onSelect(ck.checked); });
  const main = h("button", { class: "r-main", type: "button" }, ...o.lines.filter(Boolean));
  const inner = h("div", { class: "r-in" }, ck ? h("label", { class: "r-ck" }, ck) : h("span"), main, o.side ? h("span", { class: "r-side" }, o.side) : h("span"));
  const sw = o.swipe ? h("div", { class: "r-sw", "aria-hidden": "true" }, h("span", { class: "r" }, o.swipe.right ? `${o.swipe.right[0]} →` : ""), h("span", { class: "l" }, o.swipe.left ? `← ${o.swipe.left[0]}` : "")) : null;
  const li = h("li", { class: `cv-row${o.selectable ? "" : " nosel"}${o.selected ? " sel" : ""}${o.cls ? " " + o.cls : ""}`, "data-id": o.id }, sw, inner);
  let swiped = 0;
  main.addEventListener("click", () => { if (Date.now() - swiped > 350) o.onOpen(); });
  if (o.swipe) swipeable(li, inner, o.swipe, o.S, () => { swiped = Date.now(); });
  return li;
}

/** Swipe right or left past a third of a row (fingers and pens only). */
function swipeable(li, inner, sw, S, mark) {
  inner.addEventListener("pointerdown", (e) => {
    if (e.pointerType === "mouse" || e.target.closest("input, label")) return;
    const x0 = e.clientX, y0 = e.clientY;
    let dx = 0, on = false, dead = false;
    const W = li.offsetWidth || 360;
    const move = (ev) => {
      if (dead) return;
      const mx = ev.clientX - x0, my = ev.clientY - y0;
      if (!on) {
        if (Math.abs(my) > 10 && Math.abs(my) > Math.abs(mx)) { dead = true; return; }
        if (Math.abs(mx) < 12) return;
        on = true; if (S) S.swiping = true; li.classList.add("swiping"); inner.style.transition = "none";
      }
      dx = Math.max(sw.left ? -W : 0, Math.min(sw.right ? W : 0, mx));
      inner.style.transform = `translateX(${dx}px)`;
      li.classList.toggle("go-r", dx > W / 3);
      li.classList.toggle("go-l", dx < -W / 3);
    };
    const up = () => {
      window.removeEventListener("pointermove", move);
      window.removeEventListener("pointerup", up);
      window.removeEventListener("pointercancel", up);
      if (S) S.swiping = false;
      if (!on) li.classList.remove("swiping");
      inner.style.transition = "";
      if (!on) return;
      mark();
      const go = dx > W / 3 ? sw.right : dx < -W / 3 ? sw.left : null;
      if (go) { inner.style.transform = `translateX(${dx > 0 ? W : -W}px)`; setTimeout(() => go[1](), 160); }
      else { inner.style.transform = ""; li.classList.remove("go-r", "go-l"); setTimeout(() => li.classList.remove("swiping"), 200); }
    };
    window.addEventListener("pointermove", move, { passive: true });
    window.addEventListener("pointerup", up);
    window.addEventListener("pointercancel", up);
  });
}

/** A step screen's head: back to the pipeline, the name and count, a hint, an action. */
export function stepHead(C, name, count, hint, action) {
  return h("header", { class: "cv-sh" },
    h("a", { class: "cv-back", href: C.href("home") }, "‹ Inbox"),
    h("div", { class: "row" }, h("h1", {}, name, count != null ? h("span", { class: "c" }, String(count)) : null), action || null),
    hint ? h("p", { class: "hint" }, hint) : null);
}

/** An empty step, saying what to do next. */
export function none(text, ...actions) {
  return h("div", { class: "cv-none" }, h("p", {}, text), ...actions);
}

/** The sticky bar of a selection: "N selected" and its buttons. */
export function bulkBar(n, ...buttons) {
  return h("div", { class: "cv-bulk", role: "toolbar", "aria-label": "Selected" }, h("span", { class: "n" }, `${n} selected`), ...buttons);
}

export { put };
