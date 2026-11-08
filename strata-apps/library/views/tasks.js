// title: Tasks
// description: A to-do list sorted by urgency: add tasks with a due date and priority, tick them off, and filter by today, upcoming, overdue or done.
// tables: tasks(title text, notes text, due keyword, priority keyword, done bool, done_at timestamp)
async function render(root, app) {
  const PRIORITIES = [
    { value: 'high', label: 'High' },
    { value: 'normal', label: 'Normal' },
    { value: 'low', label: 'Low' },
  ];
  const PRIO_RANK = { high: 0, normal: 1, low: 2 };
  const STATUS_RANK = { overdue: 0, today: 1, soon: 2, later: 3 };
  const TONE = { overdue: 'bad', today: 'warn', soon: 'info', later: '', done: 'ok' };
  const FILTERS = [
    { value: 'all', label: 'All' },
    { value: 'today', label: 'Today' },
    { value: 'upcoming', label: 'Upcoming' },
    { value: 'overdue', label: 'Overdue' },
    { value: 'done', label: 'Done' },
  ];

  let filter = 'all';
  let rows = [];
  const busy = new Set();

  root.append(h('style', null, `
    .tasks-view .task { display: flex; gap: 10px; align-items: flex-start; }
    .tasks-view .task input[type=checkbox] { margin-top: 4px; width: 18px; height: 18px; flex: none; }
    .tasks-view .task .body { flex: 1; min-width: 0; }
    .tasks-view .task .title { overflow-wrap: anywhere; }
    .tasks-view .task .meta { display: flex; gap: 6px; flex-wrap: wrap; margin-top: 4px; }
    .tasks-view .task .notes { white-space: pre-wrap; overflow-wrap: anywhere; margin: 4px 0 0; }
    .tasks-view form.add input[name=title] { flex: 1 1 200px; min-width: 0; }
    .tasks-view form.add input[name=notes] { flex: 1 1 100%; min-width: 0; }
  `));

  const statusOf = (t, today) =>
    lib.deadlines.status(t.due || null, { today, done: !!t.done });

  // ---- add form (outside the part that redraws) ----
  const titleIn = h('input', { name: 'title', placeholder: 'Add a task and press Enter', autocomplete: 'off', 'aria-label': 'Task' });
  const dueIn = h('input', { type: 'date', name: 'due', 'aria-label': 'Due date' });
  const prioIn = h('select', { name: 'priority', 'aria-label': 'Priority' },
    PRIORITIES.map(p => h('option', { value: p.value }, p.label)));
  prioIn.value = 'normal';
  const notesIn = h('input', { name: 'notes', placeholder: 'Notes (optional)', autocomplete: 'off', 'aria-label': 'Notes' });
  const addBtn = h('button', { type: 'submit', class: 'primary' }, 'Add');

  const form = h('form', {
    class: 'add',
    onsubmit: async (e) => {
      e.preventDefault();
      const title = titleIn.value.trim();
      if (!title) { titleIn.focus(); return; }
      addBtn.disabled = true;
      try {
        await app.insert('tasks', {
          title,
          notes: notesIn.value.trim() || null,
          due: lib.dates.isDay(dueIn.value) ? dueIn.value : null,
          priority: prioIn.value || 'normal',
          done: false,
          done_at: null,
        });
        titleIn.value = ''; notesIn.value = ''; dueIn.value = ''; prioIn.value = 'normal';
        titleIn.focus();
        await load();
      } catch (err) {
        app.toast('Could not add the task: ' + (err && err.message || err));
      } finally {
        addBtn.disabled = false;
      }
    },
  }, titleIn, dueIn, prioIn, addBtn, notesIn);

  const summary = h('p', { class: 'muted small' });
  const chipsBox = h('div');
  const listBox = h('div');

  root.append(h('div', { class: 'tasks-view col' },
    h('h1', null, 'Tasks'),
    summary,
    form,
    chipsBox,
    listBox,
  ));

  async function load() {
    const [open, done] = await Promise.all([
      app.sql('SELECT * FROM tasks WHERE (done = false OR done IS NULL) ORDER BY created ASC LIMIT 500'),
      app.sql('SELECT * FROM tasks WHERE done ORDER BY done_at DESC, created DESC LIMIT 200'),
    ]);
    rows = open.concat(done);
    draw();
  }

  function byUrgency(a, b, today) {
    const sa = STATUS_RANK[statusOf(a, today)] ?? 4;
    const sb = STATUS_RANK[statusOf(b, today)] ?? 4;
    if (sa !== sb) return sa - sb;
    if ((a.due || '') !== (b.due || '')) return (a.due || '') < (b.due || '') ? -1 : 1;
    const pa = PRIO_RANK[a.priority] ?? 1, pb = PRIO_RANK[b.priority] ?? 1;
    if (pa !== pb) return pa - pb;
    return (a.created || '') < (b.created || '') ? -1 : 1;
  }

  function draw() {
    const today = app.today();
    const open = rows.filter(t => !t.done).sort((a, b) => byUrgency(a, b, today));
    const done = rows.filter(t => t.done);
    const st = (t) => statusOf(t, today);
    const groups = {
      all: open.concat(done),
      today: open.filter(t => st(t) === 'today'),
      upcoming: open.filter(t => st(t) === 'soon' || st(t) === 'later'),
      overdue: open.filter(t => st(t) === 'overdue'),
      done,
    };

    const overdueN = groups.overdue.length;
    summary.replaceChildren(
      rows.length === 0 ? 'Nothing on your list yet.'
        : [lib.text.plural(open.length, 'open task'),
           overdueN ? ' · ' + overdueN + ' overdue' : null,
           groups.today.length ? ' · ' + groups.today.length + ' due today' : null,
           done.length ? ' · ' + done.length + ' done' : null]
    );

    chipsBox.replaceChildren(ui.filterChips({
      options: FILTERS.map(f => ({ value: f.value, label: f.label, count: groups[f.value].length })),
      value: filter,
      all: false,
      onChange: (v) => {
        filter = v || 'all';
        app.track('filter', 'show=' + filter);
        draw();
      },
    }));

    const items = groups[filter] || [];
    if (!items.length) {
      const msg = {
        all: ['✅', 'No tasks yet', 'Type a task above and press Enter to add it.'],
        today: ['☀️', 'Nothing due today', 'Tasks due today show up here.'],
        upcoming: ['📅', 'Nothing upcoming', 'Give a task a due date after today to see it here.'],
        overdue: ['🎉', 'Nothing overdue', 'You are on top of things.'],
        done: ['☑️', 'Nothing done yet', 'Tick a task to mark it done.'],
      }[filter];
      listBox.replaceChildren(ui.emptyState({ icon: msg[0], title: msg[1], text: msg[2], compact: true }));
      return;
    }
    listBox.replaceChildren(h('div', { class: 'list' }, items.map(t => taskRow(t, today))));
  }

  function taskRow(t, today) {
    const status = statusOf(t, today);
    const box = h('input', {
      type: 'checkbox',
      checked: !!t.done,
      disabled: busy.has(t.id),
      'aria-label': t.done ? 'Mark as not done' : 'Mark as done',
      onchange: async (e) => {
        const done = e.target.checked;
        busy.add(t.id);
        e.target.disabled = true;
        try {
          const saved = await app.update('tasks', t.id, { done, done_at: done ? app.now() : null });
          rows = rows.map(r => r.id === t.id ? saved : r);
        } catch (err) {
          app.toast('Could not save: ' + (err && err.message || err));
        } finally {
          busy.delete(t.id);
          draw();
        }
      },
    });

    const meta = [];
    if (t.done) {
      if (t.done_at) meta.push(h('span', { class: 'small muted' }, 'Done ' + app.fmt.ago(t.done_at)));
    } else if (t.due) {
      const tone = TONE[status];
      meta.push(h('span', {
        class: 'badge' + (tone ? ' ' + tone : ''),
        title: app.fmt.date(t.due + 'T12:00:00'),
      }, lib.deadlines.label(t.due, { today }) + (status === 'later' ? ' · ' + app.fmt.date(t.due + 'T12:00:00') : '')));
    }
    if (!t.done && t.priority === 'high') meta.push(h('span', { class: 'badge bad' }, 'High'));
    if (!t.done && t.priority === 'low') meta.push(h('span', { class: 'badge' }, 'Low'));

    return h('div', { class: 'item task' },
      box,
      h('div', { class: 'body' },
        h('div', { class: 'title' + (t.done ? ' strike muted' : '') }, t.title || 'Untitled'),
        t.notes ? h('p', { class: 'notes small muted' }, t.notes) : null,
        meta.length ? h('div', { class: 'meta' }, meta) : null,
      ),
      h('button', {
        class: 'ghost icon small',
        title: 'Delete',
        'aria-label': 'Delete task',
        onclick: async (e) => {
          if (!confirm('Delete "' + (t.title || 'this task') + '"?')) return;
          e.currentTarget.disabled = true;
          try {
            await app.remove('tasks', t.id);
            rows = rows.filter(r => r.id !== t.id);
            draw();
          } catch (err) {
            app.toast('Could not delete: ' + (err && err.message || err));
          }
        },
      }, '✕'),
    );
  }

  app.onChange(['tasks'], () => { load(); });
  await load();
}
