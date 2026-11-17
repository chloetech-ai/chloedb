// A 2-D canvas map of ~66k points laid out as topic islands (k-means on the embeddings,
// placed and packed at load; see lib/layout.mjs). Zoom (wheel), pan (drag), pick (grid),
// a time-lapse reveal, bucket filters, a highlight set, and the islands' labels drawn on
// the canvas. Servers with no declared secret are drawn dim so the secret-bearing ones pop.
const DIM = '#7a7f8f';

export class MapView {
  constructor(canvas, data, { onPick, onHover } = {}) {
    this.c = canvas; this.g = canvas.getContext('2d', { alpha: false });
    this.d = data; this.onPick = onPick; this.onHover = onHover;
    this.filter = new Set(data.bucketOrder);
    this.secretOnly = false;
    this.timeMax = data.day_span;
    this.highlight = null;
    this.colorByIdx = data.bucketOrder.map(k => data.colors[k] || '#888');
    this.noneIdx = data.bucketOrder.indexOf('none');
    this.islands = (data.islands || []).slice().sort((a, b) => b.size - a.size);
    this.islandPos = new Map(this.islands.map((t, k) => [t.id, k]));
    this.hoverI = -1;
    this.labelBoxes = [];
    this.bounds();
    this.buildGrid();
    this.resize();
    this.fit();
    this._bind();
    window.addEventListener('resize', () => { this.resize(); this.fit(); this.draw(); });
  }
  bounds() {
    const d = this.d; let x0 = Infinity, x1 = -Infinity, y0 = Infinity, y1 = -Infinity;
    for (let i = 0; i < d.n; i++) { const x = d.x[i], y = d.y[i]; if (x < x0) x0 = x; if (x > x1) x1 = x; if (y < y0) y0 = y; if (y > y1) y1 = y; }
    if (!Number.isFinite(x0)) { x0 = 0; x1 = 1000; y0 = 0; y1 = 1000; }
    this.bx0 = x0; this.by0 = y0; this.bw = Math.max(1, x1 - x0); this.bh = Math.max(1, y1 - y0);
  }
  buildGrid() {
    this.gN = 200; this.cells = Array.from({ length: this.gN * this.gN }, () => []);
    for (let i = 0; i < this.d.n; i++) this.cells[this.cellOf(this.d.x[i], this.d.y[i])].push(i);
  }
  cellXY(x, y) {
    const cx = Math.min(this.gN - 1, Math.max(0, ((x - this.bx0) / this.bw * this.gN) | 0));
    const cy = Math.min(this.gN - 1, Math.max(0, ((y - this.by0) / this.bh * this.gN) | 0));
    return [cx, cy];
  }
  cellOf(x, y) { const [cx, cy] = this.cellXY(x, y); return cy * this.gN + cx; }
  resize() {
    const r = this.c.getBoundingClientRect();
    this.dpr = Math.min(2, window.devicePixelRatio || 1);
    this.W = r.width; this.H = r.height;
    this.c.width = Math.round(this.W * this.dpr); this.c.height = Math.round(this.H * this.dpr);
    this.g.setTransform(this.dpr, 0, 0, this.dpr, 0, 0);
  }
  fit() {
    // room at the top for labels, at the bottom for the time bar, at the right for the legend
    const padL = 24, padR = 24, padT = 70, padB = 76;
    const s = Math.min((this.W - padL - padR) / this.bw, (this.H - padT - padB) / this.bh);
    this.scale = s;
    this.tx = padL + (this.W - padL - padR - this.bw * s) / 2 - this.bx0 * s;
    this.ty = padT + (this.H - padT - padB - this.bh * s) / 2 - this.by0 * s;
  }
  zoomTo(x, y, r) {
    const s = Math.min(this.W, this.H) / (r * 2.8);
    this.scale = s; this.tx = this.W / 2 - x * s; this.ty = this.H / 2 - y * s; this.draw();
  }
  sx(x) { return x * this.scale + this.tx; }
  sy(y) { return y * this.scale + this.ty; }
  inv(px, py) { return [(px - this.tx) / this.scale, (py - this.ty) / this.scale]; }
  visible(i) {
    const d = this.d;
    if (this.secretOnly && !d.secret[i]) return false;
    if (!this.filter.has(d.bucketOrder[d.cls[i]])) return false;
    if (d.day[i] > this.timeMax) return false;
    return true;
  }
  draw() {
    const g = this.g, d = this.d;
    g.fillStyle = '#05070f'; g.fillRect(0, 0, this.W, this.H);
    const ps = Math.max(1.2, Math.min(6, this.scale * 1.5));
    const half = ps / 2;
    const hl = this.highlight;
    const seen = new Int32Array(this.islands.length);
    const W = this.W, H = this.H;

    // faint island grounds
    g.globalAlpha = 1;
    for (const t of this.islands) {
      const X = this.sx(t.x), Y = this.sy(t.y), R = t.r * this.scale;
      if (X + R < 0 || X - R > W || Y + R < 0 || Y - R > H) continue;
      g.beginPath(); g.arc(X, Y, R + ps, 0, 6.2832);
      g.fillStyle = 'rgba(91,108,255,0.035)'; g.fill();
    }
    // pass 1: servers that declare no secret, dim
    g.globalAlpha = hl ? 0.08 : 0.22;
    g.fillStyle = DIM;
    let shown = 0;
    for (let i = 0; i < d.n; i++) {
      if (!this.visible(i)) continue;
      shown++;
      const k = this.islandPos.get(d.island[i]); if (k != null) seen[k]++;
      if (d.cls[i] !== this.noneIdx) continue;
      if (hl && hl.has(d.names[i])) continue;
      const X = this.sx(d.x[i]), Y = this.sy(d.y[i]);
      if (X < -4 || X > W + 4 || Y < -4 || Y > H + 4) continue;
      g.fillRect(X - half, Y - half, ps, ps);
    }
    // pass 2: secret-bearing servers, in their credential colour
    g.globalAlpha = hl ? 0.18 : 0.95;
    for (let i = 0; i < d.n; i++) {
      if (d.cls[i] === this.noneIdx || !this.visible(i)) continue;
      if (hl && hl.has(d.names[i])) continue;
      const X = this.sx(d.x[i]), Y = this.sy(d.y[i]);
      if (X < -4 || X > W + 4 || Y < -4 || Y > H + 4) continue;
      g.fillStyle = this.colorByIdx[d.cls[i]];
      g.fillRect(X - half, Y - half, ps, ps);
    }
    // pass 3: highlighted servers, bright and larger
    if (hl) {
      g.globalAlpha = 1;
      const hs = Math.max(3.5, ps + 2.5);
      for (let i = 0; i < d.n; i++) {
        if (!hl.has(d.names[i]) || !this.visible(i)) continue;
        const X = this.sx(d.x[i]), Y = this.sy(d.y[i]);
        g.fillStyle = d.cls[i] === this.noneIdx ? '#c7cdf0' : this.colorByIdx[d.cls[i]];
        g.fillRect(X - hs / 2, Y - hs / 2, hs, hs);
        g.strokeStyle = 'rgba(255,255,255,.8)'; g.lineWidth = 1; g.strokeRect(X - hs / 2 - .5, Y - hs / 2 - .5, hs + 1, hs + 1);
      }
    }
    if (this.hoverI >= 0 && this.visible(this.hoverI)) {
      const X = this.sx(d.x[this.hoverI]), Y = this.sy(d.y[this.hoverI]);
      g.globalAlpha = 1; g.strokeStyle = '#fff'; g.lineWidth = 1.5; g.beginPath(); g.arc(X, Y, 7, 0, 6.2832); g.stroke();
    }
    g.globalAlpha = 1;
    this.drawLabels(seen);
    this.shown = shown;
    if (this._onDraw) this._onDraw(shown);
  }
  drawLabels(seen) {
    const g = this.g; const boxes = []; this.labelBoxes = [];
    const fs = Math.max(10.5, Math.min(15, 9 + this.scale * 2.2));
    g.textAlign = 'center'; g.textBaseline = 'bottom';
    this.islands.forEach((t, k) => {
      const frac = t.size ? seen[k] / t.size : 0;
      if (frac <= 0.02) return;
      const X = this.sx(t.x), R = t.r * this.scale, Y = this.sy(t.y) - R - 4;
      if (X < -100 || X > this.W + 100 || Y < 10 || Y > this.H + 30) return;
      g.font = `650 ${fs}px Inter, system-ui, sans-serif`;
      const w1 = g.measureText(t.label).width;
      const sub = `${seen[k].toLocaleString('en-US')} · ${Math.round(100 * t.secret / Math.max(1, t.size))}% declare a secret`;
      g.font = `500 ${fs - 2.5}px Inter, system-ui, sans-serif`;
      const w2 = g.measureText(sub).width;
      const w = Math.max(w1, w2) + 8, h = fs * 2 + 4;
      const box = [X - w / 2, Y - h, X + w / 2, Y];
      if (boxes.some(b => box[0] < b[2] && box[2] > b[0] && box[1] < b[3] && box[3] > b[1])) return;
      boxes.push(box); this.labelBoxes.push({ box, t });
      const a = Math.min(1, frac * 3);
      g.lineJoin = 'round'; g.lineWidth = 4; g.strokeStyle = `rgba(5,7,15,${0.85 * a})`;
      g.font = `650 ${fs}px Inter, system-ui, sans-serif`;
      g.strokeText(t.label, X, Y - fs + 1); g.fillStyle = `rgba(233,237,255,${a})`; g.fillText(t.label, X, Y - fs + 1);
      g.font = `500 ${fs - 2.5}px Inter, system-ui, sans-serif`;
      g.strokeText(sub, X, Y); g.fillStyle = `rgba(154,166,201,${a})`; g.fillText(sub, X, Y);
    });
  }
  pick(px, py) {
    const [dx, dy] = this.inv(px, py);
    const [gx, gy] = this.cellXY(dx, dy);
    const rpx = 8 / this.scale;
    let best = -1, bd = rpx * rpx;
    const R = Math.max(1, Math.ceil(rpx / (this.bw / this.gN)));
    for (let cy = gy - R; cy <= gy + R; cy++) for (let cx = gx - R; cx <= gx + R; cx++) {
      if (cx < 0 || cy < 0 || cx >= this.gN || cy >= this.gN) continue;
      for (const i of this.cells[cy * this.gN + cx]) {
        if (!this.visible(i)) continue;
        const ddx = this.d.x[i] - dx, ddy = this.d.y[i] - dy; const dd = ddx * ddx + ddy * ddy;
        if (dd < bd || (dd === bd && this.d.cls[i] !== this.noneIdx)) { bd = dd; best = i; }
      }
    }
    return best;
  }
  labelAt(px, py) { const hit = this.labelBoxes.find(({ box }) => px >= box[0] && px <= box[2] && py >= box[1] && py <= box[3]); return hit ? hit.t : null; }
  zoomAt(px, py, factor) {
    const [dx, dy] = this.inv(px, py);
    this.scale = Math.max(0.05, Math.min(80, this.scale * factor));
    this.tx = px - dx * this.scale; this.ty = py - dy * this.scale;
    this.draw();
  }
  _bind() {
    const c = this.c; let dragging = false, lx = 0, ly = 0, moved = 0;
    c.addEventListener('wheel', e => { e.preventDefault(); const r = c.getBoundingClientRect(); this.zoomAt(e.clientX - r.left, e.clientY - r.top, e.deltaY < 0 ? 1.15 : 1 / 1.15); }, { passive: false });
    c.addEventListener('pointerdown', e => { dragging = true; moved = 0; lx = e.clientX; ly = e.clientY; c.setPointerCapture(e.pointerId); });
    c.addEventListener('pointermove', e => {
      const r = c.getBoundingClientRect(); const px = e.clientX - r.left, py = e.clientY - r.top;
      if (dragging) { this.tx += e.clientX - lx; this.ty += e.clientY - ly; lx = e.clientX; ly = e.clientY; moved += Math.abs(e.movementX) + Math.abs(e.movementY); this.draw(); return; }
      const i = this.pick(px, py); if (i !== this.hoverI) { this.hoverI = i; this.draw(); if (this.onHover) this.onHover(i, px, py); }
      c.style.cursor = i < 0 && this.labelAt(px, py) ? 'zoom-in' : '';
    });
    c.addEventListener('pointerup', e => {
      dragging = false;
      if (moved >= 5) return;
      const r = c.getBoundingClientRect(); const px = e.clientX - r.left, py = e.clientY - r.top;
      const t = this.labelAt(px, py); if (t) { this.zoomTo(t.x, t.y, t.r); return; }
      const i = this.pick(px, py); if (i >= 0 && this.onPick) this.onPick(i);
    });
    c.addEventListener('pointerleave', () => { this.hoverI = -1; if (this.onHover) this.onHover(-1); this.draw(); });
  }
  setFilter(set) { this.filter = set; this.draw(); }
  setSecretOnly(v) { this.secretOnly = v; this.draw(); }
  setTime(t) { this.timeMax = t; this.draw(); }
  setHighlight(set) { this.highlight = set; this.draw(); }
  onDraw(fn) { this._onDraw = fn; }
}
