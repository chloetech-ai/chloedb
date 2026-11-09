// title: Color picker
// description: Choose a colour from a row of swatches or type a hex code (tags, categories, calendars, chart series, avatars); the chosen swatch is ringed and ticked.
// props: value: '#rrggbb' the chosen colour (default none; '#rgb' and missing '#' are read too); onChange(hex) with the new colour as lower-case '#rrggbb';
//   colors: ['#rrggbb', ...] the swatches (default a 16-colour palette that reads on light and dark themes; invalid entries and duplicates are dropped);
//   allowCustom: true (default) to also show a system colour well and a hex box for any other colour (Enter or leaving the box applies it, an invalid code is undone);
//   size: swatch size in px (default 28, 16 to 64); label: accessible name of the swatches (default "Colour")
function component(props) {
  css('sa-colorpicker', `
.sa-colorpicker{display:flex;flex-direction:column;gap:8px;max-width:100%}
.sa-colorpicker .sw{display:flex;flex-wrap:wrap;gap:6px}
.sa-colorpicker .sw button{width:var(--s);height:var(--s);min-width:0;min-height:0;padding:0;margin:0;border-radius:50%;border:1px solid color-mix(in srgb,var(--fg) 15%,transparent);background:var(--c);box-shadow:none;cursor:pointer;display:inline-flex;align-items:center;justify-content:center;font-size:calc(var(--s) * .5);line-height:1;color:var(--t)}
.sa-colorpicker .sw button[aria-checked="true"]{box-shadow:0 0 0 2px var(--bg),0 0 0 4px var(--fg)}
.sa-colorpicker .sw button:focus-visible{outline:2px solid var(--accent);outline-offset:3px}
.sa-colorpicker .custom{display:flex;align-items:center;gap:8px;flex-wrap:wrap}
.sa-colorpicker input[type=color]{width:calc(var(--s) + 8px);height:calc(var(--s) + 4px);padding:2px;border:1px solid var(--line);border-radius:var(--radius);background:var(--card);cursor:pointer}
.sa-colorpicker input.hex{width:7.5em;font-family:ui-monospace,monospace}
.sa-colorpicker input.hex[aria-invalid="true"]{border-color:var(--bad);outline-color:var(--bad)}
`);
  const norm = (s) => {
    let x = String(s == null ? '' : s).trim().replace(/^#/, '');
    if (/^[0-9a-f]{3}$/i.test(x)) x = x.split('').map((c) => c + c).join('');
    return /^[0-9a-f]{6}$/i.test(x) ? '#' + x.toLowerCase() : null;
  };
  const DEFAULT = ['#ef4444', '#f97316', '#f59e0b', '#eab308', '#84cc16', '#22c55e', '#10b981', '#14b8a6',
    '#06b6d4', '#3b82f6', '#6366f1', '#8b5cf6', '#a855f7', '#ec4899', '#f43f5e', '#64748b'];
  const colors = [...new Set((Array.isArray(props.colors) && props.colors.length ? props.colors : DEFAULT).map(norm).filter(Boolean))];
  const allowCustom = props.allowCustom !== false;
  const size = Math.max(16, Math.min(64, Number(props.size) || 28));
  const label = props.label || 'Colour';
  let cur = norm(props.value);

  const swatches = colors.map((c) => h('button', {
    type: 'button', role: 'radio', title: c, 'aria-label': c,
    style: `--c:${c};--t:${lib.color.readable(c)}`,
    onclick: () => set(c, true),
    onkeydown: (e) => {
      const i = colors.indexOf(c);
      const n = colors.length;
      const to = { ArrowRight: i + 1, ArrowDown: i + 1, ArrowLeft: i - 1, ArrowUp: i - 1, Home: 0, End: n - 1 }[e.key];
      if (to === undefined) return;
      e.preventDefault();
      const j = (to + n) % n;
      set(colors[j], true);
      swatches[j].focus();
    },
  }));

  const well = allowCustom ? h('input', {
    type: 'color', 'aria-label': 'Any colour', value: cur || '#000000',
    oninput: (e) => { hex.value = e.target.value; hex.removeAttribute('aria-invalid'); },
    onchange: (e) => set(norm(e.target.value), true),
  }) : null;

  const hex = allowCustom ? h('input', {
    type: 'text', class: 'hex', value: cur || '', placeholder: '#3b82f6', maxlength: 7,
    spellcheck: 'false', autocomplete: 'off', 'aria-label': 'Hex colour code', title: 'A hex code such as #3b82f6',
    oninput: (e) => {
      const ok = !e.target.value.trim() || norm(e.target.value);
      if (ok) e.target.removeAttribute('aria-invalid'); else e.target.setAttribute('aria-invalid', 'true');
    },
    onkeydown: (e) => { if (e.key === 'Enter') { e.preventDefault(); apply(); } },
    onchange: () => apply(),
  }) : null;

  function apply() {
    const v = norm(hex.value);
    if (v) set(v, true);
    else { hex.value = cur || ''; hex.removeAttribute('aria-invalid'); }
  }

  function set(c, fire) {
    if (!c) return;
    const changed = c !== cur;
    cur = c;
    paint();
    if (fire && changed && typeof props.onChange === 'function') props.onChange(c);
  }

  function paint() {
    const at = colors.indexOf(cur);
    swatches.forEach((b, i) => {
      const on = i === at;
      b.setAttribute('aria-checked', on ? 'true' : 'false');
      b.tabIndex = on || (at < 0 && i === 0) ? 0 : -1;
      b.textContent = on ? '✓' : '';
    });
    if (well) well.value = cur || '#000000';
    if (hex && document.activeElement !== hex) { hex.value = cur || ''; hex.removeAttribute('aria-invalid'); }
  }

  const root = h('div', { class: 'sa-colorpicker', style: `--s:${size}px` },
    colors.length ? h('div', { class: 'sw', role: 'radiogroup', 'aria-label': label }, swatches) : null,
    allowCustom ? h('div', { class: 'custom' }, well, hex) : null);
  paint();
  return root;
}
