// title: Journal
// description: Write today's entry with a mood and text that saves as you type, keep a streak of days written, and read past entries month by month with their mood.
// tables: entries(day keyword, mood int, body text)
async function render(root, app) {
  const MOODS = [
    { v: 1, face: '😞', label: 'Awful' },
    { v: 2, face: '🙁', label: 'Bad' },
    { v: 3, face: '😐', label: 'Okay' },
    { v: 4, face: '🙂', label: 'Good' },
    { v: 5, face: '😄', label: 'Great' },
  ];
  const faceOf = (v) => (MOODS.find((m) => m.v === v) || {}).face || '';
  const labelOf = (v) => (MOODS.find((m) => m.v === v) || {}).label || '';
  const today = app.today();
  const thisMonth = today.slice(0, 7);
  let month = thisMonth;
  let earliestMonth = thisMonth;

  const dayLabel = (d) =>
    lib.dates.weekdayName(d) + ', ' + Number(d.slice(8)) + ' ' + lib.dates.monthName(d);
  const monthLabel = (m) => lib.dates.monthName(m) + ' ' + m.slice(0, 4);
  const hasContent = (r) => r.mood != null || (r.body || '').trim() !== '';

  const style = h('style', null, `
    .journal .moods { display: flex; gap: 6px; flex-wrap: wrap; }
    .journal .mood-btn { flex: 1 1 56px; display: flex; flex-direction: column; align-items: center; gap: 2px; padding: 8px 4px; min-width: 52px; }
    .journal .mood-btn .face { font-size: 26px; line-height: 1; filter: grayscale(0.6); opacity: 0.75; }
    .journal .mood-btn.on { border-color: var(--accent); box-shadow: 0 0 0 2px var(--accent) inset; }
    .journal .mood-btn.on .face { filter: none; opacity: 1; }
    .journal textarea { width: 100%; min-height: 160px; resize: vertical; box-sizing: border-box; }
    .journal .entry summary { cursor: pointer; display: flex; gap: 10px; align-items: center; list-style: none; }
    .journal .entry summary::-webkit-details-marker { display: none; }
    .journal .entry .face { font-size: 22px; width: 28px; text-align: center; flex: none; }
    .journal .entry .body { white-space: pre-wrap; margin: 8px 0 0 38px; overflow-wrap: anywhere; }
    .journal .entry .meta { min-width: 0; }
  `);

  // ---- Today's entry (never redrawn by onChange) ----
  const found = await app.sql(
    'SELECT id, day, mood, body FROM entries WHERE day = :d ORDER BY created ASC LIMIT 1',
    { d: today }
  );
  let entry = found[0] || null;
  let mood = entry && entry.mood != null ? entry.mood : null;
  let body = (entry && entry.body) || '';
  let timer = null;
  let dirty = false;
  let queue = Promise.resolve();

  const status = h('small', { class: 'muted', 'aria-live': 'polite' }, '');

  const faces = MOODS.map((m) =>
    h('button', {
      type: 'button', class: 'mood-btn', title: m.label, 'aria-label': m.label,
      onclick: () => pickMood(m.v),
    }, h('span', { class: 'face' }, m.face), h('span', { class: 'small muted' }, m.label))
  );
  function paintMood() {
    faces.forEach((b, i) => {
      const on = MOODS[i].v === mood;
      b.classList.toggle('on', on);
      b.setAttribute('aria-pressed', on ? 'true' : 'false');
    });
  }
  function pickMood(v) {
    mood = mood === v ? null : v;
    paintMood();
    app.track('mood', labelOf(v));
    flush();
  }

  const area = h('textarea', {
    rows: 8,
    placeholder: "How was your day? What's on your mind?",
    'aria-label': "Today's entry",
    value: body,
    oninput: () => {
      body = area.value;
      dirty = true;
      status.textContent = 'Editing…';
      clearTimeout(timer);
      timer = setTimeout(flush, 700);
    },
    onblur: () => { if (dirty || timer) flush(); },
  });

  function flush() {
    clearTimeout(timer);
    timer = null;
    dirty = false;
    const fields = { mood, body };
    status.textContent = 'Saving…';
    queue = queue
      .then(async () => {
        if (!entry) {
          if (fields.mood == null && !fields.body.trim()) return false;
          entry = await app.insert('entries', { day: today, ...fields });
        } else {
          entry = await app.update('entries', entry.id, fields);
        }
        return true;
      })
      .then(
        (saved) => {
          if (!timer && !dirty) status.textContent = saved ? 'Saved ' + app.fmt.time(app.now()) : '';
        },
        () => {
          dirty = true;
          status.textContent = "Couldn't save. Keep typing to try again.";
        }
      );
  }

  paintMood();
  if (entry) status.textContent = 'Saved ' + app.fmt.ago(entry.created);

  const todayCard = h('section', { class: 'card col' },
    h('div', { class: 'row' },
      h('h2', { style: 'margin:0' }, 'Today'),
      h('span', { class: 'muted' }, dayLabel(today)),
    ),
    h('div', { class: 'moods', role: 'group', 'aria-label': 'Mood' }, faces),
    area,
    h('div', { class: 'row' }, status),
  );

  // ---- Parts that redraw ----
  const statsBox = h('div');
  const pastBox = h('section', { class: 'section col' });

  async function loadStats() {
    const [rows, first] = await Promise.all([
      app.sql('SELECT day, mood, length(body) AS len FROM entries ORDER BY day DESC LIMIT 500'),
      app.sql('SELECT day FROM entries ORDER BY day ASC LIMIT 1'),
    ]);
    earliestMonth = first[0] && first[0].day < today ? first[0].day.slice(0, 7) : thisMonth;
    const days = [...new Set(rows.filter((r) => r.mood != null || r.len > 0).map((r) => r.day))];
    const current = lib.streak.current(days, { today });
    const best = lib.streak.best(days);
    const wroteToday = days.includes(today);
    statsBox.replaceChildren(
      h('div', { class: 'kpis' },
        h('div', { class: 'kpi' },
          h('div', { class: 'value' }, (current > 0 ? '🔥 ' : '') + current),
          h('div', { class: 'label' }, current === 1 ? 'day streak' : 'days streak')),
        h('div', { class: 'kpi' },
          h('div', { class: 'value' }, best),
          h('div', { class: 'label' }, 'best streak')),
        h('div', { class: 'kpi' },
          h('div', { class: 'value' }, days.length >= 500 ? '500+' : days.length),
          h('div', { class: 'label' }, days.length === 1 ? 'day written' : 'days written')),
      ),
      !wroteToday
        ? h('p', { class: 'small muted' },
            current > 0 ? 'Write today to keep your streak going.' : 'Write a few lines today to start a streak.')
        : null,
    );
  }

  async function loadPast() {
    const rows = await app.sql(
      'SELECT id, day, mood, body FROM entries WHERE day LIKE :m ORDER BY day DESC, created ASC LIMIT 500',
      { m: month + '-%' }
    );
    const list = rows.filter((r) => r.day !== today && hasContent(r));
    const prev = lib.dates.month(lib.dates.addMonths(month + '-01', -1));
    const next = lib.dates.month(lib.dates.addMonths(month + '-01', 1));
    const go = (m) => { month = m; app.track('month', m); loadPast(); };

    pastBox.replaceChildren(
      h('div', { class: 'toolbar row' },
        h('button', { class: 'icon ghost', 'aria-label': 'Previous month', disabled: month <= earliestMonth, onclick: () => go(prev) }, '‹'),
        h('h3', { style: 'margin:0' }, monthLabel(month)),
        h('button', { class: 'icon ghost', 'aria-label': 'Next month', disabled: month >= thisMonth, onclick: () => go(next) }, '›'),
        h('span', { class: 'right small muted' }, lib.text.plural(list.length, 'entry', 'entries')),
      ),
      list.length === 0
        ? ui.emptyState({
            compact: true,
            icon: '📖',
            title: month === thisMonth ? 'No earlier entries this month' : 'No entries in ' + monthLabel(month),
            text: month === thisMonth
              ? 'The days you write about show up here.'
              : 'Use the arrows to look at another month.',
          })
        : h('div', { class: 'list' }, list.map((r) => {
            const text = (r.body || '').trim();
            return h('details', { class: 'item entry' },
              h('summary', null,
                h('span', { class: 'face', title: labelOf(r.mood) }, faceOf(r.mood) || '·'),
                h('div', { class: 'meta grow' },
                  h('div', { class: 'title' }, dayLabel(r.day)),
                  h('div', { class: 'small muted clip' }, text ? lib.text.truncate(text.replace(/\s+/g, ' '), 120) : labelOf(r.mood)),
                ),
              ),
              text ? h('div', { class: 'body' }, text) : null,
            );
          })),
    );
  }

  let token = 0;
  async function refresh() {
    const t = ++token;
    await loadStats();
    if (t === token) await loadPast();
  }

  root.replaceChildren(h('div', { class: 'journal col' },
    style,
    h('h1', null, 'Journal'),
    statsBox,
    todayCard,
    h('h2', null, 'Past entries'),
    pastBox,
  ));

  await refresh();

  app.onChange(['entries'], async () => {
    await refresh();
    // Pick up today's entry from elsewhere only when nobody is typing here.
    if (dirty || timer || document.activeElement === area) return;
    const rows = await app.sql(
      'SELECT id, day, mood, body FROM entries WHERE day = :d ORDER BY created ASC LIMIT 1',
      { d: today }
    );
    if (dirty || timer || document.activeElement === area) return;
    entry = rows[0] || null;
    mood = entry && entry.mood != null ? entry.mood : null;
    body = (entry && entry.body) || '';
    if (area.value !== body) area.value = body;
    paintMood();
  });
}
