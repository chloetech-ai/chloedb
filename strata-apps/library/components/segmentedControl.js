// title: Segmented control
// description: Pick one of a few options shown side by side as joined buttons, with an optional count on each; for switching a list between Day, Week and Month, Open and Done, or Grid and List.
// props: options: [values] (strings or numbers) or [{value, label? (default the value), count? (a number shown after the label)}];
//   value: the chosen value (default none chosen); onChange(value) when the person picks another option (not called for the one already chosen);
//   size: 'normal' (default) or 'small'; full: true to stretch across the width, the options sharing it equally (default false);
//   label: accessible name of the group (default "Options")
function component(props) {
  css('sa-segmented', `
.sa-segmented{display:inline-flex;max-width:100%;overflow-x:auto;gap:2px;padding:3px;border-radius:var(--radius);background:color-mix(in srgb,var(--line) 60%,transparent);border:1px solid var(--line);vertical-align:middle;scrollbar-width:none}
.sa-segmented::-webkit-scrollbar{display:none}
.sa-segmented.full{display:flex;width:100%;overflow:hidden}
.sa-segmented button{flex:0 0 auto;display:inline-flex;align-items:center;justify-content:center;gap:6px;margin:0;border:0;box-shadow:none;background:transparent;color:var(--muted);padding:6px 14px;font:inherit;font-size:.95em;line-height:1.2;border-radius:calc(var(--radius) - 2px);cursor:pointer;white-space:nowrap;min-height:32px}
.sa-segmented.full button{flex:1 1 0;min-width:0}
.sa-segmented button .sa-seg-label{overflow:hidden;text-overflow:ellipsis}
.sa-segmented button:hover{color:var(--fg)}
.sa-segmented button.on{background:var(--card);color:var(--fg);font-weight:600;box-shadow:0 1px 3px color-mix(in srgb,var(--fg) 18%,transparent)}
.sa-segmented button:focus-visible{outline:2px solid var(--accent);outline-offset:1px}
.sa-segmented .sa-seg-count{font-size:.8em;font-weight:500;padding:0 6px;border-radius:999px;background:color-mix(in srgb,var(--line) 80%,transparent);color:var(--muted)}
.sa-segmented button.on .sa-seg-count{background:color-mix(in srgb,var(--accent) 18%,transparent);color:var(--accent)}
.sa-segmented.small{padding:2px}
.sa-segmented.small button{padding:3px 10px;font-size:.85em;min-height:26px}
`);
  const options = (Array.isArray(props.options) ? props.options : [])
    .filter((o) => o !== null && o !== undefined)
    .map((o) => (typeof o === 'object'
      ? { value: o.value, label: o.label !== undefined && o.label !== null ? String(o.label) : String(o.value), count: o.count }
      : { value: o, label: String(o) }));
  let current = props.value;
  const small = props.size === 'small';
  const root = h('div', {
    class: `sa-segmented${small ? ' small' : ''}${props.full ? ' full' : ''}`,
    role: 'radiogroup',
    'aria-label': props.label || 'Options',
  });
  const buttons = [];

  const paint = () => {
    const chosen = options.findIndex((o) => o.value === current);
    buttons.forEach((b, i) => {
      const on = i === chosen;
      b.classList.toggle('on', on);
      b.setAttribute('aria-checked', on ? 'true' : 'false');
      b.tabIndex = on || (chosen < 0 && i === 0) ? 0 : -1;
    });
  };

  const pick = (i, focus) => {
    const o = options[i];
    if (!o) return;
    if (focus) buttons[i].focus();
    if (o.value === current) return;
    current = o.value;
    paint();
    if (typeof props.onChange === 'function') props.onChange(o.value);
  };

  options.forEach((o, i) => {
    const hasCount = typeof o.count === 'number' && isFinite(o.count);
    const b = h('button', {
      type: 'button',
      role: 'radio',
      title: o.label,
      'aria-label': hasCount ? `${o.label} (${o.count})` : null,
      onclick: () => pick(i, false),
      onkeydown: (e) => {
        const n = options.length;
        let to = null;
        if (e.key === 'ArrowRight' || e.key === 'ArrowDown') to = (i + 1) % n;
        else if (e.key === 'ArrowLeft' || e.key === 'ArrowUp') to = (i - 1 + n) % n;
        else if (e.key === 'Home') to = 0;
        else if (e.key === 'End') to = n - 1;
        if (to === null) return;
        e.preventDefault();
        pick(to, true);
      },
    },
      h('span', { class: 'sa-seg-label' }, o.label),
      hasCount ? h('span', { class: 'sa-seg-count', 'aria-hidden': 'true' }, app.fmt.number(o.count)) : null);
    buttons.push(b);
    root.append(b);
  });

  paint();
  return root;
}
