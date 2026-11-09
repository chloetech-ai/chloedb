// title: Review list
// description: Reviews of a product, place or service with a summary of the average and a bar per star, sortable by newest, most helpful, highest or lowest, each showing who wrote it, when, its stars and text, with an optional "Helpful" button.
// props: reviews: [{id, author (a name), rating (1-5), text, at (ISO time or 'YYYY-MM-DD'), helpful? (number of helpful votes)}];
//   summary: true to show the average, the count and a bar per star above the list (default true);
//   sort: 'new' (default) | 'top' (most helpful first) | 'high' (highest rating first) | 'low' (lowest rating first);
//   onSort(sort) when the person picks another order (the list re-sorts itself either way);
//   onHelpful(review) to show a "Helpful" button per review (each can be pressed once while the list is shown);
//   title: a heading above it (optional); empty: text when there are no reviews (default 'No reviews yet')
function component(props) {
  css('sa-reviews', `
.sa-reviews{display:flex;flex-direction:column;gap:12px;min-width:0}
.sa-reviews .sa-sum{display:flex;gap:16px;align-items:center;flex-wrap:wrap;padding:12px;border:1px solid var(--line);border-radius:var(--radius);background:var(--card)}
.sa-reviews .sa-avg{display:flex;flex-direction:column;align-items:center;gap:4px;min-width:96px}
.sa-reviews .sa-avg b{font-size:2.2em;line-height:1}
.sa-reviews .sa-bars{flex:1;min-width:180px;display:flex;flex-direction:column;gap:4px}
.sa-reviews .sa-bar{display:grid;grid-template-columns:2.2em 1fr 2.6em;gap:8px;align-items:center;font-size:.85em;color:var(--muted)}
.sa-reviews .sa-track{height:8px;border-radius:4px;background:var(--line);overflow:hidden}
.sa-reviews .sa-fill{height:100%;background:var(--warn);border-radius:4px}
.sa-reviews .sa-bar span:last-child{text-align:right}
.sa-reviews .sa-tools{display:flex;gap:8px;align-items:center;flex-wrap:wrap}
.sa-reviews .sa-tools select{width:auto}
.sa-reviews ul{list-style:none;margin:0;padding:0;display:flex;flex-direction:column}
.sa-reviews li{padding:12px 0;border-top:1px solid var(--line);display:flex;flex-direction:column;gap:6px}
.sa-reviews li:first-child{border-top:0}
.sa-reviews .sa-head{display:flex;gap:8px;align-items:center;flex-wrap:wrap}
.sa-reviews .sa-who{font-weight:600;overflow-wrap:anywhere}
.sa-reviews .sa-when{color:var(--muted);font-size:.85em;margin-left:auto}
.sa-reviews .sa-text{margin:0;white-space:pre-wrap;overflow-wrap:anywhere}
.sa-reviews .sa-foot{display:flex;gap:8px;align-items:center;color:var(--muted);font-size:.85em}
`);
  const SORTS = [['new', 'Newest'], ['top', 'Most helpful'], ['high', 'Highest rated'], ['low', 'Lowest rated']];
  const valid = SORTS.map(s => s[0]);
  const showSummary = props.summary !== false;
  const onHelpful = typeof props.onHelpful === 'function' ? props.onHelpful : null;
  let sort = valid.includes(props.sort) ? props.sort : 'new';
  const voted = new Set();

  const time = (at) => {
    if (!at) return NaN;
    const t = Date.parse(/^\d{4}-\d{2}-\d{2}$/.test(at) ? at + 'T12:00:00' : at);
    return t;
  };
  const reviews = (Array.isArray(props.reviews) ? props.reviews : [])
    .filter(r => r && typeof r === 'object')
    .map((r, i) => {
      const n = Math.round(Number(r.rating));
      return {
        src: r,
        key: r.id != null ? String(r.id) : 'r' + i,
        author: (r.author && String(r.author).trim()) || 'Anonymous',
        rating: Number.isFinite(n) ? Math.max(1, Math.min(5, n)) : null,
        text: r.text == null ? '' : String(r.text).trim(),
        t: time(r.at),
        at: r.at,
        helpful: Math.max(0, Math.floor(Number(r.helpful) || 0)),
      };
    });

  const root = h('section', { class: 'sa-reviews', 'aria-label': props.title || 'Reviews' });
  if (props.title) root.append(h('h3', { style: 'margin:0' }, props.title));

  if (!reviews.length) {
    root.append(h('div', { class: 'empty' }, props.empty || 'No reviews yet'));
    return root;
  }

  if (showSummary) {
    const rated = reviews.filter(r => r.rating != null);
    const counts = [0, 0, 0, 0, 0, 0];
    rated.forEach(r => counts[r.rating]++);
    const avg = rated.length ? rated.reduce((s, r) => s + r.rating, 0) / rated.length : 0;
    const shown = Math.round(avg * 10) / 10;
    const most = Math.max(1, ...counts.slice(1));
    const bars = [];
    for (let s = 5; s >= 1; s--) {
      const pct = rated.length ? Math.round(counts[s] / rated.length * 100) : 0;
      bars.push(h('div', { class: 'sa-bar', role: 'img', 'aria-label': `${s} star${s === 1 ? '' : 's'}: ${counts[s]} review${counts[s] === 1 ? '' : 's'} (${pct}%)` },
        h('span', null, `${s} ★`),
        h('div', { class: 'sa-track' }, h('div', { class: 'sa-fill', style: `width:${counts[s] / most * 100}%` })),
        h('span', null, String(counts[s]))));
    }
    root.append(h('div', { class: 'sa-sum' },
      h('div', { class: 'sa-avg' },
        h('b', null, rated.length ? shown.toFixed(1) : '–'),
        ui.ratingStars({ value: Math.round(avg * 2) / 2, size: 18, label: 'Average rating' }),
        h('small', { class: 'muted' }, `${reviews.length} review${reviews.length === 1 ? '' : 's'}`)),
      h('div', { class: 'sa-bars' }, bars)));
  }

  const list = h('ul', { 'aria-live': 'polite' });
  const byTime = (a, b) => (Number.isNaN(b.t) ? -Infinity : b.t) - (Number.isNaN(a.t) ? -Infinity : a.t);
  const order = {
    new: byTime,
    top: (a, b) => b.helpful - a.helpful || byTime(a, b),
    high: (a, b) => (b.rating || 0) - (a.rating || 0) || byTime(a, b),
    low: (a, b) => (a.rating ?? 6) - (b.rating ?? 6) || byTime(a, b),
  };

  const item = (r) => {
    const foot = [];
    let count = r.helpful;
    const label = () => count ? `${count} found this helpful` : '';
    const countEl = h('span', null, label());
    if (onHelpful) {
      const done = voted.has(r.key);
      const btn = h('button', {
        type: 'button', class: 'small ghost', disabled: done, 'aria-pressed': done ? 'true' : 'false',
        onclick: () => {
          if (voted.has(r.key)) return;
          voted.add(r.key);
          btn.disabled = true;
          btn.setAttribute('aria-pressed', 'true');
          btn.textContent = 'Thanks!';
          count += 1; r.helpful = count;
          countEl.textContent = label();
          try { onHelpful(r.src); } catch (e) { console.error(e); }
        },
      }, done ? 'Thanks!' : '👍 Helpful');
      foot.push(btn);
    }
    foot.push(countEl);
    const valid = !Number.isNaN(r.t);
    return h('li', null,
      h('div', { class: 'sa-head' },
        ui.avatar({ name: r.author, size: 28 }),
        h('span', { class: 'sa-who' }, r.author),
        r.rating != null ? ui.ratingStars({ value: r.rating, size: 16, label: 'Rating' }) : null,
        valid ? h('time', { class: 'sa-when', datetime: String(r.at), title: app.fmt.datetime(new Date(r.t).toISOString()) }, app.fmt.date(new Date(r.t).toISOString())) : null),
      r.text ? h('p', { class: 'sa-text' }, r.text) : h('p', { class: 'sa-text muted' }, 'No written review'),
      (onHelpful || count) ? h('div', { class: 'sa-foot' }, foot) : null);
  };

  const draw = () => list.replaceChildren(reviews.slice().sort(order[sort]).map(item));

  if (reviews.length > 1) {
    const sel = h('select', {
      'aria-label': 'Sort reviews', value: sort,
      onchange: (e) => {
        sort = valid.includes(e.target.value) ? e.target.value : 'new';
        draw();
        if (typeof props.onSort === 'function') props.onSort(sort);
      },
    }, SORTS.map(([v, l]) => h('option', { value: v, selected: v === sort }, l)));
    root.append(h('div', { class: 'sa-tools' }, h('label', { class: 'small muted' }, 'Sort by'), sel));
  }
  draw();
  root.append(list);
  return root;
}
