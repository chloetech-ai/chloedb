// title: Markdown
// description: Shows Markdown text safely (headings, lists, task lists, quotes, code, links, pictures), with clickable task boxes when asked; or an editor with a small toolbar and a preview.
// props: text: the Markdown to show; onTask(index, checked) makes task boxes clickable (lib.markdown.toggleTask(text, index, checked) gives the new text);
//   editing: true for the editor instead, with value, onChange(text), placeholder, rows (default 8) and onSubmit(text) for Ctrl/⌘+Enter
//   (pictures written as ![alt](file:<id>) show the app's stored file)
function component(props) {
  css('sa-md', `
.sa-md{line-height:1.55;overflow-wrap:anywhere}
.sa-md>:first-child{margin-top:0}.sa-md>:last-child{margin-bottom:0}
.sa-md img{max-width:100%;border-radius:8px}
.sa-md blockquote{margin:8px 0;padding:2px 12px;border-left:3px solid var(--line);color:var(--muted)}
.sa-md pre{background:var(--soft);border-radius:8px;padding:10px 12px;overflow:auto;font-size:13px}
.sa-md li.task{list-style:none;margin-left:-1.3em}
.sa-md li.task input{margin-right:6px;vertical-align:-2px}
.sa-md li.task.done>span{color:var(--muted);text-decoration:line-through}
.sa-mded{display:flex;flex-direction:column;border:1px solid var(--line);border-radius:10px;overflow:hidden;background:var(--card)}
.sa-mded .tb{display:flex;gap:2px;padding:4px;border-bottom:1px solid var(--line);flex-wrap:wrap}
.sa-mded .tb button{border:0;box-shadow:none;background:none;padding:4px 8px;font-size:13px;border-radius:6px}
.sa-mded .tb button:hover,.sa-mded .tb button.on{background:var(--soft)}
.sa-mded textarea{border:0!important;box-shadow:none!important;border-radius:0!important;resize:vertical;font-family:inherit}
.sa-mded .pv{padding:10px 12px;min-height:80px}
`);
  function inline(nodes) {
    return (nodes || []).map((n) => {
      switch (n.type) {
        case 'text': return n.text;
        case 'break': return h('br');
        case 'code': return h('code', null, n.text);
        case 'strong': return h('strong', null, inline(n.children));
        case 'em': return h('em', null, inline(n.children));
        case 'del': return h('del', null, inline(n.children));
        case 'link': return h('a', { href: n.href, target: '_blank', rel: 'noopener' }, inline(n.children));
        case 'image': return h('img', { src: n.src.startsWith('file:') ? app.fileUrl(n.src.slice(5)) : n.src, alt: n.alt || '', loading: 'lazy' });
        default: return null;
      }
    });
  }
  function render(text) {
    let task = 0;
    const block = (b) => {
      switch (b.type) {
        case 'heading': return h(`h${Math.min(6, b.level + 1)}`, null, inline(b.children));
        case 'paragraph': return h('p', null, inline(b.children));
        case 'rule': return h('hr');
        case 'code': return h('pre', null, h('code', null, b.text));
        case 'quote': return h('blockquote', null, b.children.map(block));
        case 'list': return h(b.ordered ? 'ol' : 'ul', b.ordered && b.start !== 1 ? { start: b.start } : null, b.items.map((it) => {
          if (!it.task) return h('li', null, inline(it.children));
          const index = task++;
          const box = h('input', { type: 'checkbox', checked: !!it.checked, disabled: !props.onTask, onchange: () => props.onTask(index, box.checked) });
          return h('li', { class: `task${it.checked ? ' done' : ''}` }, box, h('span', null, inline(it.children)));
        }));
        default: return null;
      }
    };
    return lib.markdown.parse(text).map(block);
  }
  if (!props.editing) return h('div', { class: 'sa-md' }, render(props.text || ''));
  const ta = h('textarea', { rows: props.rows || 8, placeholder: props.placeholder || 'Write here. **bold**, *italic*, - lists, - [ ] tasks, [links](https://…)', value: props.value || '' });
  const pv = h('div', { class: 'pv sa-md', hidden: true });
  const emit = () => { if (props.onChange) props.onChange(ta.value); };
  ta.addEventListener('input', emit);
  ta.addEventListener('keydown', (e) => { if (e.key === 'Enter' && (e.metaKey || e.ctrlKey) && props.onSubmit) { e.preventDefault(); props.onSubmit(ta.value); } });
  const wrap = (before, after = before, empty = 'text') => {
    const a = ta.selectionStart; const b = ta.selectionEnd;
    const sel = ta.value.slice(a, b) || empty;
    ta.setRangeText(before + sel + after, a, b, 'end');
    ta.focus(); ta.setSelectionRange(a + before.length, a + before.length + sel.length); emit();
  };
  const line = (prefix) => {
    const a = ta.selectionStart;
    const start = ta.value.lastIndexOf('\n', a - 1) + 1;
    ta.setRangeText(prefix, start, start, 'end'); ta.focus(); emit();
  };
  const preview = h('button', { type: 'button', onclick: () => { const on = pv.hidden; pv.hidden = !on; ta.hidden = on; preview.classList.toggle('on', on); if (on) pv.replaceChildren(...render(ta.value)); } }, 'Preview');
  return h('div', { class: 'sa-mded' },
    h('div', { class: 'tb' },
      h('button', { type: 'button', title: 'Bold', onclick: () => wrap('**') }, h('b', null, 'B')),
      h('button', { type: 'button', title: 'Italic', onclick: () => wrap('*') }, h('i', null, 'I')),
      h('button', { type: 'button', title: 'Heading', onclick: () => line('## ') }, 'H'),
      h('button', { type: 'button', title: 'List', onclick: () => line('- ') }, '• List'),
      h('button', { type: 'button', title: 'Task', onclick: () => line('- [ ] ') }, '☐ Task'),
      h('button', { type: 'button', title: 'Link', onclick: () => wrap('[', '](https://)', 'link') }, 'Link'),
      h('span', { style: 'flex:1' }), preview),
    ta, pv);
}
