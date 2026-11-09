// title: Date range picker
// description: Choose a span of days (a report period, a filter for spending, workouts or tasks) by typing a from and a to date or tapping a preset such as this week, last month or all time.
// props: value: {from, to} each 'YYYY-MM-DD' or null for an open end (default {from: null, to: null}, all time);
//   onChange({from, to}) when the person picks a preset or edits a date (from is never after to; null means open-ended);
//   presets: which quick picks to show, in order: names among 'thisWeek', 'last7', 'thisMonth', 'lastMonth', 'last30', 'thisYear', 'all',
//   and/or custom {label, from, to} (default all seven named ones; false or [] shows none);
//   today: 'YYYY-MM-DD' the presets count from (default app.today()); weekStart: 1 Monday (default) or 0 Sunday, for "this week";
//   label: accessible name of the group (default "Date range")
function component(props) {
  css('sa-daterange', `
.sa-daterange{display:flex;flex-direction:column;gap:8px;min-width:0}
.sa-daterange .sa-dr-fields{display:flex;flex-wrap:wrap;gap:8px;align-items:flex-end}
.sa-daterange label{display:flex;flex-direction:column;gap:2px;flex:1 1 140px;min-width:0;font-size:.85em;color:var(--muted)}
.sa-daterange input{width:100%;min-width:0;box-sizing:border-box;color:var(--fg)}
.sa-daterange .sa-dr-sum{font-size:.85em;color:var(--muted)}
`);
  const D = lib.dates;
  const day = (v) => (typeof v === 'string' && D.isDay(v) ? v : null);
  const today = day(props.today) || app.today();
  const ws = props.weekStart === 0 ? 0 : 1;
  const year = today.slice(0, 4);
  const lastMonth = D.addMonths(D.startOfMonth(today), -1);
  const builtIn = {
    thisWeek: { label: 'This week', from: D.startOfWeek(today, ws), to: D.endOfWeek(today, ws) },
    last7: { label: 'Last 7 days', from: D.addDays(today, -6), to: today },
    thisMonth: { label: 'This month', from: D.startOfMonth(today), to: D.endOfMonth(today) },
    lastMonth: { label: 'Last month', from: lastMonth, to: D.endOfMonth(lastMonth) },
    last30: { label: 'Last 30 days', from: D.addDays(today, -29), to: today },
    thisYear: { label: 'This year', from: `${year}-01-01`, to: `${year}-12-31` },
    all: { label: 'All time', from: null, to: null },
  };
  const names = ['thisWeek', 'last7', 'thisMonth', 'lastMonth', 'last30', 'thisYear', 'all'];
  const list = props.presets === false ? [] : Array.isArray(props.presets) ? props.presets : names;
  const presets = list.map((p) => {
    if (typeof p === 'string') return builtIn[p] || null;
    if (p && p.label) {
      const r = { label: String(p.label), from: day(p.from), to: day(p.to) };
      if (r.from && r.to && r.from > r.to) [r.from, r.to] = [r.to, r.from];
      return r;
    }
    return null;
  }).filter(Boolean);

  const v = props.value || {};
  let cur = { from: day(v.from), to: day(v.to) };
  if (cur.from && cur.to && cur.from > cur.to) cur = { from: cur.to, to: cur.from };

  const show = (d) => `${Number(d.slice(8))} ${D.monthName(d, true)} ${d.slice(0, 4)}`;
  const summary = () => {
    if (!cur.from && !cur.to) return 'All dates';
    if (!cur.to) return `From ${show(cur.from)}`;
    if (!cur.from) return `Until ${show(cur.to)}`;
    const n = D.diffDays(cur.from, cur.to) + 1;
    return `${cur.from === cur.to ? show(cur.from) : `${show(cur.from)} – ${show(cur.to)}`} · ${n} day${n === 1 ? '' : 's'}`;
  };

  function set(next, edited) {
    if (next.from && next.to && next.from > next.to) {
      if (edited === 'to') next.from = next.to; else next.to = next.from;
    }
    cur = next;
    sync();
    if (typeof props.onChange === 'function') props.onChange({ from: cur.from, to: cur.to });
  }

  const fromIn = h('input', { type: 'date', onchange: (e) => set({ from: day(e.target.value), to: cur.to }, 'from') });
  const toIn = h('input', { type: 'date', onchange: (e) => set({ from: cur.from, to: day(e.target.value) }, 'to') });
  const sum = h('div', { class: 'sa-dr-sum', 'aria-live': 'polite' });
  const chips = presets.map((p) => h('button', {
    type: 'button', class: 'chip', 'aria-pressed': 'false',
    onclick: () => set({ from: p.from, to: p.to }, null),
  }, p.label));

  function attr(el, name, val) { if (val) el.setAttribute(name, val); else el.removeAttribute(name); }
  function sync() {
    fromIn.value = cur.from || '';
    toIn.value = cur.to || '';
    attr(fromIn, 'max', cur.to);
    attr(toIn, 'min', cur.from);
    presets.forEach((p, i) => {
      const on = p.from === cur.from && p.to === cur.to;
      chips[i].classList.toggle('on', on);
      chips[i].setAttribute('aria-pressed', on ? 'true' : 'false');
    });
    sum.textContent = summary();
  }

  const root = h('div', { class: 'sa-daterange', role: 'group', 'aria-label': props.label || 'Date range' },
    chips.length ? h('div', { class: 'chips', role: 'toolbar', 'aria-label': 'Quick ranges' }, chips) : null,
    h('div', { class: 'sa-dr-fields' },
      h('label', null, 'From', fromIn),
      h('label', null, 'To', toIn)),
    sum);
  sync();
  return root;
}
