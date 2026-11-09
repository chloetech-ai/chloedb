// title: Notification list
// description: Notifications newest first with their read state (alerts, mentions, reminders, activity), filtered to all or unread, opened with a tap, marked read one at a time or all at once.
// props: items: [{id, title, text? (a line under the title), at (ISO time), read (bool), icon? (a short text or emoji), tone? ('ok' | 'warn' | 'bad' | 'info')}];
//   onOpen(id) when a notification is tapped (an unread one is also reported to onMarkRead); onMarkRead(ids) with [ids] to mark read (shows a ✓ button on unread ones);
//   onMarkAllRead() for the "Mark all read" button (without it, the button calls onMarkRead with every unread id);
//   filter: 'all' (default) or 'unread'; onFilter(filter) when the person switches; empty: text when there is nothing (default "No notifications";
//   "Nothing unread" under the unread filter); title: heading (optional)
function component(props) {
  css('sa-notifs', `
.sa-notifs{display:flex;flex-direction:column;gap:8px;min-width:0}
.sa-notifs .sa-head{display:flex;align-items:center;gap:8px;flex-wrap:wrap}
.sa-notifs .sa-head h3{margin:0;flex:1 1 auto}
.sa-notifs .sa-tabs{display:inline-flex;gap:4px}
.sa-notifs .sa-all{margin-left:auto}
.sa-notifs ul{list-style:none;margin:0;padding:0;border:1px solid var(--line);border-radius:var(--radius);overflow:hidden;background:var(--card)}
.sa-notifs li{display:flex;align-items:flex-start;gap:6px;padding:0 6px 0 0;border-top:1px solid var(--line)}
.sa-notifs li:first-child{border-top:0}
.sa-notifs li.unread{background:color-mix(in srgb,var(--accent) 7%,transparent)}
.sa-notifs .sa-main{flex:1 1 auto;min-width:0;display:flex;gap:10px;align-items:flex-start;text-align:left;border:0;background:none;box-shadow:none;margin:0;padding:10px 4px 10px 10px;border-radius:0;color:var(--fg);font:inherit;font-weight:normal}
.sa-notifs button.sa-main{cursor:pointer}
.sa-notifs button.sa-main:hover{background:color-mix(in srgb,var(--fg) 4%,transparent)}
.sa-notifs .sa-icon{flex:0 0 auto;width:30px;height:30px;border-radius:50%;display:flex;align-items:center;justify-content:center;font-size:15px;line-height:1;background:color-mix(in srgb,var(--t) 16%,transparent);color:var(--t)}
.sa-notifs .sa-body{flex:1 1 auto;min-width:0;display:flex;flex-direction:column;gap:2px}
.sa-notifs .sa-line{display:flex;gap:8px;align-items:baseline}
.sa-notifs .sa-t{flex:1 1 auto;min-width:0;overflow-wrap:anywhere}
.sa-notifs li.unread .sa-t{font-weight:600}
.sa-notifs .sa-when{flex:0 0 auto;font-size:.8em;color:var(--muted);white-space:nowrap}
.sa-notifs .sa-text{font-size:.9em;color:var(--muted);overflow-wrap:anywhere;display:-webkit-box;-webkit-line-clamp:2;-webkit-box-orient:vertical;overflow:hidden}
.sa-notifs .sa-side{flex:0 0 auto;display:flex;align-items:center;gap:4px;padding-top:10px;min-height:30px}
.sa-notifs .sa-dot{width:8px;height:8px;border-radius:50%;background:var(--accent);margin:0 6px}
.sa-notifs .sa-mark{padding:2px 8px;min-width:0;line-height:1.4}
.sa-notifs .sa-empty{padding:24px 12px;text-align:center;color:var(--muted);border:1px dashed var(--line);border-radius:var(--radius)}
`);
  const tones = { ok: 'var(--ok)', warn: 'var(--warn)', bad: 'var(--bad)', info: 'var(--accent)' };
  const icons = { ok: '✓', warn: '!', bad: '!', info: 'i' };
  const canOpen = typeof props.onOpen === 'function';
  const canMark = typeof props.onMarkRead === 'function';
  const canMarkAll = typeof props.onMarkAllRead === 'function' || canMark;
  const items = (Array.isArray(props.items) ? props.items : [])
    .filter((n) => n && n.id != null)
    .map((n) => ({ ...n, t: Date.parse(n.at) }))
    .sort((a, b) => (isNaN(b.t) ? -Infinity : b.t) - (isNaN(a.t) ? -Infinity : a.t));
  const marked = new Set();
  let filter = props.filter === 'unread' ? 'unread' : 'all';
  const isRead = (n) => !!n.read || marked.has(n.id);

  const root = h('section', { class: 'sa-notifs', 'aria-label': props.title || 'Notifications' });

  function markRead(ids) {
    ids = ids.filter((id) => !marked.has(id));
    if (!ids.length) return;
    ids.forEach((id) => marked.add(id));
    if (canMark) props.onMarkRead(ids);
    draw();
  }
  function markAll() {
    const ids = items.filter((n) => !isRead(n)).map((n) => n.id);
    if (!ids.length) return;
    ids.forEach((id) => marked.add(id));
    if (typeof props.onMarkAllRead === 'function') props.onMarkAllRead();
    else props.onMarkRead(ids);
    draw();
  }
  function setFilter(f) {
    if (f === filter) return;
    filter = f;
    if (typeof props.onFilter === 'function') props.onFilter(f);
    draw();
  }
  function when(n) {
    if (isNaN(n.t)) return null;
    return h('time', { class: 'sa-when', datetime: n.at, title: app.fmt.datetime(n.at) }, app.fmt.ago(n.at));
  }
  function row(n) {
    const unread = !isRead(n);
    const tone = tones[n.tone] || 'var(--muted)';
    const content = [
      h('span', { class: 'sa-icon', style: `--t:${tone}`, 'aria-hidden': 'true' }, n.icon || icons[n.tone] || '•'),
      h('span', { class: 'sa-body' },
        h('span', { class: 'sa-line' }, h('span', { class: 'sa-t' }, String(n.title || '')), when(n)),
        n.text ? h('span', { class: 'sa-text' }, String(n.text)) : null),
    ];
    const label = `${unread ? 'Unread: ' : ''}${n.title || ''}`;
    const main = canOpen
      ? h('button', { type: 'button', class: 'sa-main', 'aria-label': label, onclick: () => { if (unread) markRead([n.id]); props.onOpen(n.id); } }, content)
      : h('div', { class: 'sa-main', 'aria-label': label }, content);
    const side = unread
      ? (canMark
        ? h('button', { type: 'button', class: 'sa-mark ghost small', title: 'Mark as read', 'aria-label': `Mark "${n.title || ''}" as read`, onclick: () => markRead([n.id]) }, '✓')
        : h('span', { class: 'sa-dot', 'aria-hidden': 'true' }))
      : null;
    return h('li', { class: unread ? 'unread' : null }, main, side ? h('span', { class: 'sa-side' }, side) : null);
  }

  function draw() {
    const unreadCount = items.filter((n) => !isRead(n)).length;
    const shown = filter === 'unread' ? items.filter((n) => !isRead(n)) : items;
    const chip = (f, text) => h('button', { type: 'button', class: `chip${filter === f ? ' on' : ''}`, 'aria-pressed': filter === f ? 'true' : 'false', onclick: () => setFilter(f) }, text);
    const head = h('div', { class: 'sa-head' },
      props.title ? h('h3', null, props.title) : null,
      h('div', { class: 'sa-tabs chips', role: 'group', 'aria-label': 'Show' },
        chip('all', 'All'),
        chip('unread', unreadCount ? `Unread (${unreadCount})` : 'Unread')),
      canMarkAll && unreadCount ? h('button', { type: 'button', class: 'sa-all ghost small', onclick: markAll }, 'Mark all read') : null);
    const body = shown.length
      ? h('ul', { role: 'list' }, shown.map(row))
      : h('div', { class: 'sa-empty', role: 'status' },
        filter === 'unread' && items.length ? 'Nothing unread' : (props.empty || (filter === 'unread' ? 'Nothing unread' : 'No notifications')));
    root.replaceChildren(head, body);
  }

  draw();
  return root;
}
