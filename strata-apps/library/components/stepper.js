// title: Steps
// description: Shows where someone is in a process of several steps (checkout, onboarding, a form in parts, an order's progress) with done, current and coming steps, and lets them go back to a done step when allowed.
// props: steps: [{id, label, description?}] or [labels] (a label alone is also its id); current: the step now, as an index (0-based) or an id
//   (default 0; an index equal to the number of steps means every step is finished); onStep(index) makes done steps buttons that report
//   their index when tapped (the current step is never a button); vertical: true to stack the steps in a column with descriptions beside
//   (default false: a row, descriptions hidden below 480 px); done: [ids] the steps that are done, for when they are not simply the ones
//   before current (default: every step before current)
function component(props) {
  css('sa-stepper', `
.sa-stepper{--d:28px;list-style:none;margin:0;padding:0;display:flex;width:100%;box-sizing:border-box}
.sa-stepper>li{flex:1 1 0;min-width:0;position:relative;margin:0;padding:0}
.sa-stepper>li:not(:last-child)::after{content:'';position:absolute;top:calc(var(--d) / 2 - 1px);left:calc(50% + var(--d) / 2 + 4px);right:calc(-50% + var(--d) / 2 + 4px);height:2px;border-radius:1px;background:var(--line)}
.sa-stepper>li.done:not(:last-child)::after{background:var(--accent)}
.sa-stepper .hit{display:flex;flex-direction:column;align-items:center;text-align:center;gap:6px;width:100%;box-sizing:border-box;padding:0 4px;margin:0;border:0;background:none;box-shadow:none;color:inherit;font:inherit;border-radius:var(--radius)}
.sa-stepper button.hit{cursor:pointer}
.sa-stepper button.hit:hover .lbl{color:var(--accent)}
.sa-stepper button.hit:focus-visible{outline:2px solid var(--accent);outline-offset:2px}
.sa-stepper .mark{flex:none;width:var(--d);height:var(--d);border-radius:50%;box-sizing:border-box;display:grid;place-items:center;border:2px solid var(--line);background:var(--card);color:var(--muted);font-size:13px;font-weight:600;line-height:1;position:relative;z-index:1}
.sa-stepper .done .mark{background:var(--accent);border-color:var(--accent);color:var(--bg)}
.sa-stepper .current .mark{border-color:var(--accent);color:var(--accent);box-shadow:0 0 0 3px color-mix(in srgb,var(--accent) 22%,transparent)}
.sa-stepper .txt{min-width:0;display:flex;flex-direction:column;gap:2px}
.sa-stepper .lbl{font-size:14px;font-weight:500;line-height:1.3;overflow-wrap:anywhere}
.sa-stepper .todo .lbl{color:var(--muted)}
.sa-stepper .current .lbl{font-weight:600}
.sa-stepper .desc{font-size:12px;line-height:1.35;color:var(--muted);overflow-wrap:anywhere}
.sa-stepper .sr{position:absolute;width:1px;height:1px;overflow:hidden;clip:rect(0 0 0 0);white-space:nowrap}
.sa-stepper.v{flex-direction:column}
.sa-stepper.v>li{flex:none}
.sa-stepper.v>li:not(:last-child){padding-bottom:18px}
.sa-stepper.v>li:not(:last-child)::after{top:calc(var(--d) + 4px);bottom:4px;left:calc(var(--d) / 2 - 1px);right:auto;width:2px;height:auto}
.sa-stepper.v .hit{flex-direction:row;align-items:flex-start;text-align:left;gap:12px;padding:0}
.sa-stepper.v .txt{padding-top:calc((var(--d) - 18px) / 2)}
@media (max-width:480px){.sa-stepper:not(.v) .desc{display:none}.sa-stepper:not(.v) .lbl{font-size:12px}.sa-stepper:not(.v) .hit{padding:0 2px}}
`);
  const steps = (Array.isArray(props.steps) ? props.steps : [])
    .filter((s) => s != null)
    .map((s, i) => typeof s === 'object'
      ? { id: s.id != null ? s.id : i, label: String(s.label != null ? s.label : s.id != null ? s.id : `Step ${i + 1}`), description: s.description || '' }
      : { id: s, label: String(s), description: '' });
  const n = steps.length;

  let cur = 0;
  const c = props.current;
  if (typeof c === 'number' && Number.isFinite(c)) cur = Math.max(0, Math.min(n, Math.floor(c)));
  else if (c != null && c !== '') {
    const k = steps.findIndex((s) => String(s.id) === String(c));
    cur = k >= 0 ? k : 0;
  }

  const doneSet = Array.isArray(props.done) ? new Set(props.done.map(String)) : null;
  const isDone = (s, i) => (doneSet ? doneSet.has(String(s.id)) : i < cur);
  const canStep = typeof props.onStep === 'function';
  const vertical = !!props.vertical;

  const check = () => svg('svg', { viewBox: '0 0 16 16', width: 14, height: 14, 'aria-hidden': 'true' },
    svg('path', { d: 'M3 8.5l3 3 7-7', fill: 'none', stroke: 'currentColor', 'stroke-width': 2.2, 'stroke-linecap': 'round', 'stroke-linejoin': 'round' }));

  const root = h('ol', {
    class: `sa-stepper${vertical ? ' v' : ''}`,
    'aria-label': n ? (cur >= n ? `Progress: all ${n} steps finished` : `Progress: step ${cur + 1} of ${n}`) : 'Progress',
  });

  steps.forEach((s, i) => {
    const done = isDone(s, i);
    const state = i === cur ? 'current' : done ? 'done' : 'todo';
    const clickable = canStep && done && i !== cur;
    const status = state === 'current' ? 'current step' : done ? 'done' : 'not done yet';
    const inner = [
      h('span', { class: 'mark', 'aria-hidden': 'true' }, state === 'done' ? check() : String(i + 1)),
      h('span', { class: 'txt' },
        h('span', { class: 'lbl' }, s.label),
        s.description ? h('span', { class: 'desc' }, s.description) : null,
        h('span', { class: 'sr' }, ` (${status})`)),
    ];
    const hit = clickable
      ? h('button', { type: 'button', class: 'hit', title: `Go to ${s.label}`, onclick: () => props.onStep(i) }, inner)
      : h('div', { class: 'hit' }, inner);
    root.append(h('li', {
      class: `${state}${done ? ' done' : ''}`,
      'aria-current': i === cur ? 'step' : null,
    }, hit));
  });

  return root;
}
