// title: Events
// description: Upcoming events as an agenda of the next weeks, with a form to add one (date, start and end time, place, notes) and past events folded away below.
// tables: events(title text, starts timestamp, ends timestamp, place text, notes text)
async function render(root, app) {
  const today = app.today();
  const pad = (n) => String(n).padStart(2, '0');
  const localTime = (iso) => { const d = new Date(iso); return pad(d.getHours()) + ':' + pad(d.getMinutes()); };
  const toIso = (day, hhmm) => new Date(day + 'T' + hhmm + ':00').toISOString();

  function when(ev) {
    const day = lib.dates.fromTime(ev.starts);
    let s = app.fmt.date(ev.starts) + ', ' + app.fmt.time(ev.starts);
    if (ev.ends) {
      const endDay = lib.dates.fromTime(ev.ends);
      s += ' – ' + (endDay === day ? app.fmt.time(ev.ends) : app.fmt.datetime(ev.ends));
    }
    return s;
  }

  function openEvent(ev) {
    const body = h('div', { class: 'col' },
      h('div', { class: 'muted' }, when(ev)),
      ev.place ? h('div', null, '📍 ', ev.place) : null,
      ev.notes ? h('p', { style: 'white-space: pre-wrap; margin: 0' }, ev.notes) : null
    );
    document.body.append(ui.dialog({
      title: ev.title || 'Event',
      body,
      actions: [
        { label: 'Delete', danger: true, onClick: async (close) => {
          if (!confirm('Delete "' + (ev.title || 'this event') + '"?')) return false;
          try { await app.remove('events', ev.id); app.toast('Event deleted'); }
          catch (e) { app.toast('Could not delete: ' + e.message); return false; }
        } },
        { label: 'Close', primary: true }
      ]
    }));
  }

  // ---- form (kept out of the part that redraws) ----
  const f = {
    title: h('input', { placeholder: 'What', required: true, class: 'grow', 'aria-label': 'Title' }),
    day: h('input', { type: 'date', value: today, required: true, 'aria-label': 'Date' }),
    start: h('input', { type: 'time', value: '18:00', required: true, 'aria-label': 'Starts' }),
    end: h('input', { type: 'time', 'aria-label': 'Ends' }),
    place: h('input', { placeholder: 'Where (optional)', class: 'grow', 'aria-label': 'Place' }),
    notes: h('textarea', { placeholder: 'Notes (optional)', rows: 2, 'aria-label': 'Notes' })
  };
  const saveBtn = h('button', { class: 'primary', type: 'submit' }, 'Add event');
  const err = h('div', { class: 'small', style: 'color: var(--bad)' });

  async function onSubmit(e) {
    e.preventDefault();
    err.textContent = '';
    const title = f.title.value.trim();
    const day = f.day.value;
    if (!title) { err.textContent = 'Give the event a title.'; f.title.focus(); return; }
    if (!lib.dates.isDay(day)) { err.textContent = 'Choose a date.'; return; }
    if (!f.start.value) { err.textContent = 'Choose a start time.'; return; }
    const starts = toIso(day, f.start.value);
    let ends = null;
    if (f.end.value) {
      // An end earlier than the start runs past midnight into the next day.
      const endDay = f.end.value <= f.start.value ? lib.dates.addDays(day, 1) : day;
      ends = toIso(endDay, f.end.value);
    }
    saveBtn.disabled = true;
    try {
      await app.insert('events', {
        title, starts, ends,
        place: f.place.value.trim() || null,
        notes: f.notes.value.trim() || null
      });
      f.title.value = ''; f.place.value = ''; f.notes.value = ''; f.end.value = '';
      app.toast('Event added');
      f.title.focus();
    } catch (e2) {
      err.textContent = 'Could not save: ' + e2.message;
    } finally {
      saveBtn.disabled = false;
    }
  }

  const form = h('form', { class: 'col card', onsubmit: onSubmit },
    h('div', { class: 'row' }, f.title),
    h('div', { class: 'row' },
      h('label', { class: 'col small' }, 'Date', f.day),
      h('label', { class: 'col small' }, 'Starts', f.start),
      h('label', { class: 'col small' }, 'Ends', f.end)
    ),
    h('div', { class: 'row' }, f.place),
    f.notes,
    h('div', { class: 'row' }, err, h('span', { class: 'right' }, saveBtn))
  );

  // ---- the part that redraws ----
  const list = h('div', { class: 'col' });
  let pastOpen = false;

  async function draw() {
    const now = app.now();
    const [upcoming, past] = await Promise.all([
      app.sql(
        'SELECT * FROM events WHERE ends >= :now OR (ends IS NULL AND starts >= :now) ORDER BY starts ASC LIMIT 500',
        { now }),
      app.sql(
        'SELECT * FROM events WHERE ends < :now OR (ends IS NULL AND starts < :now) ORDER BY starts DESC LIMIT 200',
        { now })
    ]);

    const byId = {};
    upcoming.forEach((ev) => { byId[ev.id] = ev; });

    const agendaPart = upcoming.length
      ? ui.agenda({
          items: upcoming.map((ev) => ({
            id: ev.id,
            date: lib.dates.fromTime(ev.starts),
            time: localTime(ev.starts),
            start: ev.starts,
            end: ev.ends || null,
            title: ev.title || 'Untitled',
            subtitle: ev.place || ''
          })),
          onSelect: (item) => { const ev = byId[item && item.id]; if (ev) openEvent(ev); }
        })
      : ui.emptyState({
          icon: '📅',
          title: 'Nothing coming up',
          text: 'Add an event above with its date and time, and it shows here.',
          compact: true
        });

    let pastPart = null;
    if (past.length) {
      const details = h('details', { open: pastOpen, ontoggle: () => { pastOpen = details.open; } },
        h('summary', null, 'Past events ', h('span', { class: 'muted' }, '(' + (past.length >= 200 ? '200+' : past.length) + ')')),
        h('div', { class: 'list' }, past.map((ev) =>
          h('div', { class: 'item row', style: 'cursor: pointer', onclick: () => openEvent(ev) },
            h('div', { class: 'col grow', style: 'gap: 2px; min-width: 0' },
              h('div', { class: 'clip' }, ev.title || 'Untitled'),
              h('div', { class: 'small muted clip' }, when(ev), ev.place ? ' · ' + ev.place : '')
            )
          )
        ))
      );
      pastPart = h('div', { class: 'section' }, details);
    }

    list.replaceChildren(
      h('h2', null, 'Coming up'),
      agendaPart,
      pastPart
    );
  }

  root.append(h('h1', null, 'Events'), form, list);
  await draw();
  app.onChange(['events'], draw);
}
