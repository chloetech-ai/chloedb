// title: Text
// description: Everyday text work: slugs, truncation, initials, plurals, accent-free matching, fuzzy search over a list, highlighting what matched, #tags and links in text, reading time, and quantities like "2 kg flour".
// api: slugify(text) -> 'hello-world'; truncate(text, n, end = '…'); initials(name, n = 2) -> 'RK'; capitalize(text);
//   plural(n, word, plural = word + 's') -> '1 item', '3 items';
//   normalize(text) -> lower case without accents; words(text) -> array; wordCount(text); readingMinutes(text, wpm = 220);
//   fuzzy(query, text) -> score 0..1 (0 = no match; all of the query's letters in order, close together and at word starts score higher);
//   search(query, items, by = (x) => x) -> items that match, best first (by: a field name or function giving the text);
//   highlight(text, query) -> [{text, hit}] pieces to render, hit true where a query word matches;
//   tags(text) -> ['#work' words without '#', lower case]; links(text) -> the http(s) URLs in it;
//   quantity(text) -> {amount, unit, name} from '2 kg flour', '1/2 cup sugar', '3 eggs' (amount null when none)
function helper() {
  const normalize = (s) => String(s ?? '').normalize('NFD').replace(/[̀-ͯ]/g, '').toLowerCase();
  const slugify = (s) => normalize(s).replace(/[^a-z0-9]+/g, '-').replace(/^-+|-+$/g, '').slice(0, 80);
  const truncate = (s, n, end = '…') => { const t = String(s ?? ''); return t.length <= n ? t : `${t.slice(0, Math.max(0, n - end.length)).replace(/\s+\S*$/, '') || t.slice(0, n - end.length)}${end}`; };
  const initials = (name, n = 2) => String(name ?? '').trim().split(/[\s@._-]+/).filter(Boolean).slice(0, n).map((w) => w[0].toUpperCase()).join('');
  const capitalize = (s) => { const t = String(s ?? ''); return t.charAt(0).toUpperCase() + t.slice(1); };
  const plural = (n, word, many) => `${n} ${Number(n) === 1 ? word : many || `${word}s`}`;
  const words = (s) => String(s ?? '').match(/[\p{L}\p{N}][\p{L}\p{N}'’-]*/gu) || [];
  const wordCount = (s) => words(s).length;
  const readingMinutes = (s, wpm = 220) => Math.max(1, Math.round(wordCount(s) / wpm));
  function fuzzy(query, text) {
    const q = normalize(query).replace(/\s+/g, '');
    const t = normalize(text);
    if (!q) return 1;
    if (!t) return 0;
    const at = t.indexOf(q);
    if (at >= 0) return (t === q ? 1 : at === 0 ? 0.97 : /\W/.test(t[at - 1]) ? 0.94 : 0.88) - Math.min(0.03, t.length / 10000);
    let score = 0;
    let ti = 0;
    let prev = -2;
    for (const c of q) {
      const i = t.indexOf(c, ti);
      if (i < 0) return 0;
      score += i === prev + 1 ? 1 : i === 0 || /\W/.test(t[i - 1]) ? 0.8 : 0.4;
      prev = i;
      ti = i + 1;
    }
    return Math.min(0.85, (score / q.length) * 0.85 * Math.min(1, (q.length + 2) / (prev - t.indexOf(q[0]) + 3)));
  }
  function search(query, items, by) {
    const get = by === undefined ? (x) => x : typeof by === 'function' ? by : (x) => x && x[by];
    if (!String(query ?? '').trim()) return (items || []).slice();
    const terms = normalize(query).split(/\s+/).filter(Boolean);
    return (items || []).map((x) => {
      const t = String(get(x) ?? '');
      const scores = terms.map((q) => fuzzy(q, t));
      return { x, s: scores.includes(0) ? 0 : scores.reduce((a, b) => a + b, 0) / scores.length };
    }).filter((r) => r.s > 0).sort((a, b) => b.s - a.s).map((r) => r.x);
  }
  function highlight(text, query) {
    const t = String(text ?? '');
    const terms = normalize(query).split(/\s+/).filter((w) => w.length > 0);
    if (!terms.length) return [{ text: t, hit: false }];
    const n = normalize(t);
    const marks = new Array(t.length).fill(false);
    for (const w of terms) for (let i = n.indexOf(w); i >= 0; i = n.indexOf(w, i + 1)) for (let k = i; k < i + w.length && k < t.length; k++) marks[k] = true;
    const out = [];
    for (let i = 0; i < t.length; i++) {
      if (out.length && out[out.length - 1].hit === marks[i]) out[out.length - 1].text += t[i];
      else out.push({ text: t[i], hit: marks[i] });
    }
    return out.length ? out : [{ text: '', hit: false }];
  }
  const tags = (s) => [...new Set((String(s ?? '').match(/(^|\s)#([\p{L}\p{N}_-]+)/gu) || []).map((m) => m.trim().slice(1).toLowerCase()))];
  const links = (s) => (String(s ?? '').match(/https?:\/\/[^\s<>"')\]]+/g) || []).map((u) => u.replace(/[.,;:!?]+$/, ''));
  const UNITS = 'kg|g|mg|lb|lbs|oz|l|ml|cl|dl|cup|cups|tbsp|tsp|tablespoons?|teaspoons?|pinch|clove|cloves|slice|slices|can|cans|pack|packs|bunch|dozen|pcs|pieces?|bottles?|jars?|bags?|boxes|box';
  function quantity(text) {
    const s = String(text ?? '').trim();
    const m = s.match(new RegExp(`^(\\d+\\s+\\d+/\\d+|\\d+/\\d+|\\d+(?:[.,]\\d+)?|[½¼¾⅓⅔])\\s*(?:x\\s*)?(${UNITS})?\\b\\.?\\s*(?:of\\s+)?(.*)$`, 'i'));
    if (!m) return { amount: null, unit: null, name: s };
    const frac = { '½': 0.5, '¼': 0.25, '¾': 0.75, '⅓': 1 / 3, '⅔': 2 / 3 };
    let amount;
    const a = m[1];
    if (frac[a] !== undefined) amount = frac[a];
    else if (/\s/.test(a)) { const [w, f] = a.split(/\s+/); const [p, q] = f.split('/'); amount = Number(w) + Number(p) / Number(q); }
    else if (a.includes('/')) { const [p, q] = a.split('/'); amount = Number(p) / Number(q); }
    else amount = Number(a.replace(',', '.'));
    return { amount: Math.round(amount * 1000) / 1000, unit: m[2] ? m[2].toLowerCase() : null, name: m[3].trim() || s };
  }
  return { slugify, truncate, initials, capitalize, plural, normalize, words, wordCount, readingMinutes, fuzzy, search, highlight, tags, links, quantity };
}
