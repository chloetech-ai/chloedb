// title: Timeline
// description: Things that happened (or will), newest first, grouped by day with a line and a dot per entry; for activity feeds, journals, order history and logs.
// props: items: [{time (ISO time or 'YYYY-MM-DD'), title, body?, tone? ('ok' | 'warn' | 'bad' | 'info'), icon? (a short text or emoji in the dot), onClick?}];
//   order: 'newest' (default) or 'oldest'; showTime: false to hide the clock times; empty: text when there is nothing (default "Nothing yet")
function component(props) {
  css('sa-timeline', `
.sa-tl{display:flex;flex-direction:column;gap:14px}
.sa-tl-day{font-size:12px;font-weight:600;color:var(--muted);text-transform:uppercase;letter-spacing:.04em;margin-bottom:6px}
.sa-tl-list{position:relative;display:flex;flex-direction:column;gap:12px;padding-left:22px}
.sa-tl-list::before{content:"";position:absolute;left:7px;top:6px;bottom:6px;width:2px;background:var(--line);border-radius:1px}
.sa-tl-it{position:relative;display:flex;flex-direction:column;gap:2px;text-align:left;background:none;border:0;padding:0;box-shadow:none;font:inherit;color:inherit;white-space:normal}
button.sa-tl-it{cursor:pointer}
button.sa-tl-it:hover .t{text-decoration:underline}
.sa-tl-dot{position:absolute;left:-22px;top:2px;width:16px;height:16px;border-radius:50%;background:var(--card);border:2px solid var(--c,var(--accent));display:grid;place-items:center;font-size:9px;line-height:1}
.sa-tl-it .t{font-weight:550}
.sa-tl-it .m{font-size:12px;color:var(--muted)}
.sa-tl-it .b{font-size:13.5px;color:var(--fg);white-space:pre-wrap}
`);
  const D = lib.dates;
  const tones = { ok: 'var(--ok)', warn: 'var(--warn)', bad: 'var(--bad)', info: 'var(--info)' };
  const dayOf = (t) => (String(t).length > 10 ? D.fromTime(t) : String(t).slice(0, 10));
  const items = (props.items || []).filter((x) => x && x.time).slice()
    .sort((a, b) => (props.order === 'oldest' ? 1 : -1) * (Date.parse(String(a.time).length === 10 ? `${a.time}T00:00:00` : a.time) - Date.parse(String(b.time).length === 10 ? `${b.time}T00:00:00` : b.time)));
  if (!items.length) return h('div', { class: 'empty' }, props.empty || 'Nothing yet');
  const groups = [];
  for (const it of items) {
    const d = dayOf(it.time);
    if (!groups.length || groups[groups.length - 1].day !== d) groups.push({ day: d, items: [] });
    groups[groups.length - 1].items.push(it);
  }
  const today = app.today();
  const dayLabel = (d) => { const r = D.relative(d, today); return ['today', 'yesterday', 'tomorrow'].includes(r) ? `${r[0].toUpperCase()}${r.slice(1)} · ${app.fmt.date(d)}` : app.fmt.date(d); };
  return h('div', { class: 'sa-tl' }, groups.map((g) => h('div', null,
    h('div', { class: 'sa-tl-day' }, dayLabel(g.day)),
    h('div', { class: 'sa-tl-list' }, g.items.map((it) => {
      const kids = [h('span', { class: 'sa-tl-dot', style: `--c:${tones[it.tone] || 'var(--accent)'}` }, it.icon || ''),
        h('span', { class: 't' }, it.title || ''),
        props.showTime !== false && String(it.time).length > 10 ? h('span', { class: 'm' }, app.fmt.time(it.time)) : null,
        it.body ? h('span', { class: 'b' }, it.body) : null];
      return it.onClick ? h('button', { class: 'sa-tl-it', type: 'button', onclick: () => it.onClick(it) }, kids) : h('div', { class: 'sa-tl-it' }, kids);
    })))));
}
