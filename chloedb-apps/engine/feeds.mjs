// Feeds and JSON lists as rows: what CREATE SOURCE and the PULL step read.
//
// A feed is RSS 2.0, RSS 1.0 (RDF) or Atom; each item gives the same fields,
// whatever the format called them:
//   title, link, guid (the feed's own id, else the link), published and
//   updated (ISO 8601), summary (the description as plain text), content (the
//   full text when the feed carries it), author, categories, image
// A JSON source is a list of objects, found at a path (`AT 'data.items'`),
// and its fields are read by path too (`price = offer.price`).
//
// No dependency: the parser is small and forgiving (CDATA, entities, the
// namespaced tags feeds use), and it never runs anything it reads.

const ENT = { amp: '&', lt: '<', gt: '>', quot: '"', apos: "'", nbsp: ' ', mdash: '—', ndash: '–', hellip: '…', rsquo: '’', lsquo: '‘', rdquo: '”', ldquo: '“' };
export function decode(s) {
  return String(s ?? '').replace(/&(#x[0-9a-f]+|#\d+|[a-z]+);/gi, (m, e) => {
    if (e[0] === '#') { const n = e[1].toLowerCase() === 'x' ? parseInt(e.slice(2), 16) : Number(e.slice(1)); return n > 0 && n < 0x110000 ? String.fromCodePoint(n) : m; }
    return ENT[e.toLowerCase()] ?? m;
  });
}

// An element's inner text: CDATA kept as it is, the rest decoded.
function inner(raw) {
  if (raw == null) return '';
  let out = '';
  const re = /<!\[CDATA\[([\s\S]*?)\]\]>/g;
  let at = 0;
  for (let m; (m = re.exec(raw));) { out += decode(raw.slice(at, m.index)) + m[1]; at = m.index + m[0].length; }
  return (out + decode(raw.slice(at))).trim();
}

// HTML (as feeds put in a description) as plain text.
export function plain(html) {
  return decode(String(html ?? '')
    .replace(/<!--[\s\S]*?-->/g, ' ')
    .replace(/<(script|style|noscript|svg|template|iframe)\b[\s\S]*?<\/\1>/gi, ' ')
    .replace(/<(br|\/p|\/div|\/li|\/h[1-6]|\/tr|hr)\b[^>]*>/gi, '\n')
    .replace(/<[^>]+>/g, ' '))
    .replace(/[ \t\f\v ]+/g, ' ').replace(/ *\n */g, '\n').replace(/\n{3,}/g, '\n\n').trim();
}

const esc = (s) => s.replace(/[.*+?^${}()|[\]\\]/g, '\\$&');
// The first <tag ...>...</tag> (any of the names, with or without a prefix):
// {attrs, body} or null. A self-closed <tag .../> gives an empty body.
function el(xml, names) {
  for (const name of [].concat(names)) {
    const re = new RegExp(`<(${esc(name)})((?:\\s[^>]*?)?)(/>|>([\\s\\S]*?)</\\1\\s*>)`, 'i');
    const m = xml.match(re);
    if (m) return { attrs: m[2] || '', body: m[4] ?? '' };
  }
  return null;
}
function all(xml, name) {
  const re = new RegExp(`<(${esc(name)})((?:\\s[^>]*?)?)(/>|>([\\s\\S]*?)</\\1\\s*>)`, 'gi');
  return [...xml.matchAll(re)].map((m) => ({ attrs: m[2] || '', body: m[4] ?? '' }));
}
const attr = (attrs, name) => decode(attrs.match(new RegExp(`\\b${esc(name)}\\s*=\\s*(?:"([^"]*)"|'([^']*)')`, 'i'))?.slice(1).find((x) => x !== undefined) ?? '');

function iso(s) {
  const t = Date.parse(String(s || '').trim());
  return Number.isNaN(t) ? null : new Date(t).toISOString();
}

function absolute(href, base) {
  if (!href) return '';
  try { return new URL(href, base || undefined).href; } catch { return href; }
}

// Items of a feed, newest first as the feed gives them.
export function parseFeed(xml, base = '') {
  const s = String(xml || '').replace(/^﻿/, '');
  if (!/<(rss|feed|rdf:RDF|channel)\b/i.test(s)) throw new Error('not an RSS or Atom feed');
  const atom = /<feed\b/i.test(s) && !/<rss\b/i.test(s);
  const blocks = atom ? all(s, 'entry') : all(s, 'item');
  return blocks.map(({ body: b }) => {
    const text = (names) => inner(el(b, names)?.body);
    let link = '';
    if (atom) {
      const links = all(b, 'link');
      const alt = links.find((l) => !attr(l.attrs, 'rel') || attr(l.attrs, 'rel') === 'alternate') || links[0];
      link = alt ? attr(alt.attrs, 'href') : '';
    } else {
      link = text(['link']) || attr(el(b, ['link'])?.attrs || '', 'href');
    }
    link = absolute(link, base);
    const html = text(atom ? ['summary', 'content'] : ['description', 'summary']);
    const full = text(['content:encoded', 'content']);
    const enclosure = el(b, ['enclosure', 'media:content', 'media:thumbnail']);
    const img = enclosure ? attr(enclosure.attrs, 'url') : (html.match(/<img\b[^>]*\bsrc\s*=\s*["']([^"']+)/i)?.[1] || '');
    const author = atom ? inner(el(el(b, ['author'])?.body || '', ['name'])?.body) || text(['author']) : text(['dc:creator', 'author']);
    const cats = atom ? all(b, 'category').map((c) => attr(c.attrs, 'term') || inner(c.body)) : all(b, 'category').map((c) => inner(c.body));
    const published = iso(text(['pubDate', 'published', 'dc:date', 'issued', 'updated']));
    return {
      title: plain(text(['title'])),
      link,
      guid: text(['guid', 'id']) || link,
      published,
      updated: iso(text(['updated', 'atom:updated', 'dc:modified'])) || published,
      summary: plain(html).slice(0, 4000),
      content: plain(full || html).slice(0, 50_000),
      author: plain(author),
      categories: cats.filter(Boolean).join(', '),
      image: absolute(img, base),
    };
  }).filter((it) => it.title || it.link);
}

// A value by path: "a.b.0.c".
export function at(obj, path) {
  if (!path) return obj;
  let v = obj;
  for (const k of String(path).split('.')) { if (v == null) return undefined; v = v[k]; }
  return v;
}

// The objects of a JSON answer: at `path`, else the answer itself when it is
// a list, else its first list of objects.
export function jsonItems(json, path = '') {
  let v = path ? at(json, path) : json;
  if (!path && v && !Array.isArray(v) && typeof v === 'object') v = Object.values(v).find((x) => Array.isArray(x) && x.some((y) => y && typeof y === 'object')) ?? v;
  if (!Array.isArray(v)) throw new Error(path ? `nothing at ${path} is a list` : 'the answer holds no list (say where it is with AT \'path.to.list\')');
  return v.filter((x) => x && typeof x === 'object');
}

export const FEED_FIELDS = ['title', 'link', 'guid', 'published', 'updated', 'summary', 'content', 'author', 'categories', 'image'];
