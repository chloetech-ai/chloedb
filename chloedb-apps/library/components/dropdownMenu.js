// title: Dropdown menu
// description: A button that opens a short menu of actions (edit, duplicate, export, delete) for a row, a card or a toolbar; it closes on a choice, an outside click or Escape, and works with the arrow keys.
// props: items: [{label, onClick(), danger? (shown in the danger colour), disabled? (shown but not chosen), icon? (a short text or emoji before the label), divider?: true (a separating line instead of an action)}];
//   label: text or element of the button (default '⋯', then named "More actions" for screen readers); align: 'left' (default) or 'right', the edge of the button the menu lines up with
function component(props) {
  css('sa-menu', `
.sa-menu{position:relative;display:inline-block}
.sa-menu>.sa-menu-list{position:absolute;top:calc(100% + 4px);left:0;z-index:50;min-width:170px;max-width:calc(100vw - 16px);max-height:min(60vh,360px);overflow:auto;
  background:var(--card);color:var(--fg);border:1px solid var(--line);border-radius:var(--radius);box-shadow:0 8px 24px rgba(0,0,0,.18);padding:4px;display:flex;flex-direction:column}
.sa-menu>.sa-menu-list[hidden]{display:none}
.sa-menu.right>.sa-menu-list{left:auto;right:0}
.sa-menu.up>.sa-menu-list{top:auto;bottom:calc(100% + 4px)}
.sa-menu.flip>.sa-menu-list{left:auto;right:0}
.sa-menu.right.flip>.sa-menu-list{right:auto;left:0}
.sa-menu-list>button{all:unset;box-sizing:border-box;display:flex;align-items:center;gap:8px;width:100%;padding:8px 12px;border-radius:calc(var(--radius) - 2px);
  cursor:pointer;white-space:nowrap;overflow:hidden;text-overflow:ellipsis;color:var(--fg);font:inherit;line-height:1.3}
.sa-menu-list>button:hover:not(:disabled),.sa-menu-list>button:focus{background:var(--line);background:color-mix(in srgb,var(--accent) 14%,transparent);outline:none}
.sa-menu-list>button:focus-visible{box-shadow:inset 0 0 0 2px var(--accent)}
.sa-menu-list>button.bad{color:var(--bad)}
.sa-menu-list>button.bad:hover:not(:disabled),.sa-menu-list>button.bad:focus{background:color-mix(in srgb,var(--bad) 14%,transparent)}
.sa-menu-list>button:disabled{opacity:.45;cursor:default}
.sa-menu-list .ic{width:1.2em;text-align:center;flex:none}
.sa-menu-list .lb{overflow:hidden;text-overflow:ellipsis}
.sa-menu-list>.sep{height:1px;background:var(--line);margin:4px 2px;flex:none}
`);
  const items = Array.isArray(props.items) ? props.items.filter(Boolean) : [];
  const align = props.align === 'right' ? 'right' : 'left';
  const label = props.label == null || props.label === '' ? '⋯' : props.label;
  const iconOnly = label === '⋯';
  const id = 'sa-menu-' + Math.random().toString(36).slice(2, 10);
  let open = false;

  const buttons = [];
  const entries = items.map((it) => {
    if (it.divider) return h('div', { class: 'sep', role: 'separator' });
    const b = h('button', {
      type: 'button', role: 'menuitem', tabindex: '-1',
      class: it.danger ? 'bad' : null,
      disabled: it.disabled ? true : null,
      'aria-disabled': it.disabled ? 'true' : null,
      onclick: () => choose(it),
    },
      it.icon != null && it.icon !== '' ? h('span', { class: 'ic', 'aria-hidden': 'true' }, it.icon) : null,
      h('span', { class: 'lb' }, it.label == null ? '' : String(it.label)));
    buttons.push(b);
    return b;
  });
  // drop dividers at the ends and doubled ones
  const cleaned = entries.filter((el, i) => {
    if (el.getAttribute('role') !== 'separator') return true;
    const prev = entries.slice(0, i).reverse().find(Boolean);
    const hasBefore = entries.slice(0, i).some((e) => e.getAttribute('role') !== 'separator');
    const hasAfter = entries.slice(i + 1).some((e) => e.getAttribute('role') !== 'separator');
    return hasBefore && hasAfter && prev && prev.getAttribute('role') !== 'separator';
  });

  const enabled = () => buttons.filter((b) => !b.disabled);
  const focusAt = (i) => {
    const list = enabled();
    if (!list.length) return;
    list[((i % list.length) + list.length) % list.length].focus();
  };
  const current = () => enabled().indexOf(document.activeElement);

  const menu = h('div', {
    id, class: 'sa-menu-list', role: 'menu',
    'aria-label': typeof label === 'string' && !iconOnly ? label : 'Actions',
    onkeydown: (e) => {
      const n = enabled().length;
      const cur = current();
      if (e.key === 'ArrowDown') { e.preventDefault(); focusAt(cur < 0 ? 0 : cur + 1); }
      else if (e.key === 'ArrowUp') { e.preventDefault(); focusAt(cur < 0 ? n - 1 : cur - 1); }
      else if (e.key === 'Home') { e.preventDefault(); focusAt(0); }
      else if (e.key === 'End') { e.preventDefault(); focusAt(n - 1); }
      else if (e.key === 'Escape') { e.preventDefault(); e.stopPropagation(); close(true); }
      else if (e.key === 'Tab') close(false);
    },
  }, cleaned);
  menu.hidden = true;

  const trigger = h('button', {
    type: 'button',
    class: iconOnly ? 'icon ghost' : null,
    'aria-haspopup': 'menu', 'aria-expanded': 'false', 'aria-controls': id,
    'aria-label': iconOnly ? 'More actions' : null,
    title: iconOnly ? 'More actions' : null,
    disabled: buttons.length ? null : true,
    onclick: () => (open ? close(true) : show(0)),
    onkeydown: (e) => {
      if (e.key === 'ArrowDown') { e.preventDefault(); show(0); }
      else if (e.key === 'ArrowUp') { e.preventDefault(); show(-1); }
      else if (e.key === 'Escape' && open) { e.preventDefault(); e.stopPropagation(); close(true); }
    },
  }, label);

  const root = h('span', { class: `sa-menu ${align}` }, trigger, menu);

  const outside = (e) => { if (!root.contains(e.target)) close(false); };

  function show(i) {
    if (open || !buttons.length) return;
    open = true;
    root.classList.remove('up', 'flip');
    menu.hidden = false;
    trigger.setAttribute('aria-expanded', 'true');
    const r = menu.getBoundingClientRect();
    const t = trigger.getBoundingClientRect();
    const vh = window.innerHeight || document.documentElement.clientHeight;
    const vw = window.innerWidth || document.documentElement.clientWidth;
    if (r.bottom > vh && t.top - r.height - 4 > 0) root.classList.add('up');
    if ((align === 'left' && r.right > vw) || (align === 'right' && r.left < 0)) root.classList.add('flip');
    document.addEventListener('pointerdown', outside, true);
    focusAt(i);
  }

  function close(refocus) {
    if (!open) return;
    open = false;
    menu.hidden = true;
    trigger.setAttribute('aria-expanded', 'false');
    document.removeEventListener('pointerdown', outside, true);
    if (refocus && root.isConnected) trigger.focus();
  }

  function choose(it) {
    if (it.disabled) return;
    close(true);
    if (typeof it.onClick === 'function') it.onClick();
  }

  return root;
}
