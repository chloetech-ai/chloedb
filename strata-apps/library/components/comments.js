// title: Comments
// description: A thread of comments under anything (a task, a photo, a page): who wrote each and when, the text as Markdown, a box to add one (Enter sends, Shift+Enter for a new line), and removing your own.
// props: items: [{id, author (a person id), name, text, created}] oldest first; me: app.me()'s answer (null shows a Sign in prompt instead of the box);
//   onAdd(text) to save a new comment (may return a Promise); onRemove(item) to show remove on the comments me may remove
//   (their own, or any for an owner); people: [{id, name, email}] to name authors when items carry only author; placeholder (default 'Write a comment');
//   empty (default 'No comments yet')
function component(props) {
  css('sa-comments', `
.sa-comments{display:flex;flex-direction:column;gap:12px}
.sa-comments .c{display:flex;gap:10px;align-items:flex-start}
.sa-comments .c .b{flex:1;min-width:0}
.sa-comments .c .meta{display:flex;gap:8px;align-items:baseline;font-size:13px}
.sa-comments .c .meta .t{color:var(--muted);font-size:12px}
.sa-comments .c .meta button{margin-left:auto;border:0;box-shadow:none;background:none;color:var(--muted);font-size:12px;padding:0 4px;cursor:pointer}
.sa-comments .new{display:flex;gap:10px;align-items:flex-start}
.sa-comments .new textarea{flex:1;min-height:40px;resize:vertical}
`);
  const me = props.me || null;
  const names = new Map((props.people || []).map((p) => [String(p.id), p]));
  const who = (c) => { const p = names.get(String(c.author)); return { name: c.name || (p && p.name) || 'Someone', email: p && p.email }; };
  const mayRemove = (c) => !!props.onRemove && !!me && (String(c.author) === String(me.id) || me.role === 'owner');
  const list = (props.items || []).map((c) => {
    const p = who(c);
    return h('div', { class: 'c' }, ui.avatar({ name: p.name, email: p.email, size: 30 }),
      h('div', { class: 'b' },
        h('div', { class: 'meta' }, h('b', null, p.name), c.created ? h('span', { class: 't', title: app.fmt.datetime(c.created) }, app.fmt.ago(c.created)) : null,
          mayRemove(c) ? h('button', { type: 'button', onclick: () => { if (confirm('Delete this comment?')) props.onRemove(c); } }, 'Delete') : null),
        ui.markdown({ text: c.text || '' })));
  });
  let box;
  if (me && props.onAdd) {
    const ta = h('textarea', { rows: 1, placeholder: props.placeholder || 'Write a comment' });
    const send = h('button', { type: 'button', class: 'primary', onclick: () => post() }, 'Send');
    async function post() {
      const text = ta.value.trim();
      if (!text) return;
      send.disabled = true;
      try { await props.onAdd(text); ta.value = ''; } catch (e) { app.toast(e.message); }
      send.disabled = false;
      ta.focus();
    }
    ta.addEventListener('keydown', (e) => { if (e.key === 'Enter' && !e.shiftKey && !e.isComposing) { e.preventDefault(); post(); } });
    box = h('div', { class: 'new' }, ui.avatar({ name: me.name, email: me.email, size: 30 }), ta, send);
  } else if (props.onAdd) {
    box = h('div', { class: 'small muted' }, 'Sign in to comment. ',
      h('button', { type: 'button', class: 'small', onclick: () => document.body.append(ui.dialog({ title: 'Sign in', body: ui.signIn({ title: ' ' }) })) }, 'Sign in'));
  }
  return h('div', { class: 'sa-comments' }, list.length ? list : h('div', { class: 'small muted' }, props.empty || 'No comments yet'), box || null);
}
