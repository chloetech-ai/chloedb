// title: Nutrition
// description: Food and body numbers: body mass index, resting and daily energy needs, calories from macronutrients, macro targets in grams, and portions and totals of food rows (meal logs, diet planners, fitness apps).
// api: bmi(kg, cm) -> {bmi (1 decimal), category: 'underweight' (< 18.5) | 'normal' (< 25) | 'overweight' (< 30) | 'obese'} or null;
//   bmr({kg, cm, age, sex}) -> resting kcal per day, rounded (Mifflin-St Jeor: 10 kg + 6.25 cm - 5 age, +5 for 'male'/'m', -161 for 'female'/'f',
//   -78 (their middle) when sex is missing or other), or null;
//   tdee(bmr, activity = 'sedentary') -> kcal per day, rounded; activity 'sedentary' 1.2 | 'light' 1.375 | 'moderate' 1.55 | 'active' 1.725 | 'very' 1.9
//   ('very active', 'very_active' and 'extra' work too) or a factor as a number; null when unknown;
//   calories({protein, carbs, fat, alcohol}) -> kcal from grams (4, 4, 9, 7 per gram), rounded; missing parts count 0;
//   macros(kcal, {protein = 30, carbs = 40, fat = 30}) -> {protein, carbs, fat} in whole grams for those percents of kcal (percents are scaled to add up to 100), or null;
//   scale(food, grams, per = 100) -> the food row with every number scaled from per grams to grams (1 decimal), other fields kept; null when food or grams is missing;
//   totals(items, fields?) -> {field: sum} over the numbers in the rows (1 decimal), every numeric field or only the fields listed; {} for none
function helper() {
  const num = (x) => (typeof x === 'number' && isFinite(x) ? x : typeof x === 'string' && x.trim() !== '' && isFinite(Number(x)) ? Number(x) : null);
  const r1 = (x) => Math.round(x * 10) / 10;

  function bmi(kg, cm) {
    const w = num(kg);
    const h = num(cm);
    if (w == null || h == null || w <= 0 || h <= 0) return null;
    const m = h / 100;
    const value = r1(w / (m * m));
    const category = value < 18.5 ? 'underweight' : value < 25 ? 'normal' : value < 30 ? 'overweight' : 'obese';
    return { bmi: value, category };
  }

  function bmr({ kg, cm, age, sex } = {}) {
    const w = num(kg);
    const h = num(cm);
    const a = num(age);
    if (w == null || h == null || a == null || w <= 0 || h <= 0 || a < 0) return null;
    const s = String(sex || '').trim().toLowerCase();
    const adj = ['male', 'm', 'man'].includes(s) ? 5 : ['female', 'f', 'woman'].includes(s) ? -161 : -78;
    return Math.round(10 * w + 6.25 * h - 5 * a + adj);
  }

  const FACTORS = { sedentary: 1.2, light: 1.375, moderate: 1.55, active: 1.725, very: 1.9, very_active: 1.9, 'very active': 1.9, extra: 1.9 };
  function tdee(base, activity = 'sedentary') {
    const b = num(base);
    if (b == null || b <= 0) return null;
    let f = typeof activity === 'number' ? activity : FACTORS[String(activity == null ? 'sedentary' : activity).trim().toLowerCase()];
    if (typeof f !== 'number' || !isFinite(f) || f <= 0) return null;
    return Math.round(b * f);
  }

  function calories({ protein, carbs, fat, alcohol } = {}) {
    const g = (x) => num(x) || 0;
    return Math.round(g(protein) * 4 + g(carbs) * 4 + g(fat) * 9 + g(alcohol) * 7);
  }

  function macros(kcal, { protein = 30, carbs = 40, fat = 30 } = {}) {
    const k = num(kcal);
    const p = Math.max(0, num(protein) || 0);
    const c = Math.max(0, num(carbs) || 0);
    const f = Math.max(0, num(fat) || 0);
    const total = p + c + f;
    if (k == null || k < 0 || total <= 0) return null;
    return {
      protein: Math.round((k * p) / total / 4),
      carbs: Math.round((k * c) / total / 4),
      fat: Math.round((k * f) / total / 9),
    };
  }

  function scale(food, grams, per = 100) {
    const g = num(grams);
    const base = num(per);
    if (!food || typeof food !== 'object' || g == null || g < 0 || base == null || base <= 0) return null;
    const out = {};
    for (const key of Object.keys(food)) {
      const v = food[key];
      out[key] = typeof v === 'number' && isFinite(v) ? r1((v * g) / base) : v;
    }
    return out;
  }

  function totals(items, fields) {
    const out = {};
    if (!Array.isArray(items)) return out;
    const only = Array.isArray(fields) && fields.length ? fields : null;
    if (only) for (const f of only) out[f] = 0;
    for (const row of items) {
      if (!row || typeof row !== 'object') continue;
      for (const key of only || Object.keys(row)) {
        const v = row[key];
        if (typeof v !== 'number' || !isFinite(v)) continue;
        out[key] = (out[key] || 0) + v;
      }
    }
    for (const key of Object.keys(out)) out[key] = r1(out[key]);
    return out;
  }

  return { bmi, bmr, tdee, calories, macros, scale, totals };
}
