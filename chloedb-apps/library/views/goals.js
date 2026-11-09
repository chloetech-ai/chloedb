// title: Goals
// description: Goals with a target, a deadline and a progress bar each; log progress, see whether each goal is on track or behind for its deadline, and mark goals done.
// tables: goals(title text, target float, unit text, deadline keyword, done bool); goal_updates(goal keyword, amount float, note text)
async function render(root, app) {
  const today = app.today();
  let filter = 'active';
  let goals = [];
  let totals = {};

  const num = (n) => app.fmt.number(Math.round((Number(n) || 0) * 100) / 100);
  const parseNum = (s) => {
    const v = parseFloat(String(s).replace(',', '.'));
    return Number.isFinite(v) ? v : null;
  };

  root.append(h('style', null, `
    .goals progress { width: 100%; height: 10px; }
    .goals .goal { display: flex; flex-direction: column; gap: 8px; }
    .goals .goal-title { font-weight: 600; min-width: 0; overflow-wrap: anywhere; }
    .goals .add input { flex: 1 1 140px; min-width: 0; }
    .goals .add input.short { flex: 0 1 100px; }
    .goals .err { color: var(--bad); }
    .goals .hist { max-height: 260px; overflow: auto; }
  `));

  // ---- add form (never redrawn) ----
  const fTitle = h('input', { placeholder: 'Goal, e.g. Run 100 km', maxlength: 120, 'aria-label': 'Goal' });
  const fTarget = h('input', { type: 'number', step: 'any', min: '0', placeholder: 'Target', class: 'short', inputmode: 'decimal', 'aria-label': 'Target' });
  const fUnit = h('input', { placeholder: 'Unit (km, €, pages)', class: 'short', maxlength: 30, 'aria-label': 'Unit' });
  const fDeadline = h('input', { type: 'date', min: today, class: 'short', 'aria-label': 'Deadline' });
  const fBtn = h('button', { class: 'primary', type: 'submit' }, 'Add goal');
  const fErr = h('small', { class: 'err' });

  async function addGoal(e) {
    e.preventDefault();
    fErr.textContent = '';
    const title = fTitle.value.trim();
    const target = parseNum(fTarget.value);
    if (!title) { fErr.textContent = 'Give the goal a name.'; fTitle.focus(); return; }
    if (target === null || target <= 0) { fErr.textContent = 'Set a target above 0.'; fTarget.focus(); return; }
    fBtn.disabled = true;
    try {
      await app.insert('goals', {
        title, target, unit: fUnit.value.trim() || null,
        deadline: fDeadline.value || null, done: false,
      });
      fTitle.value = ''; fTarget.value = ''; fUnit.value = ''; fDeadline.value = '';
      fTitle.focus();
      if (filter === 'done') { filter = 'active'; }
      await load();
    } catch (err) {
      fErr.textContent = 'Could not save: ' + (err.message || err);
    } finally {
      fBtn.disabled = false;
    }
  }

  const form = h('form', { class: 'add', onsubmit: addGoal }, fTitle, fTarget, fUnit, fDeadline, fBtn);

  const kpis = h('div');
  const toolbar = h('div', { class: 'toolbar' });
  const list = h('div', { class: 'col' });

  root.append(h('div', { class: 'goals col' },
    h('h1', null, 'Goals'),
    h('div', { class: 'card col' }, form, fErr),
    kpis, toolbar, list,
  ));

  // ---- progress and pace ----
  function info(g) {
    const cur = totals[g.id] ? Number(totals[g.id].total) || 0 : 0;
    const target = Number(g.target);
    const hasTarget = Number.isFinite(target) && target > 0;
    const pct = hasTarget ? Math.max(0, cur / target) : null;
    const validDeadline = g.deadline && lib.dates.isDay(g.deadline);
    let state = null;
    let hint = '';
    if (g.done) {
      state = { label: 'Done', tone: 'ok' };
    } else if (hasTarget && cur >= target) {
      state = { label: 'Reached', tone: 'ok' };
      hint = 'Target reached. Mark it done when you are finished.';
    } else if (validDeadline) {
      const left = lib.dates.diffDays(today, g.deadline);
      if (left < 0) {
        state = { label: 'Overdue', tone: 'bad' };
      } else if (hasTarget) {
        const start = lib.dates.fromTime(g.created) || today;
        const span = lib.dates.diffDays(start, g.deadline);
        const elapsed = span > 0
          ? Math.min(1, Math.max(0, lib.dates.diffDays(start, today) / span))
          : 1;
        const expected = target * elapsed;
        const onTrack = cur + 1e-9 >= expected;
        state = onTrack ? { label: 'On track', tone: 'info' } : { label: 'Behind', tone: 'warn' };
        const remaining = target - cur;
        const daysLeft = left + 1; // today counts
        const unit = g.unit ? ' ' + g.unit : '';
        hint = `${num(remaining)}${unit} to go · about ${num(remaining / daysLeft)}${unit} a day` +
          (onTrack ? '' : ` · ${num(expected - cur)}${unit} behind pace`);
      }
    }
    return { cur, target, hasTarget, pct, state, hint, validDeadline };
  }

  // ---- data ----
  async function load() {
    const [g, sums] = await Promise.all([
      app.sql('SELECT id, title, target, unit, deadline, done, created FROM goals ORDER BY created DESC LIMIT 500'),
      app.sql('SELECT goal, SUM(amount) AS total, COUNT(*) AS n, MAX(created) AS last FROM goal_updates GROUP BY goal LIMIT 500'),
    ]);
    goals = g;
    totals = {};
    for (const s of sums) totals[s.goal] = s;
    draw();
  }

  function sorted(rows) {
    const active = rows.filter((g) => !g.done).sort((a, b) => {
      if (a.deadline && b.deadline && a.deadline !== b.deadline) return a.deadline < b.deadline ? -1 : 1;
      if (!!a.deadline !== !!b.deadline) return a.deadline ? -1 : 1;
      return String(a.title || '').localeCompare(String(b.title || ''));
    });
    const done = rows.filter((g) => g.done); // newest first, as queried
    return [...active, ...done];
  }

  function draw() {
    const infos = new Map(goals.map((g) => [g.id, info(g)]));
    const active = goals.filter((g) => !g.done);
    const doneCount = goals.length - active.length;
    const count = (label) => active.filter((g) => infos.get(g.id).state && infos.get(g.id).state.label === label).length;

    kpis.replaceChildren(goals.length ? ui.statTiles({ items: [
      { label: 'Active', value: active.length },
      { label: 'On track', value: count('On track') + count('Reached') },
      { label: 'Behind', value: count('Behind') + count('Overdue') },
      { label: 'Done', value: doneCount },
    ] }) : null);

    toolbar.replaceChildren(goals.length ? ui.segmentedControl({
      options: [
        { value: 'active', label: 'Active', count: active.length },
        { value: 'done', label: 'Done', count: doneCount },
        { value: 'all', label: 'All', count: goals.length },
      ],
      value: filter,
      size: 'small',
      label: 'Show goals',
      onChange: (v) => { filter = v; app.track('filter', 'goals=' + v); draw(); },
    }) : null);

    if (!goals.length) {
      list.replaceChildren(ui.emptyState({
        icon: '🎯', title: 'No goals yet', compact: true,
        text: 'Add a goal above with a target and, if you like, a deadline. Then log progress as you go.',
        action: { label: 'Add a goal', onClick: () => fTitle.focus() },
      }));
      return;
    }

    const shown = sorted(goals.filter((g) => filter === 'all' || (filter === 'done' ? g.done : !g.done)));
    if (!shown.length) {
      list.replaceChildren(ui.emptyState({
        compact: true,
        title: filter === 'done' ? 'No finished goals yet' : 'No active goals',
        text: filter === 'done' ? 'Goals you mark done show up here.' : 'Add a new goal above.',
      }));
      return;
    }
    list.replaceChildren(shown.map((g) => card(g, infos.get(g.id))));
  }

  function card(g, i) {
    const unit = g.unit ? ' ' + g.unit : '';
    const last = totals[g.id] && totals[g.id].last;
    const dueText = i.validDeadline
      ? `${g.done ? 'Deadline' : 'Due'} ${app.fmt.date(g.deadline)}` +
        (g.done ? '' : ` · ${lib.deadlines.label(g.deadline, { today })}`)
      : 'No deadline';
    return h('div', { class: 'card goal' },
      h('div', { class: 'row' },
        h('span', { class: 'goal-title grow' + (g.done ? ' strike muted' : '') }, g.title || 'Untitled goal'),
        i.state ? h('span', { class: 'badge ' + i.state.tone }, i.state.label) : null,
      ),
      i.hasTarget ? h('progress', { value: Math.min(i.cur, i.target), max: i.target }) : null,
      h('div', { class: 'row small' },
        h('span', null, h('strong', null, num(i.cur)), i.hasTarget ? ` / ${num(i.target)}` : '', unit),
        i.pct !== null ? h('span', { class: 'muted' }, Math.round(i.pct * 100) + '%') : null,
        h('span', { class: 'muted right' }, dueText),
      ),
      i.hint && !g.done ? h('div', { class: 'small muted' }, i.hint) : null,
      last ? h('div', { class: 'small muted' }, 'Last update ' + app.fmt.ago(last)) : null,
      h('div', { class: 'row' },
        g.done ? null : h('button', { class: 'primary small', onclick: () => openLog(g) }, 'Log progress'),
        h('button', { class: 'ghost small', onclick: () => openLog(g, true) }, 'History'),
        h('button', { class: 'ghost small', onclick: () => openEdit(g) }, 'Edit'),
        h('button', { class: 'ghost small', onclick: (e) => toggleDone(g, e.currentTarget) }, g.done ? 'Reopen' : 'Mark done'),
        h('span', { class: 'right' }, ui.confirmButton({ label: 'Delete', small: true, onConfirm: () => removeGoal(g) })),
      ),
    );
  }

  async function toggleDone(g, btn) {
    btn.disabled = true;
    try {
      await app.update('goals', g.id, { done: !g.done });
      app.toast(g.done ? 'Goal reopened' : 'Goal done 🎉');
      await load();
    } catch (err) {
      btn.disabled = false;
      app.toast('Could not save: ' + (err.message || err));
    }
  }

  async function removeGoal(g) {
    try {
      for (;;) {
        const ups = await app.sql('SELECT id FROM goal_updates WHERE goal = :g LIMIT 500', { g: g.id });
        if (!ups.length) break;
        for (const u of ups) await app.remove('goal_updates', u.id);
        if (ups.length < 500) break;
      }
      await app.remove('goals', g.id);
      app.toast('Goal deleted');
      await load();
    } catch (err) {
      app.toast('Could not delete: ' + (err.message || err));
    }
  }

  // ---- log progress / history dialog ----
  function openLog(g, historyOnly) {
    const unit = g.unit ? ' ' + g.unit : '';
    const amount = h('input', { type: 'number', step: 'any', inputmode: 'decimal', placeholder: 'Amount' + (g.unit ? ` (${g.unit})` : ''), 'aria-label': 'Amount', style: 'flex: 1 1 110px; min-width: 0' });
    const note = h('input', { placeholder: 'Note (optional)', maxlength: 200, 'aria-label': 'Note', style: 'flex: 2 1 140px; min-width: 0' });
    const btn = h('button', { class: 'primary', type: 'submit' }, 'Log');
    const err = h('small', { class: 'err' });
    const hist = h('div', { class: 'list hist' });
    let dlg;

    async function drawHist() {
      const rows = await app.sql(
        'SELECT id, amount, note, created FROM goal_updates WHERE goal = :g ORDER BY created DESC LIMIT 100',
        { g: g.id });
      hist.replaceChildren(rows.length ? rows.map((u) => h('div', { class: 'item row' },
        h('div', { class: 'grow col', style: 'gap: 2px; min-width: 0' },
          h('span', null, h('strong', null, (u.amount > 0 ? '+' : '') + num(u.amount)), unit),
          u.note ? h('span', { class: 'small', style: 'overflow-wrap: anywhere' }, u.note) : null,
          h('span', { class: 'small muted' }, app.fmt.datetime(u.created)),
        ),
        ui.confirmButton({ label: 'Remove', small: true, onConfirm: async () => {
          await app.remove('goal_updates', u.id);
          await drawHist();
        } }),
      )) : h('div', { class: 'empty small' }, 'No updates yet.'));
    }

    async function save(e) {
      e.preventDefault();
      err.textContent = '';
      const v = parseNum(amount.value);
      if (v === null || v === 0) { err.textContent = 'Enter an amount (a negative one corrects a mistake).'; amount.focus(); return; }
      btn.disabled = true;
      try {
        await app.insert('goal_updates', { goal: g.id, amount: v, note: note.value.trim() || null });
        app.toast(`Logged ${v > 0 ? '+' : ''}${num(v)}${unit}`);
        dlg.close();
      } catch (ex) {
        err.textContent = 'Could not save: ' + (ex.message || ex);
        btn.disabled = false;
      }
    }

    const logForm = g.done || historyOnly ? null
      : h('form', { onsubmit: save }, amount, note, btn);
    dlg = ui.dialog({
      title: g.title || 'Goal',
      body: [logForm, logForm ? err : null, h('h3', null, 'Updates'), hist],
      actions: [{ label: 'Close' }],
    });
    document.body.append(dlg);
    drawHist();
    if (logForm) setTimeout(() => amount.focus(), 0);
  }

  // ---- edit dialog ----
  function openEdit(g) {
    const t = h('input', { value: g.title || '', maxlength: 120 });
    const tg = h('input', { type: 'number', step: 'any', min: '0', inputmode: 'decimal', value: g.target != null ? String(g.target) : '' });
    const un = h('input', { value: g.unit || '', maxlength: 30 });
    const dl = h('input', { type: 'date', value: g.deadline || '' });
    const err = h('small', { class: 'err' });
    let dlg;
    let saving = false;

    async function save() {
      if (saving) return;
      err.textContent = '';
      const title = t.value.trim();
      const target = parseNum(tg.value);
      if (!title) { err.textContent = 'Give the goal a name.'; return; }
      if (target === null || target <= 0) { err.textContent = 'Set a target above 0.'; return; }
      saving = true;
      try {
        await app.update('goals', g.id, { title, target, unit: un.value.trim() || null, deadline: dl.value || null });
        dlg.close();
        await load();
      } catch (ex) {
        err.textContent = 'Could not save: ' + (ex.message || ex);
      } finally {
        saving = false;
      }
    }

    const body = h('form', { class: 'col', onsubmit: (e) => { e.preventDefault(); save(); } },
      h('label', null, 'Goal', t),
      h('div', { class: 'row' },
        h('label', { class: 'grow' }, 'Target', tg),
        h('label', { class: 'grow' }, 'Unit', un),
      ),
      h('label', null, 'Deadline', dl),
      err,
      h('button', { type: 'submit', hidden: true }),
    );
    dlg = ui.dialog({
      title: 'Edit goal',
      body,
      actions: [
        { label: 'Cancel' },
        { label: 'Save', primary: true, onClick: () => { save(); return false; } },
      ],
    });
    document.body.append(dlg);
    setTimeout(() => t.focus(), 0);
  }

  app.onChange(['goals', 'goal_updates'], () => load());
  await load();
}
