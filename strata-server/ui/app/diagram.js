// Diagram helpers shared by the views that draw one: pan and zoom of an
// SVG picture, dragging a part of it, and one server's storage pipeline
// (how its writes reach storage and its queries read it).

import { h, s, fmt, clamp } from "./core.js";

const short = (url) => (url || "").replace(/^https?:\/\//, "");

/** Pan and zoom of a <g> inside an <svg>. */
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
    svgEl.addEventListener("wheel", (e) => { e.preventDefault(); const r = svgEl.getBoundingClientRect(); vp.zoom(Math.exp(-e.deltaY * 0.0015), e.clientX - r.left, e.clientY - r.top); }, { passive: false });
    svgEl.addEventListener("pointerdown", (e) => {
      if (e.target !== svgEl && !e.target.classList.contains("bg")) return;
      const sx = e.clientX - x, sy = e.clientY - y;
      svgEl.classList.add("panning"); svgEl.setPointerCapture(e.pointerId);
      const move = (ev) => { x = ev.clientX - sx; y = ev.clientY - sy; apply(); };
      const up = () => { svgEl.classList.remove("panning"); svgEl.removeEventListener("pointermove", move); svgEl.removeEventListener("pointerup", up); };
      svgEl.addEventListener("pointermove", move); svgEl.addEventListener("pointerup", up);
    });
  }
  apply();
  return vp;
}

/** Makes a <g> draggable in content space; onMove(dx, dy), onEnd(). */
export function draggable(g, vp, onMove, onEnd) {
  g.addEventListener("pointerdown", (e) => {
    if (e.button !== 0) return;
    e.stopPropagation();
    let lx = e.clientX, ly = e.clientY, moved = 0;
    g.setPointerCapture(e.pointerId);
    const move = (ev) => { const dx = (ev.clientX - lx) / vp.k, dy = (ev.clientY - ly) / vp.k; lx = ev.clientX; ly = ev.clientY; moved += Math.abs(dx) + Math.abs(dy); if (moved > 3) { g.classList.add("dragging"); onMove(dx, dy); } };
    const up = () => { g.removeEventListener("pointermove", move); g.removeEventListener("pointerup", up); g.classList.remove("dragging"); g.moved = moved > 3; setTimeout(() => (g.moved = false), 0); if (moved > 3) onEnd(); };
    g.addEventListener("pointermove", move); g.addEventListener("pointerup", up);
  });
}

/**
 * How one server's writes reach storage and its queries read it: a
 * pipeline with dots moving at the rates of the latest point.
 */
export function storageFlow(host, p, health, system) {
  const W = Math.max(560, host.clientWidth || 760), H = Math.max(260, host.clientHeight || 300);
  const reps = health?.replication?.role === "primary" ? health.replication.replicas || [] : [];
  let svgEl = host.querySelector("svg.flow");
  const fresh = !svgEl || Math.abs(+svgEl.dataset.w - W) > 4 || +svgEl.dataset.reps !== reps.length;
  const cols = 5, cw = (W - 40) / cols, bw = Math.min(150, cw - 26), bh = 70;
  const X = (i) => 20 + i * cw + (cw - bw) / 2;
  const yW = 34, yR = H - bh - 30;
  const stages = [
    { id: "in", x: X(0), y: (yW + yR) / 2, t: "Clients", s: "HTTP and gRPC", v: () => fmt.si(p.http_s) + "/s" },
    { id: "queue", x: X(1), y: yW, t: "Write queue", s: `${fmt.si(p.writes_s)} writes/s`, v: () => fmt.n(p.write_queue ?? 0) },
    { id: "wal", x: X(2), y: yW, t: "Write-ahead log", s: "group commit", v: () => fmt.bytes(p.wal_bytes) },
    { id: "mem", x: X(3), y: yW, t: "Memtable", s: "rows not flushed", v: () => fmt.si(p.memtable_docs ?? 0) },
    { id: "seg", x: X(4), y: (yW + yR) / 2, t: "Segments", s: `${fmt.n(p.segments)} · ${fmt.si(p.deleted_docs ?? 0)} deleted`, v: () => fmt.bytes(p.file_bytes) },
    { id: "adm", x: X(1), y: yR, t: "Admission", s: system ? `${p.slots_free ?? "–"} of ${system.limits.max_concurrent} slots free` : "slots", v: () => fmt.n(p.running ?? 0) + " running" },
    { id: "exec", x: X(2) + (X(3) - X(2)) / 2, y: yR, t: "Execute", s: `${fmt.si(p.qps)} q/s · p99 ${fmt.ms(p.p99_ms)}`, v: () => fmt.n(p.pool_queued ?? 0) + " queued" },
  ];
  const at = (id) => stages.find((x) => x.id === id);
  const links = [
    ["in", "queue", "w"], ["queue", "wal", "w"], ["wal", "mem", "w"], ["mem", "seg", "w"],
    ["in", "adm", "r"], ["adm", "exec", "r"], ["exec", "seg", "r"],
  ];
  const mid = (a, side) => side === "r" ? [a.x + bw, a.y + bh / 2] : side === "l" ? [a.x, a.y + bh / 2] : side === "t" ? [a.x + bw / 2, a.y] : [a.x + bw / 2, a.y + bh];
  const path = (a, b) => {
    const [x1, y1] = mid(a, "r"), [x2, y2] = mid(b, "l");
    const mx = (x1 + x2) / 2;
    return { d: `M${x1},${y1} C${mx},${y1} ${mx},${y2} ${x2},${y2}`, x1, y1, x2, y2 };
  };
  if (fresh) {
    svgEl = s("svg", { class: "flow", viewBox: `0 0 ${W} ${H}`, style: { width: "100%", height: H + "px" }, dataset: { w: W, reps: reps.length } });
    host.replaceChildren(svgEl);
    const lanes = s("g");
    lanes.append(s("text", { class: "flow-sub", x: 20, y: 18 }, "WRITES"), s("text", { class: "flow-sub", x: 20, y: H - 8 }, "QUERIES"));
    svgEl.append(lanes);
    for (const [a, b, k] of links) {
      const pth = path(at(a), at(b));
      svgEl.append(s("path", { class: "edge", d: pth.d, dataset: { k, a, b }, id: `fl-${a}-${b}` }));
    }
    reps.forEach((r, i) => {
      const wal = at("wal");
      svgEl.append(s("path", { class: "edge repl", d: `M${wal.x + bw / 2},${wal.y + bh} L${wal.x + bw / 2},${H / 2 - 4 + i * 22}` }));
    });
    const g = s("g", { class: "stages" });
    svgEl.append(g, s("g", { class: "parts" }));
  }
  const g = svgEl.querySelector(".stages");
  g.replaceChildren();
  for (const st of stages) {
    const warn = (st.id === "queue" && p.write_queue > 0) || (st.id === "adm" && p.slots_free === 0) || (st.id === "seg" && p.merge_failing) || (st.id === "mem" && p.flush_failing);
    const gg = s("g", { transform: `translate(${st.x},${st.y})` });
    gg.append(s("rect", { class: "flow-stage", width: bw, height: bh, rx: 10, style: { stroke: warn ? "var(--warn)" : null, strokeWidth: warn ? 2 : 1 } }),
      s("text", { class: "flow-lbl", x: 12, y: 20 }, st.t), s("text", { class: "flow-val", x: 12, y: 44 }, st.v()), s("text", { class: "flow-sub", x: 12, y: 61 }, st.s));
    if (st.id === "seg" && p.merges_running) gg.append(s("text", { class: "flow-sub", x: bw - 10, y: 20, "text-anchor": "end", style: { fill: "var(--s3)" } }, `${p.merges_running} merging`));
    g.append(gg);
  }
  if (reps.length) {
    const wal = at("wal");
    reps.forEach((r, i) => {
      const y = H / 2 - 4 + i * 22;
      const gg = s("g", { transform: `translate(${wal.x + bw / 2 - 70},${y})` });
      gg.append(s("rect", { width: 140, height: 18, rx: 9, style: { fill: r.in_sync ? "var(--good-soft)" : "var(--warn-soft)", stroke: "var(--line)" } }),
        s("text", { x: 70, y: 13, "text-anchor": "middle", style: { fontSize: "10.5px", fill: "var(--text)" } }, `${short(r.url)} · ${r.in_sync ? "in sync" : fmt.si(r.behind) + " behind"}`));
      g.append(gg);
    });
  }
  // Dots moving along the links at the point's rates.
  const parts = svgEl.querySelector(".parts");
  const rate = { w: p.rows_s || p.writes_s || 0, r: p.qps || 0 };
  if (!svgEl.anim) {
    svgEl.anim = { dots: [], last: performance.now() };
    const tick = (now) => {
      if (!svgEl.isConnected) return;
      const a = svgEl.anim, dt = Math.min(0.1, (now - a.last) / 1000); a.last = now;
      for (const k of ["w", "r"]) {
        const n = a.rate?.[k] || 0;
        a.acc = a.acc || { w: 0, r: 0 };
        a.acc[k] += Math.min(12, Math.log2(1 + n) * 1.3) * dt;
        while (a.acc[k] >= 1 && a.dots.length < 80) {
          a.acc[k] -= 1;
          const route = k === "w" ? ["in-queue", "queue-wal", "wal-mem", "mem-seg"] : ["in-adm", "adm-exec", "exec-seg"];
          const dot = { route, step: 0, t: 0, el: s("circle", { r: 3.2, class: "particle" + (k === "w" ? " w" : "") }) };
          a.dots.push(dot); parts.append(dot.el);
        }
      }
      a.dots = a.dots.filter((d) => {
        d.t += dt * 1.1;
        if (d.t >= 1) { d.t = 0; d.step++; }
        if (d.step >= d.route.length) { d.el.remove(); return false; }
        const pe = svgEl.querySelector("#fl-" + d.route[d.step]);
        if (!pe) { d.el.remove(); return false; }
        const len = pe.getTotalLength(); const pt = pe.getPointAtLength(d.t * len);
        d.el.setAttribute("cx", pt.x); d.el.setAttribute("cy", pt.y);
        return true;
      });
      requestAnimationFrame(tick);
    };
    if (!matchMedia("(prefers-reduced-motion: reduce)").matches) requestAnimationFrame(tick);
  }
  svgEl.anim.rate = rate;
}
