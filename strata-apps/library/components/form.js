// title: Form
// description: A form built from a list of fields (text, long text, number, money, date, time, choice, yes/no, email, link, tags), with labels, help, required fields checked before sending, and the error shown when saving fails.
// props: fields: [{name, label?, type?: 'text' | 'textarea' | 'number' | 'money' | 'date' | 'time' | 'datetime' | 'select' | 'checkbox' | 'email' | 'url' | 'tags',
//   options? (for select: [values] or [{value, label}]), required?, placeholder?, help?, min?, max?, step?, suggestions? (for tags)}];
//   values: the starting values by name; onSubmit(values) (may return a promise; a thrown error is shown); submitLabel (default "Save");
//   onCancel() to show a Cancel button; resetOnSubmit: true to clear after a save (default false); columns: 1 or 2 (default 1)
function component(props) {
  css('sa-form', `
.sa-form{display:flex;flex-direction:column;gap:12px;align-items:stretch}
.sa-form-grid{display:grid;grid-template-columns:1fr;gap:12px}
.sa-form-grid.two{grid-template-columns:repeat(auto-fit,minmax(220px,1fr))}
.sa-form-f{display:flex;flex-direction:column;gap:4px;min-width:0}
.sa-form-f.wide{grid-column:1/-1}
.sa-form-f > span{font-size:13px;color:var(--muted)}
.sa-form-f > span b{color:var(--bad);font-weight:600}
.sa-form-f input:not([type=checkbox]),.sa-form-f select,.sa-form-f textarea{width:100%}
.sa-form-f.check{flex-direction:row;align-items:center;gap:8px}
.sa-form-f.check > span{color:var(--fg);font-size:14.5px}
.sa-form-help{font-size:12px;color:var(--muted)}
.sa-form-f.bad input,.sa-form-f.bad select,.sa-form-f.bad textarea{border-color:var(--bad)}
.sa-form-err{color:var(--bad);font-size:13px}
.sa-form-bar{display:flex;gap:8px;align-items:center}
`);
  const fields = (props.fields || []).filter((f) => f && f.name);
  const start = props.values || {};
  const inputs = new Map();
  const errs = new Map();
  const toLocalDT = (v) => { if (!v) return ''; const d = new Date(v); if (Number.isNaN(d.getTime())) return ''; const p = (n) => String(n).padStart(2, '0'); return `${d.getFullYear()}-${p(d.getMonth() + 1)}-${p(d.getDate())}T${p(d.getHours())}:${p(d.getMinutes())}`; };
  const make = (f) => {
    const v = start[f.name];
    const common = { name: f.name, required: !!f.required, placeholder: f.placeholder || null, 'aria-label': f.label || f.name };
    switch (f.type) {
      case 'textarea': return h('textarea', { ...common, value: v ?? '' });
      case 'number': return h('input', { ...common, type: 'number', value: v ?? '', min: f.min ?? null, max: f.max ?? null, step: f.step ?? 'any' });
      case 'money': return h('input', { ...common, inputmode: 'decimal', value: v === null || v === undefined || v === '' ? '' : String(v) });
      case 'date': return h('input', { ...common, type: 'date', value: v ? String(v).slice(0, 10) : '', min: f.min ?? null, max: f.max ?? null });
      case 'time': return h('input', { ...common, type: 'time', value: v ?? '' });
      case 'datetime': return h('input', { ...common, type: 'datetime-local', value: toLocalDT(v) });
      case 'email': return h('input', { ...common, type: 'email', value: v ?? '' });
      case 'url': return h('input', { ...common, type: 'url', value: v ?? '' });
      case 'checkbox': return h('input', { type: 'checkbox', name: f.name, checked: !!v });
      case 'select': {
        const opts = (f.options || []).map((o) => (typeof o === 'object' ? o : { value: o, label: String(o) }));
        return h('select', common, f.required ? null : h('option', { value: '' }, '—'), opts.map((o) => h('option', { value: String(o.value), selected: String(o.value) === String(v ?? '') }, o.label ?? String(o.value))));
      }
      case 'tags': {
        let tags = Array.isArray(v) ? v.slice() : v ? String(v).split(',').map((t) => t.trim()).filter(Boolean) : [];
        const box = h('div');
        const draw = () => box.replaceChildren(ui.tagInput({ value: tags, suggestions: f.suggestions || [], placeholder: f.placeholder, onChange: (t) => { tags = t; draw(); } }));
        draw();
        box.read = () => tags;
        return box;
      }
      default: return h('input', { ...common, type: 'text', value: v ?? '' });
    }
  };
  const read = (f) => {
    const el = inputs.get(f.name);
    if (f.type === 'tags') return el.read();
    if (f.type === 'checkbox') return el.checked;
    const raw = el.value;
    if (raw === '') return null;
    if (f.type === 'number') return Number(raw);
    if (f.type === 'money') return lib.money.parse(raw);
    if (f.type === 'datetime') return new Date(raw).toISOString();
    return raw;
  };
  const check = (f, v) => {
    if (f.required && (v === null || v === '' || (Array.isArray(v) && !v.length))) return 'Required';
    if (f.type === 'money' && inputs.get(f.name).value.trim() && v === null) return 'Not an amount';
    if (f.type === 'number' && v !== null && !Number.isFinite(v)) return 'Not a number';
    if (f.type === 'email' && v && !/^[^\s@]+@[^\s@]+\.[^\s@]+$/.test(v)) return 'Not an email address';
    if (f.type === 'url' && v && !/^https?:\/\/\S+$/.test(v)) return 'Starts with http:// or https://';
    return null;
  };
  const grid = h('div', { class: `sa-form-grid${props.columns === 2 ? ' two' : ''}` });
  for (const f of fields) {
    const el = make(f);
    inputs.set(f.name, el);
    const err = h('span', { class: 'sa-form-err', hidden: true });
    errs.set(f.name, err);
    const label = f.label || f.name.replace(/_/g, ' ').replace(/^./, (m) => m.toUpperCase());
    grid.append(f.type === 'checkbox'
      ? h('label', { class: 'sa-form-f check wide' }, el, h('span', null, label))
      : h('label', { class: `sa-form-f${f.type === 'textarea' || f.type === 'tags' ? ' wide' : ''}` }, h('span', null, label, f.required ? h('b', null, ' *') : null), el,
        f.help ? h('span', { class: 'sa-form-help' }, f.help) : null, err));
  }
  const fail = h('div', { class: 'sa-form-err', hidden: true });
  const save = h('button', { class: 'primary', type: 'submit' }, props.submitLabel || 'Save');
  const form = h('form', { class: 'sa-form', novalidate: true }, grid, fail,
    h('div', { class: 'sa-form-bar' }, save, props.onCancel ? h('button', { type: 'button', onclick: () => props.onCancel() }, 'Cancel') : null));
  form.addEventListener('submit', async (e) => {
    e.preventDefault();
    const values = {};
    let bad = false;
    for (const f of fields) {
      const v = read(f);
      const msg = check(f, v);
      const err = errs.get(f.name);
      err.hidden = !msg;
      err.textContent = msg || '';
      err.closest('.sa-form-f')?.classList.toggle('bad', !!msg);
      if (msg) bad = true;
      values[f.name] = v;
    }
    if (bad) return;
    fail.hidden = true;
    save.disabled = true;
    try {
      if (props.onSubmit) await props.onSubmit(values);
      if (props.resetOnSubmit) for (const f of fields) { const el = inputs.get(f.name); if (f.type === 'checkbox') el.checked = false; else if (f.type !== 'tags') el.value = ''; }
    } catch (err) {
      fail.textContent = err && err.message ? err.message : String(err);
      fail.hidden = false;
    } finally { save.disabled = false; }
  });
  return form;
}
