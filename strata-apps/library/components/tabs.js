// title: Tabs
// description: A row of tabs that switches between panels of one screen (Open / Done / Archived, Overview / History / Settings), with an optional count on each tab, arrow keys to move between tabs, and sideways scrolling when the tabs do not fit on a phone.
// props: tabs: [{id (text or number), label (text), count? (number or short text shown as a small badge), content (an element, a string, an array of them,
//   or a function () => element called the first time the tab opens and kept afterwards)}];
//   value: the id of the open tab (default the first; an unknown id also opens the first); onChange(id) when the person opens another tab
//   (the component switches by itself, so the view only keeps the id if it redraws); label: accessible name of the row of tabs (default "Tabs")
function component(props) {
  css('sa-tabs', `
.sa-tabs{display:flex;flex-direction:column;gap:12px;min-width:0}
.sa-tabs-bar{position:relative;display:flex;gap:2px;overflow-x:auto;overflow-y:hidden;box-shadow:inset 0 -1px 0 var(--line);scrollbar-width:none;-webkit-overflow-scrolling:touch;overscroll-behavior-x:contain}
.sa-tabs-bar::-webkit-scrollbar{display:none}
.sa-tabs-tab{flex:0 0 auto;display:inline-flex;align-items:center;gap:6px;margin:0;padding:8px 12px;border:0;border-bottom:2px solid transparent;border-radius:0;background:none;box-shadow:none;color:var(--muted);font:inherit;white-space:nowrap;cursor:pointer}
.sa-tabs-tab:hover{color:var(--fg)}
.sa-tabs-tab.on{color:var(--fg);border-bottom-color:var(--accent);font-weight:600}
.sa-tabs-tab:focus-visible{outline:2px solid var(--accent);outline-offset:-2px}
.sa-tabs-count{font-size:.75em;line-height:1.4;min-width:1.6em;padding:0 6px;border-radius:999px;background:var(--line);color:var(--fg);font-weight:500;text-align:center}
.sa-tabs-tab.on .sa-tabs-count{background:var(--accent);color:var(--bg)}
.sa-tabs-panel{min-width:0}
.sa-tabs-panel:focus-visible{outline:2px solid var(--accent);outline-offset:2px;border-radius:var(--radius)}
`);
  const tabs = (Array.isArray(props.tabs) ? props.tabs : []).filter((t) => t && t.id != null);
  const root = h('div', { class: 'sa-tabs' });
  if (!tabs.length) return root;

  const uid = 'sa-tabs-' + Math.random().toString(36).slice(2, 9);
  const indexOf = (id) => tabs.findIndex((t) => String(t.id) === String(id));
  let current = Math.max(0, props.value == null ? 0 : indexOf(props.value));
  const cache = new Map();

  const bar = h('div', { class: 'sa-tabs-bar', role: 'tablist', 'aria-label': props.label || 'Tabs', 'aria-orientation': 'horizontal' });
  const panel = h('div', { class: 'sa-tabs-panel', id: `${uid}-panel`, role: 'tabpanel', tabindex: '0' });

  const buttons = tabs.map((t, i) => h('button', {
    type: 'button', class: 'sa-tabs-tab', role: 'tab', id: `${uid}-tab-${i}`, 'aria-controls': `${uid}-panel`,
    onclick: () => select(i, false, true),
    onkeydown: (e) => {
      const n = tabs.length;
      const to = e.key === 'ArrowRight' ? (i + 1) % n
        : e.key === 'ArrowLeft' ? (i - 1 + n) % n
        : e.key === 'Home' ? 0
        : e.key === 'End' ? n - 1 : -1;
      if (to < 0) return;
      e.preventDefault();
      select(to, true, true);
    },
  },
  h('span', null, t.label == null ? String(t.id) : String(t.label)),
  t.count != null && t.count !== '' ? h('span', { class: 'sa-tabs-count' }, String(t.count)) : null));
  bar.append(buttons);

  function contentOf(i) {
    if (cache.has(i)) return cache.get(i);
    const c = tabs[i].content;
    let el;
    try { el = typeof c === 'function' ? c() : c; }
    catch (err) { el = h('p', { class: 'muted' }, 'This panel could not be shown.'); }
    if (el != null) cache.set(i, el);
    return el;
  }

  function reveal(btn) {
    const left = btn.offsetLeft, right = left + btn.offsetWidth;
    if (left < bar.scrollLeft) bar.scrollLeft = Math.max(0, left - 16);
    else if (right > bar.scrollLeft + bar.clientWidth) bar.scrollLeft = right - bar.clientWidth + 16;
  }

  function select(i, focus, notify) {
    const changed = i !== current;
    current = i;
    buttons.forEach((b, j) => {
      const on = j === i;
      b.classList.toggle('on', on);
      b.setAttribute('aria-selected', on ? 'true' : 'false');
      b.tabIndex = on ? 0 : -1;
    });
    panel.setAttribute('aria-labelledby', `${uid}-tab-${i}`);
    const el = contentOf(i);
    panel.replaceChildren(...(el == null ? [] : Array.isArray(el) ? el : [el]));
    if (focus) buttons[i].focus();
    reveal(buttons[i]);
    if (notify && changed && typeof props.onChange === 'function') props.onChange(tabs[i].id);
  }

  select(current, false, false);
  requestAnimationFrame(() => { if (root.isConnected) reveal(buttons[current]); });
  root.append(bar, panel);
  return root;
}
