// title: Quizzes
// description: Scores quizzes and drills (points, percent, pass or fail), shuffles a question's choices reproducibly, and picks the questions to practise from past answers, for flashcards, tests, trivia and lessons.
// api: score(questions, answers) -> {points, possible, percent, correct: [indexes], wrong: [indexes]};
//   questions: [{answer, points = 1, id?}]; answers: an array in question order, or an object keyed by question id (or index);
//   an answer matches when equal, strings compared trimmed, case-insensitive and with spaces collapsed, numbers and numeric text by value;
//   an array answer means all of these in any order (multiple choice); a missing answer is wrong; questions without an answer are left out;
//   percent is rounded to 1 decimal, null when nothing was possible;
//   passed(result, passPct = 70) -> true when result.percent >= passPct;
//   shuffleChoices(question {choices, answer}, seed = 0) -> a copy with choices in an order that is always the same for the same seed (number or text),
//   and answer moved with them: a whole number answer (or each of an array) is a choice's index; any other answer is a choice's value and stays;
//   null when question is not an object;
//   review(history, {mastered = 3, limit}) -> question ids to practise, weakest first: a question is learnt once its last `mastered` answers
//   were all correct; the rest sort by the run of correct answers they end on, then by share correct, then the longest unpractised;
//   history: [{question (an id), correct (true when right), at ('YYYY-MM-DD' or ISO time)}] in any order;
//   streak(history, question?) -> the run of correct answers ending at the latest one, over every question or only the one given
function helper() {
  const norm = (s) => s.trim().replace(/\s+/g, ' ').toLowerCase();
  function same(a, b) {
    if (a === b) return true;
    if (a == null || b == null) return false;
    if (typeof a === 'string' && typeof b === 'string') return norm(a) === norm(b);
    if (typeof a === 'number' && typeof b === 'string' && b.trim() !== '') return a === Number(b);
    if (typeof b === 'number' && typeof a === 'string' && a.trim() !== '') return b === Number(a);
    return false;
  }
  const unique = (list) => list.reduce((out, x) => (out.some((y) => same(x, y)) ? out : out.concat([x])), []);
  function matches(expected, given) {
    if (Array.isArray(expected)) {
      if (!Array.isArray(given)) return false;
      const e = unique(expected);
      const g = unique(given);
      return e.length === g.length && e.every((x) => g.some((y) => same(x, y)));
    }
    return !Array.isArray(given) && same(expected, given);
  }
  const round = (n, k) => Math.round(n * k) / k;

  function score(questions, answers) {
    const qs = Array.isArray(questions) ? questions : [];
    const pick = (q, i) => {
      if (Array.isArray(answers)) return answers[i];
      if (answers && typeof answers === 'object') {
        if (q.id != null && Object.prototype.hasOwnProperty.call(answers, q.id)) return answers[q.id];
        return answers[i];
      }
      return undefined;
    };
    let points = 0;
    let possible = 0;
    const correct = [];
    const wrong = [];
    qs.forEach((q, i) => {
      if (!q || typeof q !== 'object' || q.answer === undefined) return;
      let pts = q.points == null ? 1 : Number(q.points);
      if (!(Number.isFinite(pts) && pts >= 0)) pts = 1;
      possible += pts;
      if (matches(q.answer, pick(q, i))) { points += pts; correct.push(i); } else wrong.push(i);
    });
    points = round(points, 1e6);
    possible = round(possible, 1e6);
    return { points, possible, percent: possible > 0 ? round((points / possible) * 100, 10) : null, correct, wrong };
  }

  function passed(result, passPct = 70) {
    const need = Number.isFinite(Number(passPct)) && passPct !== null ? Number(passPct) : 70;
    return !!result && typeof result.percent === 'number' && result.percent >= need;
  }

  function rng(seed) {
    let h = 2166136261;
    const s = String(seed == null ? 0 : seed);
    for (let i = 0; i < s.length; i++) { h ^= s.charCodeAt(i); h = Math.imul(h, 16777619); }
    let a = h >>> 0;
    return () => {
      a = (a + 0x6d2b79f5) | 0;
      let t = Math.imul(a ^ (a >>> 15), 1 | a);
      t = (t + Math.imul(t ^ (t >>> 7), 61 | t)) ^ t;
      return ((t ^ (t >>> 14)) >>> 0) / 4294967296;
    };
  }
  function shuffleChoices(question, seed = 0) {
    if (!question || typeof question !== 'object' || Array.isArray(question)) return null;
    if (!Array.isArray(question.choices)) return { ...question };
    const choices = question.choices;
    const order = choices.map((_, i) => i);
    const rand = rng(seed);
    for (let i = order.length - 1; i > 0; i--) {
      const j = Math.floor(rand() * (i + 1));
      const t = order[i]; order[i] = order[j]; order[j] = t;
    }
    const move = (a) => (Number.isInteger(a) && a >= 0 && a < choices.length ? order.indexOf(a) : a);
    const answer = Array.isArray(question.answer) ? question.answer.map(move) : move(question.answer);
    return { ...question, choices: order.map((i) => choices[i]), answer };
  }

  function toMs(at) {
    if (typeof at === 'number') return Number.isFinite(at) ? at : null;
    if (typeof at !== 'string') return null;
    if (/^\d{4}-\d{2}-\d{2}$/.test(at)) return Date.UTC(Number(at.slice(0, 4)), Number(at.slice(5, 7)) - 1, Number(at.slice(8, 10)));
    if (/^\d{4}-\d{2}-\d{2}T/.test(at)) { const ms = Date.parse(at); return Number.isFinite(ms) ? ms : null; }
    return null;
  }
  function ordered(history) {
    return (Array.isArray(history) ? history : [])
      .map((r, idx) => (r && typeof r === 'object' && r.question != null ? { q: r.question, ok: r.correct === true, ms: toMs(r.at), idx } : null))
      .filter(Boolean)
      .sort((a, b) => {
        const x = a.ms == null ? -Infinity : a.ms;
        const y = b.ms == null ? -Infinity : b.ms;
        if (x !== y) return x < y ? -1 : 1;
        return a.idx - b.idx;
      });
  }
  const trailing = (list) => { let n = 0; for (let i = list.length - 1; i >= 0 && list[i].ok; i--) n++; return n; };

  function review(history, { mastered = 3, limit } = {}) {
    const need = Number.isFinite(Number(mastered)) && Number(mastered) > 0 ? Number(mastered) : 3;
    const byQ = new Map();
    let first = 0;
    for (const r of ordered(history)) {
      if (!byQ.has(r.q)) byQ.set(r.q, { id: r.q, list: [], seen: first++ });
      byQ.get(r.q).list.push(r);
    }
    const weak = [];
    for (const g of byQ.values()) {
      const run = trailing(g.list);
      if (run >= need) continue;
      const last = g.list[g.list.length - 1].ms;
      weak.push({ id: g.id, run, acc: g.list.filter((r) => r.ok).length / g.list.length, last: last == null ? -Infinity : last, seen: g.seen });
    }
    weak.sort((a, b) => a.run - b.run || a.acc - b.acc || (a.last !== b.last ? (a.last < b.last ? -1 : 1) : 0) || a.seen - b.seen);
    const ids = weak.map((w) => w.id);
    return Number.isInteger(limit) && limit >= 0 ? ids.slice(0, limit) : ids;
  }

  function streak(history, question) {
    const list = ordered(history).filter((r) => question === undefined || r.q === question);
    return trailing(list);
  }

  return { score, passed, shuffleChoices, review, streak };
}
