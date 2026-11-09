// title: Combobox
// description: A searchable select: type to narrow a list of choices by fuzzy match, move with the arrow keys and pick with Enter or a tap, optionally adding what was typed as a new choice (a category, a project, a contact, a country).
// props: options: [{value, label?, hint?}] or [strings] (label defaults to the value; hint is small muted text beside it, also searched);
//   value: the chosen value (default none); onChange(value) when the person picks another (null when cleared);
//   placeholder (default 'Choose or type to search'); label: accessible name (default the placeholder);
//   allowNew: true to offer adding what was typed when no choice has that label (default false), with onCreate(text) to save it
//   (may return, or resolve to, the new value or {value, label}, which then shows as chosen);
//   clearable: true to show a button that clears the choice (default false)
function component(props) {
  css('sa-combobox', `
.sa-combobox{position:relative;display:block;width:100%;max-width:100%}
.sa-combobox .sa-cb-field{position:relative;display:flex;align-items:center}
.sa-combobox input{width:100%;box-sizing:border-box;padding-right:2.4em;margin:0}
.sa-combobox.clear input{padding-right:4.4em}
.sa-combobox .sa-cb-btn{position:absolute;top:50%;transform:translateY(-50%);border:0;background:none;box-shadow:none;padding:0 .4em;margin:0;min-width:0;height:1.8em;color:var(--muted);cursor:pointer;font-size:1em;line-height:1}
.sa-combobox .sa-cb-btn:hover{color:var(--fg)}
.sa-combobox .sa-cb-toggle{right:.3em}
.sa-combobox .sa-cb-clear{right:2.1em}
.sa-combobox .sa-cb-list{position:absolute;left:0;right:0;top:calc(100% + 4px);z-index:30;margin:0;padding:4px;list-style:none;background:var(--card);color:var(--fg);border:1px solid var(--line);border-radius:var(--radius);box-shadow:0 8px 24px rgba(0,0,0,.18);max-height:260px;overflow-y:auto}
.sa-combobox .sa-cb-list[hidden]{display:none}
.sa-combobox .sa-cb-opt{display:flex;align-items:baseline;gap:.6em;padding:.45em .6em;border-radius:calc(var(--radius) - 2px);cursor:pointer;user-select:none}
.sa-combobox .sa-cb-opt.active{background:color-mix(in srgb,var(--accent) 15%,transparent)}
.sa-combobox .sa-cb-opt .sa-cb-label{flex:1;min-width:0;overflow:hidden;text-overflow:ellipsis;white-space:nowrap}
.sa-combobox .sa-cb-opt.chosen .sa-cb-label{font-weight:600}
.sa-combobox .sa-cb-opt.chosen .sa-cb-label::after{content:' ✓';color:var(--accent)}
.sa-combobox .sa-cb-hint{color:var(--muted);font-size:.85em;white-space:nowrap;overflow:hidden;text-overflow:ellipsis;max-width:45%}
.sa-combobox .sa-cb-new{color:var(--accent)}
.sa-combobox .sa-cb-empty{padding:.5em .6em;color:var(--muted)}
`);
  const LIMIT = 100;
  const norm = (o) => o !== null && typeof o === 'object'
    ? { value: o.value, label: o.label != null ? String(o.label) : String(o.value ?? ''), hint: o.hint != null ? String(o.hint) : '' }
    : { value: o, label: String(o), hint: '' };
  const options = (Array.isArray(props.options) ? props.options : []).filter((o) => o != null).map(norm);
  const same = (a, b) => a != null && b != null && String(a) === String(b);
  const placeholder = props.placeholder || 'Choose or type to search';
  const canCreate = !!props.allowNew && typeof props.onCreate === 'function';
  const clearable = !!props.clearable;
  const uid = 'sa-cb-' + Math.random().toString(36).slice(2, 9);
  const fuzzy = (q, t) => {
    if (!t) return 0;
    if (typeof lib !== 'undefined' && lib.text && lib.text.fuzzy) return lib.text.fuzzy(q, t) || 0;
    return t.toLowerCase().includes(q.toLowerCase()) ? 0.5 : 0;
  };

  let current = props.value ?? null;
  let open = false, typed = false, busy = false, active = -1, items = [];

  const labelOf = (v) => {
    if (v == null) return '';
    const o = options.find((x) => same(x.value, v));
    return o ? o.label : String(v);
  };

  const input = h('input', {
    type: 'text', role: 'combobox', autocomplete: 'off', spellcheck: 'false',
    'aria-autocomplete': 'list', 'aria-expanded': 'false', 'aria-controls': uid + '-list',
    'aria-label': props.label || placeholder, placeholder, value: labelOf(current),
    oninput: () => { typed = true; if (!open) openList(); else { build(); active = items.length ? 0 : -1; draw(); } },
    onkeydown: onKey,
    onclick: () => { if (!open) openList(); },
    onfocus: () => input.select(),
    onblur: () => { if (open) close(true); },
  });
  const list = h('ul', { id: uid + '-list', role: 'listbox', class: 'sa-cb-list', 'aria-label': props.label || placeholder });
  list.hidden = true;
  list.addEventListener('mousedown', (e) => e.preventDefault());

  const clearBtn = clearable ? h('button', {
    type: 'button', class: 'sa-cb-btn sa-cb-clear', tabindex: '-1', 'aria-label': 'Clear', title: 'Clear',
    onmousedown: (e) => e.preventDefault(),
    onclick: () => {
      const had = current != null;
      current = null; input.value = ''; typed = false;
      syncClear();
      if (open) { build(); active = -1; draw(); }
      input.focus();
      if (had && typeof props.onChange === 'function') props.onChange(null);
    },
  }, '×') : null;
  const toggleBtn = h('button', {
    type: 'button', class: 'sa-cb-btn sa-cb-toggle', tabindex: '-1', 'aria-label': 'Show choices',
    onmousedown: (e) => e.preventDefault(),
    onclick: () => { if (open) close(true); else { input.focus(); openList(); } },
  }, '▾');

  const root = h('div', { class: 'sa-combobox' }, h('div', { class: 'sa-cb-field' }, input, clearBtn, toggleBtn), list);

  function syncClear() {
    if (!clearBtn) return;
    const show = current != null || input.value !== '';
    clearBtn.hidden = !show;
    root.classList.toggle('clear', show);
  }

  function build() {
    const q = typed ? input.value.trim() : '';
    if (!q) items = options.slice(0, LIMIT).map((o) => ({ o }));
    else {
      items = options
        .map((o, i) => ({ o, i, s: Math.max(fuzzy(q, o.label), o.hint ? fuzzy(q, o.hint) * 0.6 : 0) }))
        .filter((x) => x.s > 0)
        .sort((a, b) => b.s - a.s || a.i - b.i)
        .slice(0, LIMIT)
        .map((x) => ({ o: x.o }));
      if (canCreate && !options.some((o) => o.label.toLowerCase() === q.toLowerCase())) items.push({ text: q });
    }
  }

  function draw() {
    if (active >= items.length) active = items.length - 1;
    list.replaceChildren();
    if (busy) list.append(h('li', { class: 'sa-cb-empty', role: 'presentation' }, 'Adding…'));
    else if (!items.length) list.append(h('li', { class: 'sa-cb-empty', role: 'presentation' }, options.length ? 'No matches' : 'Nothing to choose yet'));
    items.forEach((it, i) => {
      const chosen = it.o && same(it.o.value, current);
      const cls = ['sa-cb-opt', i === active ? 'active' : '', chosen ? 'chosen' : '', it.text != null ? 'sa-cb-new' : ''].filter(Boolean).join(' ');
      list.append(h('li', {
        id: `${uid}-${i}`, role: 'option', class: cls, 'aria-selected': i === active ? 'true' : 'false',
        onmousemove: () => { if (active !== i) { active = i; mark(); } },
        onclick: () => choose(it),
      }, it.o
        ? [h('span', { class: 'sa-cb-label' }, it.o.label), it.o.hint ? h('span', { class: 'sa-cb-hint' }, it.o.hint) : null]
        : h('span', { class: 'sa-cb-label' }, `Add “${it.text}”`)));
    });
    mark();
  }

  function mark() {
    Array.from(list.children).forEach((el, i) => {
      if (el.getAttribute('role') !== 'option') return;
      el.classList.toggle('active', i === active);
      el.setAttribute('aria-selected', i === active ? 'true' : 'false');
    });
    if (active >= 0) {
      input.setAttribute('aria-activedescendant', `${uid}-${active}`);
      const el = list.children[active];
      if (el && el.scrollIntoView) el.scrollIntoView({ block: 'nearest' });
    } else input.removeAttribute('aria-activedescendant');
  }

  function openList() {
    open = true;
    build();
    const at = items.findIndex((it) => it.o && same(it.o.value, current));
    active = typed ? (items.length ? 0 : -1) : at;
    list.hidden = false;
    input.setAttribute('aria-expanded', 'true');
    draw();
  }

  function close(restore) {
    open = false; typed = false; active = -1;
    list.hidden = true;
    input.setAttribute('aria-expanded', 'false');
    input.removeAttribute('aria-activedescendant');
    if (restore) input.value = labelOf(current);
    syncClear();
  }

  async function choose(it) {
    if (busy || !it) return;
    if (it.o) {
      const prev = current;
      current = it.o.value;
      input.value = it.o.label;
      close(false);
      if (!same(prev, current) && typeof props.onChange === 'function') props.onChange(current);
      return;
    }
    const text = it.text;
    busy = true; draw();
    try {
      const r = await props.onCreate(text);
      if (r != null) {
        const o = norm(typeof r === 'object' ? { label: text, ...r } : { value: r, label: text });
        if (!options.some((x) => same(x.value, o.value))) options.push(o);
        current = o.value;
      }
      busy = false;
      close(false);
      input.value = r != null ? labelOf(current) : text;
      syncClear();
    } catch (e) {
      busy = false;
      if (open) draw();
    }
  }

  function onKey(e) {
    if (e.key === 'ArrowDown' || e.key === 'ArrowUp') {
      e.preventDefault();
      if (!open) { openList(); return; }
      if (!items.length) return;
      const step = e.key === 'ArrowDown' ? 1 : -1;
      active = active < 0 ? (step > 0 ? 0 : items.length - 1) : (active + step + items.length) % items.length;
      mark();
    } else if (e.key === 'Enter') {
      if (!open) return;
      e.preventDefault();
      if (active >= 0) choose(items[active]);
    } else if (e.key === 'Escape') {
      if (!open) return;
      e.preventDefault(); e.stopPropagation();
      close(true);
    } else if (e.key === 'Tab') {
      if (open) close(true);
    }
  }

  syncClear();
  return root;
}
