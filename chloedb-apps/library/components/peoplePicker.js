// title: People picker
// description: Choose one or several members of the app from a searchable list with avatars (an assignee, the people at a meeting); it loads the app's members itself unless given a list.
// props: value: a person id, or [ids] with multiple; onChange(id or [ids]); multiple (default false);
//   people: [{id, name, email}] to choose from (default: the app's members, from app.people()); placeholder (default 'Choose someone');
//   allowNone (default true for one person): a "Nobody" choice
function component(props) {
  css('sa-people', `
.sa-people{position:relative;min-width:200px}
.sa-people .field{display:flex;flex-wrap:wrap;gap:6px;align-items:center;border:1px solid var(--line);border-radius:8px;background:var(--card);padding:4px 8px;min-height:38px;cursor:pointer}
.sa-people .field:focus-within{border-color:var(--accent)}
.sa-people .chosen{display:inline-flex;align-items:center;gap:6px;background:var(--soft);border-radius:999px;padding:2px 8px 2px 2px;font-size:13px}
.sa-people .chosen button{border:0;box-shadow:none;background:none;padding:0 2px;color:var(--muted);cursor:pointer}
.sa-people input{border:0!important;box-shadow:none!important;outline:0;flex:1;min-width:80px;padding:2px!important;background:transparent!important}
.sa-people .menu{position:absolute;left:0;right:0;top:calc(100% + 4px);background:var(--card);border:1px solid var(--line);border-radius:8px;box-shadow:0 8px 24px rgba(0,0,0,.14);max-height:240px;overflow:auto;padding:4px;z-index:40}
.sa-people .menu button{display:flex;align-items:center;gap:8px;width:100%;text-align:left;border:0;box-shadow:none;background:none;padding:6px 8px;border-radius:6px}
.sa-people .menu button:hover,.sa-people .menu button.on{background:var(--soft)}
`);
  const multiple = !!props.multiple;
  let chosen = [].concat(props.value === undefined || props.value === null || props.value === '' ? [] : props.value).map(String);
  let people = props.people || null;
  const input = h('input', { placeholder: chosen.length ? '' : props.placeholder || 'Choose someone', 'aria-label': props.placeholder || 'Choose someone' });
  const chips = h('span', { style: 'display:contents' });
  const menu = h('div', { class: 'menu', hidden: true });
  const root = h('div', { class: 'sa-people' }, h('div', { class: 'field', onclick: () => { input.focus(); open(); } }, chips, input), menu);
  const byId = (id) => (people || []).find((p) => String(p.id) === id);
  const emit = () => { if (props.onChange) props.onChange(multiple ? chosen.slice() : chosen[0] || null); };
  function drawChips() {
    chips.replaceChildren(chosen.map((id) => { const p = byId(id) || { name: '…' }; return h('span', { class: 'chosen' }, ui.avatar({ name: p.name, email: p.email, size: 20 }), p.name,
      h('button', { type: 'button', 'aria-label': `Remove ${p.name}`, onclick: (e) => { e.stopPropagation(); chosen = chosen.filter((x) => x !== id); drawChips(); emit(); } }, '✕')); }));
    input.placeholder = chosen.length ? '' : props.placeholder || 'Choose someone';
  }
  function open() {
    if (!people) { menu.hidden = false; menu.replaceChildren(h('div', { class: 'small muted', style: 'padding:6px' }, 'Loading…')); return; }
    const q = input.value.trim();
    const pool = people.filter((p) => !chosen.includes(String(p.id)) || !multiple);
    const hits = q ? lib.text.search(q, pool, (p) => `${p.name} ${p.email || ''}`) : pool;
    const none = !multiple && props.allowNone !== false && !q ? [h('button', { type: 'button', onclick: () => pick(null) }, h('span', { class: 'muted' }, 'Nobody'))] : [];
    menu.replaceChildren(...none, ...hits.slice(0, 50).map((p) => h('button', { type: 'button', class: chosen.includes(String(p.id)) ? 'on' : null, onclick: () => pick(p) },
      ui.avatar({ name: p.name, email: p.email, size: 24 }), h('span', { class: 'grow' }, p.name, p.email ? h('div', { class: 'small muted' }, p.email) : null))));
    if (!hits.length && !none.length) menu.append(h('div', { class: 'small muted', style: 'padding:6px' }, people.length ? 'No one matches' : 'Nobody has joined this app yet'));
    menu.hidden = false;
  }
  function pick(p) {
    if (!p) chosen = [];
    else if (multiple) chosen = [...chosen, String(p.id)];
    else chosen = [String(p.id)];
    input.value = '';
    drawChips();
    emit();
    if (!multiple) menu.hidden = true; else open();
  }
  input.addEventListener('input', open);
  input.addEventListener('keydown', (e) => { if (e.key === 'Escape') menu.hidden = true; if (e.key === 'Backspace' && !input.value && chosen.length) { chosen = chosen.slice(0, -1); drawChips(); emit(); } });
  document.addEventListener('click', (e) => { if (!root.contains(e.target)) menu.hidden = true; });
  if (!people) app.people().then((list) => { people = list || []; drawChips(); if (!menu.hidden) open(); }).catch(() => { people = []; });
  drawChips();
  return root;
}
