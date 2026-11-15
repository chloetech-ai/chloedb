// title: Price list
// description: Prices that read at a glance: each name with its price on the same line, joined by a dotted leader, a short note under it and an optional badge, grouped under headings; or, with columns, a rate table with labelled columns that becomes one card per row on a phone. For services and their prices, rates per hour or page, plans and fees.
// props: items: [{name, price (a number in the app's currency, or text such as 'from 40' or 'on request'), note? (a short line under the name), badge? (a short word: New, Popular), group? (a heading the item goes under), id?}] in their order;
//   columns: [{key, label, format? ('money' | 'number' | 'text')}] for a rate table instead of a list: each item's values by key, the first column its name (default none: a list);
//   unit: words after every price ('per hour', 'per page'; optional); currency: overrides the app's for numbers; onSelect(item) when a row is tapped (rows are buttons only then);
//   empty: text when there are no items (default 'No prices yet')
function component(props) {
  css('sa-prices', `
.sa-prices{display:flex;flex-direction:column;gap:18px;margin:0}
.sa-prices h4{margin:0 0 4px;font-size:.78em;letter-spacing:.06em;text-transform:uppercase;color:var(--muted);font-weight:600}
.sa-prices ul{list-style:none;margin:0;padding:0;display:flex;flex-direction:column}
.sa-prices li{margin:0;padding:0;border-bottom:1px solid var(--line)}
.sa-prices li:last-child{border-bottom:0}
.sa-prices .it{display:flex;flex-direction:column;gap:2px;width:100%;padding:12px 2px;margin:0;border:0;background:none;color:inherit;font:inherit;text-align:left;box-shadow:none;min-height:44px;box-sizing:border-box}
.sa-prices button.it{cursor:pointer;border-radius:var(--radius)}
.sa-prices button.it:hover{background:color-mix(in srgb,var(--fg) 5%,transparent)}
.sa-prices button.it:focus-visible{outline:2px solid var(--accent);outline-offset:-2px}
.sa-prices .ln{display:flex;align-items:baseline;gap:8px;min-width:0}
.sa-prices .nm{font-weight:600;min-width:0;overflow-wrap:anywhere}
.sa-prices .dots{flex:1 1 24px;min-width:16px;border-bottom:1px dotted color-mix(in srgb,var(--fg) 35%,transparent);transform:translateY(-4px)}
.sa-prices .pr{font-weight:600;font-variant-numeric:tabular-nums;white-space:nowrap}
.sa-prices .un{font-weight:400;color:var(--muted);font-size:.86em;margin-left:4px}
.sa-prices .nt{color:var(--muted);font-size:.9em;line-height:1.4}
.sa-prices .bd{display:inline-block;margin-left:8px;padding:1px 8px;border-radius:999px;font-size:.72em;font-weight:600;background:color-mix(in srgb,var(--accent) 16%,transparent);color:var(--fg);vertical-align:2px}
.sa-prices table{width:100%;border-collapse:collapse;font-variant-numeric:tabular-nums}
.sa-prices th{text-align:left;font-size:.78em;letter-spacing:.04em;text-transform:uppercase;color:var(--muted);font-weight:600;padding:8px 10px;border-bottom:1px solid var(--line)}
.sa-prices td{padding:12px 10px;border-bottom:1px solid var(--line);vertical-align:top}
.sa-prices th.r,.sa-prices td.r{text-align:right;white-space:nowrap}
.sa-prices td.k{font-weight:600}
.sa-prices tr.sel{cursor:pointer}
.sa-prices tr.sel:hover td{background:color-mix(in srgb,var(--fg) 5%,transparent)}
@media (max-width:560px){
.sa-prices table,.sa-prices tbody,.sa-prices tr,.sa-prices td{display:block;width:100%;box-sizing:border-box}
.sa-prices thead{display:none}
.sa-prices tr{padding:10px 0;border-bottom:1px solid var(--line)}
.sa-prices td{border:0;padding:3px 2px;display:flex;justify-content:space-between;gap:12px;text-align:left}
.sa-prices td.r{text-align:left}
.sa-prices td[data-l]::before{content:attr(data-l);color:var(--muted);font-size:.86em}
.sa-prices td.k{font-size:1.05em}
.sa-prices td.k::before{content:none}
}
`);
  const items = (Array.isArray(props.items) ? props.items : []).filter((it) => it && it.name != null && String(it.name).trim() !== '');
  const sel = typeof props.onSelect === 'function';
  const unit = props.unit ? String(props.unit) : '';
  const money = (v) => (props.currency ? app.fmt.money(v, props.currency) : app.fmt.money(v));
  const priceText = (v) => (typeof v === 'number' && Number.isFinite(v) ? money(v) : v == null || String(v).trim() === '' ? '—' : String(v));
  if (!items.length) return h('p', { class: 'muted' }, props.empty == null ? 'No prices yet' : String(props.empty));

  const cols = Array.isArray(props.columns) ? props.columns.filter((c) => c && c.key) : [];
  if (cols.length) {
    const cell = (c, v) => (c.format === 'money' ? priceText(v) : c.format === 'number' && typeof v === 'number' ? app.fmt.number(v) : v == null || v === '' ? '—' : String(v));
    const right = (c) => c.format === 'money' || c.format === 'number';
    return h('div', { class: 'sa-prices', role: 'region', 'aria-label': 'Prices' },
      h('table', null,
        h('thead', null, h('tr', null, cols.map((c) => h('th', { class: right(c) ? 'r' : null, scope: 'col' }, String(c.label || c.key))))),
        h('tbody', null, items.map((it) => h('tr', { class: sel ? 'sel' : null, tabindex: sel ? '0' : null, onclick: sel ? () => props.onSelect(it) : null, onkeydown: sel ? (e) => { if (e.key === 'Enter') props.onSelect(it); } : null },
          cols.map((c, i) => h('td', { class: [i === 0 ? 'k' : '', right(c) ? 'r' : ''].join(' ').trim() || null, 'data-l': i === 0 ? null : String(c.label || c.key) },
            i === 0 ? String(it[c.key] ?? it.name) : h('span', null, cell(c, it[c.key]), c.format === 'money' && unit ? h('span', { class: 'un' }, unit) : null))))))));
  }

  // A list grouped under headings, in the order the groups first come.
  const groups = [];
  for (const it of items) {
    const g = it.group == null ? '' : String(it.group);
    let grp = groups.find((x) => x.name === g);
    if (!grp) groups.push(grp = { name: g, items: [] });
    grp.items.push(it);
  }
  const row = (it) => {
    const inner = [
      h('span', { class: 'ln' },
        h('span', { class: 'nm' }, String(it.name), it.badge ? h('span', { class: 'bd' }, String(it.badge)) : null),
        h('span', { class: 'dots', 'aria-hidden': 'true' }),
        h('span', { class: 'pr' }, priceText(it.price), unit ? h('span', { class: 'un' }, unit) : null)),
      it.note ? h('span', { class: 'nt' }, String(it.note)) : null,
    ];
    return h('li', null, sel ? h('button', { type: 'button', class: 'it', onclick: () => props.onSelect(it) }, inner) : h('div', { class: 'it' }, inner));
  };
  return h('div', { class: 'sa-prices' }, groups.map((g) => h('section', null, g.name ? h('h4', null, g.name) : null, h('ul', null, g.items.map(row)))));
}
