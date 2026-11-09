// title: Settings
// description: The app's settings as named values with descriptions, each changed in place (text, number, on/off or a choice), saved at once and reset to its default, with a form to add a setting.
// tables: settings(name keyword, value text, label text, kind keyword, choices text, description text)
async function render(root, app) {
  // A row is one setting. Other views read it with
  //   app.sql('SELECT value FROM settings WHERE name = :n LIMIT 1', {n: 'theme'})
  // value is text: numbers as '12.5', on/off as 'true' or 'false', a choice as the option's value.
  // kind: 'text' (default), 'number', 'bool' (on/off) or 'choice'.
  // choices: the options of a choice, separated by commas or new lines ('light, dark, system'),
  // or JSON {"options": [...], "default": ...}; for other kinds JSON {"default": ...} gives the default.
  // Without one, the default is the first option of a choice, off for on/off, and empty otherwise.

  css('settings-view', `
    .settings-view .setting-row { align-items: flex-start; }
    .settings-view .setting-text { min-width: 180px; gap: 2px; }
    .settings-view .setting-control { flex: 0 1 260px; max-width: 100%; display: flex; justify-content: flex-end; }
    .settings-view .setting-control input, .settings-view .setting-control select { width: 100%; }
    .settings-view .setting-foot { margin-top: 6px; align-items: center; min-height: 0; }
    .settings-view .setting-foot:empty { display: none; }
    .settings-view .status.ok { color: var(--ok); }
    .settings-view .status.bad { color: var(--bad); }
    .settings-view details > summary { cursor: pointer; font-weight: 600; }
  `);

  const kindOf = (k) => {
    k = String(k || '').trim().toLowerCase();
    if (['number', 'int', 'integer', 'float', 'decimal'].includes(k)) return 'number';
    if (['bool', 'boolean', 'toggle', 'switch', 'onoff', 'on/off', 'yesno'].includes(k)) return 'bool';
    if (['choice', 'select', 'enum', 'options'].includes(k)) return 'choice';
    return 'text';
  };
  const TRUE = ['true', '1', 'yes', 'on', 'y'];
  const FALSE = ['false', '0', 'no', 'off', 'n'];
  const truthy = (v) => TRUE.includes(String(v ?? '').trim().toLowerCase());

  function parseChoices(text) {
    const t = String(text ?? '').trim();
    let options = [], def;
    if (t.startsWith('{') || t.startsWith('[')) {
      try {
        const o = JSON.parse(t);
        const list = Array.isArray(o) ? o : Array.isArray(o.options) ? o.options : [];
        options = list.map((x) => x && typeof x === 'object'
          ? { value: String(x.value ?? x.label ?? ''), label: String(x.label ?? x.value ?? '') }
          : { value: String(x), label: String(x) });
        if (!Array.isArray(o) && o.default != null) def = String(o.default);
        return { options: options.filter((x) => x.value !== ''), def };
      } catch (err) { /* not JSON: read as a plain list */ }
    }
    options = t.split(/[\n,]/).map((x) => x.trim()).filter(Boolean).map((x) => ({ value: x, label: x }));
    return { options, def };
  }

  const humanize = (name) => lib.text.capitalize(String(name || 'Setting').replace(/[_.-]+/g, ' ').trim());

  function spec(row) {
    const kind = kindOf(row.kind);
    const { options, def } = parseChoices(row.choices);
    let d = def;
    if (kind === 'bool') d = d == null ? 'false' : (truthy(d) ? 'true' : 'false');
    if (kind === 'choice' && d == null) d = options.length ? options[0].value : null;
    if (kind !== 'choice' && kind !== 'bool' && d == null) d = null;
    return { kind, options: kind === 'choice' ? options : [], def: d, label: (row.label || '').trim() || humanize(row.name) };
  }
  const current = (row, s) => (row.value != null ? row.value : s.def);
  function norm(kind, v) {
    if (kind === 'bool') return truthy(v) ? 'true' : 'false';
    if (v == null) return '';
    if (kind === 'number' && String(v).trim() !== '' && isFinite(Number(v))) return String(Number(v));
    return String(v);
  }
  const isDefault = (row, s) => norm(s.kind, current(row, s)) === norm(s.kind, s.def);
  function shownValue(s, v) {
    if (s.kind === 'bool') return truthy(v) ? 'on' : 'off';
    if (s.kind === 'choice') { const o = s.options.find((x) => x.value === v); return o ? o.label : String(v); }
    return v === '' || v == null ? 'empty' : String(v);
  }
  const sig = (r) => [r.name, r.label, r.kind, r.choices, r.description].join('\u0001');

  const entries = new Map();

  function makeEntry(row) {
    const head = h('div', { class: 'col grow setting-text' });
    const ctrlBox = h('div', { class: 'setting-control' });
    const foot = h('div', { class: 'row small setting-foot' });
    const el = h('div', { class: 'item' }, h('div', { class: 'row setting-row' }, head, ctrlBox), foot);
    const status = h('span', { class: 'status small', 'aria-live': 'polite' });
    let timer = null, footKey = null;
    const e = { el, row, shown: null };

    e.setStatus = (text, tone) => {
      clearTimeout(timer);
      status.textContent = text || '';
      status.className = 'status small' + (tone ? ' ' + tone : '');
      if (tone === 'ok') timer = setTimeout(() => { status.textContent = ''; }, 1500);
    };
    const focusIn = () => ctrlBox.contains(document.activeElement);

    async function save(value) {
      e.setStatus('Saving…');
      try {
        const stored = await app.update('settings', e.row.id, { value });
        e.row = stored;
        const s = spec(stored);
        e.shown = norm(s.kind, current(stored, s));
        paintFoot();
        e.setStatus('Saved', 'ok');
        return stored;
      } catch (err) {
        e.setStatus('Could not save', 'bad');
        app.toast('Could not save: ' + (err && err.message ? err.message : err));
        throw err;
      }
    }

    function paintHead() {
      const s = spec(e.row);
      head.replaceChildren(
        h('strong', {}, s.label),
        e.row.description ? h('small', { class: 'muted' }, e.row.description) : null,
        e.row.name ? h('small', { class: 'muted mono' }, e.row.name) : null,
      );
    }

    function paintControl() {
      const s = spec(e.row);
      const cur = current(e.row, s);
      e.shown = norm(s.kind, cur);
      let c;
      if (s.kind === 'bool') {
        c = ui.toggleSwitch({ checked: truthy(cur), onChange: (on) => save(on ? 'true' : 'false') });
      } else if (s.kind === 'choice') {
        const opts = s.options.slice();
        if (cur != null && !opts.some((o) => o.value === cur)) opts.unshift({ value: cur, label: cur });
        if (!opts.length) {
          c = h('small', { class: 'muted' }, 'No options defined');
        } else if (opts.length <= 4 && opts.every((o) => o.label.length <= 12)) {
          c = ui.segmentedControl({ options: opts, value: cur, size: 'small', label: s.label, onChange: (v) => save(v).catch(() => {}) });
        } else {
          c = h('select', { 'aria-label': s.label, onchange: () => save(c.value).catch(() => {}) },
            opts.map((o) => h('option', { value: o.value }, o.label)));
          c.value = cur ?? '';
        }
      } else if (s.kind === 'number') {
        c = h('input', {
          type: 'text', inputmode: 'decimal', value: cur ?? '', 'aria-label': s.label,
          placeholder: s.def != null ? 'Default ' + s.def : '',
          onchange: () => {
            const t = c.value.trim();
            if (t === '') {
              save(null).then((r) => { c.value = current(r, s) ?? ''; }).catch(() => {});
              return;
            }
            const n = lib.numbers.parse(t);
            if (n == null || !isFinite(n)) { e.setStatus('Enter a number', 'bad'); return; }
            c.value = String(n);
            save(String(n)).catch(() => {});
          },
        });
      } else {
        c = h('input', {
          type: 'text', value: cur ?? '', 'aria-label': s.label,
          placeholder: s.def ? 'Default ' + s.def : '',
          onchange: () => save(c.value).catch(() => {}),
        });
      }
      ctrlBox.replaceChildren(c);
    }

    function paintFoot() {
      const s = spec(e.row);
      const same = isDefault(e.row, s);
      const key = same + '|' + s.def + '|' + s.kind + '|' + e.row.choices;
      if (key === footKey) return;
      footKey = key;
      const reset = h('button', {
        class: 'ghost small', type: 'button',
        onclick: async () => {
          reset.disabled = true;
          try { await save(s.def); paintControl(); } catch (err) { reset.disabled = false; }
        },
      }, 'Reset to default');
      foot.replaceChildren(
        same ? null : h('span', { class: 'badge info' }, 'Changed'),
        !same && s.def != null ? h('span', { class: 'muted' }, 'Default: ' + shownValue(s, s.def)) : null,
        same ? null : reset,
        status,
        h('span', { class: 'right' }, ui.confirmButton({
          label: 'Remove', small: true,
          onConfirm: async () => {
            try { await app.remove('settings', e.row.id); app.toast('Setting removed'); }
            catch (err) { app.toast('Could not remove: ' + (err && err.message ? err.message : err)); }
          },
        })),
      );
    }

    e.update = (r) => {
      const changedShape = sig(r) !== sig(e.row);
      e.row = r;
      const s = spec(r);
      if (changedShape) { paintHead(); if (!focusIn()) paintControl(); }
      else if (norm(s.kind, current(r, s)) !== e.shown && !focusIn()) paintControl();
      paintFoot();
    };

    paintHead(); paintControl(); paintFoot();
    return e;
  }

  // Page
  let q = '';
  const list = h('div', { class: 'list' });
  const noMatch = h('p', { class: 'empty', style: 'display:none' }, 'No settings match your search.');
  const emptyBox = h('div', { style: 'display:none' });
  const search = h('div', { class: 'toolbar', style: 'display:none' },
    h('div', { class: 'grow' }, ui.searchBox({ placeholder: 'Search settings', onSearch: (t) => { q = t; applyFilter(); if (t) app.track('search', t); } })));

  const formBox = h('div', { class: 'section' });
  const adder = h('details', { class: 'card' }, h('summary', {}, 'Add a setting'), formBox);

  emptyBox.append(ui.emptyState({
    icon: '⚙️', title: 'No settings yet',
    text: 'Settings are named values the app’s screens read, like a theme or a daily goal. Add one below.',
    action: { label: 'Add a setting', onClick: () => { adder.open = true; adder.scrollIntoView({ behavior: 'smooth', block: 'start' }); } },
  }));

  formBox.append(ui.form({
    columns: 2,
    resetOnSubmit: true,
    submitLabel: 'Add setting',
    values: { kind: 'text' },
    fields: [
      { name: 'name', label: 'Name', required: true, placeholder: 'daily_goal', help: 'How other screens find it: letters, digits, _ . or -' },
      { name: 'label', label: 'Label', placeholder: 'Daily goal' },
      { name: 'kind', label: 'Type', type: 'select', options: [
        { value: 'text', label: 'Text' }, { value: 'number', label: 'Number' },
        { value: 'bool', label: 'On/off' }, { value: 'choice', label: 'Choice' }] },
      { name: 'default', label: 'Default', placeholder: 'Optional', help: 'For on/off: on or off' },
      { name: 'choices', label: 'Options', placeholder: 'light, dark, system', help: 'For a choice: the options, separated by commas' },
      { name: 'description', label: 'Description', type: 'textarea', placeholder: 'What it changes' },
    ],
    onSubmit: async (v) => {
      const name = String(v.name || '').trim();
      if (!/^[A-Za-z][A-Za-z0-9_.-]*$/.test(name)) throw new Error('Start the name with a letter and use only letters, digits, _ . or -.');
      const dup = await app.sql('SELECT id FROM settings WHERE name = :n LIMIT 1', { n: name });
      if (dup.length) throw new Error('There is already a setting called ' + name + '.');
      const kind = kindOf(v.kind);
      const opts = [...new Set(String(v.choices || '').split(/[\n,]/).map((x) => x.trim()).filter(Boolean))];
      if (kind === 'choice' && opts.length < 2) throw new Error('Give a choice at least two options, separated by commas.');
      let def = String(v.default ?? '').trim();
      if (def !== '') {
        if (kind === 'number') {
          const n = lib.numbers.parse(def);
          if (n == null || !isFinite(n)) throw new Error('The default of a number must be a number.');
          def = String(n);
        } else if (kind === 'bool') {
          const t = def.toLowerCase();
          if (!TRUE.includes(t) && !FALSE.includes(t)) throw new Error('The default of an on/off setting must be on or off.');
          def = TRUE.includes(t) ? 'true' : 'false';
        } else if (kind === 'choice' && !opts.includes(def)) {
          throw new Error('The default must be one of the options.');
        }
      }
      let choices = null;
      if (kind === 'choice') choices = def && def !== opts[0] ? JSON.stringify({ options: opts, default: def }) : opts.join(', ');
      else if (def !== '') choices = JSON.stringify({ default: def });
      const value = def !== '' ? def : kind === 'choice' ? opts[0] : kind === 'bool' ? 'false' : null;
      await app.insert('settings', {
        name, kind, choices, value,
        label: String(v.label || '').trim() || null,
        description: String(v.description || '').trim() || null,
      });
      app.toast('Setting added');
    },
  }));

  root.append(h('div', { class: 'settings-view col' },
    h('h1', {}, 'Settings'),
    search, emptyBox, list, noMatch,
    adder,
  ));

  function applyFilter() {
    const words = lib.text.normalize(q || '').split(/\s+/).filter(Boolean);
    let shown = 0;
    for (const e of entries.values()) {
      const s = spec(e.row);
      const hay = lib.text.normalize([s.label, e.row.name, e.row.description].filter(Boolean).join(' '));
      const ok = words.every((w) => hay.includes(w));
      e.el.style.display = ok ? '' : 'none';
      if (ok) shown++;
    }
    noMatch.style.display = entries.size && !shown ? '' : 'none';
  }

  async function refresh() {
    let rows;
    try {
      rows = await app.sql('SELECT id, name, value, label, kind, choices, description, created FROM settings ORDER BY created ASC, name ASC LIMIT 500');
    } catch (err) {
      app.toast('Could not load settings');
      return;
    }
    const seen = new Set();
    for (const r of rows) {
      const e = entries.get(r.id);
      if (e) e.update(r); else entries.set(r.id, makeEntry(r));
      seen.add(r.id);
    }
    for (const [id, e] of entries) if (!seen.has(id)) { e.el.remove(); entries.delete(id); }
    const want = rows.map((r) => entries.get(r.id).el);
    const have = [...list.children];
    if (want.length !== have.length || want.some((el, i) => el !== have[i])) list.append(want);
    list.style.display = rows.length ? '' : 'none';
    emptyBox.style.display = rows.length ? 'none' : '';
    search.style.display = rows.length > 8 ? '' : 'none';
    applyFilter();
  }

  await refresh();
  app.onChange(['settings'], refresh);
}
