// title: Inventory
// description: Stock on hand with SKU, quantity, reorder level and location; filter low and out of stock items, adjust quantities with quick +/- buttons that log a movement, and see the total stock value.
// tables: items(name text, sku keyword, qty int, reorder_at int, cost float, location keyword); stock_moves(item keyword, change int, reason text)
async function render(root, app) {
  const PAGE_SIZE = 25;
  let items = [];
  let filter = '';
  let query = '';
  let page = 1;
  const busy = new Set();

  const toInt = (v) => {
    if (v === '' || v == null) return null;
    const n = Number(v);
    return Number.isFinite(n) ? Math.round(n) : null;
  };
  const toFloat = (v) => {
    if (v === '' || v == null) return null;
    const n = typeof v === 'number' ? v : lib.money.parse(String(v));
    return n == null || !Number.isFinite(n) ? null : n;
  };
  const clean = (v) => (v == null ? '' : String(v).trim());
  const stateOf = (it) =>
    lib.inventory.status(it.qty || 0, { reorderAt: it.reorder_at == null ? undefined : it.reorder_at });
  const STATE = {
    out: { label: 'Out', cls: 'bad' },
    low: { label: 'Low', cls: 'warn' },
  };

  const style = h('style', null, `
    .inv-item { display: flex; align-items: center; gap: 10px; }
    .inv-main { flex: 1; min-width: 0; cursor: pointer; }
    .inv-main:hover .title { text-decoration: underline; }
    .inv-adjust { display: flex; align-items: center; gap: 6px; flex-shrink: 0; }
    .inv-qty { min-width: 3ch; text-align: center; font-variant-numeric: tabular-nums; font-weight: 600; }
    .inv-form { display: flex; flex-wrap: wrap; gap: 8px; }
    .inv-form input { flex: 1 1 120px; min-width: 0; }
    .inv-form .inv-name { flex: 2 1 200px; }
    .inv-form button { flex: 0 0 auto; }
  `);

  const statsBox = h('div');
  const chipsBox = h('div');
  const listBox = h('div');
  const pagerBox = h('div');

  // ---------- add form (kept out of what redraws) ----------
  const f = {
    name: h('input', { class: 'inv-name', placeholder: 'Item name', required: true, 'aria-label': 'Item name' }),
    sku: h('input', { placeholder: 'SKU', 'aria-label': 'SKU' }),
    qty: h('input', { type: 'number', min: 0, step: 1, placeholder: 'Qty', 'aria-label': 'Quantity' }),
    reorder_at: h('input', { type: 'number', min: 0, step: 1, placeholder: 'Reorder at', 'aria-label': 'Reorder level' }),
    cost: h('input', { type: 'number', min: 0, step: '0.01', placeholder: 'Unit cost', 'aria-label': 'Unit cost' }),
    location: h('input', { placeholder: 'Location', 'aria-label': 'Location' }),
  };
  const addBtn = h('button', { type: 'submit', class: 'primary' }, 'Add item');
  const addForm = h('form', { class: 'inv-form', onsubmit: addItem },
    f.name, f.sku, f.qty, f.reorder_at, f.cost, f.location, addBtn);

  async function addItem(e) {
    e.preventDefault();
    const name = clean(f.name.value);
    if (!name) { f.name.focus(); return; }
    const qty = Math.max(0, toInt(f.qty.value) || 0);
    const reorder = toInt(f.reorder_at.value);
    const cost = toFloat(f.cost.value);
    const sku = clean(f.sku.value);
    if (sku && items.some((it) => (it.sku || '').toLowerCase() === sku.toLowerCase())) {
      if (!confirm(`Another item already has SKU ${sku}. Add anyway?`)) return;
    }
    addBtn.disabled = true;
    try {
      const row = await app.insert('items', {
        name,
        sku: sku || null,
        qty,
        reorder_at: reorder == null ? null : Math.max(0, reorder),
        cost: cost == null ? null : Math.max(0, cost),
        location: clean(f.location.value) || null,
      });
      if (qty > 0) await app.insert('stock_moves', { item: row.id, change: qty, reason: 'Initial stock' });
      addForm.reset();
      f.name.focus();
      app.toast(`Added ${name}`);
      await load();
    } catch (err) {
      app.toast('Could not add the item: ' + (err.message || err));
    } finally {
      addBtn.disabled = false;
    }
  }

  // ---------- data ----------
  async function load() {
    items = await app.sql(
      'SELECT id, name, sku, qty, reorder_at, cost, location FROM items ORDER BY name ASC LIMIT 500');
    draw();
  }

  async function adjust(it, delta) {
    if (busy.has(it.id)) return;
    busy.add(it.id);
    draw();
    try {
      const [cur] = await app.sql('SELECT qty FROM items WHERE id = :id LIMIT 1', { id: it.id });
      if (!cur) { app.toast('That item was removed'); return; }
      const now = cur.qty || 0;
      const next = now + delta;
      if (next < 0) { app.toast(`${it.name} is already out of stock`); return; }
      await app.update('items', it.id, { qty: next });
      await app.insert('stock_moves', { item: it.id, change: delta, reason: 'Quick adjustment' });
      it.qty = next;
    } catch (err) {
      app.toast('Could not save: ' + (err.message || err));
    } finally {
      busy.delete(it.id);
      await load();
    }
  }

  function openEdit(it) {
    let dlg;
    const formEl = ui.form({
      fields: [
        { name: 'name', label: 'Name', required: true },
        { name: 'sku', label: 'SKU' },
        { name: 'qty', label: 'Quantity on hand', type: 'number', min: 0, step: 1, help: 'A change here is logged as a count correction.' },
        { name: 'reorder_at', label: 'Reorder at', type: 'number', min: 0, step: 1, help: 'Marked low at or below this quantity.' },
        { name: 'cost', label: 'Unit cost', type: 'money', min: 0 },
        { name: 'location', label: 'Location' },
      ],
      values: {
        name: it.name || '', sku: it.sku || '', qty: it.qty || 0,
        reorder_at: it.reorder_at == null ? '' : it.reorder_at,
        cost: it.cost == null ? '' : it.cost, location: it.location || '',
      },
      columns: 2,
      submitLabel: 'Save',
      onCancel: () => dlg.close(),
      onSubmit: async (v) => {
        const name = clean(v.name);
        if (!name) throw new Error('Give the item a name');
        const qty = Math.max(0, toInt(v.qty) || 0);
        const reorder = toInt(v.reorder_at);
        const cost = toFloat(v.cost);
        const [cur] = await app.sql('SELECT qty FROM items WHERE id = :id LIMIT 1', { id: it.id });
        if (!cur) throw new Error('That item was removed');
        await app.update('items', it.id, {
          name,
          sku: clean(v.sku) || null,
          qty,
          reorder_at: reorder == null ? null : Math.max(0, reorder),
          cost: cost == null ? null : Math.max(0, cost),
          location: clean(v.location) || null,
        });
        const diff = qty - (cur.qty || 0);
        if (diff !== 0) await app.insert('stock_moves', { item: it.id, change: diff, reason: 'Count correction' });
        dlg.close();
        app.toast('Saved');
        await load();
      },
    });
    dlg = ui.dialog({
      title: it.name || 'Item',
      body: formEl,
      actions: [{
        label: 'Delete item',
        danger: true,
        onClick: () => {
          if (!confirm(`Delete ${it.name}? Its stock movements stay in the log.`)) return false;
          app.remove('items', it.id)
            .then(() => { app.toast('Deleted'); return load(); })
            .catch((err) => app.toast('Could not delete: ' + (err.message || err)));
        },
      }],
    });
    document.body.append(dlg);
  }

  // ---------- drawing ----------
  function draw() {
    const withState = items.map((it) => ({ it, state: stateOf(it) }));
    const lowCount = withState.filter((x) => x.state === 'low').length;
    const outCount = withState.filter((x) => x.state === 'out').length;
    const units = items.reduce((s, it) => s + Math.max(0, it.qty || 0), 0);
    const value = lib.inventory.value(items, { qty: 'qty', cost: 'cost' });

    statsBox.replaceChildren(items.length ? ui.statTiles({
      items: [
        { label: 'Items', value: items.length, format: 'number' },
        { label: 'Units on hand', value: units, format: 'number' },
        { label: 'Stock value', value, format: 'money', hint: items.some((it) => it.cost == null) ? 'Items without a cost count as 0' : undefined },
        { label: 'Low or out', value: lowCount + outCount, format: 'number', hint: `${outCount} out of stock`,
          onClick: () => setFilter(outCount ? 'out' : 'low') },
      ],
    }) : null);

    chipsBox.replaceChildren(ui.filterChips({
      options: [
        { value: 'low', label: 'Low stock', count: lowCount },
        { value: 'out', label: 'Out of stock', count: outCount },
      ],
      value: filter || null,
      onChange: (v) => setFilter(v || ''),
    }));

    if (!items.length) {
      listBox.replaceChildren(ui.emptyState({
        icon: '📦', title: 'No stock yet',
        text: 'Add your first item with the form above: its name, SKU, how many you have and where it is kept.',
        action: { label: 'Add an item', onClick: () => f.name.focus() },
      }));
      pagerBox.replaceChildren();
      return;
    }

    const q = lib.text.normalize(query);
    const shown = withState.filter(({ it, state }) => {
      if (filter && state !== filter) return false;
      if (!q) return true;
      return lib.text.normalize([it.name, it.sku, it.location].filter(Boolean).join(' ')).includes(q);
    });

    if (!shown.length) {
      listBox.replaceChildren(ui.emptyState({
        compact: true, icon: filter && !q ? '✅' : '🔍',
        title: filter && !q ? (filter === 'out' ? 'Nothing is out of stock' : 'Nothing is running low') : 'Nothing matches',
        text: q ? `No item matches “${query}”.` : 'Every item is above its reorder level.',
        action: { label: 'Show all items', onClick: () => { filter = ''; page = 1; draw(); } },
      }));
      pagerBox.replaceChildren();
      return;
    }

    const pg = lib.paging.page(shown, page, PAGE_SIZE);
    page = pg.page;

    listBox.replaceChildren(h('div', { class: 'list' }, pg.items.map(({ it, state }) => {
      const s = STATE[state];
      const meta = [
        it.sku ? `SKU ${it.sku}` : null,
        it.location || null,
        it.cost != null ? `${app.fmt.money(it.cost)} each` : null,
        it.reorder_at != null ? `reorder at ${it.reorder_at}` : null,
      ].filter(Boolean).join(' · ');
      const saving = busy.has(it.id);
      return h('div', { class: 'item inv-item' },
        h('div', { class: 'inv-main', onclick: () => openEdit(it), title: 'Edit item' },
          h('div', { class: 'row', style: 'gap:6px;flex-wrap:nowrap' },
            h('span', { class: 'title clip' }, it.name || 'Untitled'),
            s ? h('span', { class: `badge ${s.cls}` }, s.label) : null),
          meta ? h('div', { class: 'small muted clip' }, meta) : null),
        h('div', { class: 'inv-adjust' },
          h('button', {
            class: 'icon small', 'aria-label': `Remove one ${it.name}`, title: 'Remove one',
            disabled: saving || (it.qty || 0) <= 0, onclick: () => adjust(it, -1),
          }, '−'),
          h('span', { class: 'inv-qty' }, app.fmt.number(it.qty || 0)),
          h('button', {
            class: 'icon small', 'aria-label': `Add one ${it.name}`, title: 'Add one',
            disabled: saving, onclick: () => adjust(it, 1),
          }, '+')));
    })));

    pagerBox.replaceChildren(
      ui.pagination({ page, pages: pg.pages, onPage: (n) => { page = n; draw(); } }),
      items.length >= 500 ? h('p', { class: 'small muted center' }, 'Showing the first 500 items by name.') : null);
  }

  function setFilter(v) {
    filter = v;
    page = 1;
    if (v) app.track('filter', `stock=${v}`);
    draw();
  }

  root.append(
    style,
    h('h1', null, 'Inventory'),
    statsBox,
    h('div', { class: 'card section' }, h('h3', null, 'Add an item'), addForm),
    h('div', { class: 'toolbar col' },
      ui.searchBox({
        placeholder: 'Search name, SKU or location',
        onSearch: (text) => {
          query = text; page = 1;
          if (text) app.track('search', text);
          draw();
        },
      }),
      chipsBox),
    listBox,
    pagerBox);

  app.onChange(['items'], load);
  await load();
}
