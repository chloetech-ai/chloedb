// title: Addresses
// description: Postal addresses as plain values: print them as lines or one line in US or European order, read a typed address back into its parts, link to it on OpenStreetMap, and tell whether two addresses are the same (contacts, deliveries, invoices, venues).
// api: format({name, street, street2, city, region, postal, country}, {oneLine = false, style = 'US' | 'EU'}) -> the non-empty lines, or one line joined with ', ' when oneLine
//   (US: 'Springfield, IL 62704'; EU: '10115 Berlin' then the region on its own line); [] or '' when there is no address;
//   parse(text) -> best-effort {street, city, region, postal, country} (missing parts are '') from 'street, city, ST 12345[, country]',
//   'street, 75001 Paris, France', 'street, city, ON M5V 2T6' and the like (commas or new lines between parts), or null when there is no text;
//   mapsUrl(address) -> 'https://www.openstreetmap.org/search?query=...' for an address object (its name left out) or text, or null when empty;
//   same(a, b) -> true when a and b (objects or text) are the same address ignoring case, spaces, punctuation and abbreviations
//   (St/Street, Ave/Avenue, Apt/#, N/North, ...); the street must be given on both; city, region, postal and country are compared when both have them
//   (a ZIP+4 matches its 5-digit ZIP)
function helper() {
  const STATES = new Set(('AL AK AZ AR CA CO CT DE FL GA HI ID IL IN IA KS KY LA ME MD MA MI MN MS MO MT NE NV NH NJ NM NY NC ND OH OK OR PA RI SC SD TN TX UT VT VA WA WV WI WY ' +
    'DC PR GU VI AS MP AB BC MB NB NL NS NT NU ON PE QC SK YT').split(' '));
  const COUNTRIES = {
    'usa': 'us', 'us': 'us', 'u s a': 'us', 'u s': 'us', 'united states': 'us', 'united states of america': 'us', 'america': 'us',
    'uk': 'gb', 'gb': 'gb', 'u k': 'gb', 'united kingdom': 'gb', 'great britain': 'gb', 'england': 'gb', 'scotland': 'gb', 'wales': 'gb',
    'canada': 'ca', 'germany': 'de', 'deutschland': 'de', 'france': 'fr', 'spain': 'es', 'españa': 'es', 'italy': 'it', 'italia': 'it',
    'netherlands': 'nl', 'the netherlands': 'nl', 'holland': 'nl', 'belgium': 'be', 'switzerland': 'ch', 'austria': 'at', 'ireland': 'ie',
    'portugal': 'pt', 'sweden': 'se', 'norway': 'no', 'denmark': 'dk', 'finland': 'fi', 'poland': 'pl', 'australia': 'au', 'mexico': 'mx',
  };
  const ABBR = {
    street: 'st', avenue: 'ave', av: 'ave', road: 'rd', boulevard: 'blvd', drive: 'dr', lane: 'ln', court: 'ct', place: 'pl',
    square: 'sq', terrace: 'ter', parkway: 'pkwy', highway: 'hwy', circle: 'cir', crescent: 'cres', trail: 'trl', alley: 'aly',
    expressway: 'expy', freeway: 'fwy', plaza: 'plz', point: 'pt', heights: 'hts', center: 'ctr', centre: 'ctr', junction: 'jct',
    route: 'rte', apartment: 'apt', suite: 'ste', floor: 'fl', building: 'bldg', room: 'rm', department: 'dept',
    north: 'n', south: 's', east: 'e', west: 'w', northeast: 'ne', northwest: 'nw', southeast: 'se', southwest: 'sw',
    mount: 'mt', fort: 'ft', saint: 'st',
  };
  const RX_NA = /^(?:(.+?)\s+)?([A-Za-z]{2})\.?\s+(\d{5}(?:-\d{4})?|[A-Za-z]\d[A-Za-z] ?\d[A-Za-z]\d)$/;
  const RX_EU = /^((?:[A-Za-z]{1,2}-)?(?:\d{4} ?[A-Z]{2}(?= )|\d{2}-\d{3}|\d{3} \d{2}|\d{4,5}))\s+(\D.*)$/;
  const RX_TAIL = /^(\D+?)\s+(\d{4,6})$/;

  const str = (v) => (v == null || typeof v === 'object' || typeof v === 'function' ? '' : String(v).replace(/\s+/g, ' ').trim());
  const key = (s) => str(s).toLowerCase().replace(/[^a-z0-9\u00c0-\u024f]+/g, ' ').trim();
  const isState = (s) => STATES.has(str(s).toUpperCase());
  const up2 = (s) => (/^[A-Za-z]{2}$/.test(s) ? s.toUpperCase() : s);

  function format(address, opts) {
    const { oneLine = false, style = 'US' } = opts || {};
    if (!address || typeof address !== 'object') return oneLine ? '' : [];
    const a = {};
    for (const k of ['name', 'street', 'street2', 'city', 'region', 'postal', 'country']) a[k] = str(address[k]);
    const lines = [a.name, a.street, a.street2];
    if (String(style).toUpperCase() === 'EU') {
      lines.push([a.postal, a.city].filter(Boolean).join(' '), a.region);
    } else {
      const right = [a.region, a.postal].filter(Boolean).join(' ');
      lines.push(a.city && right ? `${a.city}, ${right}` : a.city || right);
    }
    lines.push(a.country);
    const out = lines.filter(Boolean);
    return oneLine ? out.join(', ') : out;
  }

  function parse(text) {
    if (typeof text !== 'string') return null;
    const parts = text.split(/[\n,;]/).map(str).filter(Boolean);
    if (!parts.length) return null;
    const out = { street: '', city: '', region: '', postal: '', country: '' };
    const last = parts[parts.length - 1];
    if (parts.length >= 2 && !/\d/.test(last) && !isState(last) &&
      (COUNTRIES[key(last)] || (parts.length >= 3 && /\d/.test(parts[parts.length - 2])) || parts.length >= 4)) {
      out.country = parts.pop();
    }
    if (parts.length >= 2 || (parts.length && out.country)) {
      const tail = parts[parts.length - 1];
      let m;
      if ((m = tail.match(RX_NA))) {
        parts.pop();
        out.region = m[2].toUpperCase();
        out.postal = m[3].toUpperCase();
        if (m[1]) out.city = m[1];
      } else if ((m = tail.match(RX_EU))) {
        parts.pop();
        out.postal = m[1].toUpperCase();
        out.city = m[2].trim();
      } else if ((m = tail.match(RX_TAIL))) {
        parts.pop();
        out.postal = m[2];
        if (parts.length >= 2 || (parts.length === 1 && !/\d/.test(parts[0]))) out.region = up2(m[1]);
        else out.city = m[1];
      } else if (isState(tail) || (/^[A-Za-z]{2}$/.test(tail) && parts.length >= 3)) {
        parts.pop();
        out.region = tail.toUpperCase();
      }
    }
    if (!out.city) {
      if (parts.length >= 2) out.city = parts.pop();
      else if (parts.length === 1 && !/\d/.test(parts[0]) && (out.region || out.postal || out.country)) out.city = parts.pop();
    }
    out.street = parts.join(', ');
    return out;
  }

  function mapsUrl(address) {
    let q = '';
    if (typeof address === 'string') q = str(address);
    else if (address && typeof address === 'object') q = format(Object.assign({}, address, { name: '' }), { oneLine: true });
    return q ? 'https://www.openstreetmap.org/search?query=' + encodeURIComponent(q) : null;
  }

  function norm(s) {
    return str(s).toLowerCase().replace(/#\s*/g, ' apt ').replace(/[^a-z0-9\u00c0-\u024f]+/g, ' ')
      .split(' ').filter(Boolean)
      .map((w) => ABBR[w] || w.replace(/stra(?:ss|ß)e$/, 'str'))
      .join(' ');
  }
  function postalKey(p) {
    const k = str(p).toUpperCase().replace(/[\s-]/g, '');
    return /^\d{5}(\d{4})?$/.test(k) ? k.slice(0, 5) : k;
  }
  function countryKey(c) { const k = key(c); return COUNTRIES[k] || k; }
  function toAddr(x) {
    if (typeof x === 'string') return parse(x);
    return x && typeof x === 'object' ? x : null;
  }

  function same(a, b) {
    const A = toAddr(a);
    const B = toAddr(b);
    if (!A || !B) return false;
    const sa = norm([A.street, A.street2].map(str).filter(Boolean).join(' '));
    const sb = norm([B.street, B.street2].map(str).filter(Boolean).join(' '));
    if (!sa || !sb || sa !== sb) return false;
    const checks = [[norm, 'city'], [norm, 'region'], [postalKey, 'postal'], [countryKey, 'country']];
    for (const [fn, field] of checks) {
      const x = fn(A[field]);
      const y = fn(B[field]);
      if (x && y && x !== y) return false;
    }
    return true;
  }

  return { format, parse, mapsUrl, same };
}
