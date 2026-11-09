// title: Grouped list
// description: Items under group headers with a count on each (tasks by status, contacts by letter, expenses by month, recipes by course), groups that fold open and shut, and rows that open on tap.
// props: items: [{id, title, subtitle?, meta? (text or an element shown at the end of the row), group (a group name; empty or missing goes under "Other", last)}] in their order;
//   groupOrder: [group names] to place first in this order (default: groups in the order first seen; groups with no items are not shown);
//   collapsible: true (default) to fold groups by tapping their header; collapsed: [group names] that start folded (default none);
//   onCollapse(names) when a group is folded or unfolded, with every folded group's name (keep it and pass it back as collapsed to survive a redraw);
//   onSelect(id) when a row is tapped or chosen with Enter (rows are clickable only then);
//   render(item) -> an element shown instead of the default row (title, subtitle, meta);
//   empty: text when there are no items (default "Nothing yet")
let saGlistSeq = 0;
function component(props) {
  css('sa-glist', `
.sa-glist{display:flex;flex-direction:column;gap:12px;min-width:0}
.sa-glist .sa-glist-group{border:1px solid var(--line);border-radius:var(--radius);background:var(--card);overflow:hidden}
.sa-glist .sa-glist-head{display:flex;align-items:center;gap:8px;width:100%;margin:0;padding:10px 12px;border:0;border-radius:0;box-shadow:none;background:none;color:var(--fg);font:inherit;font-weight:600;text-align:left}
.sa-glist button.sa-glist-head{cursor:pointer}
.sa-glist button.sa-glist-head:hover{background:color-mix(in srgb,var(--line) 35%,transparent)}
.sa-glist .sa-glist-chev{display:inline-block;width:1em;color:var(--muted);transition:transform .15s}
.sa-glist .closed .sa-glist-chev{transform:rotate(-90deg)}
.sa-glist .sa-glist-name{flex:1;min-width:0;overflow:hidden;text-overflow:ellipsis;white-space:nowrap}
.sa-glist .sa-glist-count{flex:none;font-size:.8em;font-weight:500;color:var(--muted);background:color-mix(in srgb,var(--line) 60%,transparent);border-radius:999px;padding:1px 8px}
.sa-glist .sa-glist-body{list-style:none;margin:0;padding:0;border-top:1px solid var(--line)}
.sa-glist .sa-glist-body[hidden]{display:none}
.sa-glist .sa-glist-row{display:flex;align-items:center;gap:10px;padding:9px 12px;min-width:0}
.sa-glist .sa-glist-body>li+li{border-top:1px solid var(--line)}
.sa-glist .sa-glist-row.pick{cursor:pointer}
.sa-glist .sa-glist-row.pick:hover{background:color-mix(in srgb,var(--line) 35%,transparent)}
.sa-glist .sa-glist-row.pick:focus-visible{outline:2px solid var(--accent);outline-offset:-2px}
.sa-glist .sa-glist-text{flex:1;min-width:0}
.sa-glist .sa-glist-title,.sa-glist .sa-glist-sub{overflow:hidden;text-overflow:ellipsis;white-space:nowrap}
.sa-glist .sa-glist-sub{font-size:.85em;color:var(--muted)}
.sa-glist .sa-glist-meta{flex:none;max-width:40%;font-size:.85em;color:var(--muted);text-align:right;overflow:hidden;text-overflow:ellipsis;white-space:nowrap}
`);
  const items = (Array.isArray(props.items) ? props.items : []).filter((it) => it && typeof it === 'object');
  const collapsible = props.collapsible !== false;
  const select = typeof props.onSelect === 'function' ? props.onSelect : null;
  const custom = typeof props.render === 'function' ? props.render : null;
  const root = h('div', { class: 'sa-glist' });

  if (!items.length) {
    root.append(h('div', { class: 'empty' }, props.empty || 'Nothing yet'));
    return root;
  }

  const groups = new Map();
  for (const it of items) {
    const key = it.group == null ? '' : String(it.group).trim();
    if (!groups.has(key)) groups.set(key, []);
    groups.get(key).push(it);
  }
  const order = [];
  for (const g of Array.isArray(props.groupOrder) ? props.groupOrder : []) {
    const key = g == null ? '' : String(g).trim();
    if (groups.has(key) && !order.includes(key)) order.push(key);
  }
  for (const key of groups.keys()) if (key !== '' && !order.includes(key)) order.push(key);
  if (groups.has('') && !order.includes('')) order.push('');

  const closed = new Set(collapsible && Array.isArray(props.collapsed) ? props.collapsed.map((g) => (g == null ? '' : String(g).trim())) : []);
  const uid = `sa-glist-${++saGlistSeq}`;

  const row = (it) => {
    const content = custom ? custom(it) : [
      h('div', { class: 'sa-glist-text' },
        h('div', { class: 'sa-glist-title' }, it.title == null || it.title === '' ? 'Untitled' : String(it.title)),
        it.subtitle != null && it.subtitle !== '' ? h('div', { class: 'sa-glist-sub' }, it.subtitle) : null),
      it.meta != null && it.meta !== '' ? h('div', { class: 'sa-glist-meta' }, it.meta) : null,
    ];
    if (!select) return h('li', null, h('div', { class: 'sa-glist-row' }, content));
    const el = h('div', {
      class: 'sa-glist-row pick', role: 'button', tabindex: '0',
      'aria-label': custom ? null : String(it.title || 'Untitled'),
      onclick: () => select(it.id),
      onkeydown: (e) => {
        if (e.target !== el) return;
        if (e.key === 'Enter' || e.key === ' ') { e.preventDefault(); select(it.id); }
      },
    }, content);
    return h('li', null, el);
  };

  order.forEach((key, i) => {
    const list = groups.get(key);
    const name = key || 'Other';
    const bodyId = `${uid}-${i}`;
    const isClosed = closed.has(key);
    const body = h('ul', { class: 'sa-glist-body', id: bodyId, 'aria-label': name }, list.map(row));
    body.hidden = isClosed;
    const parts = [
      collapsible ? h('span', { class: 'sa-glist-chev', 'aria-hidden': 'true' }, '▾') : null,
      h('span', { class: 'sa-glist-name' }, name),
      h('span', { class: 'sa-glist-count', 'aria-label': `${list.length} item${list.length === 1 ? '' : 's'}` }, list.length),
    ];
    const section = h('section', { class: `sa-glist-group${isClosed ? ' closed' : ''}` });
    let head;
    if (collapsible) {
      head = h('button', {
        type: 'button', class: 'sa-glist-head', 'aria-expanded': isClosed ? 'false' : 'true', 'aria-controls': bodyId,
        onclick: () => {
          const nowClosed = !closed.has(key);
          if (nowClosed) closed.add(key); else closed.delete(key);
          body.hidden = nowClosed;
          head.setAttribute('aria-expanded', nowClosed ? 'false' : 'true');
          section.classList.toggle('closed', nowClosed);
          if (typeof props.onCollapse === 'function') props.onCollapse([...closed]);
        },
      }, parts);
    } else {
      head = h('div', { class: 'sa-glist-head', role: 'heading', 'aria-level': '3' }, parts);
    }
    section.append(head, body);
    root.append(section);
  });
  return root;
}
