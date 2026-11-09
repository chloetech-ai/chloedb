// title: Timesheet
// description: Log hours per project per day, see the week as a grid with totals per day and per project, move between weeks and see what is billable this week.
// tables: projects(name text, rate float, color keyword); hours(project keyword, day keyword, hours float, note text, billable bool)
async function render(root, app) {
  const D = lib.dates;
  const PALETTE = ['#3b82f6', '#10b981', '#f59e0b', '#ef4444', '#8b5cf6', '#ec4899', '#14b8a6', '#64748b'];
  let weekStart = D.startOfWeek(app.today(), 1);
  let projects = [];
  let seq = 0;

  const fmtH = (n) => lib.numbers.format(lib.numbers.roundTo(n || 0, 0.01));
  const dayNum = (d) => Number(d.slice(8, 10));
  const shortDay = (d) => `${D.weekdayName(d, true)} ${dayNum(d)} ${D.monthName(d, true)}`;
  const weekDays = () => D.range(weekStart, D.addDays(weekStart, 6));
  const projectById = (id) => projects.find((p) => p.id === id);
  const dot = (color) => h('span', { class: 'ts-dot', style: { background: color || 'var(--muted)' } });

  const style = h('style', null, `
    .ts-wrap { overflow-x: auto; }
    .ts-grid { width: 100%; border-collapse: collapse; font-variant-numeric: tabular-nums; }
    .ts-grid th, .ts-grid td { padding: 6px 8px; text-align: right; white-space: nowrap; }
    .ts-grid th:first-child, .ts-grid td:first-child { text-align: left; white-space: normal; min-width: 7em; }
    .ts-grid td.cell { cursor: pointer; }
    .ts-grid td.cell:hover { background: color-mix(in srgb, var(--accent) 10%, transparent); }
    .ts-grid .today { background: color-mix(in srgb, var(--accent) 8%, transparent); }
    .ts-grid tfoot td, .ts-grid .tot { font-weight: 600; }
    .ts-grid tfoot td { border-top: 1px solid var(--line); }
    .ts-dot { display: inline-block; width: 10px; height: 10px; border-radius: 50%; margin-right: 6px; flex: none; vertical-align: middle; }
    .ts-form input[type=number] { width: 6.5em; }
    .ts-form input[type=date] { width: 10.5em; }
    .ts-form select { min-width: 9em; }
  `);

  // ---- week navigation ----
  const weekLabel = h('strong', { class: 'nowrap' });
  const prevBtn = h('button', { class: 'icon', title: 'Previous week', 'aria-label': 'Previous week', onclick: () => moveWeek(-7) }, '‹');
  const nextBtn = h('button', { class: 'icon', title: 'Next week', 'aria-label': 'Next week', onclick: () => moveWeek(7) }, '›');
  const thisBtn = h('button', { class: 'ghost small', onclick: () => { weekStart = D.startOfWeek(app.today(), 1); app.track('week', 'this'); load(); } }, 'This week');
  function moveWeek(n) {
    weekStart = D.addDays(weekStart, n);
    app.track('week', weekStart);
    load();
  }

  // ---- log hours form (never redrawn) ----
  const projSel = h('select', { required: true, 'aria-label': 'Project' });
  const dayIn = h('input', { type: 'date', value: app.today(), required: true, 'aria-label': 'Day' });
  const hoursIn = h('input', { type: 'number', min: '0.25', max: '24', step: '0.25', placeholder: 'Hours', required: true, 'aria-label': 'Hours' });
  const noteIn = h('input', { placeholder: 'Note (optional)', class: 'grow', 'aria-label': 'Note' });
  const billIn = h('input', { type: 'checkbox', checked: true });
  const logBtn = h('button', { class: 'primary', type: 'submit' }, 'Log');
  const logForm = h('form', { class: 'ts-form', onsubmit: logHours },
    projSel, dayIn, hoursIn, noteIn, h('label', { class: 'row nowrap' }, billIn, 'Billable'), logBtn);

  async function logHours(e) {
    e.preventDefault();
    const project = projSel.value;
    const day = dayIn.value;
    const hrs = Number(hoursIn.value);
    if (!project) return app.toast('Choose a project');
    if (!D.isDay(day)) return app.toast('Choose a day');
    if (!(hrs > 0) || hrs > 24) return app.toast('Hours must be more than 0 and at most 24');
    logBtn.disabled = true;
    try {
      await app.insert('hours', { project, day, hours: hrs, note: noteIn.value.trim() || null, billable: billIn.checked });
      hoursIn.value = '';
      noteIn.value = '';
      app.toast(`Logged ${fmtH(hrs)} h`);
      const ws = D.startOfWeek(day, 1);
      if (ws !== weekStart) { weekStart = ws; load(); }
      hoursIn.focus();
    } catch (err) {
      app.toast('Could not save: ' + (err.message || err));
    } finally {
      logBtn.disabled = false;
    }
  }

  // ---- add project form (never redrawn) ----
  const pName = h('input', { placeholder: 'Project name', required: true, class: 'grow', 'aria-label': 'Project name' });
  const pRate = h('input', { type: 'number', min: '0', step: '0.01', placeholder: 'Rate / h', 'aria-label': 'Hourly rate' });
  const pColor = h('input', { type: 'color', value: PALETTE[0], 'aria-label': 'Color' });
  const pBtn = h('button', { type: 'submit' }, 'Add project');
  const projForm = h('form', { class: 'ts-form', onsubmit: addProject }, pName, pRate, pColor, pBtn);
  const projDetails = h('details', null, h('summary', null, 'Add a project'), h('div', { class: 'section' }, projForm));

  async function addProject(e) {
    e.preventDefault();
    const name = pName.value.trim();
    if (!name) return app.toast('Give the project a name');
    if (projects.some((p) => p.name.trim().toLowerCase() === name.toLowerCase())) return app.toast('There is already a project with that name');
    const rate = pRate.value === '' ? null : Number(pRate.value);
    if (rate != null && !(rate >= 0)) return app.toast('The rate must be 0 or more');
    pBtn.disabled = true;
    try {
      const row = await app.insert('projects', { name, rate, color: pColor.value });
      pName.value = '';
      pRate.value = '';
      pColor.value = PALETTE[(projects.length + 1) % PALETTE.length];
      projSel.value = row.id;
      app.toast(`Added ${name}`);
    } catch (err) {
      app.toast('Could not save: ' + (err.message || err));
    } finally {
      pBtn.disabled = false;
    }
  }

  function fillProjects() {
    const cur = projSel.value;
    projSel.replaceChildren(
      h('option', { value: '' }, projects.length ? 'Choose a project' : 'Add a project first'),
      projects.map((p) => h('option', { value: p.id }, p.name))
    );
    projSel.value = projects.some((p) => p.id === cur) ? cur : (projects.length === 1 ? projects[0].id : '');
    projDetails.open = projects.length === 0;
    if (projects.length === 0) pColor.value = PALETTE[0];
  }

  function prefill(projectId, day) {
    if (projectById(projectId)) projSel.value = projectId;
    dayIn.value = day;
    hoursIn.focus();
  }

  // ---- the part that redraws ----
  const out = h('div', { class: 'col' });

  root.append(
    style,
    h('h1', null, 'Timesheet'),
    h('div', { class: 'toolbar row' }, prevBtn, weekLabel, nextBtn, thisBtn),
    h('div', { class: 'card col' }, logForm, projDetails),
    out
  );

  async function loadProjects() {
    projects = await app.sql('SELECT id, name, rate, color FROM projects ORDER BY name ASC LIMIT 500');
    fillProjects();
  }

  async function load() {
    const my = ++seq;
    const days = weekDays();
    const end = days[6];
    weekLabel.textContent = D.month(weekStart) === D.month(end)
      ? `${dayNum(weekStart)} – ${dayNum(end)} ${D.monthName(end, true)} ${end.slice(0, 4)}`
      : `${dayNum(weekStart)} ${D.monthName(weekStart, true)} – ${dayNum(end)} ${D.monthName(end, true)} ${end.slice(0, 4)}`;
    const params = {};
    days.forEach((d, i) => { params['d' + i] = d; });
    const rows = await app.sql(
      `SELECT id, project, day, hours, note, billable FROM hours WHERE day IN (${days.map((_, i) => ':d' + i).join(', ')}) ORDER BY day ASC, created ASC LIMIT 500`,
      params
    );
    if (my !== seq) return;
    draw(days, rows.map((r) => ({ ...r, day: String(r.day).slice(0, 10), hours: Number(r.hours) || 0 })));
  }

  function draw(days, rows) {
    const today = app.today();
    if (!projects.length && !rows.length) {
      out.replaceChildren(ui.emptyState({
        icon: '⏱',
        title: 'No projects yet',
        text: 'Add a project with its hourly rate, then log the hours you spend on it each day.',
        action: { label: 'Add a project', onClick: () => { projDetails.open = true; pName.focus(); } },
      }));
      return;
    }

    // totals
    const byProj = new Map();
    const dayTotals = Object.fromEntries(days.map((d) => [d, 0]));
    let total = 0, billH = 0, billAmt = 0;
    for (const r of rows) {
      if (!byProj.has(r.project)) byProj.set(r.project, { byDay: {}, total: 0 });
      const p = byProj.get(r.project);
      p.byDay[r.day] = (p.byDay[r.day] || 0) + r.hours;
      p.total += r.hours;
      if (r.day in dayTotals) dayTotals[r.day] += r.hours;
      total += r.hours;
      if (r.billable === true) {
        billH += r.hours;
        billAmt += r.hours * (Number(projectById(r.project)?.rate) || 0);
      }
    }
    billAmt = lib.money.round(billAmt);

    const tiles = ui.statTiles({ items: [
      { label: 'Hours this week', value: fmtH(total) },
      { label: 'Billable hours', value: fmtH(billH), hint: total > billH ? `${fmtH(total - billH)} h not billable` : null },
      { label: 'Billable this week', value: billAmt, format: 'money' },
    ] });

    if (!rows.length) {
      out.replaceChildren(tiles, ui.emptyState({
        compact: true,
        icon: '🗓',
        title: 'Nothing logged this week',
        text: 'Pick a project, a day and the hours above, then press Log.',
      }));
      return;
    }

    const projIds = [...byProj.keys()].sort((a, b) =>
      lib.list.naturalCompare(projectById(a)?.name ?? '\uffff', projectById(b)?.name ?? '\uffff'));
    const nameOf = (id) => projectById(id)?.name || 'Removed project';

    const grid = h('div', { class: 'card ts-wrap' }, h('table', { class: 'ts-grid' },
      h('thead', null, h('tr', null,
        h('th', null, 'Project'),
        days.map((d) => h('th', { class: d === today ? 'today' : null, title: d }, h('div', null, D.weekdayName(d, true)), h('small', { class: 'muted' }, dayNum(d)))),
        h('th', null, 'Total')
      )),
      h('tbody', null, projIds.map((id) => {
        const p = byProj.get(id);
        return h('tr', null,
          h('td', null, dot(projectById(id)?.color), nameOf(id)),
          days.map((d) => h('td', {
            class: 'cell' + (d === today ? ' today' : ''),
            title: `Log hours for ${nameOf(id)} on ${shortDay(d)}`,
            onclick: () => prefill(id, d),
          }, p.byDay[d] ? fmtH(p.byDay[d]) : h('span', { class: 'muted' }, '·'))),
          h('td', { class: 'tot' }, fmtH(p.total))
        );
      })),
      h('tfoot', null, h('tr', null,
        h('td', null, 'Total'),
        days.map((d) => h('td', { class: d === today ? 'today' : null }, dayTotals[d] ? fmtH(dayTotals[d]) : h('span', { class: 'muted' }, '0'))),
        h('td', null, fmtH(total))
      ))
    ));

    const list = h('div', { class: 'list card' }, rows.map((r) => {
      const p = projectById(r.project);
      return h('div', { class: 'item row' },
        dot(p?.color),
        h('div', { class: 'col grow', style: 'gap: 2px; min-width: 0' },
          h('div', { class: 'row', style: 'gap: 6px' }, h('strong', { class: 'clip' }, nameOf(r.project)), h('small', { class: 'muted nowrap' }, shortDay(r.day))),
          r.note ? h('small', { class: 'muted clip' }, r.note) : null
        ),
        h('strong', { class: 'nowrap' }, `${fmtH(r.hours)} h`),
        ui.toggleSwitch({
          checked: r.billable === true,
          label: 'Billable',
          onChange: (v) => app.update('hours', r.id, { billable: v }),
        }),
        ui.confirmButton({ label: 'Delete', small: true, onConfirm: () => app.remove('hours', r.id) })
      );
    }));

    out.replaceChildren(tiles, grid, h('h3', null, 'Entries this week'), list);
  }

  app.onChange(['projects', 'hours'], async (tables) => {
    if (tables.includes('projects')) await loadProjects();
    load();
  });

  await loadProjects();
  await load();
}
