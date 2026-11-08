// title: Image viewer
// description: A full-screen lightbox for pictures: previous and next with the arrow keys, buttons or a swipe, click or double-tap to zoom and drag to look around, a caption, a counter and a download link.
// props: items: [{file (an app file id) or src, caption, name}]; index: the one to open on (default 0); onIndex(i) when it moves; onClose();
//   It returns the overlay: put it in the page with document.body.append(ui.imageViewer({...})); its close() closes it.
function component(props) {
  css('sa-viewer', `
.sa-viewer{position:fixed;inset:0;z-index:1100;background:rgba(8,10,14,.92);color:#fff;display:flex;flex-direction:column;user-select:none;touch-action:none}
.sa-viewer .bar{display:flex;align-items:center;gap:10px;padding:10px 14px;font-size:14px}
.sa-viewer .bar .n{opacity:.7}
.sa-viewer .bar a,.sa-viewer .bar button{color:#fff;background:rgba(255,255,255,.12);border:0;box-shadow:none;border-radius:8px;padding:6px 10px;text-decoration:none;font-size:13px;cursor:pointer}
.sa-viewer .stage{flex:1;position:relative;overflow:hidden;display:flex;align-items:center;justify-content:center}
.sa-viewer .stage img{max-width:100%;max-height:100%;object-fit:contain;transition:transform .15s;cursor:zoom-in}
.sa-viewer .stage img.zoomed{cursor:grab;transition:none}
.sa-viewer .nav{position:absolute;top:50%;transform:translateY(-50%);background:rgba(255,255,255,.14);color:#fff;border:0;box-shadow:none;border-radius:50%;width:44px;height:44px;font-size:22px;cursor:pointer}
.sa-viewer .nav.prev{left:12px}.sa-viewer .nav.next{right:12px}
.sa-viewer .cap{text-align:center;padding:10px 16px 16px;font-size:14px;opacity:.9;min-height:20px}
`);
  const items = (props.items || []).filter((x) => x && (x.file || x.src));
  let i = Math.max(0, Math.min(items.length - 1, Number(props.index) || 0));
  let zoom = 1; let tx = 0; let ty = 0;
  const img = h('img', { alt: '' });
  const counter = h('span', { class: 'n' });
  const cap = h('div', { class: 'cap' });
  const dl = h('a', { download: '', target: '_blank' }, 'Download');
  const close = () => { document.removeEventListener('keydown', onKey); root.remove(); if (props.onClose) props.onClose(); };
  const apply = () => { img.style.transform = `translate(${tx}px, ${ty}px) scale(${zoom})`; img.classList.toggle('zoomed', zoom > 1); };
  function show(n) {
    if (!items.length) return;
    i = (n + items.length) % items.length;
    const it = items[i];
    zoom = 1; tx = 0; ty = 0; apply();
    img.src = app.fileUrl(it.file || it.src);
    img.alt = it.caption || it.name || '';
    counter.textContent = items.length > 1 ? `${i + 1} of ${items.length}` : '';
    cap.textContent = it.caption || it.name || '';
    dl.href = it.file ? app.fileUrl(it.file, { download: true }) : app.fileUrl(it.src);
    if (props.onIndex) props.onIndex(i);
  }
  const onKey = (e) => { if (e.key === 'Escape') close(); else if (e.key === 'ArrowLeft') show(i - 1); else if (e.key === 'ArrowRight') show(i + 1); };
  const toggleZoom = (e) => { if (zoom > 1) { zoom = 1; tx = 0; ty = 0; } else { zoom = 2.2; const r = img.getBoundingClientRect(); tx = (r.left + r.width / 2 - e.clientX) * 1.2; ty = (r.top + r.height / 2 - e.clientY) * 1.2; } apply(); };
  let down = null; let moved = false;
  img.addEventListener('pointerdown', (e) => { down = { x: e.clientX, y: e.clientY, tx, ty }; moved = false; img.setPointerCapture(e.pointerId); });
  img.addEventListener('pointermove', (e) => {
    if (!down) return;
    const dx = e.clientX - down.x; const dy = e.clientY - down.y;
    if (Math.abs(dx) + Math.abs(dy) > 6) moved = true;
    if (zoom > 1) { tx = down.tx + dx; ty = down.ty + dy; apply(); }
  });
  img.addEventListener('pointerup', (e) => {
    const dx = down ? e.clientX - down.x : 0;
    down = null;
    if (!moved) return toggleZoom(e);
    if (zoom === 1 && Math.abs(dx) > 50) show(i + (dx < 0 ? 1 : -1));
  });
  const stage = h('div', { class: 'stage', onclick: (e) => { if (e.target === stage) close(); } }, img,
    items.length > 1 ? h('button', { class: 'nav prev', 'aria-label': 'Previous', onclick: () => show(i - 1) }, '‹') : null,
    items.length > 1 ? h('button', { class: 'nav next', 'aria-label': 'Next', onclick: () => show(i + 1) }, '›') : null);
  const root = h('div', { class: 'sa-viewer', role: 'dialog', 'aria-label': 'Pictures' },
    h('div', { class: 'bar' }, counter, h('span', { class: 'grow', style: 'flex:1' }), dl, h('button', { type: 'button', onclick: close, 'aria-label': 'Close' }, 'Close')), stage, cap);
  root.close = close;
  document.addEventListener('keydown', onKey);
  show(i);
  return root;
}
