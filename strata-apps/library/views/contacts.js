// title: Contacts
// description: An address book: search people by name, email, phone or company, filter by group, see a person's details with email and call links, and add, edit or delete contacts.
// tables: contacts(name text, email keyword, phone keyword, company text, grp keyword, notes text, birthday keyword)
async function render(root, app) {
  const LIMIT = 500;
  let q = '';
  let group = null;
  let selectedId = null;
  let rows = [];
  let groups = [];
  let total = 0;
  let matching = 0;
  let token = 0;

  const style = h('style', null, `
    .contacts-item { cursor: pointer; align-items: center; flex-wrap: nowrap; }
    .contacts-item:hover { background: color-mix(in srgb, var(--accent) 6%, transparent); }
    .contacts-item.on { background: color-mix(in srgb, var(--accent) 14%, transparent); }
    .contacts-item .contacts-text { min-width: 0; gap: 0; }
    .contacts-detail { align-self: start; }
    .contacts-fields { display: grid; grid-template-columns: max-content 1fr; gap: 6px 14px; margin: 12px 0; }
    .contacts-fields dt { color: var(--muted); }
    .contacts-fields dd { margin: 0; overflow-wrap: anywhere; }
    .contacts-notes { white-space: pre-wrap; overflow-wrap: anywhere; }
    .contacts-head { align-items: center; flex-wrap: nowrap; }
    .contacts-head > div { min-width: 0; }
  `);

  const searchEl = ui.searchBox({
    placeholder: 'Search name, email, phone or company',
    onSearch: (text) => {
      q = text;
      if (text) app.track('search', text);
      refresh();
    },
  });
  const chipsBox = h('div');
  const countBox = h('small', { class: 'muted' });
  const listBox = h('div');
  const detailBox = h('div', { class: 'card contacts-detail' });

  root.append(
    style,
    h('div', { class: 'row' },
      h('h1', { class: 'grow' }, 'Contacts'),
      h('button', { class: 'primary', onclick: () => openForm(null) }, 'Add contact')),
    h('div', { class: 'toolbar' }, h('div', { class: 'grow' }, searchEl)),
    chipsBox,
    h('div', { class: 'split' }, h('div', { class: 'col' }, countBox, listBox), detailBox)
  );

  function clean(v) {
    if (v == null) return null;
    const s = String(v).trim();
    return s ? s : null;
  }

  function fmtDay(d) {
    if (!lib.dates.isDay(d)) return d || '';
    const [y, , dd] = d.split('-');
    return `${Number(dd)} ${lib.dates.monthName(d)} ${y}`;
  }

  function nextBirthday(d) {
    if (!lib.dates.isDay(d)) return '';
    const today = app.today();
    const md = d.slice(5);
    const at = (y) => {
      const day = `${y}-${md}`;
      return lib.dates.isDay(day) ? day : lib.dates.addDays(`${y}-02-28`, 1);
    };
    const y = Number(today.slice(0, 4));
    let next = at(y);
    if (next < today) next = at(y + 1);
    return lib.dates.relative(next, today);
  }

  function whereClause() {
    const parts = [];
    const params = {};
    if (group) { parts.push('grp = :g'); params.g = group; }
    if (q) {
      parts.push('(lower(name) LIKE :p OR lower(email) LIKE :p OR lower(company) LIKE :p OR phone LIKE :p)');
      params.p = '%' + q.toLowerCase() + '%';
    }
    return { where: parts.length ? 'WHERE ' + parts.join(' AND ') : '', params };
  }

  async function refresh() {
    const my = ++token;
    const [g, t] = await Promise.all([
      app.sql("SELECT grp, COUNT(*) AS n FROM contacts WHERE grp IS NOT NULL AND grp != '' GROUP BY grp ORDER BY grp LIMIT 200"),
      app.sql('SELECT COUNT(*) AS n FROM contacts'),
    ]);
    if (my !== token) return;
    groups = g;
    total = t[0] ? t[0].n : 0;
    if (group && !groups.some((x) => x.grp === group)) group = null;
    const { where, params } = whereClause();
    const [list, c] = await Promise.all([
      app.sql(`SELECT * FROM contacts ${where} ORDER BY name ASC LIMIT ${LIMIT}`, params),
      app.sql(`SELECT COUNT(*) AS n FROM contacts ${where}`, params),
    ]);
    if (my !== token) return;
    rows = list;
    matching = c[0] ? c[0].n : 0;
    let selected = null;
    if (selectedId) {
      selected = rows.find((r) => r.id === selectedId);
      if (!selected) {
        const one = await app.sql('SELECT * FROM contacts WHERE id = :id LIMIT 1', { id: selectedId });
        if (my !== token) return;
        selected = one[0] || null;
        if (!selected) selectedId = null;
      }
    }
    drawChips();
    drawList();
    drawDetail(selected);
  }

  function drawChips() {
    if (!groups.length) { chipsBox.replaceChildren(); return; }
    chipsBox.replaceChildren(ui.filterChips({
      options: groups.map((x) => ({ value: x.grp, label: x.grp, count: x.n })),
      value: group,
      onChange: (v) => {
        group = v || null;
        app.track('filter', 'group=' + (group || 'all'));
        refresh();
      },
    }));
  }

  function drawList() {
    if (total === 0) {
      countBox.textContent = '';
      listBox.replaceChildren(ui.emptyState({
        icon: '👤',
        title: 'No contacts yet',
        text: 'Add the people you keep in touch with: their email, phone, company and notes.',
        action: { label: 'Add contact', onClick: () => openForm(null) },
      }));
      return;
    }
    if (!rows.length) {
      countBox.textContent = '';
      listBox.replaceChildren(ui.emptyState({
        compact: true,
        icon: '🔍',
        title: 'No contacts match',
        text: q ? `Nothing found for “${q}”${group ? ' in ' + group : ''}.` : `No contacts in ${group}.`,
        action: group ? { label: 'Search all groups', onClick: () => { group = null; refresh(); } } : null,
      }));
      return;
    }
    countBox.textContent = matching > rows.length
      ? `Showing ${rows.length} of ${app.fmt.number(matching)}: search to narrow down`
      : lib.text.plural(matching, 'contact');
    listBox.replaceChildren(h('div', { class: 'list' }, rows.map((r) => {
      const sub = [r.company, r.email || r.phone].filter(Boolean).join(' · ');
      const pick = () => select(r.id);
      return h('div', {
        class: 'item row contacts-item' + (r.id === selectedId ? ' on' : ''),
        role: 'button', tabindex: '0',
        onclick: pick,
        onkeydown: (e) => { if (e.key === 'Enter' || e.key === ' ') { e.preventDefault(); pick(); } },
      },
        ui.avatar({ name: r.name, email: r.email, size: 36 }),
        h('div', { class: 'col grow contacts-text' },
          h('div', { class: 'clip' }, r.name || 'Unnamed'),
          sub ? h('small', { class: 'muted clip' }, sub) : null),
        r.grp ? h('span', { class: 'badge info nowrap' }, r.grp) : null);
    })));
  }

  function select(id) {
    selectedId = id;
    drawList();
    drawDetail(rows.find((r) => r.id === id) || null);
    if (window.matchMedia && window.matchMedia('(max-width: 700px)').matches) {
      detailBox.scrollIntoView({ behavior: 'smooth', block: 'start' });
    }
  }

  function drawDetail(c) {
    if (!c) {
      detailBox.replaceChildren(ui.emptyState({
        compact: true,
        icon: '📇',
        title: total ? 'Choose a contact' : 'Details show here',
        text: total ? 'Tap someone in the list to see their details.' : 'Once you add a contact, their details appear here.',
      }));
      return;
    }
    const fields = [];
    const add = (label, value) => { if (value) fields.push(h('dt', null, label), h('dd', null, value)); };
    add('Email', c.email ? h('a', { href: 'mailto:' + c.email }, c.email) : null);
    if (c.phone) {
      const tel = lib.phones.tel(c.phone);
      const shown = lib.phones.format(c.phone) || c.phone;
      add('Phone', tel ? h('a', { href: tel }, shown) : shown);
    }
    add('Company', c.company);
    add('Group', c.grp ? h('span', { class: 'badge info' }, c.grp) : null);
    if (c.birthday) {
      const rel = nextBirthday(c.birthday);
      add('Birthday', h('span', null, fmtDay(c.birthday), rel ? h('small', { class: 'muted' }, ` · next ${rel}`) : null));
    }
    const del = async (btn) => {
      if (!confirm(`Delete ${c.name || 'this contact'}?`)) return;
      btn.disabled = true;
      try {
        await app.remove('contacts', c.id);
        if (selectedId === c.id) selectedId = null;
        app.toast('Contact deleted');
        refresh();
      } catch (e) {
        btn.disabled = false;
        app.toast('Could not delete: ' + (e.message || e));
      }
    };
    const delBtn = h('button', { class: 'danger small', onclick: () => del(delBtn) }, 'Delete');
    detailBox.replaceChildren(
      h('div', { class: 'row contacts-head' },
        ui.avatar({ name: c.name, email: c.email, size: 56 }),
        h('div', { class: 'grow' },
          h('h2', { style: 'margin:0;overflow-wrap:anywhere' }, c.name || 'Unnamed'),
          c.company ? h('div', { class: 'muted clip' }, c.company) : null)),
      fields.length ? h('dl', { class: 'contacts-fields' }, fields) : h('p', { class: 'muted' }, 'No details yet.'),
      c.notes ? h('div', { class: 'section' }, h('h3', null, 'Notes'), h('p', { class: 'contacts-notes' }, c.notes)) : null,
      h('div', { class: 'row' },
        h('button', { class: 'small', onclick: () => openForm(c) }, 'Edit'),
        delBtn,
        h('small', { class: 'muted right' }, 'Added ' + app.fmt.date(c.created)))
    );
  }

  function openForm(c) {
    let dlg;
    const known = groups.map((x) => x.grp);
    const form = ui.form({
      fields: [
        { name: 'name', label: 'Name', required: true, placeholder: 'Full name' },
        { name: 'email', label: 'Email', type: 'email', placeholder: 'name@example.com' },
        { name: 'phone', label: 'Phone', placeholder: '+1 415 555 0123' },
        { name: 'company', label: 'Company' },
        { name: 'grp', label: 'Group', placeholder: 'Family, Work, Friends…',
          help: known.length ? 'Existing: ' + known.slice(0, 8).join(', ') : null },
        { name: 'birthday', label: 'Birthday', type: 'date' },
        { name: 'notes', label: 'Notes', type: 'textarea' },
      ],
      values: c ? {
        name: c.name || '', email: c.email || '', phone: c.phone || '', company: c.company || '',
        grp: c.grp || '', birthday: c.birthday || '', notes: c.notes || '',
      } : { grp: group || '' },
      submitLabel: c ? 'Save' : 'Add contact',
      onCancel: () => dlg.close(),
      onSubmit: async (v) => {
        const name = clean(v.name);
        if (!name) throw new Error('Enter a name.');
        const email = clean(v.email) ? String(v.email).trim().toLowerCase() : null;
        if (email && !lib.validate.isEmail(email)) throw new Error('That email address does not look right.');
        const birthday = clean(v.birthday);
        if (birthday && !lib.dates.isDay(birthday)) throw new Error('Enter the birthday as a date.');
        let grp = clean(v.grp);
        if (grp) {
          const same = known.find((k) => k.toLowerCase() === grp.toLowerCase());
          if (same) grp = same;
        }
        const row = {
          name, email, phone: clean(v.phone), company: clean(v.company),
          grp, birthday, notes: clean(v.notes),
        };
        if (c) {
          await app.update('contacts', c.id, row);
          app.toast('Contact saved');
        } else {
          const stored = await app.insert('contacts', row);
          selectedId = stored.id;
          app.toast('Contact added');
        }
        dlg.close();
        refresh();
      },
    });
    dlg = ui.dialog({ title: c ? 'Edit contact' : 'New contact', body: form, actions: [] });
    document.body.append(dlg);
  }

  app.onChange(['contacts'], () => refresh());
  await refresh();
}
