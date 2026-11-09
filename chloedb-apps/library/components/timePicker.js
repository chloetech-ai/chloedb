// title: Time picker
// description: Pick a time of day from a list of times or by typing one ("9", "930", "9:30 pm", "21:30"); for appointments, reminders, opening hours and schedules.
// props: value: 'HH:MM' in 24 h, or '' for none (default ''); onChange(value) with the new 'HH:MM', or '' when cleared;
//   step: minutes between the listed times (default 15); from: the earliest time, 'HH:MM' (default '00:00'); to: the latest time, 'HH:MM' (default '23:59');
//   format: '24h' (default) or '12h', how times read and are typed (the value stays 24 h); clearable: true to allow no time, with a clear button (default false);
//   label: accessible name (default "Time")
function component(props) {
  css('sa-timepicker', `
.sa-timepicker{display:inline-flex;flex-direction:column;width:11em;max-width:100%;vertical-align:top}
.sa-timepicker .box{position:relative;display:flex;align-items:center}
.sa-timepicker input{width:100%;margin:0;padding-right:4.4em;font-variant-numeric:tabular-nums}
.sa-timepicker .btns{position:absolute;right:4px;top:50%;transform:translateY(-50%);display:flex;gap:2px}
.sa-timepicker .btns button{border:0;background:none;box-shadow:none;margin:0;padding:4px 6px;min-width:0;min-height:0;line-height:1;font-size:1em;color:var(--muted);cursor:pointer}
.sa-timepicker .btns button:hover{color:var(--fg)}
.sa-timepicker .btns button[hidden]{display:none}
.sa-timepicker ul{position:absolute;left:0;right:0;top:100%;margin:4px 0 0;padding:4px;list-style:none;max-height:240px;overflow-y:auto;background:var(--card);color:var(--fg);border:1px solid var(--line);border-radius:var(--radius);box-shadow:0 8px 24px rgba(0,0,0,.18);z-index:50}
.sa-timepicker ul[hidden]{display:none}
.sa-timepicker li{padding:6px 10px;border-radius:calc(var(--radius) - 2px);cursor:pointer;white-space:nowrap;font-variant-numeric:tabular-nums}
.sa-timepicker li.active{background:color-mix(in srgb,var(--accent) 15%,transparent)}
.sa-timepicker li[aria-selected="true"]{color:var(--accent);font-weight:600}
.sa-timepicker .msg{font-size:.8em;color:var(--bad);margin-top:3px}
.sa-timepicker .msg:empty{display:none}
`);
  const pad = (n) => String(n).padStart(2, '0');
  const toMin = (t) => {
    const m = /^(\d{1,2}):(\d{2})$/.exec(String(t == null ? '' : t).trim());
    if (!m) return null;
    const hh = +m[1], mm = +m[2];
    return hh < 24 && mm < 60 ? hh * 60 + mm : null;
  };
  const toHM = (n) => pad(Math.floor(n / 60)) + ':' + pad(n % 60);
  const h12 = props.format === '12h';
  const show = (t) => {
    const n = toMin(t);
    if (n == null) return '';
    if (!h12) return toHM(n);
    const hh = Math.floor(n / 60);
    return `${hh % 12 || 12}:${pad(n % 60)} ${hh < 12 ? 'AM' : 'PM'}`;
  };
  // Reads what people type: '9', '09', '930', '0930', '9:30', '9.30', '9h30', '9pm', '9:30 a.m.', '21:30'. Returns minutes, '' for empty, null when unreadable.
  const parse = (text) => {
    const s = String(text || '').toLowerCase().replace(/\s+/g, '').replace(/\./g, (c, i, str) => (/\d/.test(str[i + 1] || '') ? ':' : ''));
    if (!s) return '';
    const m = /^(\d{1,4})(?:[:h](\d{2}))?(am|pm|a|p)?$/.exec(s);
    if (!m) return null;
    let hh, mm;
    if (m[2] != null) { if (m[1].length > 2) return null; hh = +m[1]; mm = +m[2]; }
    else if (m[1].length <= 2) { hh = +m[1]; mm = 0; }
    else { hh = +m[1].slice(0, -2); mm = +m[1].slice(-2); }
    if (mm > 59) return null;
    if (m[3]) {
      if (hh < 1 || hh > 12) return null;
      hh = (hh % 12) + (m[3][0] === 'p' ? 12 : 0);
    } else if (hh > 23) return null;
    return hh * 60 + mm;
  };
  const norm = (s) => String(s).toLowerCase().replace(/[^0-9apm]/g, '');
  const keys = (t) => {
    const n = toMin(t);
    const short = `${Math.floor(n / 60)}${pad(n % 60)}`;
    return h12 ? [norm(show(t))] : [norm(t), short];
  };

  let lo = toMin(props.from); if (lo == null) lo = 0;
  let hi = toMin(props.to); if (hi == null) hi = 1439;
  if (hi < lo) [lo, hi] = [hi, lo];
  const step = Math.max(1, Math.min(1440, Math.round(Number(props.step)) || 15));
  const times = [];
  for (let n = lo; n <= hi; n += step) times.push(toHM(n));
  const label = props.label || 'Time';
  const clearable = !!props.clearable;
  const id = 'sa-tp-' + Math.random().toString(36).slice(2, 9);
  const example = h12 ? '9:30 AM' : '09:30';

  let current = toMin(props.value) != null ? toHM(toMin(props.value)) : '';
  let open = false, shown = [], active = -1, anchor = -1, typed = false;

  const input = h('input', {
    type: 'text', role: 'combobox', autocomplete: 'off', spellcheck: 'false',
    'aria-label': label, 'aria-autocomplete': 'list', 'aria-expanded': 'false', 'aria-controls': id + '-l',
    placeholder: example, value: show(current),
    onclick: () => { if (!open) openList(null); },
    oninput: () => { typed = true; msg.textContent = ''; input.removeAttribute('aria-invalid'); openList(input.value); },
    onkeydown: onKey,
    onblur: () => { if (typed) commit(); else input.value = show(current); typed = false; closeList(); },
  });
  const list = h('ul', { id: id + '-l', role: 'listbox', 'aria-label': label, onmousedown: (e) => e.preventDefault() });
  list.hidden = true;
  const clearBtn = clearable ? h('button', {
    type: 'button', 'aria-label': `Clear ${label.toLowerCase()}`, title: 'Clear',
    onmousedown: (e) => e.preventDefault(),
    onclick: () => { typed = false; msg.textContent = ''; set(''); closeList(); input.focus(); },
  }, '×') : null;
  const toggleBtn = h('button', {
    type: 'button', tabindex: '-1', 'aria-label': 'Show times', title: 'Show times',
    onmousedown: (e) => e.preventDefault(),
    onclick: () => { if (open) closeList(); else { input.focus(); openList(null); } },
  }, '▾');
  const msg = h('div', { class: 'msg', 'aria-live': 'polite' });
  const root = h('div', { class: 'sa-timepicker' },
    h('div', { class: 'box' }, input, h('span', { class: 'btns' }, clearBtn, toggleBtn), list),
    msg);
  syncClear();

  function syncClear() { if (clearBtn) clearBtn.hidden = !current; }

  function set(v) {
    input.value = show(v);
    if (v === current) return;
    current = v;
    syncClear();
    if (typeof props.onChange === 'function') props.onChange(v);
  }

  function nearestIndex(target) {
    if (!shown.length) return -1;
    const i = shown.findIndex((t) => toMin(t) >= target);
    return i < 0 ? shown.length - 1 : i;
  }

  function render(filterText) {
    let pick = times, target = current ? toMin(current) : null, filtered = false;
    if (filterText != null) {
      const q = norm(filterText);
      if (q) {
        const f = times.filter((t) => keys(t).some((k) => k.startsWith(q)));
        if (f.length) { pick = f; filtered = true; }
      }
      const p = parse(filterText);
      if (typeof p === 'number') target = p;
    }
    shown = pick;
    if (target != null) active = nearestIndex(target);
    else active = filtered ? 0 : -1;
    const d = new Date();
    anchor = active >= 0 ? active : nearestIndex(d.getHours() * 60 + d.getMinutes());
    list.replaceChildren(shown.map((t, i) => h('li', {
      id: `${id}-${i}`, role: 'option', 'aria-selected': t === current ? 'true' : 'false',
      onclick: () => choose(t),
    }, show(t))));
    highlight();
  }

  function highlight(center) {
    const items = list.children;
    for (let i = 0; i < items.length; i++) items[i].classList.toggle('active', i === active);
    if (active >= 0) input.setAttribute('aria-activedescendant', `${id}-${active}`);
    else input.removeAttribute('aria-activedescendant');
    const li = items[active >= 0 ? active : anchor];
    if (!li) return;
    if (center) list.scrollTop = li.offsetTop - (list.clientHeight - li.offsetHeight) / 2;
    else if (li.offsetTop < list.scrollTop) list.scrollTop = li.offsetTop;
    else if (li.offsetTop + li.offsetHeight > list.scrollTop + list.clientHeight) list.scrollTop = li.offsetTop + li.offsetHeight - list.clientHeight;
  }

  function openList(filterText) {
    open = true;
    list.hidden = false;
    input.setAttribute('aria-expanded', 'true');
    render(filterText);
    highlight(true);
  }

  function closeList() {
    open = false;
    list.hidden = true;
    input.setAttribute('aria-expanded', 'false');
    input.removeAttribute('aria-activedescendant');
  }

  function fail(text) {
    input.value = show(current);
    input.setAttribute('aria-invalid', 'true');
    msg.textContent = text;
  }

  function commit() {
    const n = parse(input.value);
    if (n === '') {
      if (clearable) set('');
      else input.value = show(current);
      return;
    }
    if (n == null) return fail(`Type a time like ${example}`);
    if (n < lo || n > hi) return fail(`Choose a time from ${show(toHM(lo))} to ${show(toHM(hi))}`);
    msg.textContent = '';
    input.removeAttribute('aria-invalid');
    set(toHM(n));
  }

  function choose(t) {
    typed = false;
    msg.textContent = '';
    input.removeAttribute('aria-invalid');
    set(t);
    closeList();
  }

  function onKey(e) {
    if (e.key === 'ArrowDown' || e.key === 'ArrowUp') {
      e.preventDefault();
      if (!open) { openList(typed ? input.value : null); return; }
      if (!shown.length) return;
      if (active < 0) active = Math.max(0, anchor);
      else active = e.key === 'ArrowDown' ? Math.min(shown.length - 1, active + 1) : Math.max(0, active - 1);
      typed = false;
      highlight();
    } else if (e.key === 'Enter') {
      if (open && active >= 0 && !typed) { e.preventDefault(); choose(shown[active]); }
      else if (open || typed) { e.preventDefault(); commit(); typed = false; closeList(); }
    } else if (e.key === 'Escape') {
      if (open || typed) {
        e.preventDefault();
        closeList();
        typed = false;
        input.value = show(current);
      }
    }
  }

  return root;
}
