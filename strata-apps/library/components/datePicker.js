// title: Date picker
// description: A date field that opens a month to choose a day from, with quick picks such as Today, Tomorrow and Next week and a button to clear it; for due dates, appointments, birthdays and any day a form asks for.
// props: value: 'YYYY-MM-DD' or '' (default '', no date); onChange(day) with the chosen 'YYYY-MM-DD', or '' when cleared;
//   quick: quick picks shown under the field, each 'today' | 'tomorrow' | 'yesterday' | 'next week' (the first day of next week) | 'next month'
//   | a 'YYYY-MM-DD' day | {label, date: 'YYYY-MM-DD'} (default ['today', 'tomorrow', 'next week']; [] or false for none);
//   min, max: 'YYYY-MM-DD', days outside are disabled (default none); clearable: show a button to clear the date (default true);
//   placeholder: text when there is no date (default 'Choose a date'); weekStart: 1 Monday (default) or 0 Sunday;
//   label: accessible name of the field (default 'Date')
function component(props) {
  css('sa-datepicker', `
.sa-datepicker{display:inline-flex;flex-direction:column;gap:6px;max-width:100%;vertical-align:top}
.sa-datepicker .sa-dp-box{position:relative;display:flex;align-items:stretch;gap:4px;max-width:100%}
.sa-datepicker .sa-dp-field{display:inline-flex;align-items:center;gap:8px;min-width:13rem;max-width:100%;flex:1;justify-content:flex-start;text-align:left;font-weight:normal}
.sa-datepicker .sa-dp-field svg{flex:none;color:var(--muted)}
.sa-datepicker .sa-dp-field .ph{color:var(--muted)}
.sa-datepicker .sa-dp-field .val{white-space:nowrap}
.sa-datepicker .sa-dp-field .rel{margin-left:auto;padding-left:8px;color:var(--muted);font-size:.85em;white-space:nowrap}
.sa-datepicker .sa-dp-clear{flex:none;padding:0 .7em;font-size:1.1em;line-height:1}
.sa-datepicker .sa-dp-quick .chip small{color:var(--muted);margin-left:4px}
.sa-datepicker .sa-dp-quick .chip.on small{color:inherit;opacity:.8}
.sa-datepicker .sa-dp-pop{position:absolute;top:calc(100% + 4px);left:0;z-index:30;width:17rem;max-width:calc(100vw - 16px);box-sizing:border-box;padding:8px;background:var(--card);color:var(--fg);border:1px solid var(--line);border-radius:var(--radius);box-shadow:0 8px 24px rgba(0,0,0,.18)}
.sa-datepicker .sa-dp-pop.flip{left:auto;right:0}
.sa-datepicker .sa-dp-head{display:flex;align-items:center;justify-content:space-between;gap:4px;margin-bottom:6px}
.sa-datepicker .sa-dp-head strong{font-size:.95em}
.sa-datepicker .sa-dp-head button{padding:0;width:2rem;height:2rem;font-size:1.2em;line-height:1}
.sa-datepicker .sa-dp-grid{display:grid;grid-template-columns:repeat(7,1fr);gap:2px;text-align:center}
.sa-datepicker .sa-dp-wd{font-size:.75em;color:var(--muted);padding:2px 0}
.sa-datepicker .sa-dp-grid button{margin:0;padding:0;height:2.1rem;min-width:0;border:0;box-shadow:none;background:none;color:var(--fg);border-radius:8px;font-weight:normal;cursor:pointer}
.sa-datepicker .sa-dp-grid button:hover:not(:disabled){background:var(--line)}
.sa-datepicker .sa-dp-grid button.today{box-shadow:inset 0 0 0 1px var(--accent)}
.sa-datepicker .sa-dp-grid button.sel,.sa-datepicker .sa-dp-grid button.sel:hover{background:var(--accent);color:var(--bg);font-weight:600}
.sa-datepicker .sa-dp-grid button:disabled{opacity:.35;cursor:default}
.sa-datepicker .sa-dp-grid button:focus-visible{outline:2px solid var(--accent);outline-offset:1px}
`);
  const D = lib.dates;
  const weekStart = Number(props.weekStart) === 0 ? 0 : 1;
  const min = D.isDay(props.min) ? props.min : null;
  const max = D.isDay(props.max) ? props.max : null;
  const clearable = props.clearable !== false;
  const placeholder = props.placeholder || 'Choose a date';
  const label = props.label || 'Date';
  const report = typeof props.onChange === 'function' ? props.onChange : () => {};
  const today = app.today();
  const inRange = (d) => (!min || d >= min) && (!max || d <= max);
  const clamp = (d) => (min && d < min ? min : max && d > max ? max : d);
  const fmt = (d) => `${D.weekdayName(d, true)} ${Number(d.slice(8))} ${D.monthName(d, true)} ${d.slice(0, 4)}`;
  const short = (d) => `${D.weekdayName(d, true)} ${Number(d.slice(8))}`;

  const NAMED = {
    today: ['Today', () => today],
    tomorrow: ['Tomorrow', () => D.addDays(today, 1)],
    yesterday: ['Yesterday', () => D.addDays(today, -1)],
    'next week': ['Next week', () => D.startOfWeek(D.addDays(today, 7), weekStart)],
    'next month': ['Next month', () => D.addMonths(today, 1)],
  };
  const resolve = (q) => {
    if (q && typeof q === 'object') return D.isDay(q.date) ? { label: q.label || short(q.date), date: q.date } : null;
    const k = String(q == null ? '' : q).trim().toLowerCase();
    if (NAMED[k]) return { label: NAMED[k][0], date: NAMED[k][1]() };
    return D.isDay(k) ? { label: short(k), date: k } : null;
  };
  const quick = (props.quick === false ? [] : Array.isArray(props.quick) ? props.quick : ['today', 'tomorrow', 'next week'])
    .map(resolve).filter(Boolean);

  let value = D.isDay(props.value) ? props.value : '';
  let open = false;
  let focus = clamp(value || today);
  let month = D.month(focus);

  const root = h('div', { class: 'sa-datepicker' });
  const box = h('div', { class: 'sa-dp-box' });

  const onDoc = (e) => {
    if (!root.isConnected) return detach();
    if (!root.contains(e.target)) close(false);
  };
  function attach() { document.addEventListener('pointerdown', onDoc, true); document.addEventListener('focusin', onDoc, true); }
  function detach() { document.removeEventListener('pointerdown', onDoc, true); document.removeEventListener('focusin', onDoc, true); }

  function show() {
    open = true;
    focus = clamp(value || today);
    month = D.month(focus);
    attach();
    draw(`[data-day="${focus}"]`);
  }
  function close(refocus) {
    if (!open) return;
    open = false;
    detach();
    draw(refocus ? '.sa-dp-field' : null);
  }
  function set(d, sel) {
    if (d && !inRange(d)) return;
    value = d;
    if (open) { open = false; detach(); }
    draw(sel);
    report(d);
  }

  root.addEventListener('keydown', (e) => {
    if (e.key === 'Escape' && open) { e.preventDefault(); e.stopPropagation(); close(true); }
  });

  const MOVES = { ArrowLeft: -1, ArrowRight: 1, ArrowUp: -7, ArrowDown: 7 };
  function onGridKey(e) {
    let d;
    if (e.key in MOVES) d = D.addDays(focus, MOVES[e.key]);
    else if (e.key === 'Home') d = D.startOfWeek(focus, weekStart);
    else if (e.key === 'End') d = D.endOfWeek(focus, weekStart);
    else if (e.key === 'PageUp') d = D.addMonths(focus, e.shiftKey ? -12 : -1);
    else if (e.key === 'PageDown') d = D.addMonths(focus, e.shiftKey ? 12 : 1);
    else return;
    e.preventDefault();
    focus = clamp(d);
    month = D.month(focus);
    draw(`[data-day="${focus}"]`);
  }

  function pop() {
    const first = month + '-01';
    const prevOk = !min || D.endOfMonth(D.addMonths(first, -1)) >= min;
    const nextOk = !max || D.addMonths(first, 1) <= max;
    const go = (n) => {
      focus = clamp(D.addMonths(focus, n));
      month = D.month(focus);
      draw(`[data-nav="${n}"]`);
    };
    const title = `${D.monthName(month)} ${month.slice(0, 4)}`;
    const cells = [];
    for (let i = 0; i < 7; i++) {
      cells.push(h('div', { class: 'sa-dp-wd', 'aria-hidden': 'true' }, D.weekdayName((weekStart + i) % 7, true).slice(0, 2)));
    }
    for (const week of D.monthGrid(month, weekStart)) {
      for (const d of week) {
        if (!d) { cells.push(h('div', { 'aria-hidden': 'true' })); continue; }
        const cls = [d === value ? 'sel' : '', d === today ? 'today' : ''].filter(Boolean).join(' ');
        cells.push(h('button', {
          type: 'button', class: cls || null, 'data-day': d, tabindex: d === focus ? '0' : '-1',
          disabled: inRange(d) ? null : true, 'aria-label': fmt(d), 'aria-pressed': d === value ? 'true' : 'false',
          'aria-current': d === today ? 'date' : null, onclick: () => set(d, '.sa-dp-field'),
        }, Number(d.slice(8))));
      }
    }
    return h('div', { class: 'sa-dp-pop', role: 'dialog', 'aria-label': 'Choose a date' },
      h('div', { class: 'sa-dp-head' },
        h('button', { type: 'button', class: 'ghost', 'data-nav': '-1', 'aria-label': 'Previous month', disabled: prevOk ? null : true, onclick: () => go(-1) }, '‹'),
        h('strong', { 'aria-live': 'polite' }, title),
        h('button', { type: 'button', class: 'ghost', 'data-nav': '1', 'aria-label': 'Next month', disabled: nextOk ? null : true, onclick: () => go(1) }, '›')),
      h('div', { class: 'sa-dp-grid', role: 'group', 'aria-label': title, onkeydown: onGridKey }, cells));
  }

  const icon = () => svg('svg', { viewBox: '0 0 24 24', width: 16, height: 16, fill: 'none', stroke: 'currentColor', 'stroke-width': 2, 'stroke-linecap': 'round', 'aria-hidden': 'true' },
    svg('rect', { x: 3, y: 5, width: 18, height: 16, rx: 2 }), svg('path', { d: 'M3 10h18M8 3v4M16 3v4' }));

  function draw(sel) {
    const field = h('button', {
      type: 'button', class: 'sa-dp-field', 'aria-haspopup': 'dialog', 'aria-expanded': open ? 'true' : 'false',
      'aria-label': `${label}: ${value ? fmt(value) : 'none chosen'}`, onclick: () => (open ? close(true) : show()),
    }, icon(),
      value ? h('span', { class: 'val' }, fmt(value)) : h('span', { class: 'ph' }, placeholder),
      value ? h('span', { class: 'rel' }, D.relative(value, today)) : null);
    const clear = clearable && value
      ? h('button', { type: 'button', class: 'sa-dp-clear ghost', title: 'Clear', 'aria-label': 'Clear date', onclick: () => set('', '.sa-dp-field') }, '×')
      : null;
    const panel = open ? pop() : null;
    box.replaceChildren(...[field, clear, panel].filter(Boolean));
    const chips = quick.length
      ? h('div', { class: 'chips sa-dp-quick', role: 'group', 'aria-label': 'Quick picks' }, quick.map((q) =>
        h('button', {
          type: 'button', class: `chip${q.date === value ? ' on' : ''}`, title: fmt(q.date),
          'aria-pressed': q.date === value ? 'true' : 'false', disabled: inRange(q.date) ? null : true,
          onclick: () => set(q.date),
        }, q.label, q.label === short(q.date) || q.date === today ? null : h('small', null, short(q.date)))))
      : null;
    root.replaceChildren(...[box, chips].filter(Boolean));
    if (panel && panel.isConnected) {
      const r = panel.getBoundingClientRect();
      if (r.right > window.innerWidth - 4) panel.classList.add('flip');
    }
    if (sel) {
      const el = root.querySelector(sel);
      if (el && !el.disabled) el.focus();
      else if (open) { const day = root.querySelector(`[data-day="${focus}"]`); if (day) day.focus(); }
    }
  }

  draw();
  return root;
}
