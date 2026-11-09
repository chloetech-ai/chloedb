// title: Orders
// description: Orders on a board with a column per stage (new, preparing, ready, done): add an order, move it along with buttons, see the count in each column and today's revenue.
// tables: orders(customer text, items text, total float, status keyword, due timestamp, notes text)
async function render(root, app) {
  const STAGES = [
    { id: 'new', title: 'New', next: 'Start' },
    { id: 'preparing', title: 'Preparing', next: 'Ready' },
    { id: 'ready', title: 'Ready', next: 'Done' },
    { id: 'done', title: 'Done' },
  ];
  const DONE_SHOWN = 30;
  const stageIndex = (s) => {
    const i = STAGES.findIndex((x) => x.id === s);
    return i < 0 ? 0 : i; // a missing or unknown status counts as new
  };
  const money = (n) => (typeof n === 'number' && isFinite(n) ? app.fmt.money(n) : '');

  root.append(h('style', null, `
    .orders-board { overflow-x: auto; padding-bottom: 4px; }
    .orders-board .columns { min-width: min-content; }
    .orders-board .column { min-width: 230px; flex: 1; }
    .orders-board .column-head { display: flex; align-items: center; gap: 8px; margin-bottom: 8px; }
    .orders-board .column-head h3 { margin: 0; }
    .orders-board .order { display: flex; flex-direction: column; gap: 6px; margin-bottom: 8px; }
    .orders-board .order .items { white-space: pre-line; overflow-wrap: anywhere; }
    .orders-board .order .notes { font-style: italic; overflow-wrap: anywhere; }
    .orders-board .order .actions { display: flex; gap: 6px; flex-wrap: wrap; align-items: center; }
    @media (max-width: 600px) {
      .orders-board .columns { flex-direction: column; min-width: 0; }
      .orders-board .column { min-width: 0; }
    }
  `));

  root.append(h('h1', null, 'Orders'));

  // The form stays outside what redraws.
  const formBox = h('details', { class: 'card section' }, h('summary', null, h('strong', null, 'New order')));
  formBox.append(ui.form({
    columns: 2,
    resetOnSubmit: true,
    submitLabel: 'Add order',
    fields: [
      { name: 'customer', label: 'Customer', required: true, placeholder: 'Name' },
      { name: 'total', label: 'Total', type: 'money', required: true, min: 0 },
      { name: 'items', label: 'Items', type: 'textarea', required: true, placeholder: '2 × Flat white\n1 × Croissant' },
      { name: 'due', label: 'Due (optional)', type: 'datetime' },
      { name: 'notes', label: 'Notes (optional)', type: 'textarea' },
    ],
    onSubmit: async (v) => {
      const total = typeof v.total === 'number' ? v.total : lib.money.parse(String(v.total ?? ''));
      if (total == null || !isFinite(total) || total < 0) throw new Error('Enter the total as an amount, such as 12.50');
      let due = null;
      if (v.due) {
        const d = new Date(v.due);
        if (isNaN(d.getTime())) throw new Error('The due time is not valid');
        due = d.toISOString();
      }
      await app.insert('orders', {
        customer: String(v.customer || '').trim(),
        items: String(v.items || '').trim(),
        total: lib.money.round(total),
        status: 'new',
        due,
        notes: String(v.notes || '').trim() || null,
      });
      app.toast('Order added');
      refresh();
    },
  }));

  const statsBox = h('div', { class: 'section' });
  const emptyBox = h('div');
  const boardBox = h('div', { class: 'orders-board' });
  root.append(statsBox, formBox, emptyBox, boardBox);

  async function move(row, to, btn) {
    btn.disabled = true;
    try {
      await app.update('orders', row.id, { status: to });
      app.toast(`${row.customer || 'Order'}: ${STAGES[stageIndex(to)].title.toLowerCase()}`);
      refresh();
    } catch (e) {
      btn.disabled = false;
      app.toast('Could not move the order');
    }
  }

  function card(row) {
    const i = stageIndex(row.status);
    const stage = STAGES[i];
    const isDone = stage.id === 'done';
    let dueLine = null;
    if (row.due) {
      const late = !isDone && new Date(row.due).getTime() < Date.now();
      const sameDay = lib.dates.fromTime(row.due) === app.today();
      dueLine = h('div', { class: 'row small' },
        h('span', { class: 'muted' }, 'Due ' + (sameDay ? app.fmt.time(row.due) : app.fmt.datetime(row.due))),
        late ? h('span', { class: 'badge bad' }, 'Late') : null);
    }
    const back = i > 0 ? h('button', {
      class: 'small ghost', title: 'Back to ' + STAGES[i - 1].title,
      onclick: (e) => move(row, STAGES[i - 1].id, e.currentTarget),
    }, '← ' + STAGES[i - 1].title) : null;
    const fwd = stage.next ? h('button', {
      class: 'small primary',
      onclick: (e) => move(row, STAGES[i + 1].id, e.currentTarget),
    }, stage.next + ' →') : null;
    return h('div', { class: 'card order' },
      h('div', { class: 'row' },
        h('strong', { class: 'grow clip' }, row.customer || 'No name'),
        h('span', { class: 'nowrap' }, money(row.total))),
      row.items ? h('div', { class: 'small items' }, row.items) : null,
      dueLine,
      row.notes ? h('div', { class: 'small muted notes' }, row.notes) : null,
      h('div', { class: 'small muted' }, 'Taken ' + app.fmt.ago(row.created)),
      h('div', { class: 'actions' }, back, fwd,
        h('span', { class: 'right' }, ui.confirmButton({
          label: 'Delete', small: true,
          onConfirm: async () => {
            await app.remove('orders', row.id);
            app.toast('Order deleted');
            refresh();
          },
        }))));
  }

  let serial = 0;
  async function refresh() {
    const mine = ++serial;
    const from = new Date(app.today() + 'T00:00:00').toISOString();
    const cols = 'id, customer, items, total, status, due, notes, created';
    const [open, done, counts, rev] = await Promise.all([
      app.sql(`SELECT ${cols} FROM orders WHERE status IS NULL OR status != 'done' ORDER BY created ASC LIMIT 500`),
      app.sql(`SELECT ${cols} FROM orders WHERE status = 'done' ORDER BY created DESC LIMIT ${DONE_SHOWN}`),
      app.sql('SELECT status, COUNT(*) AS n FROM orders GROUP BY status LIMIT 500'),
      app.sql('SELECT SUM(total) AS total, COUNT(*) AS n FROM orders WHERE created >= :from', { from }),
    ]);
    if (mine !== serial) return; // a newer refresh is on its way

    const count = { new: 0, preparing: 0, ready: 0, done: 0 };
    for (const c of counts) count[STAGES[stageIndex(c.status)].id] += Number(c.n) || 0;
    const all = count.new + count.preparing + count.ready + count.done;
    const openCount = count.new + count.preparing + count.ready;

    const byStage = { new: [], preparing: [], ready: [], done };
    for (const r of open) byStage[STAGES[stageIndex(r.status)].id].push(r);
    // Open orders: soonest due first, those without a due time after, oldest first.
    for (const id of ['new', 'preparing', 'ready']) byStage[id] = lib.list.sortBy(byStage[id], ['due', 'created']);

    const todayN = Number(rev[0]?.n) || 0;
    statsBox.replaceChildren(ui.statTiles({ items: [
      { label: "Today's revenue", value: Number(rev[0]?.total) || 0, format: 'money',
        hint: lib.text.plural(todayN, 'order') + ' taken today' },
      { label: 'Open orders', value: openCount, hint: count.ready ? `${count.ready} ready to hand over` : null },
    ] }));

    emptyBox.replaceChildren(all === 0 ? ui.emptyState({
      icon: '🧾', title: 'No orders yet',
      text: 'Add the first order: it starts in New, then moves along to Done with the buttons on its card.',
      action: { label: 'New order', onClick: () => { formBox.open = true; formBox.querySelector('input, textarea')?.focus(); } },
    }) : null);

    boardBox.replaceChildren(all === 0 ? null : h('div', { class: 'columns' }, STAGES.map((s) => {
      const rows = byStage[s.id];
      const more = s.id === 'done' && count.done > rows.length
        ? h('p', { class: 'small muted center' }, `Showing the latest ${rows.length} of ${count.done}`) : null;
      return h('div', { class: 'column' },
        h('div', { class: 'column-head' }, h('h3', null, s.title), h('span', { class: 'badge' }, count[s.id])),
        rows.length ? rows.map(card) : h('div', { class: 'empty small' },
          s.id === 'new' ? 'No new orders' : s.id === 'done' ? 'Nothing handed over yet' : 'Nothing ' + s.title.toLowerCase()),
        more);
    })));
  }

  app.onChange(['orders'], refresh);
  await refresh();
}
