// title: Progress bar
// description: A labelled bar that fills toward a goal (pages read, savings saved, budget spent, tasks done), with the amount beside it and a colour that turns to warning or alarm past chosen points.
// props: value: number (default 0; below 0 counts as 0, past max fills the bar); max: the goal, a number above 0 (default 100);
//   label: text above the bar, left (optional); showValue: true (default) shows '7 / 10', or '70%' when max is 100; false hides it;
//   format(value, max) -> text to show instead of the default; color: css color of the fill (default the accent);
//   warnAt: fraction of max (0.8) from which the fill takes the warning color (default none);
//   overAt: fraction of max (1) past which the fill takes the alarm color, for over budget (default none);
//   height: pixels of the bar (default 8)
function component(props) {
  css('sa-progress', `
.sa-progress{display:flex;flex-direction:column;gap:4px;width:100%;min-width:0}
.sa-progress .head{display:flex;align-items:baseline;gap:8px;font-size:.9em}
.sa-progress .lbl{flex:1;min-width:0;overflow:hidden;text-overflow:ellipsis;white-space:nowrap}
.sa-progress .val{margin-left:auto;color:var(--muted);font-variant-numeric:tabular-nums;white-space:nowrap}
.sa-progress .val.warn{color:var(--warn)}
.sa-progress .val.over{color:var(--bad);font-weight:600}
.sa-progress .track{height:var(--h);border-radius:calc(var(--h) / 2);background:var(--line);overflow:hidden}
.sa-progress .fill{height:100%;width:var(--w);background:var(--c);border-radius:inherit;transition:width .3s ease,background-color .3s ease}
@media (prefers-reduced-motion: reduce){.sa-progress .fill{transition:none}}
`);
  const num = (x, d) => (typeof x === 'number' || (typeof x === 'string' && x.trim() !== '')) && isFinite(Number(x)) ? Number(x) : d;
  const max = num(props.max, 100) > 0 ? num(props.max, 100) : 100;
  const value = Math.max(0, num(props.value, 0));
  const frac = value / max;
  const pct = Math.min(100, frac * 100);
  const warnAt = num(props.warnAt, null);
  const overAt = num(props.overAt, null);
  const state = overAt != null && frac > overAt ? 'over' : warnAt != null && frac >= warnAt ? 'warn' : '';
  const fill = state === 'over' ? 'var(--bad)' : state === 'warn' ? 'var(--warn)' : (props.color || 'var(--accent)');
  const height = Math.max(2, num(props.height, 8));

  const n = (x) => (app && app.fmt && app.fmt.number) ? app.fmt.number(Math.round(x * 100) / 100) : String(Math.round(x * 100) / 100);
  let text;
  if (typeof props.format === 'function') text = String(props.format(value, max) ?? '');
  else if (max === 100) text = `${Math.round(frac * 100)}%`;
  else text = `${n(value)} / ${n(max)}`;

  const show = props.showValue !== false;
  const label = props.label != null && props.label !== '' ? String(props.label) : '';
  const head = label || show
    ? h('div', { class: 'head' },
        label ? h('span', { class: 'lbl', title: label }, label) : null,
        show ? h('span', { class: `val${state ? ' ' + state : ''}` }, text) : null)
    : null;

  return h('div', {
      class: 'sa-progress',
      style: `--h:${height}px;--w:${pct}%;--c:${fill}`,
    },
    head,
    h('div', {
        class: 'track',
        role: 'progressbar',
        'aria-label': label || 'Progress',
        'aria-valuemin': '0',
        'aria-valuemax': String(max),
        'aria-valuenow': String(Math.min(value, max)),
        'aria-valuetext': text + (state === 'over' ? ' (over)' : ''),
      },
      h('div', { class: 'fill' })));
}
