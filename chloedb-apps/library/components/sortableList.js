// title: Sortable list
// description: A list the person puts in order by dragging items with a mouse or a finger, or with the keyboard; use it for priorities, playlist order, steps of a routine or a ranked shortlist.
// props: items: [{id, content: an element or text}] in their current order;
//   onReorder(ids): called with every item's id in the new order after a move is dropped (not called when nothing moved);
//   handle: true to drag only by a grip at the start of each row, leaving the content free for its own buttons and text (default false: the whole row drags,
//   and on touch a press of about 0.3 s picks it up so the page still scrolls); disabled: true to show the list without reordering (default false);
//   label: accessible name of the list (default "Sortable list").
//   Keyboard: focus a row (or its grip), Space or Enter picks it up, Up and Down or Home and End move it, Space or Enter drops it, Escape puts it back.
function component(props) {
  css('sa-sortable', `
.sa-sortable{position:relative}
.sa-sortable-list{list-style:none;margin:0;padding:0;display:flex;flex-direction:column;gap:6px}
.sa-sortable-item{display:flex;align-items:center;gap:8px;margin:0;padding:10px 12px;background:var(--card);border:1px solid var(--line);border-radius:var(--radius);position:relative;min-width:0;transition:transform .15s ease,box-shadow .15s ease;-webkit-touch-callout:none}
.sa-sortable.free:not(.disabled) .sa-sortable-item{cursor:grab;user-select:none;-webkit-user-select:none}
.sa-sortable-body{flex:1;min-width:0;overflow-wrap:anywhere}
.sa-sortable-grip{flex:none;display:inline-flex;align-items:center;justify-content:center;width:28px;height:32px;min-width:0;margin:-4px 0 -4px -6px;padding:0;border:0;border-radius:6px;background:none;box-shadow:none;color:var(--muted);cursor:grab;touch-action:none;user-select:none;-webkit-user-select:none}
.sa-sortable-grip:hover{color:var(--fg);background:none}
.sa-sortable-grip:disabled{opacity:.35;cursor:default}
.sa-sortable-item:focus{outline:none}
.sa-sortable-item:focus-visible,.sa-sortable-grip:focus-visible{outline:2px solid var(--accent);outline-offset:2px}
.sa-sortable-item.drag{z-index:2;transition:box-shadow .15s ease;box-shadow:0 8px 24px rgba(0,0,0,.2);border-color:var(--accent)}
.sa-sortable-item.grabbed{border-color:var(--accent);box-shadow:0 0 0 2px var(--accent)}
.sa-sortable.dragging,.sa-sortable.dragging *{cursor:grabbing!important;user-select:none;-webkit-user-select:none}
.sa-sortable-sr{position:absolute;width:1px;height:1px;margin:-1px;padding:0;overflow:hidden;clip:rect(0 0 0 0);white-space:nowrap;border:0}
`);
  const seen = new Set();
  const items = (Array.isArray(props.items) ? props.items : []).filter((it) => {
    if (!it || it.id == null || seen.has(String(it.id))) return false;
    seen.add(String(it.id));
    return true;
  });
  const handle = !!props.handle;
  const disabled = !!props.disabled;
  const report = (ids) => { if (typeof props.onReorder === 'function') props.onReorder(ids); };
  const uid = 'sa-sort-' + Math.random().toString(36).slice(2, 9);

  let order = items.slice();
  const nodes = new Map();   // item -> li
  const targets = new Map(); // item -> the element that drags and takes keys
  let drag = null;           // pointer drag state
  let kb = null;             // keyboard move state

  const live = h('div', { class: 'sa-sortable-sr', role: 'status', 'aria-live': 'assertive' });
  const help = h('div', { id: uid + '-help', class: 'sa-sortable-sr' },
    'Press Space to pick up, the up and down arrows to move, Space again to drop, Escape to cancel.');
  const list = h('ul', { class: 'sa-sortable-list', role: 'list', 'aria-label': props.label || 'Sortable list' });
  const root = h('div', { class: `sa-sortable ${handle ? 'handled' : 'free'}${disabled ? ' disabled' : ''}` }, list, live, help);

  const say = (text) => { live.textContent = ''; setTimeout(() => { live.textContent = text; }, 30); };
  const nameOf = (it) => {
    const c = it.content;
    const t = c instanceof Node ? c.textContent : c == null ? '' : String(c);
    return (t || '').replace(/\s+/g, ' ').trim().slice(0, 60) || 'Item';
  };
  const ids = () => order.map((it) => it.id);
  const placeAll = () => list.append(...order.map((it) => nodes.get(it)));

  const grip = () => svg('svg', { viewBox: '0 0 10 16', width: 10, height: 16, 'aria-hidden': 'true' },
    [3, 8, 13].flatMap((y) => [svg('circle', { cx: 2.5, cy: y, r: 1.5, fill: 'currentColor' }), svg('circle', { cx: 7.5, cy: y, r: 1.5, fill: 'currentColor' })]));

  for (const item of order) {
    const c = item.content;
    const body = h('div', { class: 'sa-sortable-body' }, c instanceof Node ? c : c == null ? '' : String(c));
    const g = handle ? h('button', {
      type: 'button', class: 'sa-sortable-grip', disabled: disabled || null,
      'aria-label': `Move ${nameOf(item)}`, 'aria-describedby': disabled ? null : uid + '-help',
    }, grip()) : null;
    const li = h('li', { class: 'sa-sortable-item' }, g, body);
    if (!handle && !disabled) {
      li.tabIndex = 0;
      li.setAttribute('aria-roledescription', 'sortable item');
      li.setAttribute('aria-describedby', uid + '-help');
    }
    nodes.set(item, li);
    const target = g || li;
    targets.set(item, target);
    if (disabled) continue;
    target.addEventListener('pointerdown', (e) => down(e, item));
    target.addEventListener('keydown', (e) => key(e, item));
    target.addEventListener('blur', () => setTimeout(() => {
      if (kb && kb.item === item && document.activeElement !== target) dropKey(false);
    }, 0));
    // after a long press picks a row up on touch, keep the page from scrolling
    li.addEventListener('touchmove', (e) => { if (drag && drag.started && e.cancelable) e.preventDefault(); }, { passive: false });
    li.addEventListener('contextmenu', (e) => { if (drag) e.preventDefault(); });
    list.append(li);
  }
  if (disabled) list.append(...order.map((it) => nodes.get(it)));

  // ---- pointer dragging (mouse, pen, touch) ----
  function down(e, item) {
    if (drag || kb || order.length < 2) return;
    if (e.pointerType === 'mouse' && e.button !== 0) return;
    const li = nodes.get(item);
    if (!handle) {
      const hit = e.target.closest && e.target.closest('input,textarea,select,button,a,label,[contenteditable],[data-no-drag]');
      if (hit && li.contains(hit)) return;
    }
    if (e.pointerType !== 'touch') e.preventDefault();
    drag = {
      item, li, pointer: e.pointerId, x0: e.clientX, y0: e.clientY, lastY: e.clientY,
      listTop0: list.getBoundingClientRect().top, started: false,
      wait: e.pointerType === 'touch' && !handle, timer: null, raf: 0,
    };
    if (drag.wait) drag.timer = setTimeout(() => { if (drag && !drag.started) begin(); }, 300);
    window.addEventListener('pointermove', move, { passive: false });
    window.addEventListener('pointerup', up);
    window.addEventListener('pointercancel', cancel);
    window.addEventListener('keydown', escape, true);
  }

  function begin() {
    const top = drag.listTop0;
    drag.rects = order.map((it) => {
      const r = nodes.get(it).getBoundingClientRect();
      return { top: r.top - top, h: r.height };
    });
    const r = drag.rects, d = order.indexOf(drag.item);
    const gap = d + 1 < r.length ? r[d + 1].top - (r[d].top + r[d].h) : r[d].top - (r[d - 1].top + r[d - 1].h);
    drag.from = drag.to = d;
    drag.shift = r[d].h + Math.max(0, gap);
    drag.grabY = drag.y0 - top - r[d].top;
    drag.started = true;
    drag.li.classList.add('drag');
    root.classList.add('dragging');
    if (drag.wait && navigator.vibrate) { try { navigator.vibrate(10); } catch (err) { /* not offered */ } }
    const tick = () => {
      if (!drag || !drag.started) return;
      const y = drag.lastY, H = window.innerHeight, edge = 48;
      const v = y < edge ? -Math.min(14, (edge - y) / 3) : y > H - edge ? Math.min(14, (y - (H - edge)) / 3) : 0;
      if (v) { window.scrollBy(0, v); layout(); }
      drag.raf = requestAnimationFrame(tick);
    };
    drag.raf = requestAnimationFrame(tick);
    layout();
  }

  function layout() {
    const r = drag.rects, d = drag.from, last = r.length - 1;
    const listTop = list.getBoundingClientRect().top;
    let top = drag.lastY - listTop - drag.grabY;
    top = Math.max(r[0].top, Math.min(r[last].top + r[last].h - r[d].h, top));
    drag.li.style.transform = `translateY(${top - r[d].top}px)`;
    const center = top + r[d].h / 2;
    let to = d;
    for (let i = d + 1; i <= last; i++) if (center > r[i].top + r[i].h / 2) to = i;
    for (let i = d - 1; i >= 0; i--) if (center < r[i].top + r[i].h / 2) to = i;
    order.forEach((it, i) => {
      if (i === d) return;
      const off = to > d && i > d && i <= to ? -drag.shift : to < d && i < d && i >= to ? drag.shift : 0;
      nodes.get(it).style.transform = off ? `translateY(${off}px)` : '';
    });
    drag.to = to;
  }

  function move(e) {
    if (!drag || e.pointerId !== drag.pointer) return;
    drag.lastY = e.clientY;
    if (!drag.started) {
      const dist = Math.hypot(e.clientX - drag.x0, e.clientY - drag.y0);
      if (drag.wait) { if (dist > 8) finish(false); return; }
      if (dist < 4) return;
      begin();
    }
    e.preventDefault();
    layout();
  }
  function up(e) { if (drag && e.pointerId === drag.pointer) finish(true); }
  function cancel(e) { if (drag && e.pointerId === drag.pointer) finish(false); }
  function escape(e) { if (drag && e.key === 'Escape') { e.preventDefault(); e.stopPropagation(); finish(false); } }

  function finish(commit) {
    const d = drag;
    drag = null;
    clearTimeout(d.timer);
    cancelAnimationFrame(d.raf);
    window.removeEventListener('pointermove', move);
    window.removeEventListener('pointerup', up);
    window.removeEventListener('pointercancel', cancel);
    window.removeEventListener('keydown', escape, true);
    if (!d.started) return;
    // a drag is not a click on what lies under it
    const stop = (ev) => { ev.preventDefault(); ev.stopPropagation(); };
    window.addEventListener('click', stop, { capture: true, once: true });
    setTimeout(() => window.removeEventListener('click', stop, true), 0);
    root.classList.remove('dragging');
    d.li.classList.remove('drag');
    for (const li of nodes.values()) { li.style.transition = 'none'; li.style.transform = ''; }
    const moved = commit && d.to !== d.from;
    if (moved) {
      order.splice(d.from, 1);
      order.splice(d.to, 0, d.item);
      placeAll();
    }
    void list.offsetHeight;
    for (const li of nodes.values()) li.style.transition = '';
    if (moved) {
      say(`${nameOf(d.item)} moved to position ${d.to + 1} of ${order.length}.`);
      report(ids());
    }
  }

  // ---- keyboard ----
  function key(e, item) {
    if (e.target !== e.currentTarget || drag) return;
    const k = e.key;
    if (!kb) {
      if ((k === ' ' || k === 'Enter') && order.length > 1) {
        e.preventDefault();
        const from = order.indexOf(item);
        kb = { item, from };
        nodes.get(item).classList.add('grabbed');
        targets.get(item).setAttribute('aria-pressed', 'true');
        say(`Picked up ${nameOf(item)}, position ${from + 1} of ${order.length}. Use the arrow keys to move, Space to drop, Escape to cancel.`);
      }
      return;
    }
    if (kb.item !== item) return;
    const at = order.indexOf(item);
    if (k === 'ArrowUp' || k === 'ArrowLeft') { e.preventDefault(); moveKey(at - 1); }
    else if (k === 'ArrowDown' || k === 'ArrowRight') { e.preventDefault(); moveKey(at + 1); }
    else if (k === 'Home') { e.preventDefault(); moveKey(0); }
    else if (k === 'End') { e.preventDefault(); moveKey(order.length - 1); }
    else if (k === ' ' || k === 'Enter') { e.preventDefault(); dropKey(true); }
    else if (k === 'Escape') { e.preventDefault(); e.stopPropagation(); dropKey(false); }
  }

  function moveKey(i) {
    const item = kb.item, at = order.indexOf(item);
    i = Math.max(0, Math.min(order.length - 1, i));
    if (i === at) return;
    order.splice(at, 1);
    order.splice(i, 0, item);
    placeAll();
    targets.get(item).focus();
    say(`${nameOf(item)}, position ${i + 1} of ${order.length}.`);
  }

  function dropKey(commit) {
    const { item, from } = kb;
    kb = null;
    nodes.get(item).classList.remove('grabbed');
    const target = targets.get(item);
    target.removeAttribute('aria-pressed');
    const to = order.indexOf(item);
    if (!commit) {
      if (to !== from) {
        order.splice(to, 1);
        order.splice(from, 0, item);
        const hadFocus = document.activeElement === target;
        placeAll();
        if (hadFocus) target.focus();
      }
      say(`Move cancelled. ${nameOf(item)} is back at position ${from + 1} of ${order.length}.`);
      return;
    }
    say(`${nameOf(item)} dropped at position ${to + 1} of ${order.length}.`);
    if (to !== from) report(ids());
  }

  return root;
}
