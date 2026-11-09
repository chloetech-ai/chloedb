// title: Workouts
// description: Log exercises with sets, reps and weight, see the day's session, each exercise's history with a chart of its best set, and your personal records.
// tables: workouts(day keyword, exercise keyword, sets int, reps int, weight float, notes text)
async function render(root, app) {
  const today = app.today();
  let day = today;          // the session shown under the form
  let histEx = null;        // the exercise shown in History
  let tab = 'today';
  let exercises = [];       // [{exercise, best, maxreps, n, last}]

  const num = (n) => app.fmt.number(Math.round(n * 100) / 100);
  const setText = (r) => `${r.sets || 1} × ${r.reps || 0}` + (r.weight != null ? ` @ ${num(r.weight)}` : '');
  const dayLabel = (d) => {
    const rel = lib.dates.relative(d, today);
    return (rel === 'today' || rel === 'yesterday' || rel === 'tomorrow')
      ? rel[0].toUpperCase() + rel.slice(1) + ', ' + app.fmt.date(d)
      : lib.dates.weekdayName(d, true) + ', ' + app.fmt.date(d);
  };
  const canonical = (name) => {
    const hit = exercises.find((e) => e.exercise.toLowerCase() === name.toLowerCase());
    return hit ? hit.exercise : name;
  };

  root.append(h('style', null, `
    .wk-form input[type=number] { width: 5.5em; }
    .wk-form .wk-ex { flex: 1 1 12em; min-width: 0; }
    .wk-form .wk-notes { flex: 1 1 14em; min-width: 0; }
    .wk-set { font-variant-numeric: tabular-nums; }
    .wk-day { margin: 1rem 0 .25rem; }
  `));
  root.append(h('h1', null, 'Workouts'));

  // ---- form (never redrawn) ----
  const datalist = h('datalist', { id: 'wk-exercises' });
  const dayInput = h('input', {
    type: 'date', value: day, required: true, 'aria-label': 'Day',
    onchange: () => { day = lib.dates.isDay(dayInput.value) ? dayInput.value : today; drawToday(); },
  });
  const exInput = h('input', { class: 'wk-ex', list: 'wk-exercises', placeholder: 'Exercise', 'aria-label': 'Exercise', required: true, autocomplete: 'off' });
  const setsInput = h('input', { type: 'number', min: 1, step: 1, placeholder: 'Sets', 'aria-label': 'Sets', required: true });
  const repsInput = h('input', { type: 'number', min: 1, step: 1, placeholder: 'Reps', 'aria-label': 'Reps', required: true });
  const weightInput = h('input', { type: 'number', min: 0, step: 'any', placeholder: 'Weight', 'aria-label': 'Weight (leave empty for bodyweight)' });
  const notesInput = h('input', { class: 'wk-notes', placeholder: 'Notes (optional)', 'aria-label': 'Notes' });
  const saveBtn = h('button', { class: 'primary', type: 'submit' }, 'Log');

  const form = h('form', { class: 'wk-form row', onsubmit: async (e) => {
    e.preventDefault();
    const name = canonical(exInput.value.trim().replace(/\s+/g, ' '));
    const sets = parseInt(setsInput.value, 10);
    const reps = parseInt(repsInput.value, 10);
    const weight = weightInput.value === '' ? null : parseFloat(weightInput.value);
    const d = dayInput.value;
    if (!name) return app.toast('Name the exercise');
    if (!lib.dates.isDay(d)) return app.toast('Pick a day');
    if (!(sets >= 1) || !(reps >= 1)) return app.toast('Sets and reps must be at least 1');
    if (weight != null && !(weight >= 0)) return app.toast('Weight must be 0 or more');
    saveBtn.disabled = true;
    try {
      const [prior] = await app.sql('SELECT MAX(weight) AS best, COUNT(*) AS n FROM workouts WHERE exercise = :e', { e: name });
      await app.insert('workouts', { day: d, exercise: name, sets, reps, weight, notes: notesInput.value.trim() || null });
      const isPr = weight != null && weight > 0 && prior && prior.n > 0 && weight > (prior.best || 0);
      app.toast(isPr ? `New record for ${name}: ${num(weight)}` : `Logged ${name}`);
      for (const i of [exInput, setsInput, repsInput, weightInput, notesInput]) i.value = '';
      exInput.focus();
    } catch (err) {
      app.toast('Could not save: ' + (err && err.message || err));
    } finally {
      saveBtn.disabled = false;
    }
  } }, dayInput, exInput, setsInput, repsInput, weightInput, notesInput, saveBtn, datalist);

  // ---- panes ----
  const todayBox = h('div', { class: 'section' });
  const histBox = h('div', { class: 'section' });
  const recBox = h('div', { class: 'section' });

  root.append(ui.tabs({
    label: 'Workouts',
    value: tab,
    onChange: (id) => { tab = id; app.track('tab', id); },
    tabs: [
      { id: 'today', label: 'Session', content: h('div', { class: 'col' }, h('div', { class: 'card' }, form), todayBox) },
      { id: 'history', label: 'History', content: histBox },
      { id: 'records', label: 'Records', content: recBox },
    ],
  }));

  const delBtn = (r) => h('button', {
    class: 'ghost small icon', title: 'Delete', 'aria-label': 'Delete',
    onclick: async (e) => {
      e.stopPropagation();
      if (!confirm(`Delete ${r.exercise || 'this entry'} ${setText(r)} on ${app.fmt.date(r.day)}?`)) return;
      e.currentTarget.disabled = true;
      try { await app.remove('workouts', r.id); } catch (err) { app.toast('Could not delete'); }
    },
  }, '✕');

  const bestOf = (ex) => {
    const e = exercises.find((x) => x.exercise === ex);
    return e ? e.best : null;
  };

  async function loadExercises() {
    exercises = await app.sql(
      'SELECT exercise, MAX(weight) AS best, MAX(reps) AS maxreps, COUNT(*) AS n, MAX(day) AS last FROM workouts WHERE exercise IS NOT NULL GROUP BY exercise ORDER BY n DESC LIMIT 500');
    datalist.replaceChildren(exercises.map((e) => h('option', { value: e.exercise })));
  }

  async function drawToday() {
    const rows = await app.sql('SELECT * FROM workouts WHERE day = :d ORDER BY created ASC LIMIT 500', { d: day });
    const title = h('h2', null, day === today ? 'Today' : dayLabel(day));
    if (!rows.length) {
      todayBox.replaceChildren(title, ui.emptyState({
        icon: '🏋️', compact: true,
        title: day === today ? 'Nothing logged today' : 'Nothing logged on this day',
        text: 'Type an exercise, its sets, reps and weight above and press Enter.',
      }));
      return;
    }
    const totalSets = rows.reduce((s, r) => s + (r.sets || 1), 0);
    const volume = rows.reduce((s, r) => s + (r.sets || 1) * (r.reps || 0) * (r.weight || 0), 0);
    const exCount = new Set(rows.map((r) => r.exercise)).size;
    todayBox.replaceChildren(
      title,
      h('div', { class: 'kpis' },
        h('div', { class: 'kpi' }, h('div', { class: 'value' }, exCount), h('div', { class: 'label' }, exCount === 1 ? 'exercise' : 'exercises')),
        h('div', { class: 'kpi' }, h('div', { class: 'value' }, totalSets), h('div', { class: 'label' }, totalSets === 1 ? 'set' : 'sets')),
        volume > 0 ? h('div', { class: 'kpi' }, h('div', { class: 'value' }, num(volume)), h('div', { class: 'label' }, 'volume')) : null),
      h('div', { class: 'list' }, rows.map((r) => {
        const best = bestOf(r.exercise);
        const pr = r.weight != null && r.weight > 0 && best != null && r.weight >= best;
        return h('div', { class: 'item row' },
          h('div', { class: 'grow col', style: 'gap:2px;min-width:0' },
            h('div', { class: 'row', style: 'gap:6px' }, h('strong', { class: 'clip' }, r.exercise || 'Exercise'), pr ? h('span', { class: 'badge ok' }, 'PR') : null),
            r.notes ? h('small', { class: 'muted clip' }, r.notes) : null),
          h('span', { class: 'wk-set nowrap' }, setText(r)),
          delBtn(r));
      })));
  }

  async function drawHistory() {
    if (!exercises.length) {
      histBox.replaceChildren(ui.emptyState({ icon: '📈', title: 'No history yet', text: 'Log an exercise and its history and progress chart appear here.' }));
      return;
    }
    if (!exercises.some((e) => e.exercise === histEx)) histEx = exercises[0].exercise;
    const names = exercises.map((e) => e.exercise).sort(lib.list.naturalCompare);
    const select = h('select', {
      'aria-label': 'Exercise', value: histEx,
      onchange: () => { histEx = select.value; app.track('exercise', histEx); drawHistory(); },
    }, names.map((n) => h('option', { value: n, selected: n === histEx }, n)));

    const ex = histEx;
    const rows = await app.sql(
      'SELECT id, day, exercise, sets, reps, weight, notes FROM workouts WHERE exercise = :e ORDER BY day DESC, created DESC LIMIT 500', { e: ex });
    if (ex !== histEx) return; // a newer choice is drawing

    const weighted = rows.some((r) => r.weight != null && r.weight > 0);
    const perDay = {};
    for (const r of rows) {
      if (!r.day) continue;
      const v = weighted ? (r.weight || 0) : (r.reps || 0);
      if (perDay[r.day] == null || v > perDay[r.day]) perDay[r.day] = v;
    }
    const points = Object.keys(perDay).sort().map((d) => ({ x: d, y: perDay[d] }));
    const chart = points.length >= 2
      ? h('div', { class: 'card' },
          h('h3', null, weighted ? 'Heaviest set per day' : 'Most reps per day'),
          ui.lineChart({ data: points, height: 200, format: (v) => weighted ? num(v) : `${num(v)} reps` }))
      : h('p', { class: 'muted small' }, 'Log this exercise on another day to see a chart of your best set.');

    const groups = [];
    for (const r of rows) {
      const last = groups[groups.length - 1];
      if (last && last.day === r.day) last.rows.push(r); else groups.push({ day: r.day, rows: [r] });
    }
    histBox.replaceChildren(
      h('div', { class: 'toolbar row' }, select, h('span', { class: 'muted small right' }, lib.text.plural(rows.length, 'entry', 'entries'))),
      chart,
      groups.map((g) => [
        h('h3', { class: 'wk-day small muted' }, g.day ? dayLabel(g.day) : 'No date'),
        h('div', { class: 'list' }, g.rows.map((r) => h('div', { class: 'item row' },
          h('span', { class: 'wk-set nowrap' }, setText(r)),
          h('small', { class: 'muted grow clip' }, r.notes || ''),
          delBtn(r)))),
      ]),
      rows.length === 500 ? h('p', { class: 'muted small' }, 'Showing the latest 500 entries.') : null);
  }

  async function drawRecords() {
    if (!exercises.length) {
      recBox.replaceChildren(ui.emptyState({ icon: '🏆', title: 'No records yet', text: 'Your heaviest set for each exercise shows up here once you log one.' }));
      return;
    }
    const tops = await app.sql(
      `SELECT w.exercise AS exercise, w.weight AS weight, w.reps AS reps, w.day AS day
       FROM workouts w JOIN (SELECT exercise, MAX(weight) AS best FROM workouts GROUP BY exercise) b
       ON w.exercise = b.exercise AND w.weight = b.best
       ORDER BY w.reps DESC, w.day ASC LIMIT 500`);
    const topBy = {};
    for (const t of tops) if (!topBy[t.exercise]) topBy[t.exercise] = t;

    const list = exercises.slice().sort((a, b) => lib.list.naturalCompare(a.exercise, b.exercise));
    recBox.replaceChildren(h('div', { class: 'list' }, list.map((e) => {
      const t = topBy[e.exercise];
      const lifted = t && t.weight > 0;
      const orm = lifted ? lib.fitness.oneRepMax(t.weight, t.reps || 1) : null;
      const meta = [
        orm && t.reps > 1 ? `est. 1RM ${num(orm)}` : null,
        lifted && t.day ? app.fmt.date(t.day) : null,
        lib.text.plural(e.n, 'entry', 'entries'),
        e.last ? 'last ' + lib.dates.relative(e.last, today) : null,
      ].filter(Boolean).join(' · ');
      return h('div', { class: 'item row' },
        h('div', { class: 'grow col', style: 'gap:2px;min-width:0' },
          h('strong', { class: 'clip' }, e.exercise),
          h('small', { class: 'muted' }, meta)),
        h('span', { class: 'big wk-set nowrap' },
          lifted ? `${num(t.weight)} × ${t.reps || 1}` : `${e.maxreps || 0} reps`));
    })));
  }

  let running = null, again = false;
  async function refresh() {
    if (running) { again = true; return running; }
    running = (async () => {
      do {
        again = false;
        try {
          await loadExercises();
          await Promise.all([drawToday(), drawHistory(), drawRecords()]);
        } catch (err) {
          app.toast('Could not load workouts');
        }
      } while (again);
    })();
    await running;
    running = null;
  }

  app.onChange(['workouts'], refresh);
  await refresh();
}
