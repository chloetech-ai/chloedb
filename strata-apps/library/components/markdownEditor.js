// title: Markdown editor
// description: A box to write Markdown with a toolbar for bold, italic, headings, lists, links and code, and a Preview tab that shows how it reads; for notes, journal entries, descriptions and posts.
// props: value: string, the text to start with (default ''); onChange(text): the text after the person stops typing for 300 ms (also sent at once on blur, on save and on preview);
//   onSave(text): called on Ctrl+S or Cmd+S (the shortcut is left to the browser when not given); rows: height of the box in lines (default 8);
//   placeholder (default 'Write in Markdown…'); label: accessible name of the box (default 'Markdown text')
function component(props) {
  css('sa-mdedit', `
.sa-mdedit{display:flex;flex-direction:column;border:1px solid var(--line);border-radius:var(--radius);background:var(--card);overflow:hidden;min-width:0}
.sa-mdedit:focus-within{border-color:var(--accent)}
.sa-mdedit [hidden]{display:none!important}
.sa-mdedit .bar{display:flex;flex-wrap:wrap;align-items:center;gap:4px;padding:4px 6px;border-bottom:1px solid var(--line)}
.sa-mdedit .tabs{display:flex;gap:2px}
.sa-mdedit .tabs button{border:0;background:none;box-shadow:none;margin:0;padding:5px 10px;border-radius:calc(var(--radius) - 2px);color:var(--muted);font:inherit;font-size:.9em;cursor:pointer}
.sa-mdedit .tabs button[aria-selected=true]{color:var(--fg);background:color-mix(in srgb,var(--accent) 14%,transparent)}
.sa-mdedit .tools{display:flex;flex-wrap:wrap;gap:2px;margin-left:auto}
.sa-mdedit .tools button{border:0;background:none;box-shadow:none;margin:0;padding:0;min-width:32px;height:32px;display:inline-flex;align-items:center;justify-content:center;border-radius:calc(var(--radius) - 2px);color:var(--fg);font:inherit;font-size:15px;line-height:1;cursor:pointer}
.sa-mdedit .tools button:hover,.sa-mdedit .tools button:focus-visible{background:color-mix(in srgb,var(--fg) 8%,transparent)}
.sa-mdedit textarea{display:block;width:100%;box-sizing:border-box;border:0;border-radius:0;margin:0;padding:10px 12px;background:transparent;color:var(--fg);font:inherit;line-height:1.5;resize:vertical;box-shadow:none;outline:none}
.sa-mdedit .pv{padding:10px 12px;overflow:auto;overflow-wrap:anywhere}
.sa-mdedit .pv .none{color:var(--muted);margin:0}
`);
  const rows = Math.max(2, Math.min(40, Math.round(Number(props.rows)) || 8));
  const onChange = typeof props.onChange === 'function' ? props.onChange : null;
  const onSave = typeof props.onSave === 'function' ? props.onSave : null;
  const uid = 'sa-md-' + Math.random().toString(36).slice(2, 9);
  let sent = props.value == null ? '' : String(props.value);
  let timer = null;

  function flush() {
    clearTimeout(timer); timer = null;
    if (onChange && ta.value !== sent) { sent = ta.value; onChange(sent); }
  }
  function schedule() {
    if (!onChange) return;
    clearTimeout(timer);
    timer = setTimeout(flush, 300);
  }

  const ta = h('textarea', {
    rows, value: sent, spellcheck: 'true',
    placeholder: props.placeholder == null ? 'Write in Markdown…' : props.placeholder,
    'aria-label': props.label || 'Markdown text',
    oninput: schedule, onblur: flush,
  });

  // Replace start..end with text, keeping the browser's undo when it can, then select s0..s1.
  function edit(start, end, text, s0, s1) {
    const v = ta.value;
    const want = v.slice(0, start) + text + v.slice(end);
    ta.focus();
    ta.setSelectionRange(start, end);
    try {
      if (text) document.execCommand('insertText', false, text);
      else if (start < end) document.execCommand('delete');
    } catch (e) { /* fall back below */ }
    if (ta.value !== want) ta.value = want;
    ta.setSelectionRange(s0, s1);
    schedule();
  }

  function trimmed() {
    const v = ta.value;
    let s = ta.selectionStart, e = ta.selectionEnd;
    while (s < e && /\s/.test(v[s])) s++;
    while (e > s && /\s/.test(v[e - 1])) e--;
    return { v, s, e, sel: v.slice(s, e) };
  }

  function inline(mark, word) {
    const { v, s, e, sel } = trimmed();
    const m = mark.length;
    if (v.slice(s - m, s) === mark && v.slice(e, e + m) === mark && s >= m) {
      return edit(s - m, e + m, sel, s - m, e - m);
    }
    if (sel.length >= 2 * m && sel.startsWith(mark) && sel.endsWith(mark)) {
      const inner = sel.slice(m, sel.length - m);
      return edit(s, e, inner, s, s + inner.length);
    }
    const body = sel || word;
    edit(s, e, mark + body + mark, s + m, s + m + body.length);
  }

  function code() {
    const { v, s, e, sel } = trimmed();
    if (sel.includes('\n')) {
      const before = s > 0 && v[s - 1] !== '\n' ? '\n' : '';
      const after = e < v.length && v[e] !== '\n' ? '\n' : '';
      const open = before + '```\n';
      return edit(s, e, open + sel + '\n```' + after, s + open.length, s + open.length + sel.length);
    }
    if (sel.includes('`') && !(sel.startsWith('`') && sel.endsWith('`'))) {
      return edit(s, e, '`` ' + sel + ' ``', s + 3, s + 3 + sel.length);
    }
    inline('`', 'code');
  }

  function lines(kind) {
    const v = ta.value;
    const s0 = ta.selectionStart, e0 = ta.selectionEnd;
    const a = v.lastIndexOf('\n', s0 - 1) + 1;
    const endPos = e0 > s0 && v[e0 - 1] === '\n' ? e0 - 1 : e0;
    let b = v.indexOf('\n', endPos);
    if (b < 0) b = v.length;
    const ls = v.slice(a, b).split('\n');
    const filled = ls.filter((l) => l.trim());
    const keep = (l) => !l.trim() && ls.length > 1;
    let out;
    if (kind === 'heading') {
      const all = filled.length > 0 && filled.every((l) => /^##\s/.test(l));
      out = ls.map((l) => keep(l) ? l : all ? l.replace(/^##\s+/, '') : '## ' + l.replace(/^#{1,6}\s+/, ''));
    } else {
      const re = /^(\s*)[-*+]\s+/;
      const all = filled.length > 0 && filled.every((l) => re.test(l));
      out = ls.map((l) => keep(l) ? l : all ? l.replace(re, '$1') : l.replace(/^(\s*)(?:\d+[.)]\s+)?/, '$1- '));
    }
    const text = out.join('\n');
    if (s0 === e0 && ls.length === 1) {
      const p = Math.max(a, Math.min(a + text.length, s0 + text.length - ls[0].length));
      edit(a, b, text, p, p);
    } else {
      edit(a, b, text, a, a + text.length);
    }
  }

  function link() {
    const { s, e, sel } = trimmed();
    if (!sel) return edit(s, e, '[link text](https://)', s + 1, s + 10);
    if (/^(https?:\/\/|mailto:|www\.)\S+$/i.test(sel)) {
      const url = /^www\./i.test(sel) ? 'https://' + sel : sel;
      return edit(s, e, '[link text](' + url + ')', s + 1, s + 10);
    }
    const at = s + sel.length + 3;
    edit(s, e, '[' + sel + '](https://)', at, at + 8);
  }

  const ico = (d) => svg('svg', { viewBox: '0 0 24 24', width: 16, height: 16, fill: 'none', stroke: 'currentColor', 'stroke-width': 2, 'stroke-linecap': 'round', 'stroke-linejoin': 'round', 'aria-hidden': 'true' }, svg('path', { d }));
  const tool = (label, content, run) => h('button', {
    type: 'button', title: label, 'aria-label': label,
    onmousedown: (e) => e.preventDefault(),
    onclick: run,
  }, content);

  const tools = h('div', { class: 'tools', role: 'toolbar', 'aria-label': 'Formatting', 'aria-controls': uid + '-w' },
    tool('Bold', h('b', { style: 'font-weight:700' }, 'B'), () => inline('**', 'bold text')),
    tool('Italic', h('i', { style: 'font-family:serif;font-size:17px' }, 'I'), () => inline('_', 'italic text')),
    tool('Heading', h('span', { style: 'font-weight:700' }, 'H'), () => lines('heading')),
    tool('Bulleted list', ico('M9 6h11M9 12h11M9 18h11M4 6h.01M4 12h.01M4 18h.01'), () => lines('list')),
    tool('Link', ico('M10 14a4 4 0 0 0 5.7 0l3-3a4 4 0 0 0-5.7-5.7l-1 1M14 10a4 4 0 0 0-5.7 0l-3 3a4 4 0 0 0 5.7 5.7l1-1'), link),
    tool('Code', ico('M8 7l-5 5 5 5M16 7l5 5-5 5'), code),
  );

  const tabW = h('button', { type: 'button', role: 'tab', id: uid + '-tw', 'aria-controls': uid + '-w', onclick: () => show('write') }, 'Write');
  const tabP = h('button', { type: 'button', role: 'tab', id: uid + '-tp', 'aria-controls': uid + '-p', onclick: () => show('preview') }, 'Preview');
  const tabs = h('div', {
    class: 'tabs', role: 'tablist', 'aria-label': 'Editor mode',
    onkeydown: (e) => {
      if (e.key !== 'ArrowLeft' && e.key !== 'ArrowRight') return;
      e.preventDefault();
      const next = mode === 'write' ? 'preview' : 'write';
      show(next, true);
      (next === 'write' ? tabW : tabP).focus();
    },
  }, tabW, tabP);

  const write = h('div', { role: 'tabpanel', id: uid + '-w', 'aria-labelledby': uid + '-tw' }, ta);
  const pv = h('div', { class: 'pv', role: 'tabpanel', id: uid + '-p', 'aria-labelledby': uid + '-tp', tabindex: '0', style: `min-height:${(rows * 1.5).toFixed(2)}em` });

  let mode = 'write';
  function show(m, keepFocus) {
    mode = m;
    const w = m === 'write';
    tabW.setAttribute('aria-selected', w ? 'true' : 'false');
    tabP.setAttribute('aria-selected', w ? 'false' : 'true');
    tabW.tabIndex = w ? 0 : -1;
    tabP.tabIndex = w ? -1 : 0;
    write.hidden = !w;
    pv.hidden = w;
    tools.hidden = !w;
    if (w) {
      pv.replaceChildren();
      if (!keepFocus) ta.focus();
    } else {
      flush();
      pv.replaceChildren(ta.value.trim() ? ui.markdown({ text: ta.value }) : h('p', { class: 'none' }, 'Nothing to preview'));
    }
  }

  const root = h('div', {
    class: 'sa-mdedit',
    onkeydown: (e) => {
      if (!onSave || e.isComposing || e.altKey || !(e.ctrlKey || e.metaKey) || String(e.key).toLowerCase() !== 's') return;
      e.preventDefault();
      flush();
      onSave(ta.value);
    },
  }, h('div', { class: 'bar' }, tabs, tools), write, pv);

  show('write', true);
  return root;
}
