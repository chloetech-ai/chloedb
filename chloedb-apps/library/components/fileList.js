// title: File list
// description: A list of stored files with a kind icon or thumbnail, name, size and date, opening or downloading each, and an optional remove button.
// props: files: [{id, name, type, size, created}] (from app.upload or app.files); onOpen(file) when a name is clicked (default: images open in the viewer, other files open in a new tab);
//   onRemove(file) to show a remove button; empty (default 'No files yet'); compact: true for one line per file without thumbnails
function component(props) {
  css('sa-files', `
.sa-files{display:flex;flex-direction:column}
.sa-files .f{display:flex;align-items:center;gap:10px;padding:8px 2px;border-bottom:1px solid var(--line);min-width:0}
.sa-files .f:last-child{border-bottom:0}
.sa-files .ic{width:38px;height:38px;border-radius:8px;background:var(--soft);display:flex;align-items:center;justify-content:center;font-size:11px;font-weight:700;color:var(--muted);flex:none;overflow:hidden;text-transform:uppercase}
.sa-files .ic img{width:100%;height:100%;object-fit:cover}
.sa-files .nm{flex:1;min-width:0}
.sa-files .nm button{border:0;box-shadow:none;background:none;padding:0;color:var(--fg);font:inherit;font-weight:500;cursor:pointer;max-width:100%;overflow:hidden;text-overflow:ellipsis;white-space:nowrap;display:block;text-align:left}
.sa-files .nm button:hover{color:var(--accent)}
.sa-files.compact .ic{width:26px;height:26px;font-size:9px}
`);
  const files = (props.files || []).filter(Boolean);
  if (!files.length) return h('div', { class: 'empty' }, props.empty || 'No files yet');
  const bytes = (n) => (n < 1024 ? `${n} B` : n < 1048576 ? `${Math.round(n / 1024)} KB` : `${(n / 1048576).toFixed(1)} MB`);
  const isImage = (f) => /^image\/(png|jpeg|gif|webp|avif)$/.test(f.type || '');
  const ext = (f) => (String(f.name || '').match(/\.([a-z0-9]{1,4})$/i) || [, (f.type || 'file').split('/').pop().slice(0, 4)])[1];
  const images = files.filter(isImage);
  const open = (f) => {
    if (props.onOpen) return props.onOpen(f);
    if (isImage(f)) document.body.append(ui.imageViewer({ items: images.map((x) => ({ file: x.id, name: x.name })), index: images.indexOf(f) }));
    else window.open(app.fileUrl(f.id), '_blank', 'noopener');
  };
  return h('div', { class: `sa-files${props.compact ? ' compact' : ''}` }, files.map((f) => h('div', { class: 'f' },
    h('span', { class: 'ic' }, isImage(f) && !props.compact ? h('img', { src: app.fileUrl(f.id), alt: '', loading: 'lazy' }) : ext(f)),
    h('div', { class: 'nm' }, h('button', { type: 'button', title: f.name, onclick: () => open(f) }, f.name || 'file'),
      props.compact ? null : h('div', { class: 'small muted' }, [f.size ? bytes(Number(f.size)) : null, f.created ? app.fmt.ago(f.created) : null].filter(Boolean).join(' · '))),
    h('a', { class: 'small', href: app.fileUrl(f.id, { download: true }), download: f.name || '', title: 'Download' }, '↓'),
    props.onRemove ? h('button', { type: 'button', class: 'ghost small', 'aria-label': `Remove ${f.name}`, onclick: () => props.onRemove(f) }, '✕') : null)));
}
