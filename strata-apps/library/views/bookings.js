// title: Bookings
// description: Reserve one shared resource by the hour: pick a day, see which times are free or taken, book a time under a name, and cancel your own bookings.
// tables: bookings(day keyword, slot keyword, name text, author keyword, notes text)
async function render(root, app) {
  // Opening hours of the resource; adjust here.
  const OPEN = '08:00';
  const CLOSE = '20:00';
  const STEP = 60;      // minutes between slot starts, and the length of a booking
  const DAYS = 14;      // how many days ahead can be booked
  const CLOSED = [];    // weekdays with no slots (0 Sunday .. 6 Saturday)

  const me = await app.me().catch(() => null);
  const mine = new Set(); // bookings made in this window, cancellable even when signed out
  const today = app.today();
  const days = lib.dates.range(today, lib.dates.addDays(today, DAYS - 1));
  let day = days.includes(app.params && app.params.day) ? app.params.day : today;
  let pending = null; // {day, time, start}
  let rows = [];

  const dayLabel = (d) => `${lib.dates.weekdayName(d)} ${app.fmt.date(d)}`;
  const endOf = (slot) => lib.times.add(slot, STEP) || '';
  const canCancel = (b) =>
    mine.has(b.id) || !!(me && (b.author === me.id || me.role === 'owner'));

  async function load() {
    const params = {};
    const marks = days.map((d, i) => { params['d' + i] = d; return ':d' + i; }).join(', ');
    rows = await app.sql(
      `SELECT id, day, slot, name, author, notes FROM bookings WHERE day IN (${marks}) ORDER BY day, slot LIMIT 500`,
      params
    );
  }

  const slotsBox = h('div', { class: 'section' });
  const listBox = h('div', { class: 'section' });

  // Booking form: built once, kept out of what redraws.
  const formTitle = h('h3', null);
  const nameInput = h('input', { type: 'text', placeholder: 'Your name', required: true, class: 'grow', value: (me && me.name) || '' });
  const notesInput = h('input', { type: 'text', placeholder: 'Notes (optional)', class: 'grow' });
  const bookBtn = h('button', { type: 'submit', class: 'primary' }, 'Book');
  const backBtn = h('button', { type: 'button', class: 'ghost', onclick: () => hideForm() }, 'Back');
  const form = h('form', { class: 'col', onsubmit: submit },
    nameInput, notesInput, h('div', { class: 'row' }, bookBtn, backBtn));
  const formBox = h('div', { class: 'card section', style: 'display:none' }, formTitle, form);

  function showForm(choice) {
    pending = choice;
    formTitle.textContent = `Book ${choice.time}–${endOf(choice.time)}, ${dayLabel(choice.day)}`;
    formBox.style.display = '';
    draw();
    nameInput.focus();
  }

  function hideForm() {
    pending = null;
    formBox.style.display = 'none';
    draw();
  }

  async function submit(e) {
    e.preventDefault();
    if (!pending) return;
    const name = nameInput.value.trim();
    if (!name) { nameInput.focus(); return; }
    const { day: d, time } = pending;
    bookBtn.disabled = true;
    try {
      const clash = await app.sql('SELECT id FROM bookings WHERE day = :d AND slot = :s LIMIT 1', { d, s: time });
      if (clash.length) {
        app.toast('Sorry, that time was just taken. Pick another.');
        hideForm();
        await refresh();
        return;
      }
      const row = await app.insert('bookings', {
        day: d, slot: time, name, author: (me && me.id) || '', notes: notesInput.value.trim()
      });
      mine.add(row.id);
      app.toast(`Booked ${time} on ${dayLabel(d)}`);
      nameInput.value = (me && me.name) || '';
      notesInput.value = '';
      hideForm();
      await refresh();
    } catch (err) {
      app.toast('Could not book: ' + (err && err.message ? err.message : err));
    } finally {
      bookBtn.disabled = false;
    }
  }

  async function cancel(b) {
    try {
      await app.remove('bookings', b.id);
      mine.delete(b.id);
      app.toast('Booking cancelled');
      await refresh();
    } catch (err) {
      app.toast('Could not cancel: ' + (err && err.message ? err.message : err));
    }
  }

  function draw() {
    slotsBox.replaceChildren(ui.bookingSlots({
      taken: rows.map((r) => `${r.day}T${r.slot}`),
      from: today, days: DAYS, open: OPEN, close: CLOSE, step: STEP, length: STEP, closed: CLOSED,
      day,
      onDay: (d) => { day = d; draw(); },
      selected: pending ? pending.start : undefined,
      onBook: (choice) => { day = choice.day; showForm(choice); }
    }));

    const onDay = rows.filter((r) => r.day === day);
    listBox.replaceChildren(
      h('h2', null, `Booked on ${dayLabel(day)}`),
      onDay.length
        ? h('div', { class: 'list' }, onDay.map((b) => h('div', { class: 'item row' },
            h('span', { class: 'mono nowrap' }, `${b.slot}–${endOf(b.slot)}`),
            h('div', { class: 'col grow', style: 'gap:2px;min-width:0' },
              h('span', { class: 'clip' }, b.name || 'Someone'),
              b.notes ? h('small', { class: 'muted clip' }, b.notes) : null),
            canCancel(b)
              ? ui.confirmButton({ label: 'Cancel', confirmLabel: 'Cancel it?', small: true, onConfirm: () => cancel(b) })
              : null)))
        : ui.emptyState({ compact: true, icon: '📅', title: 'Nothing booked this day', text: 'Pick a free time above to book it.' })
    );
  }

  async function refresh() {
    await load();
    if (pending && rows.some((r) => r.day === pending.day && r.slot === pending.time)) {
      app.toast('That time was just booked by someone else.');
      pending = null;
      formBox.style.display = 'none';
    }
    draw();
  }

  root.append(
    h('h1', null, 'Bookings'),
    h('p', { class: 'muted' }, `Choose a day and a free time, ${OPEN}–${CLOSE}. Greyed-out times are taken or past.`),
    slotsBox,
    formBox,
    listBox
  );

  await load();
  draw();
  app.onChange(['bookings'], refresh);
}
