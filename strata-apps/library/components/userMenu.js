// title: User menu
// description: The signed-in person's avatar and name for a toolbar, opening a small menu with their email, role and sign-out; when nobody is signed in, a Sign in button that opens the sign-in box in a dialog.
// props: user: app.me()'s answer (null when nobody is signed in); items: extra menu entries [{label, onClick()}];
//   compact: true to show only the avatar; note: the line shown in the sign-in dialog
function component(props) {
  css('sa-usermenu', `
.sa-usermenu{position:relative;display:inline-flex}
.sa-usermenu>button{display:inline-flex;align-items:center;gap:8px;border-radius:999px;padding:3px 10px 3px 3px}
.sa-usermenu .menu{position:absolute;right:0;top:calc(100% + 6px);min-width:220px;background:var(--card);border:1px solid var(--line);border-radius:10px;box-shadow:0 10px 30px rgba(0,0,0,.15);padding:6px;z-index:50}
.sa-usermenu .menu .head{padding:8px 10px;border-bottom:1px solid var(--line);margin-bottom:4px}
.sa-usermenu .menu button{display:block;width:100%;text-align:left;border:0;box-shadow:none;background:none;padding:7px 10px;border-radius:6px}
.sa-usermenu .menu button:hover{background:var(--soft)}
`);
  const u = props.user;
  if (!u) {
    return h('button', { type: 'button', class: 'small', onclick: () => document.body.append(ui.dialog({ title: 'Sign in', body: ui.signIn({ title: ' ', note: props.note }) })) }, 'Sign in');
  }
  const menu = h('div', { class: 'menu', hidden: true },
    h('div', { class: 'head' }, h('b', null, u.name), h('div', { class: 'small muted' }, u.email), h('div', { class: 'small muted' }, u.role === 'owner' ? 'Owner of this app' : 'Member')),
    (props.items || []).map((it) => h('button', { type: 'button', onclick: () => { menu.hidden = true; it.onClick && it.onClick(); } }, it.label)),
    h('button', { type: 'button', onclick: async () => { menu.hidden = true; await app.signOut(); } }, 'Sign out'));
  const toggle = h('button', { type: 'button', class: props.compact ? 'ghost' : null, 'aria-haspopup': 'menu', onclick: (e) => { e.stopPropagation(); menu.hidden = !menu.hidden; } },
    ui.avatar({ name: u.name, email: u.email, size: 26 }), props.compact ? null : h('span', { class: 'clip', style: 'max-width:140px' }, u.name));
  if (props.compact) toggle.style.padding = '3px';
  document.addEventListener('click', (e) => { if (!root.contains(e.target)) menu.hidden = true; });
  const root = h('div', { class: 'sa-usermenu' }, toggle, menu);
  return root;
}
