// title: Notes
// description: Write notes in Markdown with tags, find them by search or tag, pin the important ones to the top, and see when each was last edited.
// tables: notes(title text, body text, tags keyword[], pinned bool, edited timestamp)
async function render(root, app) {
  root.append(h('style', {}, `
    .notes { display: grid; grid-template-columns: minmax(240px, 340px) minmax(0, 1fr); gap: 20px; align-items: start; }
    .notes > section { min-width: 0; }
    .notes-item { cursor: pointer; display: flex; flex-direction: column; align-items: stretch; gap: 2px; min-width: 0; }
    .notes-item:focus-visible { outline: 2px solid var(--accent); outline-offset: -2px; }
    .notes-item.sel { background: color-mix(in srgb, var(--accent) 12%, transparent); }
    .notes-item .clip { min-width: 0; }
    .notes-title { width: 100%; font-size: 1.3em; font-weight: 600; }
    .notes-back { display: none; }
    @media (max-width: 720px) {
      .notes { grid-template-columns: minmax(0, 1fr); }
      .notes.has-sel .notes-list { display: none; }
      .notes:not(.has-sel) .notes-editor { display: none; }
      .notes-back { display: inline-flex; }
    }
  `));

  const params = app.params || {};
  const FIELDS = 'id, title, body, tags, pinned, edited, created, coalesce(edited, created) AS touched';
  let query = '';
  let tag = null;
  let selectedId = null;
  let rows = [];
  let truncated = false;
  let tagCounts = [];
  let seq = 0;

  // editor state (never redrawn by onChange)
  let draft = null;
  let dirty = false;
  let timer = null;
  let saving = Promise.resolve();
  let statusEl = null;

  const newBtn = h('button', { class: 'primary', onclick: () => newNote() }, '+ New note');
  const filterArea = h('div');
  const listBody = h('div');
  const editorPane = h('section', { class: 'notes-editor col' });
  const listPane = h('section', { class: 'notes-list col' },
    h('div', { class: 'row' }, h('h1', { style: 'margin:0' }, 'Notes'), h('span', { class: 'right' }, newBtn)),
    ui.searchBox({
      placeholder: 'Search notes',
      onSearch: (q) => {
        query = q;
        if (q) app.track('search', q);
        load();
      },
    }),
    filterArea,
    listBody
  );
  const shell = h('div', { class: 'notes' }, listPane, editorPane);
  root.append(shell);

  function ftsQuery(q) {
    const words = (q || '').toLowerCase().match(/[\p{L}\p{N}]+/gu) || [];
    return words.map((w, i) => (i === words.length - 1 ? w + '*' : w)).join(' ');
  }

  function noteName(n) {
    const t = (n.title || '').trim();
    if (t) return t;
    return lib.markdown.excerpt(n.body || '', 60) || 'Untitled';
  }

  async function fetchRows(textCond, p) {
    const base = [];
    if (tag) base.push('tags = :tag');
    if (textCond) base.push(textCond);
    const where = (extra) => ' WHERE ' + [...base, extra].join(' AND ');
    const [pinned, rest] = await Promise.all([
      app.sql(`SELECT ${FIELDS} FROM notes${where('pinned')} ORDER BY touched DESC LIMIT 100`, p),
      app.sql(`SELECT ${FIELDS} FROM notes${where('(pinned = false OR pinned IS NULL)')} ORDER BY touched DESC LIMIT 300`, p),
    ]);
    return { pinned, rest };
  }

  async function load() {
    const my = ++seq;
    const p = {};
    if (tag) p.tag = tag;
    let result;
    const fts = ftsQuery(query);
    try {
      if (fts) {
        p.q = fts;
        try {
          result = await fetchRows('(MATCH(title, :q) OR MATCH(body, :q))', p);
        } catch (e) {
          delete p.q;
          p.lk = '%' + query.toLowerCase() + '%';
          result = await fetchRows('(lower(title) LIKE :lk OR lower(body) LIKE :lk)', p);
        }
      } else {
        result = await fetchRows(null, p);
      }
      const tagRows = await app.sql('SELECT tags FROM notes WHERE tags IS NOT NULL LIMIT 500');
      if (my !== seq) return;
      rows = [...result.pinned, ...result.rest];
      truncated = result.rest.length >= 300;
      const counts = {};
      for (const r of tagRows) for (const t of r.tags || []) counts[t] = (counts[t] || 0) + 1;
      if (tag && !counts[tag]) counts[tag] = 0;
      tagCounts = Object.entries(counts).sort((a, b) => b[1] - a[1] || a[0].localeCompare(b[0]));
    } catch (e) {
      if (my !== seq) return;
      rows = [];
      app.toast('Could not load notes');
    }
    drawList();
  }

  function drawList() {
    filterArea.replaceChildren(tagCounts.length
      ? ui.filterChips({
          options: tagCounts.map(([value, count]) => ({ value, label: '#' + value, count })),
          value: tag,
          onChange: (v) => {
            tag = v || null;
            app.track('filter', tag ? 'tag=' + tag : 'all');
            load();
          },
        })
      : null);

    if (!rows.length) {
      const filtered = query || tag;
      listBody.replaceChildren(filtered
        ? ui.emptyState({
            compact: true,
            icon: '🔍',
            title: 'No notes match',
            text: tag ? 'Try another word, or show all tags.' : 'Try another word.',
            secondary: tag ? { label: 'Show all tags', onClick: () => { tag = null; load(); } } : null,
          })
        : ui.emptyState({
            icon: '📝',
            title: 'No notes yet',
            text: 'Write down an idea, a list or what was said at a meeting.',
            action: { label: 'New note', onClick: () => newNote() },
          }));
      return;
    }

    const items = rows.map((n) => {
      const excerpt = (n.title || '').trim() ? lib.markdown.excerpt(n.body || '', 100) : '';
      const when = n.touched || n.created;
      return h('div', {
        class: 'item notes-item' + (n.id === selectedId ? ' sel' : ''),
        role: 'button',
        tabindex: '0',
        onclick: () => select(n.id),
        onkeydown: (e) => { if (e.key === 'Enter' || e.key === ' ') { e.preventDefault(); select(n.id); } },
      },
        h('div', { class: 'row', style: 'flex-wrap:nowrap;gap:6px' },
          n.pinned ? h('span', { title: 'Pinned', 'aria-label': 'Pinned' }, '📌') : null,
          h('strong', { class: 'clip grow' }, noteName(n)),
          h('small', { class: 'muted nowrap', title: app.fmt.datetime(when) }, app.fmt.ago(when))
        ),
        excerpt ? h('div', { class: 'small muted clip' }, excerpt) : null,
        (n.tags || []).length
          ? h('div', { class: 'row', style: 'gap:4px' },
              n.tags.slice(0, 3).map((t) => h('span', { class: 'badge info' }, '#' + t)),
              n.tags.length > 3 ? h('small', { class: 'muted' }, '+' + (n.tags.length - 3)) : null)
          : null
      );
    });
    listBody.replaceChildren(
      h('div', { class: 'list' }, items),
      truncated ? h('p', { class: 'small muted' }, 'Showing the most recent notes. Search to find older ones.') : null
    );
  }

  function setStatus(text) {
    if (statusEl) statusEl.textContent = text;
  }

  function savedText(d) {
    return d.edited ? 'Edited ' + app.fmt.ago(d.edited) : '';
  }

  function schedule() {
    dirty = true;
    setStatus('Editing…');
    clearTimeout(timer);
    timer = setTimeout(flush, 700);
  }

  function flush() {
    clearTimeout(timer);
    timer = null;
    if (!dirty || !draft) return saving;
    dirty = false;
    const d = draft;
    const fields = { title: d.title.trim(), body: d.body, tags: d.tags, edited: app.now() };
    saving = saving.then(async () => {
      if (draft === d) setStatus('Saving…');
      try {
        const row = await app.update('notes', d.id, fields);
        d.edited = row.edited || fields.edited;
        if (draft === d && !dirty) setStatus('Saved · ' + savedText(d).toLowerCase());
      } catch (e) {
        if (draft === d) { dirty = true; setStatus('Not saved'); }
        app.toast('Could not save the note');
      }
    });
    return saving;
  }

  async function leaveCurrent() {
    await flush();
    const d = draft;
    if (d && !d.title.trim() && !d.body.trim()) {
      try { await app.remove('notes', d.id); } catch (e) { /* keep it */ }
    }
  }

  async function select(id) {
    if (id && id === selectedId) { shell.classList.add('has-sel'); return; }
    await leaveCurrent();
    selectedId = id;
    let note = null;
    if (id) {
      const found = await app.sql(`SELECT ${FIELDS} FROM notes WHERE id = :id LIMIT 1`, { id });
      note = found[0] || null;
      if (!note) selectedId = null;
    }
    openEditor(note);
    drawList();
  }

  async function newNote() {
    newBtn.disabled = true;
    try {
      await leaveCurrent();
      const row = await app.insert('notes', { title: '', body: '', tags: tag ? [tag] : [], pinned: false, edited: app.now() });
      selectedId = row.id;
      openEditor(row, true);
      drawList();
    } catch (e) {
      app.toast('Could not create a note');
    } finally {
      newBtn.disabled = false;
    }
  }

  function openEditor(note, focusTitle) {
    clearTimeout(timer);
    timer = null;
    dirty = false;
    draft = note ? {
      id: note.id,
      title: note.title || '',
      body: note.body || '',
      tags: note.tags || [],
      pinned: !!note.pinned,
      edited: note.edited || note.created,
    } : null;
    shell.classList.toggle('has-sel', !!note);

    if (!note) {
      statusEl = null;
      editorPane.replaceChildren(ui.emptyState({
        icon: '🗒️',
        title: 'No note open',
        text: 'Pick a note from the list, or start a new one.',
        action: { label: 'New note', onClick: () => newNote() },
      }));
      return;
    }

    const d = draft;
    statusEl = h('small', { class: 'muted grow clip' }, savedText(d));

    const pinBtn = h('button', { class: 'ghost small', onclick: async () => {
      pinBtn.disabled = true;
      const next = !d.pinned;
      try {
        await app.update('notes', d.id, { pinned: next });
        d.pinned = next;
        pinBtn.textContent = next ? '📌 Unpin' : '📌 Pin';
      } catch (e) {
        app.toast('Could not change the pin');
      } finally {
        pinBtn.disabled = false;
      }
    } }, d.pinned ? '📌 Unpin' : '📌 Pin');

    const delBtn = h('button', { class: 'ghost small danger', onclick: async () => {
      const name = d.title.trim() || 'this note';
      if (!confirm(`Delete "${name}"? This cannot be undone.`)) return;
      delBtn.disabled = true;
      clearTimeout(timer);
      dirty = false;
      try {
        await saving;
        await app.remove('notes', d.id);
        selectedId = null;
        openEditor(null);
        drawList();
        app.toast('Note deleted');
      } catch (e) {
        delBtn.disabled = false;
        app.toast('Could not delete the note');
      }
    } }, 'Delete');

    const backBtn = h('button', { class: 'ghost small notes-back', onclick: () => select(null) }, '← Notes');

    const titleInput = h('input', {
      class: 'notes-title',
      value: d.title,
      placeholder: 'Title',
      'aria-label': 'Title',
      oninput: (e) => { d.title = e.target.value; schedule(); },
      onkeydown: (e) => {
        if (e.key === 'Enter') {
          e.preventDefault();
          const ta = bodyEl.querySelector('textarea');
          if (ta) ta.focus();
        }
      },
      onblur: () => flush(),
    });

    const bodyEl = ui.markdown({
      editing: true,
      value: d.body,
      rows: 14,
      placeholder: 'Write in Markdown: # headings, - lists, - [ ] tasks, **bold**',
      onChange: (text) => { d.body = text; schedule(); },
      onSubmit: (text) => { d.body = text; dirty = true; flush(); },
    });

    editorPane.replaceChildren(
      h('div', { class: 'toolbar row', style: 'flex-wrap:nowrap' }, backBtn, statusEl, pinBtn, delBtn),
      titleInput,
      ui.tagInput({
        value: d.tags,
        suggestions: tagCounts.map(([t]) => t),
        placeholder: 'Add a tag',
        onChange: (tags) => { d.tags = tags; schedule(); },
      }),
      bodyEl,
      h('small', { class: 'muted' }, 'Created ' + app.fmt.datetime(note.created || d.edited))
    );
    if (focusTitle) setTimeout(() => titleInput.focus(), 0);
  }

  app.onChange(['notes'], async () => {
    load();
    if (selectedId && draft && draft.id === selectedId) {
      const id = selectedId;
      try {
        const still = await app.sql('SELECT id FROM notes WHERE id = :id LIMIT 1', { id });
        if (!still.length && selectedId === id) {
          clearTimeout(timer);
          dirty = false;
          selectedId = null;
          openEditor(null);
          drawList();
        }
      } catch (e) { /* ignore */ }
    }
  });

  window.addEventListener('pagehide', () => { flush(); });
  document.addEventListener('visibilitychange', () => { if (document.hidden) flush(); });

  await load();
  if (params.id) await select(params.id);
  else openEditor(null);
}
