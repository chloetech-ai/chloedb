// title: Tickets
// description: A help desk: list support tickets with status chips, priority and assignee filters and search, open one to read and answer its reply thread, change its status and priority, and assign it to a member.
// tables: tickets(subject text, requester text, email keyword, priority keyword, status keyword, assignee keyword, body text); ticket_replies(ticket keyword, author keyword, body text)
async function render(root, app) {
  const STATUS = {
    open: { label: 'Open', tone: 'warn' },
    pending: { label: 'Pending', tone: 'info' },
    solved: { label: 'Solved', tone: 'ok' },
  };
  const STATUS_KEYS = ['open', 'pending', 'solved'];
  const PRIORITY = {
    urgent: { label: 'Urgent', tone: 'bad' },
    high: { label: 'High', tone: 'warn' },
    normal: { label: 'Normal', tone: '' },
    low: { label: 'Low', tone: '' },
  };
  const PRIORITY_KEYS = ['urgent', 'high', 'normal', 'low'];
  const statusOf = (t) => (STATUS[t.status] ? t.status : 'open');
  const priorityOf = (t) => (PRIORITY[t.priority] ? t.priority : 'normal');

  const me = await app.me().catch(() => null);
  const people = (await app.people().catch(() => [])) || [];
  const byId = {};
  for (const p of people) byId[p.id] = p;
  const personName = (id) => {
    if (!id) return '';
    if (me && id === me.id) return 'You';
    const p = byId[id];
    return p ? p.name || p.email || 'Member' : 'Former member';
  };

  const params = app.params || {};
  const state = { status: 'open', priority: '', assignee: '', q: '', selected: params.ticket || null };

  function selectEl(options, value, onchange, label) {
    const el = h('select', { onchange: (e) => onchange(e.target.value), 'aria-label': label },
      options.map((o) => h('option', { value: o.value }, o.label)));
    el.value = value;
    return el;
  }

  root.append(h('style', null, `
    .tk .tk-item { cursor: pointer; border-left: 3px solid transparent; padding-left: .5rem; }
    .tk .tk-item.on { border-left-color: var(--accent); background: color-mix(in srgb, var(--accent) 8%, transparent); }
    .tk .tk-item:focus-visible { outline: 2px solid var(--accent); outline-offset: -2px; }
    .tk .tk-line, .tk .tk-meta { flex-wrap: nowrap; min-width: 0; }
    .tk .tk-line .grow, .tk .tk-meta .clip { min-width: 0; }
    .tk .tk-title { font-weight: 600; }
    .tk .tk-subject { margin: 0; overflow-wrap: anywhere; }
    .tk .tk-msg { border: 1px solid var(--line); border-radius: var(--radius); padding: .6rem .75rem; }
    .tk .tk-msg.requester { background: var(--card); }
    .tk .tk-msg.agent { border-left: 3px solid var(--accent); }
    .tk .tk-body { white-space: pre-wrap; overflow-wrap: anywhere; margin-top: .3rem; }
    .tk .tk-field { display: flex; flex-direction: column; gap: .25rem; min-width: 0; }
    .tk .tk-new form textarea, .tk .tk-new form input { width: 100%; }
    @media (max-width: 700px) { .tk.has-sel .tk-listcol { display: none; } }
  `));

  const wrap = h('div', { class: 'tk col' });
  root.append(wrap);

  // ---------- New ticket (kept out of every redraw) ----------
  const fSubject = h('input', { placeholder: 'Subject', required: true, 'aria-label': 'Subject' });
  const fRequester = h('input', { placeholder: 'Requester name', 'aria-label': 'Requester name' });
  const fEmail = h('input', { type: 'email', placeholder: 'Requester email', 'aria-label': 'Requester email' });
  const fPriority = selectEl(PRIORITY_KEYS.map((k) => ({ value: k, label: PRIORITY[k].label + ' priority' })), 'normal', () => {}, 'Priority');
  const fBody = h('textarea', { rows: 3, placeholder: 'What do they need help with?', 'aria-label': 'Description' });
  const fSave = h('button', { type: 'submit', class: 'primary' }, 'Create ticket');
  const newBox = h('details', { class: 'tk-new' },
    h('summary', null, 'New ticket'),
    h('form', {
      class: 'col',
      onsubmit: async (e) => {
        e.preventDefault();
        const subject = fSubject.value.trim();
        const email = fEmail.value.trim();
        if (!subject) { fSubject.focus(); return; }
        if (email && !lib.validate.isEmail(email)) { app.toast('That email address does not look right'); fEmail.focus(); return; }
        fSave.disabled = true;
        try {
          const row = await app.insert('tickets', {
            subject,
            requester: fRequester.value.trim() || null,
            email: email ? lib.validate.email(email) : null,
            priority: fPriority.value || 'normal',
            status: 'open',
            assignee: null,
            body: fBody.value.trim() || null,
          });
          fSubject.value = ''; fRequester.value = ''; fEmail.value = ''; fBody.value = '';
          fPriority.value = 'normal';
          newBox.open = false;
          app.toast('Ticket created');
          if (state.status && state.status !== 'open') state.status = 'open';
          await refreshList();
          select(row.id);
        } catch (err) {
          app.toast('Could not create the ticket: ' + (err.message || err));
        } finally {
          fSave.disabled = false;
        }
      },
    },
      fSubject,
      h('div', { class: 'row' }, h('div', { class: 'grow tk-field' }, fRequester), h('div', { class: 'grow tk-field' }, fEmail), fPriority),
      fBody,
      h('div', { class: 'row' }, fSave)));

  wrap.append(h('div', { class: 'row' }, h('h1', { style: 'margin:0' }, 'Support tickets')), newBox);

  // ---------- Filters ----------
  const searchSlot = h('div', { class: 'grow', style: 'min-width: 180px' });
  function makeSearch() {
    searchSlot.replaceChildren(ui.searchBox({
      value: state.q,
      placeholder: 'Search subject, requester, email or text',
      onSearch: (q) => { state.q = q; if (q) app.track('search', q); refreshList(); },
    }));
  }
  makeSearch();

  const prioritySel = selectEl(
    [{ value: '', label: 'Any priority' }].concat(PRIORITY_KEYS.map((k) => ({ value: k, label: PRIORITY[k].label }))),
    '', (v) => { state.priority = v; app.track('filter', 'priority=' + (v || 'any')); refreshList(); }, 'Priority filter');

  const assigneeOptions = [{ value: '', label: 'Anyone' }];
  if (me) assigneeOptions.push({ value: 'me', label: 'Assigned to me' });
  assigneeOptions.push({ value: 'none', label: 'Unassigned' });
  for (const p of people) if (!me || p.id !== me.id) assigneeOptions.push({ value: p.id, label: p.name || p.email || 'Member' });
  const assigneeSel = selectEl(assigneeOptions, '', (v) => {
    state.assignee = v;
    app.track('filter', 'assignee=' + (v === 'me' || v === 'none' || !v ? (v || 'anyone') : 'member'));
    refreshList();
  }, 'Assignee filter');

  const chipsBox = h('div');
  const listInfo = h('div', { class: 'small muted' });
  const listBox = h('div');
  const listCol = h('div', { class: 'col tk-listcol' },
    h('div', { class: 'toolbar row' }, searchSlot, prioritySel, assigneeSel),
    chipsBox, listInfo, listBox);
  const detail = h('div', { class: 'tk-detail' });
  wrap.append(h('div', { class: 'split' }, listCol, detail));

  function clearFilters() {
    state.status = ''; state.priority = ''; state.assignee = ''; state.q = '';
    prioritySel.value = ''; assigneeSel.value = '';
    makeSearch();
    refreshList();
  }

  // ---------- List ----------
  let lastRows = [];
  let totalAll = 0;
  let listSeq = 0;

  async function refreshList() {
    const seq = ++listSeq;
    const where = [];
    const p = {};
    if (state.status === 'open') where.push("(status = 'open' OR status IS NULL)");
    else if (state.status) { where.push('status = :status'); p.status = state.status; }
    if (state.priority === 'normal') where.push("(priority = 'normal' OR priority IS NULL)");
    else if (state.priority) { where.push('priority = :priority'); p.priority = state.priority; }
    if (state.assignee === 'none') where.push("(assignee IS NULL OR assignee = '')");
    else if (state.assignee === 'me' && me) { where.push('assignee = :assignee'); p.assignee = me.id; }
    else if (state.assignee && state.assignee !== 'me') { where.push('assignee = :assignee'); p.assignee = state.assignee; }
    if (state.q) {
      where.push('(lower(subject) LIKE :like OR lower(requester) LIKE :like OR lower(email) LIKE :like OR lower(body) LIKE :like)');
      p.like = '%' + state.q.toLowerCase() + '%';
    }
    const sql = 'SELECT * FROM tickets' + (where.length ? ' WHERE ' + where.join(' AND ') : '') + ' ORDER BY created DESC LIMIT 500';
    let rows, counts;
    try {
      [rows, counts] = await Promise.all([
        app.sql(sql, p),
        app.sql('SELECT status, COUNT(*) AS n FROM tickets GROUP BY status'),
      ]);
    } catch (err) {
      if (seq === listSeq) listBox.replaceChildren(h('p', { class: 'empty' }, 'Could not load tickets: ' + (err.message || err)));
      return;
    }
    if (seq !== listSeq) return;

    const c = { open: 0, pending: 0, solved: 0 };
    for (const r of counts) c[STATUS[r.status] ? r.status : 'open'] += Number(r.n) || 0;
    totalAll = c.open + c.pending + c.solved;
    chipsBox.replaceChildren(ui.filterChips({
      options: STATUS_KEYS.map((k) => ({ value: k, label: STATUS[k].label, count: c[k] })),
      value: state.status || null,
      onChange: (v) => { state.status = v || ''; app.track('filter', 'status=' + (v || 'all')); refreshList(); },
    }));

    const sRank = (t) => STATUS_KEYS.indexOf(statusOf(t));
    const pRank = (t) => PRIORITY_KEYS.indexOf(priorityOf(t));
    lastRows = rows.slice().sort((a, b) =>
      sRank(a) - sRank(b) || pRank(a) - pRank(b) || String(b.created || '').localeCompare(String(a.created || '')));
    paintList();
  }

  function itemEl(t) {
    const s = statusOf(t), pr = priorityOf(t);
    return h('div', {
      class: 'item tk-item' + (t.id === state.selected ? ' on' : ''),
      role: 'button', tabindex: '0',
      onclick: () => select(t.id),
      onkeydown: (e) => { if (e.key === 'Enter' || e.key === ' ') { e.preventDefault(); select(t.id); } },
    },
      h('div', { class: 'row tk-line' },
        h('span', { class: 'grow clip tk-title' }, t.subject || '(No subject)'),
        pr !== 'normal' ? h('span', { class: 'badge ' + PRIORITY[pr].tone }, PRIORITY[pr].label) : null),
      h('div', { class: 'row small muted tk-meta' },
        h('span', { class: 'clip' }, t.requester || t.email || 'Unknown requester'),
        h('span', { class: 'badge ' + STATUS[s].tone }, STATUS[s].label),
        h('span', { class: 'nowrap' }, t.assignee ? personName(t.assignee) : 'Unassigned'),
        h('span', { class: 'right nowrap', title: app.fmt.datetime(t.created) }, app.fmt.ago(t.created))));
  }

  function paintList() {
    if (!lastRows.length) {
      listInfo.textContent = '';
      const filtered = state.status || state.priority || state.assignee || state.q;
      if (totalAll === 0) {
        listBox.replaceChildren(ui.emptyState({
          icon: '📨', title: 'No tickets yet',
          text: 'When someone asks for help, add it here with New ticket.',
          action: { label: 'New ticket', onClick: () => { newBox.open = true; fSubject.focus(); } },
        }));
      } else if (state.status === 'open' && !state.priority && !state.assignee && !state.q) {
        listBox.replaceChildren(ui.emptyState({
          icon: '✅', title: 'No open tickets', text: 'Everything has been answered.',
          action: { label: 'Show all tickets', onClick: clearFilters },
        }));
      } else {
        listBox.replaceChildren(ui.emptyState({
          icon: '🔍', title: 'No tickets match',
          text: filtered ? 'Try another search or fewer filters.' : '',
          action: { label: 'Clear filters', onClick: clearFilters },
        }));
      }
      return;
    }
    listInfo.textContent = lib.text.plural(lastRows.length, 'ticket') + (lastRows.length >= 500 ? ' (showing the newest 500)' : '');
    listBox.replaceChildren(h('div', { class: 'list' }, lastRows.map(itemEl)));
  }

  // ---------- Detail ----------
  const detailHead = h('div', { class: 'col' });
  const thread = h('div', { class: 'col' });
  let detailSeq = 0;

  function select(id) {
    state.selected = id || null;
    wrap.classList.toggle('has-sel', !!state.selected);
    paintList();
    buildDetail();
    if (state.selected && window.innerWidth < 700) window.scrollTo({ top: 0 });
  }

  function makeReplyForm(ticketId) {
    const ta = h('textarea', { rows: 3, placeholder: 'Write a reply (Enter sends, Shift+Enter for a new line)', 'aria-label': 'Reply' });
    const btn = h('button', { type: 'submit', class: 'primary' }, 'Send reply');
    const form = h('form', {
      class: 'col',
      onsubmit: async (e) => {
        e.preventDefault();
        const body = ta.value.trim();
        if (!body || btn.disabled) return;
        btn.disabled = true;
        try {
          await app.insert('ticket_replies', { ticket: ticketId, author: me ? me.id : null, body });
          ta.value = '';
        } catch (err) {
          app.toast('Could not send the reply: ' + (err.message || err));
        } finally {
          btn.disabled = false;
          ta.focus();
        }
      },
    }, ta, h('div', { class: 'row' }, btn));
    ta.onkeydown = (e) => {
      if (e.key === 'Enter' && !e.shiftKey && !e.isComposing) { e.preventDefault(); form.requestSubmit(); }
    };
    return form;
  }

  function buildDetail() {
    if (!state.selected) {
      detail.replaceChildren(ui.emptyState({
        icon: '💬', title: 'No ticket open', compact: true,
        text: 'Pick a ticket from the list to read it, reply and assign it.',
      }));
      return;
    }
    detailHead.replaceChildren(h('p', { class: 'muted' }, 'Loading…'));
    thread.replaceChildren();
    detail.replaceChildren(h('div', { class: 'card col' },
      detailHead, h('h3', { style: 'margin:0' }, 'Conversation'), thread, makeReplyForm(state.selected)));
    refreshDetail();
  }

  async function save(id, fields, what) {
    try {
      await app.update('tickets', id, fields);
      if (what) app.toast(what);
    } catch (err) {
      app.toast('Could not save: ' + (err.message || err));
      refreshDetail();
    }
  }

  async function removeTicket(id) {
    try {
      for (;;) {
        const rs = await app.sql('SELECT id FROM ticket_replies WHERE ticket = :id LIMIT 500', { id });
        if (!rs.length) break;
        for (const r of rs) await app.remove('ticket_replies', r.id);
        if (rs.length < 500) break;
      }
      await app.remove('tickets', id);
      app.toast('Ticket deleted');
      if (state.selected === id) select(null);
    } catch (err) {
      app.toast('Could not delete: ' + (err.message || err));
    }
  }

  function message(name, time, body, kind) {
    return h('div', { class: 'tk-msg ' + kind },
      h('div', { class: 'row small' },
        h('strong', null, name),
        h('span', { class: 'muted right nowrap', title: app.fmt.datetime(time) }, app.fmt.ago(time))),
      h('div', { class: 'tk-body' }, body));
  }

  async function refreshDetail() {
    const id = state.selected;
    if (!id) return;
    const seq = ++detailSeq;
    let rows, replies;
    try {
      [rows, replies] = await Promise.all([
        app.sql('SELECT * FROM tickets WHERE id = :id LIMIT 1', { id }),
        app.sql('SELECT * FROM ticket_replies WHERE ticket = :id ORDER BY created ASC LIMIT 500', { id }),
      ]);
    } catch (err) {
      if (seq === detailSeq) detailHead.replaceChildren(h('p', { class: 'muted' }, 'Could not load the ticket.'));
      return;
    }
    if (seq !== detailSeq || id !== state.selected) return;
    const t = rows[0];
    if (!t) { select(null); return; }

    const s = statusOf(t);
    const who = t.requester || t.email || 'Unknown requester';

    let assignControl;
    if (people.length) {
      assignControl = h('div', { class: 'row' },
        h('div', { class: 'grow', style: 'min-width: 160px' }, ui.peoplePicker({
          value: t.assignee || null, people, placeholder: 'Unassigned',
          onChange: (pid) => save(t.id, { assignee: pid || null }, pid ? 'Assigned to ' + personName(pid) : 'Unassigned'),
        })),
        me && t.assignee !== me.id
          ? h('button', { class: 'small ghost', onclick: () => save(t.id, { assignee: me.id }, 'Assigned to you') }, 'Assign to me')
          : null);
    } else {
      assignControl = h('span', { class: 'small muted' }, 'Nobody to assign yet: members show here once they sign in to the app.');
    }

    detailHead.replaceChildren(
      h('div', { class: 'row' },
        h('button', { class: 'ghost small', onclick: () => select(null) }, '← Tickets'),
        h('span', { class: 'right' }, ui.confirmButton({ label: 'Delete', small: true, onConfirm: () => removeTicket(t.id) }))),
      h('h2', { class: 'tk-subject' }, t.subject || '(No subject)'),
      h('div', { class: 'row small muted' },
        h('span', null, who),
        t.email ? h('a', { href: 'mailto:' + t.email }, t.email) : null,
        h('span', { class: 'nowrap' }, 'opened ' + app.fmt.datetime(t.created))),
      h('div', { class: 'row', style: 'align-items: flex-end' },
        h('label', { class: 'tk-field' }, h('span', { class: 'small muted' }, 'Status'),
          ui.segmentedControl({
            options: STATUS_KEYS.map((k) => ({ value: k, label: STATUS[k].label })),
            value: s, size: 'small', label: 'Status',
            onChange: (v) => save(t.id, { status: v }, 'Marked ' + STATUS[v].label.toLowerCase()),
          })),
        h('label', { class: 'tk-field' }, h('span', { class: 'small muted' }, 'Priority'),
          selectEl(PRIORITY_KEYS.map((k) => ({ value: k, label: PRIORITY[k].label })), priorityOf(t),
            (v) => save(t.id, { priority: v }), 'Priority'))),
      h('div', { class: 'tk-field' }, h('span', { class: 'small muted' }, 'Assignee'), assignControl));

    const msgs = [message(who, t.created, t.body || 'No description.', 'requester')];
    for (const r of replies) msgs.push(message(r.author ? personName(r.author) : 'Support', r.created, r.body || '', 'agent'));
    if (!replies.length) msgs.push(h('p', { class: 'small muted', style: 'margin:0' }, 'No replies yet. Write the first one below.'));
    thread.replaceChildren(msgs);
  }

  app.onChange(['tickets', 'ticket_replies'], (changed) => {
    if (changed.includes('tickets')) refreshList();
    refreshDetail();
  });

  wrap.classList.toggle('has-sel', !!state.selected);
  await refreshList();
  buildDetail();
}
