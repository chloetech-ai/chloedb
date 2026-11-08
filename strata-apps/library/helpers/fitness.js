// title: Fitness
// description: Training numbers for runners and lifters: pace and speed from a distance and a time, finish times from a pace, one-rep max estimates, plates to load on a bar, heart-rate zones, calories from MET values and lifted volume.
// api: pace(km, minutes) -> 'm:ss /km' (null when km or minutes is not above 0); minutes is a number, or a clock text 'mm:ss' or 'h:mm:ss';
//   paceMiles(km, minutes) -> 'm:ss /mi', the same run per mile; speed(km, minutes) -> km/h rounded to 2 places, or null;
//   finishTime(km, pace) -> 'h:mm:ss' for km at pace ('m:ss', 'm:ss /km', or a number of minutes per km), or null;
//   oneRepMax(weight, reps, formula = 'epley' | 'brzycki' | 'average') -> estimated 1RM rounded to 0.1 (reps 1 gives the weight;
//   null for reps below 1, or for brzycki at 37 reps or more); Epley weight * (1 + reps / 30), Brzycki weight * 36 / (37 - reps);
//   plates(target, bar = 20, plates = [25, 20, 15, 10, 5, 2.5, 1.25]) -> {perSide: [plates, heaviest first], total: what that loads,
//   remainder: target - total (0 when exact)}, any number of each plate; null when target is below the bar;
//   zones(maxHr or age) -> five zones [{zone: 1..5, name, low, high (percent of max), min, max (beats per minute)}];
//   a value below 120 is an age and the max is 220 - age; 50-60 Recovery, 60-70 Endurance, 70-80 Tempo, 80-90 Threshold, 90-100 Maximum; [] when missing;
//   metCalories(met, kg, minutes) -> whole kcal, met * 3.5 * kg / 200 per minute (null when an input is missing or negative);
//   volume(sets) -> the sum of reps * weight over [{reps, weight}] (sets without a weight count 0), rounded to 0.01
function helper() {
  const KM_PER_MILE = 1.609344;
  const pad = (n) => String(n).padStart(2, '0');
  const num = (x) => {
    if (typeof x === 'number') return isFinite(x) ? x : null;
    if (typeof x === 'string' && x.trim() !== '' && isFinite(Number(x))) return Number(x);
    return null;
  };
  // 'm:ss' or 'h:mm:ss' -> seconds
  const clock = (text) => {
    const m = /^\s*(\d+):(\d{1,2})(?::(\d{1,2}))?(?:\.\d+)?\s*(?:\/\s*\w+)?\s*$/.exec(String(text));
    if (!m) return null;
    const a = Number(m[1]), b = Number(m[2]);
    if (m[3] === undefined) return b < 60 ? a * 60 + b : null;
    const c = Number(m[3]);
    return b < 60 && c < 60 ? a * 3600 + b * 60 + c : null;
  };
  // a number of minutes, or a clock text -> seconds
  const seconds = (x) => {
    const n = num(x);
    if (n !== null) return n * 60;
    return typeof x === 'string' ? clock(x) : null;
  };
  const fmtPace = (sec) => { const s = Math.round(sec); return `${Math.floor(s / 60)}:${pad(s % 60)}`; };
  const fmtClock = (sec) => {
    const s = Math.round(sec);
    return `${Math.floor(s / 3600)}:${pad(Math.floor((s % 3600) / 60))}:${pad(s % 60)}`;
  };
  const round = (x, places) => { const f = Math.pow(10, places); return Math.round(x * f) / f; };
  const positive = (x) => x !== null && x > 0;

  function pace(km, minutes) {
    const d = num(km), t = seconds(minutes);
    if (!positive(d) || !positive(t)) return null;
    return `${fmtPace(t / d)} /km`;
  }
  function paceMiles(km, minutes) {
    const d = num(km), t = seconds(minutes);
    if (!positive(d) || !positive(t)) return null;
    return `${fmtPace((t * KM_PER_MILE) / d)} /mi`;
  }
  function speed(km, minutes) {
    const d = num(km), t = seconds(minutes);
    if (!positive(d) || !positive(t)) return null;
    return round(d / (t / 3600), 2);
  }
  function finishTime(km, perKm) {
    const d = num(km), p = seconds(perKm);
    if (d === null || d < 0 || !positive(p)) return null;
    return fmtClock(d * p);
  }
  function oneRepMax(weight, reps, formula = 'epley') {
    const w = num(weight), r = num(reps);
    if (w === null || w < 0 || r === null || r < 1) return null;
    if (r === 1) return round(w, 1);
    const epley = w * (1 + r / 30);
    const brzycki = r < 37 ? (w * 36) / (37 - r) : null;
    if (formula === 'brzycki') return brzycki === null ? null : round(brzycki, 1);
    if (formula === 'average') return brzycki === null ? round(epley, 1) : round((epley + brzycki) / 2, 1);
    return round(epley, 1);
  }
  function plates(target, bar = 20, sizes = [25, 20, 15, 10, 5, 2.5, 1.25]) {
    const t = num(target), b = num(bar) === null ? 20 : num(bar);
    if (t === null || b < 0 || t < b) return null;
    const list = (Array.isArray(sizes) ? sizes : []).map(num).filter((p) => p !== null && p > 0)
      .map((p) => Math.round(p * 100)).sort((x, y) => y - x);
    let left = Math.round(((t - b) * 100) / 2);
    const perSide = [];
    for (const p of list) while (left >= p) { perSide.push(p / 100); left -= p; }
    const total = round(b + 2 * perSide.reduce((s, p) => s + p, 0), 2);
    return { perSide, total, remainder: round(t - total, 2) };
  }
  const ZONES = [[50, 60, 'Recovery'], [60, 70, 'Endurance'], [70, 80, 'Tempo'], [80, 90, 'Threshold'], [90, 100, 'Maximum']];
  function zones(maxHrOrAge) {
    const v = num(maxHrOrAge);
    if (!positive(v)) return [];
    const max = v < 120 ? 220 - v : v;
    if (max <= 0) return [];
    return ZONES.map(([low, high, name], i) => ({
      zone: i + 1, name, low, high, min: Math.round((max * low) / 100), max: Math.round((max * high) / 100),
    }));
  }
  function metCalories(met, kg, minutes) {
    const m = num(met), w = num(kg), t = num(minutes);
    if (m === null || w === null || t === null || m < 0 || w < 0 || t < 0) return null;
    return Math.round(((m * 3.5 * w) / 200) * t);
  }
  function volume(sets) {
    if (!Array.isArray(sets)) return 0;
    let total = 0;
    for (const s of sets) {
      if (!s || typeof s !== 'object') continue;
      const r = num(s.reps), w = num(s.weight);
      if (r === null || w === null) continue;
      total += r * w;
    }
    return round(total, 2);
  }
  return { pace, paceMiles, speed, finishTime, oneRepMax, plates, zones, metCalories, volume };
}
