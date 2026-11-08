// title: Data table
// description: A table of rows with sortable columns, a search box over the text, pages for long lists, formatted cells (money, dates, yes/no, tags) and rows that open on tap.
// props: rows: [objects]; columns: [{key, label?, format? ('text' | 'number' | 'money' | 'date' | 'datetime' | 'ago' | 'bool' | 'tags' | a function (value, row) returning text or an element),
//   align? ('left' | 'right'), sortable? (default true), width?}] (default: every key of the first row but id);
//   sort: {key, dir: 'asc' | 'desc'} to start with; search: true for a search box (default true over 8 rows); pageSize: rows per page (default 25);
//   onRowClick(row); empty: text when there are no rows (default "Nothing yet"); currency: for money (default 'USD')
function component(props) {
  css('sa-table', `
.sa-table{display:flex;flex-direction:column;gap:8px;width:100%}
.sa-table-wrap{overflow-x:auto;border:1px solid var(--line);border-radius:var(--radius);background:var(--card)}
.sa-table table{margin:0}
.sa-table th{white-space:nowrap;user-select:none}
.sa-table th.sort{cursor:pointer}
.sa-table th.sort:hover{color:var(--fg)}
.sa-table th .arr{font-size:10px;margin-left:3px}
.sa-table td.r,.sa-table th.r{text-align:right;font-variant-numeric:tabular-nums}
.sa-table tr.click{cursor:pointer}
.sa-table tbody tr:last-child td{border-bottom:0}
.sa-table-foot{display:flex;align-items:center;gap:8px;color:var(--muted);font-size:13px}
.sa-table-foot .grow{flex:1}
.sa-table mark{background:color-mix(in srgb,var(--warn) 30%,transparent);color:inherit;border-radius:2px}
`);
  const rows = (props.rows || []).filter(Boolean);
  const cols = (props.columns && props.columns.length ? props.columns : Object.keys(rows[0] || {}).filter((k) => k !== 'id').map((key) => ({ key })))
    .map((c) => (typeof c === 'string' ? { key: c } : c));
  const pageSize = Math.max(1, Number(props.pageSize) || 25);
  const state = { q: '', sort: props.sort && props.sort.key ? { key: props.sort.key, dir: props.sort.dir === 'desc' ? 'desc' : 'asc' } : null, page: 0 };
  const label = (c) => c.label || String(c.key).replace(/_/g, ' ').replace(/^./, (m) => m.toUpperCase());
  const numeric = (c) => c.align === 'right' || ['number', 'money'].includes(c.format);
  const cell = (c, row) => {
    const v = row[c.key];
    if (typeof c.format === 'function') return c.format(v, row);
    if (v === null || v === undefined || v === '') return '';
    switch (c.format) {
      case 'number': return Number(v).toLocaleString(undefined, { maximumFractionDigits: 2 });
      case 'money': return lib.money.format(Number(v), props.currency || 'USD');
      case 'date': return app.fmt.date(v);
      case 'datetime': return app.fmt.datetime(v);
      case 'ago': return app.fmt.ago(v);
      case 'bool': return v === true || v === 'true' || v === 1 ? '✓' : '';
      case 'tags': return h('span', { class: 'chips' }, (Array.isArray(v) ? v : String(v).split(',')).filter(Boolean).map((t) => h('span', { class: 'badge' }, String(t).trim())));
      default: return Array.isArray(v) ? v.join(', ') : typeof v === 'object' ? JSON.stringify(v) : String(v);
    }
  };
  const text = (row) => cols.map((c) => { const v = row[c.key]; return Array.isArray(v) ? v.join(' ') : v === null || v === undefined ? '' : String(v); }).join(' ');
  const showSearch = props.search === undefined ? rows.length > 8 : !!props.search;
  const q = h('input', { type: 'search', placeholder: 'Search…', 'aria-label': 'Search the table', style: 'max-width:280px' });
  const body = h('div', { class: 'sa-table-wrap' });
  const foot = h('div', { class: 'sa-table-foot' });
  q.addEventListener('input', () => { state.q = q.value; state.page = 0; draw(); });
  function sorted(list) {
    if (!state.sort) return list;
    const { key, dir } = state.sort;
    const sign = dir === 'desc' ? -1 : 1;
    return list.slice().sort((a, b) => {
      const x = a[key]; const y = b[key];
      if (x === y) return 0;
      if (x === null || x === undefined || x === '') return 1;
      if (y === null || y === undefined || y === '') return -1;
      const nx = Number(x); const ny = Number(y);
      if (typeof x !== 'boolean' && Number.isFinite(nx) && Number.isFinite(ny) && String(x).trim() !== '' && String(y).trim() !== '') return sign * (nx - ny);
      return sign * String(x).localeCompare(String(y), undefined, { numeric: true, sensitivity: 'base' });
    });
  }
  function draw() {
    const found = state.q.trim() ? lib.text.search(state.q, rows, text) : rows;
    const list = sorted(found);
    const pages = Math.max(1, Math.ceil(list.length / pageSize));
    state.page = Math.min(state.page, pages - 1);
    const shown = list.slice(state.page * pageSize, (state.page + 1) * pageSize);
    const head = h('tr', null, cols.map((c) => {
      const can = c.sortable !== false;
      const on = state.sort && state.sort.key === c.key;
      return h('th', { class: [numeric(c) ? 'r' : '', can ? 'sort' : ''].filter(Boolean).join(' ') || null, style: c.width ? `width:${c.width}` : null,
        'aria-sort': on ? (state.sort.dir === 'asc' ? 'ascending' : 'descending') : null,
        onclick: can ? () => { state.sort = on && state.sort.dir === 'asc' ? { key: c.key, dir: 'desc' } : on ? null : { key: c.key, dir: 'asc' }; draw(); } : null },
      label(c), on ? h('span', { class: 'arr' }, state.sort.dir === 'asc' ? '▲' : '▼') : null);
    }));
    body.replaceChildren(list.length
      ? h('table', null, h('thead', null, head), h('tbody', null, shown.map((row) => h('tr', { class: props.onRowClick ? 'click' : null, onclick: props.onRowClick ? () => props.onRowClick(row) : null },
        cols.map((c) => h('td', { class: numeric(c) ? 'r' : null }, cell(c, row)))))))
      : h('div', { class: 'empty', style: 'border:0' }, state.q ? 'Nothing matches.' : props.empty || 'Nothing yet'));
    foot.replaceChildren(h('span', { class: 'grow' }, `${list.length === rows.length ? rows.length : `${list.length} of ${rows.length}`} ${rows.length === 1 ? 'row' : 'rows'}`),
      pages > 1 ? [h('button', { class: 'small', type: 'button', disabled: state.page === 0, onclick: () => { state.page--; draw(); } }, '‹ Prev'),
        h('span', null, `${state.page + 1} / ${pages}`),
        h('button', { class: 'small', type: 'button', disabled: state.page >= pages - 1, onclick: () => { state.page++; draw(); } }, 'Next ›')] : null);
  }
  draw();
  return h('div', { class: 'sa-table' }, showSearch ? q : null, body, foot);
}
