// title: Numbers
// description: Everyday number work for counts, scores, prices and stats: clamp and round to a step, read what people type ('1,234.5', '1.234,5', '12%'), print with thousands separators, as percents, compact ('1.2k'), ordinals ('22nd') and roman numerals, and sum, average and interpolate.
// api: clamp(n, min, max) -> n kept within min..max (a missing bound is open; swapped bounds are put in order), null when n is not a number;
//   roundTo(n, step = 1) -> n rounded to the nearest multiple of step (0.25, 5, 100), halves away from zero, without float noise (null when n is not a number);
//   compact(n, places = 1) -> '950', '1.2k', '3.4M', '7B', '1.5T' (trailing zeros dropped; 999950 -> '1M'); '' when n is not a number;
//   ordinal(n) -> '1st', '2nd', '3rd', '11th', '22nd', '-1st' for whole numbers; '' otherwise;
//   percent(n, places = 0) -> '42%' from 0.42, with thousands separators ('1,250%'); '' when n is not a number;
//   parse(text) -> number or null: '1,234.5', '1.234,5', '1 234,5', '-12', '+3', '.5', '1e3', '12%' -> 0.12; a lone ',' or '.' is a decimal point
//   unless it groups thousands ('1,234' -> 1234, '1,5' -> 1.5, '1.234' -> 1.234, '1.234.567' -> 1234567); a finite number is returned as it is;
//   between(n, a, b) -> true when n is within a..b inclusive, in either order (false when any is not a number);
//   lerp(a, b, t) -> the value t of the way from a to b (t 0 gives a, 1 gives b; t outside 0..1 extrapolates), null when any is not a number;
//   sum(numbers) -> the total of the finite numbers in the list (others skipped), without float noise (0.1 + 0.2 -> 0.3); 0 for none;
//   average(numbers) -> their mean, null when there are none;
//   format(n, places) -> '1,234,567.89' with ',' between thousands and '.' for decimals; places fixes the decimals (default: as many as n has, up to 10); '' when n is not a number;
//   roman(n) -> 'MCMXCIV' for whole numbers 1..3999, '' otherwise
function helper() {
  const num = (v) => (typeof v === 'number' && isFinite(v) ? v : null);

  // round half away from zero to p decimals, without float noise
  function round(x, p) {
    const a = Math.abs(x);
    const s = String(a);
    let r;
    if (s.indexOf('e') < 0) {
      const m = String(Math.round(Number(s + 'e' + p)));
      r = m.indexOf('e') < 0 ? Number(m + 'e-' + p) : Math.round(a * Math.pow(10, p)) / Math.pow(10, p);
    } else {
      r = Math.round(a * Math.pow(10, p)) / Math.pow(10, p);
    }
    return x < 0 && r !== 0 ? -r : r;
  }
  // decimals a number is written with, -1 when it is in exponent form
  function decimals(x) {
    const s = String(x);
    if (s.indexOf('e') >= 0) return -1;
    const i = s.indexOf('.');
    return i < 0 ? 0 : s.length - i - 1;
  }
  const places = (p, dflt) => {
    const v = num(p);
    return v === null ? dflt : Math.min(20, Math.max(0, Math.floor(v)));
  };
  const group = (digits) => digits.replace(/\B(?=(\d{3})+(?!\d))/g, ',');

  function clamp(n, min, max) {
    const x = num(n);
    if (x === null) return null;
    let lo = num(min) === null ? -Infinity : min;
    let hi = num(max) === null ? Infinity : max;
    if (lo > hi) { const t = lo; lo = hi; hi = t; }
    return Math.min(hi, Math.max(lo, x));
  }

  function roundTo(n, step = 1) {
    const x = num(n);
    if (x === null) return null;
    const st = num(step) !== null && step > 0 ? step : 1;
    const q = x / st;
    const k = q < 0 ? -Math.round(-q) : Math.round(q);
    const r = k * st;
    const d = decimals(st);
    const out = d >= 0 && d <= 15 ? round(r, d) : r;
    return out === 0 ? 0 : out;
  }

  function format(n, p) {
    const x = num(n);
    if (x === null) return '';
    const fixed = places(p, null);
    let s;
    if (fixed === null) {
      const r = round(x, 10);
      const a = Math.abs(r);
      if (a >= 1e21) return String(r);
      s = String(a);
      if (s.indexOf('e') >= 0) s = a.toFixed(10).replace(/\.?0+$/, '');
      if (r === 0) return '0';
      const [i, f] = s.split('.');
      return (r < 0 ? '-' : '') + group(i) + (f ? '.' + f : '');
    }
    const r = round(x, fixed);
    const a = Math.abs(r);
    if (a >= 1e21) return String(r);
    s = a.toFixed(fixed);
    const [i, f] = s.split('.');
    return (r < 0 && /[1-9]/.test(s) ? '-' : '') + group(i) + (f ? '.' + f : '');
  }

  function compact(n, p = 1) {
    const x = num(n);
    if (x === null) return '';
    const pl = places(p, 1);
    const units = ['', 'k', 'M', 'B', 'T'];
    let v = Math.abs(x);
    let i = 0;
    while (v >= 1000 && i < units.length - 1) { v /= 1000; i++; }
    let r = round(v, pl);
    if (r >= 1000 && i < units.length - 1) { r = round(r / 1000, pl); i++; }
    if (r === 0) return '0';
    return (x < 0 ? '-' : '') + String(r) + units[i];
  }

  function ordinal(n) {
    if (!Number.isInteger(n)) return '';
    const a = Math.abs(n);
    const t = a % 100;
    const u = a % 10;
    const suffix = t >= 11 && t <= 13 ? 'th' : u === 1 ? 'st' : u === 2 ? 'nd' : u === 3 ? 'rd' : 'th';
    return String(n) + suffix;
  }

  function percent(n, p = 0) {
    const x = num(n);
    if (x === null) return '';
    return format(x * 100, places(p, 0)) + '%';
  }

  function parse(text) {
    if (typeof text === 'number') return isFinite(text) ? text : null;
    if (typeof text !== 'string') return null;
    let s = text.trim().replace(/[\s\u00a0\u202f'’_]/g, '').replace(/\u2212/g, '-');
    if (!s) return null;
    let pct = false;
    if (s[s.length - 1] === '%') { pct = true; s = s.slice(0, -1); }
    let neg = false;
    if (s[0] === '-' || s[0] === '+') { neg = s[0] === '-'; s = s.slice(1); }
    if (!s) return null;
    let v;
    if (/^(\d+\.?\d*|\.\d+)e[+-]?\d+$/i.test(s)) {
      v = Number(s);
    } else {
      const lastC = s.lastIndexOf(',');
      const lastD = s.lastIndexOf('.');
      let dec = null;
      let grp = null;
      if (lastC >= 0 && lastD >= 0) {
        dec = lastC > lastD ? ',' : '.';
        grp = dec === ',' ? '.' : ',';
      } else if (lastC >= 0) {
        const count = s.split(',').length - 1;
        if (count === 1 && !/^[1-9]\d{0,2},\d{3}$/.test(s)) dec = ',';
        else grp = ',';
      } else if (lastD >= 0) {
        if (s.split('.').length - 1 > 1) grp = '.';
        else dec = '.';
      }
      let int = s;
      let frac = '';
      if (dec) {
        const parts = s.split(dec);
        if (parts.length !== 2) return null;
        int = parts[0];
        frac = parts[1];
      }
      if (!/^\d*$/.test(frac)) return null;
      if (grp) {
        const g = grp === '.' ? '\\.' : ',';
        if (!new RegExp('^\\d+(' + g + '\\d+)*$').test(int)) return null;
        int = int.split(grp).join('');
      }
      if (!/^\d*$/.test(int) || (!int && !frac)) return null;
      v = Number((int || '0') + '.' + (frac || '0'));
    }
    if (!isFinite(v)) return null;
    if (pct) {
      const d = decimals(v);
      v = d >= 0 && d <= 13 ? round(v / 100, d + 2) : v / 100;
    }
    return neg && v !== 0 ? -v : v;
  }

  function between(n, a, b) {
    if (num(n) === null || num(a) === null || num(b) === null) return false;
    return n >= Math.min(a, b) && n <= Math.max(a, b);
  }

  function lerp(a, b, t) {
    if (num(a) === null || num(b) === null || num(t) === null) return null;
    return a * (1 - t) + b * t;
  }

  function sum(numbers) {
    if (!Array.isArray(numbers)) return 0;
    let total = 0;
    let most = 0;
    let exact = true;
    for (const v of numbers) {
      if (num(v) === null) continue;
      total += v;
      const d = decimals(v);
      if (d < 0) exact = false; else if (d > most) most = d;
    }
    if (exact && most <= 10) total = round(total, most);
    return total === 0 ? 0 : total;
  }

  function average(numbers) {
    if (!Array.isArray(numbers)) return null;
    const ok = numbers.filter((v) => num(v) !== null);
    if (!ok.length) return null;
    return sum(ok) / ok.length;
  }

  const ROMAN = [[1000, 'M'], [900, 'CM'], [500, 'D'], [400, 'CD'], [100, 'C'], [90, 'XC'], [50, 'L'], [40, 'XL'], [10, 'X'], [9, 'IX'], [5, 'V'], [4, 'IV'], [1, 'I']];
  function roman(n) {
    if (!Number.isInteger(n) || n < 1 || n > 3999) return '';
    let left = n;
    let out = '';
    for (const [value, letters] of ROMAN) {
      while (left >= value) { out += letters; left -= value; }
    }
    return out;
  }

  return { clamp, roundTo, compact, ordinal, percent, parse, between, lerp, sum, average, format, roman };
}
