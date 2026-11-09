// title: Paging
// description: Splits lists into pages and lays out page links (result lists, tables, search hits, long feeds), with the SQL offset for a page and a page number kept in range.
// api: page(items, page = 1, size = 20) -> {items (that page's slice), page (the page shown, kept between 1 and pages), pages (at least 1), total,
//   from, to (1-based positions of the first and last item shown, 0 and 0 when there are none)};
//   pages(current, total, around = 1) -> the page links to show: the first, the last, and around pages on each side of current, with '…' for the gaps,
//   e.g. pages(5, 20) -> [1, '…', 4, 5, 6, '…', 20]; a gap of a single page shows that page instead of '…'; [] when total is 0 or less;
//   offset(page, size = 20) -> (page - 1) * size, the SQL OFFSET (0 for page 1 or less);
//   clamp(page, pages) -> page as a whole number between 1 and pages (1 when page is missing or pages is 0 or less);
//   page numbers start at 1; fractional numbers are rounded down, missing or odd ones read as 1
function helper() {
  const whole = (x, def) => {
    const n = Number(x);
    return x === null || x === '' || !Number.isFinite(n) ? def : Math.floor(n);
  };
  const sizeOf = (size) => Math.max(1, whole(size, 20));

  function clamp(page, pages) {
    const last = Math.max(1, whole(pages, 1));
    return Math.min(last, Math.max(1, whole(page, 1)));
  }

  function offset(page, size = 20) {
    return (Math.max(1, whole(page, 1)) - 1) * sizeOf(size);
  }

  function page(items, page = 1, size = 20) {
    const list = Array.isArray(items) ? items : [];
    const per = sizeOf(size);
    const total = list.length;
    const count = Math.max(1, Math.ceil(total / per));
    const p = clamp(page, count);
    const start = (p - 1) * per;
    const slice = list.slice(start, start + per);
    return {
      items: slice,
      page: p,
      pages: count,
      total,
      from: slice.length ? start + 1 : 0,
      to: slice.length ? start + slice.length : 0,
    };
  }

  function pages(current, total, around = 1) {
    const last = whole(total, 0);
    if (last <= 0) return [];
    const cur = clamp(current, last);
    const span = Math.max(0, whole(around, 1));
    const shown = [1];
    for (let n = Math.max(2, cur - span); n <= Math.min(last - 1, cur + span); n++) shown.push(n);
    if (last > 1) shown.push(last);
    const out = [];
    let prev = 0;
    for (const n of shown) {
      if (prev && n - prev === 2) out.push(prev + 1);
      else if (prev && n - prev > 2) out.push('…');
      out.push(n);
      prev = n;
    }
    return out;
  }

  return { page, pages, offset, clamp };
}
