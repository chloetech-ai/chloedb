// title: Drawing pad
// description: Draw or sign with a finger, pen or mouse: colours and pen sizes, undo and clear, saved as a PNG picture among the app's files. In signature mode it is a single dark pen over a "sign here" line.
// props: onSave(file) with the stored picture {id, name, type, size, url} (app.upload); saveLabel (default 'Save');
//   mode: 'draw' (default) or 'sign'; height in px (default 240, 160 to sign); colors: pen colours (default six); background: a colour to fill behind
//   (default none for drawings, white for signatures); name: the file name (default drawing.png or signature.png); onChange(empty) as strokes are added or cleared
function component(props) {
  css('sa-draw', `
.sa-draw{display:flex;flex-direction:column;gap:8px}
.sa-draw .pad{position:relative;border:1px solid var(--line);border-radius:12px;overflow:hidden;background:var(--card);touch-action:none}
.sa-draw canvas{display:block;width:100%;cursor:crosshair}
.sa-draw .line{position:absolute;left:24px;right:24px;bottom:34px;border-bottom:1.5px solid #9aa1ab;pointer-events:none}
.sa-draw .here{position:absolute;left:24px;bottom:12px;font-size:12px;color:#7a828d;pointer-events:none}
.sa-draw .tools{display:flex;gap:6px;align-items:center;flex-wrap:wrap}
.sa-draw .sw{width:24px;height:24px;border-radius:50%;padding:0;border:2px solid transparent;box-shadow:0 0 0 1px var(--line)}
.sa-draw .sw.on{border-color:var(--card);box-shadow:0 0 0 2px var(--accent)}
`);
  const sign = props.mode === 'sign';
  const colors = sign ? ['#111827'] : props.colors || ['#111827', '#e03131', '#1971c2', '#2f9e44', '#f08c00', '#ffffff'];
  let color = colors[0];
  let width = sign ? 2.2 : 3;
  const bg = props.background || (sign ? '#ffffff' : null);
  const H = Number(props.height) || (sign ? 160 : 240);
  const strokes = [];
  const canvas = h('canvas', { height: H });
  const ctx = canvas.getContext('2d');
  const pad = h('div', { class: 'pad', style: bg ? { background: bg } : null }, canvas, sign ? h('div', { class: 'line' }) : null, sign ? h('div', { class: 'here' }, 'Sign above the line') : null);
  const ratio = () => window.devicePixelRatio || 1;
  function size() {
    const w = pad.clientWidth || 600;
    canvas.width = Math.round(w * ratio()); canvas.height = Math.round(H * ratio());
    canvas.style.height = `${H}px`;
    redraw();
  }
  function line(s) {
    ctx.strokeStyle = s.color; ctx.lineWidth = s.width * ratio(); ctx.lineCap = 'round'; ctx.lineJoin = 'round';
    ctx.beginPath();
    s.points.forEach((p, i) => { const x = p[0] * canvas.width; const y = p[1] * canvas.height; if (i) ctx.lineTo(x, y); else ctx.moveTo(x, y); });
    if (s.points.length === 1) ctx.lineTo(s.points[0][0] * canvas.width + 0.1, s.points[0][1] * canvas.height);
    ctx.stroke();
  }
  function redraw() {
    ctx.clearRect(0, 0, canvas.width, canvas.height);
    if (bg) { ctx.fillStyle = bg; ctx.fillRect(0, 0, canvas.width, canvas.height); }
    strokes.forEach(line);
  }
  const changed = () => { save.disabled = !strokes.length; undo.disabled = !strokes.length; if (props.onChange) props.onChange(!strokes.length); };
  let cur = null;
  const at = (e) => { const r = canvas.getBoundingClientRect(); return [(e.clientX - r.left) / r.width, (e.clientY - r.top) / r.height]; };
  canvas.addEventListener('pointerdown', (e) => { canvas.setPointerCapture(e.pointerId); cur = { color, width: width * (e.pointerType === 'pen' && e.pressure ? 0.6 + e.pressure : 1), points: [at(e)] }; strokes.push(cur); line(cur); });
  canvas.addEventListener('pointermove', (e) => { if (!cur) return; cur.points.push(at(e)); redraw(); });
  const end = () => { if (cur) { cur = null; changed(); } };
  canvas.addEventListener('pointerup', end);
  canvas.addEventListener('pointercancel', end);
  const undo = h('button', { type: 'button', class: 'small', disabled: true, onclick: () => { strokes.pop(); redraw(); changed(); } }, 'Undo');
  const clear = h('button', { type: 'button', class: 'small ghost', onclick: () => { strokes.length = 0; redraw(); changed(); } }, 'Clear');
  const msg = h('span', { class: 'small muted' });
  const save = h('button', { type: 'button', class: 'small primary', disabled: true, onclick: async () => {
    save.disabled = true; msg.textContent = 'Saving…';
    try {
      const blob = await new Promise((r) => canvas.toBlob(r, 'image/png'));
      const file = await app.upload(new File([blob], props.name || (sign ? 'signature.png' : 'drawing.png'), { type: 'image/png' }));
      msg.textContent = 'Saved';
      if (props.onSave) props.onSave(file);
    } catch (e) { msg.textContent = e.message; }
    save.disabled = !strokes.length;
  } }, props.saveLabel || 'Save');
  const swatches = sign ? [] : colors.map((c) => h('button', { type: 'button', class: `sw${c === color ? ' on' : ''}`, style: { background: c }, 'aria-label': `Colour ${c}`, onclick: (e) => { color = c; for (const s of e.target.parentNode.querySelectorAll('.sw')) s.classList.toggle('on', s === e.target); } }));
  const sizes = sign ? null : h('select', { 'aria-label': 'Pen size', onchange: (e) => { width = Number(e.target.value); } }, [[2, 'Fine'], [3, 'Medium'], [6, 'Thick'], [12, 'Marker']].map(([v, l]) => h('option', { value: v, selected: v === width }, l)));
  new ResizeObserver(() => size()).observe(pad);
  return h('div', { class: 'sa-draw' }, pad, h('div', { class: 'tools' }, swatches, sizes, h('span', { style: 'flex:1' }), undo, clear, save, msg));
}
