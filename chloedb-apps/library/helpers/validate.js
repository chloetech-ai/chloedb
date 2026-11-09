// title: Validation
// description: Checks what people type before it is saved: email addresses, phone numbers, links, dates, numbers in a range, lengths and required fields, with a message per field that says what to fix.
// api: check(values, rules) -> {ok, errors: {field: message}}, where rules is {field: 'required|email|min:3|max:80'}
//   (rules: required, email, url, phone, date (YYYY-MM-DD), number, int, min:N and max:N (the value with number or int, else the length),
//   in:a,b,c, match:regex; an array of rule strings or {rule, message} works too);
//   isEmail(s), isUrl(s), isPhone(s), isDate(s) -> bool; phone(s) -> digits with a leading + kept, or null;
//   url(s) -> the link with https:// added when missing, or null; email(s) -> trimmed and lower-cased, or null
function helper() {
  const EMAIL = /^[^\s@<>()",;:]+@[^\s@<>()",;:]+\.[a-z]{2,}$/i;
  const isEmail = (s) => EMAIL.test(String(s || '').trim());
  function url(s) {
    let t = String(s || '').trim();
    if (!t || /\s/.test(t)) return null;
    if (!/^[a-z][a-z0-9+.-]*:/i.test(t)) t = `https://${t}`;
    const m = t.match(/^(https?):\/\/([^/?#\s]+)([/?#].*)?$/i);
    if (!m || !/\.[a-z]{2,}$|^localhost(:\d+)?$|^\d+\.\d+\.\d+\.\d+(:\d+)?$/i.test(m[2].replace(/:\d+$/, ''))) return null;
    return t;
  }
  const isUrl = (s) => url(s) !== null;
  function phone(s) {
    const t = String(s || '').trim();
    if (!/^[+\d()\-.\s/]{6,}$/.test(t)) return null;
    const digits = t.replace(/\D/g, '');
    if (digits.length < 6 || digits.length > 15) return null;
    return (t.startsWith('+') ? '+' : '') + digits;
  }
  const isPhone = (s) => phone(s) !== null;
  function isDate(s) {
    const m = String(s || '').match(/^(\d{4})-(\d{2})-(\d{2})$/);
    if (!m) return false;
    const d = new Date(Date.UTC(+m[1], +m[2] - 1, +m[3]));
    return d.getUTCMonth() === +m[2] - 1 && d.getUTCDate() === +m[3];
  }
  const email = (s) => (isEmail(s) ? String(s).trim().toLowerCase() : null);
  const empty = (v) => v === null || v === undefined || (typeof v === 'string' && !v.trim()) || (Array.isArray(v) && !v.length);
  const label = (f) => String(f).replace(/_/g, ' ');
  function one(rule, v, field, numeric) {
    const [name, arg] = String(rule).split(/:(.*)/s);
    const L = label(field);
    switch (name) {
      case 'required': return empty(v) ? `${L} is needed` : null;
      case 'email': return !empty(v) && !isEmail(v) ? `${L} should be an email address, like name@example.com` : null;
      case 'url': return !empty(v) && !isUrl(v) ? `${L} should be a link, like example.com/page` : null;
      case 'phone': return !empty(v) && !isPhone(v) ? `${L} should be a phone number` : null;
      case 'date': return !empty(v) && !isDate(v) ? `${L} should be a date (YYYY-MM-DD)` : null;
      case 'number': return !empty(v) && !Number.isFinite(Number(v)) ? `${L} should be a number` : null;
      case 'int': return !empty(v) && !Number.isInteger(Number(v)) ? `${L} should be a whole number` : null;
      case 'min': case 'max': {
        if (empty(v)) return null;
        const n = Number(arg);
        const isNum = typeof v === 'number' || numeric;
        const size = isNum ? Number(v) : Array.isArray(v) ? v.length : String(v).length;
        if (name === 'min' && size < n) return isNum ? `${L} should be at least ${n}` : `${L} needs at least ${n} ${Array.isArray(v) ? 'items' : 'characters'}`;
        if (name === 'max' && size > n) return isNum ? `${L} should be at most ${n}` : `${L} takes at most ${n} ${Array.isArray(v) ? 'items' : 'characters'}`;
        return null;
      }
      case 'in': return !empty(v) && !String(arg).split(',').map((x) => x.trim()).includes(String(v)) ? `${L} should be one of ${String(arg).split(',').join(', ')}` : null;
      case 'match': return !empty(v) && !new RegExp(arg).test(String(v)) ? `${L} is not in the right form` : null;
      default: return null;
    }
  }
  function check(values, rules) {
    const errors = {};
    for (const [field, spec] of Object.entries(rules || {})) {
      const list = Array.isArray(spec) ? spec : String(spec).split('|').filter(Boolean);
      const names = list.map((r) => String(typeof r === 'object' ? r.rule : r).split(':')[0]);
      const numeric = names.includes('number') || names.includes('int');
      for (const r of list) {
        const rule = typeof r === 'object' ? r.rule : r;
        const msg = one(rule, (values || {})[field], field, numeric);
        if (msg) { errors[field] = typeof r === 'object' && r.message ? r.message : msg.charAt(0).toUpperCase() + msg.slice(1); break; }
      }
    }
    return { ok: !Object.keys(errors).length, errors };
  }
  return { check, isEmail, isUrl, isPhone, isDate, phone, url, email };
}
