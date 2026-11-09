// title: Gallery
// description: A grid of picture thumbnails that opens the full-screen viewer on a click, with captions, an optional remove button and an empty state.
// props: items: [{file (an app file id) or src, caption, id}]; size: thumbnail width in px (default 140; the grid fills the width);
//   ratio: thumbnail shape, width/height (default 1); onOpen(index) instead of the built-in viewer; onRemove(item) to show a remove button;
//   empty: the text when there are no pictures (default 'No pictures yet')
function component(props) {
  css('sa-gallery', `
.sa-gallery{display:grid;gap:8px}
.sa-gallery figure{margin:0;position:relative;display:flex;flex-direction:column;gap:4px;min-width:0}
.sa-gallery figcaption{font-size:12.5px;color:var(--muted);white-space:nowrap;overflow:hidden;text-overflow:ellipsis}
.sa-gallery .rm{position:absolute;top:6px;right:6px;width:26px;height:26px;border-radius:50%;padding:0;background:rgba(0,0,0,.55);color:#fff;border:0;box-shadow:none;opacity:0;transition:opacity .12s;cursor:pointer}
.sa-gallery figure:hover .rm,.sa-gallery .rm:focus{opacity:1}
@media (hover:none){.sa-gallery .rm{opacity:1}}
`);
  const items = (props.items || []).filter((x) => x && (x.file || x.src));
  if (!items.length) return h('div', { class: 'empty' }, props.empty || 'No pictures yet');
  const size = Number(props.size) || 140;
  const open = (i) => (props.onOpen ? props.onOpen(i) : document.body.append(ui.imageViewer({ items, index: i })));
  return h('div', { class: 'sa-gallery', style: { gridTemplateColumns: `repeat(auto-fill, minmax(${Math.min(size, 160)}px, 1fr))` } },
    items.map((it, i) => h('figure', null,
      ui.image({ file: it.file, src: it.src, alt: it.caption || '', ratio: props.ratio || 1, onClick: () => open(i) }),
      it.caption ? h('figcaption', null, it.caption) : null,
      props.onRemove ? h('button', { class: 'rm', type: 'button', 'aria-label': 'Remove', onclick: (e) => { e.stopPropagation(); props.onRemove(it); } }, '✕') : null)));
}
