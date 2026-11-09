// title: Units
// description: Converts quantities between units of length, weight, volume, temperature and speed, reads amounts like "1 1/2 cups" or "250 g", scales a recipe line, and picks the unit that reads best (1500 g as 1.5 kg).
// api: convert(value, from, to) -> number or null ('kg' to 'lb', 'c' to 'f', 'cup' to 'ml'; names and plurals work: 'pounds', 'litres');
//   parse('1 1/2 cups' | '250g' | '½ tsp' | '3.5 km') -> {value, unit, kind} or null (unit is the canonical name, or '' when none);
//   format(value, unit, {places = 2, fractions}) -> '1.5 kg', '¾ cup' (fractions default on for cups and spoons);
//   scale(text, factor) -> the quantity at the start of a line times factor ('1 1/2 cups flour', 2 -> '3 cups flour');
//   units are named as people write them; temperatures canonically 'degc', 'degf' and 'k' ('c' alone is a cup);
//   best(value, unit) -> {value, unit} in the unit of the same system that reads best; kind(unit) -> 'length' | 'mass' | 'volume' | 'temperature' | 'speed' | null;
//   units(kind) -> the canonical unit names of a kind
function helper() {
  const K = {
    length: { mm: 0.001, cm: 0.01, m: 1, km: 1000, in: 0.0254, ft: 0.3048, yd: 0.9144, mi: 1609.344 },
    mass: { mg: 0.001, g: 1, kg: 1000, oz: 28.349523125, lb: 453.59237, st: 6350.29318 },
    volume: { ml: 1, cl: 10, dl: 100, l: 1000, tsp: 4.92892159375, tbsp: 14.78676478125, floz: 29.5735295625, cup: 236.5882365, pt: 473.176473, qt: 946.352946, gal: 3785.411784 },
    speed: { kmh: 1, mph: 1.609344, ms: 3.6, kn: 1.852 },
    temperature: { degc: 1, degf: 1, k: 1 },
  };
  const ALIAS = {
    millimeter: 'mm', millimetre: 'mm', centimeter: 'cm', centimetre: 'cm', meter: 'm', metre: 'm', kilometer: 'km', kilometre: 'km',
    inch: 'in', inches: 'in', '"': 'in', foot: 'ft', feet: 'ft', "'": 'ft', yard: 'yd', mile: 'mi',
    milligram: 'mg', gram: 'g', gr: 'g', kilogram: 'kg', kilo: 'kg', kgs: 'kg', ounce: 'oz', pound: 'lb', lbs: 'lb', stone: 'st',
    milliliter: 'ml', millilitre: 'ml', liter: 'l', litre: 'l', teaspoon: 'tsp', tsps: 'tsp', tablespoon: 'tbsp', tbs: 'tbsp', tbsps: 'tbsp',
    'fl oz': 'floz', 'fl.oz': 'floz', 'fluid ounce': 'floz', cups: 'cup', c: 'cup', pint: 'pt', quart: 'qt', gallon: 'gal',
    '°c': 'degc', 'ºc': 'degc', celsius: 'degc', '°f': 'degf', 'ºf': 'degf', f: 'degf', fahrenheit: 'degf', kelvin: 'k', 'km/h': 'kmh', kph: 'kmh', 'm/s': 'ms', knot: 'kn', knots: 'kn',
  };
  function canon(u) {
    let s = String(u || '').trim().toLowerCase().replace(/\.$/, '');
    if (!s) return '';
    if (ALIAS[s]) return ALIAS[s];
    for (const k of Object.values(K)) if (s in k) return s;
    if (s.endsWith('s') && ALIAS[s.slice(0, -1)]) return ALIAS[s.slice(0, -1)];
    if (s.endsWith('s')) for (const k of Object.values(K)) if (s.slice(0, -1) in k) return s.slice(0, -1);
    return null;
  }
  const kindOf = (u) => { const c = canon(u); if (!c) return null; for (const [k, t] of Object.entries(K)) if (c in t) return k; return null; };
  const label = (u, plural) => ({ degc: '°C', degf: '°F', k: 'K', kmh: 'km/h', ms: 'm/s', floz: 'fl oz' }[u] || (plural && PLURAL[u]) || u);
  // 'c' alone is a cup in a recipe, unless the other side is a temperature.
  function convert(value, from, to) {
    let a = canon(from); let b = canon(to);
    const v = Number(value);
    if (!Number.isFinite(v) || !a || !b) return null;
    if (a === 'cup' && /^c$/i.test(String(from).trim()) && kindOf(b) === 'temperature') a = 'degc';
    if (b === 'cup' && /^c$/i.test(String(to).trim()) && kindOf(a) === 'temperature') b = 'degc';
    const ka = kindOf(a); const kb = kindOf(b);
    if (!ka || ka !== kb) return null;
    if (ka === 'temperature') {
      const c = a === 'degc' ? v : a === 'degf' ? (v - 32) * 5 / 9 : v - 273.15;
      return b === 'degc' ? c : b === 'degf' ? c * 9 / 5 + 32 : c + 273.15;
    }
    return (v * K[ka][a]) / K[ka][b];
  }
  const VULGAR = { '½': 0.5, '⅓': 1 / 3, '⅔': 2 / 3, '¼': 0.25, '¾': 0.75, '⅛': 0.125, '⅜': 0.375, '⅝': 0.625, '⅞': 0.875, '⅕': 0.2 };
  const NUM = /^\s*(\d+(?:[.,]\d+)?(?:\s+\d+\/\d+)?|\d+\/\d+|\d*[½⅓⅔¼¾⅛⅜⅝⅞⅕])/;
  function number(s) {
    const t = s.trim().replace(',', '.');
    let m = t.match(/^(\d+)?\s*([½⅓⅔¼¾⅛⅜⅝⅞⅕])$/);
    if (m) return (Number(m[1]) || 0) + VULGAR[m[2]];
    m = t.match(/^(?:(\d+)\s+)?(\d+)\/(\d+)$/);
    if (m) return (Number(m[1]) || 0) + Number(m[2]) / Number(m[3]);
    return Number(t);
  }
  function parse(text) {
    const s = String(text || '');
    const m = s.match(NUM);
    if (!m) return null;
    const value = number(m[1]);
    if (!Number.isFinite(value)) return null;
    const rest = s.slice(m[0].length).trim();
    const um = rest.match(/^(fl\.? ?oz|fluid ounces?|°[cf]|km\/h|m\/s|[a-z]+\.?|["'])/i);
    const unit = um ? canon(um[1]) : null;
    return { value, unit: unit || '', kind: unit ? kindOf(unit) : null, rest: unit ? rest.slice(um[0].length).trim() : rest };
  }
  const FRAC = [[0, ''], [0.125, '⅛'], [0.25, '¼'], [1 / 3, '⅓'], [0.375, '⅜'], [0.5, '½'], [0.625, '⅝'], [2 / 3, '⅔'], [0.75, '¾'], [0.875, '⅞'], [1, '']];
  function nice(v, places, fractions) {
    if (fractions) {
      const whole = Math.floor(v + 1e-9);
      const part = v - whole;
      let best = FRAC[0];
      for (const f of FRAC) if (Math.abs(part - f[0]) < Math.abs(part - best[0])) best = f;
      if (Math.abs(part - best[0]) < 0.04) {
        const w = best[0] === 1 ? whole + 1 : whole;
        return best[1] ? `${w || ''}${best[1]}` : String(w);
      }
    }
    return String(Math.round(v * 10 ** places) / 10 ** places);
  }
  const KITCHEN = new Set(['cup', 'tsp', 'tbsp']);
  const PLURAL = { cup: 'cups' };
  function format(value, unit, { places = 2, fractions } = {}) {
    const u = canon(unit) ?? String(unit || '');
    const n = nice(Number(value), places, fractions === undefined ? KITCHEN.has(u) : fractions);
    const l = label(u, Number(value) > 1);
    return l ? `${n} ${l}` : n;
  }
  function scale(text, factor) {
    const s = String(text || '');
    const m = s.match(NUM);
    if (!m) return s;
    const v = number(m[1]) * Number(factor);
    const p = parse(s);
    const kitchen = p && KITCHEN.has(p.unit);
    let rest = s.slice(m[0].length);
    if (p && p.unit === 'cup') rest = rest.replace(/^(\s*)cups?\b/i, `$1${v > 1 ? 'cups' : 'cup'}`);
    return `${s.slice(0, m.index)}${m[0].match(/^\s*/)[0]}${nice(v, 2, kitchen || /\//.test(m[1]) || /[½⅓⅔¼¾⅛⅜⅝⅞⅕]/.test(m[1]))}${rest}`;
  }
  const LADDER = { length: { metric: ['mm', 'cm', 'm', 'km'], imperial: ['in', 'ft', 'mi'] }, mass: { metric: ['mg', 'g', 'kg'], imperial: ['oz', 'lb'] }, volume: { metric: ['ml', 'l'], imperial: ['tsp', 'tbsp', 'cup', 'qt', 'gal'] } };
  function best(value, unit) {
    const u = canon(unit); const k = kindOf(u);
    if (!LADDER[k]) return { value: Number(value), unit: u };
    const system = LADDER[k].metric.includes(u) ? 'metric' : 'imperial';
    const ladder = LADDER[k][system];
    let pick = ladder[0];
    for (const cand of ladder) if (Math.abs(convert(value, u, cand)) >= 1) pick = cand;
    return { value: convert(value, u, pick), unit: pick };
  }
  const units = (kind) => Object.keys(K[kind] || {});
  return { convert, parse, format, scale, best, kind: kindOf, units };
}
