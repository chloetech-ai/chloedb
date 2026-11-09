// title: Empty state
// description: A friendly message where a list, search or page has nothing to show yet (no tasks, no results, an empty inbox), with an icon, a title, a line of text and buttons for what to do next.
// props: icon: an emoji or short text shown large above (optional); title: the main line (default "Nothing here yet");
//   text: a sentence under it, a string or an element (optional); action: {label, onClick()} the main button (optional);
//   secondary: {label, onClick()} a quieter second button (optional); compact: true for a smaller version inside cards and lists (default false)
function component(props) {
  css('sa-empty', `
.sa-empty{display:flex;flex-direction:column;align-items:center;justify-content:center;text-align:center;gap:8px;padding:40px 16px;color:var(--muted);box-sizing:border-box;width:100%;max-width:100%}
.sa-empty .sa-empty-icon{font-size:40px;line-height:1;margin-bottom:4px;color:var(--fg);opacity:.85}
.sa-empty .sa-empty-title{margin:0;font-size:1.1em;font-weight:600;color:var(--fg);overflow-wrap:anywhere}
.sa-empty .sa-empty-text{margin:0;max-width:36em;line-height:1.45;overflow-wrap:anywhere}
.sa-empty .sa-empty-actions{display:flex;flex-wrap:wrap;gap:8px;justify-content:center;margin-top:8px}
.sa-empty.compact{padding:16px 12px;gap:4px;font-size:.92em}
.sa-empty.compact .sa-empty-icon{font-size:24px;margin-bottom:2px}
.sa-empty.compact .sa-empty-title{font-size:1em}
.sa-empty.compact .sa-empty-actions{margin-top:6px}
`);
  const compact = !!props.compact;
  const title = props.title == null || props.title === '' ? 'Nothing here yet' : String(props.title);
  const icon = props.icon == null || props.icon === '' ? null : String(props.icon);
  const text = props.text == null || props.text === '' ? null : props.text;

  const button = (a, primary) => {
    if (!a || !a.label) return null;
    const cls = [primary ? 'primary' : 'ghost', compact ? 'small' : ''].filter(Boolean).join(' ');
    return h('button', {
      type: 'button',
      class: cls,
      onclick: (e) => { if (typeof a.onClick === 'function') a.onClick(e); }
    }, String(a.label));
  };
  const main = button(props.action, true);
  const second = button(props.secondary, !main);

  return h('div', { class: `sa-empty${compact ? ' compact' : ''}`, role: 'status' },
    icon ? h('div', { class: 'sa-empty-icon', 'aria-hidden': 'true' }, icon) : null,
    h(compact ? 'h4' : 'h3', { class: 'sa-empty-title' }, title),
    text ? (typeof text === 'string' || typeof text === 'number'
      ? h('p', { class: 'sa-empty-text' }, String(text))
      : h('div', { class: 'sa-empty-text' }, text)) : null,
    main || second ? h('div', { class: 'sa-empty-actions' }, main, second) : null
  );
}
