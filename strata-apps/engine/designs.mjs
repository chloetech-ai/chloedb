// Three designs for each app: one quick call to a small model, given what the
// app is for, answers with three sets of design tokens (web/design.js). They
// are kept in Strata (sa_designs), and the owner picks one in the studio; a
// person using the app on its own can pick their own. Picking one swaps a
// block of CSS variables in each view: no code changes, nothing is rebuilt.
import { ask } from './llm.mjs';
import { CHOICES, checkDesign, presetDesigns, hslOf } from '../web/design.js';

export const DESIGN_MODEL = process.env.STRATA_APPS_DESIGN_MODEL || 'claude-haiku-5-5';

// The look (plain, bold) is not the model's to pick yet: it is chosen for
// the studio as a whole while the looks are compared.
const choices = Object.entries(CHOICES).filter(([k]) => k !== 'look').map(([k, opts]) => `- ${k}: ${Object.entries(opts).map(([v, what]) => `"${v}" (${what})`).join(', ')}`).join('\n');

export const DESIGN_SYSTEM = `You are a brand designer. You give a small app its look as a few tokens; its screens stay as they are, drawn like a good phone app (a grouped page, white or dark cells, large titles, one accent color).

Answer with a JSON array of exactly three designs and nothing else. Each design:
{"name": "one or two words", "about": "the mood in under 12 words", "scheme": "auto",
 "light": {"bg": "#rrggbb", "card": "#rrggbb", "fg": "#rrggbb", "accent": "#rrggbb"},
 "dark": {"bg": "#rrggbb", "card": "#rrggbb", "fg": "#rrggbb", "accent": "#rrggbb"},
 "font": "system", "titles": "system", "corners": "soft", "density": "regular", "surface": "grouped", "nav": "tabs", "width": "wide",
 "gradient": ["#rrggbb", "#rrggbb", "#rrggbb"], "motion": "calm"}

The tokens and their values:
${choices}

The accent is the app's identity: the one color for its buttons, links and the current selection, and nothing else is saturated. Choose its hue for this subject and the people who use it, as you would for a brand of its own. Green and teal are not a default: use them only when the subject itself is green (plants, gardens, farming, the outdoors, money), and never for more than one of the three.
Pages are white or genuinely dark: in light, card white and bg white or a faint cool gray; in dark, bg near black and card a little lighter. No beige, cream or pastel page wash. Text (fg) reads on bg and on card at 7:1 or more; the accent reads as text on card at 4.5:1 or more, in light and in dark (a lighter tint of it in dark). gradient is the accent and two neighbours that flatter it, for front pages: vivid, never neon.

The three differ in more than color: their accents come from three different hue families, and each pair differs in at least two of titles, nav, density and corners. Each fits the app's subject and its people:
1. The natural one: the hue the subject suggests first, system titles, regular density (compact when the app is mostly tables), tabs.
2. A warmer or more editorial one: serif or rounded titles, a different hue family, airy or regular.
3. A bolder one: a dark scheme or a deep, strong accent, or a sidebar when the app has many views or big tables.
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

export function designPrompt(app, view) {
  const views = (view?.views || []).map((v) => v.title).filter(Boolean);
  const tables = (view?.tables || []).map((t) => t.spec?.name).filter(Boolean);
  return `The app: ${app.name}
What it is for: ${String(app.description || '').slice(0, 1200)}
${views.length ? `Its views: ${views.join(', ')}\n` : ''}${tables.length ? `Its tables: ${tables.join(', ')}\n` : ''}
Design three looks for it.`;
}

const parse = (text) => {
  const m = String(text).match(/\[[\s\S]*\]/);
  const list = JSON.parse(m ? m[0] : text);
  if (!Array.isArray(list) || list.length < 3) throw new Error('the answer held fewer than three designs');
  return list.slice(0, 3).map((d) => checkDesign(d));
};

// Three designs for an app. A model that fails or answers nonsense gives way
// to three of the presets, so an app always has its three.
export async function makeDesigns(app, view, { ask: askFn = ask, model = DESIGN_MODEL } = {}) {
  const t0 = Date.now();
  try {
    const prompt = designPrompt(app, view);
    const r = await askFn({ system: DESIGN_SYSTEM, prompt, model, timeoutMs: 90_000, idleMs: 45_000 });
    let designs = parse(r.text);
    let cost = r.cost || 0;
    // A palette checked in code, not only asked for: one more try when it
    // misses, and the second answer is kept whatever it holds.
    const problems = paletteProblems(designs, app);
    if (problems.length) {
      try {
        const again = await askFn({ system: DESIGN_SYSTEM, prompt: `${prompt}\n\nYour last answer:\n${JSON.stringify(designs.map(({ name, light, dark }) => ({ name, light, dark })))}\nWhat is wrong with it: ${problems.join('; ')}. Answer again with all three designs.`, model, timeoutMs: 90_000, idleMs: 45_000 });
        cost += again.cost || 0;
        designs = parse(again.text);
      } catch { /* the first answer stands */ }
    }
    return { designs, source: model, ms: Date.now() - t0, cost, problems };
  } catch (e) {
    return { designs: presetDesigns(app.id), source: 'presets', ms: Date.now() - t0, cost: 0, error: e.message };
  }
}

// One generation per app at a time: a second ask waits for the first.
const making = new Map();
export function ensureDesigns(store, appId, { fresh = false, onMade = () => {}, make = makeDesigns } = {}) {
  if (making.has(appId)) return making.get(appId);
  const p = (async () => {
    const had = await store.designs(appId);
    if (had && !fresh) return had;
    const app = await store.getApp(appId);
    if (!app) throw Object.assign(new Error(`no app ${appId}`), { status: 404 });
    const view = app.head ? await store.resolve(appId).catch(() => null) : null;
    const out = await make(app, view);
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
