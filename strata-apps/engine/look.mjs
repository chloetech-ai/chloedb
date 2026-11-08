// The drawn-page check. Every live version is drawn the way people see it,
// on a phone (390 px) and on a computer (1280 px), each screen measured by
// rules that hold for any app (nothing scrolls sideways, every tap is 44 px,
// text reads at 4.5:1, no raw ids), and the phone screens looked at once by
// the model against the app's brief: can the person do the job from the
// first screen, in a tap or two? What fails goes to the repair as one
// request, so the next version fixes it.

import { loadPlaywright } from './checks.mjs';
import { ask, extractJson } from './llm.mjs';
import { briefOf } from './store.mjs';

export const LOOK = process.env.STRATA_APPS_LOOK !== 'off';
export const LOOK_MODEL = process.env.STRATA_APPS_LOOK_MODEL === 'off' ? null : (process.env.STRATA_APPS_LOOK_MODEL || 'claude-sonnet-5-5');
const DEVICES = {
  phone: { viewport: { width: 390, height: 844 }, deviceScaleFactor: 2, isMobile: true, hasTouch: true },
  computer: { viewport: { width: 1280, height: 860 }, deviceScaleFactor: 1 },
};
const MAX_VIEWS = 5;

// Measures a drawn screen. It runs inside the app's frame (page.evaluate),
// so it is one function with nothing from outside it; `fold` is how much of
// the frame the first screenful shows.
export function measure(fold) {
  const doc = document.scrollingElement || document.documentElement;
  const W = doc.clientWidth;
  const H = Number(fold) || window.innerHeight;
  const shown = (e) => {
    const r = e.getBoundingClientRect();
    if (!r.width || !r.height) return null;
    for (let n = e; n && n.nodeType === 1; n = n.parentElement) {
      const cs = getComputedStyle(n);
      if (cs.display === 'none' || cs.visibility === 'hidden' || Number(cs.opacity) === 0) return null;
    }
    return r;
  };
  const words = (s) => String(s || '').trim().replace(/\s+/g, ' ');
  const nameOf = (e) => words(e.getAttribute('aria-label') || e.textContent || e.getAttribute('placeholder') || e.getAttribute('title') || e.getAttribute('name') || e.type || e.tagName.toLowerCase()).slice(0, 40);
  // Colors as the browser draws them, whatever their syntax.
  const cv = document.createElement('canvas');
  cv.width = 1; cv.height = 1;
  const cx = cv.getContext('2d', { willReadFrequently: true });
  const seen = new Map();
  const rgba = (c) => {
    if (seen.has(c)) return seen.get(c);
    cx.clearRect(0, 0, 1, 1);
    cx.fillStyle = 'rgba(0,0,0,0)';
    cx.fillStyle = c;
    cx.fillRect(0, 0, 1, 1);
    const [r, g, b, a] = cx.getImageData(0, 0, 1, 1).data;
    const v = { r, g, b, a: a / 255 };
    seen.set(c, v);
    return v;
  };
  const over = (top, under) => ({ r: top.r * top.a + under.r * (1 - top.a), g: top.g * top.a + under.g * (1 - top.a), b: top.b * top.a + under.b * (1 - top.a), a: 1 });
  const background = (e) => {
    const layers = [];
    for (let n = e; n && n.nodeType === 1; n = n.parentElement) {
      const cs = getComputedStyle(n);
      if (cs.backgroundImage && cs.backgroundImage !== 'none') return null; // a picture or a gradient: not measured
      const c = rgba(cs.backgroundColor);
      if (c.a > 0) { layers.push(c); if (c.a >= 1) break; }
    }
    let out = { r: 255, g: 255, b: 255, a: 1 };
    for (const c of layers.reverse()) out = over(c, out);
    return out;
  };
  const lum = ({ r, g, b }) => {
    const f = (x) => { const s = x / 255; return s <= 0.03928 ? s / 12.92 : ((s + 0.055) / 1.055) ** 2.4; };
    return 0.2126 * f(r) + 0.7152 * f(g) + 0.0722 * f(b);
  };
  const ratio = (a, b) => { const x = lum(a); const y = lum(b); return (Math.max(x, y) + 0.05) / (Math.min(x, y) + 0.05); };

  // Things to tap smaller than a finger.
  const small = [];
  for (const e of document.querySelectorAll('button, a[href], input:not([type=hidden]), select, textarea, [role=button]')) {
    const r = shown(e);
    if (!r || e.disabled) continue;
    if (e.matches('input[type=checkbox], input[type=radio]')) { const l = e.closest('label'); const lr = l && shown(l); if (lr && lr.height >= 40) continue; }
    if (e.tagName === 'A' && getComputedStyle(e).display === 'inline') continue; // a link in a sentence
    if (r.height < 40 || r.width < 40) small.push(`${nameOf(e)} (${Math.round(r.width)}×${Math.round(r.height)} px)`);
  }
  // Buttons with the same words, nothing saying which row each is for.
  const names = new Map();
  for (const e of document.querySelectorAll('button, [role=button]')) {
    if (!shown(e)) continue;
    const n = words(e.getAttribute('aria-label') || e.textContent);
    if (n) names.set(n, (names.get(n) || 0) + 1);
  }
  const repeats = [...names].filter(([, c]) => c > 2).map(([n, c]) => `"${n}" ×${c}`);
  // Text that does not read against what is behind it.
  const faint = [];
  let measured = 0;
  const walker = document.createTreeWalker(document.body, NodeFilter.SHOW_TEXT);
  const done = new Set();
  for (let t = walker.nextNode(); t && measured < 600; t = walker.nextNode()) {
    const e = t.parentElement;
    if (!e || done.has(e) || !words(t.data) || e.closest('script, style, option, [aria-hidden=true]') || e.closest('button:disabled, input:disabled')) continue;
    done.add(e);
    if (!shown(e)) continue;
    measured++;
    const cs = getComputedStyle(e);
    const bg = background(e);
    if (!bg) continue;
    let alpha = 1;
    for (let n = e; n && n.nodeType === 1; n = n.parentElement) alpha *= Number(getComputedStyle(n).opacity);
    const fg = rgba(cs.color);
    const r = ratio(over({ ...fg, a: fg.a * alpha }, bg), bg);
    const size = parseFloat(cs.fontSize);
    const large = size >= 24 || (size >= 18.66 && Number(cs.fontWeight) >= 700);
    if (r < (large ? 3 : 4.5)) faint.push({ text: words(t.data).slice(0, 40), ratio: Math.round(r * 10) / 10 });
  }
  faint.sort((a, b) => a.ratio - b.ratio);
  // Row ids shown as they are (an engine id: ten letters and digits).
  const ids = new Set();
  const w2 = document.createTreeWalker(document.body, NodeFilter.SHOW_TEXT);
  for (let t = w2.nextNode(); t && ids.size < 3; t = w2.nextNode()) {
    for (const m of t.data.matchAll(/\b(?=[a-z0-9]*\d)(?=[a-z0-9]*[a-z])[a-z0-9]{10}\b/g)) if (t.parentElement && shown(t.parentElement)) ids.add(m[0]);
  }
  // What sticks out to the right, when the page scrolls sideways.
  let wide = null;
  if (doc.scrollWidth > W + 2) {
    for (const e of document.body.querySelectorAll('*')) {
      const r = e.getBoundingClientRect();
      if (r.right > W + 2 && r.width > 0 && (!wide || r.right > wide.right)) wide = { right: Math.round(r.right), what: `${e.tagName.toLowerCase()}${e.className && typeof e.className === 'string' ? `.${e.className.split(' ')[0]}` : ''}` };
    }
  }
  const fields = [...document.querySelectorAll('input:not([type=hidden]):not([type=checkbox]):not([type=radio]), select, textarea')].filter((e) => { const r = shown(e); return r && r.top < H; }).length;
  return {
    width: W, scroll: doc.scrollWidth, height: doc.scrollHeight, screen: H, wide,
    small: small.slice(0, 6), smallCount: small.length, repeats: repeats.slice(0, 4), faint: faint.slice(0, 4), faintCount: faint.length,
    ids: [...ids], fieldsOnFirstScreen: fields,
    text: words(document.body.innerText).length, controls: document.querySelectorAll('button, input, select, textarea, a[href]').length,
  };
}

// The problems the rules find in one drawn screen, in words, each with how
// much it matters: "high" when it stands in the way.
export function rules(m, device) {
  if (!m) return [];
  const out = [];
  if (m.text < 3 && !m.controls) out.push({ problem: 'the screen draws nothing', severity: 'high' });
  if (m.scroll > m.width + 2) out.push({ problem: `the page is ${m.scroll - m.width} px wider than the ${device} and scrolls sideways${m.wide ? ` (${m.wide.what} reaches ${m.wide.right} px)` : ''}`, severity: 'high' });
  if (device === 'phone' && m.smallCount) out.push({ problem: `${m.smallCount} thing${m.smallCount === 1 ? '' : 's'} to tap ${m.smallCount === 1 ? 'is' : 'are'} smaller than a finger (44 px): ${m.small.slice(0, 3).join(', ')}`, severity: 'high' });
  if (m.faintCount) out.push({ problem: `text that is hard to read: ${m.faint.slice(0, 2).map((f) => `"${f.text}" at ${f.ratio}:1`).join(', ')} (it needs 4.5:1)`, severity: 'high' });
  if (m.ids.length) out.push({ problem: `raw ids are shown: ${m.ids.join(', ')} (show the name of the row instead)`, severity: 'high' });
  if (m.repeats.length) out.push({ problem: `buttons say the same on every row with nothing to tell them apart: ${m.repeats.join(', ')}`, severity: 'low' });
  return out;
}

// Draws every screen of the app (the live version, or version v) on each
// device: [{tab, phone: {m, png}, computer: {m}}], errors per screen kept.
export async function drawApp(baseUrl, appId, { v = null, wait = 1500 } = {}) {
  const pw = loadPlaywright();
  if (!pw || !baseUrl) return null;
  const browser = await pw.chromium.launch();
  const views = new Map();
  try {
    for (const [device, opts] of Object.entries(DEVICES)) {
      const ctx = await browser.newContext({ ...opts, colorScheme: 'light' });
      // Drawing it is not using it: nothing goes to the app's use log.
      await ctx.route('**/track**', (r) => r.fulfill({ status: 200, contentType: 'application/json', body: '{}' }));
      const page = await ctx.newPage();
      let errors = [];
      page.on('pageerror', (e) => errors.push(e.message.split('\n')[0].slice(0, 200)));
      page.on('dialog', (d) => d.dismiss());
      await page.goto(`${baseUrl}/a/${appId}${v ? `?v=${v}` : ''}`, { waitUntil: 'load', timeout: 15000 });
      await page.waitForTimeout(wait);
      const tabs = page.locator('#tabs button.tab[data-slug]');
      const n = Math.min(Math.max(await tabs.count(), 1), MAX_VIEWS);
      for (let i = 0; i < n; i++) {
        let tab = '';
        let slug = '';
        if (await tabs.count()) {
          tab = (await tabs.nth(i).innerText()).trim();
          slug = await tabs.nth(i).getAttribute('data-slug');
          if (i > 0 || !(await tabs.nth(i).isVisible())) { await tabs.nth(i).evaluate((b) => b.click()); await page.waitForTimeout(wait); }
          await page.evaluate(() => window.scrollTo(0, 0));
        }
        const el = await page.locator('iframe:visible').first().elementHandle().catch(() => null);
        const frame = el && await el.contentFrame();
        const top = el ? (await el.boundingBox())?.y || 0 : 0;
        const m = frame ? await frame.evaluate(measure, opts.viewport.height - top).catch((e) => ({ error: e.message })) : null;
        const entry = views.get(i) || { tab, slug };
        entry[device] = { m: m && !m.error ? m : null, errors: [...new Set(errors)] };
        if (device === 'phone') entry.phone.png = (await page.screenshot({ type: 'png' })).toString('base64');
        views.set(i, entry);
        errors = [];
      }
      await ctx.close();
    }
  } finally { await browser.close(); }
  return { views: [...views.values()] };
}

const LOOK_SYSTEM = `You check the screens of an app the way a UX researcher would, against the app's brief: who uses it, the job they come back for, how often, on what device, which screen opens first and what it should show at a glance. You see each screen as it is drawn on a phone, its first screenful. Name only problems you can see, the ones that stand in the way of doing the job quickly and surely on that device, most important first. For each, say the screen (its tab name), the problem in plain words, the fix as a request to whoever builds the app, and "high" when it stands in the way of the job (the job is not on the first screen without scrolling, a form must be filled for something done daily, a step is missing, something does not read or does not work) or "low" for polish. When the screens serve the brief well, say so and list nothing. Answer with JSON alone: {"ok": true or false, "problems": [{"view": "...", "problem": "...", "fix": "...", "severity": "high" or "low"}]}`;

// One look by the model at the phone screens, against the brief.
export async function lookAt({ app, brief, views, model = LOOK_MODEL }) {
  const shots = views.filter((x) => x.phone?.png).slice(0, MAX_VIEWS);
  if (!model || !shots.length) return { problems: [], cost: 0, ran: false };
  const about = brief
    ? `Who: ${brief.who}\nJob: ${brief.job}\nHow often: ${brief.often}\nDevice: ${brief.device}\nOpens first: ${brief.first}\nAt a glance: ${brief.glance}`
    : `(no brief: judge from the app's name and screens)`;
  const prompt = `# The app: ${app.name}\n${app.description ? `Described as: ${app.description}\n` : ''}\n# Its brief\n${about}\n\n# The screens, in order (one picture each, the phone's first screenful)\n${shots.map((x, i) => `${i + 1}. ${x.tab || 'the app'}${x.slug ? ` (slug ${x.slug})` : ''}: the page is ${x.phone.m ? `${(x.phone.m.height / x.phone.m.screen).toFixed(1)} phone screens long, with ${x.phone.m.fieldsOnFirstScreen} field${x.phone.m.fieldsOnFirstScreen === 1 ? '' : 's'} to fill on the first one` : 'not measured'}`).join('\n')}\n\nAt most 4 problems. JSON alone.`;
  const answer = await ask({ system: LOOK_SYSTEM, prompt, images: shots.map((x) => ({ data: x.phone.png })), model, idleMs: 90000, timeoutMs: 180000 });
  const out = extractJson(answer.text);
  const problems = (Array.isArray(out.problems) ? out.problems : []).filter((p) => p && p.problem).slice(0, 4)
    .map((p) => ({ view: String(p.view || ''), problem: String(p.problem).slice(0, 400), fix: String(p.fix || '').slice(0, 400), severity: p.severity === 'high' ? 'high' : 'low' }));
  return { problems, ok: !!out.ok && !problems.some((p) => p.severity === 'high'), cost: answer.cost || 0, ran: true };
}

// The whole check of one version: drawn, measured, looked at.
// {ran, views: [{tab, phone, computer}], found: [{view, device, problem, fix, severity, by}], cost}
export async function checkDrawn(engine, appId, { v = null } = {}) {
  const view = await engine.view(appId);
  const drawn = await drawApp(engine.baseUrl, appId, { v });
  if (!drawn) return { ran: false, views: [], found: [], cost: 0 };
  const found = [];
  for (const d of drawn.views) {
    for (const device of Object.keys(DEVICES)) {
      for (const r of rules(d[device]?.m, device)) found.push({ view: d.tab, device, ...r, by: 'rule' });
    }
  }
  let look = { problems: [], cost: 0, ran: false };
  try { look = await lookAt({ app: view.app, brief: briefOf(view.app), views: drawn.views }); } catch (e) { look.error = e.message; }
  for (const p of look.problems) found.push({ ...p, device: 'phone', by: 'look' });
  return {
    ran: true, cost: look.cost, looked: look.ran, lookError: look.error || null,
    views: drawn.views.map((d) => ({ tab: d.tab, slug: d.slug, phone: d.phone?.m || null, computer: d.computer?.m || null, errors: [...(d.phone?.errors || []), ...(d.computer?.errors || [])] })),
    found,
  };
}

// The request the repair gets for what stands in the way, or null.
export function repairRequest(found) {
  const high = found.filter((f) => f.severity === 'high');
  if (!high.length) return null;
  const line = (f) => `- ${f.view || 'the app'} (${f.device}): ${f.problem.replace(/[.\s]+$/, '')}${f.fix ? `. Fix: ${f.fix}` : ''}`;
  return `The screens were drawn on a phone (390 px) and a computer (1280 px) and checked against the app's brief. Fix these, changing only what each fix needs and keeping everything else as it is:\n${high.slice(0, 8).map(line).join('\n')}`;
}
