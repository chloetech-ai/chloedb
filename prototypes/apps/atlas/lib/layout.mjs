// MCP Atlas — the map's layout: topic islands. No model calls; deterministic.
//
//   1. k-means (k = 48, seeded k-means++ init) on the L2-normalised potion-base-8M vectors
//      ChloeDB built for each server (title + description).
//   2. Each island is labelled with the words most over-represented in its servers' titles and
//      descriptions: a word's share of the island's servers against its share of all servers
//      (lift >= 2), among words in at least 2.5% of the island's servers, scored lift x share.
//      Words that come mostly (> 50%) from one publisher are skipped, so a product name
//      repeated across one publisher's templated servers does not name an island.
//   3. Islands are placed by a 2-D PCA of the centroids (stretched to the map's aspect), then
//      pushed apart until no two discs overlap (disc area ∝ island size).
//   4. Inside an island, a point's direction comes from a PCA of that island's vectors and its
//      distance from the centre from its rank by cosine distance to the centroid (the nearest
//      at the centre), so each island is a filled disc whose inner layout means something.
//
// Coordinates: x in [0, 1000], y in [0, 1000 / ASPECT].

export const K = 48;
export const ASPECT = 2.5;
const SEED = 20261117;

function rng(seed) { // mulberry32
  let a = seed >>> 0;
  return () => { a = (a + 0x6D2B79F5) >>> 0; let t = a; t = Math.imul(t ^ (t >>> 15), t | 1); t ^= t + Math.imul(t ^ (t >>> 7), t | 61); return ((t ^ (t >>> 14)) >>> 0) / 4294967296; };
}

function normalise(buf, dim, n) {
  const out = new Float32Array(n * dim);
  for (let i = 0; i < n; i++) {
    let s = 0; for (let d = 0; d < dim; d++) { const v = buf[i * dim + d]; s += v * v; }
    s = Math.sqrt(s) || 1; for (let d = 0; d < dim; d++) out[i * dim + d] = buf[i * dim + d] / s;
  }
  return out;
}

function kmeans(X, dim, n, k, iters = 22) {
  const rand = rng(SEED);
  const C = new Float32Array(k * dim);
  // k-means++ on a sample
  const S = Math.min(n, 12000);
  const sample = Array.from({ length: S }, (_, i) => Math.floor(i * n / S));
  const dist2 = (i, c) => { let s = 0; for (let d = 0; d < dim; d++) { const t = X[i * dim + d] - C[c * dim + d]; s += t * t; } return s; };
  let first = sample[Math.floor(rand() * S)];
  C.set(X.subarray(first * dim, first * dim + dim), 0);
  const best = new Float64Array(S).fill(Infinity);
  for (let c = 1; c < k; c++) {
    let tot = 0;
    for (let j = 0; j < S; j++) { const v = dist2(sample[j], c - 1); if (v < best[j]) best[j] = v; tot += best[j]; }
    let r = rand() * tot, pick = sample[S - 1];
    for (let j = 0; j < S; j++) { r -= best[j]; if (r <= 0) { pick = sample[j]; break; } }
    C.set(X.subarray(pick * dim, pick * dim + dim), c * dim);
  }
  const assign = new Int32Array(n);
  for (let it = 0; it < iters; it++) {
    let changed = 0;
    for (let i = 0; i < n; i++) {
      let bc = 0, bd = Infinity;
      const off = i * dim;
      for (let c = 0; c < k; c++) {
        // vectors are unit length: minimising distance = maximising dot
        let dot = 0; const co = c * dim;
        for (let d = 0; d < dim; d++) dot += X[off + d] * C[co + d];
        const dd = -dot;
        if (dd < bd) { bd = dd; bc = c; }
      }
      if (assign[i] !== bc || it === 0) { changed++; assign[i] = bc; }
    }
    const sum = new Float64Array(k * dim), cnt = new Int32Array(k);
    for (let i = 0; i < n; i++) { const c = assign[i]; cnt[c]++; const off = i * dim, co = c * dim; for (let d = 0; d < dim; d++) sum[co + d] += X[off + d]; }
    for (let c = 0; c < k; c++) {
      if (!cnt[c]) { const p = Math.floor(rand() * n); C.set(X.subarray(p * dim, p * dim + dim), c * dim); continue; }
      let s = 0; for (let d = 0; d < dim; d++) { const v = sum[c * dim + d] / cnt[c]; C[c * dim + d] = v; s += v * v; }
      s = Math.sqrt(s) || 1; for (let d = 0; d < dim; d++) C[c * dim + d] /= s; // spherical k-means
    }
    if (it > 2 && changed < n * 0.002) break;
  }
  return { C, assign };
}

// top-2 principal directions of rows `idx` of X (sampled), by power iteration
function pca2Of(X, dim, idx) {
  const m = idx.length;
  const mean = new Float64Array(dim);
  for (const i of idx) for (let d = 0; d < dim; d++) mean[d] += X[i * dim + d];
  for (let d = 0; d < dim; d++) mean[d] /= m || 1;
  const step = Math.max(1, Math.floor(m / 2500));
  const rows = [];
  for (let j = 0; j < m; j += step) { const i = idx[j]; const r = new Float64Array(dim); for (let d = 0; d < dim; d++) r[d] = X[i * dim + d] - mean[d]; rows.push(r); }
  // power iteration on X^T X without forming it
  const apply = v => { const out = new Float64Array(dim); for (const r of rows) { let s = 0; for (let d = 0; d < dim; d++) s += r[d] * v[d]; for (let d = 0; d < dim; d++) out[d] += s * r[d]; } return out; };
  const nrm = v => { let s = 0; for (const x of v) s += x * x; s = Math.sqrt(s) || 1; for (let d = 0; d < dim; d++) v[d] /= s; return v; };
  const eig = (deflate) => {
    let v = nrm(Float64Array.from({ length: dim }, (_, d) => Math.sin(d * 1.3 + 0.7 + (deflate ? 2 : 0))));
    for (let it = 0; it < 40; it++) {
      let w = apply(v);
      if (deflate) { let p = 0; for (let d = 0; d < dim; d++) p += w[d] * deflate[d]; for (let d = 0; d < dim; d++) w[d] -= p * deflate[d]; }
      v = nrm(w);
    }
    return v;
  };
  const v1 = eig(null), v2 = eig(v1);
  return { mean, v1, v2 };
}

const STOP = new Set(('mcp server servers the and for with your from into via using use api apis tool tools ai agent agents ' +
  'model context protocol client app apps official remote local data service services integration integrations access ' +
  'connect connector connectors manage management platform simple based open source free powered get plus pro lite ' +
  'that this are can any all its their our you will about more other provides provide lets let allow allows enables ' +
  'support supports search query queries find list create run make help helps through without over across real time ' +
  'agentic llm llms claude cursor assistant assistants info information details using one new each also like way ' +
  'per own first stdio mcps hosted').split(' '));

export function words(text) {
  const t = String(text || '').toLowerCase();
  const out = new Set();
  for (const w of t.split(/[^a-z0-9]+/)) { if (w.length >= 3 && !STOP.has(w) && !/^\d+$/.test(w)) out.add(w); }
  return out;
}

// texts[i] = title + description of server i; pubs[i] its publisher. A word counts once per
// server. A word that comes mostly from one publisher (a product name repeated across that
// publisher's templated servers) is not used as a label.
export function labelIslands(texts, assign, k, pubs = null) {
  const n = texts.length;
  const all = new Map(); const per = Array.from({ length: k }, () => new Map()); const size = new Int32Array(k);
  const byPub = new Map(); // word -> Map(pub -> count)
  for (let i = 0; i < n; i++) {
    const c = assign[i]; size[c]++;
    for (const w of words(texts[i])) {
      all.set(w, (all.get(w) || 0) + 1); per[c].set(w, (per[c].get(w) || 0) + 1);
      if (pubs) { let m = byPub.get(w); if (!m) byPub.set(w, m = new Map()); m.set(pubs[i], (m.get(pubs[i]) || 0) + 1); }
    }
  }
  const topPubShare = w => { const m = byPub.get(w); if (!m) return 0; let mx = 0; for (const v of m.values()) if (v > mx) mx = v; return mx / all.get(w); };
  const labels = [];
  for (let c = 0; c < k; c++) {
    const minCount = Math.max(3, Math.ceil(size[c] * 0.025));
    const scored = [];
    for (const [w, cnt] of per[c]) {
      if (cnt < minCount) continue;
      const lift = (cnt / size[c]) / (all.get(w) / n);
      if (lift < 2) continue;
      if (pubs && topPubShare(w) > 0.5) continue;
      scored.push([w, Math.log(lift) * Math.sqrt(cnt / size[c])]);
    }
    scored.sort((a, b) => b[1] - a[1]);
    const pick = [];
    for (const [w] of scored) {
      if (pick.some(p => p.slice(0, 4) === w.slice(0, 4))) continue;
      pick.push(w); if (pick.length === 3) break;
    }
    labels.push({ words: pick, label: pick.join(' · ') || 'assorted' });
  }
  return labels;
}

// Place islands: PCA of the centroids gives each island a home; the homes are squeezed into an
// ellipse of the map's aspect sized to hold the discs, then every island is pulled toward its
// home while overlapping discs are pushed apart, so the archipelago is compact and keeps the
// centroids' arrangement.
function placeIslands(C, dim, k, sizes) {
  const idx = Array.from({ length: k }, (_, i) => i);
  const { mean, v1, v2 } = pca2Of(C, dim, idx);
  let H = idx.map(c => { let x = 0, y = 0; for (let d = 0; d < dim; d++) { const t = C[c * dim + d] - mean[d]; x += t * v1[d]; y += t * v2[d]; } return [x, y]; });
  const total = sizes.reduce((a, b) => a + b, 0);
  const R = sizes.map(s => Math.sqrt(s / total)); // disc areas proportional to size (sum of R^2 = 1)
  const discArea = Math.PI;                        // sum of pi R^2
  const area = discArea / 0.62;                    // packing density
  const b = Math.sqrt(area / (Math.PI * ASPECT)), a = ASPECT * b; // ellipse half-axes
  // normalise homes to the ellipse; anything beyond its rim is brought onto it
  const sx = Math.max(...H.map(p => Math.abs(p[0]))) || 1, sy = Math.max(...H.map(p => Math.abs(p[1]))) || 1;
  H = H.map(p => { let x = p[0] / sx, y = p[1] / sy; const m = Math.hypot(x, y); if (m > 1) { x /= m; y /= m; } return [x * a * 0.9, y * b * 0.9]; });
  const P = H.map(p => [...p]);
  const gap = 0.012;
  for (let it = 0; it < 900; it++) {
    const pull = it < 600 ? 0.04 : 0.01;
    for (let c = 0; c < k; c++) { P[c][0] += (H[c][0] - P[c][0]) * pull; P[c][1] += (H[c][1] - P[c][1]) * pull; }
    for (let pass = 0; pass < 3; pass++) {
      for (let i = 0; i < k; i++) for (let j = i + 1; j < k; j++) {
        const dx = P[j][0] - P[i][0], dy = P[j][1] - P[i][1];
        const d = Math.hypot(dx, dy) || 1e-6, need = R[i] + R[j] + gap;
        if (d >= need) continue;
        const push = need - d, ux = dx / d, uy = dy / d, wi = R[j] ** 2 / (R[i] ** 2 + R[j] ** 2), wj = 1 - wi;
        P[i][0] -= ux * push * wi; P[i][1] -= uy * push * wi;
        P[j][0] += ux * push * wj; P[j][1] += uy * push * wj;
      }
    }
  }
  return { P, R };
}

export function computeIslands(buf, dim, names, texts, { k = K, pubs = null } = {}) {
  const n = names.length;
  const X = normalise(buf, dim, n);
  const { C, assign } = kmeans(X, dim, n, k);
  const members = Array.from({ length: k }, () => []);
  for (let i = 0; i < n; i++) members[assign[i]].push(i);
  const sizes = members.map(m => m.length);
  const labels = labelIslands(texts, assign, k, pubs);
  const { P, R } = placeIslands(C, dim, k, sizes);

  const lx = new Float64Array(n), ly = new Float64Array(n);
  for (let c = 0; c < k; c++) {
    const idx = members[c]; if (!idx.length) continue;
    const { mean, v1, v2 } = pca2Of(X, dim, idx);
    const pts = idx.map(i => {
      let u = 0, v = 0, dot = 0;
      for (let d = 0; d < dim; d++) { const t = X[i * dim + d] - mean[d]; u += t * v1[d]; v += t * v2[d]; dot += X[i * dim + d] * C[c * dim + d]; }
      return { i, u, v, dist: 1 - dot };
    });
    pts.sort((a, b) => a.dist - b.dist || (a.i - b.i));
    const m = pts.length;
    pts.forEach((p, rank) => {
      const r = R[c] * Math.sqrt((rank + 0.5) / m);
      const len = Math.sqrt(p.u * p.u + p.v * p.v);
      // direction from the in-island PCA; a point with none gets a golden-angle direction
      const ang = len > 1e-9 ? Math.atan2(p.v, p.u) : rank * 2.39996;
      lx[p.i] = P[c][0] + r * Math.cos(ang); ly[p.i] = P[c][1] + r * Math.sin(ang);
    });
  }
  // fit to x in [0,1000], y in [0, 1000/ASPECT], uniform scale (discs stay round)
  let x0 = Infinity, x1 = -Infinity, y0 = Infinity, y1 = -Infinity;
  for (let c = 0; c < k; c++) { if (!sizes[c]) continue; x0 = Math.min(x0, P[c][0] - R[c]); x1 = Math.max(x1, P[c][0] + R[c]); y0 = Math.min(y0, P[c][1] - R[c]); y1 = Math.max(y1, P[c][1] + R[c]); }
  const W = 1000, H = 1000 / ASPECT, pad = 12;
  const s = Math.min((W - 2 * pad) / (x1 - x0), (H - 2 * pad) / (y1 - y0));
  const ox = (W - (x1 - x0) * s) / 2 - x0 * s, oy = (H - (y1 - y0) * s) / 2 - y0 * s;
  const coords = new Array(n);
  for (let i = 0; i < n; i++) coords[i] = [lx[i] * s + ox, ly[i] * s + oy];
  const islands = [];
  for (let c = 0; c < k; c++) {
    if (!sizes[c]) continue;
    islands.push({ id: c, label: labels[c].label, words: labels[c].words, size: sizes[c],
      x: +(P[c][0] * s + ox).toFixed(2), y: +(P[c][1] * s + oy).toFixed(2), r: +(R[c] * s).toFixed(2) });
  }
  return { coords, assign, islands };
}
