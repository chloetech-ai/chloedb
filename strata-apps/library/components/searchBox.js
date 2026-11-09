// title: Search box
// description: A search input with a magnifier, a clear button and a short wait before searching, for filtering lists, finding notes, contacts or records as the person types; shows how many results matched when given.
// props: value: text to start with (default ''); onSearch(text): called with the trimmed text once typing pauses for delay ms, and at once when cleared;
//   delay: milliseconds to wait (default 250; 0 searches on every key); placeholder (default 'Search'; also the box's accessible name);
//   autofocus: true to put the cursor in the box when shown (default false); onEnter(text): called with the trimmed text when Enter is pressed
//   (a pending search is sent first); count: number of results, shown as "3 results" or "No results" when given (default none);
//   Escape clears the box when it holds text
function component(props) {
  css('sa-searchbox', `
.sa-searchbox{position:relative;display:flex;align-items:center;width:100%;min-width:0}
.sa-searchbox .sa-sb-icon{position:absolute;left:10px;top:50%;transform:translateY(-50%);width:16px;height:16px;color:var(--muted);pointer-events:none}
.sa-searchbox input{width:100%;min-width:0;margin:0;padding-left:34px;padding-right:34px;box-sizing:border-box}
.sa-searchbox.has-count input{padding-right:var(--pr,34px)}
.sa-searchbox input::-webkit-search-cancel-button,.sa-searchbox input::-webkit-search-decoration{-webkit-appearance:none;appearance:none}
.sa-searchbox .sa-sb-end{position:absolute;right:6px;top:50%;transform:translateY(-50%);display:flex;align-items:center;gap:4px}
.sa-searchbox .sa-sb-count{font-size:12px;color:var(--muted);white-space:nowrap;pointer-events:none}
.sa-searchbox .sa-sb-clear{border:0;background:none;box-shadow:none;margin:0;padding:0;width:24px;height:24px;min-width:24px;border-radius:50%;
  display:inline-flex;align-items:center;justify-content:center;color:var(--muted);cursor:pointer;font-size:16px;line-height:1}
.sa-searchbox .sa-sb-clear:hover,.sa-searchbox .sa-sb-clear:focus-visible{color:var(--fg);background:var(--line)}
.sa-searchbox .sa-sb-clear[hidden]{display:none}
`);
  const onSearch = typeof props.onSearch === 'function' ? props.onSearch : null;
  const onEnter = typeof props.onEnter === 'function' ? props.onEnter : null;
  const d = Number(props.delay);
  const delay = Number.isFinite(d) && d >= 0 ? d : 250;
  const placeholder = props.placeholder == null || props.placeholder === '' ? 'Search' : String(props.placeholder);
  const start = props.value == null ? '' : String(props.value);
  const hasCount = typeof props.count === 'number' && Number.isFinite(props.count);

  let timer = null;
  let last = start.trim();   // the text last reported to onSearch
  let composing = false;

  const send = (force) => {
    if (timer) { clearTimeout(timer); timer = null; }
    const text = input.value.trim();
    if (!force && text === last) return;
    last = text;
    if (onSearch) onSearch(text);
  };
  const schedule = () => {
    if (timer) clearTimeout(timer);
    timer = null;
    if (delay === 0) return send(false);
    timer = setTimeout(() => { timer = null; if (root.isConnected) send(false); }, delay);
  };
  const syncClear = () => { clear.hidden = input.value === ''; };

  const input = h('input', {
    type: 'search', value: start, placeholder, 'aria-label': placeholder,
    autocomplete: 'off', spellcheck: 'false', enterkeyhint: 'search',
    oninput: () => { syncClear(); if (!composing) schedule(); },
    oncompositionstart: () => { composing = true; },
    oncompositionend: () => { composing = false; schedule(); },
    onkeydown: (e) => {
      if (composing || e.isComposing) return;
      if (e.key === 'Enter') {
        e.preventDefault();
        send(false);
        if (onEnter) onEnter(input.value.trim());
      } else if (e.key === 'Escape' && input.value !== '') {
        e.preventDefault();
        e.stopPropagation();
        doClear();
      }
    }
  });

  const doClear = () => {
    input.value = '';
    syncClear();
    send(false);
    input.focus();
  };

  const clear = h('button', { type: 'button', class: 'sa-sb-clear', 'aria-label': 'Clear search', title: 'Clear', onclick: doClear }, '×');

  let countEl = null;
  if (hasCount) {
    const n = Math.max(0, Math.round(props.count));
    const text = n === 0 ? 'No results' : `${app.fmt.number(n)} result${n === 1 ? '' : 's'}`;
    countEl = h('span', { class: 'sa-sb-count', 'aria-live': 'polite' }, text);
  }

  const icon = svg('svg', { class: 'sa-sb-icon', viewBox: '0 0 24 24', fill: 'none', stroke: 'currentColor', 'stroke-width': '2.2', 'stroke-linecap': 'round', 'aria-hidden': 'true' },
    svg('circle', { cx: '11', cy: '11', r: '7' }),
    svg('line', { x1: '16.5', y1: '16.5', x2: '21', y2: '21' }));

  const root = h('div', { class: `sa-searchbox${hasCount ? ' has-count' : ''}`, role: 'search' },
    icon, input, h('span', { class: 'sa-sb-end' }, countEl, clear));

  syncClear();
  if (hasCount) {
    // leave room for the count and the clear button at the end of the box
    const len = countEl.textContent.length;
    root.style.setProperty('--pr', `${Math.round(len * 7 + 40)}px`);
  }
  if (props.autofocus) {
    setTimeout(() => { if (input.isConnected) { input.focus(); const l = input.value.length; try { input.setSelectionRange(l, l); } catch (e) {} } }, 0);
  }
  return root;
}
