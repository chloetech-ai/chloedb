// title: Money
// description: Amounts of money without rounding errors: parse what people type, format with a currency, add, split a bill evenly or by shares, percentages, and monthly cost of something that recurs.
// api: parse(text) -> number or null ('$1,234.50', '1.234,50 €', '-12', '(40)' all work);
//   format(amount, currency = 'USD', {cents = true, sign = false}) -> '$1,234.50' (sign: '+$5.00' for gains);
//   round(amount, places = 2) -> number; sum(amounts, by?) -> number to the cent;
//   split(total, ways or [weights]) -> amounts that add up to total exactly (the odd cents go to the first shares);
//   percent(part, whole, places = 1) -> number or null; applyPercent(amount, pct) -> amount * pct / 100 to the cent;
//   monthly(amount, period: 'day' | 'week' | 'month' | 'quarter' | 'year') -> the cost per month;
//   symbol(currency) -> '$', '€', '£', ...
function helper() {
  const SYMBOLS = { USD: '$', EUR: '€', GBP: '£', JPY: '¥', CNY: '¥', INR: '₹', KRW: '₩', BRL: 'R$', CAD: 'CA$', AUD: 'A$', MXN: 'MX$', CLP: 'CLP$', ARS: 'AR$', CHF: 'CHF ', SEK: 'kr ', NOK: 'kr ', DKK: 'kr ' };
  const NO_CENTS = new Set(['JPY', 'KRW', 'CLP']);
  const toCents = (x) => Math.round(Number(x) * 100 + (Number(x) >= 0 ? 1e-9 : -1e-9));
  function parse(text) {
    if (typeof text === 'number') return Number.isFinite(text) ? text : null;
    if (text == null) return null;
    let s = String(text).trim();
    if (!s) return null;
    const negative = /^\(.*\)$/.test(s) || /^-|-$|^[^\d]*-/.test(s);
    s = s.replace(/[^\d.,]/g, '');
    if (!s) return null;
    const lastDot = s.lastIndexOf('.');
    const lastComma = s.lastIndexOf(',');
    let decimal = null;
    if (lastDot >= 0 && lastComma >= 0) decimal = lastDot > lastComma ? '.' : ',';
    else if (lastComma >= 0) decimal = s.length - lastComma - 1 === 3 && s.split(',').length > 1 && !/^0,/.test(s) ? null : ',';
    else if (lastDot >= 0) decimal = s.length - lastDot - 1 === 3 && s.split('.').length > 2 ? null : '.';
    let n;
    if (decimal) {
      const i = s.lastIndexOf(decimal);
      n = Number(`${s.slice(0, i).replace(/[.,]/g, '')}.${s.slice(i + 1)}`);
    } else n = Number(s.replace(/[.,]/g, ''));
    if (!Number.isFinite(n)) return null;
    return negative ? -n : n;
  }
  function symbol(currency = 'USD') { return SYMBOLS[String(currency).toUpperCase()] ?? `${String(currency).toUpperCase()} `; }
  function format(amount, currency = 'USD', { cents, sign = false } = {}) {
    if (amount == null || !Number.isFinite(Number(amount))) return '';
    const cur = String(currency).toUpperCase();
    const places = cents === undefined ? (NO_CENTS.has(cur) ? 0 : 2) : cents ? 2 : 0;
    const n = Number(amount);
    const abs = Math.abs(n).toFixed(places);
    const [int, frac] = abs.split('.');
    const grouped = int.replace(/\B(?=(\d{3})+(?!\d))/g, ',');
    const body = `${symbol(cur)}${grouped}${frac ? `.${frac}` : ''}`;
    return n < 0 ? `-${body}` : sign && n > 0 ? `+${body}` : body;
  }
  function round(amount, places = 2) { const f = 10 ** places; return Math.round(Number(amount) * f + (Number(amount) >= 0 ? 1e-9 : -1e-9)) / f; }
  function sum(amounts, by) {
    const get = by === undefined ? (x) => x : typeof by === 'function' ? by : (x) => x && x[by];
    return (amounts || []).reduce((a, x) => a + (Number.isFinite(Number(get(x))) && get(x) !== null && get(x) !== '' ? toCents(get(x)) : 0), 0) / 100;
  }
  function split(total, ways) {
    const weights = Array.isArray(ways) ? ways.map((w) => Math.max(0, Number(w) || 0)) : Array.from({ length: Math.max(1, Math.round(ways)) }, () => 1);
    const totalW = weights.reduce((a, b) => a + b, 0);
    if (!totalW) throw new Error('split: the shares add up to 0');
    const cents = toCents(total);
    const raw = weights.map((w) => (cents * w) / totalW);
    const out = raw.map((x) => Math.trunc(x));
    let left = cents - out.reduce((a, b) => a + b, 0);
    const step = left >= 0 ? 1 : -1;
    const order = raw.map((x, i) => [Math.abs(x - Math.trunc(x)), i]).sort((a, b) => b[0] - a[0] || a[1] - b[1]);
    for (let k = 0; left !== 0 && k < order.length * 2; k++, left -= step) out[order[k % order.length][1]] += step;
    return out.map((c) => c / 100);
  }
  function percent(part, whole, places = 1) { return Number(whole) ? round((Number(part) / Number(whole)) * 100, places) : null; }
  function applyPercent(amount, pct) { return round((Number(amount) * Number(pct)) / 100); }
  function monthly(amount, period = 'month') {
    const per = { day: 365.25 / 12, week: 52.1775 / 12, month: 1, quarter: 1 / 3, year: 1 / 12 }[period];
    if (per === undefined) throw new Error('monthly: period is day, week, month, quarter or year');
    return round(Number(amount) * per);
  }
  return { parse, format, round, sum, split, percent, applyPercent, monthly, symbol };
}
