// title: Filter chips
// description: A row of chips to filter a list (status, category, tag, period), one at a time or several at once, with an optional count on each and an "All" chip.
// props: options: [values] or [{value, label?, count?}]; value: the chosen value (or [values] when multiple);
//   onChange(value or [values]); multiple: true to allow several; all: label of a chip that clears the choice (default "All"; false for none)
function component(props) {
  css('sa-chips', `
.sa-chips{display:flex;flex-wrap:wrap;gap:6px}
.sa-chips button{border:1px solid var(--line);background:var(--card);border-radius:999px;padding:4px 12px;font-size:13px;cursor:pointer;box-shadow:none;display:inline-flex;gap:6px;align-items:center}
.sa-chips button:hover{border-color:var(--fg)}
.sa-chips button.on{background:var(--fg);color:var(--bg);border-color:var(--fg)}
.sa-chips button .n{font-size:11.5px;opacity:.7;font-variant-numeric:tabular-nums}
`);
  const opts = (props.options || []).map((o) => (typeof o === 'object' && o !== null ? o : { value: o })).map((o) => ({ ...o, label: o.label ?? String(o.value) }));
  const multiple = !!props.multiple;
  const chosen = new Set(multiple ? (Array.isArray(props.value) ? props.value : []).map(String) : props.value === null || props.value === undefined || props.value === '' ? [] : [String(props.value)]);
  const emit = () => {
    if (!props.onChange) return;
    const vals = opts.filter((o) => chosen.has(String(o.value))).map((o) => o.value);
    props.onChange(multiple ? vals : vals.length ? vals[0] : null);
  };
  const root = h('div', { class: 'sa-chips', role: 'group' });
  const draw = () => {
    root.replaceChildren(
      props.all === false ? null : h('button', { type: 'button', class: chosen.size ? null : 'on', 'aria-pressed': chosen.size ? 'false' : 'true', onclick: () => { chosen.clear(); draw(); emit(); } }, props.all || 'All'),
      opts.map((o) => {
        const on = chosen.has(String(o.value));
        return h('button', { type: 'button', class: on ? 'on' : null, 'aria-pressed': on ? 'true' : 'false', onclick: () => {
          if (multiple) { if (on) chosen.delete(String(o.value)); else chosen.add(String(o.value)); } else { const was = on; chosen.clear(); if (!was) chosen.add(String(o.value)); }
          draw();
          emit();
        } }, o.label, o.count !== undefined && o.count !== null ? h('span', { class: 'n' }, String(o.count)) : null);
      }));
  };
  draw();
  return root;
}
