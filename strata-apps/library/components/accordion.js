// title: Accordion
// description: Sections that open and close under their titles, one at a time or several at once; for FAQs, settings groups, details per day or per category, and long forms split into parts.
// props: items: [{id, title, subtitle?, content: an element, a string or () => element (called the first time the section opens)}];
//   open: [ids] of the sections open at the start (default none; a single id works too); onToggle(id, isOpen) when a section opens or closes
//   (with multiple false, opening one also reports each section it closes, before the one it opens);
//   multiple: true to let several be open at once (default true; when false, only the first of open starts open)
function component(props) {
  css('sa-accordion', `
.sa-accordion{border:1px solid var(--line);border-radius:var(--radius);background:var(--card);overflow:hidden;min-width:0}
.sa-accordion-section+.sa-accordion-section{border-top:1px solid var(--line)}
.sa-accordion-heading{margin:0;font:inherit}
.sa-accordion-head{display:flex;align-items:center;gap:12px;width:100%;margin:0;padding:12px 14px;border:0;border-radius:0;background:none;box-shadow:none;color:var(--fg);font:inherit;text-align:left;cursor:pointer}
.sa-accordion-head:hover{background:color-mix(in srgb,var(--fg) 4%,transparent)}
.sa-accordion-head:focus-visible{outline:2px solid var(--accent);outline-offset:-2px}
.sa-accordion-text{display:flex;flex-direction:column;gap:2px;flex:1;min-width:0;overflow-wrap:anywhere}
.sa-accordion-title{font-weight:600}
.sa-accordion-sub{color:var(--muted);font-size:.875em}
.sa-accordion-chev{flex:none;width:8px;height:8px;margin:0 4px 4px 0;border-right:2px solid var(--muted);border-bottom:2px solid var(--muted);transform:rotate(45deg);transition:transform .15s}
.sa-accordion-head[aria-expanded="true"] .sa-accordion-chev{transform:rotate(-135deg);margin:4px 4px 0 0}
.sa-accordion-panel{padding:0 14px 14px;min-width:0;overflow-wrap:anywhere}
.sa-accordion-panel[hidden]{display:none}
@media (prefers-reduced-motion:reduce){.sa-accordion-chev{transition:none}}
`);
  const items = (Array.isArray(props.items) ? props.items : []).filter(it => it && typeof it === 'object');
  const multiple = props.multiple !== false;
  const keyOf = (it, i) => (it.id != null ? String(it.id) : String(i));
  const keys = items.map(keyOf);
  const idByKey = new Map(items.map((it, i) => [keys[i], it.id != null ? it.id : i]));
  const given = Array.isArray(props.open) ? props.open : props.open != null ? [props.open] : [];
  let open = new Set(given.map(String).filter(k => keys.includes(k)));
  if (!multiple && open.size > 1) open = new Set([[...open][0]]);

  const uid = 'sa-acc-' + Math.random().toString(36).slice(2, 9);
  const root = h('div', { class: 'sa-accordion' });
  const parts = new Map();

  function fill(k) {
    const p = parts.get(k);
    if (p.filled) return;
    p.filled = true;
    let c = p.item.content;
    try { c = typeof c === 'function' ? c() : c; }
    catch (e) { c = h('p', { class: 'muted small' }, 'This section could not be shown.'); }
    if (c != null && c !== false) p.panel.append(c);
  }

  function show(k, isOpen) {
    const p = parts.get(k);
    if (isOpen) fill(k);
    p.head.setAttribute('aria-expanded', isOpen ? 'true' : 'false');
    p.panel.hidden = !isOpen;
  }

  function toggle(k) {
    const willOpen = !open.has(k);
    const report = [];
    if (willOpen) {
      if (!multiple) for (const o of [...open]) { open.delete(o); show(o, false); report.push([o, false]); }
      open.add(k);
    } else open.delete(k);
    show(k, willOpen);
    report.push([k, willOpen]);
    if (typeof props.onToggle === 'function') for (const [o, s] of report) props.onToggle(idByKey.get(o), s);
  }

  items.forEach((item, i) => {
    const k = keys[i];
    const hid = `${uid}-h${i}`, pid = `${uid}-p${i}`;
    const head = h('button', { type: 'button', id: hid, class: 'sa-accordion-head', 'aria-controls': pid, 'aria-expanded': 'false', onclick: () => toggle(k) },
      h('span', { class: 'sa-accordion-text' },
        h('span', { class: 'sa-accordion-title' }, item.title != null && item.title !== '' ? String(item.title) : 'Untitled'),
        item.subtitle != null && item.subtitle !== '' ? h('span', { class: 'sa-accordion-sub' }, String(item.subtitle)) : null),
      h('span', { class: 'sa-accordion-chev', 'aria-hidden': 'true' }));
    const panel = h('div', { id: pid, class: 'sa-accordion-panel', role: 'region', 'aria-labelledby': hid });
    parts.set(k, { item, head, panel, filled: false });
    root.append(h('div', { class: 'sa-accordion-section' }, h('h3', { class: 'sa-accordion-heading' }, head), panel));
    show(k, open.has(k));
  });

  root.addEventListener('keydown', e => {
    const heads = [...parts.values()].map(p => p.head);
    const at = heads.indexOf(e.target);
    if (at < 0) return;
    let to = -1;
    if (e.key === 'ArrowDown') to = (at + 1) % heads.length;
    else if (e.key === 'ArrowUp') to = (at - 1 + heads.length) % heads.length;
    else if (e.key === 'Home') to = 0;
    else if (e.key === 'End') to = heads.length - 1;
    if (to < 0) return;
    e.preventDefault();
    heads[to].focus();
  });

  return root;
}
