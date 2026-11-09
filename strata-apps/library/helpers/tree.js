// title: Trees
// description: Rows that point to a parent (folders, categories, comment threads, org charts, outlines) turned into a tree and back: nesting, flat lists with depth, paths, ancestors and descendants, and a check that a move would not make a loop.
// api: rows are objects with an id field and a parent field holding the parent's id (null, missing or '' for a top-level row);
//   opts: {id = 'id', parent = 'parent'} name those fields; ids compare as text (3 and '3' are the same); rows without an id are left out, and of rows sharing an id the first counts;
//   build(rows, opts) -> roots: copies of the rows with children: [...] added, in the order the rows came;
//     a row whose parent is missing or itself is a root; a loop of rows is broken at its earliest row, which becomes a root;
//   flatten(roots) -> the nodes in tree order (each before its children), as copies without children and with depth (0 for roots);
//   path(rows, id, opts) -> the rows from the root down to id, itself included ([] when id is unknown);
//   ancestors(rows, id, opts) -> the rows above id, its parent first and the root last ([] when none or unknown);
//   descendants(rows, id, opts) -> the ids of every row below id, in tree order, itself not included ([] when none or unknown);
//   moveCheck(rows, id, newParent, opts) -> true when id may be given newParent (null for the top level);
//     false when it would make a loop (newParent is id or one of its descendants), or when id or newParent is not among the rows;
//   sortTree(roots, compare(a, b)) -> copies of the roots with every level sorted by compare (stable; without compare the order is kept); the input is not changed
function helper() {
  const key = (v) => (v === null || v === undefined || v === '' ? null : String(v));

  function index(rows, opts) {
    const o = opts || {};
    const idf = o.id || 'id';
    const pf = o.parent || 'parent';
    const byId = new Map();
    const list = [];
    for (const r of Array.isArray(rows) ? rows : []) {
      if (!r || typeof r !== 'object') continue;
      const k = key(r[idf]);
      if (k === null || byId.has(k)) continue;
      byId.set(k, r);
      list.push(r);
    }
    const idOf = (r) => key(r[idf]);
    const parentOf = (r) => { const p = key(r[pf]); return p !== null && byId.has(p) ? p : null; };
    return { idf, byId, list, idOf, parentOf };
  }

  // the row, then its parent, up to the root (stops where a loop comes back)
  function up(ix, id) {
    let k = key(id);
    if (k === null || !ix.byId.has(k)) return [];
    const out = [];
    const seen = new Set();
    while (k !== null && !seen.has(k)) {
      seen.add(k);
      const r = ix.byId.get(k);
      out.push(r);
      k = ix.parentOf(r);
    }
    return out;
  }

  function build(rows, opts) {
    const ix = index(rows, opts);
    const nodes = new Map();
    const pos = new Map();
    ix.list.forEach((r, i) => { const k = ix.idOf(r); nodes.set(k, Object.assign({}, r, { children: [] })); pos.set(k, i); });
    const roots = [];
    const parentNode = new Map();
    for (const r of ix.list) {
      const k = ix.idOf(r);
      const p = ix.parentOf(r);
      if (p !== null && p !== k) { nodes.get(p).children.push(nodes.get(k)); parentNode.set(k, p); }
      else roots.push(nodes.get(k));
    }
    const seen = new Set();
    const mark = (n) => {
      const stack = [n];
      while (stack.length) {
        const x = stack.pop();
        const xk = key(x[ix.idf]);
        if (seen.has(xk)) continue;
        seen.add(xk);
        for (const c of x.children) stack.push(c);
      }
    };
    roots.forEach(mark);
    // rows not reached hang from a loop: break each loop at its earliest row
    for (const r of ix.list) {
      const k = ix.idOf(r);
      if (seen.has(k)) continue;
      const walked = new Set();
      let cur = k;
      while (!walked.has(cur)) { walked.add(cur); cur = parentNode.get(cur); }
      let m = cur;
      let best = cur;
      do { if (pos.get(m) < pos.get(best)) best = m; m = parentNode.get(m); } while (m !== cur);
      const n = nodes.get(best);
      const siblings = nodes.get(parentNode.get(best)).children;
      siblings.splice(siblings.indexOf(n), 1);
      parentNode.delete(best);
      roots.push(n);
      mark(n);
    }
    return roots;
  }

  function flatten(roots) {
    const out = [];
    const seen = new Set();
    const list = Array.isArray(roots) ? roots : [];
    const stack = [];
    for (let i = list.length - 1; i >= 0; i--) stack.push([list[i], 0]);
    while (stack.length) {
      const [n, depth] = stack.pop();
      if (!n || typeof n !== 'object' || seen.has(n)) continue;
      seen.add(n);
      const { children, ...rest } = n;
      out.push(Object.assign(rest, { depth }));
      const kids = Array.isArray(children) ? children : [];
      for (let i = kids.length - 1; i >= 0; i--) stack.push([kids[i], depth + 1]);
    }
    return out;
  }

  function path(rows, id, opts) { return up(index(rows, opts), id).reverse(); }
  function ancestors(rows, id, opts) { return up(index(rows, opts), id).slice(1); }

  function descendants(rows, id, opts) {
    const ix = index(rows, opts);
    const start = key(id);
    if (start === null || !ix.byId.has(start)) return [];
    const kids = new Map();
    for (const r of ix.list) {
      const k = ix.idOf(r);
      const p = ix.parentOf(r);
      if (p === null || p === k) continue;
      if (!kids.has(p)) kids.set(p, []);
      kids.get(p).push(r);
    }
    const out = [];
    const seen = new Set([start]);
    const stack = (kids.get(start) || []).slice().reverse();
    while (stack.length) {
      const r = stack.pop();
      const k = ix.idOf(r);
      if (seen.has(k)) continue;
      seen.add(k);
      out.push(r[ix.idf]);
      const c = kids.get(k) || [];
      for (let i = c.length - 1; i >= 0; i--) stack.push(c[i]);
    }
    return out;
  }

  function moveCheck(rows, id, newParent, opts) {
    const ix = index(rows, opts);
    const k = key(id);
    if (k === null || !ix.byId.has(k)) return false;
    const p = key(newParent);
    if (p === null) return true;
    if (!ix.byId.has(p)) return false;
    const seen = new Set();
    let cur = p;
    while (cur !== null && !seen.has(cur)) {
      if (cur === k) return false;
      seen.add(cur);
      cur = ix.parentOf(ix.byId.get(cur));
    }
    return true;
  }

  function sortTree(roots, compare) {
    const cmp = typeof compare === 'function' ? compare : null;
    const seen = new Set();
    const level = (list) => {
      const arr = [];
      for (const n of Array.isArray(list) ? list : []) {
        if (!n || typeof n !== 'object' || seen.has(n)) continue;
        seen.add(n);
        arr.push(Object.assign({}, n));
      }
      if (!cmp) return arr;
      return arr.map((n, i) => [n, i]).sort((a, b) => cmp(a[0], b[0]) || a[1] - b[1]).map((x) => x[0]);
    };
    const top = level(roots);
    const stack = top.slice();
    while (stack.length) {
      const n = stack.pop();
      if (!Array.isArray(n.children)) continue;
      n.children = level(n.children);
      for (const c of n.children) stack.push(c);
    }
    return top;
  }

  return { build, flatten, path, ancestors, descendants, moveCheck, sortTree };
}
