// Screens: views written as data. A screen names its data (SQL against the
// app's tables), its state, and its blocks (headings, forms, tables, lists,
// shared components with their props); this runtime fetches the data, draws
// the blocks, re-runs the queries after every write and redraws when the
// state changes. Values that start with "=" are expressions; the studio
// compiles each one into a function before the frame loads (window.__SA_EXPR,
// window.__SA_CODE), so the frame needs no eval.
(() => {
  const isExpr = (v) => v && typeof v === 'object' && !Array.isArray(v) && typeof v.$e === 'number' && Object.keys(v).length === 1;
  const OPT = (o) => (o && typeof o === 'object' ? { value: o.value ?? o.id ?? o.label, label: o.label ?? o.name ?? String(o.value) } : { value: o, label: String(o) });
  const human = (k) => String(k).replace(/_/g, ' ').replace(/^./, (c) => c.toUpperCase());

  window.__sa_screen = async (root, app, spec) => {
    const E = window.__SA_EXPR || [];
    const SRC = window.__SA_EXPR_SRC || [];
    const CODE = window.__SA_CODE || [];
    let state = {};
    let data = {};
    let lets = {};
    let drawing = false;
    let again = false;
    const forms = new Map();
    const reported = new Set();
    // How many rows a search or a filter on state[key] found: the first list
    // that depends on it (a LET over the key, else the data it is a parameter
    // of). Nothing when no list depends on it.
    const exprIds = (v, out = []) => {
      if (isExpr(v)) out.push(v.$e);
      else if (Array.isArray(v)) v.forEach((x) => exprIds(x, out));
      else if (v && typeof v === 'object') Object.values(v).forEach((x) => exprIds(x, out));
      return out;
    };
    const mentions = (v, res) => exprIds(v).some((i) => res.some((re) => re.test(String(SRC[i] || ''))));
    const resultsFor = (key) => {
      const k = String(key).replace(/[^A-Za-z0-9_$]/g, '');
      if (!k) return undefined;
      const byKey = [new RegExp(`state\\.${k}\\b|state\\[['"]${k}['"]\\]`)];
      const dataDeps = Object.entries(spec.data || {}).filter(([, q]) => mentions(q, byKey)).map(([n]) => n);
      const byData = dataDeps.map((n) => new RegExp(`\\b${n.replace(/[^A-Za-z0-9_$]/g, '')}\\b`));
      const direct = Object.entries(spec.let || {}).filter(([, v]) => mentions(v, byKey)).map(([n]) => lets[n]);
      const over = Object.entries(spec.let || {}).filter(([, v]) => byData.length && mentions(v, byData)).map(([n]) => lets[n]);
      const list = [...direct, ...dataDeps.map((n) => data[n]), ...over].find(Array.isArray);
      return list ? list.length : undefined;
    };
    // The data in hand once the queries a change started have run.
    let loading = Promise.resolve();
    const settled = () => loading.then(() => new Promise((r) => setTimeout(r, 0)));
    // A search is recorded once typing stops, as the words someone looked for
    // and how many rows they found.
    const typing = new Map();
    const trackSearch = (key, v) => {
      clearTimeout(typing.get(key));
      typing.set(key, setTimeout(() => { const q = String(v ?? '').trim(); if (q) settled().then(() => app.track('search', q, { results: resultsFor(key) })); }, 1200));
    };
    const trackFilter = (key, value) => settled().then(() => app.track('filter', `${key}=${value}`, { results: resultsFor(key) }));
    // A form typed into and then closed unsaved (Cancel, another screen, or
    // a redraw that no longer shows it) is recorded as given up.
    const abandon = (f) => {
      if (!f || !f.touched) return;
      app.track('abandon', `${f.table} ${f.editing ? 'edit' : 'new'}`, { table: f.table, ms: Date.now() - f.touched });
      f.touched = 0;
    };
    if (window.__sa_onLeave) window.__sa_onLeave(() => { for (const f of forms.values()) if (f.el && f.el.isConnected) abandon(f); });

    const fail = (where, e) => {
      const err = new Error(`${where}: ${e && e.message ? e.message : e}`);
      err.__cap = e && e.__cap;
      return err;
    };
    // The value of v in scope s: expressions evaluated, objects and arrays walked.
    const val = (v, s, where = 'screen') => {
      if (isExpr(v)) {
        const f = E[v.$e];
        if (typeof f !== 'function') throw new Error(`${where}: the expression ${JSON.stringify(SRC[v.$e] || '')} does not parse`);
        try { return f(s); } catch (e) { throw fail(`${where}: ${SRC[v.$e]}`, e); }
      }
      if (Array.isArray(v)) return v.map((x, i) => val(x, s, `${where}[${i}]`));
      if (v && typeof v === 'object') { const o = {}; for (const [k, x] of Object.entries(v)) o[k] = val(x, s, `${where}.${k}`); return o; }
      return v;
    };
    const scope = (extra) => ({ ...data, ...lets, data, state, set, refresh, app, ui, lib, h, svg, fmt: app.fmt, params: app.params, today: app.today(), ...(extra || {}) });

    // A handler: run it, then fetch the data again (writes show at once).
    const run = async (fn, args, where) => {
      try {
        const r = typeof fn === 'function' ? fn(...args) : fn;
        if (r && typeof r.then === 'function') await r;
      } catch (e) { if (!(e && e.refused)) report(fail(where, e)); app.toast(e && e.message ? e.message : String(e)); }
      await refresh();
    };
    const handler = (v, s, where) => {
      const fn = val(v, s, where);
      return typeof fn === 'function' ? (...args) => run(fn, args, where) : null;
    };
    // Callbacks inside a component's props become handlers that refresh after.
    const wrapProps = (props, where) => {
      const out = {};
      for (const [k, v] of Object.entries(props || {})) out[k] = typeof v === 'function' && /^on[A-Z]/.test(k) ? (...args) => run(v, args, `${where}.${k}`) : v;
      return out;
    };
    // A preview (drawn while the model still writes the app) has no tables
    // or components yet: what fails stays quiet.
    const preview = !!spec.preview;
    function report(e) {
      if (preview) return;
      const key = String(e && e.message);
      if (reported.has(key)) return;
      reported.add(key);
      window.__sa_report && window.__sa_report(e);
    }

    async function load() {
      const s = scope();
      const entries = Object.entries(spec.data || {});
      // A preview has no app to ask yet: its data is empty.
      if (preview) { data = Object.fromEntries(entries.map(([name]) => [name, []])); return; }
      const out = await Promise.all(entries.map(async ([name, q]) => {
        const where = `data.${name}`;
        try {
          if (isExpr(q)) return await val(q, s, where);
          const def = typeof q === 'string' ? { sql: q } : q || {};
          // An expression for sql gives the SELECT to run (one that depends on
          // state), or the rows themselves.
          if (isExpr(def.sql)) {
            const sql = await val(def.sql, s, where);
            if (typeof sql !== 'string') return sql;
            return await app.sql(sql, val(def.params || {}, s, `${where}.params`));
          }
          return await app.sql(def.sql, val(def.params || {}, s, `${where}.params`));
        } catch (e) { report(fail(where, e)); return []; }
      }));
      data = Object.fromEntries(entries.map(([name], i) => [name, out[i] ?? []]));
    }
    function compute() {
      lets = {};
      for (const [name, v] of Object.entries(spec.let || {})) {
        try { lets[name] = val(v, scope(), `let.${name}`); } catch (e) { report(e); lets[name] = undefined; }
      }
    }

    function set(changes) {
      const next = typeof changes === 'function' ? changes(state) : changes;
      state = { ...state, ...(next || {}) };
      if (hasParams) refresh(); else { compute(); draw(); }
    }
    async function refresh() { const p = load(); loading = p.catch(() => {}); await p; compute(); draw(); }
    const hasParams = Object.values(spec.data || {}).some((q) => q && typeof q === 'object' && (isExpr(q) || q.params || isExpr(q.sql)));

    // ---- blocks
    const blocks = (list, s, path) => (Array.isArray(list) ? list : list == null ? [] : [list]).map((b, i) => block(b, s, `${path}[${i}]`));
    const T = (v, s, where) => { const x = val(v, s, where); return x == null ? '' : String(x); };

    function block(b, s, path) {
      try {
        if (b == null || b === false) return null;
        if (typeof b === 'string' || isExpr(b)) return h('p', null, T(b, s, path));
        if (b.if !== undefined) return blocks(val(b.if, s, `${path}.if`) ? b.then : b.else, s, `${path}.${val(b.if, s) ? 'then' : 'else'}`);
        // Front-page blocks (web/front.js): a hero, carousels, galleries...
        if (front) { const k = front.kinds.find((x) => b[x] !== undefined); if (k) return front.blocks[k](b, s, path); }
        if (b.h !== undefined) return h(`h${b.level || 2}`, { class: b.class }, T(b.h, s, path));
        if (b.text !== undefined) return h('p', { class: [b.muted ? 'muted' : '', b.small ? 'small' : '', b.class || ''].join(' ').trim() || null }, T(b.text, s, path));
        if (b.ui !== undefined) {
          const name = val(b.ui, s, path);
          if (typeof ui[name] !== 'function') throw new Error(`no shared component ${name}`);
          const el = ui[name](wrapProps(val(b.props || {}, s, `${path}.props`), `${path}.props`));
          return el instanceof Node ? el : h('div', null, el == null ? '' : String(el));
        }
        for (const kind of ['row', 'col', 'grid', 'split']) if (b[kind] !== undefined) return h('div', { class: [kind, b.class].filter(Boolean).join(' ') }, blocks(b[kind], s, `${path}.${kind}`));
        if (b.card !== undefined) return h('div', { class: ['card', b.class].filter(Boolean).join(' ') }, b.title ? h('h3', null, T(b.title, s, path)) : null, h('div', { class: 'col' }, blocks(b.card, s, `${path}.card`)));
        if (b.section !== undefined) return h('section', { class: 'section col' }, b.title ? h('h3', null, T(b.title, s, path)) : null, blocks(b.section, s, `${path}.section`));
        if (b.each !== undefined) return each(b, s, path);
        if (b.kpis !== undefined) return h('div', { class: 'kpis' }, (val(b.kpis, s, path) || []).map((k) => h('div', { class: 'kpi' }, h('div', { class: 'value' }, k.value == null ? '–' : String(k.value)), h('div', { class: 'label' }, k.label), k.hint ? h('div', { class: 'small muted' }, k.hint) : null)));
        if (b.chips !== undefined) return chips(b, s, path);
        if (b.input !== undefined) return input(b, s, path);
        if (b.button !== undefined) return button(b, s, path);
        if (b.form !== undefined) return form(b, s, path);
        if (b.table !== undefined) return table(b, s, path);
        if (b.code !== undefined) return code(b, s, path);
        throw new Error(`unknown block ${JSON.stringify(Object.keys(b))}`);
      } catch (e) {
        if (preview) return h('div', { class: 'card small muted', style: 'border-style: dashed' }, b && b.ui ? `ui.${b.ui}` : 'Being written');
        report(fail(path, e));
        return h('div', { class: 'card small', style: 'border-color: var(--bad); color: var(--bad)' }, `This part of the screen failed: ${e.message}`);
      }
    }

    function each(b, s, path) {
      const list = val(b.each, s, `${path}.each`) || [];
      if (!list.length) return h('div', { class: 'empty' }, T(b.empty ?? 'Nothing here yet.', s, path));
      const cls = b.class || 'list';
      return h('div', { class: cls }, list.map((item, i) => {
        const inner = scope({ ...s, [b.as || 'item']: item, index: i });
        const kids = blocks(b.body, inner, `${path}.body`);
        // An item that is one card already is not boxed again.
        const one = Array.isArray(b.body) && b.body.length === 1 && b.body[0] && typeof b.body[0] === 'object' && (b.body[0].card !== undefined || b.body[0].ui !== undefined);
        if (one && kids.length === 1 && kids[0]) return kids[0];
        return cls === 'list' ? h('div', { class: 'item' }, kids) : cls === 'grid' ? h('div', { class: 'card col' }, kids) : h('div', { class: 'col' }, kids);
      }));
    }

    function chips(b, s, path) {
      const key = b.chips;
      return h('div', { class: 'chips' }, (val(b.options, s, path) || []).map(OPT).map((o) =>
        h('button', { class: `chip${state[key] === o.value ? ' on' : ''}`, type: 'button', onclick: () => { const off = state[key] === o.value && b.toggle; const changed = !off && state[key] !== o.value; set({ [key]: off ? null : o.value }); if (changed) trackFilter(key, o.value); } }, o.label)));
    }

    function input(b, s, path) {
      const key = b.input;
      const type = b.type || 'text';
      const label = b.label ? T(b.label, s, path) : null;
      let el;
      if (type === 'select') {
        el = h('select', { onchange: (e) => { set({ [key]: e.target.value }); if (e.target.value) trackFilter(key, e.target.value); } }, (val(b.options, s, path) || []).map(OPT).map((o) => h('option', { value: o.value, selected: String(state[key] ?? '') === String(o.value) }, o.label)));
      } else if (type === 'checkbox') {
        el = h('input', { type: 'checkbox', checked: !!state[key], onchange: (e) => set({ [key]: e.target.checked }) });
      } else {
        el = h('input', { type: type === 'search' ? 'search' : type, placeholder: b.placeholder ? T(b.placeholder, s, path) : null, value: state[key] ?? '',
          oninput: (e) => { const v = type === 'number' ? (e.target.value === '' ? null : Number(e.target.value)) : e.target.value; set({ [key]: v }); if (type === 'search') trackSearch(key, v); } });
      }
      el.dataset.bind = `${path}`;
      return label ? h('label', { class: 'col', style: 'gap:4px' }, h('span', { class: 'small muted' }, label), el) : el;
    }

    function button(b, s, path) {
      const fn = val(b.do, s, `${path}.do`);
      const btn = h('button', { class: b.class || null, type: 'button', onclick: async () => {
        if (b.confirm && !confirm(T(b.confirm, s, path))) return;
        btn.disabled = true;
        await run(fn, [], `${path}.do`);
        btn.disabled = false;
      } }, T(b.button, s, path));
      return btn;
    }

    // A form keeps its element (and what is typed) across redraws; it is made
    // again only when the row it edits changes.
    function form(b, s, path) {
      const inCol = b.layout === 'col';
      const tableName = b.form;
      const editing = b.edit !== undefined ? val(b.edit, s, `${path}.edit`) : null;
      const sig = `${editing && editing.id ? editing.id : ''}|${JSON.stringify(b.fields)}`;
      const kept = forms.get(path);
      if (kept && kept.sig === sig) { kept.s = s; kept.refresh(s); return kept.el; }
      if (kept) abandon(kept);
      const entry = { sig, s, el: null, table: tableName, editing: !!(editing && editing.id), touched: 0 };
      // A field may be a heading for the fields after it: {"section": "Payment"}.
      const all = (b.fields || []).map((f) => (typeof f === 'string' ? { name: f } : f));
      const fields = all.filter((f) => f && f.name);
      const controls = new Map();
      const valueOf = (f, el) => {
        const t = f.type || 'text';
        if (t === 'checkbox') return el.checked;
        if (t === 'number' || t === 'rating') return el.value === '' ? null : Number(el.value);
        if (t === 'tags') return el.value.split(',').map((x) => x.trim()).filter(Boolean);
        if (t === 'datetime') return el.value ? new Date(el.value).toISOString() : null;
        if (t === 'file') return el.__fileId || (editing ? editing[f.name] : null) || null;
        return el.value === '' ? null : el.value;
      };
      // A select's options can come from data ("=kids.map(k => k.name)"): they
      // are filled again on every draw, since the form itself is kept while
      // its rows load and change (a kid added elsewhere shows up at once).
      const fillSelect = (el, f, s, v) => {
        const opts = (val(f.options, s, path) || []).map(OPT);
        const key = JSON.stringify(opts);
        if (el.__opts === key) return;
        const had = el.__opts !== undefined ? el.value : v;
        el.__opts = key;
        el.replaceChildren(...(f.required ? [] : [h('option', { value: '' }, f.placeholder || '—')]), ...opts.map((o) => h('option', { value: o.value }, o.label)));
        if (had != null && opts.some((o) => String(o.value) === String(had))) el.value = String(had);
      };
      entry.refresh = (s2) => { for (const f of fields) if ((f.type || 'text') === 'select' && isExpr(f.options)) fillSelect(controls.get(f.name), f, s2, null); };
      // A form of more than three fields, or with a long one, is laid out
      // on a grid: every field labelled above it, long ones across the
      // whole row, checkboxes beside their label, the buttons on a row of
      // their own. A short form stays one line ([title] [due] Add).
      // A form in a column (layout "col") is a grid too: it has the room of
      // its card, and narrows to one column where the card is narrow.
      const wide = inCol ? fields.length > 1 || fields.some((f) => f.type === 'textarea' || f.type === 'file') : fields.length > 3 || fields.some((f) => f.type === 'textarea' || f.type === 'file');
      // A control a placeholder cannot name (a date, a select...) gets a
      // label; in a one-line form, then, every field does, so they line up.
      const named = (f) => !!f.label || ['checkbox', 'file', 'rating', 'date', 'time', 'datetime', 'select'].includes(f.type || 'text');
      const labelAll = !inCol && !wide && fields.length > 1 && fields.some(named);
      const make = (f) => {
        const t = f.type || 'text';
        const v = editing ? editing[f.name] : val(f.value, s, `${path}.${f.name}`);
        const ph = f.placeholder ? T(f.placeholder, s, path) : f.label || wide || labelAll ? null : human(f.name);
        let el;
        if (t === 'textarea') el = h('textarea', { name: f.name, placeholder: ph, required: !!f.required, rows: f.rows || 3 }, v ?? '');
        else if (t === 'select') { el = h('select', { name: f.name, required: !!f.required }); fillSelect(el, f, s, v); }
        else if (t === 'checkbox') el = h('input', { type: 'checkbox', name: f.name, checked: !!v });
        else if (t === 'tags') el = h('input', { name: f.name, placeholder: ph || 'tags, comma separated', value: Array.isArray(v) ? v.join(', ') : v ?? '' });
        else if (t === 'rating') el = h('select', { name: f.name }, h('option', { value: '' }, '—'), [1, 2, 3, 4, 5].map((n) => h('option', { value: n, selected: Number(v) === n }, '★'.repeat(n))));
        else if (t === 'file') {
          el = h('input', { type: 'file', name: f.name, accept: f.accept || null });
          el.addEventListener('change', async () => { if (el.files[0]) { const up = await app.upload(el.files[0]); el.__fileId = up.id; } });
        } else {
          const typ = { date: 'date', time: 'time', datetime: 'datetime-local', number: 'number', email: 'email', url: 'url', color: 'color' }[t] || 'text';
          let shown = v ?? '';
          if (t === 'datetime' && v) { const d = new Date(v); shown = new Date(d.getTime() - d.getTimezoneOffset() * 60000).toISOString().slice(0, 16); }
          el = h('input', { type: typ, name: f.name, placeholder: ph, required: !!f.required, value: shown, step: t === 'number' ? (f.step || 'any') : null, min: f.min ?? null, max: f.max ?? null });
        }
        controls.set(f.name, el);
        if (wide) {
          const text = f.label ? T(f.label, s, path) : human(f.name);
          const hint = f.hint ? h('span', { class: 'hint' }, T(f.hint, s, path)) : null;
          if (t === 'checkbox') return h('label', { class: 'check' }, el, h('span', null, text), hint);
          return h('label', { class: t === 'textarea' || t === 'file' || t === 'tags' ? 'span' : null }, h('span', null, text, f.required ? h('span', { class: 'req', 'aria-hidden': 'true' }, ' *') : null), el, hint);
        }
        // In a column a control keeps its own height (a form's controls flex
        // along the row).
        if (inCol && t !== 'checkbox') el.style.flex = 'none';
        if (inCol && t !== 'checkbox') el.style.width = '100%';
        const lab = f.label ? T(f.label, s, path) : labelAll || named(f) ? human(f.name) : null;
        return lab ? h('label', { class: 'col', style: inCol ? 'gap:4px' : 'gap:4px;flex:1 1 140px' }, h('span', { class: 'small muted' }, lab), el) : el;
      };
      const saveBtn = h('button', { class: 'primary', type: 'submit', style: inCol && !wide ? 'align-self:flex-start' : null }, T(editing ? (b.update || 'Save') : (b.submit || 'Add'), s, path));
      const el = h('form', { class: wide ? 'wide' : inCol ? 'col' : null, style: inCol && !wide ? 'align-items:stretch' : null, onsubmit: async (e) => {
        e.preventDefault();
        const cur = entry.s;
        const row = {};
        for (const f of fields) row[f.name] = valueOf(f, controls.get(f.name));
        Object.assign(row, val(b.set || {}, cur, `${path}.set`));
        for (const f of fields) if (f.required && (row[f.name] === null || row[f.name] === '')) { app.toast(`${f.label || human(f.name)} is needed`); return; }
        saveBtn.disabled = true;
        try {
          const stored = editing && editing.id ? await app.update(tableName, editing.id, row) : await app.insert(tableName, row);
          entry.touched = 0;
          if (!editing) { el.reset(); for (const c of controls.values()) c.__fileId = null; }
          forms.delete(path);
          const after = b.after !== undefined ? val(b.after, cur, `${path}.after`) : null;
          await run(typeof after === 'function' ? after : null, [stored], `${path}.after`);
        } catch (err) { app.toast(err.message); report(fail(`${path} (${tableName})`, err)); } finally { saveBtn.disabled = false; }
      } }, wide ? [] : fields.map(make));
      if (wide) {
        // Each section (the fields after a {"section"} heading, or all of
        // them): short fields first, as many columns as make even rows (5
        // fields: 3 and 2, not 4 and 1), then the long ones across, then the
        // checkboxes together; each group keeps the order it was given in.
        const long = (f) => ['textarea', 'file', 'tags'].includes(f.type);
        const groups = [];
        for (const f of all) {
          if (f && f.section !== undefined && !f.name) groups.push({ title: T(f.section, s, path), hint: f.hint ? T(f.hint, s, path) : null, fields: [] });
          else if (f && f.name) { if (!groups.length) groups.push({ title: null, fields: [] }); groups.at(-1).fields.push(f); }
        }
        const most = Math.max(1, ...groups.map((g) => g.fields.filter((f) => !long(f) && f.type !== 'checkbox').length));
        const rows = Math.max(1, Math.ceil(most / 4));
        el.style.setProperty('--cols', String(Math.max(1, Math.ceil(most / rows))));
        for (const g of groups) {
          if (g.title) el.append(h('div', { class: 'part' }, h('span', null, g.title), g.hint ? h('span', { class: 'hint' }, g.hint) : null));
          const short = g.fields.filter((f) => !long(f) && f.type !== 'checkbox');
          el.append(...short.map(make), ...g.fields.filter(long).map(make));
          const checks = g.fields.filter((f) => f.type === 'checkbox');
          if (checks.length) el.append(h('div', { class: 'checks' }, checks.map(make)));
        }
      }
      // Cancel shows while a row is edited, or always on a form that opens
      // and closes ("closes": true, as a pattern's New form does).
      const cancelBtn = (editing || b.closes) && b.cancel !== undefined ? h('button', { class: 'ghost', type: 'button', onclick: () => { abandon(entry); forms.delete(path); const c = val(b.cancel, entry.s, `${path}.cancel`); run(c, [], `${path}.cancel`); } }, 'Cancel') : null;
      if (wide) el.append(h('div', { class: 'actions' }, cancelBtn, saveBtn));
      else el.append(...[saveBtn, cancelBtn].filter(Boolean));
      el.addEventListener('input', () => { if (!entry.touched) entry.touched = Date.now(); });
      entry.el = el;
      forms.set(path, entry);
      return el;
    }

    const FORMAT = {
      date: (v) => app.fmt.date(v), datetime: (v) => app.fmt.datetime(v), time: (v) => app.fmt.time(v), ago: (v) => app.fmt.ago(v),
      money: (v) => app.fmt.money(v), number: (v) => app.fmt.number(v), percent: (v) => (v == null ? '' : `${Math.round(Number(v) * 100)}%`),
      stars: (v) => (v ? '★'.repeat(Math.round(v)) : ''), bool: (v) => (v ? '✓' : ''),
    };
    // A badge's color from what its word means (paid, late, waiting, new...);
    // a column may name them itself: "badge": {"shipped": "ok", "lost": "bad"}.
    const TONES = {
      ok: /^(paid|done|complete[d]?|yes|active|ok|okay|approved|accepted|delivered|ready|confirmed|present|won|live|published|available|resolved|success|passed|good|in stock|healthy|on track|finished|kept|enabled|true|✓|returned|received|fulfilled|signed)$/,
      bad: /^(late|overdue|no|failed|fail|cancell?ed|rejected|out|out of stock|lost|absent|declined|blocked|error|urgent|expired|unpaid|critical|missing|broken|disabled|false|off track|bounced|damaged|needs repair|lost|refunded)$/,
      warn: /^(due|waiting|pending|low|low stock|maybe|partial|partly paid|in progress|doing|preparing|review|in review|tentative|soon|on hold|hold|reorder|draft|stale|warning|busy|sent|fair|worn|out now|rented|on loan)$/,
      info: /^(new|planned|scheduled|todo|to do|open|booked|invited|queued|next|info)$/,
    };
    function toneOf(v, given) {
      const w = String(v).trim().toLowerCase();
      if (given && typeof given === 'object' && given[w] !== undefined) return given[w] || 'plain';
      for (const [tone, re] of Object.entries(TONES)) if (re.test(w)) return tone;
      return 'plain';
    }
    function table(b, s, path) {
      const rows = val(b.table, s, `${path}.table`) || [];
      if (!rows.length) return h('div', { class: 'empty' }, T(b.empty ?? 'Nothing here yet.', s, path));
      const given = b.columns !== undefined ? val(b.columns, s, `${path}.columns`) : null;
      const cols = (Array.isArray(given) && given.length ? given : Object.keys(rows[0]).filter((k) => k !== 'id' && k !== 'created').map((key) => ({ key }))).map((c) => (typeof c === 'string' ? { key: c } : c));
      const fns = cols.map((c, i) => (c.value !== undefined ? val(c.value, s, `${path}.columns[${i}].value`) : null));
      const acts = (b.actions || []).map((a, i) => ({ ...a, fn: val(a.do, s, `${path}.actions[${i}].do`) }));
      const onRow = b.onRow !== undefined ? val(b.onRow, s, `${path}.onRow`) : null;
      const cell = (c, fn, r) => {
        let v = typeof fn === 'function' ? fn(r) : fn !== null ? fn : r[c.key];
        if (v instanceof Node) return v;
        if (c.format && FORMAT[c.format]) v = FORMAT[c.format](v);
        if (Array.isArray(v)) v = v.join(', ');
        return c.badge && v != null && v !== '' ? h('span', { class: `badge ${toneOf(v, c.badge)}` }, String(v)) : v == null ? '' : String(v);
      };
      return h('div', { style: 'overflow-x:auto' }, h('table', null,
        h('thead', null, h('tr', null, cols.map((c) => h('th', null, c.label ?? human(c.key || ''))), acts.length ? h('th') : null)),
        h('tbody', null, rows.map((r) => h('tr', { style: onRow ? 'cursor:pointer' : null, onclick: onRow ? (e) => { if (!e.target.closest('button')) run(onRow, [r], `${path}.onRow`); } : null },
          cols.map((c, i) => h('td', null, cell(c, fns[i], r))),
          acts.length ? h('td', { class: 'nowrap right' }, acts.map((a) => h('button', { class: a.class || 'small ghost', type: 'button', onclick: () => { if (a.confirm && !confirm(T(a.confirm, scope({ ...s, row: r }), path))) return; run(a.fn, [r], `${path}.actions`); } }, T(a.label, s, path)))) : null)))));
    }

    function code(b, s, path) {
      const make = CODE[b.code && b.code.$c];
      if (typeof make !== 'function') throw new Error('the code block does not parse');
      const render = make();
      if (typeof render !== 'function') throw new Error('the code block defines no render(root, ctx)');
      const box = h('div');
      const ctx = { state, set, refresh, data: { ...data, ...lets }, app, ...data, ...lets };
      Promise.resolve().then(() => render(box, ctx)).catch((e) => { report(fail(path, e)); box.replaceChildren(h('div', { class: 'small', style: 'color: var(--bad)' }, `This part of the screen failed: ${e.message}`)); });
      return box;
    }

    // A front page (spec.page "front") is drawn edge to edge, its parts in
    // sections; its blocks work in any screen.
    const front = window.__sa_front ? window.__sa_front({ h, val, T, block, run, app, scope }) : null;
    const isFront = spec.page === 'front' && !!front;
    if (isFront) document.body.classList.add('front');

    // ---- drawing: the whole screen again, keeping focus and what is typed.
    function focusPath() {
      const a = document.activeElement;
      if (!a || a === document.body || !root.contains(a)) return null;
      const idx = [];
      for (let n = a; n && n !== root; n = n.parentNode) idx.unshift([...n.parentNode.children].indexOf(n));
      return { idx, start: a.selectionStart, end: a.selectionEnd, tag: a.tagName };
    }
    function restore(f) {
      if (!f) return;
      let n = root;
      for (const i of f.idx) { n = n && n.children[i]; }
      if (n && n.tagName === f.tag && n !== document.activeElement) {
        n.focus();
        try { if (f.start != null) n.setSelectionRange(f.start, f.end); } catch { /* not a text field */ }
      }
    }
    function draw() {
      if (drawing) { again = true; return; }
      drawing = true;
      try {
        do {
          again = false;
          const f = focusPath();
          if (isFront) {
            root.replaceChildren(h('div', { class: 'screen front' }, front.page(spec.body, scope())));
          } else root.replaceChildren(h('div', { class: 'col screen' }, blocks(spec.body, scope(), 'body')));
          if (front) front.after(root);
          restore(f);
        } while (again);
      } finally { drawing = false; }
      // A form this draw no longer shows was closed.
      for (const [p, f] of forms) if (!f.el || !f.el.isConnected) { abandon(f); forms.delete(p); }
    }

    state = { ...(val(spec.state || {}, scope(), 'state') || {}) };
    await load();
    compute();
    draw();
    app.onChange('*', () => refresh());
  };
})();
