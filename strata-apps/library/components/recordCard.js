// title: Record card
// description: A card for one record (a contact, an order, a book, a task) showing its title, a subtitle, small meta texts and a status badge, opening the record when tapped.
// props: title: text (default 'Untitled'); subtitle: text under the title (optional);
//   meta: [texts or numbers] shown small on one wrapping line, separated by dots (empty ones are left out; a single text works too);
//   badge: a text, or {text, tone? ('ok' | 'warn' | 'bad' | 'info'), color? (any css color, used when no tone)} shown at the top right (optional);
//   onClick() to make the whole card a button that opens the record (Enter and Space work too); label: accessible name (default the title)
function component(props) {
  css('sa-record', `
.sa-record{display:flex;flex-direction:column;gap:4px;min-width:0;text-align:left}
.sa-record .sa-record-head{display:flex;align-items:flex-start;gap:8px;min-width:0}
.sa-record .sa-record-title{flex:1;min-width:0;margin:0;font-size:1.02em;font-weight:600;line-height:1.3;overflow-wrap:anywhere}
.sa-record .sa-record-sub{margin:0;color:var(--muted);line-height:1.35;overflow-wrap:anywhere}
.sa-record .sa-record-meta{display:flex;flex-wrap:wrap;gap:2px 0;margin:2px 0 0;padding:0;list-style:none;color:var(--muted);font-size:.85em}
.sa-record .sa-record-meta li{white-space:nowrap;max-width:100%;overflow:hidden;text-overflow:ellipsis}
.sa-record .sa-record-meta li+li::before{content:'·';margin:0 6px;opacity:.7}
.sa-record .badge{flex:none;max-width:45%;overflow:hidden;text-overflow:ellipsis;white-space:nowrap}
.sa-record.click{cursor:pointer;transition:border-color .15s,box-shadow .15s}
.sa-record.click:hover{border-color:var(--accent)}
.sa-record.click:focus-visible{outline:2px solid var(--accent);outline-offset:2px}
`);
  const text = (v) => (v === null || v === undefined ? '' : String(v).trim());
  const title = text(props.title) || 'Untitled';
  const subtitle = text(props.subtitle);
  const meta = (Array.isArray(props.meta) ? props.meta : props.meta === undefined || props.meta === null ? [] : [props.meta])
    .map(text).filter(Boolean);

  let badge = null;
  const b = props.badge;
  const bText = b && typeof b === 'object' ? text(b.text) : text(b);
  if (bText) {
    const tone = b && typeof b === 'object' && ['ok', 'warn', 'bad', 'info'].includes(b.tone) ? b.tone : '';
    const color = b && typeof b === 'object' && !tone && b.color ? String(b.color) : '';
    badge = h('span', { class: `badge${tone ? ' ' + tone : ''}`, style: color ? `--c:${color}` : null, title: bText }, bText);
  }

  const click = typeof props.onClick === 'function';
  const el = h('div', {
    class: `card sa-record${click ? ' click' : ''}`,
    role: click ? 'button' : 'group',
    tabindex: click ? '0' : null,
    'aria-label': text(props.label) || title,
    onclick: click ? () => props.onClick() : null,
    onkeydown: click ? (e) => {
      if (e.target !== e.currentTarget) return;
      if (e.key === 'Enter' || e.key === ' ') { e.preventDefault(); props.onClick(); }
    } : null,
  },
    h('div', { class: 'sa-record-head' }, h('h3', { class: 'sa-record-title' }, title), badge),
    subtitle ? h('p', { class: 'sa-record-sub' }, subtitle) : null,
    meta.length ? h('ul', { class: 'sa-record-meta' }, meta.map((m) => h('li', { title: m }, m))) : null,
  );
  return el;
}
