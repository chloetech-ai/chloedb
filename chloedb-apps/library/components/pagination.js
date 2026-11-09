// title: Pagination
// description: Page controls under a long list or table (search results, orders, entries): first, previous, next and last buttons and numbered pages, with ellipses where pages are skipped around the current one.
// props: page: number, the current page, 1-based (default 1; kept within 1..pages); onPage(n): the person chose page n (never the current one);
//   pages: number of pages; or total: number of items with size: items per page (default 25) to work the pages out (pages wins when both are given);
//   label: accessible name of the controls (default "Pagination"). With one page or none it renders an empty, hidden element; without onPage the buttons are disabled
function component(props) {
  css('sa-pagination', `
.sa-pagination{display:flex;flex-wrap:wrap;gap:4px;align-items:center;justify-content:center;margin:8px 0}
.sa-pagination[hidden]{display:none}
.sa-pagination button{min-width:32px;height:32px;padding:0 8px;margin:0;border-radius:var(--radius);font-variant-numeric:tabular-nums;line-height:1}
.sa-pagination button[aria-current="page"]{background:var(--accent);border-color:var(--accent);color:#fff;font-weight:600;cursor:default}
.sa-pagination button:disabled:not([aria-current]){opacity:.4;cursor:default}
.sa-pagination .gap{min-width:20px;text-align:center;color:var(--muted);user-select:none}
@media (max-width:420px){.sa-pagination .edge{display:none}.sa-pagination button{min-width:30px;padding:0 6px}}
`);
  const size = Math.max(1, Math.floor(Number(props.size)) || 25);
  let pages = Math.floor(Number(props.pages));
  if (!(pages >= 0)) pages = Math.ceil(Math.max(0, Number(props.total) || 0) / size);
  pages = Math.max(0, pages || 0);
  const page = Math.max(1, Math.min(pages || 1, Math.floor(Number(props.page)) || 1));
  const can = typeof props.onPage === 'function';
  const nav = h('nav', { class: 'sa-pagination', 'aria-label': props.label || 'Pagination' });
  if (pages <= 1) { nav.hidden = true; return nav; }

  const go = (n) => { if (can && n !== page && n >= 1 && n <= pages) props.onPage(n); };
  const btn = (text, n, label, cls) => h('button', {
    type: 'button', class: cls || null, 'aria-label': label,
    disabled: !can || n === page || n < 1 || n > pages,
    onclick: () => go(n)
  }, text);

  // Always seven slots once there are more than seven pages: 1 … p-1 p p+1 … last
  let list;
  if (pages <= 7) list = Array.from({ length: pages }, (_, i) => i + 1);
  else if (page <= 4) list = [1, 2, 3, 4, 5, null, pages];
  else if (page >= pages - 3) list = [1, null, pages - 4, pages - 3, pages - 2, pages - 1, pages];
  else list = [1, null, page - 1, page, page + 1, null, pages];

  nav.append(
    btn('«', 1, 'First page', 'edge ghost'),
    btn('‹', page - 1, 'Previous page', 'ghost'),
    list.map((n) => n === null
      ? h('span', { class: 'gap', 'aria-hidden': 'true' }, '…')
      : n === page
        ? h('button', { type: 'button', 'aria-current': 'page', 'aria-label': `Page ${n} of ${pages}`, disabled: true }, String(n))
        : btn(String(n), n, `Page ${n}`, 'ghost')),
    btn('›', page + 1, 'Next page', 'ghost'),
    btn('»', pages, 'Last page', 'edge ghost')
  );
  return nav;
}
