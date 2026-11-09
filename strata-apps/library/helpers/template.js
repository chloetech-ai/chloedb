// title: Templates
// description: Fills text templates with data ({name}, {customer.name}, {amount|money}) for emails, reminders, invoices, labels and mail merges, lists the fields a template uses and which are missing, and makes text HTML-safe.
// api: fill(template, data, {missing}) -> the filled text ('' when template is not text); a field is {key} or {key|filter|filter:arg};
//   keys may be nested ({customer.name}, {items.0.title}; a key holding the dots itself, like a CSV header 'customer.name', wins);
//   missing: text to put for a field with no value (null or absent), or a function (key) -> text; default leaves the field as written;
//   filters, applied left to right: money (money:EUR for another currency, default USD) -> '$1,234.50'; upper; lower;
//   date -> '8 Nov 2026' from 'YYYY-MM-DD' or an ISO time; number (number:2 for fixed places, default at most 2) -> '1,234.5';
//   default:x -> x when the value is missing or ''; plural:word or plural:word,words -> '1 item', '3 items';
//   unknown filters and values a filter cannot read pass through unchanged; lists print joined with ', ';
//   '{{' and '}}' print literal braces; text in braces that is not a key ('{ a b }') stays as it is;
//   fields(template) -> [keys] the template uses, in order of first use, each once;
//   check(template, data) -> [keys] the template uses that data has no value for (null or absent), [] when none;
//   mailMerge(template, rows, {missing}) -> [texts], one fill per row ([] when rows is not a list);
//   escape(text) -> the text with & < > " ' as HTML entities ('' for null)
function helper() {
  const MONTHS = ['Jan', 'Feb', 'Mar', 'Apr', 'May', 'Jun', 'Jul', 'Aug', 'Sep', 'Oct', 'Nov', 'Dec'];
  const KEY = /^[A-Za-z_$][\w$]*(\.[\w$]+)*$/;
  const SYMBOLS = { USD: '$', EUR: '€', GBP: '£', JPY: '¥', INR: '₹', CNY: '¥', KRW: '₩', CAD: 'CA$', AUD: 'A$', CHF: 'CHF ' };
  const pad = (n) => String(n).padStart(2, '0');
  const tokens = () => /\{\{|\}\}|\{([^{}]*)\}/g;

  function parse(inner) {
    const parts = inner.split('|');
    const key = parts[0].trim();
    if (!KEY.test(key)) return null;
    const filters = parts.slice(1).map((p) => {
      const i = p.indexOf(':');
      return i < 0 ? { name: p.trim().toLowerCase(), arg: null } : { name: p.slice(0, i).trim().toLowerCase(), arg: p.slice(i + 1).trim() };
    });
    return { key, filters };
  }
  function lookup(data, key) {
    if (data == null || typeof data !== 'object') return undefined;
    if (Object.prototype.hasOwnProperty.call(data, key)) return data[key];
    let v = data;
    for (const p of key.split('.')) {
      if (v == null || typeof v !== 'object') return undefined;
      v = v[p];
    }
    return v;
  }
  function toNum(v) {
    if (typeof v === 'number') return isFinite(v) ? v : null;
    if (typeof v === 'string' && v.trim() !== '') { const n = Number(v.trim()); return isFinite(n) ? n : null; }
    return null;
  }
  const group = (s) => s.replace(/\B(?=(\d{3})+(?!\d))/g, ',');
  function grouped(n, places) {
    const text = places == null ? String(Math.round(Math.abs(n) * 100) / 100) : Math.abs(n).toFixed(places);
    if (/e/i.test(text)) return String(n);
    const [i, f] = text.split('.');
    const neg = n < 0 && Number(text) !== 0;
    return (neg ? '-' : '') + group(i) + (f ? '.' + f : '');
  }
  function money(n, currency) {
    const cur = (currency || 'USD').toUpperCase();
    try {
      if (typeof lib !== 'undefined' && lib.money && lib.money.format) return lib.money.format(n, cur);
    } catch (e) { /* fall back below */ }
    const sym = SYMBOLS[cur] || cur + ' ';
    return (n < 0 && Math.abs(n) >= 0.005 ? '-' : '') + sym + grouped(n, 2).replace(/^-/, '');
  }
  function date(v) {
    if (typeof v !== 'string') return v;
    let y; let m; let d;
    if (/^\d{4}-\d{2}-\d{2}$/.test(v)) { y = Number(v.slice(0, 4)); m = Number(v.slice(5, 7)); d = Number(v.slice(8, 10)); }
    else if (/^\d{4}-\d{2}-\d{2}T\d{2}:\d{2}/.test(v)) {
      const t = new Date(v);
      if (isNaN(t.getTime())) return v;
      y = t.getFullYear(); m = t.getMonth() + 1; d = t.getDate();
    } else return v;
    if (m < 1 || m > 12 || d < 1 || d > 31) return v;
    return `${d} ${MONTHS[m - 1]} ${y}`;
  }
  function str(v) {
    if (v == null) return '';
    if (Array.isArray(v)) return v.map(str).filter((x) => x !== '').join(', ');
    if (typeof v === 'object') { try { return JSON.stringify(v); } catch (e) { return ''; } }
    return String(v);
  }
  function apply(v, { name, arg }) {
    if (name === 'default') return v == null || v === '' ? (arg == null ? '' : arg) : v;
    if (v == null) return v;
    const n = toNum(v);
    switch (name) {
      case 'upper': return str(v).toUpperCase();
      case 'lower': return str(v).toLowerCase();
      case 'money': return n == null ? v : money(n, arg);
      case 'number': {
        if (n == null) return v;
        const places = arg != null && /^\d+$/.test(arg) ? Math.min(Number(arg), 20) : null;
        return grouped(n, places);
      }
      case 'date': return date(v);
      case 'plural': {
        if (n == null || !arg) return v;
        const [word, many] = arg.split(',').map((s) => s.trim());
        return `${n} ${n === 1 ? word : many || word + 's'}`;
      }
      default: return v;
    }
  }
  function missingText(opts, key, written) {
    const m = opts.missing;
    if (typeof m === 'function') { try { return str(m(key)); } catch (e) { return ''; } }
    if (m === undefined) return written;
    return str(m);
  }

  function fill(template, data, opts) {
    if (typeof template !== 'string') return '';
    const o = opts || {};
    return template.replace(tokens(), (m, inner) => {
      if (m === '{{') return '{';
      if (m === '}}') return '}';
      const f = parse(inner);
      if (!f) return m;
      let v = lookup(data, f.key);
      for (const flt of f.filters) v = apply(v, flt);
      return v == null ? missingText(o, f.key, m) : str(v);
    });
  }
  function fields(template) {
    if (typeof template !== 'string') return [];
    const out = [];
    const re = tokens();
    let m;
    while ((m = re.exec(template))) {
      if (m[1] === undefined) continue;
      const f = parse(m[1]);
      if (f && !out.includes(f.key)) out.push(f.key);
    }
    return out;
  }
  function check(template, data) {
    return fields(template).filter((k) => lookup(data, k) == null);
  }
  function mailMerge(template, rows, opts) {
    if (!Array.isArray(rows)) return [];
    return rows.map((row) => fill(template, row, opts));
  }
  function escape(text) {
    if (text == null) return '';
    return String(text).replace(/&/g, '&amp;').replace(/</g, '&lt;').replace(/>/g, '&gt;').replace(/"/g, '&quot;').replace(/'/g, '&#39;');
  }
  return { fill, fields, check, mailMerge, escape };
}
