// title: Spaced repetition
// description: When to review a card next by the SM-2 method (flashcards, vocabulary, facts to learn), from how well the person remembered it, plus which cards are due today.
// api: review(card {ease = 2.5, interval = 0, reps = 0, due}, grade, today = the local date) -> the card with new {ease, interval, reps, due}
//   (other fields kept), or null for an unknown grade; grade is 'again' | 'hard' | 'good' | 'easy' (SM-2 quality 1, 3, 4, 5) or a quality 0..5;
//   'again' (quality below 3) starts the card over: reps 0, interval 1 day, ease unchanged; otherwise reps + 1 and the interval is
//   1 day, then 6 days, then the last interval times ease (rounded, at least 1); ease then moves by 0.1 - (5 - q) * (0.08 + (5 - q) * 0.02),
//   never below 1.3 (easy +0.1, good 0, hard -0.14); due is today plus interval, as 'YYYY-MM-DD';
//   preview(card) -> {again, hard, good, easy}: the interval in days each grade would give (for labels on the buttons);
//   isDue(card, today = the local date) -> true when the card has no due day yet or it is today or earlier;
//   dueCards(cards, today = the local date) -> the cards due, most overdue first, new cards (no due) last
function helper() {
  const DAY = 86400000;
  const MIN_EASE = 1.3;
  const GRADES = { again: 1, hard: 3, good: 4, easy: 5 };
  const pad = (n) => String(n).padStart(2, '0');
  const isDay = (d) => typeof d === 'string' && /^\d{4}-\d{2}-\d{2}/.test(d);
  const toUTC = (d) => Date.UTC(Number(d.slice(0, 4)), Number(d.slice(5, 7)) - 1, Number(d.slice(8, 10)));
  const fromUTC = (ms) => { const d = new Date(ms); return `${d.getUTCFullYear()}-${pad(d.getUTCMonth() + 1)}-${pad(d.getUTCDate())}`; };
  const localToday = () => { const d = new Date(); return `${d.getFullYear()}-${pad(d.getMonth() + 1)}-${pad(d.getDate())}`; };
  const dayOf = (today) => (isDay(today) ? today.slice(0, 10) : localToday());

  function quality(grade) {
    if (typeof grade === 'number') return Number.isFinite(grade) && grade >= 0 && grade <= 5 ? Math.round(grade) : null;
    if (typeof grade !== 'string') return null;
    const q = GRADES[grade.trim().toLowerCase()];
    return q === undefined ? null : q;
  }
  function state(card) {
    const c = card && typeof card === 'object' ? card : {};
    const e = Number(c.ease);
    const i = Number(c.interval);
    const r = Number(c.reps);
    return {
      ease: Number.isFinite(e) && e > 0 ? Math.max(MIN_EASE, e) : 2.5,
      interval: Number.isFinite(i) && i > 0 ? i : 0,
      reps: Number.isFinite(r) && r > 0 ? Math.floor(r) : 0,
    };
  }
  // the SM-2 step: {ease, interval, reps} after a review of quality q
  function step(s, q) {
    if (q < 3) return { ease: s.ease, interval: 1, reps: 0 };
    const reps = s.reps + 1;
    const interval = reps === 1 ? 1 : reps === 2 ? 6 : Math.max(1, Math.round(s.interval * s.ease));
    const d = 5 - q;
    const ease = Math.max(MIN_EASE, Math.round((s.ease + 0.1 - d * (0.08 + d * 0.02)) * 100) / 100);
    return { ease, interval, reps };
  }

  function review(card, grade, today) {
    const q = quality(grade);
    if (q === null) return null;
    const next = step(state(card), q);
    const base = card && typeof card === 'object' ? card : {};
    return { ...base, ...next, due: fromUTC(toUTC(dayOf(today)) + next.interval * DAY) };
  }
  function preview(card) {
    const s = state(card);
    const out = {};
    for (const g of Object.keys(GRADES)) out[g] = step(s, GRADES[g]).interval;
    return out;
  }
  function isDue(card, today) {
    if (!card || typeof card !== 'object') return false;
    if (!isDay(card.due)) return true;
    return card.due.slice(0, 10) <= dayOf(today);
  }
  function dueCards(cards, today) {
    if (!Array.isArray(cards)) return [];
    const t = dayOf(today);
    return cards
      .filter((c) => isDue(c, t))
      .map((c, i) => ({ c, i, d: isDay(c.due) ? c.due.slice(0, 10) : null }))
      .sort((a, b) => {
        if (a.d === b.d) return a.i - b.i;
        if (a.d === null) return 1;
        if (b.d === null) return -1;
        return a.d < b.d ? -1 : 1;
      })
      .map((x) => x.c);
  }
  return { review, preview, isDue, dueCards };
}
