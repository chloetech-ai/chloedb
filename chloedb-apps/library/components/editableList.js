// title: Editable list
// description: A list of short texts to add, edit, remove and reorder in place (steps of a routine, packing items, agenda points, options of a poll), with an optional limit and numbering.
// props: items: [strings] to start with (default []); onChange(items) with the whole new list after each add, edit, remove or move
//   (without it the list is only shown); placeholder: text of the add box (default 'Add an item'); max: the most items allowed
//   (default no limit; the add box is disabled at the limit and a count shows); numbered: true to number the items (default false);
//   reorder: false to hide the up and down buttons (default true)
function component(props) {
  css('sa-elist', `
.sa-elist{display:flex;flex-direction:column;gap:8px;min-width:0}
.sa-elist .items{list-style:none;margin:0;padding:0;border:1px solid var(--line);border-radius:var(--radius);background:var(--card)}
.sa-elist .items:empty{display:none}
.sa-elist li{display:flex;align-items:center;gap:2px;padding:3px 6px;border-top:1px solid var(--line);min-height:40px}
.sa-elist li:first-child{border-top:0}
.sa-elist .n{flex:none;color:var(--muted);min-width:1.8em;text-align:right;padding-right:4px;font-variant-numeric:tabular-nums}
.sa-elist .txt{flex:1;min-width:0;text-align:left;background:none;border:0;box-shadow:none;margin:0;padding:6px 4px;color:inherit;font:inherit;font-weight:normal;overflow-wrap:anywhere;white-space:normal;cursor:text;border-radius:calc(var(--radius) / 2)}
.sa-elist span.txt{cursor:default}
.sa-elist button.txt:hover{background:color-mix(in srgb,var(--fg) 5%,transparent)}
.sa-elist .edit{flex:1;min-width:0;margin:0}
.sa-elist li .icon{flex:none}
.sa-elist li .icon:disabled{opacity:.3}
.sa-elist .add{display:flex;gap:6px;align-items:center;margin:0}
.sa-elist .add input{flex:1;min-width:0}
.sa-elist .count{flex:none;color:var(--muted);font-size:.85em;font-variant-numeric:tabular-nums}
`);
  const onChange = typeof props.onChange === 'function' ? props.onChange : null;
  const max = Number(props.max) > 0 ? Math.floor(Number(props.max)) : Infinity;
  const numbered = props.numbered === true;
  const reorder = props.reorder !== false && !!onChange;
  const placeholder = props.placeholder || 'Add an item';
  let items = (Array.isArray(props.items) ? props.items : [])
    .map((x) => (x == null ? '' : String(x)))
    .filter((x) => x.trim() !== '');
  let editing = -1;

  const root = h('div', { class: 'sa-elist' });
  const list = h(numbered ? 'ol' : 'ul', { class: 'items', role: 'list' });
  const empty = h('p', { class: 'muted small', style: 'margin:0' }, 'Nothing yet');
  const input = h('input', { type: 'text', placeholder, 'aria-label': placeholder, enterkeyhint: 'done' });
  const addBtn = h('button', { type: 'submit', class: 'primary' }, 'Add');
  const count = h('span', { class: 'count', 'aria-live': 'polite' });
  const form = onChange ? h('form', { class: 'add', onsubmit: (e) => {
    e.preventDefault();
    const text = input.value.trim();
    if (!text || items.length >= max) return;
    input.value = '';
    commit([...items, text]);
    if (!input.disabled) input.focus();
  } }, input, addBtn, max !== Infinity ? count : null) : null;

  function commit(next, focus) {
    items = next;
    editing = -1;
    draw(focus);
    onChange && onChange(items.slice());
  }

  function move(i, d) {
    const j = i + d;
    if (j < 0 || j >= items.length) return;
    const next = items.slice();
    [next[i], next[j]] = [next[j], next[i]];
    // keep focus on the same button unless it is now disabled
    const part = (d < 0 && j === 0) ? 'down' : (d > 0 && j === items.length - 1) ? 'up' : d < 0 ? 'up' : 'down';
    commit(next, { index: j, part });
  }

  function editRow(i) {
    let done = false;
    const field = h('input', { type: 'text', class: 'edit', value: items[i], 'aria-label': `Edit item ${i + 1}` });
    const finish = (save) => {
      if (done) return;
      done = true;
      const text = field.value.trim();
      if (save && text && text !== items[i]) {
        const next = items.slice();
        next[i] = text;
        commit(next, { index: i, part: 'txt' });
      } else {
        editing = -1;
        draw({ index: i, part: 'txt' });
      }
    };
    field.addEventListener('keydown', (e) => {
      if (e.key === 'Enter') { e.preventDefault(); finish(true); }
      else if (e.key === 'Escape') { e.preventDefault(); e.stopPropagation(); finish(false); }
    });
    field.addEventListener('blur', () => finish(true));
    return field;
  }

  function draw(focus) {
    const rows = items.map((text, i) => {
      const last = i === items.length - 1;
      const parts = {};
      let body;
      if (onChange && editing === i) {
        body = parts.edit = editRow(i);
      } else if (onChange) {
        body = parts.txt = h('button', { type: 'button', class: 'txt', title: 'Tap to edit', 'aria-label': `${text}, edit`,
          onclick: () => { editing = i; draw({ index: i, part: 'edit' }); } }, text);
      } else {
        body = h('span', { class: 'txt' }, text);
      }
      return h('li', null,
        numbered ? h('span', { class: 'n', 'aria-hidden': 'true' }, `${i + 1}.`) : null,
        body,
        reorder ? [
          parts.up = h('button', { type: 'button', class: 'ghost icon small', disabled: i === 0, 'aria-label': `Move up: ${text}`, title: 'Move up', onclick: () => move(i, -1) }, '↑'),
          parts.down = h('button', { type: 'button', class: 'ghost icon small', disabled: last, 'aria-label': `Move down: ${text}`, title: 'Move down', onclick: () => move(i, 1) }, '↓'),
        ] : null,
        onChange ? h('button', { type: 'button', class: 'ghost icon small', 'aria-label': `Remove: ${text}`, title: 'Remove',
          onclick: () => {
            const next = items.filter((_, k) => k !== i);
            commit(next, next.length ? { index: Math.min(i, next.length - 1), part: 'txt' } : null);
            if (!next.length) input.focus();
          } }, '×') : null,
      ).__parts = parts, rows_parts(parts);
    });
    function rows_parts(p) { return p; }
    list.replaceChildren(rows.map((p, i) => list_item(i, p)));
    function list_item() { return null; }
  }

  // build rows (kept simple: one pass that also restores focus)
  function render(focus) {
    list.replaceChildren();
    let target = null;
    items.forEach((text, i) => {
      const last = i === items.length - 1;
      let body, txt = null, up = null, down = null;
      if (onChange && editing === i) body = editRow(i);
      else if (onChange) body = txt = h('button', { type: 'button', class: 'txt', title: 'Tap to edit', 'aria-label': `${text}, edit`,
        onclick: () => { editing = i; render({ index: i, part: 'edit' }); } }, text);
      else body = h('span', { class: 'txt' }, text);
      if (reorder) {
        up = h('button', { type: 'button', class: 'ghost icon small', disabled: i === 0, 'aria-label': `Move up: ${text}`, title: 'Move up', onclick: () => move(i, -1) }, '↑');
        down = h('button', { type: 'button', class: 'ghost icon small', disabled: last, 'aria-label': `Move down: ${text}`, title: 'Move down', onclick: () => move(i, 1) }, '↓');
      }
      const remove = onChange ? h('button', { type: 'button', class: 'ghost icon small', 'aria-label': `Remove: ${text}`, title: 'Remove',
        onclick: () => {
          const next = items.filter((_, k) => k !== i);
          commit(next, next.length ? { index: Math.min(i, next.length - 1), part: 'txt' } : null);
          if (!next.length && !input.disabled) input.focus();
        } }, '×') : null;
      list.append(h('li', null, numbered ? h('span', { class: 'n', 'aria-hidden': 'true' }, `${i + 1}.`) : null, body, up, down, remove));
      if (focus && focus.index === i) target = { txt, up, down, edit: editing === i ? body : null }[focus.part] || txt;
    });
    const full = items.length >= max;
    input.disabled = full;
    addBtn.disabled = full;
    input.placeholder = full ? `Limit of ${max} reached` : placeholder;
    count.textContent = `${items.length} / ${max}`;
    empty.style.display = !onChange && !items.length ? '' : 'none';
    if (target) {
      target.focus();
      if (target.select && target.tagName === 'INPUT') target.select();
    }
  }
  draw = render;

  root.append(list, empty, form);
  render();
  return root;
}
