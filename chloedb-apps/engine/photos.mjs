// Photos for front pages. A screen names a photo by what it shows
// ({"hero": "...", "photo": "coffee shop interior"}), and when the version is
// stored each such block gets "src" (and "credit" when the photo's license
// asks for one); the query stays, so a later change can ask again. Example
// rows may hold "photo:a query" in a column, which becomes a URL likewise.
//
// Sources, in order:
// - Unsplash's API, when STRATA_APPS_UNSPLASH_KEY holds an access key (free
//   at unsplash.com/developers): any subject, credited as its guidelines ask;
// - the built-in set, library/photos.json: 304 Unsplash photos (free to use,
//   no credit needed), each checked by eye and described in words, matched on
//   those words;
// - Openverse (openly licensed photos, no key), for a subject the built-in
//   set does not have; credited. STRATA_APPS_PHOTOS=builtin keeps to the set.
// A block whose photo is found nowhere is drawn with the design's gradient.

import { readFileSync } from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const DIR = path.join(path.dirname(fileURLToPath(import.meta.url)), '..', 'library');
const UNSPLASH_KEY = process.env.STRATA_APPS_UNSPLASH_KEY || '';
const OPEN = process.env.STRATA_APPS_PHOTOS !== 'builtin';

// How wide a photo is drawn, by where it goes (twice the CSS pixels).
export const WIDTHS = { hero: 2400, band: 2000, media: 1400, slide: 2000, card: 900, gallery: 1400, avatar: 240 };

const STOP = new Set('a an the of with and in on for at by to from photo photos image picture pictures shot view some our your my their its this that'.split(' '));
const IRREGULAR = { loaves: 'loaf', leaves: 'leaf', shelves: 'shelf', knives: 'knife', halves: 'half', wolves: 'wolf', scarves: 'scarf',
  children: 'child', people: 'person', men: 'man', women: 'woman', geese: 'goose', feet: 'foot', teeth: 'tooth', mice: 'mouse' };
const stem = (w) => (IRREGULAR[w] || w).replace(/ies$/, 'y').replace(/(?<=[a-z]{3})(es|s)$/, (m) => (m === 'es' ? 'e' : '')).replace(/e$/, '');
export const words = (text) => String(text || '').toLowerCase().split(/[^a-z0-9]+/).filter((w) => w && !STOP.has(w)).map(stem);

let SET = null;
export function builtinPhotos() {
  if (!SET) {
    let list = [];
    try { list = JSON.parse(readFileSync(path.join(DIR, 'photos.json'), 'utf8')); } catch { list = []; }
    SET = list.map((p) => ({ ...p, tagWords: new Set(words(p.tags)), aboutWords: new Set(words(p.about)) }));
  }
  return SET;
}

const unsplashUrl = (id, use) => `https://images.unsplash.com/photo-${id}?auto=format&fit=crop&q=80&w=${WIDTHS[use] || 1400}${use === 'avatar' ? `&h=${WIDTHS.avatar}&crop=faces` : ''}`;

// Words that say how a thing looks rather than what it is: "dark roast",
// "minimal desk", "close-up of latte art". They help pick between photos but
// never make one fit on their own (a dark abstract is not a dark roast).
const MOD = new Set(words(`dark light bright fresh modern happy beautiful cozy cosy minimal minimalist
  elegant premium luxury luxurious new old vintage rustic small large big close up closeup top overhead aerial
  colorful colourful warm cool soft bold simple clean professional young smiling stylish organic natural
  local handmade artisan fine best classic golden sunny moody dramatic white black red blue green pink purple
  yellow orange brown grey gray single flat lay detail various assorted hero background banner nice cute
  lovely delicious tasty perfect vibrant calm peaceful serene quiet busy friendly wide angle portrait
  landscape square image style styled studio boutique charming historic traditional authentic gourmet homemade`));

// The built-in photos that fit a query best: each word of the query that a
// photo's tags hold counts 2, one its description holds 1.5 (a look word 0.5).
// A photo fits when it has the query's subject (its first word that names a
// thing: "yoga class", "dark roast coffee"), two of its words ("house blend
// coffee bag") or more than half of them, so "kayaking river" does not get a
// city on a river. A photo already on the page is passed over while another
// fits.
export function matchBuiltin(query, { used = new Set(), limit = 1 } = {}) {
  const q = [...new Set(words(query))];
  const things = q.filter((w) => !MOD.has(w));
  const content = things.length ? things : q;
  if (!content.length) return [];
  const scored = [];
  for (const p of builtinPhotos()) {
    let s = 0;
    let hits = 0;
    for (const w of q) {
      const has = p.tagWords.has(w) ? 2 : p.aboutWords.has(w) ? 1.5 : 0;
      if (!has) continue;
      if (content.includes(w)) { s += has; hits++; } else s += 0.5;
    }
    const subject = p.tagWords.has(content[0]) || p.aboutWords.has(content[0]);
    if (subject || hits >= 2 || hits / content.length > 0.5) scored.push({ p, s: s + (subject ? 1 : 0), used: used.has(p.id) });
  }
  // Another photo stands in for one already on the page only when it fits
  // nearly as well ("coffee cups" should not become laptops beside a cup).
  const top = Math.max(0, ...scored.map((x) => x.s));
  for (let k = scored.length - 1; k >= 0; k--) if (scored[k].used || scored[k].s < top * 0.6) scored.splice(k, 1);
  // Ties go to a photo picked by the query, so two apps asking alike do not
  // always get the same one.
  const tie = (id) => { let h = 0; for (const ch of query + id) h = (h * 31 + ch.charCodeAt(0)) >>> 0; return h; };
  scored.sort((a, b) => b.s - a.s || tie(a.p.id) - tie(b.p.id));
  return scored.slice(0, limit).map((x) => x.p);
}

const cache = new Map();
async function getJson(url, headers = {}) {
  if (cache.has(url)) return cache.get(url);
  const res = await fetch(url, { headers, signal: AbortSignal.timeout(6000) });
  if (!res.ok) throw new Error(`${new URL(url).host} answered ${res.status}`);
  const out = await res.json();
  cache.set(url, out);
  return out;
}

async function fromUnsplash(query, use, used) {
  const orientation = use === 'avatar' ? 'squarish' : use === 'card' ? 'portrait' : 'landscape';
  const d = await getJson(`https://api.unsplash.com/search/photos?query=${encodeURIComponent(query)}&per_page=10&content_filter=high&orientation=${orientation}`, { authorization: `Client-ID ${UNSPLASH_KEY}`, 'accept-version': 'v1' });
  const r = (d.results || []).find((x) => !used.has(x.id)) || (d.results || [])[0];
  if (!r) return null;
  // Unsplash asks that a use be reported to its download endpoint.
  fetch(r.links.download_location, { headers: { authorization: `Client-ID ${UNSPLASH_KEY}` }, signal: AbortSignal.timeout(6000) }).catch(() => {});
  const sep = r.urls.raw.includes('?') ? '&' : '?';
  return { id: r.id, src: `${r.urls.raw}${sep}auto=format&fit=crop&q=80&w=${WIDTHS[use] || 1400}${use === 'avatar' ? `&h=${WIDTHS.avatar}&crop=faces` : ''}`,
    credit: `Photo: ${r.user.name} on Unsplash`, href: `${r.user.links.html}?utm_source=strata_apps&utm_medium=referral`, source: 'unsplash' };
}

// Wikimedia serves any width of its photos; an original may be 20 MB.
const sized = (url, w) => {
  const m = url.match(/^https:\/\/upload\.wikimedia\.org\/wikipedia\/commons\/([0-9a-f])\/([0-9a-f]{2})\/([^/]+)$/);
  return m ? `https://upload.wikimedia.org/wikipedia/commons/thumb/${m[1]}/${m[2]}/${m[3]}/${w}px-${m[3]}` : url;
};

// Openverse wants every word of a query, so a long one ("Portuguese tiles
// azulejo") finds nothing: it is asked again with the words that name things,
// then the last two of them ("tiles azulejo").
const shorter = (query) => {
  const raw = String(query).toLowerCase().split(/[^a-z0-9]+/).filter((w) => w && !STOP.has(w));
  const things = raw.filter((w) => !MOD.has(stem(w)));
  return [...new Set([raw.join(' '), things.join(' '), things.slice(-2).join(' ')])].filter(Boolean);
};

async function fromOpenverse(query, use, used) {
  const wide = use !== 'avatar' && use !== 'card';
  const minW = { hero: 1600, band: 1400, slide: 1400 }[use] || 800;
  let r = null;
  for (const q of shorter(query)) {
    const d = await getJson(`https://api.openverse.org/v1/images/?q=${encodeURIComponent(q)}&page_size=20&license_type=commercial&category=photograph&mature=false${wide ? '&aspect_ratio=wide' : ''}`);
    r = (d.results || []).find((x) => x.url && /^https:/.test(x.url) && (x.width || 0) >= minW && !used.has(x.id));
    if (r) break;
  }
  if (!r) return null;
  const lic = `${String(r.license || '').toUpperCase()}${r.license_version ? ` ${r.license_version}` : ''}`;
  const by = r.creator ? ` by ${r.creator}` : '';
  return { id: r.id, src: sized(r.url, Math.min(WIDTHS[use] || 1400, r.width || 2000)), credit: `${(r.title || 'Photo').slice(0, 60)}${by}, ${r.license === 'cc0' || r.license === 'pdm' ? 'public domain' : lic}`,
    href: r.foreign_landing_url || null, source: 'openverse' };
}

// One photo for a query, or null. `used` holds the ids already on the page.
export async function findPhoto(query, { use = 'card', used = new Set(), open = OPEN, unsplash = UNSPLASH_KEY } = {}) {
  const q = String(query || '').trim();
  if (!q) return null;
  if (unsplash) {
    try { const r = await fromUnsplash(q, use, used); if (r) return r; } catch { /* the built-in set next */ }
  }
  const [p] = matchBuiltin(q, { used });
  if (p) return { id: p.id, src: unsplashUrl(p.id, use), credit: null, href: null, source: 'builtin', tone: p.tone };
  if (open) {
    try { const r = await fromOpenverse(q, use, used); if (r) return r; } catch { /* none */ }
  }
  // Nothing new fits: the best photo again, even if the page has it.
  const [again] = matchBuiltin(q);
  return again ? { id: again.id, src: unsplashUrl(again.id, use), credit: null, href: null, source: 'builtin' } : null;
}

// The built-in photos a screen's code already shows (an edit of another
// screen passes them over while others fit).
export const photoIds = (code) => [...String(code || '').matchAll(/images\.unsplash\.com\/photo-([0-9]+-[0-9a-f]+)/g)].map((m) => m[1]);

// Where a photo goes, from the block that holds it.
const USE = { hero: 'hero', band: 'band', media: 'media', gallery: 'gallery', quotes: 'avatar', team: 'avatar', carousel: 'card', features: 'card', cards: 'card' };

// Fills "src" (and "credit") in every object of a screen that names a photo
// and has none yet. Returns the photos placed.
export async function placeScreenPhotos(screen, { used = new Set(), find = findPhoto } = {}) {
  const placed = [];
  const walk = async (v, use) => {
    if (Array.isArray(v)) { for (const x of v) await walk(x, use); return; }
    if (!v || typeof v !== 'object') return;
    const kind = Object.keys(USE).find((k) => v[k] !== undefined);
    // What this block holds is drawn as the block says (a carousel of
    // slides fills the width); a photo of the block itself, as its kind.
    const inner = kind ? (kind === 'carousel' && v.look === 'slides' ? 'slide' : USE[kind]) : use;
    const own = kind === 'hero' || kind === 'band' || kind === 'media' ? USE[kind] : use || inner || 'card';
    if (typeof v.photo === 'string' && v.photo.trim() && !v.photo.startsWith('=') && (v.src === undefined || v.src === null || v.src === '')) {
      const r = await find(v.photo, { use: own, used });
      if (r) {
        used.add(r.id);
        v.src = r.src;
        if (r.credit) v.credit = r.credit;
        if (r.href) v.credit_url = r.href;
        placed.push({ query: v.photo, source: r.source });
      }
    }
    for (const x of Object.values(v)) if (x && typeof x === 'object') await walk(x, inner);
  };
  await walk(screen.body || [], null);
  return placed;
}

// Table names that say nothing of what their rows show.
const GENERIC = new Set(words('products items listings entries records things posts goods inventory catalog catalogue offerings menu articles'));

// An example row's photo: its query, else the row's kind ("category":
// "Bread"), else its name, each first among photos not yet on the page, then
// among those that are (the croissant on the front page is the croissant on
// the menu too), and last what its table holds ("coffees"). Never a photo of
// something else: a sofa is no croissant.
function seedPhoto(query, row, table, used) {
  const text = (re) => Object.entries(row).filter(([k, v]) => re.test(k) && typeof v === 'string' && !v.startsWith('photo:') && v.length <= 60 && !/^https?:/i.test(v)).map(([, v]) => v);
  const asks = [query, ...text(/categor|kind|type|group|section|collection/i), ...text(/^(name|title)$|_name$/i)];
  for (const q of asks) { const [p] = matchBuiltin(q, { used }); if (p) return p; }
  for (const q of asks) { const [p] = matchBuiltin(q); if (p) return p; }
  const what = table.toLowerCase().split(/[^a-z0-9]+/).filter((w) => w && !GENERIC.has(stem(w))).join(' ');
  return (what && matchBuiltin(what, { used })[0]) || (what && matchBuiltin(what)[0]) || null;
}

// Every screen view of a plan, and its example rows ("photo:..." values; the
// built-in set only, as a row has no place for a credit).
export async function placePhotos(plan, code, opts = {}) {
  const placed = [];
  const used = new Set();
  for (const v of plan?.views || []) {
    if (!v?.screen || typeof v.screen !== 'object') continue;
    const got = await placeScreenPhotos(v.screen, { used, ...opts });
    if (got.length) { placed.push(...got); code[v.slug] = JSON.stringify(v.screen, null, 1); }
  }
  for (const [table, rows] of Object.entries(plan?.seed || {})) {
    for (const row of Array.isArray(rows) ? rows : []) {
      for (const [k, val] of Object.entries(row || {})) {
        if (typeof val !== 'string' || !val.startsWith('photo:')) continue;
        const p = seedPhoto(val.slice(6), row, table, used);
        row[k] = p ? unsplashUrl(p.id, 'card') : null;
        if (p) { used.add(p.id); placed.push({ query: val.slice(6), source: 'builtin' }); }
      }
    }
  }
  const by = {};
  for (const p of placed) by[p.source] = (by[p.source] || 0) + 1;
  return { count: placed.length, sources: Object.entries(by).map(([s, n]) => `${n} ${s === 'builtin' ? 'from the built-in set' : `from ${s === 'unsplash' ? 'Unsplash' : 'Openverse'}`}`).join(', ') };
}
