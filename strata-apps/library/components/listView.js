// title: List view
// description: A list of records (contacts, notes, recipes, orders) with a search box, filter chips and a sort choice, each row showing a title, a line under it, a side note, a badge, tags and a small picture, and opening on tap.
// props: items: [{id, title, subtitle?, meta? (short text at the end, such as a date or amount), badge? (text, or {text, tone? ('ok' | 'warn' | 'bad' | 'info'), color?}),
//   tags? ([text]), image? (an app file id or an image URL)}] in the order to show them;
//   onSelect(id) when a row is tapped (rows are buttons only then); selected: the id of the row to show as chosen;
//   searchable: show a search box over title, subtitle, meta, badge and tags (default true); placeholder: text of the search box (default "Search");
//   filters: [{label, test(item) -> bool}] shown as chips, one at a time, with an "All" chip and a count on each (default none);
//   sorts: [{label, compare(a, b)}] offered in a menu when there are two or more; the first applies at the start (default: the items' own order;
//   while searching, rows come best match first until the person picks a sort); empty: text when there are no items at all (default "Nothing yet")
function component(props) {
  css('sa-listview', `
.sa-listview{display:flex;flex-direction:column;gap:8px;min-width:0}
.sa-listview .sa-lv-bar{display:flex;gap:8px;flex-wrap:wrap;align-items:center}
.sa-listview .sa-lv-bar input{flex:1 1 160px;min-width:0}
.sa-listview .sa-lv-bar select{flex:0 1 auto;max-width:100%}
.sa-listview .sa-lv-count{font-size:.85em;color:var(--muted)}
.sa-listview ul{list-style:none;margin:0;padding:0;border:1px solid var(--line);border-radius:var(--radius);background:var(--card);overflow:hidden}
.sa-listview li + li{border-top:1px solid var(--line)}
.sa-listview .sa-lv-row{display:flex;gap:10px;align-items:center;width:100%;padding:10px 12px;margin:0;border:0;border-radius:0;background:none;box-shadow:none;color:inherit;font:inherit;text-align:left;min-width:0}
.sa-listview button.sa-lv-row{cursor:pointer}
.sa-listview button.sa-lv-row:hover{background:color-mix(in srgb,var(--accent) 6%,transparent)}
.sa-listview button.sa-lv-row:focus-visible{outline:2px solid var(--accent);outline-offset:-2px}
.sa-listview .sa-lv-row.on{background:color-mix(in srgb,var(--accent) 12%,transparent);box-shadow:inset 3px 0 0 var(--accent)}
.sa-listview .sa-lv-img{flex:0 0 40px;width:40px;height:40px;overflow:hidden;border-radius:8px}
.sa-listview .sa-lv-body{flex:1 1 auto;min-width:0;display:flex;flex-direction:column;gap:2px}
.sa-listview .sa-lv-title,.sa-listview .sa-lv-sub{overflow:hidden;text-overflow:ellipsis;white-space:nowrap}
.sa-listview .sa-lv-title{font-weight:600}
.sa-listview .sa-lv-sub{font-size:.88em;color:var(--muted)}
.sa-listview .sa-lv-tags{display:flex;gap:4px;flex-wrap:wrap;margin-top:2px}
.sa-listview .sa-lv-tag{font-size:.75em;padding:1px 7px;border-radius:999px;background:color-mix(in srgb,var(--muted) 14%,transparent);color:var(--muted);white-space:nowrap}
.sa-listview .sa-lv-end{flex:0 0 auto;display:flex;flex-direction:column;align-items:flex-end;gap:4px;max-width:40%}
.sa-listview .sa-lv-meta{font-size:.85em;color:var(--muted);white-space:nowrap;overflow:hidden;text-overflow:ellipsis;max-width:100%}
.sa-listview .empty button{margin-top:8px}
`);
  const items = (Array.isArray(props.items) ? props.items : []).filter((it) => it && typeof it === 'object');
  const filters = (Array.isArray(props.filters) ? props.filters : []).filter((f) => f && typeof f.test === 'function');
  const sorts = (Array.isArray(props.sorts) ? props.sorts : []).filter((s) => s && typeof s.compare === 'function');
  const searchable = props.searchable !== false;
  const onSelect = typeof props.onSelect === 'function' ? props.onSelect : null;

  let query = '';
  let filter = -1;
  let sort = sorts.length ? 0 : -1;
  let sortChosen = false;
  let selected = props.selected == null ? null : String(props.selected);

  const passes = (f, it) => { try { return !!f.test(it); } catch (e) { return false; } };
  const badgeText = (b) => (b == null || b === '' ? '' : typeof b === 'object' ? String(b.text ?? '') : String(b));
  const textOf = (it) => [it.title, it.subtitle, it.meta, badgeText(it.badge), ...(Array.isArray(it.tags) ? it.tags : [])]
    .filter((x) => x != null && x !== '').join(' ');
  const counts = filters.map((f) => items.filter((it) => passes(f, it)).length);

  function visible() {
    let rows = filter >= 0 && filters[filter] ? items.filter((it) => passes(filters[filter], it)) : items.slice();
    const q = query.trim();
    if (q) rows = lib.text.search(q, rows, textOf);
    if (sort >= 0 && (!q || sortChosen)) {
      const cmp = sorts[sort].compare;
      rows = rows.slice().sort((a, b) => { try { return Number(cmp(a, b)) || 0; } catch (e) { return 0; } });
    }
    return rows;
  }

  const root = h('div', { class: 'sa-listview' });
  const chipsBox = h('div', { class: 'sa-lv-chips' });
  const countLine = h('div', { class: 'sa-lv-count', 'aria-live': 'polite' });
  const listBox = h('div');

  const search = searchable ? h('input', {
    type: 'search', placeholder: props.placeholder || 'Search', 'aria-label': props.placeholder || 'Search',
    value: '', oninput: (e) => { query = e.target.value; drawList(); },
    onkeydown: (e) => { if (e.key === 'Escape' && query) { e.preventDefault(); e.target.value = ''; query = ''; drawList(); } },
  }) : null;

  const sortSelect = sorts.length > 1 ? h('select', {
    'aria-label': 'Sort by',
    onchange: (e) => { sort = Number(e.target.value) || 0; sortChosen = true; drawList(); },
  }, sorts.map((s, i) => h('option', { value: String(i), selected: i === sort }, s.label || `Sort ${i + 1}`))) : null;

  if (search || sortSelect) root.append(h('div', { class: 'sa-lv-bar' }, search, sortSelect));
  if (filters.length) root.append(chipsBox);
  root.append(countLine, listBox);

  function drawChips() {
    if (!filters.length) return;
    chipsBox.replaceChildren(ui.filterChips({
      options: filters.map((f, i) => ({ value: String(i), label: f.label || `Filter ${i + 1}`, count: counts[i] })),
      value: filter >= 0 ? String(filter) : null,
      all: 'All',
      onChange: (v) => {
        const n = v == null || v === '' || Array.isArray(v) ? NaN : Number(v);
        filter = Number.isInteger(n) && n >= 0 && n < filters.length ? n : -1;
        drawChips(); drawList();
      },
    }));
  }

  function badgeEl(b) {
    const text = badgeText(b);
    if (!text) return null;
    const tone = typeof b === 'object' && ['ok', 'warn', 'bad', 'info'].includes(b.tone) ? ` ${b.tone}` : '';
    const color = typeof b === 'object' && b.color ? `--c:${b.color}` : null;
    return h('span', { class: `badge${tone}`, style: color }, text);
  }

  function imageEl(img) {
    if (img == null || img === '') return null;
    const s = String(img);
    const isUrl = /^(https?:|data:image\/|blob:|\/)/i.test(s);
    return h('div', { class: 'sa-lv-img', 'aria-hidden': 'true' },
      ui.image(Object.assign(isUrl ? { src: s } : { file: s }, { alt: '', width: 40, height: 40, fit: 'cover', rounded: true })));
  }

  function rowEl(it) {
    const id = it.id == null ? null : String(it.id);
    const on = selected != null && id === selected;
    const tags = Array.isArray(it.tags) ? it.tags.filter((t) => t != null && t !== '') : [];
    const shown = tags.slice(0, 3);
    const hasEnd = (it.meta != null && it.meta !== '') || badgeText(it.badge);
    const inner = [
      imageEl(it.image),
      h('div', { class: 'sa-lv-body' },
        h('div', { class: 'sa-lv-title' }, it.title == null || it.title === '' ? 'Untitled' : String(it.title)),
        it.subtitle != null && it.subtitle !== '' ? h('div', { class: 'sa-lv-sub' }, String(it.subtitle)) : null,
        shown.length ? h('div', { class: 'sa-lv-tags' },
          shown.map((t) => h('span', { class: 'sa-lv-tag' }, String(t))),
          tags.length > 3 ? h('span', { class: 'sa-lv-tag', title: tags.slice(3).join(', ') }, `+${tags.length - 3}`) : null) : null),
      hasEnd ? h('div', { class: 'sa-lv-end' },
        it.meta != null && it.meta !== '' ? h('span', { class: 'sa-lv-meta' }, String(it.meta)) : null,
        badgeEl(it.badge)) : null,
    ];
    const cls = `sa-lv-row${on ? ' on' : ''}`;
    return h('li', null, onSelect && id != null
      ? h('button', { type: 'button', class: cls, 'aria-current': on ? 'true' : null,
          onclick: () => { selected = id; drawList(); onSelect(it.id); } }, inner)
      : h('div', { class: cls }, inner));
  }

  function reset() {
    query = ''; filter = -1;
    if (search) search.value = '';
    drawChips(); drawList();
    if (search) search.focus();
  }

  function drawList() {
    const narrowed = query.trim() !== '' || filter >= 0;
    if (!items.length) {
      countLine.replaceChildren();
      listBox.replaceChildren(h('div', { class: 'empty' }, props.empty || 'Nothing yet'));
      return;
    }
    const rows = visible();
    countLine.replaceChildren(narrowed ? `${rows.length} of ${items.length}` : '');
    countLine.style.display = narrowed ? '' : 'none';
    listBox.replaceChildren(rows.length
      ? h('ul', { 'aria-label': 'Results' }, rows.map(rowEl))
      : h('div', { class: 'empty' },
          h('div', null, query.trim() ? `No matches for “${query.trim()}”` : 'Nothing matches this filter'),
          h('button', { type: 'button', class: 'small', onclick: reset }, 'Clear search and filters')));
  }

  drawChips();
  drawList();
  return root;
}
