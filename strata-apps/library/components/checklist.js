// title: Checklist
// description: A list of things to tick off (groceries, packing, steps), with a box to add one, a button to remove one, a done count, and done items moved to the bottom and struck through.
// props: items: [{id, label, done, note?}]; onToggle(id, done) when an item is ticked or unticked;
//   onAdd(label) to show an add box (it may return a promise); onRemove(id) to show a remove button per item;
//   doneLast: false to keep done items in place (default true); placeholder: text of the add box (default "Add an item"); title: heading (optional)
function component(props) {
  css('sa-checklist', `
.sa-check{display:flex;flex-direction:column;gap:8px}
.sa-check-h{display:flex;align-items:baseline;gap:8px}
.sa-check-h .n{color:var(--muted);font-size:13px;margin-left:auto}
.sa-check-bar{height:4px;border-radius:2px;background:var(--soft);overflow:hidden}
.sa-check-bar i{display:block;height:100%;background:var(--ok);transition:width .2s}
.sa-check-list{background:var(--card);border:1px solid var(--line);border-radius:var(--radius);overflow:hidden}
.sa-check-it{display:flex;align-items:center;gap:10px;padding:9px 12px;border-bottom:1px solid var(--line)}
.sa-check-it:last-child{border-bottom:0}
.sa-check-it label{flex:1;min-width:0;display:flex;flex-direction:row;align-items:center;gap:10px;color:var(--fg);font-size:15px;cursor:pointer}
.sa-check-it.done label span{text-decoration:line-through;color:var(--muted)}
.sa-check-it .note{font-size:12.5px;color:var(--muted)}
.sa-check-it .x{opacity:.45}
.sa-check-it:hover .x{opacity:1}
.sa-check-add{display:flex;gap:8px}
.sa-check-add input{flex:1}
`);
  const items = (props.items || []).filter(Boolean);
  const list = props.doneLast === false ? items : [...items.filter((i) => !i.done), ...items.filter((i) => i.done)];
  const done = items.filter((i) => i.done).length;
  const root = h('div', { class: 'sa-check' });
  const head = h('div', { class: 'sa-check-h' }, props.title ? h('h3', null, props.title) : null, items.length ? h('span', { class: 'n' }, `${done} of ${items.length} done`) : null);
  const bar = items.length ? h('div', { class: 'sa-check-bar' }, h('i', { style: `width:${(100 * done) / items.length}%` })) : null;
  const rows = list.length ? h('div', { class: 'sa-check-list' }, list.map((it) => h('div', { class: `sa-check-it${it.done ? ' done' : ''}` },
    h('label', null, h('input', { type: 'checkbox', checked: !!it.done, onchange: (e) => props.onToggle && props.onToggle(it.id, e.target.checked) }),
      h('span', null, it.label || ''), it.note ? h('span', { class: 'note' }, it.note) : null),
    props.onRemove ? h('button', { class: 'ghost small x', type: 'button', 'aria-label': `Remove ${it.label}`, onclick: () => props.onRemove(it.id) }, '✕') : null)))
    : h('div', { class: 'empty' }, props.empty || 'Nothing on the list');
  let add = null;
  if (props.onAdd) {
    const input = h('input', { placeholder: props.placeholder || 'Add an item', 'aria-label': props.placeholder || 'Add an item' });
    const submit = async () => {
      const label = input.value.trim();
      if (!label) return;
      input.disabled = true;
      try { await props.onAdd(label); input.value = ''; } finally { input.disabled = false; input.focus(); }
    };
    input.addEventListener('keydown', (e) => { if (e.key === 'Enter') { e.preventDefault(); submit(); } });
    add = h('div', { class: 'sa-check-add' }, input, h('button', { class: 'primary', type: 'button', onclick: submit }, 'Add'));
  }
  root.append(props.title || items.length ? head : null, bar, add, rows);
  return root;
}
