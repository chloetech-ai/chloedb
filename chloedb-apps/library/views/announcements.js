// title: Announcements
// description: A feed of posts for the app's members, newest first with pinned posts on top, where signed-in members write posts, owners pin them and authors or owners remove them.
// tables: posts(title text, body text, author keyword, pinned bool)
async function render(root, app) {
  const PAGE = 30;
  const MAX = 500;
  let limit = PAGE;
  let seq = 0;

  const me = await app.me();
  const canPost = lib.access.can(me, 'add', null);
  const pinPolicy = { edit: 'owner' };

  root.append(h('style', {}, `
    .ann { max-width: 760px; margin: 0 auto; }
    .ann .post { display: flex; flex-direction: column; gap: 8px; min-width: 0; }
    .ann .post.pinned { border-color: var(--accent); }
    .ann .post h3 { margin: 0; overflow-wrap: anywhere; }
    .ann .post .body { overflow-wrap: anywhere; }
    .ann .who { gap: 0; min-width: 0; }
    .ann .feed { display: flex; flex-direction: column; gap: 12px; }
  `));

  const feed = h('div', { class: 'feed' });

  // Composer: built once, never redrawn.
  let composer;
  if (canPost) {
    composer = h('details', { class: 'card' },
      h('summary', {}, h('strong', {}, 'Write a post')),
      h('div', { style: 'margin-top: 12px' },
        ui.form({
          fields: [
            { name: 'title', label: 'Title', required: true, placeholder: 'What is it about?' },
            { name: 'body', label: 'Message', type: 'textarea', placeholder: 'The details (Markdown works)' }
          ],
          submitLabel: 'Post',
          resetOnSubmit: true,
          onSubmit: async (v) => {
            const title = String(v.title || '').trim();
            if (!title) throw new Error('Give the post a title.');
            await app.insert('posts', lib.access.stamp(me, {
              title,
              body: String(v.body || '').trim(),
              pinned: false
            }));
            app.toast('Posted');
          }
        })
      )
    );
  } else if (me) {
    composer = h('p', { class: 'card muted' },
      'You are signed in but not a member of this app, so you can read posts but not write them. Ask the owner to add you.');
  } else {
    composer = h('p', { class: 'card muted' }, 'Sign in as a member to write a post.');
  }

  root.append(h('div', { class: 'ann col' },
    h('div', { class: 'row' },
      h('h1', { class: 'grow', style: 'margin: 0' }, 'Announcements'),
      ui.userMenu({ user: me, note: 'Sign in to write and pin posts.' })
    ),
    composer,
    feed
  ));

  function postCard(p, people) {
    const person = people[p.author];
    const name = person ? (person.name || person.email) : 'Former member';
    const actions = [];
    if (lib.access.can(me, 'edit', p, pinPolicy)) {
      const btn = h('button', {
        class: 'small ghost',
        onclick: async () => {
          btn.disabled = true;
          try {
            await app.update('posts', p.id, { pinned: !p.pinned });
            app.toast(p.pinned ? 'Unpinned' : 'Pinned to the top');
          } catch (e) {
            app.toast('Could not change the pin');
            btn.disabled = false;
          }
        }
      }, p.pinned ? 'Unpin' : '📌 Pin');
      actions.push(btn);
    }
    if (lib.access.can(me, 'remove', p)) {
      actions.push(ui.confirmButton({
        label: 'Delete', small: true,
        onConfirm: async () => {
          try { await app.remove('posts', p.id); app.toast('Post deleted'); }
          catch (e) { app.toast('Could not delete the post'); }
        }
      }));
    }
    return h('article', { class: 'card post' + (p.pinned ? ' pinned' : '') },
      h('div', { class: 'row', style: 'flex-wrap: nowrap' },
        ui.avatar({ name, email: person && person.email, size: 32 }),
        h('div', { class: 'col grow who' },
          h('strong', { class: 'clip' }, name),
          h('small', { class: 'muted', title: app.fmt.datetime(p.created) }, app.fmt.ago(p.created))
        ),
        p.pinned ? h('span', { class: 'badge info nowrap' }, 'Pinned') : null
      ),
      h('h3', {}, p.title || 'Untitled'),
      p.body ? h('div', { class: 'body' }, ui.markdown({ text: p.body })) : null,
      actions.length ? h('div', { class: 'row right' }, actions) : null
    );
  }

  async function redraw() {
    const mine = ++seq;
    let pinned, rest, total, members;
    try {
      [pinned, rest, total, members] = await Promise.all([
        app.sql('SELECT id, title, body, author, pinned, created FROM posts WHERE pinned ORDER BY created DESC LIMIT 50'),
        app.sql(`SELECT id, title, body, author, pinned, created FROM posts WHERE (pinned = false OR pinned IS NULL) ORDER BY created DESC LIMIT ${limit}`),
        app.sql('SELECT COUNT(*) AS n FROM posts WHERE (pinned = false OR pinned IS NULL)'),
        app.people().catch(() => [])
      ]);
    } catch (e) {
      if (mine === seq) feed.replaceChildren(h('p', { class: 'empty' }, 'Could not load the posts. Try again in a moment.'));
      return;
    }
    if (mine !== seq) return;

    const people = {};
    for (const m of members || []) people[m.id] = m;

    if (!pinned.length && !rest.length) {
      feed.replaceChildren(ui.emptyState({
        icon: '📣',
        title: 'No announcements yet',
        text: canPost
          ? 'Write the first post above to tell everyone what is going on.'
          : 'Posts from the members of this app will show up here.'
      }));
      return;
    }

    const more = (total[0] ? total[0].n : 0) - rest.length;
    feed.replaceChildren(
      pinned.map((p) => postCard(p, people)),
      rest.map((p) => postCard(p, people)),
      more > 0 && limit < MAX
        ? h('button', {
            class: 'ghost',
            onclick: (e) => {
              e.target.disabled = true;
              limit = Math.min(MAX, limit + PAGE);
              app.track('more', String(limit));
              redraw();
            }
          }, `Show older posts (${app.fmt.number(more)} more)`)
        : null
    );
  }

  app.onChange(['posts'], redraw);
  await redraw();
}
