// title: Flashcard
// description: A card that flips from a question to its answer with a tap, a click or the keyboard, then offers Again, Hard, Good and Easy to grade how well it was known; for vocabulary, exam revision, spaced repetition and quizzes.
// props: front: the question, a string or an element; back: the answer, a string or an element;
//   flipped: boolean, show the answer side to start with (default false); onFlip(flipped) after the card turns, flipped being the new side (true = answer);
//   onGrade(grade) with 'again' | 'hard' | 'good' | 'easy', which shows the four grade buttons once the answer is showing (Enter or Space flips, keys 1 to 4 grade);
//   frontLabel (default 'Question') and backLabel (default 'Answer'): the small caption on each side; minHeight in px (default 200)
function component(props) {
  css('sa-flashcard', `
.sa-flashcard{display:flex;flex-direction:column;gap:12px;width:100%;max-width:560px;margin:0 auto}
.sa-flashcard .sa-fc-card{perspective:1200px;cursor:pointer;outline:none;border-radius:var(--radius)}
.sa-flashcard .sa-fc-card:focus-visible{box-shadow:0 0 0 3px color-mix(in srgb,var(--accent) 45%,transparent)}
.sa-flashcard .sa-fc-inner{display:grid;transition:transform .45s ease;transform-style:preserve-3d}
.sa-flashcard.flipped .sa-fc-inner{transform:rotateY(180deg)}
.sa-flashcard .sa-fc-face{grid-area:1/1;display:flex;flex-direction:column;gap:8px;min-height:var(--h);box-sizing:border-box;padding:20px;
  background:var(--card);border:1px solid var(--line);border-radius:var(--radius);backface-visibility:hidden;-webkit-backface-visibility:hidden}
.sa-flashcard .sa-fc-back{transform:rotateY(180deg)}
.sa-flashcard .sa-fc-cap{font-size:.75rem;text-transform:uppercase;letter-spacing:.06em;color:var(--muted)}
.sa-flashcard .sa-fc-body{flex:1;display:flex;flex-direction:column;align-items:center;justify-content:center;text-align:center;
  font-size:1.25rem;line-height:1.4;white-space:pre-wrap;overflow-wrap:anywhere}
.sa-flashcard .sa-fc-actions{display:flex;gap:8px;justify-content:center;flex-wrap:wrap}
.sa-flashcard .sa-fc-grades{display:grid;grid-template-columns:repeat(4,1fr);gap:8px}
.sa-flashcard .sa-fc-grades button{min-width:0;padding-left:4px;padding-right:4px;border-color:var(--g);color:var(--g)}
.sa-flashcard .sa-fc-grades button:hover{background:color-mix(in srgb,var(--g) 12%,transparent)}
.sa-flashcard .sa-fc-grades small{display:block;color:var(--muted);font-size:.7rem}
@media (prefers-reduced-motion:reduce){.sa-flashcard .sa-fc-inner{transition:none}}
`);
  const GRADES = [
    { value: 'again', label: 'Again', color: 'var(--bad)' },
    { value: 'hard', label: 'Hard', color: 'var(--warn)' },
    { value: 'good', label: 'Good', color: 'var(--ok)' },
    { value: 'easy', label: 'Easy', color: 'var(--accent)' },
  ];
  const canGrade = typeof props.onGrade === 'function';
  const minHeight = Math.max(80, Number(props.minHeight) || 200);
  const frontLabel = props.frontLabel || 'Question';
  const backLabel = props.backLabel || 'Answer';
  let flipped = !!props.flipped;

  const content = (v) => (v instanceof Node ? v : v == null || v === '' ? h('span', { class: 'muted' }, '—') : String(v));
  const frontFace = h('div', { class: 'sa-fc-face sa-fc-front' },
    h('div', { class: 'sa-fc-cap' }, frontLabel), h('div', { class: 'sa-fc-body' }, content(props.front)));
  const backFace = h('div', { class: 'sa-fc-face sa-fc-back' },
    h('div', { class: 'sa-fc-cap' }, backLabel), h('div', { class: 'sa-fc-body' }, content(props.back)));
  const card = h('div', { class: 'sa-fc-card', role: 'button', tabindex: '0', onclick: () => flip() },
    h('div', { class: 'sa-fc-inner' }, frontFace, backFace));
  const actions = h('div', { 'aria-live': 'polite' });
  const root = h('div', { class: 'sa-flashcard', style: `--h:${minHeight}px` }, card, actions);

  function flip() {
    flipped = !flipped;
    draw();
    if (typeof props.onFlip === 'function') props.onFlip(flipped);
  }
  function grade(g) {
    if (canGrade && flipped) props.onGrade(g);
  }
  function draw() {
    root.classList.toggle('flipped', flipped);
    card.setAttribute('aria-label', flipped ? `${backLabel} showing. Show the ${frontLabel.toLowerCase()}` : `${frontLabel} showing. Show the ${backLabel.toLowerCase()}`);
    frontFace.setAttribute('aria-hidden', flipped ? 'true' : 'false');
    backFace.setAttribute('aria-hidden', flipped ? 'false' : 'true');
    frontFace.inert = flipped;
    backFace.inert = !flipped;
    if (flipped && canGrade) {
      actions.className = 'sa-fc-grades';
      actions.setAttribute('role', 'group');
      actions.setAttribute('aria-label', 'How well did you know it?');
      actions.replaceChildren(GRADES.map((g, i) => h('button', {
        type: 'button', style: `--g:${g.color}`, 'aria-keyshortcuts': String(i + 1),
        onclick: () => grade(g.value),
      }, g.label, h('small', null, String(i + 1)))));
    } else {
      actions.className = 'sa-fc-actions';
      actions.removeAttribute('role');
      actions.removeAttribute('aria-label');
      actions.replaceChildren(h('button', { type: 'button', class: flipped ? 'ghost' : 'primary', onclick: () => flip() },
        flipped ? `Show ${frontLabel.toLowerCase()}` : `Show ${backLabel.toLowerCase()}`));
    }
  }

  root.addEventListener('keydown', (e) => {
    if (e.altKey || e.ctrlKey || e.metaKey) return;
    const t = e.target;
    if (t !== card && t !== root && !(t instanceof HTMLButtonElement)) return;
    if ((e.key === 'Enter' || e.key === ' ') && t === card) { e.preventDefault(); flip(); return; }
    const n = Number(e.key);
    if (canGrade && flipped && n >= 1 && n <= 4) { e.preventDefault(); grade(GRADES[n - 1].value); }
  });

  draw();
  return root;
}
