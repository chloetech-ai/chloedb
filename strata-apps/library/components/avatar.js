// title: Avatar
// description: A person's round picture, or their initials on a colour that is always the same for them; or a row of overlapping avatars for several people with "+3" for the rest.
// props: name; email (used for the colour when there is no name); src: a picture (an app file id or a URL); size in px (default 32);
//   title (default the name); people: [{name, email, src}] to show a stack instead of one; max: how many of the stack to show (default 4);
//   onClick() when clicked
function component(props) {
  css('sa-avatar', `
.sa-avatar{display:inline-flex;align-items:center;justify-content:center;border-radius:50%;overflow:hidden;flex:none;font-weight:600;letter-spacing:.02em;user-select:none;box-sizing:border-box;line-height:1}
.sa-avatar img{width:100%;height:100%;object-fit:cover;display:block}
.sa-avatars{display:inline-flex;align-items:center}
.sa-avatars>.sa-avatar{border:2px solid var(--card);margin-left:-8px}
.sa-avatars>.sa-avatar:first-child{margin-left:0}
.sa-avatars>.more{background:var(--soft);color:var(--muted)}
`);
  const size = Number(props.size) || 32;
  const one = (p) => {
    const name = String(p.name || p.email || '?');
    const bg = lib.color.forName(p.email || name);
    const el = h('span', { class: 'sa-avatar', title: p.title || name, style: { width: `${size}px`, height: `${size}px`, fontSize: `${Math.round(size * 0.4)}px`, background: bg, color: lib.color.readable(bg) } },
      lib.text.initials(name.includes('@') ? name.split('@')[0].replace(/[._-]+/g, ' ') : name));
    if (p.src) {
      const img = h('img', { src: app.fileUrl(p.src), alt: '', loading: 'lazy' });
      img.addEventListener('load', () => el.replaceChildren(img));
      img.addEventListener('error', () => img.remove());
    }
    return el;
  };
  if (Array.isArray(props.people)) {
    const max = props.max || 4;
    const list = props.people.slice(0, max).map(one);
    const rest = props.people.length - max;
    if (rest > 0) list.push(h('span', { class: 'sa-avatar more', style: { width: `${size}px`, height: `${size}px`, fontSize: `${Math.round(size * 0.36)}px` }, title: props.people.slice(max).map((p) => p.name || p.email).join(', ') }, `+${rest}`));
    return h('span', { class: 'sa-avatars', onclick: props.onClick ? () => props.onClick() : null }, list);
  }
  const el = one(props);
  if (props.onClick) { el.style.cursor = 'pointer'; el.addEventListener('click', () => props.onClick()); }
  return el;
}
