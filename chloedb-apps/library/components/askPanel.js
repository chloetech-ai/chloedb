// title: Ask panel
// description: A panel where the model reads the app's rows and answers one standing question about them (a read of this month's spending, what to work on next, how a set of things fits together): a title, a line saying what is sent and to whom, an optional note the person adds, a button, the answer with when it was asked, and Ask again. An answer already given for the same rows shows when the page opens again, at no cost. When the app's model is switched off it says so plainly instead of failing.
// props: title (default 'Ask'); note: a line under the title (default: what is sent and that a model answers);
//   prompt: the question, in full (what to answer, how long, in what shape); rows: the rows it is about (sent with it, as JSON, the first 200);
//   button: its words (default 'Ask'); input: a label for an optional text the person adds (their goals, a question), sent with it;
//   inputPlaceholder; web: true to let the model search the web (slower); empty: words when there are no rows (default 'Nothing to read yet.')
function component(props) {
  css('sa-ask', `
.sa-ask{display:flex;flex-direction:column;gap:10px;background:var(--card);border:1px solid var(--card-border,var(--line));border-top:3px solid var(--accent);border-radius:var(--radius);padding:16px;box-shadow:var(--card-shadow,none)}
.sa-ask h3{margin:0;font-family:var(--font-title);font-size:1.15em}
.sa-ask .sa-ask-note{margin:0;color:var(--muted);font-size:.92em;line-height:1.45}
.sa-ask label{display:flex;flex-direction:column;gap:6px;font-weight:600;font-size:.92em}
.sa-ask textarea{min-height:72px;resize:vertical;font-weight:400}
.sa-ask button.primary{width:100%;min-height:44px;border-radius:999px}
.sa-ask .sa-ask-answer{line-height:1.55;white-space:normal}
.sa-ask .sa-ask-answer p{margin:.4em 0}
.sa-ask .sa-ask-meta{display:flex;align-items:center;justify-content:space-between;gap:8px;color:var(--muted);font-size:.85em}
.sa-ask .sa-ask-off{background:color-mix(in srgb,var(--warn) 12%,transparent);border-radius:calc(var(--radius) - 4px);padding:10px 12px;font-size:.92em;line-height:1.45}
.sa-ask .sa-ask-wait{display:flex;align-items:center;gap:8px;color:var(--muted)}
.sa-ask .sa-ask-wait::before{content:'';width:14px;height:14px;border:2px solid currentColor;border-right-color:transparent;border-radius:50%;animation:sa-ask-spin .8s linear infinite}
@keyframes sa-ask-spin{to{transform:rotate(360deg)}}
`);
  const rows = Array.isArray(props.rows) ? props.rows.slice(0, 200) : [];
  const title = props.title || 'Ask';
  const prompt = String(props.prompt || 'Read these rows and say in a few short paragraphs what stands out and what to do about it.');
  let extra = '';
  const question = () => `${prompt}${props.input && extra.trim() ? `\n\n${props.input}: ${extra.trim()}` : ''}\n\nThe rows, as JSON:\n${JSON.stringify(rows).slice(0, 12000)}`;
  const body = h('div', { class: 'col' });
  const field = props.input ? h('textarea', { placeholder: props.inputPlaceholder || '', oninput: (e) => { extra = e.target.value; } }) : null;
  const btn = h('button', { type: 'button', class: 'primary', onclick: () => go(false) }, props.button || 'Ask');
  const show = (got) => {
    const text = String(got.answer || '');
    const ans = typeof ui !== 'undefined' && ui.markdown ? ui.markdown({ text }) : h('div', { class: 'sa-ask-answer' }, text);
    body.replaceChildren(h('div', { class: 'sa-ask-answer' }, ans),
      h('div', { class: 'sa-ask-meta' }, h('span', null, got.at ? `Asked ${app.fmt.ago(got.at)}` : 'Asked earlier'), h('button', { type: 'button', class: 'ghost small', onclick: () => go(true) }, 'Ask again')));
  };
  const off = (msg) => body.replaceChildren(h('div', { class: 'sa-ask-off', role: 'status' }, msg));
  async function go(again) {
    if (!rows.length) { body.replaceChildren(h('p', { class: 'sa-ask-note' }, props.empty || 'Nothing to read yet.')); return; }
    const q = question();
    btn.disabled = true;
    body.replaceChildren(h('div', { class: 'sa-ask-wait', role: 'status' }, props.web ? 'Reading and searching the web…' : 'Thinking…'));
    try {
      const answer = await app.ask(q, { ...(props.web ? { web: true } : {}), ...(again ? { fresh: true } : {}) });
      show({ answer, at: new Date().toISOString() });
    } catch (e) {
      const m = String((e && e.message) || e);
      if (/switched off/i.test(m)) off(`The model is switched off for this app, so it cannot answer yet. Its owner can switch on "A model"${props.web ? ' and "Web search"' : ''} under Model and web in the app's settings.`);
      else off(`It could not answer just now: ${m}`);
    } finally { btn.disabled = false; }
  }
  // what was answered for these very rows before: the engine keeps each
  // answer, and gives it back at no cost without asking anew
  if (rows.length) Promise.resolve(app.ask(question(), { kept: true, ...(props.web ? { web: true } : {}) })).then((answer) => { if (answer) show({ answer, at: null }); }).catch(() => {});
  return h('section', { class: 'sa-ask', 'aria-label': title },
    h('h3', null, title),
    h('p', { class: 'sa-ask-note' }, props.note || 'A model reads what is listed here and answers; it is sent to the model the app\'s owner switched on.'),
    field ? h('label', null, props.input, field) : null,
    btn, body);
}
