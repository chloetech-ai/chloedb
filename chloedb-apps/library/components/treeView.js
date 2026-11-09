// title: Tree view
// description: Nested items that expand and collapse, with one selected and a search that keeps matches and their parents; for folders, outlines, categories, org charts and table-of-contents navigation.
// props: nodes: [{id, label (text), children?: [nodes], badge? (text or number, shown at the end), icon? (a short text or emoji before the label)}];
//   expanded: [ids] open at the start (default none); onToggle(id, open) when the person opens or closes a node (not called for the temporary opening while searching);
//   selected: the id shown as chosen (default none); onSelect(id) when a node is tapped or picked with Enter (without it, tapping a node opens or closes it);
//   search: text to filter by (default ''): nodes whose label holds every word are kept with their ancestors, all shown open, the words highlighted;
//   label: accessible name of the tree (default "Tree"); empty: text when there are no nodes (default "Nothing here")
function component(props) {
  css('sa-tree', `
.sa-tree{display:flex;flex-direction:column;gap:1px;min-width:0}
.sa-tree:focus{outline:none}
.sa-tree .sa-tree-row{display:flex;align-items:center;gap:6px;min-height:34px;padding:4px 8px 4px calc(var(--d) * 18px + 4px);border-radius:var(--radius);cursor:pointer;user-select:none;min-width:0}
.sa-tree .sa-tree-row:hover{background:color-mix(in srgb,var(--accent) 8%,transparent)}
.sa-tree .sa-tree-row.sel{background:color-mix(in srgb,var(--accent) 16%,transparent);font-weight:600}
.sa-tree .sa-tree-row:focus{outline:none}
.sa-tree .sa-tree-row:focus-visible{outline:2px solid var(--accent);outline-offset:-2px}
.sa-tree .sa-tree-tw{flex:none;width:18px;height:18px;display:inline-flex;align-items:center;justify-content:center;color:var(--muted);font-size:11px;transition:transform .15s;border-radius:4px}
.sa-tree .sa-tree-tw.open{transform:rotate(90deg)}
.sa-tree .sa-tree-tw.kids:hover{background:color-mix(in srgb,var(--fg) 10%,transparent)}
.sa-tree .sa-tree-icon{flex:none;line-height:1}
.sa-tree .sa-tree-label{flex:1;min-width:0;overflow:hidden;text-overflow:ellipsis;white-space:nowrap}
.sa-tree mark{background:color-mix(in srgb,var(--warn) 35%,transparent);color:inherit;border-radius:2px;padding:0 1px}
.sa-tree .badge{flex:none}
`);
  const nodes = Array.isArray(props.nodes) ? props.nodes : [];
  const keyOf = (id) => String(id);
  const open = new Set((Array.isArray(props.expanded) ? props.expanded : []).map(keyOf));
  const closedInSearch = new Set();
  const selectable = typeof props.onSelect === 'function';
  let selected = props.selected == null ? null : keyOf(props.selected);
  let focusKey = null;

  const query = String(props.search || '').trim();
  const words = lib.text.words(lib.text.normalize(query)).filter(Boolean);
  const searching = words.length > 0;
  const matches = (node) => {
    const label = lib.text.normalize(String(node.label ?? ''));
    return words.every((w) => label.includes(w));
  };

  // Prepare the tree once: filtered by the search, guarded against cycles.
  function prep(list, path) {
    const out = [];
    for (const node of list) {
      if (!node || node.id == null) continue;
      const key = keyOf(node.id);
      if (path.has(key)) continue;
      path.add(key);
      const kids = prep(Array.isArray(node.children) ? node.children : [], path);
      path.delete(key);
      if (searching && !matches(node) && !kids.length) continue;
      out.push({ node, key, kids });
    }
    return out;
  }
  const tree = prep(nodes, new Set());

  const isOpen = (item) => item.kids.length > 0 && (searching ? !closedInSearch.has(item.key) : open.has(item.key));

  function flatten() {
    const rows = [];
    (function walk(items, depth, parent) {
      items.forEach((item, i) => {
        rows.push({ item, depth, parent, pos: i + 1, size: items.length });
        if (isOpen(item)) walk(item.kids, depth + 1, item.key);
      });
    })(tree, 0, null);
    return rows;
  }

  const root = h('div', { class: 'sa-tree', role: 'tree', 'aria-label': props.label || 'Tree' });
  let rows = [];

  function toggle(item) {
    if (!item.kids.length) return;
    const now = !isOpen(item);
    if (searching) {
      now ? closedInSearch.delete(item.key) : closedInSearch.add(item.key);
    } else {
      now ? open.add(item.key) : open.delete(item.key);
      if (typeof props.onToggle === 'function') props.onToggle(item.node.id, now);
    }
    focusKey = item.key;
    draw(true);
  }

  function select(item) {
    focusKey = item.key;
    if (!selectable) return toggle(item);
    selected = item.key;
    props.onSelect(item.node.id);
    draw(true);
  }

  function labelPieces(text) {
    if (!searching) return text;
    return lib.text.highlight(text, query).map((p) => (p.hit ? h('mark', null, p.text) : p.text));
  }

  function draw(keepFocus) {
    const hadFocus = keepFocus || root.contains(document.activeElement);
    rows = flatten();
    if (!rows.length) {
      root.replaceChildren(h('div', { class: 'empty' },
        searching ? `No matches for "${query}"` : (props.empty || 'Nothing here')));
      return;
    }
    if (!rows.some((r) => r.item.key === focusKey)) {
      focusKey = rows.some((r) => r.item.key === selected) ? selected : rows[0].item.key;
    }
    let focusEl = null;
    root.replaceChildren(rows.map((r) => {
      const { item } = r;
      const hasKids = item.kids.length > 0;
      const expanded = isOpen(item);
      const label = String(item.node.label ?? '');
      const el = h('div', {
        class: `sa-tree-row${selectable && item.key === selected ? ' sel' : ''}`,
        role: 'treeitem',
        style: `--d:${r.depth}`,
        tabindex: item.key === focusKey ? '0' : '-1',
        'aria-level': String(r.depth + 1),
        'aria-setsize': String(r.size),
        'aria-posinset': String(r.pos),
        'aria-expanded': hasKids ? String(expanded) : null,
        'aria-selected': selectable ? String(item.key === selected) : null,
        title: label,
        onclick: () => select(item),
        onfocus: () => { focusKey = item.key; },
      },
        h('span', {
          class: `sa-tree-tw${hasKids ? ' kids' : ''}${expanded ? ' open' : ''}`,
          'aria-hidden': 'true',
          onclick: hasKids ? (e) => { e.stopPropagation(); toggle(item); } : null,
        }, hasKids ? '▶' : ''),
        item.node.icon != null && item.node.icon !== '' ? h('span', { class: 'sa-tree-icon', 'aria-hidden': 'true' }, String(item.node.icon)) : null,
        h('span', { class: 'sa-tree-label' }, labelPieces(label)),
        item.node.badge != null && item.node.badge !== '' ? h('span', { class: 'badge' }, String(item.node.badge)) : null,
      );
      if (item.key === focusKey) focusEl = el;
      return el;
    }));
    if (hadFocus && focusEl) focusEl.focus();
  }

  root.addEventListener('keydown', (e) => {
    if (!rows.length) return;
    const i = Math.max(0, rows.findIndex((r) => r.item.key === focusKey));
    const r = rows[i];
    const moveTo = (j) => {
      const target = rows[Math.max(0, Math.min(rows.length - 1, j))];
      focusKey = target.item.key;
      draw(true);
    };
    switch (e.key) {
      case 'ArrowDown': moveTo(i + 1); break;
      case 'ArrowUp': moveTo(i - 1); break;
      case 'Home': moveTo(0); break;
      case 'End': moveTo(rows.length - 1); break;
      case 'ArrowRight':
        if (!r.item.kids.length) return;
        if (!isOpen(r.item)) toggle(r.item); else moveTo(i + 1);
        break;
      case 'ArrowLeft':
        if (isOpen(r.item)) toggle(r.item);
        else if (r.parent != null) moveTo(rows.findIndex((x) => x.item.key === r.parent));
        else return;
        break;
      case 'Enter':
      case ' ':
        select(r.item);
        break;
      default: return;
    }
    e.preventDefault();
  });

  draw(false);
  return root;
}
