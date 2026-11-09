// title: Expenses
// description: Log what you spend in a few keystrokes and see each month's total, a bar per category and every expense grouped by day.
// tables: expenses(amount float, category keyword, spent_on keyword, note text)
async function render(root, app) {
  const money = lib.money, dates = lib.dates, stats = lib.stats;
  const CUR = 'USD';
  const fmt = (n) => money.format(n || 0, CUR);
  let month = dates.month(app.today());
  let drawToken = 0;

  root.append(h('style', null, `
    .exp-form { flex-wrap: wrap; align-items: stretch; }
    .exp-form input { min-width: 0; }
    .exp-form .amt { width: 7.5rem; flex: 0 0 auto; }
    .exp-form .cat { flex: 1 1 8rem; }
    .exp-form .day { flex: 0 0 auto; }
    .exp-form .note { flex: 2 1 12rem; }
    .exp-month { align-items: center; }
    .exp-month .label { font-weight: 600; min-width: 9rem; text-align: center; }
    .exp-day { display: flex; justify-content: space-between; align-items: baseline; margin: 1rem 0 .25rem; }
    .exp-day h3 { margin: 0; font-size: 1rem; }
    .exp-item { display: flex; gap: .6rem; align-items: center; }
    .exp-item .amount { font-variant-numeric: tabular-nums; font-weight: 600; }
    .exp-total { font-size: 2rem; font-weight: 700; font-variant-numeric: tabular-nums; }
  `));

  // ---------- the form (never redrawn) ----------
  const amountIn = h('input', { class: 'amt', type: 'text', inputmode: 'decimal', placeholder: 'Amount', 'aria-label': 'Amount', autocomplete: 'off', required: true });
  const catList = h('datalist', { id: 'exp-cats' });
  const catIn = h('input', { class: 'cat', type: 'text', placeholder: 'Category', 'aria-label': 'Category', list: 'exp-cats', autocomplete: 'off' });
  const dayIn = h('input', { class: 'day', type: 'date', value: app.today(), 'aria-label': 'Date', required: true });
  const noteIn = h('input', { class: 'note', type: 'text', placeholder: 'Note (optional)', 'aria-label': 'Note', autocomplete: 'off' });
  const addBtn = h('button', { class: 'primary', type: 'submit' }, 'Add');
  const quick = h('div', { class: 'chips' });

  async function onSubmit(e) {
    e.preventDefault();
    const amount = money.parse(amountIn.value);
    if (amount === null || !isFinite(amount) || amount === 0) {
      app.toast('Enter an amount, like 12.50');
      amountIn.focus();
      return;
    }
    const day = dates.isDay(dayIn.value) ? dayIn.value : app.today();
    const category = catIn.value.trim().toLowerCase() || 'other';
    const note = noteIn.value.trim();
    addBtn.disabled = true;
    try {
      await app.insert('expenses', { amount: money.round(amount), category, spent_on: day, note: note || null });
      app.toast(`Added ${fmt(amount)} · ${category}`);
      amountIn.value = '';
      noteIn.value = '';
      if (dates.month(day) !== month) { month = dates.month(day); }
      await draw();
      amountIn.focus();
    } catch (err) {
      app.toast('Could not save: ' + (err && err.message ? err.message : err));
    } finally {
      addBtn.disabled = false;
    }
  }

  const form = h('form', { class: 'exp-form', onsubmit: onSubmit }, amountIn, catIn, dayIn, noteIn, addBtn, catList);

  // ---------- month switcher ----------
  const monthLabel = h('span', { class: 'label' });
  const thisMonthBtn = h('button', { class: 'ghost small', type: 'button', onclick: () => { month = dates.month(app.today()); app.track('month', month); draw(); } }, 'This month');
  const switcher = h('div', { class: 'row exp-month toolbar' },
    h('button', { class: 'icon', type: 'button', 'aria-label': 'Previous month', onclick: () => { month = dates.month(dates.addMonths(month + '-01', -1)); app.track('month', month); draw(); } }, '‹'),
    monthLabel,
    h('button', { class: 'icon', type: 'button', 'aria-label': 'Next month', onclick: () => { month = dates.month(dates.addMonths(month + '-01', 1)); app.track('month', month); draw(); } }, '›'),
    thisMonthBtn
  );

  const body = h('div', { class: 'col' });

  root.append(
    h('h1', null, 'Expenses'),
    h('div', { class: 'card col' }, form, quick),
    switcher,
    body
  );

  // ---------- data ----------
  function monthDays(m) {
    const first = m + '-01';
    const days = dates.range(first, dates.endOfMonth(first));
    const params = {};
    const names = days.map((d, i) => { params['d' + i] = d; return ':d' + i; });
    return { list: names.join(', '), params };
  }

  function dayLabel(day) {
    const today = app.today();
    if (day === today) return 'Today';
    if (day === dates.addDays(today, -1)) return 'Yesterday';
    const [y, , d] = day.split('-');
    const base = `${dates.weekdayName(day, true)} ${Number(d)} ${dates.monthName(day, true)}`;
    return y === today.slice(0, 4) ? base : `${base} ${y}`;
  }

  async function drawCategories() {
    const cats = await app.sql(
      'SELECT category, COUNT(*) AS n FROM expenses WHERE category IS NOT NULL GROUP BY category ORDER BY n DESC LIMIT 50'
    );
    catList.replaceChildren(cats.map((c) => h('option', { value: c.category })));
    quick.replaceChildren(cats.slice(0, 6).map((c) =>
      h('button', { type: 'button', class: 'chip', onclick: () => { catIn.value = c.category; amountIn.focus(); } }, c.category)
    ));
  }

  async function draw() {
    const token = ++drawToken;
    const isCurrent = month === dates.month(app.today());
    monthLabel.textContent = `${dates.monthName(month)} ${month.slice(0, 4)}`;
    thisMonthBtn.style.visibility = isCurrent ? 'hidden' : 'visible';

    const { list, params } = monthDays(month);
    const [byCat, rows] = await Promise.all([
      app.sql(`SELECT category, SUM(amount) AS total, COUNT(*) AS n FROM expenses WHERE spent_on IN (${list}) GROUP BY category ORDER BY total DESC LIMIT 200`, params),
      app.sql(`SELECT id, amount, category, spent_on, note, created FROM expenses WHERE spent_on IN (${list}) ORDER BY spent_on DESC, created DESC LIMIT 500`, params),
      drawCategories(),
    ]);
    if (token !== drawToken) return;

    const total = money.sum(byCat, 'total');
    const count = stats.sum(byCat, 'n') || 0;

    if (!count) {
      body.replaceChildren(ui.emptyState({
        icon: '🧾',
        title: isCurrent ? 'Nothing spent this month yet' : `No expenses in ${dates.monthName(month)}`,
        text: 'Type an amount above, add a category and press Enter.',
        action: { label: 'Add an expense', onClick: () => amountIn.focus() },
      }));
      return;
    }

    const summary = h('div', { class: 'card col' },
      h('div', { class: 'row' },
        h('div', { class: 'col' },
          h('span', { class: 'muted small' }, isCurrent ? 'Spent this month' : `Spent in ${dates.monthName(month)}`),
          h('span', { class: 'exp-total' }, fmt(total))
        ),
        h('span', { class: 'right muted small' }, `${count} ${count === 1 ? 'expense' : 'expenses'}`)
      ),
      ui.barChart({
        data: byCat.map((c) => ({ label: c.category || 'other', value: money.round(c.total || 0) })),
        horizontal: true,
        format: (v) => fmt(v),
        height: Math.max(80, byCat.length * 34),
        onSelect: (item) => app.track('category', item.label),
      })
    );

    const groups = stats.groupBy(rows, 'spent_on');
    const days = Object.keys(groups).sort().reverse();
    const listEl = h('div', null, days.map((day) => {
      const items = groups[day];
      return [
        h('div', { class: 'exp-day' },
          h('h3', null, dayLabel(day)),
          h('span', { class: 'muted small nowrap' }, fmt(money.sum(items, 'amount')))
        ),
        h('div', { class: 'list card' }, items.map((r) =>
          h('div', { class: 'item exp-item' },
            h('div', { class: 'col grow', style: 'min-width:0;gap:.15rem' },
              h('span', null, h('span', { class: 'badge', style: `--c: ${lib.color.forName(r.category || 'other')}` }, r.category || 'other')),
              r.note ? h('span', { class: 'small clip' }, r.note) : null
            ),
            h('span', { class: 'amount nowrap' }, fmt(r.amount)),
            ui.confirmButton({
              label: '✕', confirmLabel: 'Delete?', small: true,
              onConfirm: async () => { await app.remove('expenses', r.id); app.toast('Expense deleted'); },
            })
          )
        )),
      ];
    }));

    body.replaceChildren(
      summary,
      listEl,
      rows.length >= 500 && count > rows.length
        ? h('p', { class: 'muted small center' }, `Showing the latest 500 of ${count} expenses; the totals count them all.`)
        : null
    );
  }

  app.onChange(['expenses'], () => draw());
  await draw();
}
