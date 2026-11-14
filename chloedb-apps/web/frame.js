// The runtime inside a view's frame. The frame is sandboxed (no same origin,
// no network, no popups; images only from the engine and the photo hosts:
// web/policy.js), so everything a view does to data goes to the studio as a
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
  // Whether the screen shows its sign-in (a visitor signed out of a screen
  // kept to signed-in people): the app's own page then shows no tabs.
  let gatedNow = null;
  window.__sa_gated = (on) => { if (gatedNow === !!on) return; gatedNow = !!on; post({ kind: 'gate', on: gatedNow }); };
  // A pick (below) is the builder's click, not a tap.
  let pickingNow = false;
  // Every tap in the app is counted (the studio sends the count with the
  // next write): how many taps it takes to do something is a measure of use.
  document.addEventListener('click', (e) => { if (e.isTrusted && !pickingNow && e.target !== document.documentElement && e.target !== document.body) post({ kind: 'tap' }); }, true);
  window.addEventListener('error', (e) => report(e.error || e.message));
  window.addEventListener('unhandledrejection', (e) => report(e.reason));

  // The design's look (--look: native, plain, bold) as an attribute the stylesheet
  // and the screen renderer read; a screen draws again when it changes.
  const syncLook = () => {
    const look = getComputedStyle(document.documentElement).getPropertyValue('--look').trim() || 'plain';
    if (document.documentElement.dataset.look === look) return;
    document.documentElement.dataset.look = look;
    if (window.__sa_relook) { try { window.__sa_relook(); } catch (err) { report(err); } }
  };
  syncLook();

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
      if (m.error !== undefined) {
        // a rule's refusal ("Sorry, only 6 left") is kept for the button whose tap caused it (screen.js)
        if (m.refused) window.__sa_refusal = m.error;
        p.reject(Object.assign(new Error(m.error), m.refused ? { refused: true } : {}, m.auth ? { auth: true } : {}));
      }
      else p.resolve(m.value);
    } else if (m.kind === 'design') {
      // A design is a block of CSS variables: swapping it redraws nothing.
      let el = document.getElementById('__design');
      if (!el) { el = document.createElement('style'); el.id = '__design'; document.head.append(el); }
      el.textContent = m.css || '';
      syncLook();
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

  // Picking an element (for a builder that edits a screen by clicking it):
  // the page around the frame asks with {kind: 'pick', on}; while on, the
  // frame outlines what is under the pointer, a click is a pick and not a
  // tap, and the pick is posted as {kind: 'picked', el} with what the
  // element is, shows and where it sits in the frame. {kind: 'pick-edit'}
  // lets the words of the picked element be typed in place, posted back as
  // {kind: 'pick-edited', ok, oldText, newText}; the frame always puts the
  // element back as it was, since the screen's statement is what changes.
  (() => {
    let on = false, hover = null, lock = null, editing = null, box = null, lockBox = null;
    const norm = (t) => String(t || '').replace(/\s+/g, ' ').trim();
    const outline = (color) => {
      const d = document.createElement('div');
      d.setAttribute('data-sa-pick', '');
      d.style.cssText = `position:fixed;pointer-events:none;z-index:2147483647;border:2px solid ${color};border-radius:3px;display:none;box-sizing:border-box`;
      document.documentElement.append(d);
      return d;
    };
    const place = (d, el) => {
      if (!d) return;
      if (!el || !el.isConnected) { d.style.display = 'none'; return; }
      const r = el.getBoundingClientRect();
      Object.assign(d.style, { display: 'block', left: `${r.left - 2}px`, top: `${r.top - 2}px`, width: `${r.width + 4}px`, height: `${r.height + 4}px` });
    };
    const pickable = (el) => el && el.nodeType === 1 && !el.closest('[data-sa-pick]') && !['HTML', 'BODY', 'SCRIPT', 'STYLE'].includes(el.tagName);
    const selector = (el) => {
      const parts = [];
      for (let n = el; n && n.nodeType === 1 && n !== document.body && parts.length < 8; n = n.parentElement) {
        if (n.id && /^[A-Za-z][\w-]*$/.test(n.id)) { parts.unshift(`#${n.id}`); break; }
        const sib = n.parentElement ? [...n.parentElement.children].filter((c) => c.tagName === n.tagName) : [];
        parts.unshift(`${n.tagName.toLowerCase()}${sib.length > 1 ? `:nth-of-type(${sib.indexOf(n) + 1})` : ''}`);
      }
      return parts.join(' > ');
    };
    const rectOf = (el) => { const r = el.getBoundingClientRect(); return { x: Math.round(r.left), y: Math.round(r.top), width: Math.round(r.width), height: Math.round(r.height) }; };
    const about = (el) => {
      const leaf = el.children.length === 0 ? norm(el.textContent).slice(0, 4000) : '';
      let occurrences = 0;
      if (leaf) for (const n of document.querySelectorAll(el.tagName)) { if (!n.children.length && norm(n.textContent) === leaf) occurrences++; if (occurrences > 1) break; }
      const cs = getComputedStyle(el);
      const attrs = {};
      for (const k of ['id', 'class', 'href', 'src', 'alt', 'aria-label', 'title', 'placeholder', 'role', 'type', 'name']) { const v = el.getAttribute(k); if (v) attrs[k] = v.slice(0, 500); }
      return { tag: el.tagName.toLowerCase(), selector: selector(el), text: norm(el.innerText || el.textContent).slice(0, 240), leafText: leaf, textOccurrences: occurrences,
        attrs, rect: rectOf(el), styles: { color: cs.color, backgroundColor: cs.backgroundColor, fontSize: cs.fontSize, fontWeight: cs.fontWeight, fontFamily: cs.fontFamily.split(',')[0].replace(/['"]/g, '') },
        tokens: tokensOf(el) };
    };
    // Which of the design's color tokens the element's text and background
    // are drawn with, so a builder changes the token, not the element.
    // Text on the accent is told before white cards: the same white is both.
    const TOKENS = ['accent', 'accent-fg', 'fg', 'bg', 'card'];
    const tokensOf = (picked) => {
      // Read from a copy beside it: the picked element is under the pointer,
      // and its hover colors are not its tokens.
      const el = picked.cloneNode(true);
      el.style.cssText += ';position:absolute;visibility:hidden;pointer-events:none;left:-9999px';
      picked.after(el);
      const cs = getComputedStyle(el);
      const probe = document.createElement('i');
      probe.style.cssText = 'position:absolute;width:0;height:0;overflow:hidden';
      document.documentElement.append(probe);
      const value = {};
      for (const k of TOKENS) { probe.style.color = `var(--${k})`; value[k] = getComputedStyle(probe).color; }
      probe.remove();
      const match = (c) => (c && !/rgba\(\d+, \d+, \d+, 0\)|transparent/.test(c) ? TOKENS.find((k) => value[k] === c) || null : null);
      // The background seen behind the element: its own, else the nearest one under it.
      let back = null;
      for (let n = el; n && n.nodeType === 1 && !back; n = n.parentElement) back = match(getComputedStyle(n).backgroundColor) || (/rgba\(\d+, \d+, \d+, 0\)|transparent/.test(getComputedStyle(n).backgroundColor) ? null : 'other');
      const out = { text: match(cs.color), background: back === 'other' ? null : back };
      el.remove();
      return out;
    };
    const moved = () => { place(box, null); place(lockBox, lock); if (lock) post({ kind: 'pick-rect', rect: lock.isConnected ? rectOf(lock) : null }); };
    const onMove = (e) => { if (!editing && pickable(e.target) && e.target !== hover) { hover = e.target; place(box, hover); } };
    const onClick = (e) => {
      if (editing) { if (editing.el.contains(e.target)) return; e.preventDefault(); e.stopPropagation(); commit(); return; }
      e.preventDefault(); e.stopPropagation();
      if (!pickable(e.target)) return;
      lock = e.target; place(lockBox, lock); place(box, null);
      post({ kind: 'picked', el: about(lock) });
    };
    const onKey = (e) => {
      if (editing && e.key === 'Enter') { e.preventDefault(); e.stopPropagation(); commit(); }
      else if (e.key === 'Escape') { e.preventDefault(); e.stopPropagation(); if (editing) cancel(); else post({ kind: 'pick-exit' }); }
    };
    const finish = () => { const { el, before, editable } = editing; editable == null ? el.removeAttribute('contenteditable') : el.setAttribute('contenteditable', editable); el.textContent = before; el.blur(); editing = null; moved(); };
    const commit = () => {
      if (!editing) return;
      const { el, before, editId } = editing;
      const next = norm(el.textContent);
      finish();
      post(next === norm(before) ? { kind: 'pick-edited', editId, ok: false, reason: 'unchanged' } : { kind: 'pick-edited', editId, ok: true, oldText: norm(before), newText: next });
    };
    const cancel = () => { if (!editing) return; const { editId } = editing; finish(); post({ kind: 'pick-edited', editId, ok: false, reason: 'cancelled' }); };
    const start = () => {
      if (on) return;
      on = true; pickingNow = true; box = outline('rgba(0,113,227,.55)'); lockBox = outline('#0071e3');
      document.addEventListener('mousemove', onMove, true);
      document.addEventListener('click', onClick, true);
      document.addEventListener('keydown', onKey, true);
      window.addEventListener('scroll', moved, true);
      window.addEventListener('resize', moved);
    };
    const stop = () => {
      if (!on) return;
      if (editing) finish();
      on = false; pickingNow = false; hover = lock = null; box?.remove(); lockBox?.remove(); box = lockBox = null;
      document.removeEventListener('mousemove', onMove, true);
      document.removeEventListener('click', onClick, true);
      document.removeEventListener('keydown', onKey, true);
      window.removeEventListener('scroll', moved, true);
      window.removeEventListener('resize', moved);
    };
    window.addEventListener('message', (e) => {
      if (e.source !== parent) return;
      const m = e.data;
      if (!m || !m.__sa) return;
      if (m.kind === 'pick') m.on ? start() : stop();
      else if (m.kind === 'pick-clear') { if (editing) finish(); lock = null; place(lockBox, null); }
      else if (m.kind === 'pick-edit') {
        const editId = m.editId;
        if (!on || !lock || editing || lock.children.length) { post({ kind: 'pick-edited', editId, ok: false, reason: 'not-found' }); return; }
        editing = { el: lock, before: lock.textContent, editId, editable: lock.getAttribute('contenteditable') };
        lock.setAttribute('contenteditable', 'plaintext-only');
        lock.focus({ preventScroll: true });
        const range = document.createRange(); range.selectNodeContents(lock); const sel = getSelection(); sel.removeAllRanges(); sel.addRange(range);
        lock.addEventListener('input', moved);
      } else if (m.kind === 'pick-commit') commit();
      else if (m.kind === 'pick-cancel') cancel();
      else if (m.kind === 'pick-applied' && lock && !editing && norm(lock.textContent) === norm(m.oldText)) { lock.textContent = String(m.newText || ''); moved(); }
    });
    // A frame drawn again while picking (a new screen) asks to be told.
    post({ kind: 'pick-ready' });
  })();

  // A form never navigates the frame: views handle submit themselves.
  document.addEventListener('submit', (e) => e.preventDefault(), true);

  // External links open in a new tab; the frame never navigates away.
  document.addEventListener('click', (e) => {
    const a = e.target.closest && e.target.closest('a[href]');
    if (!a) return;
    const href = a.getAttribute('href');
    if (/^https?:/i.test(href)) { e.preventDefault(); if (e.isTrusted) post({ kind: 'link', href: a.href }); } else if (href === '#' || href.startsWith('#')) { e.preventDefault(); }
  });
  // The frame may open no window; window.open asks the studio, which opens
  // it only while a click is fresh.
  // A payment's page (the address app.pay gives) opened when the click is no
  // longer fresh (after a handler's awaits) is offered as the payment sheet,
  // whose button is the tap the browser wants.
  const payments = new Map();
  window.open = (url) => {
    const p = payments.get(String(url));
    const fresh = !navigator.userActivation || navigator.userActivation.isActive;
    if (p && !p.offered && !fresh && window.__sa_paySheet) { p.offered = true; window.__sa_paySheet(p); return null; }
    try { post({ kind: 'link', href: new URL(String(url), SA.origin || location.href).href }); } catch { /* not an address */ }
    return null;
  };

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
    money(n, currency = SA.currency || 'USD') { return n === null || n === undefined ? '' : Number(n).toLocaleString(undefined, { style: 'currency', currency }); },
  };

  // Files and map tiles come from the engine itself, by URL: the frame's
  // stylesheet policy lets images load from there and nothing else.
  const origin = SA.origin || '';
  const files = `${origin}/api/apps/${SA.app || ''}/files/`;
  const one = (f) => rpc('upload', [f]);
  // Every remove offers Undo for a few seconds, so nothing asks "are you
  // sure?" first. Removes close together share one toast; Undo puts every
  // row back as it was, with its id.
  const NAMES = ['name', 'title', 'label', 'subject', 'item', 'task', 'what', 'text'];
  let undone = null;
  const offerUndo = (table, row) => {
    if (!undone) {
      const el = document.createElement('div');
      el.className = '__toast __undo';
      el.setAttribute('role', 'status');
      const text = document.createElement('span');
      const btn = document.createElement('button');
      btn.type = 'button';
      btn.textContent = 'Undo';
      el.append(text, btn);
      document.body.append(el);
      const box = { el, text, rows: [], timer: 0 };
      btn.onclick = async () => {
        clearTimeout(box.timer);
        if (undone === box) undone = null;
        btn.disabled = true;
        try { for (const r of box.rows.reverse()) await rpc('insert', [r.table, r.row]); el.remove(); } catch (e) { text.textContent = `Could not undo: ${e.message || e}`; setTimeout(() => el.remove(), 4000); }
      };
      undone = box;
    }
    const box = undone;
    box.rows.push({ table, row });
    const name = box.rows.length === 1 ? NAMES.map((k) => row[k]).find((v) => typeof v === 'string' && v.trim()) : null;
    box.text.textContent = name ? `Removed “${name.trim().slice(0, 40)}”` : box.rows.length === 1 ? 'Removed' : `Removed ${box.rows.length}`;
    clearTimeout(box.timer);
    box.timer = setTimeout(() => { box.el.remove(); if (undone === box) undone = null; }, 7000);
  };
  const app = Object.freeze({
    params: Object.freeze({ ...(SA.params || {}) }),
    view: SA.slug,
    // people: one account for every app on this engine
    me: () => rpc('me', []),
    // (a sign-in or out is told to the page: 'sa-signed' on window, its user)
    signIn: (email, password) => rpc('signIn', [typeof email === 'object' && email ? email : { email, password }]).then((u) => { window.dispatchEvent(new CustomEvent('sa-signed', { detail: u })); return u; }),
    // pay({table, id, description, email} | {amount, description}): a card
    // payment on a hosted page; resolves to {url, mode, amount, currency}.
    // Open the url from a tap (window.open(url)); the app never sees a card.
    pay: (opts) => rpc('pay', [opts || {}]).then((r) => { const p = r && (r.payment || r); if (p && p.url) payments.set(String(p.url), p); return r; }),
    // payNow(payment, note): a sheet with the payment's amount and a "Pay
    // by card" button that opens its hosted page (the tap the browser wants)
    payNow: (p, note) => (window.__sa_paySheet ? window.__sa_paySheet(p, note) : (p && p.url ? window.open(p.url) : null)),
    signUp: (fields) => rpc('signUp', [fields || {}]).then((u) => { window.dispatchEvent(new CustomEvent('sa-signed', { detail: u })); return u; }),
    signOut: () => rpc('signOut', []).then((r) => { window.dispatchEvent(new CustomEvent('sa-signed', { detail: null })); return r; }),
    people: () => rpc('people', []),
    // companies (workspaces) the engine keeps: rows scoped by :company_id are the current one's
    companies: () => rpc('companies', []),
    company: () => rpc('company', []),
    createCompany: (name) => rpc('createCompany', [String(name || '')]),
    useCompany: (id) => rpc('useCompany', [String(id || '')]),
    addToCompany: (company, email, role) => rpc('addToCompany', [String(company || ''), String(email || ''), role || 'member']),
    // files: upload(File | FileList | File[]) -> {id, name, type, size, url}
    upload: (f) => (f && typeof f.length === 'number' && !(f instanceof Blob) ? Promise.all(Array.from(f).map(one)) : one(f)),
    fileUrl: (id, opts) => (!id ? '' : /^(https?:|data:|blob:)/.test(String(id)) ? String(id) : `${files}${encodeURIComponent(id)}${opts && opts.download ? '?download' : ''}`),
    files: (ids) => rpc('files', [[].concat(ids || [])]),
    email: (msg) => rpc('email', [msg || {}]),
    geocode: (q) => rpc('geocode', [String(q || '')]),
    ask: (prompt, opts) => rpc('ask', [prompt, opts || {}]),
    fetch: (url, opts) => rpc('fetch', [String(url || ''), opts || {}]),
    tiles: `${origin}/api/tiles/{z}/{x}/{y}.png`,
    sql: (sql, params) => rpc('sql', [sql, params || {}]),
    insert: (table, row) => rpc('insert', [table, row || {}]),
    update: (table, id, fields) => rpc('update', [table, id, fields || {}]),
    // remove(table, id, {undo: false}) for a remove the person undoes by
    // themselves (unticking a check).
    remove: (table, id, opts) => rpc('remove', [table, id]).then((out) => {
      if (out && out.row && !(opts && opts.undo === false)) offerUndo(table, out.row);
      return out;
    }),
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
    // csv(rows, columns): CSV text; columns are names or {key, label} (default every column but id).
    csv: (rows, columns) => {
      const list = Array.isArray(rows) ? rows : [];
      let cols = (Array.isArray(columns) && columns.length ? columns : [...new Set(list.flatMap((r) => Object.keys(r || {})))].filter((k) => k !== 'id'))
        .map((c) => (typeof c === 'string' ? { key: c, label: c } : { key: c && (c.key || c.name), label: c && (c.label || c.key || c.name) })).filter((c) => c.key);
      if (list.length) cols = cols.filter((c) => list.some((r) => r && r[c.key] !== undefined)); // computed columns have no value of their own
      const cell = (v) => {
        let t = v == null ? '' : Array.isArray(v) ? v.join('; ') : typeof v === 'object' ? JSON.stringify(v) : String(v);
        if (typeof v === 'string' && /^[=+\-@\t\r]/.test(t)) t = `'${t}`; // a spreadsheet would run it as a formula
        return /[",\r\n]/.test(t) ? `"${t.replace(/"/g, '""')}"` : t;
      };
      return [cols.map((c) => cell(String(c.label))), ...list.map((r) => cols.map((c) => cell(r && r[c.key])))].map((l) => l.join(',')).join('\r\n');
    },
    // download(name, text, type): a file the person keeps (a CSV by default).
    download: (name, text, type) => {
      const kind = String(type || 'text/csv');
      const a = document.createElement('a');
      a.href = URL.createObjectURL(new Blob([kind === 'text/csv' ? `\ufeff${text}` : String(text)], { type: `${kind};charset=utf-8` }));
      a.download = String(name || 'export.csv').replace(/[\\/:*?"<>|]+/g, '-');
      document.body.append(a);
      a.click();
      setTimeout(() => { URL.revokeObjectURL(a.href); a.remove(); }, 2000);
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
