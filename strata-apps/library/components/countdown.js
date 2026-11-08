// title: Countdown
// description: Time left until a moment (an event, a deadline, a trip, a launch), counted down every second in boxes or on one line, saying "Now" when it arrives.
// props: to: the moment, an ISO time or 'YYYY-MM-DD' (that day's local midnight); label: text above or before the time (optional);
//   units: 'auto' (default: the three largest that matter: days, hours, minutes while a day or more is left, down to seconds near the end)
//   or a list of the units to show, such as ['days', 'hours'] (the largest takes all that is above it, the smallest is rounded up);
//   onDone() when the moment is reached while it is shown (not when it had passed already); doneText: what it shows from then on (default 'Now');
//   compact: true for one short line ("2d 4h 13m") instead of boxes (default false). It ticks every second while it is in the page.
function component(props) {
  css('sa-countdown', `
.sa-countdown{display:inline-flex;flex-direction:column;gap:6px;max-width:100%}
.sa-countdown .lbl{color:var(--muted);font-size:.9em}
.sa-countdown .units{display:flex;gap:8px;flex-wrap:wrap}
.sa-countdown .unit{display:flex;flex-direction:column;align-items:center;min-width:58px;padding:8px 10px;background:var(--card);border:1px solid var(--line);border-radius:var(--radius)}
.sa-countdown .n{font-size:1.8em;font-weight:600;line-height:1.1;font-variant-numeric:tabular-nums}
.sa-countdown .u{font-size:.75em;color:var(--muted)}
.sa-countdown .done{font-size:1.6em;font-weight:600;color:var(--accent)}
.sa-countdown.compact{display:inline-flex;flex-direction:row;align-items:baseline;gap:6px;flex-wrap:wrap}
.sa-countdown.compact .lbl{font-size:inherit}
.sa-countdown.compact .t{font-variant-numeric:tabular-nums;font-weight:600;white-space:nowrap}
.sa-countdown.compact .done{font-size:inherit}
`);
  const ORDER = ['days', 'hours', 'minutes', 'seconds'];
  const SIZE = { days: 86400, hours: 3600, minutes: 60, seconds: 1 };
  const SHORT = { days: 'd', hours: 'h', minutes: 'm', seconds: 's' };
  const ONE = { days: 'day', hours: 'hour', minutes: 'minute', seconds: 'second' };

  const parse = (v) => {
    if (v instanceof Date) return isNaN(v) ? null : v.getTime();
    if (typeof v === 'number') return isFinite(v) ? v : null;
    if (typeof v !== 'string' || !v.trim()) return null;
    const m = /^(\d{4})-(\d{2})-(\d{2})$/.exec(v.trim());
    const t = m ? new Date(+m[1], +m[2] - 1, +m[3]).getTime() : new Date(v).getTime();
    return isNaN(t) ? null : t;
  };
  const target = parse(props.to);
  const label = props.label != null && props.label !== '' ? String(props.label) : null;
  const doneText = props.doneText == null ? 'Now' : String(props.doneText);
  const compact = !!props.compact;
  const fixed = Array.isArray(props.units) ? ORDER.filter((u) => props.units.includes(u)) : [];

  const pick = (secs) => {
    if (fixed.length) return fixed;
    const i = secs >= 86400 ? 0 : secs >= 3600 ? 1 : secs >= 60 ? 2 : 3;
    return ORDER.slice(i, i + 3);
  };
  const parts = (secs) => {
    const units = pick(secs);
    const last = SIZE[units[units.length - 1]];
    let rest = Math.ceil(secs / last) * last;
    return units.map((u) => { const n = Math.floor(rest / SIZE[u]); rest -= n * SIZE[u]; return { u, n }; });
  };
  const secsLeft = () => Math.ceil((target - Date.now()) / 1000);

  const root = h('div', { class: `sa-countdown${compact ? ' compact' : ''}`, role: 'timer' });
  if (target != null) {
    const dayOnly = typeof props.to === 'string' && /^\d{4}-\d{2}-\d{2}$/.test(props.to.trim());
    const iso = new Date(target).toISOString();
    root.title = `Until ${dayOnly ? app.fmt.date(iso) : app.fmt.datetime(iso)}`;
  }
  const lbl = label ? h('span', { class: 'lbl' }, label) : null;
  const body = h(compact ? 'span' : 'div', null);
  root.append(lbl, body);

  const update = (secs) => {
    const pre = label ? `${label}: ` : '';
    if (target == null) {
      body.replaceChildren(h('span', { class: compact ? 't' : 'n' }, '—'));
      root.setAttribute('aria-label', `${pre}no date`);
      return;
    }
    if (secs <= 0) {
      body.className = '';
      body.replaceChildren(h('span', { class: 'done' }, doneText));
      root.setAttribute('aria-label', pre + doneText);
      return;
    }
    const ps = parts(secs);
    root.setAttribute('aria-label', pre + ps.map((p) => `${p.n} ${ONE[p.u]}${p.n === 1 ? '' : 's'}`).join(', ') + ' left');
    if (compact) {
      body.className = 't';
      body.replaceChildren(ps.map((p) => `${p.n}${SHORT[p.u]}`).join(' '));
    } else {
      body.className = 'units';
      body.replaceChildren(ps.map((p, i) => h('div', { class: 'unit', 'aria-hidden': 'true' },
        h('span', { class: 'n' }, i > 0 ? String(p.n).padStart(2, '0') : String(p.n)),
        h('span', { class: 'u' }, p.n === 1 ? ONE[p.u] : p.u))));
    }
  };

  let secs = target == null ? 0 : secsLeft();
  update(secs);
  if (target != null && secs > 0) {
    let seen = false, waited = 0;
    const tick = () => {
      if (root.isConnected) seen = true;
      else if (seen || ++waited > 30) return; // removed from the page, or never shown: stop
      const now = secsLeft();
      if (now !== secs) { secs = now; update(secs); }
      if (secs <= 0) {
        if (seen && typeof props.onDone === 'function') { try { props.onDone(); } catch (e) { console.error(e); } }
        return;
      }
      const ms = target - Date.now();
      setTimeout(tick, (ms % 1000 || 1000) + 15);
    };
    setTimeout(tick, ((target - Date.now()) % 1000 || 1000) + 15);
  }
  return root;
}
