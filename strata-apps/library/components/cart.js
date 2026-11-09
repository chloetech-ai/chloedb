// title: Shopping cart
// description: A cart summary with each item's picture, options, price and quantity, buttons to change the quantity or remove it, and the subtotal, shipping, tax and total above a checkout button; for shops, order forms and menus.
// props: items: [{id, name, price (a unit price, number), qty (number, default 1), image? (an app file id or an image URL), options? (text, [texts] or {name: value}, such as {Size: 'M'})}];
//   onQty(id, qty) when a quantity changes (always a whole number of 1 or more; the stepper and box are shown only then, else the quantity is shown as "× 2");
//   onRemove(id) to show a remove button per item; onCheckout() to show the checkout button (disabled while the cart is empty);
//   currency: for every amount (default 'USD'); shipping: shipping cost (default 0; a row is shown when it is above 0, or reads "Free" when 0 is given);
//   taxRate: tax on the subtotal as a fraction, 0.08 for 8% (default 0, no tax row; a value above 1 is read as a percent);
//   note: a small line under the total (delivery times, a promotion); empty: text when there are no items (default 'Your cart is empty');
//   checkoutLabel: text of the checkout button (default 'Check out')
function component(props) {
  css('sa-cart', `
.sa-cart{display:flex;flex-direction:column;gap:12px;min-width:0}
.sa-cart .sa-lines{list-style:none;margin:0;padding:0;display:flex;flex-direction:column}
.sa-cart .sa-line{display:grid;grid-template-columns:auto 1fr auto;gap:4px 12px;align-items:start;padding:10px 0;border-bottom:1px solid var(--line)}
.sa-cart .sa-line:first-child{padding-top:0}
.sa-cart .sa-pic{grid-row:span 2;width:56px;height:56px;border-radius:var(--radius);overflow:hidden;background:var(--line)}
.sa-cart .sa-line.nopic{grid-template-columns:1fr auto}
.sa-cart .sa-info{min-width:0}
.sa-cart .sa-name{font-weight:600;overflow-wrap:anywhere}
.sa-cart .sa-opts,.sa-cart .sa-unit{color:var(--muted);font-size:.85em;overflow-wrap:anywhere}
.sa-cart .sa-amount{font-weight:600;white-space:nowrap;text-align:right;font-variant-numeric:tabular-nums}
.sa-cart .sa-controls{grid-column:-3/-1;display:flex;align-items:center;gap:8px;flex-wrap:wrap}
.sa-cart .sa-line.nopic .sa-controls{grid-column:1/-1}
.sa-cart .sa-qty{display:inline-flex;align-items:center;border:1px solid var(--line);border-radius:var(--radius);overflow:hidden}
.sa-cart .sa-qty button{border:0;border-radius:0;box-shadow:none;background:none;min-width:32px;height:32px;padding:0;margin:0;font-size:1.1em;line-height:1;color:var(--fg);cursor:pointer}
.sa-cart .sa-qty button:disabled{color:var(--muted);cursor:default;opacity:.5}
.sa-cart .sa-qty input{width:44px;height:32px;border:0;border-left:1px solid var(--line);border-right:1px solid var(--line);border-radius:0;text-align:center;padding:0;margin:0;background:none;color:var(--fg);-moz-appearance:textfield}
.sa-cart .sa-qty input::-webkit-outer-spin-button,.sa-cart .sa-qty input::-webkit-inner-spin-button{-webkit-appearance:none;margin:0}
.sa-cart .sa-times{color:var(--muted);font-size:.9em}
.sa-cart .sa-remove{margin-left:auto}
.sa-cart .sa-sums{display:grid;grid-template-columns:1fr auto;gap:4px 12px;margin:0;font-variant-numeric:tabular-nums}
.sa-cart .sa-sums dt{color:var(--muted);margin:0}
.sa-cart .sa-sums dd{margin:0;text-align:right;white-space:nowrap}
.sa-cart .sa-sums .sa-total{color:var(--fg);font-weight:700;font-size:1.15em;padding-top:6px;border-top:1px solid var(--line)}
.sa-cart .sa-note{color:var(--muted);font-size:.85em;margin:0}
.sa-cart .sa-checkout{width:100%}
`);
  const currency = props.currency || 'USD';
  const fmt = (n) => lib.money.format(n, currency);
  const num = (v, d = 0) => { const n = Number(v); return Number.isFinite(n) ? n : d; };
  const canQty = typeof props.onQty === 'function';
  const canRemove = typeof props.onRemove === 'function';

  const items = (Array.isArray(props.items) ? props.items : [])
    .filter((it) => it && typeof it === 'object')
    .map((it) => {
      const price = num(it.price);
      const qty = Math.max(0, Math.floor(num(it.qty, 1)));
      return { ...it, price, qty, line: lib.money.round(price * qty) };
    });

  const root = h('section', { class: 'sa-cart', 'aria-label': 'Shopping cart' });

  if (!items.length) {
    root.append(h('div', { class: 'empty' }, props.empty || 'Your cart is empty'));
    if (typeof props.onCheckout === 'function') {
      root.append(h('button', { type: 'button', class: 'primary sa-checkout', disabled: true }, props.checkoutLabel || 'Check out'));
    }
    return root;
  }

  const optionsText = (o) => {
    if (o == null || o === '') return '';
    if (Array.isArray(o)) return o.filter((x) => x != null && x !== '').join(' · ');
    if (typeof o === 'object') return Object.entries(o).filter(([, v]) => v != null && v !== '').map(([k, v]) => `${k}: ${v}`).join(' · ');
    return String(o);
  };

  const picture = (img, name) => {
    const isUrl = /^(https?:|data:image\/|blob:)/i.test(String(img));
    return ui.image({ [isUrl ? 'src' : 'file']: img, alt: name || '', width: 56, height: 56, fit: 'cover', rounded: false });
  };

  const setQty = (it, q) => {
    const n = Math.floor(Number(q));
    if (!Number.isFinite(n) || n < 1 || n === it.qty) return false;
    props.onQty(it.id, n);
    return true;
  };

  const list = h('ul', { class: 'sa-lines' });
  for (const it of items) {
    const name = it.name == null || it.name === '' ? 'Item' : String(it.name);
    const opts = optionsText(it.options);
    const controls = [];
    if (canQty) {
      const input = h('input', {
        type: 'number', min: 1, step: 1, inputmode: 'numeric', value: String(it.qty),
        'aria-label': `Quantity of ${name}`,
        onchange: (e) => { if (!setQty(it, e.target.value)) e.target.value = String(it.qty); },
        onkeydown: (e) => { if (e.key === 'Enter') { e.preventDefault(); e.target.blur(); } },
      });
      controls.push(h('div', { class: 'sa-qty', role: 'group', 'aria-label': `Quantity of ${name}` },
        h('button', { type: 'button', 'aria-label': `One fewer ${name}`, disabled: it.qty <= 1, onclick: () => setQty(it, it.qty - 1) }, '−'),
        input,
        h('button', { type: 'button', 'aria-label': `One more ${name}`, onclick: () => setQty(it, Math.max(1, it.qty + 1)) }, '+')));
    } else {
      controls.push(h('span', { class: 'sa-times' }, `× ${it.qty}`));
    }
    if (canRemove) {
      controls.push(h('button', { type: 'button', class: 'ghost small sa-remove', 'aria-label': `Remove ${name}`, onclick: () => props.onRemove(it.id) }, 'Remove'));
    }
    const hasPic = it.image != null && it.image !== '';
    list.append(h('li', { class: `sa-line${hasPic ? '' : ' nopic'}` },
      hasPic ? h('div', { class: 'sa-pic' }, picture(it.image, name)) : null,
      h('div', { class: 'sa-info' },
        h('div', { class: 'sa-name' }, name),
        opts ? h('div', { class: 'sa-opts' }, opts) : null,
        it.qty !== 1 ? h('div', { class: 'sa-unit' }, `${fmt(it.price)} each`) : null),
      h('div', { class: 'sa-amount' }, fmt(it.line)),
      h('div', { class: 'sa-controls' }, controls)));
  }
  root.append(list);

  const count = items.reduce((n, it) => n + it.qty, 0);
  const subtotal = lib.money.sum(items.map((it) => it.line));
  const shipGiven = props.shipping != null && props.shipping !== '';
  const shipping = Math.max(0, num(props.shipping));
  let rate = Math.max(0, num(props.taxRate));
  if (rate > 1) rate = rate / 100;
  const tax = rate > 0 ? lib.money.applyPercent(subtotal, rate * 100) : 0;
  const total = lib.money.sum([subtotal, shipping, tax]);

  const sums = h('dl', { class: 'sa-sums' },
    h('dt', null, `Subtotal (${count} item${count === 1 ? '' : 's'})`), h('dd', null, fmt(subtotal)));
  if (shipping > 0 || shipGiven) {
    sums.append(h('dt', null, 'Shipping'), h('dd', null, shipping > 0 ? fmt(shipping) : 'Free'));
  }
  if (rate > 0) {
    const pct = Math.round(rate * 10000) / 100;
    sums.append(h('dt', null, `Tax (${pct}%)`), h('dd', null, fmt(tax)));
  }
  sums.append(h('dt', { class: 'sa-total' }, 'Total'), h('dd', { class: 'sa-total', 'aria-live': 'polite' }, fmt(total)));
  root.append(sums);

  if (props.note) root.append(h('p', { class: 'sa-note' }, String(props.note)));
  if (typeof props.onCheckout === 'function') {
    root.append(h('button', { type: 'button', class: 'primary sa-checkout', disabled: count === 0, onclick: () => props.onCheckout() }, props.checkoutLabel || 'Check out'));
  }
  return root;
}
