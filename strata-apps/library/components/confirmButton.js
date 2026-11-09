// title: Confirm button
// description: A button that asks before it acts, in place and without a dialog: the first tap turns it into "Sure?", a second tap within a few seconds does it, and otherwise it goes back. Typical uses are deleting a row, clearing a list or cancelling a booking.
// props: label: text of the button (default 'Delete'); confirmLabel: text while it waits for the second tap (default 'Sure?');
//   onConfirm(): called on the second tap (it may return a promise; the button is disabled until it settles);
//   danger: boolean, red styling (default true; false uses the accent once armed); timeout: ms before it falls back unconfirmed (default 3000);
//   small: boolean, the small button size (default false); disabled: boolean (default false)
function component(props) {
  css('sa-confirm', `
.sa-confirm{white-space:nowrap;transition:background-color .15s,color .15s,border-color .15s}
.sa-confirm.sa-confirm-d:not(.danger){color:var(--bad)}
.sa-confirm.armed{font-weight:600}
.sa-confirm.busy{cursor:progress;opacity:.7}
`);
  const label = props.label != null && props.label !== '' ? String(props.label) : 'Delete';
  const confirmLabel = props.confirmLabel != null && props.confirmLabel !== '' ? String(props.confirmLabel) : 'Sure?';
  const danger = props.danger !== false;
  const timeout = Number(props.timeout) > 0 ? Number(props.timeout) : 3000;
  const disabled = !!props.disabled;
  const GUARD = 350; // a quick double click does not count as confirming

  let armed = false, busy = false, armedAt = 0, timer = null;

  const btn = h('button', {
    type: 'button',
    'aria-live': 'polite',
    disabled,
    onclick: (e) => {
      e.stopPropagation();
      if (busy || btn.disabled) return;
      if (!armed) return arm();
      if (Date.now() - armedAt < GUARD) return;
      confirm();
    },
    onkeydown: (e) => { if (e.key === 'Escape' && armed) { e.stopPropagation(); disarm(); } },
    onblur: () => { if (armed && !busy) disarm(); },
  }, label);

  function paint() {
    const cls = ['sa-confirm'];
    if (danger) cls.push('sa-confirm-d');
    if (props.small) cls.push('small');
    if (armed) cls.push('armed', danger ? 'danger' : 'primary');
    if (busy) cls.push('busy');
    btn.className = cls.join(' ');
    btn.textContent = armed ? confirmLabel : label;
    btn.title = armed ? `Tap again to confirm: ${label}` : '';
    btn.setAttribute('aria-label', armed ? `${confirmLabel} Tap again to ${label.toLowerCase()}` : label);
    btn.setAttribute('aria-busy', busy ? 'true' : 'false');
  }

  function arm() {
    // keep the width so the row around it does not jump
    const w = btn.getBoundingClientRect().width;
    if (w) btn.style.minWidth = `${Math.ceil(w)}px`;
    armed = true;
    armedAt = Date.now();
    clearTimeout(timer);
    timer = setTimeout(() => { if (!busy) disarm(); }, timeout);
    paint();
  }

  function disarm() {
    clearTimeout(timer);
    timer = null;
    armed = false;
    btn.style.minWidth = '';
    paint();
  }

  async function confirm() {
    clearTimeout(timer);
    timer = null;
    busy = true;
    btn.disabled = true;
    paint();
    try {
      if (typeof props.onConfirm === 'function') await props.onConfirm();
    } catch (err) {
      console.error(err);
    } finally {
      busy = false;
      btn.disabled = disabled;
      disarm();
    }
  }

  paint();
  return btn;
}
