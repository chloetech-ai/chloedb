// The key slots as a map: the 4,096 slots in a 64 × 64 grid, each coloured
// by the reads and writes the coordinator routed to it over the window
// picked (GET /v1/cluster/slots?window=), with the shards' boundaries drawn
// from the slot runs of /v1/cluster. It says which shard is hot and
// whether moving slots would cool it: a hot range is many warm cells in one
// shard, a hot slot is one bright cell.
//
// Drawn on a canvas (4,096 elements would be too many on a page that
// redraws every poll); the slots most counted are listed beside it as text.

import { h, clear, fmt, api, seg, store, tipOn, hideTip, every, bus, ApiError } from "../core.js";

const SLOTS = 4096, SIDE = 64;
const WINDOWS = [["1m", "1 min"], ["5m", "5 min"], ["15m", "15 min"]];
const VERDICT = { quiet: "", even: "ok", "hot range": "warn", "hot slot": "bad" };
const STEPS = 7; // --seq-1 .. --seq-7

/** A rate a second, short. */
const rate = (v) => (v == null ? "–" : v >= 100 ? fmt.n(v, 0) : v >= 10 ? v.toFixed(0) : v >= 1 ? v.toFixed(1) : v > 0 ? v.toFixed(2) : "0");
/** A window's real length: windows are cut at marks, so 5 min is 4 to 5. */
function span(secs) {
  if (secs == null) return "";
  if (secs < 90) return `${Math.round(secs)} s`;
  const m = Math.floor(secs / 60), s = Math.round(secs % 60);
  return s ? `${m} min ${s} s` : `${m} min`;
}

/**
 * The card. `onShard(i)` opens a shard; `model()` gives the page's model of
 * the cluster ({shards: [{i, slots: [[a, b], ...]}]}). `update()` redraws
 * from the model after a poll of /v1/cluster; the card polls the slots
 * itself, every 10 s while it is on the page.
 */
export function slotMap({ onShard, model, past, onBalance, canBalance }) {
  let win = WINDOWS.some(([k]) => k === store.get("cluster.slotWindow")) ? store.get("cluster.slotWindow") : "5m";
  let data = null, err = null, gone = false, hover = -1, loading = false;
  const owner = new Int16Array(SLOTS).fill(-1);

  const head = h("div", { class: "sm-head" });
  const cv = h("canvas", { class: "sm-canvas", role: "img", "aria-label": "Load on each key slot" });
  const list = h("div", { class: "sm-top" });
  const scale = h("div", { class: "sm-scale" });
  const body = h("div", { class: "sm-body" }, h("div", { class: "sm-grid" }, cv, scale), list);
  const winSeg = seg(WINDOWS.map(([k, l]) => [k, l, `Over the last ${l}`]), win, (v) => { win = v; store.set("cluster.slotWindow", v); load(); }, { "aria-label": "Window" });
  const el = h("section", { class: "card sm-card" },
    h("div", { class: "hd" }, h("h3", {}, "Key slots"), h("span", { class: "sub" }, "reads and writes routed by key, slot by slot"), h("div", { class: "tools" }, winSeg)),
    h("div", { class: "bd" }, head, body));

  // ---------------------------------------------------------------- data
  async function load() {
    if (loading || gone || !el.isConnected) return;
    loading = true;
    const want = win;
    try { data = await api(`/v1/cluster/slots?window=${want}`); err = null; }
    catch (e) {
      // An older coordinator has no such route: the card is not shown.
      if (e instanceof ApiError && e.status === 404) { gone = true; el.remove(); return; }
      err = e instanceof ApiError && e.status === 403 ? "Reading the load on the slots needs a key that is not limited to tables." : `The coordinator did not answer: ${e.message}`;
    } finally { loading = false; }
    // The window was changed while this one was asked: ask for that one.
    if (want !== win) { load(); return; }
    draw();
  }

  function owners() {
    owner.fill(-1);
    for (const sh of model()?.shards || []) for (const [a, b] of sh.slots || []) for (let s = Math.max(0, a); s < Math.min(SLOTS, b); s++) owner[s] = sh.i;
  }

  /** Per slot: reads and writes a second over the window. */
  function rates() {
    const r = new Float32Array(SLOTS), w = new Float32Array(SLOTS);
    const secs = data?.slots_secs || 1;
    for (const [s, , rd, wr] of data?.slots || []) { r[s] = rd / secs; w[s] = wr / secs; }
    return { r, w };
  }

  // ---------------------------------------------------------------- drawing
  let R = null, max = 0;
  function draw() {
    if (gone) return;
    owners();
    const W = data?.windows?.[win];
    // The head: the verdict, in the coordinator's words.
    if (past?.()) clear(head, h("p", { class: "small dim" }, "The load now: it is not kept for past windows."));
    else clear(head);
    if (err) { head.append(h("p", { class: "small cc-warn" }, err)); }
    else if (data && data.by_slot === false) { head.append(h("p", { class: "small dim" }, data.why || "This cluster's writes go by the hash before slots, so nothing is counted by slot.")); }
    else if (W) {
      const cls = VERDICT[W.verdict] ?? "";
      head.append(h("div", { class: "sm-verdict row", style: { gap: "8px", alignItems: "baseline", flexWrap: "wrap" } },
        h("span", { class: "pill " + cls }, W.verdict),
        W.verdict !== "quiet" ? h("span", { class: "small", title: "The busiest shard's keyed load as a multiple of the mean of the shards holding slots" }, `skew ${W.skew.toFixed(2)}×`) : null,
        // A hot range is what moving slots cools: offer it, the plan shown first.
        W.verdict === "hot range" && onBalance && canBalance?.() !== false ? h("button", { class: "btn xs", type: "button", title: "Move the busy slots from the busiest shard to the quietest: the plan is shown first", onclick: () => onBalance(win === "1m" ? "5m" : win) }, "Even out by load…") : null,
        h("span", { class: "small dim" }, `${rate(W.keyed_per_s)} keyed/s · ${rate(W.fanout_per_s)} sent to every shard/s · over ${span(W.secs)}`)),
        h("p", { class: "small dim sm-why" }, W.why[0].toUpperCase() + W.why.slice(1) + "."));
    } else if (!data) head.append(h("p", { class: "small dim" }, "Asking the coordinator…"));

    R = rates();
    max = 0;
    for (let s = 0; s < SLOTS; s++) max = Math.max(max, R.r[s] + R.w[s]);
    paint();
    drawScale();
    drawList(W);
  }

  const css = (n) => getComputedStyle(document.documentElement).getPropertyValue(n).trim();
  /** 0 for none, else 1..STEPS on a log scale from max/1000 to max. */
  function step(v) {
    if (!(v > 0) || !(max > 0)) return 0;
    const f = Math.log10(v / max) / 3 + 1; // max → 1, max/1000 → 0
    return Math.max(1, Math.min(STEPS, Math.ceil(f * STEPS)));
  }

  function paint() {
    const box = cv.parentElement?.clientWidth || 512;
    const cell = Math.max(3, Math.min(9, Math.floor(Math.min(box, 576) / SIDE)));
    const px = cell * SIDE, dpr = window.devicePixelRatio || 1;
    cv.width = px * dpr; cv.height = px * dpr;
    cv.style.width = cv.style.height = px + "px";
    const g = cv.getContext("2d");
    g.setTransform(dpr, 0, 0, dpr, 0, 0);
    const fill = [css("--surface-2") || "#eee", ...Array.from({ length: STEPS }, (_, k) => css(`--seq-${k + 1}`))];
    for (let s = 0; s < SLOTS; s++) {
      const x = (s % SIDE) * cell, y = Math.floor(s / SIDE) * cell;
      g.fillStyle = fill[step(R ? R.r[s] + R.w[s] : 0)];
      g.fillRect(x, y, cell, cell);
    }
    // The shards' boundaries: between neighbours held by different shards.
    g.strokeStyle = css("--text") || "#000";
    g.globalAlpha = 0.55;
    g.lineWidth = 1;
    g.beginPath();
    for (let s = 0; s < SLOTS; s++) {
      const x = (s % SIDE) * cell, y = Math.floor(s / SIDE) * cell;
      if (s % SIDE < SIDE - 1 && owner[s] !== owner[s + 1]) { g.moveTo(x + cell + 0.5, y); g.lineTo(x + cell + 0.5, y + cell); }
      if (s + SIDE < SLOTS && owner[s] !== owner[s + SIDE]) { g.moveTo(x, y + cell + 0.5); g.lineTo(x + cell, y + cell + 0.5); }
    }
    g.stroke();
    g.globalAlpha = 1;
    // The slots of the shard under the pointer.
    if (hover >= 0 && owner[hover] >= 0) {
      g.strokeStyle = css("--accent") || "#36f";
      g.lineWidth = 2;
      const x = (hover % SIDE) * cell, y = Math.floor(hover / SIDE) * cell;
      g.strokeRect(x + 1, y + 1, cell - 2, cell - 2);
    }
    cv.dataset.cell = cell;
  }

  function drawScale() {
    if (!(max > 0)) { clear(scale, h("span", { class: "small dim" }, data?.slots ? "No slot was read or written by key in this window." : "")); return; }
    const sw = (k) => h("i", { class: "sm-sw", style: { background: k ? `var(--seq-${k})` : "var(--surface-2)" } });
    clear(scale, h("span", { class: "small dim" }, "none"), sw(0), h("span", { class: "small dim" }, rate(max / 1000)), ...Array.from({ length: STEPS }, (_, k) => sw(k + 1)), h("span", { class: "small dim" }, `${rate(max)}/s`),
      h("span", { class: "small dim sm-note" }, "a line between shards · slot 0 top left, 64 a row"));
  }

  function drawList(W) {
    const top = (W?.top || []).slice(0, 10);
    if (!top.length) { clear(list); return; }
    clear(list, h("div", { class: "small dim", style: { marginBottom: "4px" } }, "Most counted"),
      h("ol", { class: "sm-list" }, top.map((t) => h("li", {},
        h("button", { class: "linkish mono", type: "button", title: `Open shard ${t.shard}`, onclick: () => onShard(t.shard), onpointerenter: () => { hover = t.slot; paint(); }, onpointerleave: () => { hover = -1; paint(); } },
          `slot ${t.slot}`), h("span", { class: "dim" }, ` shard ${t.shard} · `),
        h("span", { class: "num" }, `${rate(t.reads_per_s)} r · ${rate(t.writes_per_s)} w /s`)))));
  }

  // ---------------------------------------------------------------- pointer
  const at = (ev) => {
    const cell = +cv.dataset.cell || 8, r = cv.getBoundingClientRect();
    const x = Math.floor((ev.clientX - r.left) / cell), y = Math.floor((ev.clientY - r.top) / cell);
    return x >= 0 && x < SIDE && y >= 0 && y < SIDE ? y * SIDE + x : -1;
  };
  cv.addEventListener("pointermove", (ev) => { const s = at(ev); if (s !== hover) { hover = s; paint(); } });
  cv.addEventListener("pointerdown", (ev) => { hover = at(ev); });
  cv.addEventListener("pointerleave", () => { hover = -1; paint(); });
  cv.addEventListener("click", (ev) => { const s = at(ev); if (s >= 0 && owner[s] >= 0) { hideTip(); onShard(owner[s]); } });
  tipOn(cv, () => {
    if (hover < 0) return "";
    const r = R ? R.r[hover] : 0, w = R ? R.w[hover] : 0;
    return h("div", {}, h("b", {}, `slot ${hover}`), owner[hover] >= 0 ? ` · shard ${owner[hover]}` : "",
      h("div", { class: "small" }, `${rate(r)} reads/s · ${rate(w)} writes/s`),
      h("div", { class: "small dim" }, owner[hover] >= 0 ? "click to open its shard" : ""));
  });

  const offTheme = bus.on("theme", () => paint());
  // A key given after the card asked without one (OLU-308): asked again at once.
  const offToken = bus.on("token", () => { if (err) load(); });
  const ro = typeof ResizeObserver === "function" ? new ResizeObserver(() => { if (R) paint(); }) : null;
  ro?.observe(body);
  const stopPoll = every(10000, load, false);
  queueMicrotask(load);
  draw();

  return {
    el,
    /** Redraw after the cluster's map changed (a poll of /v1/cluster). */
    update() {
      if (gone || !el.isConnected) return;
      // The card was not on the page yet when it first asked (it is built first).
      if (!data && !err && !loading) load();
      draw();
    },
    stop() { stopPoll(); offTheme?.(); offToken(); ro?.disconnect(); hideTip(); },
  };
}
