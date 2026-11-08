// title: Chat thread
// description: A conversation as bubbles, grouped by day and by run of one author, with a box to write the next message (Enter sends, Shift+Enter adds a line), kept scrolled to the latest; for team chats, support threads, messages between a client and a coach.
// props: messages: [{id, author (a person id or any key), name? (shown above a run of someone else's bubbles, default the author), text, at (ISO time)}] in any order (sorted by at);
//   me: the author whose bubbles go on the right; onSend(text) when the person sends a message, the text trimmed (may return a Promise: the box clears once it resolves, and an error keeps the text and is shown);
//   without onSend there is no box (read-only); placeholder: text of the box (default 'Write a message'); typing: [names] of people writing now (default []);
//   height: px or any CSS size of the whole thread (default 420); empty: text when there are no messages (default 'No messages yet'); label: accessible name (default 'Messages').
//   The element's update({messages, typing}) redraws the messages without touching what is being typed; it stays at the latest unless the person scrolled up to read.
function component(props) {
  css('sa-chat', `
.sa-chat{display:flex;flex-direction:column;height:var(--h);min-height:200px;border:1px solid var(--line);border-radius:var(--radius);background:var(--card);overflow:hidden}
.sa-chat-log{flex:1;overflow-y:auto;padding:10px 12px;display:flex;flex-direction:column;gap:2px;overscroll-behavior:contain}
.sa-chat-log:focus{outline:none}
.sa-chat-day{align-self:center;margin:10px 0 6px;font-size:12px;color:var(--muted);background:var(--bg);border:1px solid var(--line);border-radius:999px;padding:2px 10px}
.sa-chat-run{display:flex;flex-direction:column;gap:2px;margin:4px 0;max-width:80%;align-items:flex-start}
.sa-chat-run.mine{align-self:flex-end;align-items:flex-end}
.sa-chat-name{font-size:12px;color:var(--muted);margin:0 0 1px 10px}
.sa-chat-bubble{padding:7px 11px;border-radius:16px;background:var(--bg);border:1px solid var(--line);color:var(--fg);white-space:pre-wrap;overflow-wrap:anywhere;line-height:1.35;max-width:100%}
.sa-chat-run.mine .sa-chat-bubble{background:var(--accent);border-color:var(--accent);color:var(--accent-fg,#fff)}
.sa-chat-run:not(.mine) .sa-chat-bubble:not(:last-of-type){border-bottom-left-radius:5px}
.sa-chat-run.mine .sa-chat-bubble:not(:last-of-type){border-bottom-right-radius:5px}
.sa-chat-time{font-size:11px;color:var(--muted);margin:1px 8px 0}
.sa-chat-empty{margin:auto;color:var(--muted);text-align:center;padding:20px}
.sa-chat-typing{min-height:18px;padding:0 14px 2px;font-size:12px;color:var(--muted);font-style:italic}
.sa-chat-box{display:flex;gap:8px;align-items:flex-end;padding:8px;border-top:1px solid var(--line);margin:0;flex-wrap:nowrap}
.sa-chat-box textarea{flex:1;min-width:0;resize:none;margin:0;max-height:140px;line-height:1.35}
.sa-chat-box button{margin:0;flex:none}
.sa-chat-err{color:var(--bad);font-size:12px;padding:0 12px 6px}
`);
  const me = props.me;
  const hgt = props.height == null ? '420px' : typeof props.height === 'number' ? props.height + 'px' : String(props.height);
  const root = h('div', { class: 'sa-chat', style: `--h:${hgt}` });
  const log = h('div', { class: 'sa-chat-log', role: 'log', 'aria-live': 'polite', 'aria-label': props.label || 'Messages', tabindex: '0' });
  const typingEl = h('div', { class: 'sa-chat-typing', 'aria-live': 'polite' });
  root.append(log, typingEl);

  const time = (iso) => { const t = Date.parse(iso); return isNaN(t) ? null : t; };
  const dayOf = (iso) => (time(iso) == null ? '' : lib.dates.fromTime(iso));
  const dayLabel = (day, iso) => {
    const today = app.today();
    if (day === today) return 'Today';
    if (day === lib.dates.addDays(today, -1)) return 'Yesterday';
    return app.fmt.date(iso);
  };
  const typingText = (names) => {
    const n = (names || []).filter(Boolean);
    if (!n.length) return '';
    if (n.length === 1) return `${n[0]} is typing…`;
    if (n.length === 2) return `${n[0]} and ${n[1]} are typing…`;
    if (n.length === 3) return `${n[0]}, ${n[1]} and ${n[2]} are typing…`;
    return `${n[0]}, ${n[1]} and ${n.length - 2} others are typing…`;
  };
  const atBottom = () => log.scrollHeight - log.scrollTop - log.clientHeight < 60;
  const toBottom = () => { log.scrollTop = log.scrollHeight; };
  const toBottomSoon = (n = 0) => {
    if (root.isConnected) { toBottom(); requestAnimationFrame(toBottom); }
    else if (n < 30) requestAnimationFrame(() => toBottomSoon(n + 1));
  };

  let lastKey = null;
  function draw(messages, typing) {
    const stick = !root.isConnected || atBottom();
    const list = (Array.isArray(messages) ? messages : [])
      .filter((m) => m && m.text != null && String(m.text) !== '')
      .map((m, i) => ({ m, i, t: time(m.at) }))
      .sort((a, b) => (a.t == null || b.t == null || a.t === b.t ? a.i - b.i : a.t - b.t))
      .map((x) => x.m);
    const nodes = [];
    let day = null, run = null, prev = null;
    const closeRun = () => {
      if (run && prev && time(prev.at) != null) run.append(h('div', { class: 'sa-chat-time' }, app.fmt.time(prev.at)));
    };
    for (const m of list) {
      const d = dayOf(m.at);
      if (d && d !== day) { closeRun(); run = null; day = d; nodes.push(h('div', { class: 'sa-chat-day', role: 'separator' }, dayLabel(d, m.at))); }
      const gap = prev && time(prev.at) != null && time(m.at) != null ? time(m.at) - time(prev.at) : 0;
      if (!run || prev.author !== m.author || gap > 10 * 60 * 1000) {
        closeRun();
        const mine = me != null && m.author === me;
        run = h('div', { class: `sa-chat-run${mine ? ' mine' : ''}` });
        if (!mine) run.append(h('div', { class: 'sa-chat-name' }, m.name || String(m.author ?? 'Someone')));
        nodes.push(run);
      }
      run.append(h('div', { class: 'sa-chat-bubble', title: time(m.at) == null ? null : app.fmt.datetime(m.at) }, String(m.text)));
      prev = m;
    }
    closeRun();
    log.replaceChildren(nodes.length ? nodes : h('div', { class: 'sa-chat-empty' }, props.empty || 'No messages yet'));
    typingEl.textContent = typingText(typing);
    const last = list[list.length - 1];
    const key = last ? (last.id ?? `${last.author}|${last.at}|${list.length}`) : null;
    const ownNew = last && key !== lastKey && me != null && last.author === me;
    lastKey = key;
    if (stick || ownNew) toBottomSoon();
  }

  if (typeof props.onSend === 'function') {
    const ta = h('textarea', { rows: 1, placeholder: props.placeholder || 'Write a message', 'aria-label': props.placeholder || 'Write a message' });
    const btn = h('button', { type: 'submit', class: 'primary', disabled: true }, 'Send');
    const err = h('div', { class: 'sa-chat-err', role: 'alert', hidden: true });
    let busy = false;
    const resize = () => { ta.style.height = 'auto'; ta.style.height = Math.min(ta.scrollHeight + 2, 140) + 'px'; };
    const sync = () => { btn.disabled = busy || !ta.value.trim(); };
    const send = async () => {
      const text = ta.value.trim();
      if (!text || busy) return;
      busy = true; ta.readOnly = true; sync(); err.hidden = true;
      try {
        await props.onSend(text);
        ta.value = '';
        resize();
      } catch (e) {
        err.textContent = (e && e.message) || 'The message could not be sent.';
        err.hidden = false;
      } finally {
        busy = false; ta.readOnly = false; sync(); ta.focus();
      }
    };
    ta.addEventListener('input', () => { resize(); sync(); });
    ta.addEventListener('keydown', (e) => {
      if (e.key === 'Enter' && !e.shiftKey && !e.isComposing && e.keyCode !== 229) { e.preventDefault(); send(); }
    });
    root.append(err, h('form', { class: 'sa-chat-box', onsubmit: (e) => { e.preventDefault(); send(); } }, ta, btn));
  }

  draw(props.messages, props.typing);
  root.update = (p = {}) => draw(p.messages !== undefined ? p.messages : props.messages, p.typing !== undefined ? p.typing : props.typing);
  return root;
}
