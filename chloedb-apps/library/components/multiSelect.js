// title: Multi select
// description: Choose several things from a list (tags for a task, people at a meeting, cuisines you like, days to repeat on), shown as removable chips in a box, with a dropdown list to pick from and a search box when the list is long.
// props: options: [strings] or [{value, label? (default the value), color? (css color of the chip's dot)}]; value: [values] chosen (default []);
//   onChange(values) with the new list of values, in the order they were chosen; placeholder: text shown when nothing is chosen (default "Choose…");
//   max: the most that may be chosen (default no limit; once reached, the rest of the list is disabled);
//   searchable: true to type to filter the list (default true when there are more than 8 options); label: accessible name (default the placeholder)
function component(props) {
  css('sa-msel', `
.sa-msel{position:relative;width:100%;max-width:100%}
.sa-msel .box{display:flex;flex-wrap:wrap;align-items:center;gap:4px;min-height:38px;padding:4px 6px;border:1px solid var(--line);border-radius:var(--radius);background:var(--card);cursor:pointer;box-sizing:border-box}
.sa-msel.search .box{cursor:text}
.sa-msel .box:focus-within{border-color:var(--accent)}
.sa-msel .chipsel{display:contents}
.sa-msel .tag{display:inline-flex;align-items:center;gap:5px;max-width:100%;padding:2px 2px 2px 9px;border-radius:999px;background:color-mix(in srgb,var(--c,var(--accent)) 14%,transparent);border:1px solid color-mix(in srgb,var(--c,var(--accent)) 40%,transparent);color:var(--fg);font-size:.875em;line-height:1.5;box-sizing:border-box}
.sa-msel .tag .lbl{overflow:hidden;text-overflow:ellipsis;white-space:nowrap;min-width:0}
.sa-msel .tag button{border:0;background:none;box-shadow:none;padding:0 6px;margin:0;min-height:0;height:auto;line-height:1.2;color:var(--muted);cursor:pointer;font-size:1.1em;border-radius:999px}
.sa-msel .tag button:hover,.sa-msel .tag button:focus-visible{color:var(--fg)}
.sa-msel .dot{width:8px;height:8px;border-radius:50%;background:var(--c);flex:none}
.sa-msel input.q{flex:1;min-width:90px;width:auto;border:0;outline:0;background:none;box-shadow:none;padding:4px;margin:0;color:var(--fg);font:inherit}
.sa-msel .trig{flex:1;display:flex;align-items:center;justify-content:space-between;gap:8px;min-width:48px;border:0;background:none;box-shadow:none;margin:0;padding:4px;color:var(--muted);font:inherit;text-align:left;cursor:pointer;min-height:0}
.sa-msel .caret{flex:none;font-size:.8em;margin-left:auto}
.sa-msel ul{position:absolute;left:0;right:0;top:calc(100% + 4px);z-index:30;margin:0;padding:4px;list-style:none;max-height:240px;overflow:auto;background:var(--card);border:1px solid var(--line);border-radius:var(--radius);box-shadow:0 8px 24px rgba(0,0,0,.18);box-sizing:border-box}
.sa-msel ul[hidden]{display:none}
.sa-msel li{display:flex;align-items:center;gap:8px;padding:7px 8px;border-radius:6px;cursor:pointer;color:var(--fg)}
.sa-msel li.act{background:color-mix(in srgb,var(--accent) 13%,transparent)}
.sa-msel li[aria-disabled="true"]{opacity:.45;cursor:default}
.sa-msel li .check{width:16px;flex:none;text-align:center;color:var(--accent);font-weight:600}
.sa-msel li .lbl{overflow:hidden;text-overflow:ellipsis;white-space:nowrap;min-width:0}
.sa-msel li.none,.sa-msel li.note{cursor:default;color:var(--muted);font-size:.875em}
.sa-msel li.note{font-size:.8em;border-top:1px solid var(--line);border-radius:0;margin-top:4px}
`);

  const key = (v) => String(v);
  const opts = [];
  const byKey = new Map();
  for (const o of Array.isArray(props.options) ? props.options : []) {
    const item = o !== null && typeof o === 'object'
      ? { value: o.value, label: o.label != null && o.label !== '' ? String(o.label) : String(o.value), color: o.color || null }
      : { value: o, label: String(o), color: null };
    if (item.value == null || byKey.has(key(item.value))) continue;
    byKey.set(key(item.value), item);
    opts.push(item);
  }

  let sel = [];
  for (const v of Array.isArray(props.value) ? props.value : []) {
    if (v != null && !sel.some((s) => key(s) === key(v))) sel.push(v);
  }
  const max = Number(props.max) > 0 ? Math.floor(Number(props.max)) : Infinity;
  const searchable = props.searchable != null ? !!props.searchable : opts.length > 8;
  const placeholder = props.placeholder || 'Choose…';
  const uid = 'sa-msel-' + Math.random().toString(36).slice(2, 9);
  const norm = (t) => (lib.text && lib.text.normalize ? lib.text.normalize(t) : String(t).toLowerCase());

  let open = false, query = '', active = -1, vis = opts;
  const isSel = (v) => sel.some((s) => key(s) === key(v));
  const labelOf = (v) => (byKey.get(key(v)) || { label: String(v) }).label;
  const disabledFor = (o) => !isSel(o.value) && sel.length >= max;

  const chipsEl = h('span', { class: 'chipsel' });
  const list = h('ul', { id: uid + '-list', role: 'listbox', 'aria-multiselectable': 'true', 'aria-label': props.label || placeholder, hidden: true });
  const trigger = searchable
    ? h('input', { class: 'q', type: 'text', role: 'combobox', autocomplete: 'off', 'aria-autocomplete': 'list', 'aria-controls': uid + '-list', 'aria-label': props.label || placeholder,
        oninput: (e) => { query = e.target.value; setOpen(true); active = vis.findIndex((o) => !disabledFor(o)); draw(); },
        onkeydown: onKey })
    : h('button', { class: 'trig', type: 'button', 'aria-haspopup': 'listbox', 'aria-controls': uid + '-list', 'aria-label': props.label || placeholder,
        onclick: () => { setOpen(!open); draw(); },
        onkeydown: onKey });
  const box = h('div', { class: 'box', onclick: (e) => {
    if (e.target.closest('.tag') || e.target === trigger) return;
    trigger.focus();
    setOpen(!searchable ? !open : true);
    draw();
  } }, chipsEl, trigger);
  const root = h('div', { class: `sa-msel${searchable ? ' search' : ''}` }, box, list);

  root.addEventListener('focusout', () => {
    setTimeout(() => { if (open && !root.contains(document.activeElement)) { setOpen(false); draw(); } }, 0);
  });
  function onDoc(e) {
    if (!root.isConnected) { document.removeEventListener('pointerdown', onDoc, true); return; }
    if (!root.contains(e.target)) { setOpen(false); draw(); }
  }
  function setOpen(v) {
    if (v === open) return;
    open = v;
    if (open) { active = -1; document.addEventListener('pointerdown', onDoc, true); }
    else { document.removeEventListener('pointerdown', onDoc, true); }
  }

  function emit() { if (typeof props.onChange === 'function') props.onChange(sel.slice()); }
  function toggle(o) {
    if (isSel(o.value)) sel = sel.filter((s) => key(s) !== key(o.value));
    else if (sel.length < max) sel = sel.concat([o.value]);
    else return;
    if (searchable && query) { query = ''; trigger.value = ''; }
    emit();
    draw();
  }
  function removeAt(v) {
    sel = sel.filter((s) => key(s) !== key(v));
    emit();
    draw();
    trigger.focus();
  }
  function move(step) {
    if (!vis.length) return;
    let i = active;
    for (let n = 0; n < vis.length; n++) {
      i = i < 0 ? (step > 0 ? 0 : vis.length - 1) : (i + step + vis.length) % vis.length;
      if (!disabledFor(vis[i])) { active = i; return; }
    }
  }

  function onKey(e) {
    const k = e.key;
    if (k === 'ArrowDown' || k === 'ArrowUp') {
      e.preventDefault();
      if (!open) setOpen(true);
      else move(k === 'ArrowDown' ? 1 : -1);
      if (active < 0) move(k === 'ArrowDown' ? 1 : -1);
      draw();
    } else if (k === 'Enter' || (k === ' ' && !searchable)) {
      e.preventDefault();
      if (open && active >= 0 && vis[active]) toggle(vis[active]);
      else { setOpen(!open || searchable); draw(); }
    } else if (k === 'Escape') {
      if (open) { e.preventDefault(); e.stopPropagation(); setOpen(false); draw(); }
    } else if (k === 'Backspace' && searchable && !trigger.value && sel.length) {
      e.preventDefault();
      removeAt(sel[sel.length - 1]);
    } else if (k === 'Tab' && open) {
      setOpen(false); draw();
    }
  }

  function draw() {
    const q = norm(query.trim());
    vis = q ? opts.filter((o) => norm(o.label).includes(q)) : opts;
    if (active >= vis.length) active = vis.length - 1;

    chipsEl.replaceChildren(sel.map((v) => {
      const o = byKey.get(key(v));
      const label = labelOf(v);
      return h('span', { class: 'tag', style: o && o.color ? `--c:${o.color}` : null },
        o && o.color ? h('span', { class: 'dot', 'aria-hidden': 'true' }) : null,
        h('span', { class: 'lbl', title: label }, label),
        h('button', { type: 'button', 'aria-label': `Remove ${label}`, onclick: (e) => { e.stopPropagation(); removeAt(v); } }, '×'));
    }));

    if (searchable) trigger.placeholder = sel.length ? '' : placeholder;
    else trigger.replaceChildren(h('span', { class: 'clip' }, sel.length ? '' : placeholder), h('span', { class: 'caret', 'aria-hidden': 'true' }, open ? '▴' : '▾'));
    trigger.setAttribute('aria-expanded', open ? 'true' : 'false');
    if (open && active >= 0) trigger.setAttribute('aria-activedescendant', `${uid}-${active}`);
    else trigger.removeAttribute('aria-activedescendant');

    list.hidden = !open;
    if (!open) return;
    const items = vis.map((o, i) => {
      const on = isSel(o.value), off = disabledFor(o);
      return h('li', { id: `${uid}-${i}`, role: 'option', class: i === active ? 'act' : null,
        'aria-selected': on ? 'true' : 'false', 'aria-disabled': off ? 'true' : null,
        onmousedown: (e) => e.preventDefault(),
        onmousemove: () => { if (active !== i && !off) { active = i; draw(); } },
        onclick: () => { if (!off) { active = i; toggle(o); } } },
        h('span', { class: 'check', 'aria-hidden': 'true' }, on ? '✓' : ''),
        o.color ? h('span', { class: 'dot', style: `--c:${o.color}`, 'aria-hidden': 'true' }) : null,
        h('span', { class: 'lbl', title: o.label }, o.label));
    });
    if (!items.length) items.push(h('li', { class: 'none', role: 'presentation' }, opts.length ? 'No matches' : 'Nothing to choose from'));
    if (max !== Infinity && sel.length >= max && opts.length) items.push(h('li', { class: 'note', role: 'presentation' }, `Up to ${max} can be chosen`));
    list.replaceChildren(items);
    const act = active >= 0 ? list.children[active] : null;
    if (act && act.scrollIntoView) act.scrollIntoView({ block: 'nearest' });
  }

  draw();
  return root;
}
