// title: Quiz question
// description: One question with its choices: the person picks an answer (or ticks several and checks them) and sees right or wrong, the correct choice and an explanation; for quizzes, flashcard tests, lessons and practice drills.
// props: question: text of the question; choices: [strings or {text, correct?: bool}] (default []);
//   answer: index of the correct choice, or [indices] with multiple, used when no choice carries correct;
//   explanation: text shown once answered (optional); code: a code sample shown above the choices, monospace (optional);
//   onAnswer(index, correct) when a choice is picked, or onAnswer([indices], correct) with multiple once "Check" is pressed;
//   correct is true or false, or null when the right answer is not known;
//   picked: the index (or [indices] with multiple) already given, which shows the question as answered and locked;
//   multiple: true to let the person choose several before checking (default false); checkLabel (default "Check")
function component(props) {
  css('sa-quiz', `
.sa-quiz{display:flex;flex-direction:column;gap:12px;min-width:0}
.sa-quiz .q{margin:0;font-weight:600;font-size:1.05em;line-height:1.4;white-space:pre-wrap;overflow-wrap:anywhere}
.sa-quiz pre{margin:0;padding:10px 12px;background:color-mix(in srgb,var(--fg) 6%,transparent);border:1px solid var(--line);border-radius:var(--radius);overflow-x:auto;font-size:.88em;line-height:1.45;tab-size:2}
.sa-quiz pre code{background:none;padding:0;border:0;white-space:pre}
.sa-quiz .opts{display:flex;flex-direction:column;gap:8px}
.sa-quiz .opt{display:flex;align-items:flex-start;gap:10px;width:100%;margin:0;text-align:left;padding:10px 12px;border:1px solid var(--line);border-radius:var(--radius);background:var(--card);color:var(--fg);font:inherit;line-height:1.4;box-shadow:none;cursor:pointer;white-space:normal}
.sa-quiz:not(.done) .opt:hover{border-color:var(--accent)}
.sa-quiz.done .opt{cursor:default}
.sa-quiz .k{flex:none;display:inline-flex;align-items:center;justify-content:center;min-width:1.7em;height:1.7em;padding:0 4px;border-radius:999px;border:1px solid var(--line);font-size:.85em;font-weight:600;color:var(--muted)}
.sa-quiz.multi .k{border-radius:4px}
.sa-quiz .t{flex:1;min-width:0;overflow-wrap:anywhere;white-space:pre-wrap}
.sa-quiz .m{flex:none;font-weight:700;min-width:1em;text-align:right}
.sa-quiz .sel{border-color:var(--accent);background:color-mix(in srgb,var(--accent) 10%,var(--card))}
.sa-quiz .sel .k{background:var(--accent);border-color:var(--accent);color:var(--card)}
.sa-quiz .right{border-color:var(--ok);background:color-mix(in srgb,var(--ok) 12%,var(--card))}
.sa-quiz .right .k,.sa-quiz .right .m{color:var(--ok);border-color:var(--ok)}
.sa-quiz .missed{border-color:var(--ok);border-style:dashed}
.sa-quiz .missed .m{color:var(--ok)}
.sa-quiz .wrong{border-color:var(--bad);background:color-mix(in srgb,var(--bad) 12%,var(--card))}
.sa-quiz .wrong .k,.sa-quiz .wrong .m{color:var(--bad);border-color:var(--bad)}
.sa-quiz .dim{opacity:.6}
.sa-quiz .bar{display:flex;align-items:center;gap:10px;flex-wrap:wrap}
.sa-quiz .bar .hint{flex:1;color:var(--muted);font-size:.9em}
.sa-quiz .fb{padding:10px 12px;border-radius:var(--radius);border-left:3px solid var(--muted);background:color-mix(in srgb,var(--fg) 5%,transparent)}
.sa-quiz .fb.ok{border-left-color:var(--ok)}
.sa-quiz .fb.bad{border-left-color:var(--bad)}
.sa-quiz .fb strong{display:block}
.sa-quiz .fb.ok strong{color:var(--ok)}
.sa-quiz .fb.bad strong{color:var(--bad)}
.sa-quiz .fb p{margin:4px 0 0;white-space:pre-wrap;overflow-wrap:anywhere}
`);
  const choices = (Array.isArray(props.choices) ? props.choices : []).map(c =>
    c && typeof c === 'object' ? { text: String(c.text ?? ''), correct: c.correct === true } : { text: String(c ?? ''), correct: false });
  const n = choices.length;
  const multiple = !!props.multiple;
  const norm = v => [...new Set([].concat(v ?? []).map(Number).filter(i => Number.isInteger(i) && i >= 0 && i < n))].sort((a, b) => a - b);

  const key = new Set();
  if (choices.some(c => c.correct)) choices.forEach((c, i) => { if (c.correct) key.add(i); });
  else norm(props.answer).forEach(i => key.add(i));
  const known = key.size > 0;

  let answered = norm(props.picked);
  if (!multiple) answered = answered.slice(0, 1);
  if (!answered.length) answered = null;
  const sel = new Set(answered || []);

  const isRight = list => !known ? null
    : multiple ? list.length === key.size && list.every(i => key.has(i))
    : key.has(list[0]);
  const letter = i => i < 26 ? String.fromCharCode(65 + i) : String(i + 1);
  const report = (value, ok) => {
    if (typeof props.onAnswer !== 'function') return;
    try { const r = props.onAnswer(value, ok); if (r && r.catch) r.catch(() => {}); } catch (e) { /* the view handles its own errors */ }
  };

  const root = h('div', { class: 'sa-quiz' + (multiple ? ' multi' : '') });

  function pick(i) {
    if (answered) return;
    if (multiple) {
      sel.has(i) ? sel.delete(i) : sel.add(i);
      draw(i);
      return;
    }
    answered = [i];
    draw(i);
    report(i, isRight(answered));
  }

  function check() {
    if (answered || !sel.size) return;
    answered = [...sel].sort((a, b) => a - b);
    draw();
    report(answered.slice(), isRight(answered));
  }

  function draw(focusIndex) {
    const done = !!answered;
    root.classList.toggle('done', done);
    const verdict = done ? isRight(answered) : null;

    const buttons = choices.map((c, i) => {
      let cls = 'opt', mark = '';
      const chosen = done ? answered.includes(i) : sel.has(i);
      if (done) {
        if (known && key.has(i)) { cls += chosen ? ' right' : ' missed'; mark = '✓'; }
        else if (chosen) { cls += known ? ' wrong' : ' sel'; mark = known ? '✗' : ''; }
        else cls += ' dim';
      } else if (chosen) cls += ' sel';
      const state = done && known ? (key.has(i) ? ', correct answer' : chosen ? ', wrong' : '') : '';
      return h('button', {
        type: 'button', class: cls,
        'aria-pressed': chosen ? 'true' : 'false',
        'aria-disabled': done ? 'true' : null,
        'aria-label': `${letter(i)}: ${c.text}${state}`,
        onclick: () => pick(i)
      }, h('span', { class: 'k', 'aria-hidden': 'true' }, letter(i)), h('span', { class: 't' }, c.text), h('span', { class: 'm', 'aria-hidden': 'true' }, mark));
    });

    let feedback = null;
    if (done) {
      const tone = verdict === true ? ' ok' : verdict === false ? ' bad' : '';
      const title = verdict === true ? 'Correct' : verdict === false ? 'Not quite' : 'Answer saved';
      feedback = h('div', { class: 'fb' + tone },
        h('strong', null, title),
        props.explanation ? h('p', null, String(props.explanation)) : null);
    }

    root.replaceChildren(
      props.question ? h('p', { class: 'q', id: null }, String(props.question)) : null,
      props.code ? h('pre', { class: 'mono' }, h('code', null, String(props.code))) : null,
      n ? h('div', { class: 'opts', role: 'group', 'aria-label': props.question ? String(props.question) : 'Choices' }, buttons)
        : h('div', { class: 'empty' }, 'No choices to pick from'),
      multiple && !done && n ? h('div', { class: 'bar' },
        h('span', { class: 'hint' }, sel.size ? `${sel.size} chosen` : 'Choose all that apply'),
        h('button', { type: 'button', class: 'primary', disabled: !sel.size, onclick: check }, props.checkLabel || 'Check')) : null,
      h('div', { 'aria-live': 'polite' }, feedback)
    );
    if (focusIndex != null && buttons[focusIndex]) buttons[focusIndex].focus();
  }

  draw();
  return root;
}
