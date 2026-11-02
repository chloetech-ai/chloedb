// Pan and zoom for the SVG canvases of Schema and the graph explorer
// (copied from the old topology view, which is going away): a wheel or a
// pinch zooms, a drag on the background pans, and a node can be dragged.

import { clamp } from "./core.js";

/** Pan and zoom of `root` (a <g>) inside `svgEl`. */
export function viewport(svgEl, root, { mini } = {}) {
  let k = 1, x = 0, y = 0;
  const apply = () => root.setAttribute("transform", `translate(${x.toFixed(1)},${y.toFixed(1)}) scale(${k.toFixed(3)})`);
  const vp = {
    get k() { return k; },
    fit(pad = 24) {
      const b = root.getBBox(); const r = svgEl.getBoundingClientRect();
      if (!b.width || !r.width) return;
      k = clamp(Math.min((r.width - pad * 2) / b.width, (r.height - pad * 2) / b.height), 0.25, mini ? 1 : 1.4);
      x = (r.width - b.width * k) / 2 - b.x * k; y = (r.height - b.height * k) / 2 - b.y * k; apply();
    },
    zoom(f, px, py) { const r = svgEl.getBoundingClientRect(); px ??= r.width / 2; py ??= r.height / 2; const nk = clamp(k * f, 0.25, 3); x = px - (px - x) * nk / k; y = py - (py - y) * nk / k; k = nk; apply(); },
    get state() { return { k, x, y }; }, set state(v) { ({ k, x, y } = v); apply(); },
  };
  if (!mini) {
    // The canvas takes its own touches: one finger pans, two pinch.
    svgEl.style.touchAction = "none";
    svgEl.addEventListener("wheel", (e) => { e.preventDefault(); const r = svgEl.getBoundingClientRect(); vp.zoom(Math.exp(-e.deltaY * 0.0015), e.clientX - r.left, e.clientY - r.top); }, { passive: false });
    const touches = new Map();
    let pinch = null;
    svgEl.addEventListener("pointerdown", (e) => {
      if (e.pointerType === "touch") touches.set(e.pointerId, [e.clientX, e.clientY]);
      if (touches.size === 2) {
        const [a, b] = [...touches.values()];
        pinch = { d: Math.hypot(a[0] - b[0], a[1] - b[1]) || 1 };
        return;
      }
      if (e.target !== svgEl && !e.target.classList.contains("bg")) return;
      const sx = e.clientX - x, sy = e.clientY - y;
      svgEl.classList.add("panning"); svgEl.setPointerCapture(e.pointerId);
      const move = (ev) => { if (pinch) return; x = ev.clientX - sx; y = ev.clientY - sy; apply(); };
      const up = () => { svgEl.classList.remove("panning"); svgEl.removeEventListener("pointermove", move); svgEl.removeEventListener("pointerup", up); svgEl.removeEventListener("pointercancel", up); };
      svgEl.addEventListener("pointermove", move); svgEl.addEventListener("pointerup", up); svgEl.addEventListener("pointercancel", up);
    });
    svgEl.addEventListener("pointermove", (e) => {
      if (!touches.has(e.pointerId)) return;
      touches.set(e.pointerId, [e.clientX, e.clientY]);
      if (!pinch || touches.size !== 2) return;
      const [a, b] = [...touches.values()], d = Math.hypot(a[0] - b[0], a[1] - b[1]) || 1;
      const r = svgEl.getBoundingClientRect();
      vp.zoom(d / pinch.d, (a[0] + b[0]) / 2 - r.left, (a[1] + b[1]) / 2 - r.top);
      pinch.d = d;
    });
    const lift = (e) => { touches.delete(e.pointerId); if (touches.size < 2) pinch = null; };
    svgEl.addEventListener("pointerup", lift);
    svgEl.addEventListener("pointercancel", lift);
  }
  apply();
  return vp;
}

/** Drag `g` (a node) on a canvas zoomed by `vp`: onMove(dx, dy) in canvas units, onEnd() once. */
export function draggable(g, vp, onMove, onEnd) {
  g.addEventListener("pointerdown", (e) => {
    if (e.button !== 0) return;
    e.stopPropagation();
    let lx = e.clientX, ly = e.clientY, moved = 0;
    g.setPointerCapture(e.pointerId);
    const move = (ev) => { const dx = (ev.clientX - lx) / vp.k, dy = (ev.clientY - ly) / vp.k; lx = ev.clientX; ly = ev.clientY; moved += Math.abs(dx) + Math.abs(dy); if (moved > 3) { g.classList.add("dragging"); onMove(dx, dy); } };
    const up = () => { g.removeEventListener("pointermove", move); g.removeEventListener("pointerup", up); g.removeEventListener("pointercancel", up); g.classList.remove("dragging"); g.moved = moved > 3; setTimeout(() => (g.moved = false), 0); if (moved > 3) onEnd(); };
    g.addEventListener("pointermove", move); g.addEventListener("pointerup", up); g.addEventListener("pointercancel", up);
  });
}
