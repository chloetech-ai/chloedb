// Graph walks done in the engine, for a Strata that will not do them: a
// coordinator over shards refuses REACHABLE, hops() and ppr, since a row's
// edges point at rows other shards hold (docs/strata/graphs.md, "On a
// cluster"). The engine's graphs are small (a node per shared piece, per
// app and its pieces, per kind of change), so it reads a table's nodes once
// and walks them here as Strata does on one database: breadth first for
// REACHABLE, and forward push for ppr (alpha 0.15, eps 1e-4, both ways).

import { parseLinks } from './graph.mjs';

// The error a coordinator answers a graph query with.
export const isWalkRefused = (e) => /graph queries[^]*not supported through a coordinator/i.test(String(e?.message || e));

export class Walker {
  // rows: [{id, links}], links as Strata returns an EDGES field.
  constructor(rows) {
    this.rows = new Map();
    this.out = new Map();
    this.in = new Map();
    const add = (m, k, e) => { const l = m.get(k); if (l) l.push(e); else m.set(k, [e]); };
    for (const r of rows) {
      const links = Array.isArray(r.links) && r.links.every((l) => l && typeof l === 'object' && 'to' in l) ? r.links : parseLinks(r.links);
      this.rows.set(r.id, { ...r, links });
      for (const l of links) {
        add(this.out, r.id, { to: l.to, label: l.label || '' });
        add(this.in, l.to, { to: r.id, label: l.label || '' });
      }
    }
  }

  // A node's neighbours one way ('out' along its edges, 'in' against them,
  // 'both'), keeping the edges whose label is listed when labels are given.
  next(id, dir = 'out', labels = null) {
    const pick = (l) => (labels ? (l || []).filter((e) => labels.has(e.label)) : l || []);
    if (dir === 'out') return pick(this.out.get(id)).map((e) => e.to);
    if (dir === 'in') return pick(this.in.get(id)).map((e) => e.to);
    return [...pick(this.out.get(id)), ...pick(this.in.get(id))].map((e) => e.to);
  }

  // REACHABLE(f, seeds, hops, min_hops, dir, label): id -> hops, nearest
  // first, of the nodes that exist.
  reachable(seeds, { hops = 1, minHops = 1, dir = 'out', label = '' } = {}) {
    const labels = labelSet(label);
    const seen = new Map();
    let frontier = [...new Set(seeds)].filter((s) => this.rows.has(s) || this.out.has(s) || this.in.has(s));
    for (const s of frontier) seen.set(s, 0);
    for (let d = 1; d <= hops && frontier.length; d++) {
      const nextFrontier = [];
      for (const u of frontier) {
        for (const v of this.next(u, dir, labels)) {
          if (seen.has(v)) continue;
          seen.set(v, d);
          nextFrontier.push(v);
        }
      }
      frontier = nextFrontier;
    }
    const out = new Map();
    for (const [id, d] of seen) if (d >= minHops && this.rows.has(id)) out.set(id, d);
    return out;
  }

  // ppr(f, seeds, alpha, eps, dir): id -> score, by forward push (Andersen,
  // Chung and Lang), the restart mass split evenly over the seeds.
  ppr(seeds, { alpha = 0.15, eps = 1e-4, dir = 'both', label = '', maxPushes = 1e6 } = {}) {
    const labels = labelSet(label);
    const s = [...new Set(seeds)];
    const p = new Map();
    if (!s.length) return p;
    const r = new Map(s.map((x) => [x, 1 / s.length]));
    const queue = [...s];
    const queued = new Set(s);
    const due = (u) => (r.get(u) || 0) >= eps * Math.max(1, this.next(u, dir, labels).length);
    for (let pushes = 0; queue.length && pushes < maxPushes;) {
      const u = queue.shift();
      queued.delete(u);
      if (!due(u)) continue;
      pushes++;
      const ru = r.get(u);
      r.set(u, 0);
      p.set(u, (p.get(u) || 0) + alpha * ru);
      const nb = this.next(u, dir, labels);
      if (!nb.length) continue;
      const share = ((1 - alpha) * ru) / nb.length;
      for (const v of nb) {
        r.set(v, (r.get(v) || 0) + share);
        if (!queued.has(v) && due(v)) { queue.push(v); queued.add(v); }
      }
    }
    for (const id of [...p.keys()]) if (!this.rows.has(id)) p.delete(id);
    return p;
  }

  // The rows that pass `keep`, with their scores, best first, as ORDER BY
  // ppr(...) DESC returns them.
  byScore(scores, keep, limit) {
    const out = [];
    for (const r of this.rows.values()) if (keep(r)) out.push({ ...r, score: scores.get(r.id) || 0 });
    return out.sort((a, b) => b.score - a.score).slice(0, limit);
  }

  // The nodes a walk reached, as rows with `hops`, nearest first, then by id.
  rowsByHops(found, limit = 2000) {
    return [...found].sort((a, b) => a[1] - b[1] || (a[0] < b[0] ? -1 : a[0] > b[0] ? 1 : 0)).slice(0, limit).map(([id, hops]) => ({ ...this.rows.get(id), hops }));
  }
}

const labelSet = (label) => {
  const l = String(label || '').split(',').map((x) => x.trim()).filter(Boolean);
  return l.length ? new Set(l) : null;
};
