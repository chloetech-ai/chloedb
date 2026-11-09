// title: Line items
// description: Editable line items with a running subtotal, discount, tax and total, for invoices, quotes, orders and estimates; add, edit and remove lines, or show them read-only.
// props: items: [{id?, description, qty, price}] (other fields on an item are kept; a missing qty counts as 1);
//   onChange(items) with the whole new list after every edit, add or remove (each item {...its fields, description, qty, price}; new lines have no id);
//   currency: ISO code for amounts (default 'USD'); taxRate: percent applied after the discount (default 0, the tax line is hidden at 0);
//   discount: an amount taken off the subtotal, capped at the subtotal (default 0, hidden at 0);
//   readOnly: true to only show the lines and totals (default false; also read-only when onChange is not given)
function component(props) {
  css('sa-lines', `
.sa-lines{display:flex;flex-direction:column;gap:4px}
.sa-lines-row{display:grid;grid-template-columns:minmax(0,1fr) 5.5rem 7.5rem 7.5rem;gap:8px;align-items:center;padding:6px 0;border-bottom:1px solid var(--line)}
.sa-lines.edit .sa-lines-row{grid-template-columns:minmax(0,1fr) 5.5rem 7.5rem 7.5rem 2.25rem}
.sa-lines-head{font-size:.85em;color:var(--muted);padding-top:0}
.sa-lines .num{text-align:right}
.sa-lines .desc{overflow-wrap:anywhere;min-width:0}
.sa-lines input{width:100%;min-width:0;box-sizing:border-box;margin:0}
.sa-lines .num input{text-align:right}
.sa-lines .amt{font-variant-numeric:tabular-nums;white-space:nowrap}
.sa-lines .rm{justify-self:end}
.sa-lines .empty{margin:8px 0}
.sa-lines-add{align-self:flex-start;margin-top:4px}
.sa-lines-totals{display:grid;grid-template-columns:auto auto;justify-content:end;gap:4px 24px;margin:8px 0 0}
.sa-lines-totals dt{color:var(--muted);margin:0}
.sa-lines-totals dd{margin:0;text-align:right;font-variant-numeric:tabular-nums;white-space:nowrap}
.sa-lines-totals .total{font-weight:600;font-size:1.1em;color:var(--fg);border-top:1px solid var(--line);padding-top:6px}
@media (max-width:560px){
  .sa-lines-head{display:none}
  .sa-lines-row{grid-template-columns:repeat(3,minmax(0,1fr))}
  .sa-lines.edit .sa-lines-row{grid-template-columns:repeat(3,minmax(0,1fr)) 2.25rem}
  .sa-lines-row .desc{grid-column:1/-1}
  .sa-lines.edit .sa-lines-row .desc{grid-column:1/-2}
  .sa-lines.edit .sa-lines-row .rm{grid-column:-2/-1;grid-row:1}
  .sa-lines-row [data-label]::before{content:attr(data-label);display:block;font-size:.75em;color:var(--muted);margin-bottom:2px}
}
`);
  const M = lib.money;
  const cur = props.currency || 'USD';
  const edit = typeof props.onChange === 'function' && !props.readOnly;
  const fmt = (n) => M.format(n, cur);

  const amountOf = (v) => {
    if (typeof v === 'number') return isFinite(v) ? v : 0;
    if (v == null || v === '') return 0;
    const n = M.parse(String(v));
    return n == null ? 0 : n;
  };
  const qtyOf = (v) => {
    if (v === undefined || v === null) return 1;
    if (typeof v === 'number') return isFinite(v) ? v : 0;
    const s = String(v).trim().replace(',', '.');
    if (s === '') return 0;
    const n = parseFloat(s);
    return isFinite(n) ? n : 0;
  };
  const priceText = (p) => (p === 0 ? '' : Number.isInteger(Math.round(p * 1e6) / 1e4) ? p.toFixed(2) : String(p));

  let key = 0;
  const rows = (Array.isArray(props.items) ? props.items : []).map((it) => {
    const item = it && typeof it === 'object' ? it : {};
    return { key: ++key, item, description: item.description == null ? '' : String(item.description), qty: qtyOf(item.qty), price: amountOf(item.price) };
  });

  const lineTotal = (r) => M.round(r.qty * r.price);
  function totals() {
    const subtotal = M.sum(rows.map(lineTotal));
    const discount = M.round(Math.min(Math.max(0, amountOf(props.discount)), Math.max(0, subtotal)));
    const rate = Math.max(0, Number(props.taxRate) || 0);
    const taxable = M.round(subtotal - discount);
    const tax = rate > 0 ? M.applyPercent(taxable, rate) : 0;
    return { subtotal, discount, rate, tax, total: M.round(taxable + tax) };
  }
  function emit() {
    if (!edit) return;
    props.onChange(rows.map((r) => ({ ...r.item, description: r.description, qty: r.qty, price: r.price })));
  }

  const root = h('div', { class: `sa-lines${edit ? ' edit' : ''}`, role: edit ? 'group' : 'table', 'aria-label': 'Line items' });
  const head = h('div', { class: 'sa-lines-row sa-lines-head', role: edit ? null : 'row' },
    h('span', { role: edit ? null : 'columnheader' }, 'Description'),
    h('span', { class: 'num', role: edit ? null : 'columnheader' }, 'Qty'),
    h('span', { class: 'num', role: edit ? null : 'columnheader' }, 'Price'),
    h('span', { class: 'num', role: edit ? null : 'columnheader' }, 'Amount'),
    edit ? h('span', { 'aria-hidden': 'true' }) : null);
  const body = h('div', { class: 'sa-lines-body', role: edit ? null : 'rowgroup' });
  const totalsEl = h('dl', { class: 'sa-lines-totals' });

  const descInputs = () => Array.from(body.querySelectorAll('input.sa-desc'));
  function focusDesc(i) {
    const list = descInputs();
    const el = list[Math.min(i, list.length - 1)];
    if (el) el.focus(); else if (addBtn) addBtn.focus();
  }
  function onEnter(i) {
    return (e) => {
      if (e.key !== 'Enter' || e.isComposing) return;
      e.preventDefault();
      if (i === rows.length - 1) addLine(); else focusDesc(i + 1);
    };
  }

  function rowEl(r, i) {
    const amt = h('span', { class: 'num amt', 'data-label': 'Amount', role: edit ? null : 'cell' }, fmt(lineTotal(r)));
    if (!edit) {
      return h('div', { class: 'sa-lines-row', role: 'row' },
        h('span', { class: 'desc', role: 'cell' }, r.description || h('span', { class: 'muted' }, 'No description')),
        h('span', { class: 'num amt', 'data-label': 'Qty', role: 'cell' }, app.fmt.number(r.qty)),
        h('span', { class: 'num amt', 'data-label': 'Price', role: 'cell' }, fmt(r.price)),
        amt);
    }
    const changed = () => { amt.textContent = fmt(lineTotal(r)); renderTotals(); emit(); };
    const n = i + 1;
    return h('div', { class: 'sa-lines-row' },
      h('span', { class: 'desc' },
        h('input', { type: 'text', class: 'sa-desc', value: r.description, placeholder: 'Description', 'aria-label': `Description, line ${n}`,
          oninput: (e) => { r.description = e.target.value; emit(); }, onkeydown: onEnter(i) })),
      h('span', { class: 'num', 'data-label': 'Qty' },
        h('input', { type: 'number', step: 'any', inputmode: 'decimal', value: String(r.qty), placeholder: '0', 'aria-label': `Quantity, line ${n}`,
          oninput: (e) => { r.qty = qtyOf(e.target.value); changed(); }, onkeydown: onEnter(i) })),
      h('span', { class: 'num', 'data-label': 'Price' },
        h('input', { type: 'text', inputmode: 'decimal', value: priceText(r.price), placeholder: '0.00', 'aria-label': `Price, line ${n}`,
          oninput: (e) => { r.price = amountOf(e.target.value); changed(); },
          onblur: (e) => { if (e.target.value.trim() !== '') e.target.value = priceText(r.price); },
          onkeydown: onEnter(i) })),
      amt,
      h('button', { type: 'button', class: 'ghost icon small rm', title: 'Remove line', 'aria-label': `Remove line ${n}`,
        onclick: () => { rows.splice(i, 1); renderBody(); renderTotals(); emit(); focusDesc(i); } }, '×'));
  }

  function renderBody() {
    head.style.display = rows.length ? '' : 'none';
    body.replaceChildren(rows.length
      ? rows.map(rowEl)
      : h('p', { class: 'empty' }, edit ? 'No lines yet. Add the first one below.' : 'No items'));
  }

  function renderTotals() {
    const t = totals();
    const line = (label, value, cls) => [h('dt', { class: cls || null }, label), h('dd', { class: cls || null, 'aria-live': cls ? 'polite' : null }, value)];
    const rate = String(Number(t.rate.toFixed(3)));
    totalsEl.replaceChildren(...[
      line('Subtotal', fmt(t.subtotal)),
      t.discount > 0 ? line('Discount', '−' + fmt(t.discount)) : null,
      t.rate > 0 ? line(`Tax (${rate}%)`, fmt(t.tax)) : null,
      line('Total', fmt(t.total), 'total'),
    ].filter(Boolean).flat());
  }

  function addLine() {
    rows.push({ key: ++key, item: {}, description: '', qty: 1, price: 0 });
    renderBody(); renderTotals(); emit();
    focusDesc(rows.length - 1);
  }
  const addBtn = edit ? h('button', { type: 'button', class: 'small sa-lines-add', onclick: addLine }, '+ Add line') : null;

  renderBody();
  renderTotals();
  root.append(head, body, addBtn, totalsEl);
  return root;
}
