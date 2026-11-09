// Taste rules held in code. Doable's lesson: a rule a prompt asks for leaks,
// a rule code checks holds. What a screen says is tidied wherever that is
// safe, on every screen as it runs (expandScreen calls tidyScreen): emoji go
// from its titles, labels and buttons, and an em dash becomes a colon or a
// comma. What cannot be tidied is a problem for the model (madeUp): ratings,
// review counts and customer numbers no row holds, and placeholder text.

// The words people read: the values of these keys, in a screen's blocks.
const TEXT_KEYS = new Set(['h', 'text', 'title', 'label', 'button', 'empty', 'hint', 'placeholder', 'sub', 'subtitle', 'lead', 'note', 'caption', 'tagline', 'heading', 'confirm', 'cta', 'submit', 'about', 'q', 'a', 'name', 'role', 'per', 'badge']);
// Keys whose values are never words to tidy (queries, code, state).
const SKIP = new Set(['data', 'state', 'let', 'code', 'sql', 'params', 'fields', 'set', 'options', 'value', 'values', 'tones', 'photo', 'photos', 'icon', 'do', 'open', 'onClick', 'onSelect', 'href', 'class', 'format', 'of', 'form', 'table', 'key', 'slug']);
// "name" and "badge" are words only on front-page blocks (a quote's name, a
// pricing tier's badge); elsewhere they name a column.
const FRONT_WORDS = new Set(['name', 'role', 'per', 'badge']);
const FRONT = new Set(['quotes', 'pricing', 'features', 'steps', 'team', 'nav', 'footer', 'hero', 'band', 'faq', 'stats', 'gallery', 'carousel', 'media', 'marquee']);

// An emoji as drawn (with its modifiers and joined parts); ©, ® and ™, arrows
// and check marks drawn as text are not emoji.
const EMOJI = /(?:\p{Emoji_Presentation}|\p{Extended_Pictographic}️)(?:\p{Emoji_Modifier}|️|‍(?:\p{Emoji_Presentation}|\p{Extended_Pictographic})️?)*/gu;
const DASH = /\s+[—–]\s+|—/g;

// One piece of words, tidied: emoji out (unless nothing else is left), and
// em dashes turned into a colon (the first in a short label) or a comma.
export function tidyWords(t, short = false) {
  if (typeof t !== 'string' || !t || t.startsWith('=')) return t;
  if (!/[—–]|\p{Extended_Pictographic}/u.test(t)) return t;
  // Templates keep what is inside {{...}}: data, not words.
  const parts = t.split(/(\{\{[\s\S]*?\}\})/);
  let dashes = 0;
  const out = parts.map((p, i) => {
    if (i % 2) return p;
    let x = p.replace(EMOJI, ' ');
    x = x.replace(DASH, (m) => (m === '—' || /—|\s–\s/.test(m) ? ((short && dashes++ === 0) ? ': ' : ', ') : m));
    return x;
  }).join('');
  const clean = out.replace(/[ \t]{2,}/g, ' ').replace(/\s+([,:.!?])/g, '$1').replace(/([,:])\1+/g, '$1').replace(/^[\s,:]+|[\s,:]+$/g, '');
  return clean || t;
}

const words = (x) => typeof x === 'string' && !x.startsWith('=');

// A screen with its words tidied, the same object when nothing changes.
export function tidyScreen(screen) {
  if (!screen || typeof screen !== 'object') return screen;
  const json = JSON.stringify(screen);
  if (!/[—–]|\p{Extended_Pictographic}/u.test(json)) return screen;
  let changed = false;
  const walk = (x, key, front) => {
    if (Array.isArray(x)) {
      let any = false;
      const out = x.map((y) => { const z = walk(y, key, front); if (z !== y) any = true; return z; });
      return any ? out : x;
    }
    if (x && typeof x === 'object') {
      let any = false;
      const out = {};
      const isFront = front || Object.keys(x).some((k) => FRONT.has(k));
      for (const [k, v] of Object.entries(x)) {
        let nv = v;
        if (SKIP.has(k) && !(k === 'value' && isFront)) nv = v;
        else if (typeof v === 'string') {
          if (TEXT_KEYS.has(k) && (!FRONT_WORDS.has(k) || isFront)) nv = tidyWords(v, ['h', 'title', 'label', 'button', 'submit', 'cta', 'heading', 'badge', 'name', 'role'].includes(k));
        } else nv = walk(v, k, isFront || FRONT.has(k));
        if (nv !== v) any = true;
        out[k] = nv;
      }
      return any ? out : x;
    }
    // A bare string in a block list is a paragraph.
    if (typeof x === 'string' && (key === 'body' || key === 'card' || key === 'section' || key === 'col' || key === 'row' || key === 'then' || key === 'else')) return tidyWords(x);
    return x;
  };
  const out = { ...screen, body: walk(screen.body, 'body', false) };
  for (const k of Object.keys(out)) if (k !== 'body' && out[k] !== screen[k]) changed = true;
  if (out.body !== screen.body) changed = true;
  return changed ? out : screen;
}

// A view's title (its tab) tidied.
export const tidyTitle = (t) => (typeof t === 'string' ? tidyWords(t, true) : t);

// Ratings, review counts and customer numbers written into a screen as
// words: the app's rows do not hold them, so they are made up.
const RATING = /\b[1-5][.,]\d\s*(?:★|⭐|\/\s*5\b|out of 5\b|stars?\b)|★{2,}|⭐|\b[1-5](?:[.,]\d)?\s*(?:-|\s)?stars?\b(?!\s*(?:of|for)\b)/i;
const COUNT = /\b\d[\d,.]*\s*(?:k|K|m|M)?\+?\s*(?:reviews|ratings|(?:happy|satisfied|loyal)\s+(?:customers|clients|guests|diners|users|members)|five-star)\b/i;
const CLAIM_LABEL = /\b(?:customers|clients|guests|diners|reviews?|ratings?|stars|users|downloads|happy)\b/i;
const PLACEHOLDER = /lorem ipsum|\bdolor sit amet\b|placeholder text|your (?:text|title|tagline|headline) here|sample text/i;
const MARKER = /\b(?:TBD|TODO|FIXME)\b/;

export function madeUp(screen) {
  const found = new Set();
  const say = (what) => found.add(what);
  const walk = (x, key) => {
    if (Array.isArray(x)) { x.forEach((y) => walk(y, key)); return; }
    if (x && typeof x === 'object') {
      // A big number with a claim for its label ("4.9", "average rating").
      if (key === 'stats' && (typeof x.value === 'number' || words(x.value)) && words(x.label) && CLAIM_LABEL.test(x.label)) say(`a number for "${x.label}" that no row holds (${String(x.prefix || '')}${x.value}${String(x.suffix || '')})`);
      if (key === 'quotes' && typeof x.stars === 'number') say('star ratings on quotes');
      for (const [k, v] of Object.entries(x)) if (!SKIP.has(k) || k === 'value') walk(v, k);
      return;
    }
    if (!words(x)) return;
    if (RATING.test(x)) say(`a rating ("${x.match(RATING)[0].trim()}")`);
    else if (COUNT.test(x)) say(`a count ("${x.match(COUNT)[0].trim()}")`);
    const ph = x.match(PLACEHOLDER) || x.match(MARKER);
    if (ph) say(`placeholder text ("${ph[0]}")`);
  };
  walk(screen?.body, 'body');
  return [...found];
}
