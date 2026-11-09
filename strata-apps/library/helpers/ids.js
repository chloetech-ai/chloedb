// title: IDs and codes
// description: Makes identifiers and reference codes: short url-safe ids for rows and links, v4 UUIDs, numbered codes like invoice or ticket numbers and the next one in a series, readable codes without look-alike characters for vouchers and bookings, and Luhn check digits.
// api: short(n = 8) -> a random url-safe id of n characters from A-Z a-z 0-9 _ - ('' when n < 1);
//   uuid() -> an RFC 4122 version 4 UUID, 'xxxxxxxx-xxxx-4xxx-yxxx-xxxxxxxxxxxx' in lower-case hex;
//   code(prefix, n, width = 4) -> 'INV-0042' (a '-' joins a prefix ending in a letter or digit, none otherwise: 'INV/0042';
//   no prefix gives '0042'; numbers wider than width are kept whole; null when n is not a whole number >= 0);
//   next(codes, prefix, width = 4) -> the code after the highest of codes with that prefix ('INV-0043'),
//   or the first ('INV-0001') when none match; codes with another prefix or not ending in digits are ignored;
//   readable(n = 6) -> a random code of n characters from 2-9 and A-Z without 0, O, 1, I or L, easy to read out and type ('' when n < 1);
//   check(code) -> the Luhn check digit (0-9) to append to the digits of code (other characters ignored), or null when it has no digits;
//   verify(code) -> true when the last digit of code is a correct Luhn check digit for the digits before it;
//   all randomness comes from Math.random: fine for ids and codes, not for secrets or passwords
function helper() {
  const URL_SAFE = 'ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789_-';
  const READABLE = '23456789ABCDEFGHJKMNPQRSTUVWXYZ';
  const HEX = '0123456789abcdef';
  const MAX = 1024;

  const count = (n, fallback) => {
    if (n === undefined || n === null || n === '') return fallback;
    const v = Math.floor(Number(n));
    return Number.isFinite(v) ? Math.min(Math.max(v, 0), MAX) : 0;
  };
  const pick = (alphabet, n) => {
    let out = '';
    for (let i = 0; i < n; i++) out += alphabet[Math.floor(Math.random() * alphabet.length)];
    return out;
  };
  const leadOf = (prefix) => {
    const p = prefix === undefined || prefix === null ? '' : String(prefix).trim();
    return p && /[A-Za-z0-9]$/.test(p) ? p + '-' : p;
  };
  const widthOf = (width) => {
    const w = Math.floor(Number(width));
    return Number.isFinite(w) && w >= 0 ? Math.min(w, 32) : 4;
  };

  function short(n) { return pick(URL_SAFE, count(n, 8)); }

  function readable(n) { return pick(READABLE, count(n, 6)); }

  function uuid() {
    let s = '';
    for (let i = 0; i < 32; i++) {
      let c = HEX[Math.floor(Math.random() * 16)];
      if (i === 12) c = '4';
      else if (i === 16) c = HEX[8 + Math.floor(Math.random() * 4)];
      s += c;
    }
    return `${s.slice(0, 8)}-${s.slice(8, 12)}-${s.slice(12, 16)}-${s.slice(16, 20)}-${s.slice(20)}`;
  }

  function code(prefix, n, width = 4) {
    if (n === null || n === undefined || n === '' || typeof n === 'boolean') return null;
    const v = Number(n);
    if (!Number.isFinite(v) || v < 0 || Math.floor(v) !== v) return null;
    return leadOf(prefix) + String(v).padStart(widthOf(width), '0');
  }

  function next(codes, prefix, width = 4) {
    const lead = leadOf(prefix);
    let max = 0;
    for (const item of Array.isArray(codes) ? codes : []) {
      if (item === null || item === undefined) continue;
      const s = String(item).trim();
      if (!s.startsWith(lead)) continue;
      const rest = s.slice(lead.length);
      if (!/^\d+$/.test(rest)) continue;
      const v = parseInt(rest, 10);
      if (v > max) max = v;
    }
    return code(prefix, max + 1, width);
  }

  const digitsOf = (c) => (c === null || c === undefined ? '' : String(c).replace(/\D/g, ''));
  // Luhn sum of digits, doubling from the right at positions of parity `first` (0 = rightmost)
  const luhnSum = (digits, first) => {
    let sum = 0;
    for (let i = digits.length - 1, pos = 0; i >= 0; i--, pos++) {
      let d = digits.charCodeAt(i) - 48;
      if (pos % 2 === first) { d *= 2; if (d > 9) d -= 9; }
      sum += d;
    }
    return sum;
  };

  function check(c) {
    const digits = digitsOf(c);
    if (!digits) return null;
    return (10 - (luhnSum(digits, 0) % 10)) % 10;
  }

  function verify(c) {
    const digits = digitsOf(c);
    if (digits.length < 2) return false;
    return luhnSum(digits, 1) % 10 === 0;
  }

  return { short, uuid, code, next, readable, check, verify };
}
