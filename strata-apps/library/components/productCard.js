// title: Product card
// description: One product to buy or browse (a shop item, a menu dish, a listing): its picture, name, price with an optional crossed-out old price, a badge, a star rating with its count, a short description and an Add button, opening on a tap.
// props: name: text; price: number (null hides it); currency (default 'USD'); image: an image URL or an app file id (optional);
//   description: text, cut to three lines (optional); badge: short text over the picture such as 'New' (default 'Sold out' when soldOut);
//   rating: number 0..5 (optional, shown when above 0 or with a count); ratingCount: how many ratings, shown as "(124)" (optional);
//   compareAt: the old price, shown crossed out only when above price (optional); soldOut: true to disable Add and dim the card (default false);
//   onAdd() to show an Add button (it may return a promise; the button waits for it); onClick() when the card is tapped (the Add button does not trigger it)
function component(props) {
  css('sa-product', `
.sa-product{display:flex;flex-direction:column;background:var(--card);border:1px solid var(--line);border-radius:var(--radius);overflow:hidden;min-width:0;height:100%}
.sa-product.click{cursor:pointer}
.sa-product.click:hover{border-color:var(--accent)}
.sa-product .sa-product-media{position:relative;background:var(--bg)}
.sa-product.out .sa-product-media{opacity:.6}
.sa-product .sa-product-badge{position:absolute;top:8px;left:8px;z-index:1}
.sa-product .sa-product-body{display:flex;flex-direction:column;gap:6px;padding:12px;flex:1;min-width:0}
.sa-product .sa-product-name{margin:0;font-size:1rem;line-height:1.3;overflow-wrap:anywhere}
.sa-product .sa-product-name button{all:unset;cursor:pointer;font:inherit;color:inherit}
.sa-product .sa-product-name button:focus-visible{outline:2px solid var(--accent);outline-offset:2px;border-radius:4px}
.sa-product .sa-product-desc{margin:0;color:var(--muted);font-size:.9em;display:-webkit-box;-webkit-line-clamp:3;-webkit-box-orient:vertical;overflow:hidden}
.sa-product .sa-product-rating{display:flex;align-items:center;gap:4px;font-size:.85em;color:var(--muted)}
.sa-product .sa-product-foot{display:flex;align-items:center;justify-content:space-between;gap:8px;flex-wrap:wrap;margin-top:auto;padding-top:4px}
.sa-product .sa-product-price{display:flex;align-items:baseline;gap:6px;flex-wrap:wrap}
.sa-product .sa-product-now{font-weight:600;font-size:1.05rem}
.sa-product .sa-product-now.sale{color:var(--bad)}
.sa-product .sa-product-was{color:var(--muted);text-decoration:line-through;font-size:.9em}
`);
  const num = (v) => (v === null || v === undefined || v === '' || !isFinite(Number(v)) ? null : Number(v));
  const currency = props.currency || 'USD';
  const money = (n) => app.fmt.money(n, currency);
  const name = props.name == null ? '' : String(props.name);
  const price = num(props.price);
  const was = num(props.compareAt);
  const showWas = price !== null && was !== null && was > price;
  const soldOut = !!props.soldOut;
  const badgeText = props.badge ? String(props.badge) : soldOut ? 'Sold out' : '';
  const rating = Math.max(0, Math.min(5, num(props.rating) || 0));
  const count = num(props.ratingCount);
  const clickable = typeof props.onClick === 'function';

  const badge = badgeText
    ? h('span', { class: `badge sa-product-badge ${soldOut && !props.badge ? 'bad' : 'info'}` }, badgeText)
    : null;

  const media = props.image
    ? h('div', { class: 'sa-product-media' }, badge,
        ui.image({ ...(/^(https?:|data:|\/)/.test(String(props.image)) ? { src: props.image } : { file: props.image }), alt: name, ratio: 4 / 3, rounded: false }))
    : null;

  const ratingRow = rating > 0 || (count !== null && count > 0)
    ? h('div', { class: 'sa-product-rating' },
        ui.ratingStars({ value: rating, size: 14, label: 'Rated' }),
        h('span', null, rating > 0 ? (Math.round(rating * 10) / 10).toString() : ''),
        count !== null ? h('span', { 'aria-label': `${count} ratings` }, `(${app.fmt.number(count)})`) : null)
    : null;

  let add = null;
  if (typeof props.onAdd === 'function') {
    add = h('button', {
      type: 'button', class: 'primary small', disabled: soldOut,
      'aria-label': soldOut ? `${name} is sold out` : `Add ${name}`,
      onclick: async (e) => {
        e.stopPropagation();
        if (soldOut || add.disabled) return;
        add.disabled = true;
        try { await props.onAdd(); } finally { add.disabled = soldOut; }
      },
    }, soldOut ? 'Sold out' : 'Add');
  }

  const priceRow = price !== null
    ? h('div', { class: 'sa-product-price' },
        h('span', { class: `sa-product-now${showWas ? ' sale' : ''}` }, money(price)),
        showWas ? h('span', { class: 'sa-product-was', 'aria-label': `was ${money(was)}` }, money(was)) : null)
    : h('span');

  const title = h('h3', { class: 'sa-product-name' },
    clickable ? h('button', { type: 'button', onclick: (e) => { e.stopPropagation(); props.onClick(); } }, name) : name);

  return h('article', {
    class: `sa-product${clickable ? ' click' : ''}${soldOut ? ' out' : ''}`,
    'aria-label': name || 'Product',
    onclick: clickable ? () => props.onClick() : null,
  },
    media,
    h('div', { class: 'sa-product-body' },
      !media && badge ? h('div', null, h('span', { class: badge.className.replace(' sa-product-badge', '') }, badgeText)) : null,
      title,
      ratingRow,
      props.description ? h('p', { class: 'sa-product-desc' }, String(props.description)) : null,
      (price !== null || add) ? h('div', { class: 'sa-product-foot' }, priceRow, add) : null));
}
