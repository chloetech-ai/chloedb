// title: Tag input
// description: A box of tags the person adds by typing and pressing Enter or a comma, removes with the cross or Backspace, and picks from suggestions as they type.
// props: value: [tags]; onChange(tags) with the new list; suggestions: [known tags] offered while typing;
//   placeholder (default "Add a tag"); max: the most tags allowed; lower: true to lower-case tags (default true)
function component(props) {
  css('sa-tags', `
.sa-tags{position:relative;display:flex;flex-wrap:wrap;gap:6px;align-items:center;border:1px solid var(--line);background:var(--card);border-radius:8px;padding:5px 7px;min-height:38px;cursor:text}
.sa-tags:focus-within{border-color:var(--accent);box-shadow:0 0 0 3px color-mix(in srgb,var(--accent) 20%,transparent)}
.sa-tags .tag{display:inline-flex;align-items:center;gap:4px;background:var(--soft);border-radius:999px;padding:1px 4px 1px 10px;font-size:13px}
.sa-tags .tag button{border:0;background:none;box-shadow:none;padding:0 4px;color:var(--muted);cursor:pointer;font-size:12px}
.sa-tags input{border:0!important;box-shadow:none!important;outline:0;flex:1;min-width:90px;padding:3px!important;background:transparent!important}
.sa-tags-menu{position:absolute;left:0;right:0;top:calc(100% + 4px);background:var(--card);border:1px solid var(--line);border-radius:8px;box-shadow:0 6px 18px rgba(0,0,0,.12);z-index:20;max-height:200px;overflow:auto;padding:4px}
.sa-tags-menu button{display:block;width:100%;text-align:left;border:0;box-shadow:none;background:none;padding:5px 8px;border-radius:6px;cursor:pointer}
.sa-tags-menu button.on,.sa-tags-menu button:hover{background:var(--soft)}
`);
  const lower = props.lower !== false;
  let tags = (props.value || []).map(String).filter(Boolean);
  const norm = (t) => { const s = String(t).trim().replace(/^#/, '').replace(/\s+/g, ' '); return lower ? s.toLowerCase() : s; };
  const input = h('input', { placeholder: props.placeholder || 'Add a tag', 'aria-label': props.placeholder || 'Add a tag' });
  const menu = h('div', { class: 'sa-tags-menu', hidden: true });
  const chips = h('span', { style: 'display:contents' });
  const root = h('div', { class: 'sa-tags', onclick: () => input.focus() }, chips, input, menu);
  let active = 0;
  const set = (next) => { tags = next; drawChips(); if (props.onChange) props.onChange(tags.slice()); };
  const add = (t) => { const n = norm(t); if (!n || tags.includes(n) || (props.max && tags.length >= props.max)) return; set([...tags, n]); };
  function drawChips() {
    chips.replaceChildren(tags.map((t) => h('span', { class: 'tag' }, t, h('button', { type: 'button', 'aria-label': `Remove ${t}`, onclick: (e) => { e.stopPropagation(); set(tags.filter((x) => x !== t)); } }, '✕'))));
  }
  function drawMenu() {
    const q = input.value.trim();
    const pool = (props.suggestions || []).map(norm).filter((s) => s && !tags.includes(s));
    const hits = q ? lib.text.search(q, [...new Set(pool)]).slice(0, 8) : [];
    menu.hidden = !hits.length;
    active = Math.min(active, Math.max(0, hits.length - 1));
    menu.replaceChildren(hits.map((s, i) => h('button', { type: 'button', class: i === active ? 'on' : null, onmousedown: (e) => { e.preventDefault(); add(s); input.value = ''; drawMenu(); } }, s)));
    return hits;
  }
  input.addEventListener('input', () => {
    if (/[,;]/.test(input.value)) { input.value.split(/[,;]/).slice(0, -1).forEach(add); input.value = input.value.split(/[,;]/).pop(); }
    active = 0;
    drawMenu();
  });
  input.addEventListener('keydown', (e) => {
    const hits = menu.hidden ? [] : [...menu.children];
    if (e.key === 'ArrowDown' && hits.length) { e.preventDefault(); active = (active + 1) % hits.length; drawMenu(); } else if (e.key === 'ArrowUp' && hits.length) { e.preventDefault(); active = (active - 1 + hits.length) % hits.length; drawMenu(); } else if (e.key === 'Enter') {
      e.preventDefault();
      add(hits.length ? hits[active].textContent : input.value);
      input.value = '';
      drawMenu();
    } else if (e.key === 'Backspace' && !input.value && tags.length) set(tags.slice(0, -1));
    else if (e.key === 'Escape') menu.hidden = true;
  });
  input.addEventListener('blur', () => { if (input.value.trim()) { add(input.value); input.value = ''; } menu.hidden = true; });
  drawChips();
  return root;
}
