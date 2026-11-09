// title: Phone numbers
// description: Reads, checks and prints phone numbers as people type them (contact lists, sign-up forms, call links, masked numbers on a receipt), for North America and for numbers with a country code.
// api: clean(text) -> the digits with a leading + kept when there was one ('' when none); an extension ('x12', 'ext. 12', '#12') and a leading 'tel:' are dropped,
//   so is a '(0)' after a country code ('+44 (0)20 ...');
//   e164(text, country = 'US') -> '+14155550123' or null when it is not a number it can read; a number without + is read as national to country
//   (its trunk prefix such as 0 or 1 dropped), and '00' (or '011' in North America) starts an international number;
//   isValid(text, country = 'US') -> true when e164 gives a number: the right length for known countries, North American area and exchange codes starting 2-9;
//   format(text, {country = 'US'}) -> '(415) 555-0123' for North American numbers when country is US, CA or PR,
//   '+44 20 7946 0958' style groups otherwise; the text as given, trimmed, when it cannot be read ('' for none);
//   tel(text, country = 'US') -> 'tel:+14155550123' (the cleaned digits when not valid, '' when there are none);
//   mask(text) -> '•••• 0123': the last 4 digits only ('' when none; all dots when 4 or fewer)
function helper() {
  const range = (a, b) => { const r = []; for (let i = a; i <= b; i++) r.push(i); return r; };
  // country: [calling code, trunk prefix, national lengths, groups (sizes, or a function of the national number)]
  const C = {
    US: ['1', '1', [10]], CA: ['1', '1', [10]], PR: ['1', '1', [10]],
    GB: ['44', '0', [9, 10], (n) => n.length === 9 ? [4, 5] : n[0] === '2' ? [2, 4, 4] : n[0] === '7' ? [4, 6]
      : n[0] === '1' ? (n[1] === '1' || n[2] === '1' ? [3, 3, 4] : [4, 6]) : [3, 3, 4]],
    IE: ['353', '0', [7, 8, 9]],
    FR: ['33', '0', [9], [1, 2, 2, 2, 2]],
    DE: ['49', '0', range(6, 11)],
    ES: ['34', '', [9], [3, 3, 3]],
    IT: ['39', '', range(6, 11)],
    PT: ['351', '', [9], [3, 3, 3]],
    NL: ['31', '0', [9], (n) => (n[0] === '6' ? [1, 8] : [2, 3, 4])],
    BE: ['32', '0', [8, 9]],
    CH: ['41', '0', [9], [2, 3, 2, 2]],
    SE: ['46', '0', [7, 8, 9]],
    NO: ['47', '', [8], [3, 2, 3]],
    DK: ['45', '', [8], [2, 2, 2, 2]],
    PL: ['48', '', [9], [3, 3, 3]],
    RU: ['7', '8', [10], [3, 3, 2, 2]],
    AU: ['61', '0', [9], (n) => (n[0] === '4' ? [3, 3, 3] : [1, 4, 4])],
    NZ: ['64', '0', [8, 9, 10]],
    IN: ['91', '0', [10], [5, 5]],
    JP: ['81', '0', [9, 10], (n) => (n.length === 10 ? [2, 4, 4] : [1, 4, 4])],
    CN: ['86', '0', [10, 11], (n) => (n.length === 11 ? [3, 4, 4] : [2, 4, 4])],
    HK: ['852', '', [8], [4, 4]],
    SG: ['65', '', [8], [4, 4]],
    BR: ['55', '0', [10, 11], (n) => [2, n.length - 6, 4]],
    MX: ['52', '', [10], [2, 4, 4]],
    ZA: ['27', '0', [9], [2, 3, 4]],
  };
  const BYCODE = {};
  for (const k of Object.keys(C)) if (!BYCODE[C[k][0]]) BYCODE[C[k][0]] = C[k];
  const TWO = new Set(('20 27 30 31 32 33 34 36 39 40 41 43 44 45 46 47 48 49 51 52 53 54 55 56 57 58 '
    + '60 61 62 63 64 65 66 81 82 84 86 90 91 92 93 94 95 98').split(' '));
  const GENERIC = { 5: [2, 3], 6: [3, 3], 7: [3, 4], 8: [4, 4], 9: [3, 3, 3], 10: [3, 3, 4], 11: [3, 4, 4], 12: [4, 4, 4], 13: [3, 3, 3, 4], 14: [3, 3, 4, 4] };

  function clean(text) {
    if (text == null || typeof text === 'object') return '';
    let s = String(text).replace(/^\s*tel:/i, '');
    s = s.replace(/\s*(?:ext\.?|extension|x|#)\s*\d+\s*$/i, '');
    const plus = /^\s*\(?\s*\+/.test(s);
    if (plus) s = s.replace(/\(\s*0\s*\)/g, '');
    const d = s.replace(/\D/g, '');
    return d ? (plus ? '+' : '') + d : '';
  }
  function splitCode(d) {
    if (!d || d[0] === '0') return null;
    if (d[0] === '1' || d[0] === '7') return d[0];
    if (TWO.has(d.slice(0, 2))) return d.slice(0, 2);
    return d.length > 3 ? d.slice(0, 3) : null;
  }
  function check(cc, nat) {
    const info = BYCODE[cc];
    let n = nat;
    if (info) {
      const [, trunk, lens] = info;
      // a 0 or 1 trunk never starts a national number, so drop it whenever what is left fits
      if (trunk && n.startsWith(trunk) && lens.includes(n.length - trunk.length)
        && (!lens.includes(n.length) || trunk === '0' || trunk === '1')) n = n.slice(trunk.length);
      if (!lens.includes(n.length)) return null;
      if (cc === '1' && !/^[2-9]\d\d[2-9]/.test(n)) return null;
      if (trunk === '0' && n[0] === '0') return null;
    }
    if (n.length < 4 || cc.length + n.length > 15) return null;
    return { cc, nat: n, info };
  }
  function parse(text, country) {
    const s = clean(text);
    if (!s) return null;
    const home = C[String(country || 'US').toUpperCase()];
    let d = s.replace('+', '');
    let intl = s[0] === '+';
    if (!intl) {
      const idd = home && home[0] === '1' ? '011' : '00';
      if (d.startsWith(idd)) { d = d.slice(idd.length); intl = true; }
    }
    if (intl) {
      const cc = splitCode(d);
      return cc ? check(cc, d.slice(cc.length)) : null;
    }
    return home ? check(home[0], d) : null;
  }
  function e164(text, country = 'US') {
    const p = parse(text, country);
    return p ? '+' + p.cc + p.nat : null;
  }
  function isValid(text, country = 'US') { return parse(text, country) !== null; }
  function group(n, sizes) {
    const out = [];
    let i = 0;
    for (const k of sizes) { if (i >= n.length) break; out.push(n.slice(i, i + k)); i += k; }
    if (i < n.length) out.push(n.slice(i));
    return out.join(' ');
  }
  function format(text, opts = {}) {
    const country = String((typeof opts === 'string' ? opts : opts && opts.country) || 'US').toUpperCase();
    const p = parse(text, country);
    if (!p) return text == null || typeof text === 'object' ? '' : String(text).trim();
    const n = p.nat;
    if (p.cc === '1') {
      const home = C[country];
      if (home && home[0] === '1') return `(${n.slice(0, 3)}) ${n.slice(3, 6)}-${n.slice(6)}`;
      return `+1 ${n.slice(0, 3)} ${n.slice(3, 6)} ${n.slice(6)}`;
    }
    const g = p.info && p.info[3];
    const sizes = (typeof g === 'function' ? g(n) : g) || GENERIC[n.length] || [n.length];
    return `+${p.cc} ${group(n, sizes)}`;
  }
  function tel(text, country = 'US') {
    const e = e164(text, country);
    if (e) return 'tel:' + e;
    const c = clean(text);
    return c ? 'tel:' + c : '';
  }
  function mask(text) {
    const d = clean(text).replace('+', '');
    if (!d) return '';
    if (d.length <= 4) return '•'.repeat(d.length);
    return '•••• ' + d.slice(-4);
  }
  return { clean, format, e164, isValid, tel, mask };
}
