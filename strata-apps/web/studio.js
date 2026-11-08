// The studio: start an app from a description, use it, ask for changes, take
// suggestions, go back to an earlier version, and read the code the engine
// generated and stored in Strata.

import { drawGraph, nodePanel, GRAPH_HELP } from './graph.js';

const $ = (sel, el = document) => el.querySelector(sel);
function h(tag, props, ...children) {
  const el = document.createElement(tag);
  if (props && typeof props === 'object' && !(props instanceof Node) && !Array.isArray(props)) {
    for (const [k, v] of Object.entries(props)) {
      if (v === undefined || v === null || v === false) continue;
      if (k.startsWith('on')) el.addEventListener(k.slice(2), v);
      else if (k === 'class') el.className = v;
      else if (k === 'value' || k === 'disabled' || k === 'hidden') el[k] = v;
      else el.setAttribute(k, v === true ? '' : v);
    }
  } else if (props !== undefined) children.unshift(props);
  const add = (c) => {
    if (c === null || c === undefined || c === false) return;
    if (Array.isArray(c)) c.forEach(add);
    else el.append(c instanceof Node ? c : document.createTextNode(String(c)));
  };
  children.forEach(add);
  return el;
}

async function api(method, url, body) {
  const res = await fetch(url, { method, headers: body ? { 'content-type': 'application/json' } : {}, body: body ? JSON.stringify(body) : undefined });
  const data = await res.json().catch(() => ({}));
  if (!res.ok) throw new Error(data.error || res.statusText);
  return data;
}

const ago = (v) => {
  if (!v) return '';
  const s = (Date.now() - new Date(v).getTime()) / 1000;
  if (s < 45) return 'just now';
  if (s < 3600) return `${Math.round(s / 60)} min ago`;
  if (s < 86400) return `${Math.round(s / 3600)} h ago`;
  return `${Math.round(s / 86400)} d ago`;
};

function toast(text) {
  const el = h('div', { class: 'sugg', style: 'position:fixed;left:50%;bottom:22px;transform:translateX(-50%);z-index:60;box-shadow:0 8px 24px rgba(0,0,0,.18);max-width:90vw' }, text);
  document.body.append(el);
  setTimeout(() => el.remove(), 3500);
}

const EXAMPLES = [
  'A reading list: I paste a link with a title and a note, tag it, and mark it read. Show what is unread first.',
  'A habit tracker: my daily habits, a check per day, and my current streak for each.',
  'A tiny CRM for my consulting: people, companies, and notes from each call, with a follow-up date.',
  'Track the books my kids read: who read it, when they finished, and a rating out of 5.',
  'A shared grocery list for our home, grouped by aisle, where ticking an item moves it to the bottom.',
];

// An app on its own, at /a/<id>: its views, without the studio around them.
const BARE = (location.pathname.match(/^\/a\/([a-z0-9]+)/) || [])[1] || null;
if (BARE) document.body.classList.add('bare');

const S = {
  status: null,
  appId: null,
  app: null,
  view: null,
  frames: new Map(), // slug -> {iframe, capId, params}
  panel: 'change',
  errors: new Map(), // slug -> {message, repairing}
  lines: [],         // progress of the running request
  runningText: '',
  failed: null,
  suggesting: false,
  panelOpen: false,
};

let KIT = '';
let FRAME = '';
async function assets() {
  if (!KIT) [KIT, FRAME] = await Promise.all([fetch('/kit.css').then((r) => r.text()), fetch('/frame.js').then((r) => r.text())]);
}

// ---- live updates

function connect() {
  const es = new EventSource('/api/stream');
  const dot = $('#status');
  es.addEventListener('hello', (e) => {
    const { feed } = JSON.parse(e.data);
    dot.className = 'status live';
    dot.title = feed ? 'Live: following Strata\'s feed of changes' : 'Live';
  });
  es.onerror = () => { dot.className = 'status down'; dot.title = 'Not connected to the engine'; };
  const mine = (e) => { const d = JSON.parse(e.data); return d.app === S.appId ? d : null; };
  es.addEventListener('data', (e) => {
    const d = mine(e);
    if (!d) return;
    for (const f of S.frames.values()) f.iframe.contentWindow?.postMessage({ __sa: 1, kind: 'change', tables: d.tables }, '*');
    if (S.panel === 'inside') refreshApp();
  });
  es.addEventListener('app', (e) => { if (mine(e)) refreshApp(); else if (!S.appId) route(); });
  es.addEventListener('suggestions', (e) => { if (mine(e)) refreshApp(); });
  es.addEventListener('progress', (e) => {
    const d = mine(e);
    if (!d) return;
    if (S.runningText !== d.text) { S.runningText = d.text; S.lines = []; }
    S.lines.push(d.line);
    S.failed = null;
    renderProgress();
  });
  es.addEventListener('done', (e) => {
    const d = mine(e);
    if (!d) return;
    S.lines = [];
    S.runningText = '';
    toast(`Version ${d.n} is live${d.summary ? `: ${d.summary}` : ''}`);
    refreshApp();
  });
  es.addEventListener('failed', (e) => {
    const d = mine(e);
    if (!d) return;
    S.failed = { text: S.runningText, error: d.error, lines: S.lines.slice() };
    S.lines = [];
    S.runningText = '';
    refreshApp();
  });
  es.addEventListener('repair', (e) => {
    const d = mine(e);
    if (!d) return;
    toast('A capability failed; the engine is repairing it');
  });
}

// ---- routing

function route() {
  const parts = location.hash.replace(/^#\/?/, '').split('/').filter(Boolean);
  if (BARE) return openApp(BARE, parts[0] ? decodeURIComponent(parts[0]) : null);
  $('#nav-library').classList.toggle('on', parts[0] === 'library');
  if (parts[0] === 'app' && parts[1]) return openApp(parts[1], parts[2] ? decodeURIComponent(parts[2]) : null);
  leaveApp();
  if (parts[0] === 'library') return renderLibrary(parts[1] === 'graph' && parts[2] ? decodeURIComponent(parts[2]) : null);
  return renderHome();
}

function leaveApp() {
  S.appId = null;
  S.app = null;
  S.frames.clear();
  S.errors.clear();
  S.lines = [];
  S.failed = null;
  S.runningText = '';
}

// ---- home

async function renderHome() {
  $('#crumbs').replaceChildren();
  const main = $('#main');
  const ta = h('textarea', { placeholder: 'Describe the first version of your app: what it keeps, what you do with it, what you want to see.', rows: 4 });
  const go = h('button', { class: 'btn primary', onclick: () => create() }, 'Build it');
  const err = h('span', { class: 'err small' });
  async function create() {
    const description = ta.value.trim();
    if (description.length < 5) { ta.focus(); return; }
    go.disabled = true;
    go.textContent = 'Starting...';
    try {
      const out = await api('POST', '/api/apps', { description });
      location.hash = `#/app/${out.app}`;
    } catch (e) { err.textContent = e.message; go.disabled = false; go.textContent = 'Build it'; }
  }
  ta.addEventListener('keydown', (e) => { if (e.key === 'Enter' && (e.metaKey || e.ctrlKey)) create(); });
  const list = h('div', { class: 'apps' }, h('div', { class: 'hint' }, 'Loading...'));
  main.replaceChildren(h('div', { class: 'page scroll' },
    h('section', { class: 'hero' },
      h('h1', 'Describe an app. Use it. Ask it to change.'),
      h('p', 'Each app starts from your description and grows by your requests. The engine writes the code, checks it, and stores every piece in Strata, where other apps can reuse it. It watches what you do and suggests what to add next.')),
    h('section', { class: 'describe' }, ta, h('div', { class: 'bar' }, go, h('span', { class: 'hint' }, '⌘/Ctrl + Enter'), err)),
    h('div', { class: 'examples' }, EXAMPLES.map((x) => h('button', { class: 'example', onclick: () => { ta.value = x; ta.focus(); } }, x.split(':')[0]))),
    h('section', { class: 'col', style: 'display:flex;flex-direction:column;gap:10px' }, h('h2', { class: 'label' }, 'Your apps'), list),
    h('section', { class: 'how' },
      h('div', h('b', '1. Describe it'), 'The forge asks the model for tables, views and server functions, checks the code, and stores each piece as a row in Strata.'),
      h('div', h('b', '2. Use it, change it'), 'Views run in sandboxed frames, functions in a WebAssembly sandbox. Ask for any change; each one is a new version you can go back from.'),
      h('div', h('b', '3. It grows'), 'What you do is recorded in Strata. The engine suggests changes from it, and reuses what other apps built from the shared library.')),
  ));
  setTimeout(() => ta.focus(), 0);
  try {
    const { apps } = await api('GET', '/api/apps');
    list.replaceChildren(...(apps.length ? apps.map((a) => h('a', { class: 'appcard', href: `#/app/${a.id}` },
      h('div', { class: 'name' }, a.name),
      h('div', { class: 'desc' }, a.description),
      h('div', { class: 'meta' }, h('span', { class: 'pill' }, a.head ? `v${a.head}` : 'building'), h('span', ago(a.created)))))
      : [h('div', { class: 'hint' }, 'None yet. Describe one above, or pick an example.')]));
  } catch (e) { list.replaceChildren(h('div', { class: 'err' }, e.message)); }
}

// ---- library

async function renderLibrary(selected) {
  $('#crumbs').replaceChildren(h('span', { class: 'sep' }, '/'), h('b', 'Library'));
  const main = $('#main');
  const q = h('input', { class: 'search', placeholder: 'Search what every app has built (views, functions, tables)...' });
  const out = h('div', { class: 'lib' });
  const mode = location.hash.includes('graph') ? 'graph' : 'list';
  const switcher = h('div', { class: 'seg' },
    h('button', { class: mode === 'list' ? 'on' : '', onclick: () => { location.hash = '#/library'; } }, 'List'),
    h('button', { class: mode === 'graph' ? 'on' : '', onclick: () => { location.hash = '#/library/graph'; } }, 'Graph'));
  main.replaceChildren(h('div', { class: `page scroll${mode === 'graph' ? ' wide' : ''}` },
    h('section', { class: 'hero' }, h('h1', 'Library'), h('p', 'What every app runs now: shared components and helpers, which every app can call, and each app’s views, functions and tables. Each version is a row of sa_capabilities in Strata, searched with BM25 over what it is and its code, and linked to what it uses in a graph. The forge looks here before it writes something new.')),
    switcher, mode === 'graph' ? out : q, mode === 'graph' ? null : out));
  if (mode === 'graph') return libraryGraph(out, { selected });
  let apps = [];
  try { ({ apps } = await api('GET', '/api/apps')); } catch { /* none */ }
  async function load() {
    const { capabilities } = await api('GET', `/api/library${q.value.trim() ? `?q=${encodeURIComponent(q.value.trim())}` : ''}`);
    const latest = new Map();
    for (const c of capabilities) {
      const k = `${c.app}/${c.kind}/${c.slug}`;
      if (!latest.has(k) || Number(latest.get(k).version) < Number(c.version)) latest.set(k, c);
    }
    const items = [...latest.values()].filter((c) => q.value.trim() || c.kind !== 'table')
      .sort((a, b) => ({ component: 0, helper: 1 }[a.kind] ?? 2) - ({ component: 0, helper: 1 }[b.kind] ?? 2));
    out.replaceChildren(...(items.length ? items.map((c) => {
      const sel = h('select', { class: 'btn small' }, h('option', { value: '' }, 'Use in...'), apps.filter((a) => a.id !== c.app).map((a) => h('option', { value: a.id }, a.name)));
      sel.addEventListener('change', async () => {
        if (!sel.value) return;
        const target = sel.value;
        await api('POST', `/api/apps/${target}/requests`, { source: 'library', text: c.kind === 'component' || c.kind === 'helper'
          ? `Use the shared ${c.kind} ${c.slug} ("${c.title}") in this app, where it fits.`
          : `Bring in the ${c.kind} "${c.title}" from the app "${c.app_name}" (library id ${c.id}) and make it work with this app's data.` });
        location.hash = `#/app/${target}`;
      });
      return h('div', { class: 'libitem' },
        h('span', { class: `pill ${c.kind}` }, c.kind), h('span', { class: 't' }, c.title), h('span', { class: 'meta' }, c.kind === 'component' || c.kind === 'helper'
          ? `${c.kind === 'helper' ? 'lib' : 'ui'}.${c.slug} · v${c.version}${c.author === 'builtin' ? ' · built in' : ''} · ${c.used_by?.length ? `used by ${c.used_by.map((u) => `${u.name}${u.version < c.version ? ` (v${u.version})` : ''}`).join(', ')}` : 'not used yet'}`
          : `${c.app_name} · v${c.version}${c.author && c.author !== 'forge' ? ` · ${c.author}` : ''}`),
        h('span', { style: 'margin-left:auto;display:flex;gap:6px' }, h('button', { class: 'btn small', onclick: () => showCode(c.id) }, 'Code'),
          h('button', { class: 'btn small', title: 'what it uses, what depends on it, what goes with it', onclick: () => { location.hash = `#/library/graph/${encodeURIComponent(`${c.app}/${c.kind}/${c.slug}`)}`; } }, 'Links'), c.kind !== 'table' ? sel : null),
        c.description ? h('div', { class: 'd' }, c.description) : null);
    }) : [h('div', { class: 'hint' }, q.value ? 'Nothing matches.' : 'Nothing yet: build an app first.')]));
  }
  let t;
  q.addEventListener('input', () => { clearTimeout(t); t = setTimeout(load, 200); });
  load().catch((e) => out.replaceChildren(h('div', { class: 'err' }, e.message)));
}

// The library as a graph: every node of sa_graph, and a panel with what
// Strata's graph queries say about the one picked.
async function libraryGraph(out, { app, selected } = {}) {
  let nodes;
  try { ({ nodes } = await api('GET', `/api/graph${app ? `?app=${app}` : ''}`)); } catch (e) { out.replaceChildren(h('div', { class: 'err' }, e.message)); return; }
  if (!nodes.length) { out.replaceChildren(h('div', { class: 'hint' }, 'Nothing yet: build an app first.')); return; }
  const side = h('div', { class: 'gside' }, h('div', { class: 'hint' }, 'Click a node to see what it needs, what depends on it and what goes with it.'));
  let picked = null;
  const pick = async (id) => {
    if (!id || id === picked) return;
    picked = id;
    side.replaceChildren(h('div', { class: 'hint' }, 'Asking Strata...'));
    const panel = await nodePanel(id, { h, api, showCode, onPick: (x) => g.select(x) });
    if (picked === id) side.replaceChildren(panel);
  };
  const g = drawGraph(nodes, { onSelect: pick, compact: !!app, selected });
  const edges = nodes.reduce((n, x) => n + (x.links || []).length, 0);
  out.replaceChildren(
    h('div', { class: 'where' }, GRAPH_HELP, ` ${nodes.length} nodes, ${edges} edges.`),
    h('div', { class: 'glegend' }, g.legend.map((l) => h('span', null, h('i', { style: `border-top:2px ${l.dash ? 'dashed' : 'solid'} ${l.color}` }), l.text))),
    h('div', { class: 'gwrap' }, h('div', { class: 'gcanvas' }, g.svg), side));
}

async function showMap(appId, name) {
  const modal = $('#modal');
  const close = () => { modal.hidden = true; modal.replaceChildren(); };
  modal.onclick = (e) => { if (e.target === modal) close(); };
  const body = h('div', { class: 'mapbody' }, h('div', { class: 'hint' }, 'Loading...'));
  modal.replaceChildren(h('div', { class: 'sheet wide' }, h('header', h('span', { class: 't' }, `${name}: how its pieces link`), h('button', { class: 'btn small ghost', onclick: close }, 'Close')), body));
  modal.hidden = false;
  await libraryGraph(body, { app: appId });
}

// ---- code

async function showCode(id, newer) {
  const modal = $('#modal');
  const close = () => { modal.hidden = true; modal.replaceChildren(); };
  modal.onclick = (e) => { if (e.target === modal) close(); };
  let cap;
  try { cap = await api('GET', `/api/capabilities/${encodeURIComponent(id)}`); } catch (e) { toast(e.message); return; }
  const v = Number(cap.version);
  const prev = v > 1 ? id.replace(/@\d+$/, `@${v - 1}`) : null;
  const body = cap.kind === 'table'
    ? ['id keyword (primary key)', 'created timestamp', ...(cap.spec.columns || []).map((c) => `${c.name} ${c.type}`)].join('\n')
    : cap.code;
  const trigger = cap.spec?.on ? `runs after each ${cap.spec.on.event || 'insert'} on ${cap.spec.on.table}` : null;
  modal.replaceChildren(h('div', { class: 'sheet' },
    h('header', h('span', { class: `pill ${cap.kind}` }, cap.kind), h('span', { class: 't' }, `${cap.title} · v${v}`),
      prev ? h('button', { class: 'btn small', title: 'the version before', onclick: () => showCode(prev, id) }, `← v${v - 1}`) : null,
      newer ? h('button', { class: 'btn small', onclick: () => showCode(newer) }, `v${v + 1} →`) : null,
      h('button', { class: 'btn small ghost', onclick: close }, 'Close')),
    h('div', { class: 'info' },
      cap.description ? h('div', cap.description) : null,
      trigger ? h('div', trigger) : null,
      h('div', h('span', { class: 'mono' }, cap.id), ` · written by ${cap.author || 'forge'} ${ago(cap.created)}`, cap.forked_from ? ` · from ${cap.forked_from}` : ''),
      h('div', `A row of sa_capabilities in Strata${cap.kind === 'table' ? `; its data is the table app_${cap.app}_${cap.slug}` : ''}.`)),
    h('pre', body)));
  modal.hidden = false;
}

// ---- an app

async function openApp(id, viewSlug) {
  const first = S.appId !== id;
  if (first) {
    leaveApp();
    S.appId = id;
    S.panel = 'change';
    await assets();
    $('#main').replaceChildren(h('div', { class: 'appview' },
      h('section', { class: 'stage' }, h('div', { class: 'tabs', id: 'tabs' }), h('div', { class: 'frames', id: 'frames' })),
      h('aside', { class: 'panel closed', id: 'panel' })));
  }
  if (viewSlug) S.view = viewSlug;
  await refreshApp();
}

let refreshing = null;
let again = false;
async function refreshApp() {
  if (!S.appId) return;
  if (refreshing) { again = true; return refreshing; }
  refreshing = (async () => {
    try {
      const state = await api('GET', `/api/apps/${S.appId}`);
      if (state.app.id !== S.appId) return;
      S.app = state;
      if (state.running && !S.lines.length) { S.runningText = state.running.text; S.lines = state.running.lines; }
      if (!state.running && S.lines.length && !S.runningText) S.lines = [];
      renderApp();
    } catch (e) {
      $('#main').replaceChildren(h('div', { class: 'page' }, h('div', { class: 'err' }, e.message), h('a', { href: '#/' }, 'Back')));
      S.appId = null;
    }
  })();
  await refreshing;
  refreshing = null;
  if (again) { again = false; await refreshApp(); }
}

function renderApp() {
  const st = S.app;
  if (BARE) {
    document.title = st.app.name;
    $('.brand').replaceChildren(h('span', { class: 'mark' }), st.app.name);
  } else {
    $('#crumbs').replaceChildren(h('span', { class: 'sep' }, '/'), h('b', st.app.name), st.version.n ? h('span', { class: 'pill' }, `v${st.version.n}`) : null,
      st.views.length ? h('a', { class: 'navlink small', href: `/a/${st.app.id}`, target: '_blank', title: 'The app without the studio: for a phone, or to hand to someone' }, 'Open on its own') : null);
  }
  const views = st.views;
  if (!views.find((v) => v.slug === S.view)) S.view = views[0]?.slug || null;
  // tabs
  const tabs = $('#tabs');
  tabs.replaceChildren(...views.map((v) => h('button', { class: `tab${v.slug === S.view ? ' on' : ''}`, onclick: () => switchView(v.slug) }, v.title, S.errors.has(v.slug) ? h('span', { class: 'dot', style: 'background:var(--bad)' }) : null)),
    h('button', { class: 'tab mobile-only', style: 'margin-left:auto', onclick: () => togglePanel() }, st.running ? 'Changing...' : 'Change'));
  if (!views.length) tabs.replaceChildren(h('span', { class: 'tab on' }, 'First version'), h('button', { class: 'tab mobile-only', style: 'margin-left:auto', onclick: () => togglePanel() }, 'Details'));
  renderFrames();
  renderPanel();
}

function togglePanel() {
  S.panelOpen = !S.panelOpen;
  $('#panel').classList.toggle('closed', !S.panelOpen);
}

function switchView(slug, params) {
  S.view = slug;
  const v = S.app.views.find((x) => x.slug === slug);
  if (v) api('POST', `/api/apps/${S.appId}/track`, { kind: 'open', detail: slug, cap: v.id }).catch(() => {});
  if (params) {
    const f = S.frames.get(slug);
    if (f) { f.iframe.remove(); S.frames.delete(slug); }
    S.params = { ...(S.params || {}), [slug]: params };
  }
  history.replaceState(null, '', BARE ? `#/${encodeURIComponent(slug)}` : `#/app/${S.appId}/${encodeURIComponent(slug)}`);
  renderApp();
}

const noClose = (code) => String(code).replace(/<\/(script)/gi, '<\\/$1');

function srcdoc(view, params, components, helpers) {
  const code = noClose(view.code);
  const boot = JSON.stringify({ slug: view.slug, cap: view.id, app: S.appId, origin: location.origin, params: params || {} }).replace(/</g, '\\u003c');
  // One script per shared component, so a broken one does not take the others down.
  const comps = (components || []).map((c) => `<script>__sa_component(${JSON.stringify(c.slug)}, ${JSON.stringify(c.id)}, function () {
${noClose(c.code)}
;return typeof component === 'function' ? component : null; });</script>`).join('\n');
  const libs = (helpers || []).map((c) => `<script>__sa_helper(${JSON.stringify(c.slug)}, ${JSON.stringify(c.id)}, function () {
${noClose(c.code)}
;return helper(); });</script>`).join('\n');
  return `<!doctype html><html><head><meta charset="utf-8"><meta name="viewport" content="width=device-width, initial-scale=1">
<meta http-equiv="Content-Security-Policy" content="default-src 'none'; script-src 'unsafe-inline'; style-src 'unsafe-inline'; img-src https: data: blob: ${location.origin}; media-src https: data: blob: ${location.origin}; font-src data:">
<style>${KIT}</style></head><body><div id="root"></div>
<script>window.__SA = ${boot};</script>
<script>${FRAME}</script>
${libs}
${comps}
<script>
(async () => { try {
${code}
;if (typeof render !== 'function') throw new Error('the view defines no render(root, app)');
await render(document.getElementById('root'), app);
__sa_ready();
} catch (e) { __sa_report(e); } })();
</script></body></html>`;
}

function renderFrames() {
  const box = $('#frames');
  const st = S.app;
  if (!st.views.length) {
    for (const f of S.frames.values()) f.iframe.remove();
    S.frames.clear();
    box.replaceChildren(buildingScreen());
    return;
  }
  box.querySelector('.building')?.remove();
  // drop frames of views that are gone or changed, or whose components changed
  const compSig = [...(st.components || []), ...(st.helpers || [])].map((c) => c.id).join(',');
  for (const [slug, f] of S.frames) {
    const v = st.views.find((x) => x.slug === slug);
    if (!v || v.id !== f.capId || f.comps !== compSig) { f.iframe.remove(); S.frames.delete(slug); S.errors.delete(slug); }
  }
  const v = st.views.find((x) => x.slug === S.view);
  if (v && !S.frames.has(v.slug)) {
    const params = S.params?.[v.slug] || {};
    const iframe = h('iframe', { sandbox: 'allow-scripts allow-forms allow-modals allow-popups allow-popups-to-escape-sandbox allow-downloads', title: v.title });
    iframe.srcdoc = srcdoc(v, params, st.components, st.helpers);
    box.append(iframe);
    S.frames.set(v.slug, { iframe, capId: v.id, comps: compSig, params });
  }
  for (const [slug, f] of S.frames) f.iframe.hidden = slug !== S.view;
  box.querySelector('.viewerr')?.remove();
  const err = S.errors.get(S.view);
  if (err) {
    box.append(h('div', { class: 'viewerr' },
      h('div', { class: 'msg' }, h('b', 'This view hit an error. '), err.message, err.repairing ? h('div', { class: 'hint' }, h('span', { class: 'spin' }), ' The engine is repairing it; the fixed version replaces this one when it is ready.') : null),
      !err.repairing ? h('button', { class: 'btn small', onclick: () => ask(`The view "${v?.title}" shows this error: ${err.message}. Fix it.`) }, 'Ask to fix') : null,
      h('button', { class: 'btn small ghost', onclick: () => { S.errors.delete(S.view); renderFrames(); } }, 'Dismiss')));
  }
}

function buildingScreen() {
  const st = S.app;
  const failed = !st.running && st.requests.find((r) => r.status === 'failed');
  return h('div', { class: 'building' }, h('div', { class: 'box' },
    h('h2', failed && !S.lines.length ? 'The first version did not build' : 'Building the first version'),
    h('p', { class: 'hint' }, st.app.description),
    progressBox() || (failed ? h('div', { class: 'log' }, h('div', { class: 'line bad' }, failed.error), h('button', { class: 'btn small', style: 'align-self:start;margin-top:6px', onclick: () => ask(st.app.description) }, 'Try again')) : h('div', { class: 'hint' }, 'Waiting for the engine...'))));
}

function progressBox() {
  if (!S.lines.length) return null;
  return h('div', { class: 'log', id: 'progress' },
    h('div', { class: 'req' }, h('span', { class: 'spin' }), ' ', S.runningText.length > 160 ? `${S.runningText.slice(0, 160)}...` : S.runningText),
    S.lines.map((l, i) => h('div', { class: `line${i === S.lines.length - 1 ? ' last' : ''}${/^Failed/.test(l) ? ' bad' : ''}` }, l)));
}

function renderProgress() {
  if (!S.app) return;
  if (!S.app.views.length) { renderFrames(); }
  if (S.panel === 'change') renderPanel();
}

async function ask(text, extra = {}) {
  await api('POST', `/api/apps/${S.appId}/requests`, { text, ...extra });
  S.failed = null;
  S.runningText = text;
  S.lines = ['Queued'];
  renderProgress();
}

function renderPanel() {
  const st = S.app;
  const panel = $('#panel');
  const active = document.activeElement;
  const draft = panel.querySelector('textarea')?.value || '';
  const hadFocus = active && panel.contains(active) && active.tagName === 'TEXTAREA';
  const tab = (key, label, count) => h('button', { class: `ptab${S.panel === key ? ' on' : ''}`, onclick: () => { S.panel = key; renderPanel(); if (key !== 'change') refreshApp(); } }, label, count ? h('span', { class: 'count' }, count) : null);
  const body = h('div', { class: 'pbody' });
  if (S.panel === 'change') body.append(...changeTab(draft));
  else if (S.panel === 'versions') body.append(...versionsTab());
  else body.append(...insideTab());
  panel.replaceChildren(h('div', { class: 'ptabs' }, tab('change', 'Change'), tab('versions', 'Versions', st.versions.length), tab('inside', 'Inside', st.views.length + st.functions.length + st.tables.length),
    h('button', { class: 'btn small ghost mobile-only', style: 'margin-left:auto', onclick: togglePanel }, 'Close')), body);
  if (hadFocus) { const t = panel.querySelector('textarea'); t?.focus(); t?.setSelectionRange(t.value.length, t.value.length); }
}

function changeTab(draft) {
  const st = S.app;
  const ta = h('textarea', { placeholder: st.version.n ? 'Ask for any change: "add a due date and sort by it", "show a chart of this week", "let me import a list"...' : 'Waiting for the first version...', value: draft });
  const send = h('button', { class: 'btn primary', onclick: submit }, 'Send');
  async function submit() {
    const text = ta.value.trim();
    if (!text) return;
    send.disabled = true;
    ta.value = '';
    try { await ask(text); } catch (e) { toast(e.message); } finally { send.disabled = false; }
  }
  ta.addEventListener('keydown', (e) => { if (e.key === 'Enter' && (e.metaKey || e.ctrlKey)) submit(); });
  const out = [h('div', { class: 'ask' }, ta, h('div', { class: 'bar' }, send, h('span', { class: 'hint' }, S.lines.length ? 'Requests wait their turn.' : '⌘/Ctrl + Enter')))];
  const prog = progressBox();
  if (prog) out.push(prog);
  else if (S.failed) {
    out.push(h('div', { class: 'log' }, h('div', { class: 'req' }, 'This change did not go through'), h('div', { class: 'line bad' }, S.failed.error),
      h('div', { class: 'row', style: 'display:flex;gap:6px;margin-top:6px' }, h('button', { class: 'btn small', onclick: () => ask(S.failed.text) }, 'Try again'), h('button', { class: 'btn small ghost', onclick: () => { S.failed = null; renderPanel(); } }, 'Dismiss'))));
  } else if (st.version.n && st.version.summary) {
    out.push(h('div', { class: 'where' }, h('b', `Version ${st.version.n}: `), st.version.summary));
  }
  if (st.version.n) {
    const sugBtn = h('button', { class: 'btn small ghost', disabled: S.suggesting, onclick: async () => {
      S.suggesting = true; renderPanel();
      try { const r = await api('POST', `/api/apps/${S.appId}/suggest`); if (!r.suggestions.length) toast('Nothing in your use points anywhere yet: use the app a little more'); } catch (e) { toast(e.message); }
      S.suggesting = false; refreshApp();
    } }, S.suggesting ? h('span', null, h('span', { class: 'spin' }), ' Reading your use...') : 'Suggest from my use');
    out.push(h('div', { class: 'col', style: 'display:flex;flex-direction:column;gap:8px' },
      h('div', { style: 'display:flex;align-items:center;gap:8px' }, h('h2', { class: 'label' }, 'Suggested next'), h('span', { style: 'margin-left:auto' }, sugBtn)),
      st.suggestions.length ? st.suggestions.map((s) => h('div', { class: 'sugg' },
        h('div', { class: 't' }, s.text),
        s.why ? h('div', { class: 'why' }, s.why) : null,
        h('div', { class: 'bar' },
          h('button', { class: 'btn small primary', onclick: async (e) => { e.target.disabled = true; await ask(s.text, { source: 'suggestion', suggestion: s.id }); refreshApp(); } }, 'Do it'),
          h('button', { class: 'btn small ghost', onclick: async () => { await api('POST', `/api/apps/${S.appId}/suggestions/${s.id}`, { status: 'dismissed' }); refreshApp(); } }, 'Not now'),
          h('span', { class: 'src' }, s.source === 'use' ? 'from your use' : s.source === 'component' ? 'newer shared piece' : 'from the last build'))))
        : h('div', { class: 'hint' }, 'No suggestions open.')));
  }
  return out;
}

function versionsTab() {
  const st = S.app;
  const head = st.app.head;
  const items = st.versions.map((c) => h('div', { class: `ver${Number(c.n) === Number(head) ? ' on' : ''}` },
    h('div', { class: 'n' }, `v${c.n}`),
    h('div', { class: 'body' },
      h('div', c.summary || '(no summary)'),
      c.request ? h('div', { class: 'req' }, `“${c.request}”`) : null,
      h('div', { class: 'meta' }, h('span', ago(c.created)), h('span', `${(c.caps || []).length} capabilities`),
        Number(c.n) === Number(head) ? h('span', { class: 'pill' }, 'live') : h('button', { class: 'btn small', onclick: async () => { await api('POST', `/api/apps/${S.appId}/restore`, { n: c.n }); toast(`Back to version ${c.n}`); refreshApp(); } }, 'Go back to this')))));
  const failed = st.requests.filter((r) => r.status === 'failed').slice(0, 3);
  return [
    h('div', { class: 'where' }, 'Each version is a row of sa_compositions in Strata naming the capabilities it is made of. Going back moves the app to that row; data stays as it is (tables only grow).'),
    h('div', { class: 'versions' }, items.length ? items : h('div', { class: 'hint' }, 'No version yet.')),
    failed.length ? h('div', { class: 'col', style: 'display:flex;flex-direction:column;gap:6px' }, h('h2', { class: 'label' }, 'Requests that failed'),
      failed.map((r) => h('div', { class: 'sugg' }, h('div', { class: 'why' }, `“${r.text}”`), h('div', { class: 'small err' }, r.error)))) : null,
  ].filter(Boolean);
}

function insideTab() {
  const st = S.app;
  const cap = (c, extra) => h('div', { class: 'cap', onclick: () => showCode(c.id) },
    h('span', { class: `pill ${c.kind}` }, c.kind), h('span', { class: 't' }, c.title || c.slug),
    c.author === 'repair' ? h('span', { class: 'pill repair' }, 'repaired') : null,
    c.forked_from ? h('span', { class: 'pill library' }, 'library') : null,
    extra ? h('span', { class: 'v' }, extra) : null, h('span', { class: 'v' }, `v${c.version}`));
  return [
    h('div', { class: 'where' }, `This app is data in Strata: version ${st.version.n} is the row `, h('span', { class: 'mono' }, `${st.app.id}#${st.version.n}`),
      ' of sa_compositions; each piece below is a row of sa_capabilities with its code; the app\'s data is in tables named ', h('span', { class: 'mono' }, `app_${st.app.id}_*`), '. Click a piece to read its code.'),
    st.version.n ? h('button', { class: 'btn small', style: 'align-self:flex-start', onclick: () => showMap(st.app.id, st.app.name) }, 'Map: how the pieces link') : null,
    h('h2', { class: 'label' }, 'Views'), h('div', { class: 'caps' }, st.views.map((c) => cap(c))),
    h('h2', { class: 'label' }, 'Components (shared with other apps)'), h('div', { class: 'caps' }, (st.components || []).length ? st.components.map((c) => cap(c,
      [c.users > 1 ? `used by ${c.users} apps` : 'only this app so far', c.latest > c.version ? `v${c.latest} available` : null].filter(Boolean).join(' · '))) : h('div', { class: 'hint' }, 'None')),
    h('h2', { class: 'label' }, 'Helpers (shared with other apps)'), h('div', { class: 'caps' }, (st.helpers || []).length ? st.helpers.map((c) => cap(c,
      [c.users > 1 ? `used by ${c.users} apps` : 'only this app so far', c.latest > c.version ? `v${c.latest} available` : null].filter(Boolean).join(' · '))) : h('div', { class: 'hint' }, 'None')),
    h('h2', { class: 'label' }, 'Functions'), h('div', { class: 'caps' }, st.functions.length ? st.functions.map((c) => cap(c, c.spec?.on ? `on ${c.spec.on.event || 'insert'} ${c.spec.on.table}` : null)) : h('div', { class: 'hint' }, 'None')),
    h('h2', { class: 'label' }, 'Tables'), h('div', { class: 'caps' }, st.tables.map((t) => h('div', null,
      h('div', { class: 'cap', onclick: () => showCode(t.id) }, h('span', { class: 'pill table' }, 'table'), h('span', { class: 't' }, t.name), h('span', { class: 'v' }, `${t.rows} rows`)),
      h('div', { class: 'cols' }, (t.columns || []).map((c) => `${c.name} ${c.type}`).join(' · '))))),
    h('h2', { class: 'label' }, `People (${(st.people || []).length})`), h('div', { class: 'caps' }, (st.people || []).length
      ? st.people.map((p) => h('div', { class: 'cap' }, h('span', { class: 'pill person' }, p.role), h('span', { class: 't' }, p.name), h('span', { class: 'v' }, p.email)))
      : h('div', { class: 'hint' }, 'Nobody has signed in yet. People join an app by signing in from one of its views (ui.signIn); the first becomes its owner. Rows of sa_people and sa_members.')),
    h('h2', { class: 'label' }, 'Outbox'), h('div', { class: 'caps' }, (st.outbox || []).length
      ? st.outbox.map((o) => h('div', { class: 'cap', onclick: () => showEmail(o) }, h('span', { class: `pill mail-${o.status}` }, o.status), h('span', { class: 't' }, o.subject || '(no subject)'), h('span', { class: 'v' }, `to ${(o.recipients || []).join(', ')} · ${ago(o.created)}`)))
      : h('div', { class: 'hint' }, 'No email yet. Email the app sends is a row of sa_outbox; it is held here until the engine is given a mail server (SMTP_URL and SMTP_FROM).')),
  ];
}

function showEmail(o) {
  const modal = $('#modal');
  const close = () => { modal.hidden = true; modal.replaceChildren(); };
  modal.onclick = (e) => { if (e.target === modal) close(); };
  modal.replaceChildren(h('div', { class: 'sheet' },
    h('header', h('span', { class: `pill mail-${o.status}` }, o.status), h('span', { class: 't' }, o.subject || '(no subject)'), h('button', { class: 'btn small ghost', onclick: close }, 'Close')),
    h('div', { class: 'info' }, h('div', `To ${(o.recipients || []).join(', ')} · ${ago(o.created)}`),
      o.status === 'held' ? h('div', 'Held: this engine has no mail server, so the message stays here.') : null,
      o.error ? h('div', { class: 'err' }, o.error) : null),
    h('pre', o.body || '')));
  modal.hidden = false;
}

// ---- bridge between view frames and the engine

// A file from a frame goes to the engine as the request's body.
async function upload(base, file) {
  if (!(file instanceof Blob)) throw new Error('app.upload takes a File (from an <input type="file"> or a drop)');
  const q = new URLSearchParams({ name: file.name || 'file', type: file.type || 'application/octet-stream' });
  const res = await fetch(`${base}/files?${q}`, { method: 'POST', headers: { 'content-type': 'application/octet-stream' }, body: file });
  const data = await res.json().catch(() => ({}));
  if (!res.ok) throw new Error(data.error || res.statusText);
  return { ...data.file, url: location.origin + data.file.url };
}

// After someone signs in or out every view starts again, so that each one
// sees who is there now.
function authChanged() {
  setTimeout(() => {
    for (const f of S.frames.values()) f.iframe.remove();
    S.frames.clear();
    S.errors.clear();
    refreshApp();
  }, 400);
}

window.addEventListener('message', async (e) => {
  const m = e.data;
  if (!m || !m.__sa || !S.appId) return;
  let entry = null;
  for (const [slug, f] of S.frames) if (f.iframe.contentWindow === e.source) entry = { slug, ...f };
  if (!entry) return;
  const reply = (value, error) => e.source.postMessage({ __sa: 1, kind: 'result', id: m.id, value, error }, '*');
  const base = `/api/apps/${S.appId}`;
  try {
    if (m.kind === 'rpc') {
      const [a, b, c] = m.args || [];
      let value;
      switch (m.method) {
        case 'sql': value = (await api('POST', `${base}/sql`, { sql: a, params: b })).rows; break;
        case 'insert': value = (await api('POST', `${base}/insert`, { table: a, row: b })).row; break;
        case 'update': value = (await api('POST', `${base}/update`, { table: a, id: b, fields: c })).row; break;
        case 'remove': value = await api('POST', `${base}/remove`, { table: a, id: b }); break;
        case 'call': value = (await api('POST', `${base}/call`, { name: a, input: b })).result; break;
        case 'me': value = (await api('GET', `${base}/me`)).user; break;
        case 'signIn': value = (await api('POST', `${base}/signin`, a)).user; authChanged(); break;
        case 'signUp': value = (await api('POST', `${base}/signup`, a)).user; authChanged(); break;
        case 'signOut': value = await api('POST', '/api/signout'); authChanged(); break;
        case 'people': value = (await api('GET', `${base}/people`)).people; break;
        case 'files': value = (await api('POST', `${base}/files/info`, { ids: a })).files.map((f) => ({ ...f, url: location.origin + f.url })); break;
        case 'email': value = (await api('POST', `${base}/email`, a)).email; break;
        case 'geocode': value = (await api('GET', `/api/geocode?q=${encodeURIComponent(a || '')}`)).places; break;
        case 'upload': value = await upload(base, a); break;
        default: throw new Error(`no app.${m.method}`);
      }
      reply(value);
    } else if (m.kind === 'track') {
      api('POST', `${base}/track`, { kind: 'action', action: m.action, detail: m.detail, cap: entry.capId }).catch(() => {});
    } else if (m.kind === 'open') {
      if (S.app.views.find((v) => v.slug === m.slug)) switchView(m.slug, m.params);
    } else if (m.kind === 'error') {
      const out = await api('POST', `${base}/error`, { cap: m.cap || entry.capId, message: `${m.message}\n${m.stack || ''}`.slice(0, 1500) }).catch(() => ({}));
      S.errors.set(entry.slug, { message: m.message, repairing: !!out.repairing });
      renderApp();
    }
  } catch (err) {
    if (m.kind === 'rpc') reply(undefined, err.message);
  }
});

window.addEventListener('hashchange', route);
window.addEventListener('keydown', (e) => { if (e.key === 'Escape') { const md = $('#modal'); md.hidden = true; md.replaceChildren(); } });
connect();
route();
