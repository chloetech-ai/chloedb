// The runtime inside a view's frame. The frame is sandboxed (no same origin,
// no network), so everything a view does to data goes to the studio as a
// message, and the studio calls the engine.
(() => {
  const SA = window.__SA || { params: {} };
  const pending = new Map();
  let seq = 0;
  const post = (msg) => parent.postMessage({ __sa: 1, ...msg }, '*');
  const rpc = (method, args) => new Promise((resolve, reject) => {
    const id = ++seq;
    pending.set(id, { resolve, reject });
    post({ kind: 'rpc', id, method, args });
  });

  const text = (e) => (e && (e.stack || e.message)) || String(e);
  let reported = 0;
  const report = (e) => {
    // A rule's refusal ("Only 2 left") is an answer for the person, not a bug.
    if (e && e.refused) return;
    if (reported++ > 3) return;
    const message = e && e.message ? e.message : String(e);
    console.error(e);
    post({ kind: 'error', message, stack: text(e).slice(0, 2000), cap: (e && e.__cap) || null });
  };
  window.__sa_report = report;
  window.__sa_ready = () => post({ kind: 'ready' });
  window.addEventListener('error', (e) => report(e.error || e.message));
  window.addEventListener('unhandledrejection', (e) => report(e.reason));

  const listeners = [];
  const leaving = [];
  window.__sa_onLeave = (fn) => { leaving.push(fn); };
  window.addEventListener('message', (e) => {
    if (e.source !== parent) return;
    const m = e.data;
    if (!m || !m.__sa) return;
    if (m.kind === 'result') {
      const p = pending.get(m.id);
      if (!p) return;
      pending.delete(m.id);
      if (m.error !== undefined) p.reject(Object.assign(new Error(m.error), m.refused ? { refused: true } : {}));
      else p.resolve(m.value);
    } else if (m.kind === 'design') {
      // A design is a block of CSS variables: swapping it redraws nothing.
      let el = document.getElementById('__design');
      if (!el) { el = document.createElement('style'); el.id = '__design'; document.head.append(el); }
      el.textContent = m.css || '';
    } else if (m.kind === 'leaving') {
      // The person moves to another screen: what this one left undone.
      for (const fn of leaving) { try { fn(); } catch (err) { report(err); } }
    } else if (m.kind === 'change') {
      for (const l of listeners) {
        if (l.tables === '*' || m.tables.some((t) => l.tables.includes(t))) {
          try {
            const r = l.fn(m.tables);
            if (r && typeof r.catch === 'function') r.catch(report);
          } catch (err) { report(err); }
        }
      }
    }
  });

  // A form never navigates the frame: views handle submit themselves.
  document.addEventListener('submit', (e) => e.preventDefault(), true);

  // External links open in a new tab; the frame never navigates away.
  document.addEventListener('click', (e) => {
    const a = e.target.closest && e.target.closest('a[href]');
    if (!a) return;
    const href = a.getAttribute('href');
    if (/^https?:/i.test(href)) { e.preventDefault(); window.open(href, '_blank', 'noopener'); } else if (href === '#' || href.startsWith('#')) { e.preventDefault(); }
  });

  const pad = (n) => String(n).padStart(2, '0');
  // A calendar day ("2026-11-07") is that day where the person is, not UTC
  // midnight (which is the day before anywhere west of UTC).
  const asDate = (v) => (v instanceof Date ? v : typeof v === 'string' && /^\d{4}-\d{2}-\d{2}$/.test(v) ? new Date(`${v}T00:00:00`) : new Date(v));
  // A timestamp at exactly midnight UTC was almost always written as a date.
  const dateOnly = (v) => typeof v === 'string' && /^\d{4}-\d{2}-\d{2}T00:00:00(\.0+)?Z$/.test(v);
  const fmt = {
    ago(v) {
      if (!v) return '';
      const s = (Date.now() - asDate(v).getTime()) / 1000;
      const f = s < 0 ? -s : s;
      const unit = f < 60 ? [Math.round(f), 's'] : f < 3600 ? [Math.round(f / 60), 'min'] : f < 86400 ? [Math.round(f / 3600), 'h'] : f < 86400 * 30 ? [Math.round(f / 86400), 'd'] : [Math.round(f / 86400 / 30), 'mo'];
      if (f < 45) return s < 0 ? 'in a moment' : 'just now';
      return s < 0 ? `in ${unit[0]} ${unit[1]}` : `${unit[0]} ${unit[1]} ago`;
    },
    date(v) { return v ? asDate(v).toLocaleDateString(undefined, { day: 'numeric', month: 'short', year: 'numeric', ...(dateOnly(v) ? { timeZone: 'UTC' } : {}) }) : ''; },
    time(v) { return v ? asDate(v).toLocaleTimeString(undefined, { hour: '2-digit', minute: '2-digit' }) : ''; },
    datetime(v) { return v ? `${fmt.date(v)}, ${fmt.time(v)}` : ''; },
    number(n) { return n === null || n === undefined ? '' : Number(n).toLocaleString(); },
    money(n, currency = 'USD') { return n === null || n === undefined ? '' : Number(n).toLocaleString(undefined, { style: 'currency', currency }); },
  };

  // Files and map tiles come from the engine itself, by URL: the frame's
  // stylesheet policy lets images load from there and nothing else.
  const origin = SA.origin || '';
  const files = `${origin}/api/apps/${SA.app || ''}/files/`;
  const one = (f) => rpc('upload', [f]);
  const app = Object.freeze({
    params: Object.freeze({ ...(SA.params || {}) }),
    view: SA.slug,
    // people: one account for every app on this engine
    me: () => rpc('me', []),
    signIn: (email, password) => rpc('signIn', [typeof email === 'object' && email ? email : { email, password }]),
    signUp: (fields) => rpc('signUp', [fields || {}]),
    signOut: () => rpc('signOut', []),
    people: () => rpc('people', []),
    // files: upload(File | FileList | File[]) -> {id, name, type, size, url}
    upload: (f) => (f && typeof f.length === 'number' && !(f instanceof Blob) ? Promise.all(Array.from(f).map(one)) : one(f)),
    fileUrl: (id, opts) => (!id ? '' : /^(https?:|data:|blob:)/.test(String(id)) ? String(id) : `${files}${encodeURIComponent(id)}${opts && opts.download ? '?download' : ''}`),
    files: (ids) => rpc('files', [[].concat(ids || [])]),
    email: (msg) => rpc('email', [msg || {}]),
    geocode: (q) => rpc('geocode', [String(q || '')]),
    tiles: `${origin}/api/tiles/{z}/{x}/{y}.png`,
    sql: (sql, params) => rpc('sql', [sql, params || {}]),
    insert: (table, row) => rpc('insert', [table, row || {}]),
    update: (table, id, fields) => rpc('update', [table, id, fields || {}]),
    remove: (table, id) => rpc('remove', [table, id]),
    call: (name, input) => rpc('call', [name, input === undefined ? null : input]),
    // track(action, detail, {results, table, ms}): results is how many rows
    // a search or a filter found (0 says it found nothing).
    track: (action, detail, more) => {
      const o = more && typeof more === 'object' ? more : {};
      const n = (x) => (x === undefined || x === null || !Number.isFinite(Number(x)) ? undefined : Number(x));
      post({ kind: 'track', action: String(action), detail: detail === undefined ? '' : String(detail), results: n(o.results), table: o.table === undefined ? undefined : String(o.table), ms: n(o.ms) });
    },
    open: (slug, params) => { post({ kind: 'open', slug, params: params || {} }); },
    onChange: (tables, fn) => {
      if (typeof tables === 'function') { fn = tables; tables = '*'; }
      listeners.push({ tables: tables === '*' ? '*' : [].concat(tables), fn });
    },
    toast: (msg) => {
      const el = document.createElement('div');
      el.className = '__toast';
      el.textContent = String(msg);
      document.body.append(el);
      setTimeout(() => el.remove(), 2400);
    },
    fmt: Object.freeze(fmt),
    today: () => { const d = new Date(); return `${d.getFullYear()}-${pad(d.getMonth() + 1)}-${pad(d.getDate())}`; },
    now: () => new Date().toISOString(),
  });

  // append, prepend, replaceChildren, before, after and replaceWith take
  // arrays and skip null, false and undefined, as h() does: views write
  // `box.replaceChildren(items.map(...))` and `cond ? el : null` freely.
  const flat = (args) => {
    const out = [];
    const add = (c) => {
      if (c === null || c === undefined || c === false || c === true) return;
      if (Array.isArray(c)) c.forEach(add);
      else out.push(c instanceof Node ? c : String(c));
    };
    args.forEach(add);
    return out;
  };
  const patch = (proto, names) => {
    for (const m of names) {
      const orig = proto && proto[m];
      if (typeof orig === 'function') proto[m] = function (...a) { return orig.apply(this, flat(a)); };
    }
  };
  patch(Element.prototype, ['append', 'prepend', 'replaceChildren', 'before', 'after', 'replaceWith']);
  patch(DocumentFragment.prototype, ['append', 'prepend', 'replaceChildren']);
  patch(CharacterData.prototype, ['before', 'after', 'replaceWith']);

  const PROPS = new Set(['value', 'checked', 'selected', 'disabled', 'indeterminate', 'multiple', 'readOnly', 'hidden']);
  function setProps(el, props, isSvg) {
    for (const [k, v] of Object.entries(props)) {
      if (v === undefined || v === null || v === false && !PROPS.has(k)) continue;
      if (k.startsWith('on') && typeof v === 'function') el.addEventListener(k.slice(2).toLowerCase(), v);
      else if (k === 'class' || k === 'className') { if (isSvg) el.setAttribute('class', v); else el.className = Array.isArray(v) ? v.filter(Boolean).join(' ') : v; }
      else if (k === 'style' && typeof v === 'object') Object.assign(el.style, v);
      else if (k === 'html') el.innerHTML = v;
      else if (k === 'dataset' && typeof v === 'object') Object.assign(el.dataset, v);
      else if (!isSvg && PROPS.has(k)) el[k] = v;
      else if (!isSvg && k === 'readonly') el.readOnly = !!v;
      else el.setAttribute(k, v === true ? '' : v);
    }
  }
  function addChildren(el, children) {
    for (const c of children) {
      if (c === null || c === undefined || c === false || c === true) continue;
      if (Array.isArray(c)) addChildren(el, c);
      else if (c instanceof Node) el.append(c);
      else el.append(document.createTextNode(String(c)));
    }
  }
  function make(isSvg) {
    return (tag, props, ...children) => {
      const el = isSvg ? document.createElementNS('http://www.w3.org/2000/svg', tag) : document.createElement(tag);
      if (props && typeof props === 'object' && !(props instanceof Node) && !Array.isArray(props)) setProps(el, props, isSvg);
      else if (props !== undefined) children.unshift(props);
      addChildren(el, children);
      return el;
    };
  }
  window.h = make(false);
  window.svg = make(true);
  window.app = app;

  // Shared components, called as ui.<name>(props). Each one's code is loaded
  // on first use, and an error inside one is reported against the component.
  const ui = {};
  window.__sa_component = (name, id, factory) => {
    let impl = null;
    ui[name] = (props) => {
      try {
        if (!impl) {
          impl = factory();
          if (typeof impl !== 'function') throw new Error(`the component ${name} defines no component(props)`);
        }
        return impl(props || {});
      } catch (e) {
        if (e && typeof e === 'object' && !e.__cap) { try { e.__cap = id; } catch { /* frozen */ } }
        throw e;
      }
    };
  };
  window.ui = ui;

  // Shared helpers, called as lib.<name>.fn(...): plain functions, loaded on
  // first use; an error inside one is reported against the helper.
  const lib = {};
  const tag = (e, id) => { if (e && typeof e === 'object' && !e.__cap) { try { e.__cap = id; } catch { /* frozen */ } } return e; };
  window.__sa_helper = (name, id, factory) => {
    let value;
    Object.defineProperty(lib, name, {
      enumerable: true,
      get() {
        if (value === undefined) {
          let made;
          try { made = factory(); } catch (e) { throw tag(e, id); }
          if (!made || typeof made !== 'object') throw tag(new Error(`the helper ${name} defines no helper() returning its functions`), id);
          value = {};
          for (const [k, f] of Object.entries(made)) {
            value[k] = typeof f === 'function' ? (...a) => { try { return f(...a); } catch (e) { throw tag(e, id); } } : f;
          }
          Object.freeze(value);
        }
        return value;
      },
    });
  };
  window.lib = lib;
  // css(name, text): a component's stylesheet, added once.
  const styled = new Set();
  window.css = (name, text) => {
    if (styled.has(name)) return;
    styled.add(name);
    document.head.append(h('style', null, String(text)));
  };
})();
