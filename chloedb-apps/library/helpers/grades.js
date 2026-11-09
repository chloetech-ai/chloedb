// title: Grades
// description: School grades and scores: percentages, weighted averages, letter grades with optional plus and minus, grade point averages, the score still needed to reach a target, and curving a set of scores; for gradebooks, course trackers and study planners.
// api: percent(points, possible) -> 0..100 (more with extra credit), or null when possible is not above 0;
//   weighted(items) -> the weighted average 0..100 of items [{score, weight}] (weight defaults to 1, weights need not add up to 1 or 100;
//   items without a number score or with weight <= 0 are skipped), or null when nothing counts;
//   letter(score, {scale, plusMinus = false}) -> 'B' or 'B+', or null when score is not a number;
//   scale: [{letter, min}] or {letter: min} (default A 90, B 80, C 70, D 60, F 0); a score below every min gets the lowest letter;
//   plusMinus: the top 30% of a band gets '+' and the bottom 30% gets '-' (B+ 87, B- below 83; A+ from 97), never on the lowest band;
//   letter(score, scale) also works with the scale array as the second argument;
//   gpa(letters, scale = 4) -> the credit-weighted grade point average rounded to 2 places, or null when nothing counts;
//   letters: ['A', 'B+', ...] or [{letter, credits = 1}]; case does not matter; letters not on the scale (P, W, I) are skipped;
//   points: A+ and A 4, A- 3.7, B+ 3.3, B 3, B- 2.7, C+ 2.3, C 2, C- 1.7, D+ 1.3, D 1, D- 0.7, F 0;
//   scale: a number (the points of an A, the rest in proportion: 5 makes B 3.75) or {letter: points} laid over the defaults ({'A+': 4.3});
//   needed(current, weightDone, target) -> the average score needed on the rest of the work to finish at target, or null when nothing is left;
//   weightDone is the share already graded, 0..1 (or a percent above 1); the answer may be above 100 (out of reach) or below 0 (already safe);
//   curve(scores, {to, max = 100}) -> the scores shifted so their mean is to, each kept between 0 and max (max null for no cap);
//   entries that are not numbers stay as null in place; without a number to, the scores come back unchanged
function helper() {
  const num = (x) => {
    if (typeof x === 'number') return isFinite(x) ? x : null;
    if (typeof x === 'string' && x.trim() !== '' && isFinite(Number(x))) return Number(x);
    return null;
  };
  const tidy = (x) => Math.round(x * 1e9) / 1e9;

  function percent(points, possible) {
    const p = num(points);
    const q = num(possible);
    if (p === null || q === null || q <= 0) return null;
    return tidy((p * 100) / q);
  }

  function weighted(items) {
    if (!Array.isArray(items)) return null;
    let sum = 0;
    let total = 0;
    for (const it of items) {
      if (!it || typeof it !== 'object') continue;
      const s = num(it.score);
      const w = it.weight == null ? 1 : num(it.weight);
      if (s === null || w === null || w <= 0) continue;
      sum += s * w;
      total += w;
    }
    return total > 0 ? tidy(sum / total) : null;
  }

  const DEFAULT_SCALE = [{ letter: 'A', min: 90 }, { letter: 'B', min: 80 }, { letter: 'C', min: 70 }, { letter: 'D', min: 60 }, { letter: 'F', min: 0 }];
  function readScale(scale) {
    let list = [];
    if (Array.isArray(scale)) {
      list = scale.map((b) => (Array.isArray(b) ? { letter: b[0], min: b[1] } : b || {}));
    } else if (scale && typeof scale === 'object') {
      list = Object.keys(scale).map((k) => ({ letter: k, min: scale[k] }));
    }
    list = list
      .map((b) => ({ letter: b.letter == null ? '' : String(b.letter), min: num(b.min) }))
      .filter((b) => b.letter && b.min !== null)
      .sort((a, b) => b.min - a.min);
    return list.length ? list : DEFAULT_SCALE;
  }

  function letter(score, opts) {
    const o = Array.isArray(opts) ? { scale: opts } : opts || {};
    const s = num(score);
    if (s === null) return null;
    const bands = readScale(o.scale);
    const last = bands.length - 1;
    for (let i = 0; i <= last; i++) {
      const band = bands[i];
      if (s < band.min && i !== last) continue;
      if (!o.plusMinus || i === last) return band.letter;
      const width = i === 0 ? band.min - bands[1].min : bands[i - 1].min - band.min;
      const upper = i === 0 ? band.min + width : bands[i - 1].min;
      if (s >= upper - (width * 3) / 10) return band.letter + '+';
      if (s < band.min + (width * 3) / 10) return band.letter + '-';
      return band.letter;
    }
    return null;
  }

  const POINTS = { 'A+': 4, A: 4, 'A-': 3.7, 'B+': 3.3, B: 3, 'B-': 2.7, 'C+': 2.3, C: 2, 'C-': 1.7, 'D+': 1.3, D: 1, 'D-': 0.7, F: 0 };
  function pointsTable(scale) {
    const table = {};
    if (scale && typeof scale === 'object' && !Array.isArray(scale)) {
      for (const k of Object.keys(POINTS)) table[k] = POINTS[k];
      for (const k of Object.keys(scale)) {
        const v = num(scale[k]);
        if (v !== null) table[k.trim().toUpperCase()] = v;
      }
      return table;
    }
    const top = num(scale);
    const factor = top !== null && top > 0 ? top / 4 : 1;
    for (const k of Object.keys(POINTS)) table[k] = POINTS[k] * factor;
    return table;
  }

  function gpa(letters, scale = 4) {
    if (!Array.isArray(letters)) return null;
    const table = pointsTable(scale);
    let sum = 0;
    let total = 0;
    for (const it of letters) {
      const raw = typeof it === 'string' ? it : it && typeof it === 'object' ? it.letter : null;
      if (raw == null) continue;
      const key = String(raw).trim().toUpperCase();
      if (!Object.prototype.hasOwnProperty.call(table, key)) continue;
      const credits = typeof it === 'string' || it.credits == null ? 1 : num(it.credits);
      if (credits === null || credits <= 0) continue;
      sum += table[key] * credits;
      total += credits;
    }
    if (!total) return null;
    return Math.round(tidy(sum / total) * 100) / 100;
  }

  function needed(current, weightDone, target) {
    const c = num(current);
    const w = num(weightDone);
    const t = num(target);
    if (c === null || w === null || t === null) return null;
    const done = w <= 1 ? w * 100 : w;
    if (done < 0 || done >= 100) return null;
    return tidy((t * 100 - c * done) / (100 - done));
  }

  function curve(scores, { to, max = 100 } = {}) {
    if (!Array.isArray(scores)) return [];
    const vals = scores.map(num);
    const valid = vals.filter((v) => v !== null);
    const goal = num(to);
    if (!valid.length || goal === null) return vals;
    const mean = valid.reduce((a, b) => a + b, 0) / valid.length;
    const shift = goal - mean;
    const cap = num(max);
    return vals.map((v) => {
      if (v === null) return null;
      let x = Math.max(0, v + shift);
      if (cap !== null) x = Math.min(cap, x);
      return tidy(x);
    });
  }

  return { percent, weighted, letter, gpa, needed, curve };
}
