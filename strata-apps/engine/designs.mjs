// Three designs for each app: one quick call to a small model, given what the
// app is for, answers with three sets of design tokens (web/design.js). They
// are kept in Strata (sa_designs), and the owner picks one in the studio; a
// person using the app on its own can pick their own. Picking one swaps a
// block of CSS variables in each view: no code changes, nothing is rebuilt.
import { ask } from './llm.mjs';
import { CHOICES, checkDesign, presetDesigns } from '../web/design.js';

export const DESIGN_MODEL = process.env.STRATA_APPS_DESIGN_MODEL || 'claude-haiku-5-5';

const choices = Object.entries(CHOICES).map(([k, opts]) => `- ${k}: ${Object.entries(opts).map(([v, what]) => `"${v}" (${what})`).join(', ')}`).join('\n');

export const DESIGN_SYSTEM = `You are a product designer with Apple's taste: calm, minimal, generous whitespace, one accent color, clear type, nothing decorative. You design the look of small apps as a few tokens; the app's screens stay as they are.

Answer with a JSON array of exactly three designs and nothing else. Each design:
{"name": "one or two words", "about": "the mood in under 12 words", "scheme": "auto",
 "light": {"bg": "#rrggbb", "card": "#rrggbb", "fg": "#rrggbb", "accent": "#rrggbb"},
 "dark": {"bg": "#rrggbb", "card": "#rrggbb", "fg": "#rrggbb", "accent": "#rrggbb"},
 "font": "system", "titles": "system", "corners": "round", "density": "regular", "surface": "grouped", "nav": "segmented", "width": "regular"}

The tokens and their values:
${choices}

Colors: bg is the page, card the surface of cards and lists, fg the text, accent the one color for buttons, links and selection. Text (fg) reads on bg and on card at 7:1 or more; the accent reads as text on card at 4.5:1 or more. In light, card is usually white and bg a faint tint of the accent's hue or a warm or cool gray; in dark, bg is near black and card a little lighter. Keep saturation low except the accent.

The three must differ clearly, each fitting the app's subject and the people who use it:
1. The natural one: Apple's own spirit (system font, grouped cards, segmented control), its accent taken from the subject.
2. A warmer or more editorial one: serif or rounded titles, tinted paper, outlined or raised surfaces, tabs.
3. A bolder one: a dark scheme or a strong accent, or a sidebar and wide layout when the app has many views or big tables.
Names are evocative and short (Linen, Harbor, Ember), never the app's name.`;

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
    const r = await askFn({ system: DESIGN_SYSTEM, prompt: designPrompt(app, view), model, timeoutMs: 90_000, idleMs: 45_000 });
    return { designs: parse(r.text), source: model, ms: Date.now() - t0, cost: r.cost || 0 };
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
