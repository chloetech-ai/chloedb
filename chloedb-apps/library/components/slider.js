// title: Slider
// description: A range slider with its value shown beside the label, for picking an amount by dragging or with the arrow keys (volume, a budget limit, how a workout felt, minutes of practice), with optional marked points under the track.
// props: value: number (default min; kept between min and max and snapped to step); min: number (default 0); max: number (default 100);
//   step: number above 0 (default 1); onChange(value) while the person drags or presses keys; onCommit(value) once they let go (or after a key or a mark);
//   format(value) -> text: how the value reads, beside the label and on the marks (default the plain number); label: text above the track (optional; also its accessible name, default "Value");
//   marks: [numbers] to show as labelled ticks under the track (outside min..max are left out); tapping one moves the slider there
function component(props) {
  css('sa-slider', `
.sa-slider{--t:18px;display:flex;flex-direction:column;gap:6px;width:100%;min-width:0}
.sa-slider .sa-head{display:flex;align-items:baseline;gap:8px;justify-content:space-between}
.sa-slider .sa-head label{margin:0;font-weight:500}
.sa-slider output{font-variant-numeric:tabular-nums;font-weight:600;margin-left:auto;white-space:nowrap}
.sa-slider input[type=range]{-webkit-appearance:none;appearance:none;width:100%;height:6px;margin:8px 0;padding:0;border:0;border-radius:3px;box-shadow:none;cursor:pointer;
  background:linear-gradient(to right,var(--accent) 0,var(--accent) calc(var(--t)/2 + (100% - var(--t))*var(--p)),var(--line) calc(var(--t)/2 + (100% - var(--t))*var(--p)),var(--line) 100%)}
.sa-slider input[type=range]:disabled{cursor:default;opacity:.6}
.sa-slider input[type=range]:focus{outline:none}
.sa-slider input[type=range]::-webkit-slider-thumb{-webkit-appearance:none;appearance:none;width:var(--t);height:var(--t);border-radius:50%;background:var(--card);border:2px solid var(--accent);box-shadow:0 1px 3px rgba(0,0,0,.25)}
.sa-slider input[type=range]::-moz-range-thumb{width:var(--t);height:var(--t);box-sizing:border-box;border-radius:50%;background:var(--card);border:2px solid var(--accent);box-shadow:0 1px 3px rgba(0,0,0,.25)}
.sa-slider input[type=range]::-moz-range-track{background:transparent;border:0}
.sa-slider input[type=range]:focus-visible::-webkit-slider-thumb{outline:2px solid var(--accent);outline-offset:2px}
.sa-slider input[type=range]:focus-visible::-moz-range-thumb{outline:2px solid var(--accent);outline-offset:2px}
.sa-slider .sa-marks{position:relative;height:1.6em;margin-top:-2px}
.sa-slider .sa-marks button{position:absolute;top:0;transform:translateX(-50%);border:0;background:none;box-shadow:none;padding:2px 2px 0;margin:0;min-width:0;
  font:inherit;font-size:.8em;line-height:1.2;color:var(--muted);cursor:pointer;white-space:nowrap;display:flex;flex-direction:column;align-items:center}
.sa-slider .sa-marks button::before{content:'';width:1px;height:5px;background:var(--line);margin-bottom:2px}
.sa-slider .sa-marks button.on{color:var(--fg);font-weight:600}
.sa-slider .sa-marks button:first-child:not(:only-child){transform:translateX(calc(-50% + .4em))}
.sa-slider .sa-marks button:last-child:not(:only-child){transform:translateX(calc(-50% - .4em))}
`);

  const num = (x, d) => { const n = Number(x); return x === '' || x === null || x === undefined || !isFinite(n) ? d : n; };
  let min = num(props.min, 0);
  let max = num(props.max, 100);
  if (max < min) [min, max] = [max, min];
  const step = num(props.step, 1) > 0 ? num(props.step, 1) : 1;
  const places = (n) => { const s = String(n); const i = s.indexOf('.'); return s.includes('e-') ? Number(s.split('e-')[1]) : i < 0 ? 0 : s.length - i - 1; };
  const dec = Math.min(10, Math.max(places(step), places(min)));
  const snap = (v) => {
    v = num(v, min);
    v = Math.min(max, Math.max(min, v));
    v = min + Math.round((v - min) / step) * step;
    if (v > max + 1e-9) v -= step;
    return Number(v.toFixed(dec));
  };
  const fmt = (v) => {
    if (typeof props.format === 'function') { try { const t = props.format(v); return t === null || t === undefined ? '' : String(t); } catch (e) { return String(v); } }
    return String(v);
  };
  const span = max - min;
  const pct = (v) => (span > 0 ? (v - min) / span : 0);

  let value = snap(props.value);
  const id = 'sa-sl-' + Math.random().toString(36).slice(2, 9);
  const name = props.label ? String(props.label) : 'Value';

  const out = h('output', { for: id, 'aria-hidden': 'true' }, fmt(value));
  const input = h('input', {
    type: 'range', id, min: String(min), max: String(max), step: String(step), value: String(value),
    disabled: span <= 0 ? true : null,
    'aria-label': props.label ? null : name,
    'aria-valuetext': fmt(value),
    oninput: () => set(input.value, false),
    onchange: () => set(input.value, true),
  });

  const markList = Array.isArray(props.marks)
    ? [...new Set(props.marks.map(Number).filter((m) => isFinite(m) && m >= min && m <= max))].sort((a, b) => a - b)
    : [];
  const markEls = markList.map((m) => h('button', {
    type: 'button', tabindex: '-1', 'aria-hidden': 'true',
    style: `left:calc(var(--t)/2 + (100% - var(--t))*${pct(m)})`,
    onclick: () => { if (!input.disabled) { set(m, true); input.focus(); } },
  }, fmt(m)));

  const root = h('div', { class: 'sa-slider' },
    h('div', { class: 'sa-head' }, props.label ? h('label', { for: id }, name) : null, out),
    input,
    markEls.length ? h('div', { class: 'sa-marks' }, markEls) : null);

  let committed = value;
  function paint() {
    const text = fmt(value);
    root.style.setProperty('--p', String(pct(value)));
    out.textContent = text;
    input.setAttribute('aria-valuetext', text);
    markEls.forEach((el, i) => el.classList.toggle('on', markList[i] === value));
  }
  function set(raw, commit) {
    const v = snap(raw);
    if (String(v) !== input.value) input.value = String(v);
    const moved = v !== value;
    value = v;
    paint();
    if (moved && typeof props.onChange === 'function') props.onChange(v);
    if (commit && v !== committed) {
      committed = v;
      if (typeof props.onCommit === 'function') props.onCommit(v);
    }
  }

  paint();
  return root;
}
