// title: Week view
// description: A week of days with timed events drawn as blocks on an hour grid, overlapping ones side by side, with previous and next weeks, tapping an event to open it and an empty slot to add one; becomes a list of days on phones. For appointments, classes, shifts, bookings and plans.
// props: events: [{id, title, start (ISO time), end (ISO time; missing or not after start means 30 minutes), color? (css color, default the accent)}];
//   week: 'YYYY-MM-DD' (or an ISO time), any day of the week to show (default today); onWeek(day) when the person moves to another week, day being its first day;
//   onSelect(id) when an event is tapped (events are buttons only then); onSlot(startIso) when an empty slot is tapped, startIso being the half hour tapped
//   (on the phone list, a + per day gives the first hour of that day); hours: [from, to] whole hours shown (default [7, 21]; events outside are pinned to the edge);
//   weekStart: 1 Monday (default) or 0 Sunday
function component(props) {
  css('sa-week', `
.sa-week{container-type:inline-size;display:flex;flex-direction:column;gap:8px}
.sa-week-bar{display:flex;align-items:center;gap:6px;flex-wrap:wrap}
.sa-week-label{margin-left:6px}
.sa-week-grid{border:1px solid var(--line);border-radius:var(--radius);overflow:hidden;background:var(--card)}
.sa-week-row{display:grid;grid-template-columns:48px repeat(7,minmax(0,1fr))}
.sa-week-head{border-bottom:1px solid var(--line)}
.sa-week-head>div{padding:6px 2px;text-align:center;font-size:.8em;color:var(--muted);line-height:1.2}
.sa-week-head b{display:block;font-size:1.25em;color:var(--fg)}
.sa-week-head .today,.sa-week-head .today b{color:var(--accent)}
.sa-week-axis{position:relative}
.sa-week-axis span{position:absolute;right:6px;font-size:.7em;color:var(--muted);transform:translateY(-50%);white-space:nowrap}
.sa-week-axis span:first-child{transform:translateY(2px)}
.sa-week-col{position:relative;border-left:1px solid var(--line);background-image:repeating-linear-gradient(to bottom,var(--line) 0 1px,transparent 1px var(--hh))}
.sa-week-col.slot{cursor:pointer}
.sa-week-col.slot:hover{background-color:color-mix(in srgb,var(--accent) 4%,transparent)}
.sa-week-col.today{background-color:color-mix(in srgb,var(--accent) 6%,transparent)}
.sa-week-ev{position:absolute;box-sizing:border-box;margin:0;padding:2px 4px;border:0;border-left:3px solid var(--c);border-radius:4px;background:color-mix(in srgb,var(--c) 20%,var(--card));color:var(--fg);font:inherit;font-size:.75em;line-height:1.25;text-align:left;overflow:hidden;box-shadow:none;display:flex;flex-direction:column;justify-content:flex-start;align-items:stretch;min-width:0;min-height:0;cursor:default;z-index:1}
button.sa-week-ev{cursor:pointer}
button.sa-week-ev:hover,button.sa-week-ev:focus-visible{background:color-mix(in srgb,var(--c) 34%,var(--card))}
.sa-week-ev .t{font-weight:600;overflow:hidden;text-overflow:ellipsis;white-space:nowrap}
.sa-week-ev .w{color:var(--muted);overflow:hidden;text-overflow:ellipsis;white-space:nowrap}
.sa-week-now{position:absolute;left:0;right:0;height:2px;margin-top:-1px;background:var(--bad);z-index:2;pointer-events:none}
.sa-week-list{display:none;flex-direction:column;gap:10px}
@container (max-width: 599px){.sa-week-grid{display:none}.sa-week-list{display:flex}}
.sa-week-dh{display:flex;align-items:center;justify-content:space-between;gap:8px;border-bottom:1px solid var(--line);padding:4px 0}
.sa-week-day.today .sa-week-dh strong{color:var(--accent)}
.sa-week-item{display:flex;gap:10px;align-items:baseline;width:100%;box-sizing:border-box;text-align:left;padding:6px 4px;margin:0;border:0;background:none;box-shadow:none;color:var(--fg);font:inherit;border-radius:6px}
button.sa-week-item{cursor:pointer}
button.sa-week-item:hover,button.sa-week-item:focus-visible{background:color-mix(in srgb,var(--accent) 8%,transparent)}
.sa-week-item i{width:10px;height:10px;border-radius:50%;background:var(--c);flex:none;align-self:center}
.sa-week-item .w{color:var(--muted);font-size:.85em;white-space:nowrap}
.sa-week-item .t{min-width:0;overflow:hidden;text-overflow:ellipsis;white-space:nowrap}
.sa-week-none{color:var(--muted);font-size:.85em;padding:4px}
`);
  const D = lib.dates;
  const HH = 48;   // pixels per hour
  const MIN = 20;  // shortest block, in minutes
  const ws = props.weekStart === 0 ? 0 : 1;
  const hrs = Array.isArray(props.hours) ? props.hours : [];
  const f0 = Math.floor(Number(hrs[0]));
  const from = Number.isFinite(f0) ? Math.max(0, Math.min(23, f0)) : 7;
  const t0 = Math.ceil(Number(hrs[1]));
  const to = Number.isFinite(t0) && t0 > from ? Math.min(24, t0) : Math.max(from + 1, Math.min(24, 21 > from ? 21 : from + 1));
  const span = (to - from) * 60;
  const onSelect = typeof props.onSelect === 'function' ? props.onSelect : null;
  const onSlot = typeof props.onSlot === 'function' ? props.onSlot : null;

  const pad = (n) => String(n).padStart(2, '0');
  const localDay = (d) => `${d.getFullYear()}-${pad(d.getMonth() + 1)}-${pad(d.getDate())}`;
  const at = (day, minutes) => { const [y, m, d] = day.split('-').map(Number); return new Date(y, m - 1, d, 0, minutes); };
  const clock = (d) => app.fmt.time(d.toISOString());
  const clean = (c) => String(c || 'var(--accent)').replace(/[;{}<>"]/g, '');

  function firstDay(w) {
    let day = null;
    if (typeof w === 'string' && D.isDay(w)) day = w;
    else if (w) { const d = new Date(w); if (!isNaN(d)) day = localDay(d); }
    return D.startOfWeek(day || app.today(), ws);
  }

  const events = [];
  for (const ev of Array.isArray(props.events) ? props.events : []) {
    if (!ev) continue;
    const s = new Date(ev.start);
    if (isNaN(s)) continue;
    let e = new Date(ev.end);
    if (isNaN(e) || e <= s) e = new Date(s.getTime() + 30 * 60000);
    events.push({ ev, s, e, title: String(ev.title || '').trim() || '(No title)', color: clean(ev.color) });
  }

  // The events touching a day, with their place on the grid and a column among the ones they overlap.
  function segments(day) {
    const d0 = at(day, 0), d1 = at(D.addDays(day, 1), 0), v0 = at(day, from * 60), v1 = at(day, to * 60);
    const out = [];
    for (const x of events) {
      if (!(x.s < d1 && x.e > d0)) continue;
      const a = Math.max(x.s, v0), b = Math.min(x.e, v1);
      let top, len;
      if (b > a) { top = (a - v0) / 60000; len = (b - a) / 60000; }
      else { top = x.e <= v0 ? 0 : span; len = 0; }
      len = Math.max(len, MIN);
      top = Math.max(0, Math.min(top, span - len));
      out.push({ ...x, top, len });
    }
    out.sort((p, q) => p.top - q.top || q.len - p.len);
    let cluster = [], cols = [], end = -1;
    const flush = () => { for (const c of cluster) c.n = cols.length; cluster = []; cols = []; };
    for (const g of out) {
      if (g.top >= end) { flush(); end = -1; }
      let i = cols.findIndex((stop) => stop <= g.top);
      if (i < 0) { i = cols.length; cols.push(0); }
      cols[i] = g.top + g.len;
      g.col = i;
      cluster.push(g);
      end = Math.max(end, g.top + g.len);
    }
    flush();
    return out;
  }

  function when(x, day) {
    const part = (d, dd) => (dd === day ? clock(d) : `${D.weekdayName(dd, true)} ${clock(d)}`);
    return `${part(x.s, localDay(x.s))} – ${part(x.e, localDay(new Date(x.e - 1)))}`;
  }

  function weekLabel(a) {
    const b = D.addDays(a, 6);
    const da = Number(a.slice(8)), db = Number(b.slice(8));
    if (a.slice(0, 7) === b.slice(0, 7)) return `${da} – ${db} ${D.monthName(a, true)} ${a.slice(0, 4)}`;
    if (a.slice(0, 4) === b.slice(0, 4)) return `${da} ${D.monthName(a, true)} – ${db} ${D.monthName(b, true)} ${b.slice(0, 4)}`;
    return `${da} ${D.monthName(a, true)} ${a.slice(0, 4)} – ${db} ${D.monthName(b, true)} ${b.slice(0, 4)}`;
  }

  let start = firstDay(props.week);
  const root = h('div', { class: 'sa-week', role: 'region' });

  function go(next) {
    if (next === start) return;
    start = next;
    draw();
    if (typeof props.onWeek === 'function') props.onWeek(start);
  }

  function block(g, day) {
    const label = `${g.title}, ${when(g, day)}`;
    const style = `top:${(g.top / 60) * HH}px;height:${(g.len / 60) * HH - 2}px;` +
      `left:calc(${(g.col / g.n) * 100}% + 1px);width:calc(${100 / g.n}% - 3px);--c:${g.color}`;
    const short = g.len < 40;
    return h(onSelect ? 'button' : 'div', {
      type: onSelect ? 'button' : null, class: 'sa-week-ev', style, title: label, 'aria-label': label,
      onclick: (e) => { e.stopPropagation(); if (onSelect) onSelect(g.ev.id); }
    }, h('span', { class: 't' }, short ? `${clock(g.s)} ${g.title}` : g.title), short ? null : h('span', { class: 'w' }, when(g, day)));
  }

  function draw() {
    const today = app.today();
    const days = D.range(start, D.addDays(start, 6));
    const segs = days.map(segments);
    const label = weekLabel(start);
    root.setAttribute('aria-label', `Week of ${label}`);

    const bar = h('div', { class: 'sa-week-bar' },
      h('button', { type: 'button', class: 'small icon', 'aria-label': 'Previous week', onclick: () => go(D.addDays(start, -7)) }, '‹'),
      h('button', { type: 'button', class: 'small', disabled: days.includes(today), onclick: () => go(D.startOfWeek(today, ws)) }, 'Today'),
      h('button', { type: 'button', class: 'small icon', 'aria-label': 'Next week', onclick: () => go(D.addDays(start, 7)) }, '›'),
      h('strong', { class: 'sa-week-label', 'aria-live': 'polite' }, label));

    const head = h('div', { class: 'sa-week-row sa-week-head', 'aria-hidden': 'true' }, h('div'),
      days.map((day) => h('div', { class: day === today ? 'today' : null }, D.weekdayName(day, true), h('b', null, Number(day.slice(8))))));

    const axis = h('div', { class: 'sa-week-axis', style: `height:${(to - from) * HH}px`, 'aria-hidden': 'true' });
    for (let hr = from; hr < to; hr++) {
      axis.append(h('span', { style: `top:${(hr - from) * HH}px` }, clock(new Date(2000, 0, 1, hr))));
    }

    const cols = days.map((day, i) => {
      const col = h('div', {
        class: `sa-week-col${onSlot ? ' slot' : ''}${day === today ? ' today' : ''}`,
        style: `height:${(to - from) * HH}px;--hh:${HH}px`,
        role: 'group', 'aria-label': `${D.weekdayName(day)} ${app.fmt.date(day)}`
      });
      if (onSlot) {
        col.onclick = (e) => {
          const y = e.clientY - col.getBoundingClientRect().top;
          const m = Math.max(0, Math.min(span - 30, Math.floor((y / HH) * 2) * 30));
          onSlot(at(day, from * 60 + m).toISOString());
        };
      }
      if (day === today) {
        const mins = (new Date(app.now()) - at(day, from * 60)) / 60000;
        if (mins >= 0 && mins <= span) col.append(h('div', { class: 'sa-week-now', style: `top:${(mins / 60) * HH}px` }));
      }
      col.append(segs[i].map((g) => block(g, day)));
      return col;
    });

    const grid = h('div', { class: 'sa-week-grid' }, head, h('div', { class: 'sa-week-row' }, axis, cols));

    const list = h('div', { class: 'sa-week-list' }, days.map((day, i) => {
      const items = segs[i].slice().sort((p, q) => p.s - q.s || p.e - q.e);
      const name = `${D.weekdayName(day, true)} ${Number(day.slice(8))} ${D.monthName(day, true)}`;
      return h('section', { class: `sa-week-day${day === today ? ' today' : ''}` },
        h('div', { class: 'sa-week-dh' },
          h('strong', null, name, day === today ? h('span', { class: 'muted small' }, ' · today') : null),
          onSlot ? h('button', { type: 'button', class: 'ghost small icon', 'aria-label': `Add on ${name}`, onclick: () => onSlot(at(day, from * 60).toISOString()) }, '+') : null),
        items.length
          ? items.map((g) => h(onSelect ? 'button' : 'div', {
              type: onSelect ? 'button' : null, class: 'sa-week-item', style: `--c:${g.color}`,
              onclick: onSelect ? () => onSelect(g.ev.id) : null
            }, h('i', { 'aria-hidden': 'true' }), h('span', { class: 'w' }, when(g, day)), h('span', { class: 't' }, g.title)))
          : h('div', { class: 'sa-week-none' }, 'Nothing planned'));
    }));

    root.replaceChildren(bar, grid, list);
  }

  draw();
  return root;
}
