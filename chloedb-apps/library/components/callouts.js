// title: Callouts
// description: Cards that each ask for attention: a severity label at the top ("Act on this", "Worth knowing", "Helpful"), a title, the rows it is about as chips joined by a sign, a plain explanation and a highlighted tip saying what to do. For warnings, clashes, risks, advice and findings, where a table row would bury what to do.
// props: items: [{tone ('bad' | 'warn' | 'ok' | 'info'), label? (the severity words, default by tone: 'Act on this', 'Worth knowing', 'Helpful', 'Note'), title,
//   chips?: [texts] (the rows it is about), join? (the sign between chips, default '↔'), text? (the explanation), tip? (what to do), tipLabel? (default 'Tip'),
//   actions?: [{label, onClick(item)}]}] most pressing first; empty: text when there are none (default "Nothing to flag")
function component(props) {
  css('sa-callouts', `
.sa-callouts{display:flex;flex-direction:column;gap:12px}
.sa-callout{position:relative;background:var(--card);border:1px solid var(--card-border,var(--line));border-left:4px solid var(--c);border-radius:var(--radius);padding:14px 16px;display:flex;flex-direction:column;gap:8px;box-shadow:var(--card-shadow,none)}
.sa-callout-label{display:flex;align-items:center;gap:7px;font-size:.78em;font-weight:750;letter-spacing:.08em;text-transform:uppercase;color:var(--c)}
.sa-callout-label svg{width:14px;height:14px}
.sa-callout-title{font-family:var(--font-title);font-size:1.12em;font-weight:650;margin:0}
.sa-callout-chips{display:flex;flex-wrap:wrap;align-items:center;gap:6px}
.sa-callout-chip{font-size:.88em;font-weight:600;padding:.25em .7em;border-radius:999px;background:var(--soft,color-mix(in srgb,var(--fg) 8%,transparent));color:var(--fg)}
.sa-callout-join{color:var(--muted);font-size:.9em}
.sa-callout-text{margin:0;line-height:1.5}
.sa-callout-tip{margin:0;line-height:1.5;font-weight:600;color:var(--accent)}
.sa-callout-tip b{color:var(--fg);font-weight:600;margin-right:.3em}
.sa-callout-acts{display:flex;gap:6px;flex-wrap:wrap}
`);
  const T = { bad: ['var(--bad)', 'Act on this'], warn: ['var(--warn)', 'Worth knowing'], ok: ['var(--ok)', 'Helpful'], info: ['var(--info)', 'Note'] };
  const flag = () => { const s = document.createElementNS('http://www.w3.org/2000/svg', 'svg'); s.setAttribute('viewBox', '0 0 24 24'); s.setAttribute('fill', 'none'); s.setAttribute('stroke', 'currentColor'); s.setAttribute('stroke-width', '2.2'); s.innerHTML = '<path d="M5 21V4M5 4h11l-2 4 2 4H5"/>'; return s; };
  const items = (Array.isArray(props.items) ? props.items : []).filter((x) => x && typeof x === 'object');
  if (!items.length) return h('div', { class: 'empty' }, props.empty || 'Nothing to flag');
  return h('div', { class: 'sa-callouts' }, items.map((it) => {
    const [c, word] = T[it.tone] || T.info;
    const chips = (Array.isArray(it.chips) ? it.chips : []).filter((x) => x != null && x !== '').map(String);
    const join = it.join || '↔';
    const acts = (Array.isArray(it.actions) ? it.actions : []).filter((a) => a && a.label).map((a) => h('button', { type: 'button', class: 'small', onclick: () => typeof a.onClick === 'function' && a.onClick(it) }, String(a.label)));
    return h('article', { class: 'sa-callout', style: `--c:${c}` },
      h('div', { class: 'sa-callout-label' }, it.tone === 'bad' || it.tone === 'warn' ? flag() : null, String(it.label || word)),
      it.title ? h('h3', { class: 'sa-callout-title' }, String(it.title)) : null,
      chips.length ? h('div', { class: 'sa-callout-chips' }, chips.flatMap((x, i) => [i ? h('span', { class: 'sa-callout-join', 'aria-hidden': 'true' }, join) : null, h('span', { class: 'sa-callout-chip' }, x)]).filter(Boolean)) : null,
      it.text ? h('p', { class: 'sa-callout-text' }, String(it.text)) : null,
      it.tip ? h('p', { class: 'sa-callout-tip' }, h('b', null, `${it.tipLabel || 'Tip'}:`), String(it.tip)) : null,
      acts.length ? h('div', { class: 'sa-callout-acts' }, acts) : null);
  }));
}
