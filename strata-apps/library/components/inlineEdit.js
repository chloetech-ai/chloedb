// title: Inline edit
// description: Text that turns into a box to edit it when clicked, then saves with Enter or by clicking away, for renaming a list, a title, a note or any single field in place.
// props: value: text to show (default ''); onSave(text) when the person saves a changed text, trimmed (may return a Promise; a thrown error is shown and editing stays open);
//   without onSave the text is only shown; placeholder: text shown muted when empty (default 'Click to edit');
//   multiline: true for a box of several lines, saved with Ctrl/⌘+Enter or by clicking away (default false; Enter saves a single line);
//   validate(text) -> a message to show and keep editing, or null when fine; as: the element to render, such as 'h2', 'p' or 'div' (default 'span').
//   Escape cancels and keeps the old text.
function component(props) {
  css('sa-inline', `
.sa-inline{position:relative;overflow-wrap:anywhere}
span.sa-inline.editing{display:inline-block;min-width:12em;max-width:100%;vertical-align:top}
.sa-inline-text[role=button]{cursor:text;border-radius:4px;padding:0 3px;margin:0 -3px}
.sa-inline-text[role=button]:hover{background:color-mix(in srgb,var(--accent) 10%,transparent)}
.sa-inline-text[role=button]:focus-visible{outline:2px solid var(--accent);outline-offset:1px}
.sa-inline-text.empty{color:var(--muted);font-style:italic;font-weight:normal}
.sa-inline-text.multi{white-space:pre-wrap}
.sa-inline-field{font:inherit;color:inherit;letter-spacing:inherit;line-height:inherit;width:100%;box-sizing:border-box;margin:0;padding:2px 6px}
textarea.sa-inline-field{resize:vertical;display:block}
.sa-inline-field[aria-invalid=true]{border-color:var(--bad)}
.sa-inline.saving .sa-inline-field{opacity:.6}
.sa-inline-error{display:block;color:var(--bad);font-size:.8rem;font-weight:normal;font-style:normal;margin-top:2px}
`);
  const tag = typeof props.as === 'string' && /^[a-z][a-z0-9]*$/i.test(props.as) ? props.as.toLowerCase() : 'span';
  const multiline = !!props.multiline;
  const placeholder = props.placeholder == null ? 'Click to edit' : String(props.placeholder);
  const editable = typeof props.onSave === 'function';
  let value = props.value == null ? '' : String(props.value);
  let editing = false, saving = false;
  const root = h(tag, { class: 'sa-inline' });

  function show(focus) {
    editing = false;
    root.classList.remove('editing', 'saving');
    const empty = value === '';
    const display = h('span', {
      class: 'sa-inline-text' + (empty ? ' empty' : '') + (multiline ? ' multi' : ''),
      role: editable ? 'button' : null,
      tabindex: editable ? '0' : null,
      title: editable ? 'Click to edit' : null,
      'aria-label': editable ? `${empty ? placeholder : value}, edit` : null,
      onclick: editable ? () => edit() : null,
      onkeydown: editable ? (e) => { if (e.key === 'Enter' || e.key === ' ') { e.preventDefault(); edit(); } } : null,
    }, empty ? placeholder : value);
    root.replaceChildren(display);
    if (focus) display.focus();
  }

  function edit() {
    if (editing) return;
    editing = true;
    let cancelled = false;
    root.classList.add('editing');
    const field = multiline
      ? h('textarea', { rows: Math.max(2, Math.min(12, value.split('\n').length + 1)), value })
      : h('input', { type: 'text', value });
    field.className = 'sa-inline-field';
    field.placeholder = placeholder;
    field.setAttribute('aria-label', placeholder);
    const msg = h('small', { class: 'sa-inline-error', role: 'alert' });
    msg.hidden = true;

    function fail(text) {
      msg.textContent = text;
      msg.hidden = false;
      field.setAttribute('aria-invalid', 'true');
    }
    function clear() {
      msg.hidden = true;
      msg.textContent = '';
      field.removeAttribute('aria-invalid');
    }

    async function commit(focus) {
      if (saving || !editing || cancelled) return;
      const text = multiline ? field.value.replace(/^\s*\n|\s+$/g, '') : field.value.trim();
      if (text === value) { show(focus); return; }
      let err = null;
      try { err = typeof props.validate === 'function' ? props.validate(text) : null; }
      catch (e) { err = (e && e.message) || 'This is not valid'; }
      if (err) { fail(String(err)); if (focus) field.focus(); return; }
      saving = true;
      field.disabled = true;
      root.classList.add('saving');
      try {
        await props.onSave(text);
        value = text;
        saving = false;
        show(focus);
      } catch (e) {
        saving = false;
        field.disabled = false;
        root.classList.remove('saving');
        fail((e && e.message) || 'Could not save');
        field.focus();
      }
    }

    field.addEventListener('input', clear);
    field.addEventListener('blur', () => { commit(false); });
    field.addEventListener('keydown', (e) => {
      if (e.isComposing) return;
      if (e.key === 'Escape') {
        e.preventDefault();
        if (saving) return;
        cancelled = true;
        show(true);
      } else if (e.key === 'Enter' && (!multiline || e.ctrlKey || e.metaKey)) {
        e.preventDefault();
        commit(true);
      }
    });

    root.replaceChildren(field, msg);
    field.focus();
    if (multiline) field.setSelectionRange(field.value.length, field.value.length);
    else field.select();
  }

  show(false);
  return root;
}
