// title: Map
// description: A street map (OpenStreetMap) with pins: drag to move, buttons, double-click, the wheel or a pinch to zoom, a label on each pin, and clicks on the map or a pin reported back. It fits itself to the pins unless told where to look.
// props: pins: [{lat, lng, label, color, id}]; center: {lat, lng} and zoom (1-19) to look somewhere (default: fit the pins, else the world);
//   height in px (default 320); selected: a pin id whose label shows; onPin(pin) when a pin is clicked; onClick({lat, lng}) when the map is clicked;
//   onMove({center, zoom}) after the person moves or zooms; interactive (default true); maxZoom (default 18)
function component(props) {
  css('sa-map', `
.sa-map{position:relative;overflow:hidden;border-radius:12px;border:1px solid var(--line);background:#e5e3df;touch-action:none;user-select:none;cursor:grab}
.sa-map.drag{cursor:grabbing}
.sa-map .tiles img{position:absolute;width:256px;height:256px;max-width:none;pointer-events:none}
.sa-map .pin{position:absolute;width:26px;height:26px;margin:-26px 0 0 -13px;border-radius:50% 50% 50% 0;transform:rotate(-45deg);border:2px solid #fff;box-shadow:0 2px 6px rgba(0,0,0,.35);cursor:pointer;padding:0}
.sa-map .pin::after{content:'';position:absolute;inset:7px;border-radius:50%;background:#fff}
.sa-map .tip{position:absolute;transform:translate(-50%,calc(-100% - 34px));background:var(--card);color:var(--fg);border-radius:8px;padding:5px 9px;font-size:12.5px;box-shadow:0 4px 14px rgba(0,0,0,.2);white-space:nowrap;max-width:240px;overflow:hidden;text-overflow:ellipsis;pointer-events:none}
.sa-map .zoom{position:absolute;right:8px;top:8px;display:flex;flex-direction:column;gap:4px}
.sa-map .zoom button{width:32px;height:32px;padding:0;font-size:18px;line-height:1;background:var(--card)}
.sa-map .attr{position:absolute;right:0;bottom:0;background:rgba(255,255,255,.8);color:#333;font-size:10.5px;padding:1px 6px;border-top-left-radius:6px}
.sa-map .attr a{color:#333}
`);
  const geo = lib.geo;
  const maxZoom = Math.min(19, Number(props.maxZoom) || 18);
  const pins = (props.pins || []).filter((p) => p && Number.isFinite(Number(p.lat)) && Number.isFinite(Number(p.lng)));
  const interactive = props.interactive !== false;
  let center = props.center ? { lat: Number(props.center.lat), lng: Number(props.center.lng) } : null;
  let zoom = Number.isFinite(Number(props.zoom)) ? Math.round(Number(props.zoom)) : null;
  let selected = props.selected ?? null;
  const tiles = h('div', { class: 'tiles' });
  const layer = h('div', { class: 'pins' });
  const zoomBtns = interactive ? h('div', { class: 'zoom' },
    h('button', { type: 'button', 'aria-label': 'Zoom in', onclick: (e) => { e.stopPropagation(); setZoom(zoom + 1); } }, '+'),
    h('button', { type: 'button', 'aria-label': 'Zoom out', onclick: (e) => { e.stopPropagation(); setZoom(zoom - 1); } }, '−')) : null;
  const root = h('div', { class: 'sa-map', style: { height: `${Number(props.height) || 320}px` } }, tiles, layer, zoomBtns,
    h('div', { class: 'attr' }, '© ', h('a', { href: 'https://www.openstreetmap.org/copyright' }, 'OpenStreetMap'), ' contributors'));
  if (!interactive) root.style.cursor = 'default';
  const size = () => ({ w: root.clientWidth || 600, h: root.clientHeight || Number(props.height) || 320 });
  const kept = new Map();
  function topLeft() { const c = geo.project(center, zoom); const s = size(); return { x: c.x - s.w / 2, y: c.y - s.h / 2 }; }
  function draw() {
    if (!center || zoom === null) {
      const s = size();
      const f = pins.length ? geo.fit(geo.bounds(pins), s.w, s.h, { maxZoom: Math.min(16, maxZoom) }) : { center: { lat: 25, lng: 0 }, zoom: 1 };
      if (!center) center = f.center;
      if (zoom === null) zoom = pins.length === 1 ? 15 : f.zoom;
    }
    zoom = Math.max(1, Math.min(maxZoom, zoom));
    const s = size(); const tl = topLeft(); const n = 2 ** zoom;
    const want = new Set();
    for (let tx = Math.floor(tl.x / 256); tx <= Math.floor((tl.x + s.w) / 256); tx++) {
      for (let ty = Math.floor(tl.y / 256); ty <= Math.floor((tl.y + s.h) / 256); ty++) {
        if (ty < 0 || ty >= n) continue;
        const wx = ((tx % n) + n) % n;
        const key = `${zoom}/${tx}/${ty}`;
        want.add(key);
        let img = kept.get(key);
        if (!img) {
          img = h('img', { alt: '', draggable: 'false', src: app.tiles.replace('{z}', zoom).replace('{x}', wx).replace('{y}', ty) });
          kept.set(key, img);
          tiles.append(img);
        }
        img.style.left = `${Math.round(tx * 256 - tl.x)}px`;
        img.style.top = `${Math.round(ty * 256 - tl.y)}px`;
      }
    }
    for (const [key, img] of kept) if (!want.has(key)) { img.remove(); kept.delete(key); }
    layer.replaceChildren(...pins.map((p) => {
      const pt = geo.project(p, zoom);
      const x = pt.x - tl.x; const y = pt.y - tl.y;
      if (x < -40 || y < -40 || x > s.w + 40 || y > s.h + 60) return null;
      const el = h('button', { type: 'button', class: 'pin', title: p.label || '', 'aria-label': p.label || 'Pin', style: { left: `${x}px`, top: `${y}px`, background: p.color || 'var(--accent)' },
        onpointerdown: (e) => e.stopPropagation(),
        onclick: (e) => { e.stopPropagation(); selected = p.id ?? p; draw(); if (props.onPin) props.onPin(p); } });
      const on = selected !== null && (selected === p || (p.id !== undefined && selected === p.id));
      return on && p.label ? [el, h('div', { class: 'tip', style: { left: `${x}px`, top: `${y}px` } }, p.label)] : el;
    }).flat());
  }
  const moved = () => { if (props.onMove) props.onMove({ center: { ...center }, zoom }); };
  function setZoom(z, at) {
    const nz = Math.max(1, Math.min(maxZoom, z));
    if (nz === zoom) return;
    if (at) {
      const s = size(); const tl = topLeft();
      const ll = geo.unproject({ x: tl.x + at.x, y: tl.y + at.y }, zoom);
      const p = geo.project(ll, nz);
      center = geo.unproject({ x: p.x - at.x + s.w / 2, y: p.y - at.y + s.h / 2 }, nz);
    }
    zoom = nz;
    draw();
    moved();
  }
  if (interactive) {
    const pointers = new Map();
    let start = null; let pinch = null; let dragged = false;
    root.addEventListener('pointerdown', (e) => {
      if (e.target.closest('.zoom, .attr')) return;
      pointers.set(e.pointerId, { x: e.clientX, y: e.clientY });
      root.setPointerCapture(e.pointerId);
      if (pointers.size === 1) { start = { x: e.clientX, y: e.clientY, c: geo.project(center, zoom) }; dragged = false; }
      if (pointers.size === 2) { const [a, b] = [...pointers.values()]; pinch = { d: Math.hypot(a.x - b.x, a.y - b.y), z: zoom }; }
    });
    root.addEventListener('pointermove', (e) => {
      if (!pointers.has(e.pointerId)) return;
      pointers.set(e.pointerId, { x: e.clientX, y: e.clientY });
      if (pinch && pointers.size === 2) {
        const [a, b] = [...pointers.values()];
        const r = Math.hypot(a.x - b.x, a.y - b.y) / pinch.d;
        const z = pinch.z + Math.round(Math.log2(r));
        if (z !== zoom) setZoom(z);
        dragged = true;
        return;
      }
      if (!start) return;
      const dx = e.clientX - start.x; const dy = e.clientY - start.y;
      if (!dragged && Math.abs(dx) + Math.abs(dy) < 4) return;
      dragged = true;
      root.classList.add('drag');
      center = geo.unproject({ x: start.c.x - dx, y: start.c.y - dy }, zoom);
      draw();
    });
    const up = (e) => {
      if (!pointers.has(e.pointerId)) return;
      pointers.delete(e.pointerId);
      if (pointers.size < 2) pinch = null;
      if (pointers.size) return;
      root.classList.remove('drag');
      if (dragged) moved();
      else if (start && props.onClick && !e.target.closest('.pin')) {
        const r = root.getBoundingClientRect(); const tl = topLeft();
        props.onClick(geo.unproject({ x: tl.x + e.clientX - r.left, y: tl.y + e.clientY - r.top }, zoom));
      }
      start = null;
    };
    root.addEventListener('pointerup', up);
    root.addEventListener('pointercancel', up);
    root.addEventListener('dblclick', (e) => { const r = root.getBoundingClientRect(); setZoom(zoom + 1, { x: e.clientX - r.left, y: e.clientY - r.top }); });
    let wheelAt = 0;
    root.addEventListener('wheel', (e) => {
      e.preventDefault();
      if (Date.now() - wheelAt < 250) return;
      wheelAt = Date.now();
      const r = root.getBoundingClientRect();
      setZoom(zoom + (e.deltaY < 0 ? 1 : -1), { x: e.clientX - r.left, y: e.clientY - r.top });
    }, { passive: false });
  }
  new ResizeObserver(() => draw()).observe(root);
  return root;
}
