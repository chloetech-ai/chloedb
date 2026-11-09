// title: Number stepper
// description: A number with − and + buttons and a box to type it in, kept between a minimum and a maximum (servings in a recipe, guests at a booking, sets and reps, a quantity in a cart).
// props: value: number or null (null shows an empty box; default null); min: the lowest allowed (default none); max: the highest allowed (default none);
//   step: how much − and + change it, also the precision a typed number is rounded to (default 1); onChange(value) with the new number,
//   after a button, the arrow keys, or typing and leaving the box or pressing Enter (an empty or unreadable entry goes back to the last value);
//   unit: text shown after the number ('kg', 'min', 'guests'; optional); label: text shown above it and its accessible name (optional);
//   width: width of the typed box, px or any CSS size (default '4.5em')
function component(props) {
  css('sa-stepper', `
.sa-stepper{display:inline-flex;flex-direction:column;gap:4px;max-width:100%;vertical-align:middle}
.sa-stepper .lbl{font-size:.875em;color:var(--muted)}
.sa-stepper .box{display:inline-flex;align-items:stretch;width:max-content;max-width:100%;border:1px solid var(--line);border-radius:var(--radius);background:var(--card);overflow:hidden}
.sa-stepper .box:focus-within{border-color:var(--accent)}
.sa-stepper button{border:0;border-radius:0;background:none;box-shadow:none;margin:0;padding:0;min-width:40px;min-height:40px;font-size:1.25em;line-height:1;color:var(--fg);cursor:pointer;flex:none}
.sa-stepper button:hover:not(:disabled){background:var(--line)}
.sa-stepper button:disabled{color:var(--muted);opacity:.45;cursor:default}
.sa-stepper .mid{display:flex;align-items:center;justify-content:center;gap:4px;padding:0 6px;min-width:0;border-left:1px solid var(--line);border-right:1px solid var(--line)}
.sa-stepper input{border:0;background:none;box-shadow:none;outline:none;margin:0;padding:0;width:var(--w);min-width:2em;text-align:center;font:inherit;font-variant-numeric:tabular-nums;color:var(--fg)}
.sa-stepper .unit{color:var(--muted);font-size:.875em;white-space:nowrap}
`);
  const num = (v) => (v === null || v === undefined || v === '' ? NaN : Number(v));
  const min = Number.isFinite(num(props.min)) ? num(props.min) : -Infinity;
  let max = Number.isFinite(num(props.max)) ? num(props.max) : Infinity;
  if (max < min) max = min;
  const step = num(props.step) > 0 ? num(props.step) : 1;
  const places = (n) => {
    const s = String(n);
    if (s.includes('e-')) return Number(s.split('e-')[1]) || 0;
    const i = s.indexOf('.');
    return i < 0 ? 0 : s.length - i - 1;
  };
  const dp = Math.min(10, Math.max(places(step), Number.isFinite(min) ? places(min) : 0));
  const clamp = (n) => Math.min(max, Math.max(min, n));
  const norm = (n) => clamp(Number(n.toFixed(dp)));
  const unit = props.unit ? String(props.unit) : '';
  const width = props.width == null ? '4.5em' : typeof props.width === 'number' ? `${props.width}px` : String(props.width);

  let current = Number.isFinite(num(props.value)) ? clamp(num(props.value)) : null;

  const labelId = `sa-stepper-${Math.random().toString(36).slice(2, 9)}`;
  const input = h('input', {
    type: 'text', inputmode: dp > 0 || min < 0 ? 'decimal' : 'numeric', autocomplete: 'off',
    role: 'spinbutton', 'aria-labelledby': props.label ? labelId : null, 'aria-label': props.label ? null : 'Number',
    onblur: () => commit(),
    onkeydown: (e) => {
      if (e.key === 'Enter') { e.preventDefault(); commit(); }
      else if (e.key === 'Escape') { refresh(); }
      else if (e.key === 'ArrowUp') { e.preventDefault(); commit(); stepBy(1); }
      else if (e.key === 'ArrowDown') { e.preventDefault(); commit(); stepBy(-1); }
      else if (e.key === 'Home' && Number.isFinite(min)) { e.preventDefault(); set(min); }
      else if (e.key === 'End' && Number.isFinite(max)) { e.preventDefault(); set(max); }
    },
  });
  const minus = h('button', { type: 'button', 'aria-label': 'Decrease', onclick: () => stepBy(-1) }, '−');
  const plus = h('button', { type: 'button', 'aria-label': 'Increase', onclick: () => stepBy(1) }, '+');

  function refresh() {
    const text = current === null ? '' : String(current);
    input.value = text;
    if (Number.isFinite(min)) input.setAttribute('aria-valuemin', String(min));
    if (Number.isFinite(max)) input.setAttribute('aria-valuemax', String(max));
    if (current === null) { input.removeAttribute('aria-valuenow'); input.removeAttribute('aria-valuetext'); }
    else {
      input.setAttribute('aria-valuenow', text);
      input.setAttribute('aria-valuetext', unit ? `${text} ${unit}` : text);
    }
    minus.disabled = current !== null && current <= min;
    plus.disabled = current !== null && current >= max;
  }

  function set(n) {
    if (n === current) { refresh(); return; }
    current = n;
    refresh();
    if (typeof props.onChange === 'function') props.onChange(n);
  }

  function stepBy(dir) {
    set(current === null ? norm(0) : norm(current + dir * step));
  }

  function commit() {
    let t = input.value.replace(/\s/g, '');
    if (unit && t.toLowerCase().endsWith(unit.toLowerCase().replace(/\s/g, ''))) t = t.slice(0, t.length - unit.replace(/\s/g, '').length);
    if (/^[-+]?\d*,\d+$/.test(t)) t = t.replace(',', '.');
    t = t.replace(/,/g, '').replace(/^−/, '-');
    const n = t === '' || !/^[-+]?(\d+\.?\d*|\.\d+)(e[-+]?\d+)?$/i.test(t) ? NaN : Number(t);
    if (!Number.isFinite(n)) { refresh(); return; }
    set(norm(n));
  }

  refresh();
  return h('div', { class: 'sa-stepper', style: `--w:${width}` },
    props.label ? h('span', { class: 'lbl', id: labelId }, String(props.label)) : null,
    h('div', { class: 'box', role: 'group', 'aria-labelledby': props.label ? labelId : null },
      minus,
      h('span', { class: 'mid' }, input, unit ? h('span', { class: 'unit', 'aria-hidden': 'true' }, unit) : null),
      plus));
}
