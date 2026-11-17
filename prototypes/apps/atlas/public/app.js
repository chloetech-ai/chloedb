import { topbar, api, fmt, esc, logQueries, highlightSQL, toast } from '/shared/ds.js';
import { MapView } from '/atlas/map.js';

topbar({ app: 'MCP Atlas' });

const view = document.getElementById('view');
const tabsEl = document.getElementById('tabs');
const TABS = [['map', 'The map'], ['findings', 'Findings'], ['creds', 'Credentials'], ['search', 'Search'], ['about', 'Method & licences']];
let cur = 'map';

const state = { points: null, numbers: null, creds: null, map: null, mapMounted: false, playedIntro: false };

function setTab(id) {
  cur = id;
  [...tabsEl.children].forEach(b => b.classList.toggle('on', b.dataset.id === id));
  render();
}
tabsEl.innerHTML = TABS.map(([id, label]) => `<button data-id="${id}" class="${id === cur ? 'on' : ''}">${label}</button>`).join('');
tabsEl.addEventListener('click', e => { const b = e.target.closest('button'); if (b) setTab(b.dataset.id); });

// ---------- data ----------
async function getPoints() { if (!state.points) state.points = await api('/atlas/api/points', { label: 'map points' }); return state.points; }
async function getNumbers() { if (!state.numbers) state.numbers = await api('/atlas/api/numbers', { label: 'numbers' }); return state.numbers; }
async function getCreds() { if (!state.creds) state.creds = await api('/atlas/api/creds', { label: 'credential classes' }); return state.creds; }

function num(id) { return (state.numbers && state.numbers.numbers[id]) || null; }
function nval(id) { const r = num(id); return r ? r.value : null; }

// ---------- render router ----------
function render() {
  if (cur === 'map') return renderMap();
  if (cur === 'findings') return renderFindings();
  if (cur === 'creds') return renderCreds();
  if (cur === 'search') return renderSearch();
  if (cur === 'about') return renderAbout();
}

// ========== MAP ==========
async function renderMap() {
  view.innerHTML = `
    <div class="mapwrap">
      <canvas id="map"></canvas>
      <div class="map-hud">
        <div class="counter"><span id="counter">0</span><small id="counterSub">servers appearing by publication date</small></div>
        <div class="legend" id="legend"></div>
      </div>
      <div class="map-foot">
        <button class="btn small" id="playBtn">▶ Replay time-lapse</button>
        <div class="tl" id="tlLabel">—</div>
        <input type="range" id="timeSlider" min="0" max="100" value="100">
        <label style="display:flex;align-items:center;gap:6px;font-size:12px;color:var(--muted);white-space:nowrap"><input type="checkbox" id="secretOnly" style="width:auto"> secret-bearing only</label>
        <button class="btn small ghost" id="resetBtn">Reset view</button>
      </div>
    </div>
    <p class="pub-note" style="margin-top:10px">Each point is one server. Islands are topics: k-means (k = 48) over the ChloeDB embedding of each server's title and description, labelled with the words over-represented in that island, placed by a 2-D projection of the centroids and packed so none overlap; inside an island, servers nearest its centre sit in the middle. Colour is the credential class the server declares; dim points declare none. Hover to inspect, click a point for its card and generated binding, click a label to zoom; drag to pan, scroll to zoom.</p>`;

  const d = await getPoints();
  if (d.error) { view.querySelector('.mapwrap').innerHTML = `<div class="empty">The map is not loaded yet.<br>${esc(d.error)}</div>`; return; }
  const canvas = document.getElementById('map');
  const m = new MapView(canvas, d, { onPick: i => openCard(d.names[i]), onHover: showTip });
  state.map = m;

  // legend with counts
  const counts = new Array(d.bucketOrder.length).fill(0);
  for (let i = 0; i < d.n; i++) counts[d.cls[i]]++;
  const legend = document.getElementById('legend');
  legend.innerHTML = d.buckets.map(b => {
    const idx = d.bucketOrder.indexOf(b.key);
    return `<div class="lg" data-k="${b.key}"><i style="background:${b.color};color:${b.color}"></i>${esc(b.label)}<span class="ct">${fmt.compact(counts[idx])}</span></div>`;
  }).join('');
  legend.addEventListener('click', e => {
    const lg = e.target.closest('.lg'); if (!lg) return;
    const k = lg.dataset.k;
    if (m.filter.has(k)) m.filter.delete(k); else m.filter.add(k);
    lg.classList.toggle('off', !m.filter.has(k));
    m.draw();
  });

  const counter = document.getElementById('counter');
  const sub = document.getElementById('counterSub');
  const tlLabel = document.getElementById('tlLabel');
  const slider = document.getElementById('timeSlider');
  slider.max = String(d.day_span);
  const dayToDate = t => { const base = new Date(d.day_lo + 'T00:00:00Z'); base.setUTCDate(base.getUTCDate() + t); return base.toISOString().slice(0, 10); };
  m.onDraw(shown => { counter.textContent = fmt.n(shown); });
  function setTime(t) { m.setTime(t); slider.value = String(t); tlLabel.textContent = dayToDate(t); }

  slider.addEventListener('input', () => { setTime(+slider.value); sub.textContent = 'showing servers published on or before this date'; });
  document.getElementById('secretOnly').addEventListener('change', e => m.setSecretOnly(e.target.checked));
  document.getElementById('resetBtn').addEventListener('click', () => { m.fit(); m.draw(); });
  document.getElementById('playBtn').addEventListener('click', () => playIntro());

  // keep any highlight from the credentials tab
  if (state.highlight) m.setHighlight(state.highlight);

  function playIntro() {
    const span = d.day_span; const T = 4200; const t0 = performance.now();
    sub.textContent = 'servers appearing by publication date';
    function step(now) {
      const p = Math.min(1, (now - t0) / T); const e = p * p * (3 - 2 * p);
      setTime(Math.round(e * span));
      if (p < 1) requestAnimationFrame(step); else { setTime(span); sub.textContent = `${fmt.n(m.shown)} servers in the registry`; }
    }
    requestAnimationFrame(step);
  }
  if (!state.playedIntro && !state.highlight) { state.playedIntro = true; setTime(0); playIntro(); }
  else { setTime(d.day_span); }
}

function showTip(i, px, py) {
  const tip = document.getElementById('tip');
  if (i < 0 || !state.points) { tip.style.display = 'none'; return; }
  const d = state.points;
  const rect = document.getElementById('map').getBoundingClientRect();
  const b = d.buckets.find(x => x.key === d.bucketOrder[d.cls[i]]);
  tip.innerHTML = `<b>${esc(d.names[i])}</b><div class="cl">${esc(b ? b.label : '')}${d.grade[i] ? ' · grade ' + 'ABCDF'[d.grade[i] - 1] : ''}</div>`;
  tip.style.left = (rect.left + px) + 'px'; tip.style.top = (rect.top + py) + 'px'; tip.style.display = 'block';
}

// ========== SERVER CARD ==========
const card = document.getElementById('card');
document.getElementById('cardx').addEventListener('click', closeCard);
function closeCard() { card.classList.remove('open'); }
function nodwynChip(n) {
  if (!n) return '';
  if (!n.rows) return n.file ? '<span class="nod none">no rows: every secret here is unbound, denied by default (ADR 6)</span>' : '';
  if (n.rejected) return `<span class="nod bad" title="${esc(n.error)}">${n.rejected} of ${n.rows} rows rejected by Nodwyn's parser</span>`;
  if (n.unchecked) return '<span class="nod">not checked by Nodwyn\'s parser</span>';
  return `<span class="nod ok" title="${esc(n.parser)}">${n.accepted === 1 ? 'the row is' : `all ${n.accepted} rows`} accepted by Nodwyn's parser</span>`;
}

function gradeChip(g) { return g ? `<span class="gr ${g}">${g}</span>` : ''; }
function classDot(points, cls) { const c = (points && points.colors && points.colors[cls]) || '#888'; return `<span class="dot" style="background:${c}"></span>`; }

async function openCard(name) {
  card.classList.add('open');
  const body = document.getElementById('cardbody');
  body.innerHTML = `<div class="empty">Loading ${esc(name)}…</div>`;
  let data;
  try { data = await api('/atlas/api/server?name=' + encodeURIComponent(name), { label: 'server card' }); }
  catch (e) { body.innerHTML = `<div class="empty">${esc(e.message)}</div>`; return; }
  const s = data.server; const pts = state.points;
  const basisCount = { exact: 0, inferred: 0, unknown: 0 };
  data.bindings.forEach(b => basisCount[b.basis]++);
  body.innerHTML = `
    <div class="src-tag">${esc(s.publisher)}</div>
    <h2 style="margin:4px 0 2px">${esc(s.title || s.name)}</h2>
    <div class="row" style="gap:8px;margin-bottom:8px">
      <span class="chip ${s.has_secret ? 'warn' : ''}">${s.has_secret ? 'declares a secret' : 'no declared secret'}</span>
      ${s.grade ? `<span class="chip">MCPShield ${gradeChip(s.grade)} <span class="muted">(${s.score})</span></span>` : '<span class="chip">not in MCPShield scan</span>'}
      <span class="chip">${esc(s.transport || 'remote')}</span>
      ${(s.reg_types || []).map(t => `<span class="chip">${esc(t)}</span>`).join('')}
    </div>
    <p style="font-size:13.5px;color:var(--muted)">${esc(s.description || '')}</p>
    <dl class="kv" style="margin:10px 0 14px">
      <dt>name</dt><dd class="mono" style="font-size:12px">${esc(s.name)}</dd>
      <dt>published</dt><dd>${esc((s.published || '').slice(0, 10) || '—')}</dd>
      <dt>status</dt><dd>${esc(s.status)}</dd>
      ${s.repo_slug ? `<dt>repo</dt><dd class="mono" style="font-size:12px">${esc(s.repo_slug)}</dd>` : ''}
      ${s.remote_hosts && s.remote_hosts.length ? `<dt>remote</dt><dd class="mono" style="font-size:12px">${esc(s.remote_hosts.join(', '))}</dd>` : ''}
      <dt>capability</dt><dd id="capCell">${s.capability ? `<span class="cap">${esc(s.capability)}</span> <span class="src-tag">from the description, by a model</span>` : `<button class="btn small" id="classifyBtn">classify with Haiku</button>`}</dd>
    </dl>

    <h3 style="margin-bottom:6px">Generated <span class="mono">bindings.toml</span> <span class="muted" style="font-weight:400;font-size:13px">(Nodwyn's format, least privilege)</span></h3>
    <div class="row" style="gap:6px;margin-bottom:8px;flex-wrap:wrap">
      ${basisCount.exact ? `<span class="basis exact">${basisCount.exact} exact</span>` : ''}
      ${basisCount.inferred ? `<span class="basis inferred">${basisCount.inferred} inferred</span>` : ''}
      ${basisCount.unknown ? `<span class="basis unknown">${basisCount.unknown} unknown</span>` : ''}
      ${nodwynChip(data.nodwyn)}
      ${data.nodwyn && data.nodwyn.file ? `<button class="btn small" id="copyToml" style="margin-left:auto">Copy file</button>` : ''}
    </div>
    ${data.bindings.length ? data.bindings.map(b => `
      <div style="margin-bottom:10px">
        <div class="row" style="gap:6px;margin-bottom:4px;flex-wrap:wrap"><span class="basis ${b.basis}">${b.basis}</span><span class="mono" style="font-size:12px;color:var(--muted)">${esc(b.nodwyn_class || b.class)}${b.nodwyn_dest ? ' → ' + esc(b.nodwyn_dest) : b.dest ? ' → ' + esc(b.dest) : ''}</span>${b.nodwyn_row === 1 ? (b.nodwyn_ok === 1 ? '<span class="nod ok" title="Nodwyn binding.Parse accepted this row and decoded it back unchanged">accepted by Nodwyn\'s parser</span>' : b.nodwyn_ok === 0 ? `<span class="nod bad" title="${esc(b.nodwyn_err || '')}">rejected by Nodwyn's parser</span>` : '<span class="nod">not checked</span>') : '<span class="nod none">no row: denied by default</span>'}</div>
        <pre class="toml">${esc(b.toml)}</pre>
      </div>`).join('') : '<p class="empty">No declared credential. Nothing to bind.</p>'}

    ${data.bindings.length ? `<p class="pub-note" style="margin-top:2px">Rows are Nodwyn's <span class="mono">[[bind]]</span> table (class, dest, positions, methods), written as Nodwyn's Connections writes them; the basis is the comment on each row. Exact rows take the host and header from the record; inferred rows take the host from the variable-name rule shown. An unknown binding gets <b>no row</b>: Nodwyn denies a secret-bearing egress that no row binds (ADR 6), so that secret reaches no host. Each server's whole file is run through Nodwyn's own <span class="mono">binding.Parse</span> (Go, <span class="mono">internal/binding/binding.go</span> at cdc30069, Apache-2.0) at load.</p>` : ''}

    <div class="divider"></div>
    <h3 style="margin-bottom:4px">Blast radius</h3>
    <p class="pub-note" style="margin-bottom:8px">Servers reachable from this one through a shared sensitive credential (private key, password or database) — a ChloeDB <span class="mono">REACHABLE</span> walk over the <span class="mono">atlas_cocred</span> graph.</p>
    <div id="reachBox"><button class="btn small" id="reachBtn">Walk the graph</button></div>

    <div class="divider"></div>
    <h3 style="margin-bottom:6px">Similar servers <span class="muted" style="font-weight:400;font-size:12px">rrf(bm25, similarity)</span></h3>
    <div id="simList">${data.similar.map(x => `<div class="list-row" data-name="${esc(x.name)}">${classDot(pts, x.primary_class)}<span>${esc(x.title || x.name)}</span><span class="muted" style="margin-left:auto;font-size:11px">${esc(x.publisher)}</span></div>`).join('') || '<p class="empty">none</p>'}</div>

    <div class="divider"></div>
    <h3 style="margin-bottom:6px">More from ${esc(s.publisher)}</h3>
    <div id="pubList">${data.publisher_others.length ? data.publisher_others.map(x => `<div class="list-row" data-name="${esc(x.name)}">${classDot(pts, x.primary_class)}<span>${esc(x.title || x.name)}</span>${x.has_secret ? '<span class="chip warn" style="margin-left:auto">secret</span>' : ''}</div>`).join('') : '<p class="empty">no other servers from this publisher</p>'}</div>
  `;
  body.querySelectorAll('.list-row[data-name]').forEach(el => el.addEventListener('click', () => openCard(el.dataset.name)));
  const copyBtn = body.querySelector('#copyToml');
  if (copyBtn) copyBtn.onclick = async () => {
    try { await navigator.clipboard.writeText(data.nodwyn.file); copyBtn.textContent = 'Copied'; }
    catch { copyBtn.textContent = 'Copy failed'; }
  };
  const classifyBtn = body.querySelector('#classifyBtn');
  if (classifyBtn) classifyBtn.addEventListener('click', async () => {
    classifyBtn.textContent = 'classifying…'; classifyBtn.disabled = true;
    try {
      const r = await api('/atlas/api/classify?name=' + encodeURIComponent(name), { label: 'classify (Haiku)' });
      const cell = body.querySelector('#capCell');
      if (r.capability) cell.innerHTML = `<span class="cap">${esc(r.capability)}</span> <span class="src-tag">${r.live ? 'from the description, by Haiku' : 'recorded'}</span>`;
      else cell.innerHTML = `<span class="muted" style="font-size:12px">model unavailable (${esc(r.code || r.error || 'capped')}) — recorded results shown elsewhere</span>`;
    } catch (e) { toast('classify failed: ' + e.message); classifyBtn.disabled = false; classifyBtn.textContent = 'classify with Haiku'; }
  });
  const reachBtn = body.querySelector('#reachBtn');
  reachBtn.addEventListener('click', async () => {
    reachBtn.textContent = 'walking…'; reachBtn.disabled = true;
    const r = await api('/atlas/api/reachable?name=' + encodeURIComponent(name), { label: 'REACHABLE blast radius' });
    const box = body.querySelector('#reachBox');
    if (!r.cluster.length) { box.innerHTML = '<p class="empty">This server shares no sensitive credential class with another — its blast radius is itself.</p>'; return; }
    box.innerHTML = `<p style="font-size:13px;margin-bottom:6px"><b>${r.cluster.length}</b> other servers share a sensitive credential with this one.</p>` +
      `<div style="max-height:180px;overflow:auto">${r.cluster.slice(0, 60).map(x => `<div class="list-row" data-name="${esc(x.name)}">${classDot(pts, x.primary_class)}<span>${esc(x.title || x.name)}</span>${gradeChip(x.grade)}</div>`).join('')}</div>` +
      `<button class="btn small" id="litMap" style="margin-top:8px">Light this cluster on the map →</button>`;
    box.querySelectorAll('.list-row[data-name]').forEach(el => el.addEventListener('click', () => openCard(el.dataset.name)));
    box.querySelector('#litMap').addEventListener('click', () => {
      state.highlight = new Set([name, ...r.cluster.map(x => x.name)]);
      closeCard(); setTab('map'); setTimeout(() => state.map && state.map.setHighlight(state.highlight), 60);
    });
  });
}

// ========== FINDINGS ==========
async function renderFindings() {
  await getNumbers();
  const n = state.numbers.numbers;
  const fc = (id, { pctOf, fmtv } = {}) => {
    const r = n[id]; if (!r) return '';
    const val = fmtv ? fmtv(r.value) : fmt.n(r.value);
    const pct = r.pct != null ? ` <small>${fmt.pct(r.pct, r.pct < 0.1 ? 1 : 0)}${r.n ? ' of ' + fmt.compact(r.n) : ''}</small>` : '';
    return `<div class="fcard" data-id="${id}">
      <div class="v">${val}${pct}</div>
      <div class="lab">${esc(r.label)}</div>
      <div class="mt"><span class="src-tag">${esc(r.method)}</span>${r.model ? `<span class="chip info" style="padding:2px 6px">${esc(r.model)}</span>` : ''}<span class="sqlbtn" style="margin-left:auto">SQL ▾</span></div>
      <pre>${highlightSQL(r.sql_text || '')}</pre>
    </div>`;
  };
  const basis = { e: n.basis_exact, i: n.basis_inferred, u: n.basis_unknown };
  const total = (basis.e?.value || 0) + (basis.i?.value || 0) + (basis.u?.value || 0);
  const pctw = x => total ? (100 * x / total).toFixed(1) + '%' : '0';
  view.innerHTML = `
    <h2 style="margin-bottom:4px">What the registry declares</h2>
    <p class="lede" style="margin-bottom:16px">Every number is one row in <span class="mono">atlas_numbers</span>, computed by a SQL <span class="mono">COUNT</span>/<span class="mono">SUM</span> over the loaded tables at load time. Click <b>SQL</b> on any card for the statement. Nothing here is typed by hand.</p>

    <div class="findings" style="margin-bottom:22px">
      ${fc('servers')} ${fc('publishers')} ${fc('secret_servers')} ${fc('secret_env')}
      ${fc('unmarked')} ${fc('private_key')} ${fc('password')} ${fc('db_url')}
      ${fc('llm_key')} ${fc('since_oct')} ${fc('secret_header_servers')} ${fc('top5_share')}
      ${fc('near_dup')}
    </div>

    <div class="card" style="margin-bottom:22px">
      <h3>Bindings generated: exact, inferred, unknown — reported separately</h3>
      <p class="pub-note" style="margin-bottom:10px">A binding is <b>exact</b> when the record gives both host and position (a remote URL plus a declared secret header), <b>inferred</b> when a known variable-name pattern names the provider host, and <b>unknown</b> when a declared secret has no destination we can name — then it is unbound: denied by default. We never guess a host.</p>
      <div class="basis-bar">
        <div style="width:${pctw(basis.e?.value || 0)};background:var(--green)">${pctw(basis.e?.value || 0)} exact</div>
        <div style="width:${pctw(basis.i?.value || 0)};background:var(--cyan)">${pctw(basis.i?.value || 0)} inferred</div>
        <div style="width:${pctw(basis.u?.value || 0)};background:var(--red)">${pctw(basis.u?.value || 0)} unknown</div>
      </div>
      <div class="row" style="margin-top:10px;gap:20px;font-size:13px">
        <span><b>${fmt.n(basis.e?.value)}</b> exact</span>
        <span><b>${fmt.n(basis.i?.value)}</b> inferred</span>
        <span><b>${fmt.n(basis.u?.value)}</b> unknown (denied by default)</span>
        <span class="muted">of ${fmt.n(total)} bindings across ${fmt.n(nval('secret_servers'))} secret-bearing servers</span>
      </div>
    </div>

    ${n.nodwyn_rows ? `<div class="card" style="margin-bottom:22px">
      <h3>As Nodwyn <span class="mono">[[bind]]</span> rows, checked by Nodwyn's own parser</h3>
      <p class="pub-note" style="margin-bottom:12px">Each exact or inferred binding with a host name is written as a row of Nodwyn's <span class="mono">bindings.toml</span> (class, dest, positions, methods; the basis as a comment), the way Nodwyn's Connections writes a key's rows. Each server's whole file is then run through Nodwyn's <span class="mono">binding.Parse</span>, the Go file copied unmodified from Nodwyn (commit cdc30069, Apache-2.0), and a row counts as accepted only if Parse decodes it back unchanged. An unknown binding gets no row: Nodwyn denies a secret-bearing egress that no row binds (ADR 6).</p>
      <div class="findings">
        ${fc('nodwyn_accepted')} ${fc('nodwyn_rows')} ${fc('nodwyn_servers_accepted')}
        ${fc('nodwyn_no_host')} ${fc('nodwyn_wildcard')} ${fc('nodwyn_bearer_assumed')}
      </div>
    </div>` : ''}

    ${n.cap_classified ? `<div class="card" style="margin-bottom:22px">
      <h3>What the secret-bearing servers can do — from the description, by a model</h3>
      <p class="pub-note" style="margin-bottom:12px"><b>${fmt.n(n.cap_classified.value)}</b> of the ${fmt.n(nval('secret_servers'))} secret-bearing servers, labelled by <span class="mono">claude-haiku-5-5</span> with what the <i>description</i> implies the server can do with its credential. The map and the bindings use no model.</p>
      <div id="capBars"></div>
      <p class="pub-note" style="margin-top:10px">ChloeDB's native form (run here through the spend-capped client): <span class="mono">llm('anthropic/claude-haiku-5-5', '…', description, choices =&gt; 'read,write,execute,send,pay,unclear')</span>.</p>
    </div>` : ''}

    <div class="card">
      <h3>Risk overlay — MCPShield grades, for the repositories that join</h3>
      <p class="pub-note" style="margin-bottom:12px"><b>${fmt.n(nval('graded_repos'))} of ${fmt.n(nval('servers'))}</b> registry servers have a GitHub repo that appears in the MCPShield 2026 scan. These are <span class="mono">MCPShield</span>'s grades, not ours. Most of the registry is simply unscanned.</p>
      <div id="gradeBars"></div>
    </div>`;
  view.querySelectorAll('.fcard').forEach(c => c.querySelector('.sqlbtn').addEventListener('click', () => c.classList.toggle('show')));
  // capability bars (only if measured)
  const capBox = document.getElementById('capBars');
  if (capBox) {
    const caps = ['read', 'write', 'execute', 'send', 'pay', 'unclear'];
    const ccol = { read: 'var(--cyan)', write: 'var(--amber)', execute: 'var(--indigo)', send: 'var(--violet)', pay: 'var(--green)', unclear: 'var(--faint)' };
    const cmax = Math.max(...caps.map(c => nval('cap_' + c) || 0), 1);
    capBox.innerHTML = caps.map(c => {
      const v = nval('cap_' + c) || 0;
      return `<div style="display:grid;grid-template-columns:76px 1fr 56px;gap:10px;align-items:center;margin-bottom:7px"><span class="cap" style="justify-self:start">${c}</span><span style="background:var(--panel-2);border-radius:6px;height:22px;overflow:hidden"><span style="display:block;height:100%;width:${(100 * v / cmax).toFixed(1)}%;background:${ccol[c]}"></span></span><span class="num mono" style="text-align:right">${fmt.n(v)}</span></div>`;
    }).join('');
  }
  // grade bars
  const grades = ['A', 'B', 'C', 'D', 'F'];
  const gcol = { A: 'var(--green)', B: 'var(--cyan)', C: 'var(--amber)', D: '#fb923c', F: 'var(--red)' };
  const gmax = Math.max(...grades.map(g => nval('grade_' + g) || 0), 1);
  document.getElementById('gradeBars').innerHTML = grades.map(g => {
    const v = nval('grade_' + g) || 0;
    return `<div style="display:grid;grid-template-columns:24px 1fr 56px;gap:10px;align-items:center;margin-bottom:7px"><span class="gr ${g}">${g}</span><span style="background:var(--panel-2);border-radius:6px;height:22px;overflow:hidden"><span style="display:block;height:100%;width:${(100 * v / gmax).toFixed(1)}%;background:${gcol[g]}"></span></span><span class="num mono" style="text-align:right">${fmt.n(v)}</span></div>`;
  }).join('');
}

// ========== CREDENTIALS ==========
async function renderCreds() {
  const data = await getCreds();
  const pts = state.points;
  view.innerHTML = `
    <h2 style="margin-bottom:4px">Pick a credential. See who asks for it — and what one leaked token would reach.</h2>
    <p class="lede" style="margin-bottom:16px">Each class is a node in <span class="mono">atlas_creds</span>; each server-to-credential binding is an edge in <span class="mono">atlas_bindings</span>. Choosing a class runs a join to light up every server that declares it.</p>
    <div class="grid" style="grid-template-columns:300px 1fr;gap:18px;align-items:start">
      <div class="card tight" style="max-height:640px;overflow:auto">
        <input type="search" id="credSearch" placeholder="filter classes…" style="margin-bottom:10px">
        <div id="credList"></div>
      </div>
      <div id="credDetail" class="card"><p class="empty">Choose a credential class on the left.</p></div>
    </div>`;
  const colors = data.colors;
  const listEl = document.getElementById('credList');
  function paint(filter = '') {
    listEl.innerHTML = data.creds.filter(c => !filter || c.class.includes(filter) || (c.label || '').toLowerCase().includes(filter))
      .map(c => `<div class="list-row" data-c="${esc(c.class)}"><span class="dot" style="background:${colors[c.bucket] || '#888'}"></span><span>${esc(c.class)}</span><span class="muted" style="margin-left:auto;font-size:11px">${fmt.n(c.servers)}</span></div>`).join('');
    listEl.querySelectorAll('.list-row').forEach(el => el.addEventListener('click', () => selectClass(el.dataset.c)));
  }
  paint();
  document.getElementById('credSearch').addEventListener('input', e => paint(e.target.value.toLowerCase().trim()));
  // auto-select the most security-relevant example
  const pref = data.creds.find(c => c.class === 'github') || data.creds.find(c => c.bucket === 'vcs') || data.creds[0];
  if (pref) selectClass(pref.class);

  async function selectClass(cls) {
    const det = document.getElementById('credDetail');
    det.innerHTML = `<p class="empty">Loading ${esc(cls)}…</p>`;
    const r = await api('/atlas/api/cred?class=' + encodeURIComponent(cls), { label: 'credential ' + cls });
    const info = r.info;
    state.highlight = new Set(r.servers.map(s => s.name));
    if (state.map) state.map.setHighlight(state.highlight);
    det.innerHTML = `
      <div class="row spread" style="align-items:flex-start">
        <div>
          <div class="src-tag">${esc(info.bucket || '')}</div>
          <h2 style="margin:2px 0 2px">${esc(info.class)}</h2>
          <div class="muted" style="font-size:13px">${esc(info.label || '')}${info.dest_host ? ' · binds to <span class="mono">' + esc(info.dest_host) + '</span>' : ' · no public destination host'}</div>
        </div>
        <button class="btn small" id="litMap2">Light on the map →</button>
      </div>
      <div class="kpis" style="margin:14px 0">
        <div class="stat"><div class="v">${fmt.n(info.servers || r.servers.length)}</div><div class="k">servers declare it</div></div>
        <div class="stat"><div class="v">${fmt.n(r.reaches.length)}</div><div class="k">distinct destinations one token can reach</div></div>
      </div>
      ${(info.example_names || []).length ? `<div style="margin-bottom:12px"><div class="src-tag" style="margin-bottom:6px">example variable names</div><div class="caps">${info.example_names.map(x => `<span class="cap mono">${esc(x)}</span>`).join('')}</div></div>` : ''}
      ${r.reaches.length ? `<div style="margin-bottom:12px"><div class="src-tag" style="margin-bottom:6px">what one leaked token would reach</div><div class="caps">${r.reaches.map(h => `<span class="cap mono">${esc(h)}</span>`).join('')}</div></div>` : `<p class="pub-note">This class has no known destination host, so the Atlas binds it to nothing: <b>denied by default</b>. A leaked value reaches whatever the server chooses — which is exactly why least privilege denies it.</p>`}
      <div class="src-tag" style="margin:10px 0 6px">${fmt.n(r.servers.length)} servers (click to open)</div>
      <div style="max-height:320px;overflow:auto">
        ${r.servers.slice(0, 200).map(s => `<div class="list-row" data-name="${esc(s.name)}"><span class="dot" style="background:${colors[s.primary_class] || '#888'}"></span><span>${esc(s.title || s.name)}</span><span class="basis ${s.basis}" style="margin-left:auto">${s.basis}</span>${gradeChip(s.grade)}</div>`).join('')}
      </div>`;
    det.querySelectorAll('.list-row[data-name]').forEach(el => el.addEventListener('click', () => openCard(el.dataset.name)));
    det.querySelector('#litMap2').addEventListener('click', () => { setTab('map'); setTimeout(() => state.map && state.map.setHighlight(state.highlight), 60); });
  }
}

// ========== SEARCH ==========
async function renderSearch() {
  await getCreds();
  const creds = state.creds.creds;
  view.innerHTML = `
    <h2 style="margin-bottom:4px">Search by words and meaning</h2>
    <p class="lede" style="margin-bottom:14px">Hybrid ranking in one SQL statement: <span class="mono">rrf(bm25(description, :q), similarity(emb, :q))</span>, capped at <span class="mono">MAX 2 PER publisher</span>. Add a credential or capability filter to narrow it.</p>
    <div class="card" style="margin-bottom:16px">
      <div class="row" style="gap:10px">
        <input type="search" id="q" placeholder="e.g. send email, browse the web, query a database…" style="flex:2;min-width:240px">
        <select id="credSel" style="flex:1;min-width:150px"><option value="">any credential</option>${creds.slice(0, 60).map(c => `<option value="${esc(c.class)}">${esc(c.class)} (${c.servers})</option>`).join('')}</select>
        <select id="capSel" style="flex:1;min-width:130px"><option value="">any capability</option>${['read', 'write', 'execute', 'send', 'pay', 'unclear'].map(c => `<option>${c}</option>`).join('')}</select>
        <button class="btn primary" id="go">Search</button>
      </div>
      <div class="row" style="gap:8px;margin-top:10px">
        <span class="muted" style="font-size:12px">try:</span>
        <button class="btn small ghost ex" data-q="send email" data-cred="google-gemini">send email + Google credential</button>
        <button class="btn small ghost ex" data-q="query a sql database" data-cred="database">query a database</button>
        <button class="btn small ghost ex" data-q="sign a blockchain transaction" data-cred="private-key">sign a transaction</button>
        <button class="btn small ghost ex" data-q="browse the web and take screenshots" data-cred="">browse the web</button>
      </div>
    </div>
    <div id="results"></div>`;
  const run = async () => {
    const q = document.getElementById('q').value.trim();
    const cred = document.getElementById('credSel').value;
    const cap = document.getElementById('capSel').value;
    const resEl = document.getElementById('results');
    resEl.innerHTML = '<p class="empty">Searching…</p>';
    const r = await api(`/atlas/api/search?q=${encodeURIComponent(q)}&cred=${encodeURIComponent(cred)}&cap=${encodeURIComponent(cap)}`, { label: 'hybrid search' });
    if (!r.results.length) { resEl.innerHTML = '<p class="empty">No matches.</p>'; return; }
    const pts = state.points;
    resEl.innerHTML = `<div class="src-tag" style="margin-bottom:8px">${r.results.length} results</div>` + r.results.map(s => `
      <div class="list-row" data-name="${esc(s.name)}" style="padding:10px 0">
        ${classDot(pts, s.primary_class)}
        <div><div>${esc(s.title || s.name)}</div><div class="muted" style="font-size:11px">${esc(s.publisher)}</div></div>
        <span style="margin-left:auto" class="row">${s.has_secret ? '<span class="chip warn">secret</span>' : ''}${gradeChip(s.grade)}</span>
      </div>`).join('');
    resEl.querySelectorAll('.list-row[data-name]').forEach(el => el.addEventListener('click', () => openCard(el.dataset.name)));
  };
  document.getElementById('go').addEventListener('click', run);
  document.getElementById('q').addEventListener('keydown', e => { if (e.key === 'Enter') run(); });
  view.querySelectorAll('.ex').forEach(b => b.addEventListener('click', () => {
    document.getElementById('q').value = b.dataset.q;
    document.getElementById('credSel').value = b.dataset.cred || '';
    run();
  }));
  run();
}

// ========== ABOUT ==========
async function renderAbout() {
  await getNumbers();
  view.innerHTML = `
    <div class="card" style="max-width:860px">
      <h2>Method, data and what this does not claim</h2>
      <p>The Atlas maps the <b>Model Context Protocol server registry</b> on ChloeDB. Each point is one server, placed on a topic island: the ChloeDB embedding of its title and description (potion-base-8M, computed in the database with <span class="mono">embed()</span>) is clustered by k-means (k = 48, fixed seed) in Node at load; each island is labelled with the words most over-represented in it (frequent there, rare elsewhere, and not from a single publisher), islands are placed by a 2-D projection of their centroids and pushed apart, and each server sits nearer its island's centre the closer it is to the centroid. Points are coloured by the credential class it <b>declares</b> in its record. The Atlas states what a record says. It never calls a server malicious, and it invents no risk score of its own.</p>

      <h3>Data sources and licences</h3>
      <ul style="font-size:14px;line-height:1.7">
        <li><b>MCP registry</b> — <span class="mono">servers-latest.jsonl</span>, ${fmt.n(nval('servers'))} servers (the authoritative source). The registry states <b>no data licence</b>; it is published by the Model Context Protocol project as the official registry. We treat it as public record and reproduce only declared metadata.</li>
        <li><b>MCPShield security scan 2026</b> — grades A–F for GitHub repositories, licence <b>CC-BY-4.0</b>. Only <b>${fmt.n(nval('graded_repos'))} of ${fmt.n(nval('servers'))}</b> registry servers join it by GitHub slug; the rest are unscanned. Grades shown are MCPShield's, attributed as such.</li>
        <li><b>Secret provider formats</b> — <span class="mono">provider-formats.jsonl</span> (Nosey Parker's and gitleaks's rule sets, Apache-2.0 and MIT). Consulted only for the names of the credential classes and the providers' hosts in the inference map.</li>
      </ul>

      <h3>How the bindings are made</h3>
      <p><b>Exact</b> (${fmt.n(nval('basis_exact'))}): a remote URL plus a declared secret header — host and position both in the record. <b>Inferred</b> (${fmt.n(nval('basis_inferred'))}): a curated map from a well-known environment-variable name pattern to a provider host, shown with the rule that produced it. <b>Unknown</b> (${fmt.n(nval('basis_unknown'))}): a declared secret with no destination we can name — bound to nothing, <i>denied by default</i>. Every binding row shows its basis; we never guess a host.</p>
      <p><b>Nodwyn's format.</b> Each exact or inferred binding with a host name is a <span class="mono">[[bind]]</span> row of Nodwyn's <span class="mono">bindings.toml</span>: for a remote server, class <span class="mono">&lt;server&gt;-key</span>, its URL's host, the secret header as the position (<span class="mono">header:Authorization:Bearer</span> when the record names no scheme, Nodwyn's own default) and <span class="mono">POST</span> (plus <span class="mono">GET</span> for SSE); for a contained server, the provider's host, <span class="mono">env:NAME</span> and any method. A URL whose subdomain is a variable, and a provider that serves each account on its own subdomain, bind <span class="mono">*.domain</span>. Each server's file is checked with Nodwyn's own <span class="mono">binding.Parse</span> (${fmt.n(nval('nodwyn_accepted'))} of ${fmt.n(nval('nodwyn_rows'))} rows accepted). That shows the rows are well-formed Nodwyn policy, not that a server works under them.</p>

      <h3>The model's one job</h3>
      <p>The ${fmt.n(nval('secret_servers'))} secret-bearing servers are each labelled with what the <i>description</i> says the server can do — read, write, execute, send, pay or unclear — by <span class="mono">claude-haiku-5-5</span> (about $1 for the batch), labelled everywhere as <i>"from the description, by a model"</i>. The map, the counts and the bindings use no model.</p>

      <h3>ChloeDB features on show</h3>
      <p><span class="mono">embed()</span> and vectors in the database; hybrid <span class="mono">rrf(bm25, similarity)</span> search with <span class="mono">MAX n PER publisher</span>; a <span class="mono">REACHABLE</span> walk over the <span class="mono">atlas_cocred</span> edge graph for blast radius; every view's SQL in the drawer, bottom-right.</p>

      <h3>What this does not show</h3>
      <p>It does not show that any server is malicious or compromised — only what it declares. The near-duplicate figure samples the largest publishers (the five biggest hold ${fmt.pct(num('top5_share')?.pct || 0, 0)} of the registry), so it is a floor, not an exact count. Islands are k-means clusters of 256-dimensional embeddings and their labels are word statistics, not a taxonomy: proximity is a hint, not a proof. Grades cover only the ${fmt.n(nval('graded_repos'))} scanned repositories.</p>
    </div>`;
}

// ---------- boot ----------
(async function boot() {
  try { await getNumbers(); } catch {}
  render();
})();
