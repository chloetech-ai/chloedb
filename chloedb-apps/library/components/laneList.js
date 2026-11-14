// title: Lane list
// description: Cards grouped into lanes along a rail, one lane per value of a meaningful dimension (time of day, stage, day, room): a ring and the lane's name with a short note, then its cards, each with its name, a detail line, small badges that point at related rows ("Overlaps with Room B") and its actions on the right. For a person's own routine, a pipeline or a schedule, where a flat table would hide the shape of the day.
// props: items: [{id, lane (the lane it belongs to), title, detail? (short text drawn in the mono face, a dose or a size), sub? (muted text after it),
//   badges?: [text | {text, tone? ('warn' | 'bad' | 'ok' | 'info')}], actions?: [{label, onClick(item), style? ('primary' | 'quiet')}], onClick?(item)}];
//   lanes: [name | {name, note?}] in their order (default: lanes in the order first seen; a lane with no items is not shown unless listed with show: true);
//   empty: text when there are no items (default "Nothing yet"); other: the lane name for items without one (default "Other")
function component(props) {
  css('sa-lanes', `
.sa-lanes{display:flex;flex-direction:column;gap:22px}
.sa-lane{position:relative;padding-left:26px}
.sa-lane::before{content:"";position:absolute;left:8px;top:22px;bottom:0;width:2px;background:var(--line);border-radius:1px}
.sa-lane-head{display:flex;align-items:baseline;gap:10px;margin:0 0 10px -26px}
.sa-lane-ring{flex:none;width:18px;height:18px;border-radius:50%;border:3px solid var(--accent);background:var(--bg);align-self:center}
.sa-lane-name{font-family:var(--font-title);font-size:1.2em;font-weight:650;margin:0}
.sa-lane-note{color:var(--muted);font-size:.9em}
.sa-lane-cards{display:flex;flex-direction:column;gap:10px}
.sa-lane-card{display:flex;align-items:flex-start;gap:12px;background:var(--card);border:1px solid var(--card-border,var(--line));border-radius:var(--radius);padding:14px 16px;box-shadow:var(--card-shadow,none)}
.sa-lane-card.link{cursor:pointer}
.sa-lane-main{flex:1;min-width:0;display:flex;flex-direction:column;gap:4px}
.sa-lane-title{font-weight:650;font-size:1.02em}
.sa-lane-line{display:flex;flex-wrap:wrap;gap:4px 10px;align-items:baseline;font-size:.92em}
.sa-lane-detail{font-family:var(--mono);color:var(--fg)}
.sa-lane-sub{color:var(--muted)}
.sa-lane-badges{display:flex;flex-wrap:wrap;gap:6px;margin-top:4px}
.sa-lane-badge{display:inline-flex;align-items:center;gap:6px;font-size:.86em;font-weight:600;padding:.25em .7em;border-radius:999px;background:color-mix(in srgb,var(--c) 14%,transparent);color:var(--c)}
.sa-lane-badge svg{width:13px;height:13px;flex:none}
.sa-lane-acts{display:flex;flex-wrap:wrap;gap:6px;flex:none;align-items:center}
.sa-lane-acts button{min-height:40px;border-radius:999px;padding:0 16px}
.sa-lane-acts button:not(.primary){background:transparent;color:var(--fg);border-color:transparent;box-shadow:inset 0 0 0 1.5px color-mix(in srgb,var(--fg) 26%,transparent)}
.sa-lane-acts button:not(.primary):hover{background:color-mix(in srgb,var(--fg) 7%,transparent)}
@media (max-width:520px){.sa-lane-card{flex-wrap:wrap}.sa-lane-acts{width:100%;justify-content:flex-end}}
`);
  const tones = { warn: 'var(--warn)', bad: 'var(--bad)', ok: 'var(--ok)', info: 'var(--info)' };
  const flag = () => { const s = document.createElementNS('http://www.w3.org/2000/svg', 'svg'); s.setAttribute('viewBox', '0 0 24 24'); s.setAttribute('fill', 'none'); s.setAttribute('stroke', 'currentColor'); s.setAttribute('stroke-width', '2'); s.innerHTML = '<path d="M5 21V4M5 4h11l-2 4 2 4H5"/>'; return s; };
  const other = props.other || 'Other';
  const items = (Array.isArray(props.items) ? props.items : []).filter((x) => x && typeof x === 'object');
  const listed = (Array.isArray(props.lanes) ? props.lanes : []).map((l) => (l && typeof l === 'object' ? { name: String(l.name ?? ''), note: l.note || '', show: !!l.show } : { name: String(l ?? ''), note: '', show: false })).filter((l) => l.name);
  const lanes = [...listed];
  for (const it of items) { const n = String(it.lane ?? '') || other; if (!lanes.some((l) => l.name === n)) lanes.push({ name: n, note: '', show: false }); }
  if (!items.length && !lanes.some((l) => l.show)) return h('div', { class: 'empty' }, props.empty || 'Nothing yet');
  const badge = (b) => {
    const o = b && typeof b === 'object' ? b : { text: String(b ?? '') };
    if (!o.text) return null;
    const tone = tones[o.tone] || (o.tone ? tones.info : 'var(--warn)');
    return h('span', { class: 'sa-lane-badge', style: `--c:${tone}` }, (!o.tone || o.tone === 'warn' || o.tone === 'bad') ? flag() : null, String(o.text));
  };
  const card = (it) => {
    const acts = (Array.isArray(it.actions) ? it.actions : []).filter((a) => a && a.label).map((a) => h('button', {
      type: 'button', class: a.style === 'primary' ? 'primary' : '', onclick: (e) => { e.stopPropagation(); if (typeof a.onClick === 'function') a.onClick(it); },
    }, String(a.label)));
    const line = [it.detail != null && it.detail !== '' ? h('span', { class: 'sa-lane-detail' }, String(it.detail)) : null, it.sub ? h('span', { class: 'sa-lane-sub' }, String(it.sub)) : null].filter(Boolean);
    const badges = (Array.isArray(it.badges) ? it.badges : []).map(badge).filter(Boolean);
    const el = h('div', { class: `sa-lane-card${typeof it.onClick === 'function' ? ' link' : ''}` },
      h('div', { class: 'sa-lane-main' }, h('div', { class: 'sa-lane-title' }, String(it.title ?? '')), line.length ? h('div', { class: 'sa-lane-line' }, line) : null, badges.length ? h('div', { class: 'sa-lane-badges' }, badges) : null),
      acts.length ? h('div', { class: 'sa-lane-acts' }, acts) : null);
    if (typeof it.onClick === 'function') el.addEventListener('click', () => it.onClick(it));
    return el;
  };
  return h('div', { class: 'sa-lanes' }, lanes.map((l) => {
    const mine = items.filter((it) => (String(it.lane ?? '') || other) === l.name);
    if (!mine.length && !l.show) return null;
    return h('section', { class: 'sa-lane' },
      h('div', { class: 'sa-lane-head' }, h('span', { class: 'sa-lane-ring', 'aria-hidden': 'true' }), h('h3', { class: 'sa-lane-name' }, l.name), l.note ? h('span', { class: 'sa-lane-note' }, l.note) : null),
      h('div', { class: 'sa-lane-cards' }, mine.length ? mine.map(card) : h('div', { class: 'empty small' }, props.empty || 'Nothing yet')));
  }));
}
