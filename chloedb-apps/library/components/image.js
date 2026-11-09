// title: Image
// description: Shows a stored picture (an app file id) or an image URL, sized and cropped as asked, with a soft placeholder while it loads and a plain box if it cannot.
// props: file: an app file id, or src: an image URL; alt (default ''); width and height in px or any CSS size (default width 100%, height auto);
//   fit: 'cover' (default) or 'contain'; ratio: a width/height ratio such as 1 or 4/3 when there is no height; rounded (default true); onClick()
function component(props) {
  css('sa-image', `
.sa-image{position:relative;display:block;overflow:hidden;background:var(--soft);flex:none}
.sa-image.rounded{border-radius:10px}
.sa-image img{display:block;width:100%;height:100%;opacity:0;transition:opacity .2s}
.sa-image img.on{opacity:1}
.sa-image .none{position:absolute;inset:0;display:flex;align-items:center;justify-content:center;color:var(--muted);font-size:12px}
`);
  const size = (v) => (v === undefined || v === null ? null : typeof v === 'number' ? `${v}px` : String(v));
  const style = { width: size(props.width) || '100%' };
  if (props.height !== undefined) style.height = size(props.height);
  else if (props.ratio) style.aspectRatio = String(props.ratio);
  const src = app.fileUrl(props.file || props.src || '');
  const img = h('img', { src: src || null, alt: props.alt || '', loading: 'lazy', decoding: 'async', style: { objectFit: props.fit === 'contain' ? 'contain' : 'cover', height: style.height || style.aspectRatio ? '100%' : 'auto' } });
  const box = h('span', { class: `sa-image${props.rounded === false ? '' : ' rounded'}`, style, onclick: props.onClick ? () => props.onClick() : null }, img);
  if (props.onClick) box.style.cursor = 'zoom-in';
  img.addEventListener('load', () => img.classList.add('on'));
  img.addEventListener('error', () => { img.remove(); box.append(h('span', { class: 'none' }, props.alt || 'No picture')); });
  if (!src) { img.remove(); box.append(h('span', { class: 'none' }, props.alt || 'No picture')); }
  return box;
}
