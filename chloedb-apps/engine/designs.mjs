// Three designs for each app: one quick call to a small model, given what the
// app is for, answers with three sets of design tokens (web/design.js). They
// are kept in Strata (sa_designs), and the owner picks one in the studio; a
// person using the app on its own can pick their own. Picking one swaps a
// block of CSS variables in each view: no code changes, nothing is rebuilt.
import { ask } from './llm.mjs';
import { CHOICES, checkDesign, presetDesigns, hslOf, fromHsl, typeOf, pairFor, PRESETS } from '../web/design.js';
import { accentHue } from './photos.mjs';

export const DESIGN_MODEL = process.env.STRATA_APPS_DESIGN_MODEL || 'claude-haiku-5-5';

const choices = Object.entries(CHOICES).map(([k, opts]) => `- ${k}: ${Object.entries(opts).map(([v, what]) => `"${v}" (${what})`).join(', ')}`).join('\n');
// What tells two designs apart besides color (Doable: the three directions
// differ in structure, or people see one design three times).
export const SHAPE = ['look', 'titles', 'nav', 'density', 'corners', 'surface'];

export const DESIGN_SYSTEM = `You are a brand designer. You give a small app its look as a few tokens over the same screens: three designs that are three shapes of the app, not one shape in three colors.

Answer with a JSON array of exactly three designs and nothing else. Each design:
{"name": "one or two words", "about": "the mood in under 12 words", "scheme": "auto",
 "light": {"bg": "#rrggbb", "card": "#rrggbb", "fg": "#rrggbb", "accent": "#rrggbb"},
 "dark": {"bg": "#rrggbb", "card": "#rrggbb", "fg": "#rrggbb", "accent": "#rrggbb"},
 "look": "native", "font": "system", "titles": "serif", "type": "ledger", "corners": "soft", "density": "regular", "surface": "grouped", "nav": "tabs", "width": "wide",
 "gradient": ["#rrggbb", "#rrggbb", "#rrggbb"], "motion": "calm"}

The tokens and their values:
${choices}

The accent is the app's identity: the one color for its buttons, links and the current selection, and nothing else is saturated. Choose its hue for this subject and the people who use it, as you would for a brand of its own. Green and teal are not a default: use them only when the subject itself is green (plants, gardens, farming, the outdoors, money), and never for more than one of the three.
Pages are white, a faint paper tint or genuinely dark: in light, card white and bg white, a faint cool gray or, for a warm subject (food, craft, reports, beauty, studios), a faint warm paper (#faf8f4 to #f7f3ec, never a strong beige, cream or pastel wash); in dark, bg near black and card a little lighter.
The type is the app's voice: "type" is a pairing of a display face for titles and big numbers with plain body text; choose it for this product (a report or a ledger: ledger; fashion or hospitality: classic; a design studio: elegant; a data tool: grotesk; sport or street food: bricolage), and give "titles" the family it belongs to (serif for ledger, classic and elegant; system for system, grotesk and bricolage; rounded; mono). Text (fg) reads on bg and on card at 7:1 or more; the accent reads as text on card at 4.5:1 or more, in light and in dark (a lighter tint of it in dark). gradient is the accent and two neighbours that flatter it, for front pages: vivid, never neon.

The three differ in more than color: their accents come from three different hue families, and each pair differs in at least two of look, titles, nav, density, corners and surface. Each fits the app's subject and its people:
1. The natural one, look "native": the hue the subject suggests first (the art direction's accent and type when the app has one), regular density (compact when the app is mostly tables), tabs.
2. A bold one, look "bold": the key number in a color band and each row a card, a deep, strong accent of a different hue family, serif or rounded titles when they fit the subject.
3. A third shape: look "plain" when the app is mostly tables or records to compare (a compact console: outlined surface, square or soft corners, a sidebar when it has many views), otherwise "native" again with a dark scheme or an airy, editorial page and a third hue family.
Names are evocative and short (Linen, Harbor, Ember), never the app's name.`;

// What a palette must not be (Doable found models default every option to
// green; 7 of our 12 persona apps came out green or teal): a green accent
// where the subject is not green, or two of the three in one hue family.
// The model is asked once more with the problems named.
const GREEN_SUBJECT = /\b(plant|garden|farm|crop|seed|forest|tree|leaf|park|outdoor|hike|trail|eco|green|money|budget|finance|invest|savings|golf|tennis|herb|vegan|salad|matcha|tea)/i;
export function paletteProblems(designs, app) {
  const hues = designs.map((d) => hslOf(d.light?.accent));
  const isGreen = ([hh, sat]) => sat > 0.18 && hh >= 70 && hh <= 185;
  const subject = `${app?.name || ''} ${app?.description || ''}`;
  const out = [];
  const greens = hues.map((h, i) => (isGreen(h) ? i + 1 : 0)).filter(Boolean);
  if (greens.length && !GREEN_SUBJECT.test(subject)) out.push(`design${greens.length > 1 ? 's' : ''} ${greens.join(' and ')} use a green or teal accent, and this subject is not green: choose hues the subject suggests`);
  else if (greens.length > 1) out.push(`designs ${greens.join(' and ')} are both green or teal: only one may be`);
  const dist = (a, b) => { const d = Math.abs(a[0] - b[0]) % 360; return Math.min(d, 360 - d); };
  for (let i = 0; i < hues.length; i++) for (let j = i + 1; j < hues.length; j++) {
    if (hues[i][1] > 0.18 && hues[j][1] > 0.18 && dist(hues[i], hues[j]) < 28) out.push(`designs ${i + 1} and ${j + 1} have accents of one hue family: give them different hues`);
  }
  return out;
}

// The art direction the first plan writes (forge.mjs cleanArt): what the app
// should feel like, its photos, its accent and its titles' type.
export function designPrompt(app, view, direction = null) {
  const views = (view?.views || direction?.views || []).map((v) => v.title).filter(Boolean);
  const tables = (view?.tables?.map((t) => t.spec?.name) || direction?.tables || []).filter(Boolean);
  const art = direction?.art;
  return `The app: ${app.name}
What it is for: ${String(app.description || '').slice(0, 1200)}
${views.length ? `Its views: ${views.join(', ')}\n` : ''}${tables.length ? `Its tables: ${tables.join(', ')}\n` : ''}${art ? `Its art direction (design 1 follows it: its accent's hue, its titles' type, its mood; 2 and 3 are alternatives to it):
- concept: ${art.concept}
- mood: ${art.mood || '-'}
- layout: ${art.layout || '-'}
- photos: ${art.imagery || 'none'}
- accent: ${art.accent || '-'} (the main buttons' color: their words must read on it)
- titles: ${art.type || 'system'}
- theme: ${art.theme || 'light'} (design 1's scheme: ${art.theme === 'dark' ? '"dark": a near-black ground, cards a little lighter, the accent vivid on it' : '"auto"'})
- main action: ${art.action || '-'}
` : ''}
Design three looks for it.`;
}

// Three shapes, made so in code rather than asked for (Doable's "structural
// fixes hold"): the first design has the native look (the studio's default),
// and when the three share one look the second is bold; a first design with
// an art direction takes its type. Returns pairs still too alike, in words.
export function shapeDesigns(designs, direction = null, app = null) {
  if (!designs.length) return [];
  designs[0].look = 'native';
  if (designs.length > 1 && designs.every((d) => d.look === designs[0].look)) designs[1].look = 'bold';
  const type = direction?.art?.type;
  if (type && CHOICES.titles[type]) designs[0].titles = type;
  // Each design's type pairing fits its titles and the app's subject.
  const art = direction?.art;
  const subject = `${app?.name || ''} ${app?.description || ''} ${art?.concept || ''} ${art?.mood || ''}`;
  Object.assign(designs[0], pairFor(designs[0], `${designs[0].name || ''} ${designs[0].about || ''} ${subject}`));
  // A serif design is set on paper: a cool grey page under serif titles
  // turns a faint warm white (the cards stay white).
  const d0 = designs[0];
  if (['ledger', 'classic', 'elegant'].includes(d0.type) && d0.light && /^#[0-9a-f]{6}$/i.test(d0.light.bg || '')) {
    const [r, g, b] = [1, 3, 5].map((i) => parseInt(d0.light.bg.slice(i, i + 2), 16));
    if (b >= r && Math.max(r, g, b) - Math.min(r, g, b) < 14 && r > 225) d0.light = { ...d0.light, bg: '#f8f6f1' };
  }
  for (const d of designs.slice(1)) d.type = typeOf(d, `${d.name || ''} ${d.about || ''} ${subject}`);
  // The art direction's layout sets the first design's width: a reading
  // column for an editorial page, the full width for a dashboard.
  const width = { editorial: 'narrow', tool: 'narrow', dashboard: 'wide', gallery: 'wide' }[direction?.art?.structure];
  if (width) designs[0].width = width;
  // and its theme the first design's scheme: a dark identity is always dark
  if (direction?.art?.theme === 'dark' && designs[0]) designs[0].scheme = 'dark';
  const out = [];
  for (let i = 0; i < designs.length; i++) for (let j = i + 1; j < designs.length; j++) {
    const differ = SHAPE.filter((k) => designs[i][k] !== designs[j][k]);
    if (differ.length < 2) out.push(`designs ${i + 1} and ${j + 1} differ only in ${differ.length ? differ[0] : 'color'}: make them differ in at least two of ${SHAPE.join(', ')}`);
  }
  return out;
}

// The JSON objects written one after another at the top of a text (each
// design on its own, not in an array), strings and nesting respected.
function topObjects(t) {
  const out = [];
  let depth = 0, start = -1, str = false, esc = false;
  for (let i = 0; i < t.length; i++) {
    const c = t[i];
    if (str) { if (esc) esc = false; else if (c === '\\') esc = true; else if (c === '"') str = false; continue; }
    if (c === '"') { if (depth > 0) str = true; continue; }
    if (c === '{') { if (depth++ === 0) start = i; } else if (c === '}' && depth > 0 && --depth === 0) {
      try { out.push(JSON.parse(t.slice(start, i + 1))); } catch { /* not JSON */ }
    }
  }
  return out;
}

// The designs an answer holds: a JSON array of them (fenced or not), an
// object holding one ({"designs": [...]}), or the designs written one after
// another without an array; each design object is kept.
export function designsIn(text) {
  const t = String(text || '').replace(/^```(?:json)?\s*/m, '').replace(/```\s*$/m, '').trim();
  let v = null;
  try { v = JSON.parse(t); } catch {
    const a = t.indexOf('['), b = t.lastIndexOf(']');
    if (a >= 0 && b > a) { try { v = JSON.parse(t.slice(a, b + 1)); } catch { /* none */ } }
    if (v === null) { const c = t.indexOf('{'), d = t.lastIndexOf('}'); if (c >= 0 && d > c) { try { v = JSON.parse(t.slice(c, d + 1)); } catch { /* none */ } } }
  }
  const isDesign = (d) => d && typeof d === 'object' && !Array.isArray(d) && (d.light || d.dark);
  let list = Array.isArray(v) ? v : v && typeof v === 'object' ? (Array.isArray(v.designs) ? v.designs : Object.values(v).find(Array.isArray) || (isDesign(v) ? [v] : [])) : [];
  if (list.filter(isDesign).length < 3) { const each = topObjects(t).filter(isDesign); if (each.length > list.filter(isDesign).length) list = each; }
  return list.filter((d) => d && typeof d === 'object' && (d.light || d.dark || d.name)).slice(0, 3).map((d) => checkDesign(d));
}
const parse = (text) => {
  const list = designsIn(text);
  if (list.length < 3) throw Object.assign(new Error('the answer held fewer than three designs'), { found: list, raw: String(text || '').slice(0, 800) });
  return list;
};

// Presets in the art direction's accent: what fills the designs a model did
// not give, so they still carry the app's color.
export function presetsInAccent(appId, accent, n) {
  const hue = accent ? accentHue(accent) : null;
  return presetDesigns(appId).slice(0, n).map((d) => {
    if (hue == null) return d;
    const tint = (c) => { const [, sat, l] = hslOf(c); return fromHsl(hue, Math.max(0.45, sat), l); };
    return checkDesign({ ...d, light: { ...d.light, accent: tint(d.light.accent) }, dark: { ...d.dark, accent: tint(d.dark.accent) }, gradient: null });
  });
}

// Three designs for an app. A model that fails or answers nonsense gives way
// to three of the presets, so an app always has its three.
export async function makeDesigns(app, view, { ask: askFn = ask, model = DESIGN_MODEL, direction = null } = {}) {
  const t0 = Date.now();
  try {
    const prompt = designPrompt(app, view, direction);
    const r = await askFn({ system: DESIGN_SYSTEM, prompt, model, timeoutMs: 90_000, idleMs: 45_000 });
    let cost = r.cost || 0;
    let designs = designsIn(r.text);
    // Fewer than three: asked once more; what still lacks is filled with
    // presets in the art direction's accent, and the answer is logged.
    if (designs.length < 3) {
      console.log(`designs ${app.id}: the answer held ${designs.length} design${designs.length === 1 ? '' : 's'}; asking again. It began: ${String(r.text || '').slice(0, 600).replace(/\s+/g, ' ')}`);
      try {
        const again = await askFn({ system: DESIGN_SYSTEM, prompt: `${prompt}\n\nYour last answer held ${designs.length} usable design${designs.length === 1 ? '' : 's'}. Answer with a JSON array of exactly three designs, each with "light" and "dark" palettes, and nothing else.`, model, timeoutMs: 90_000, idleMs: 45_000 });
        cost += again.cost || 0;
        const more = designsIn(again.text);
        if (more.length > designs.length) designs = more;
      } catch { /* what there is stands */ }
      if (!designs.length) throw new Error('the answer held no designs');
      if (designs.length < 3) designs = [...designs, ...presetsInAccent(app.id, direction?.art?.accent, 3 - designs.length)];
    }
    // A palette and shapes checked in code, not only asked for: one more try
    // when they miss, and the second answer is kept whatever it holds.
    const problems = [...paletteProblems(designs, app), ...shapeDesigns(designs, direction, app)];
    if (problems.length) {
      try {
        const again = await askFn({ system: DESIGN_SYSTEM, prompt: `${prompt}\n\nYour last answer:\n${JSON.stringify(designs.map(({ name, light, dark, ...rest }) => ({ name, light, dark, ...Object.fromEntries(SHAPE.map((k) => [k, rest[k]])) })))}\nWhat is wrong with it: ${problems.join('; ')}. Answer again with all three designs.`, model, timeoutMs: 90_000, idleMs: 45_000 });
        cost += again.cost || 0;
        designs = parse(again.text);
        shapeDesigns(designs, direction, app);
      } catch { /* the first answer stands */ }
    }
    return { designs, source: model, ms: Date.now() - t0, cost, problems, directed: !!direction?.art };
  } catch (e) {
    const designs = presetsInAccent(app.id, direction?.art?.accent, 3);
    shapeDesigns(designs, direction, app);
    return { designs, source: 'presets', ms: Date.now() - t0, cost: 0, error: e.message };
  }
}

// One generation per app at a time: a second ask waits for the first.
const making = new Map();
export function ensureDesigns(store, appId, { fresh = false, onMade = () => {}, make = makeDesigns, direction = null } = {}) {
  if (making.has(appId)) return making.get(appId);
  const p = (async () => {
    const had = await store.designs(appId);
    if (had && !fresh) return had;
    const app = await store.getApp(appId);
    if (!app) throw Object.assign(new Error(`no app ${appId}`), { status: 404 });
    const view = app.head ? await store.resolve(appId).catch(() => null) : null;
    const out = await make(app, view, direction ? { direction } : undefined);
    let designs = out.designs;
    let chosen = 0;
    // New alternatives keep the design in use where it is.
    if (had && fresh) {
      chosen = had.chosen;
      const others = out.designs.filter((d) => d.name !== had.designs[had.chosen]?.name);
      designs = had.designs.map((d, i) => (i === had.chosen ? d : others.shift() || d));
    }
    const saved = { designs, chosen, source: out.source };
    await store.putDesigns(appId, saved);
    onMade(saved, out);
    return saved;
  })().finally(() => making.delete(appId));
  making.set(appId, p);
  return p;
}

// A design's tokens changed by hand (a builder picks a new accent, page or
// card color, or another corner, density or title style): the design stays
// itself, checked again so its text stays readable. Colors are set for the
// light or the dark scheme; only the tokens a design has are taken.
export const COLOR_TOKENS = ['accent', 'bg', 'card', 'fg'];
const HEX6 = /^#[0-9a-f]{6}$/i;
export function tuneDesign(d, { scheme = 'light', colors = {}, ...shape } = {}) {
  if (!['light', 'dark'].includes(scheme)) throw Object.assign(new Error('scheme is light or dark'), { status: 400 });
  const next = { ...d, light: { ...d.light }, dark: { ...d.dark } };
  for (const [k, v] of Object.entries(colors || {})) {
    if (!COLOR_TOKENS.includes(k)) throw Object.assign(new Error(`a color token is ${COLOR_TOKENS.join(', ')}`), { status: 400 });
    if (!HEX6.test(String(v))) throw Object.assign(new Error(`${k} is a color like #2f5bd3`), { status: 400 });
    next[scheme][k] = String(v).toLowerCase();
  }
  for (const [k, v] of Object.entries(shape)) {
    if (!['corners', 'density', 'titles', 'font', 'surface'].includes(k)) throw Object.assign(new Error(`${k} is not a design token`), { status: 400 });
    if (!Object.hasOwn(CHOICES[k], v)) throw Object.assign(new Error(`${k} is one of ${Object.keys(CHOICES[k]).join(', ')}`), { status: 400 });
    next[k] = v;
  }
  return checkDesign(next);
}

// Colors to choose from for a token: the app's other designs' and the
// presets', the one in use first, without repeats.
export function tokenChoices(designs, chosen = 0, scheme = 'light') {
  const out = {};
  const sets = [designs[chosen], ...designs.filter((_, i) => i !== chosen), ...PRESETS.map((p) => checkDesign(p))].filter(Boolean);
  for (const k of COLOR_TOKENS) out[k] = [...new Set(sets.map((d) => d?.[scheme]?.[k]).filter((v) => HEX6.test(String(v || ''))).map((v) => v.toLowerCase()))].slice(0, 12);
  return out;
}
