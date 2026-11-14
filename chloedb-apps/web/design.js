// Designs: how an app looks, as a few tokens (colors, fonts, corners,
// density, surfaces, navigation, width) and nothing else. The stylesheet
// every view gets (kit.css) is written against CSS variables, so a design is
// a block of variables: switching one changes no code, rebuilds nothing and
// is drawn at once. Shared by the engine (checks, presets) and the studio
// (drawing), so it has no imports.

export const FONTS = {
  system: '-apple-system, BlinkMacSystemFont, "SF Pro Text", "Inter", "Segoe UI", Roboto, system-ui, sans-serif',
  rounded: 'ui-rounded, "SF Pro Rounded", "Nunito", "Varela Round", -apple-system, BlinkMacSystemFont, system-ui, sans-serif',
  serif: '"New York", ui-serif, "Iowan Old Style", "Charter", "Source Serif Pro", Georgia, serif',
  mono: 'ui-monospace, "SF Mono", "JetBrains Mono", Menlo, Consolas, monospace',
};
const TITLE_FONTS = { ...FONTS, system: '-apple-system, BlinkMacSystemFont, "SF Pro Display", "Inter", "Segoe UI", Roboto, system-ui, sans-serif' };

// Type pairings: a display face for titles and big numbers over the body
// text. The faces ship with the engine (web/fonts, SIL Open Font License,
// declared in kit.css and studio.css) and load only when a design uses
// them. weight is the titles' weight (a face with one weight keeps it).
export const TYPES = {
  system: { title: TITLE_FONTS.system, weight: 650, track: '-.02em' },
  ledger: { title: '"Fraunces", "Iowan Old Style", Georgia, serif', weight: 600, track: '-.015em' },
  classic: { title: '"Playfair Display", "Didot", Georgia, serif', weight: 650, track: '-.01em' },
  elegant: { title: '"Instrument Serif", "Iowan Old Style", Georgia, serif', weight: 400, track: '-.005em' },
  grotesk: { title: '"Space Grotesk", -apple-system, system-ui, sans-serif', weight: 600, track: '-.025em' },
  bricolage: { title: '"Bricolage Grotesque", -apple-system, system-ui, sans-serif', weight: 700, track: '-.03em' },
  rounded: { title: '"Nunito", ui-rounded, "SF Pro Rounded", system-ui, sans-serif', weight: 750, track: '-.01em', body: '"Nunito", ui-rounded, "SF Pro Rounded", -apple-system, system-ui, sans-serif' },
  mono: { title: '"JetBrains Mono", ui-monospace, "SF Mono", Menlo, monospace', weight: 650, track: '-.03em' },
};
// A design's titles and pairing for an app's subject: its own pairing when
// it chose one of its titles' family (other than plain system), else the one
// the subject suggests; a report, a ledger or the books of a firm get the
// warm serif even when the titles were left plain.
const FORMAL = /(board|report|financ|ledger|accounting|bookkeep|invoice|variance|aging|receivable|budget|cash ?flow|legal|law firm)/i;
export function pairFor(d, subject = '') {
  const titles = CHOICES.titles[d?.titles] ? d.titles : 'system';
  if (TYPES[d?.type] && d.type !== 'system' && TYPE_FAMILY[d.type] === titles) return { titles, type: d.type };
  if (titles === 'system' && FORMAL.test(String(subject))) return { titles: 'serif', type: 'ledger' };
  if (titles === 'system' && /\b(agency|studio|consultanc|atelier|architect)/i.test(String(subject))) return { titles: 'serif', type: 'elegant' };
  return { titles, type: typeOf({ titles }, subject) };
}
// The title families each pairing belongs to (the "titles" token).
const TYPE_FAMILY = { system: 'system', ledger: 'serif', classic: 'serif', elegant: 'serif', grotesk: 'system', bricolage: 'system', rounded: 'rounded', mono: 'mono' };
// The pairing a design uses: its own when it fits its titles, else the one
// its titles and its subject suggest (a ledger's warm serif, a salon's
// high-contrast one, a studio's light one; a technical grotesque for data,
// a bold one for clubs and events).
export function typeOf(d, subject = '') {
  const titles = d?.titles || 'system';
  if (TYPES[d?.type] && TYPE_FAMILY[d.type] === titles) return d.type;
  const t = String(subject || '').toLowerCase();
  if (titles === 'serif') {
    if (/salon|hair|beauty|fashion|wedding|hotel|wine|jewel|luxury|spa|perfume|boutique/.test(t)) return 'classic';
    if (/studio|agency|portfolio|gallery|architect|design|photo|art\b|ceramic|poetry|museum/.test(t)) return 'elegant';
    return 'ledger';
  }
  if (titles === 'rounded') return 'rounded';
  if (titles === 'mono') return 'mono';
  if (/club|run|race|sport|team|gym|fitness|league|event|festival|truck|taco|street|music|kids|game/.test(t)) return 'bricolage';
  if (/dashboard|data|api|engineer|metric|analytics|sales|report|server|database|code|dev|crm|pipeline/.test(t)) return 'grotesk';
  return 'system';
}

// What each token may be, and what it means (the model is told this).
export const CHOICES = {
  scheme: { auto: 'follows the device: light by day, dark at night', light: 'always light', dark: 'always dark' },
  font: { system: 'San Francisco, neutral', rounded: 'SF Rounded, friendly', serif: 'New York, editorial', mono: 'SF Mono, technical' },
  titles: { system: 'San Francisco', rounded: 'SF Rounded', serif: 'New York', mono: 'SF Mono' },
  type: { system: 'system titles, quiet', ledger: 'Fraunces, a warm serif (ledgers, food, craft, books)', classic: 'Playfair Display, a high-contrast serif (salons, fashion, hospitality)', elegant: 'Instrument Serif, a light display serif (studios, agencies, portfolios)', grotesk: 'Space Grotesk, a technical sans (data, engineering, tools)', bricolage: 'Bricolage Grotesque, a bold sans with character (clubs, sports, events, street food)', rounded: 'Nunito, friendly and rounded (families, learning)', mono: 'JetBrains Mono (technical)' },
  corners: { round: 'capsule buttons and 16px cards', soft: '7-10px corners, precise', square: '4px corners, crisp' },
  density: { airy: 'larger text, more space', regular: 'balanced: 14px text, 32px controls', compact: 'dense, for many rows and fields' },
  surface: { outlined: 'hairline boxes on a neutral page, like a console', grouped: 'borderless white cards on a tinted page (iOS Settings)', raised: 'cards lifted by a soft shadow' },
  nav: { segmented: 'a centered segmented control', tabs: 'underlined tabs, left aligned', sidebar: 'a list on the left (for apps with many views)' },
  width: { narrow: '760px: reading', regular: '1040px', wide: '1280px: tables, forms and boards' },
  motion: { calm: 'slow, quiet movement on front pages', lively: 'quicker, more movement', still: 'nothing moves by itself' },
  look: { native: 'like a good phone app: grouped white lists on a grey page, a sentence for the numbers, each row a cell with an icon, one line of detail and one action', plain: 'rows as tables and label-value cards, numbers in tiles', bold: 'a color band with the one number that matters, each row a card with its number large, its state as a pill and its next step as a button' },
};

const C = (light, dark) => ({ light, dark });
export const PRESETS = [
  { name: 'Console', about: 'Neutral and precise, like a database console', scheme: 'auto',
    ...C({ bg: '#f4f5f7', card: '#ffffff', fg: '#0f1216', accent: '#2f5bd3' }, { bg: '#0d0f12', card: '#16181c', fg: '#eceef1', accent: '#7aa2ff' }),
    font: 'system', titles: 'system', corners: 'soft', density: 'regular', surface: 'outlined', nav: 'tabs', width: 'wide', gradient: ['#2f5bd3', '#6250d6', '#1baf7a'], motion: 'calm' },
  { name: 'Classic', about: 'Apple\'s own: quiet gray page, white cards, blue', scheme: 'auto',
    ...C({ bg: '#f5f5f7', card: '#ffffff', fg: '#1d1d1f', accent: '#0071e3' }, { bg: '#000000', card: '#1c1c1e', fg: '#f5f5f7', accent: '#2997ff' }),
    font: 'system', titles: 'system', corners: 'round', density: 'regular', surface: 'grouped', nav: 'segmented', width: 'regular', gradient: ['#2997ff', '#a259ff', '#ff6b9a'] },
  { name: 'Paper', about: 'Warm paper, serif titles, ink and sienna', scheme: 'auto',
    ...C({ bg: '#fbfaf6', card: '#ffffff', fg: '#23201b', accent: '#a0461b' }, { bg: '#151412', card: '#1e1c19', fg: '#efebe3', accent: '#f0a36b' }),
    font: 'system', titles: 'serif', corners: 'soft', density: 'airy', surface: 'outlined', nav: 'tabs', width: 'narrow', gradient: ['#f0a36b', '#d9636b', '#f6d08b'] },
  { name: 'Graphite', about: 'Monochrome and dense, for work', scheme: 'auto',
    ...C({ bg: '#f2f2f2', card: '#ffffff', fg: '#111111', accent: '#1d1d1f' }, { bg: '#0e0e0f', card: '#18181a', fg: '#ededed', accent: '#f5f5f7' }),
    font: 'system', titles: 'system', corners: 'square', density: 'compact', surface: 'outlined', nav: 'sidebar', width: 'wide', gradient: ['#8e8e93', '#48484a', '#c7c7cc'], motion: 'still' },
  { name: 'Midnight', about: 'Deep night blue with a bright accent', scheme: 'dark',
    ...C({ bg: '#0b0d12', card: '#151922', fg: '#f2f4f8', accent: '#4c8dff' }, { bg: '#0b0d12', card: '#151922', fg: '#f2f4f8', accent: '#4c8dff' }),
    font: 'system', titles: 'system', corners: 'round', density: 'regular', surface: 'raised', nav: 'segmented', width: 'regular', gradient: ['#4c8dff', '#7b5cff', '#00d4ff'], motion: 'lively' },
  { name: 'Mint', about: 'Fresh green on a pale page, rounded type', scheme: 'auto',
    ...C({ bg: '#f1f6f4', card: '#ffffff', fg: '#0f241c', accent: '#0b8a63' }, { bg: '#06110d', card: '#0f1f19', fg: '#e6f4ee', accent: '#3ad29f' }),
    font: 'rounded', titles: 'rounded', corners: 'round', density: 'airy', surface: 'grouped', nav: 'segmented', width: 'narrow', gradient: ['#3ad29f', '#00b4d8', '#b8f28a'] },
  { name: 'Coral', about: 'Warm and playful, with lifted cards', scheme: 'auto',
    ...C({ bg: '#fff6f2', card: '#ffffff', fg: '#2a1712', accent: '#d9480f' }, { bg: '#140c09', card: '#221613', fg: '#f8ebe5', accent: '#ff8a5c' }),
    font: 'rounded', titles: 'rounded', corners: 'round', density: 'regular', surface: 'raised', nav: 'tabs', width: 'regular', gradient: ['#ff8a5c', '#ff4f8b', '#ffc15e'], motion: 'lively' },
  { name: 'Lavender', about: 'Calm violet, serif headings, soft corners', scheme: 'auto',
    ...C({ bg: '#f6f5fb', card: '#ffffff', fg: '#1d1a2c', accent: '#6550c8' }, { bg: '#0d0c14', card: '#18162a', fg: '#eeecf8', accent: '#a493ff' }),
    font: 'system', titles: 'serif', corners: 'soft', density: 'regular', surface: 'grouped', nav: 'segmented', width: 'regular', gradient: ['#a493ff', '#ff8fd8', '#7cc6ff'] },
  { name: 'Studio', about: 'Bright white, black type, a sidebar', scheme: 'light',
    ...C({ bg: '#ffffff', card: '#ffffff', fg: '#0a0a0a', accent: '#0066cc' }, { bg: '#ffffff', card: '#ffffff', fg: '#0a0a0a', accent: '#0066cc' }),
    font: 'system', titles: 'system', corners: 'soft', density: 'regular', surface: 'outlined', nav: 'sidebar', width: 'wide', gradient: ['#0066cc', '#5e5ce6', '#64d2ff'] },
];

// ---- colors

const HEX = /^#[0-9a-f]{6}$/i;
const rgb = (h) => [1, 3, 5].map((i) => parseInt(h.slice(i, i + 2), 16));
const hex = (c) => `#${c.map((x) => Math.max(0, Math.min(255, Math.round(x))).toString(16).padStart(2, '0')).join('')}`;
const lum = (h) => {
  const [r, g, b] = rgb(h).map((v) => { const s = v / 255; return s <= 0.03928 ? s / 12.92 : ((s + 0.055) / 1.055) ** 2.4; });
  return 0.2126 * r + 0.7152 * g + 0.0722 * b;
};
export const contrast = (a, b) => { const [x, y] = [lum(a), lum(b)].sort((p, q) => q - p); return (x + 0.05) / (y + 0.05); };
const mix = (a, b, t) => hex(rgb(a).map((v, i) => v + (rgb(b)[i] - v) * t));
// The text color that reads on a color: white or near black.
export const onColor = (h) => (contrast(h, '#ffffff') >= contrast(h, '#111111') ? '#ffffff' : '#111111');
// Moves a color toward another until it reads at `ratio` on `on`.
function readable(c, on, ratio, toward) {
  let out = c;
  for (let t = 0.08; contrast(out, on) < ratio && t <= 1; t += 0.08) out = mix(c, toward, t);
  return out;
}

// A palette that reads: text on the page and on cards at 7:1, the accent as
// text on cards at 4.5:1 (it is moved toward the text color until it does).
function palette(p, fallback, dark) {
  const ok = (k) => (typeof p?.[k] === 'string' && HEX.test(p[k]) ? p[k].toLowerCase() : fallback[k]);
  const out = { bg: ok('bg'), card: ok('card'), fg: ok('fg'), accent: ok('accent') };
  const ink = dark ? '#ffffff' : '#000000';
  if (contrast(out.fg, out.bg) < 7 || contrast(out.fg, out.card) < 7) out.fg = readable(out.fg, contrast(out.bg, ink) < contrast(out.card, ink) ? out.bg : out.card, 7, ink);
  if (contrast(out.accent, out.card) < 4.5) out.accent = readable(out.accent, out.card, 4.5, out.fg);
  return out;
}

const pick = (v, k, d) => (Object.hasOwn(CHOICES[k], v) ? v : d);

// A gradient of three colors for front pages: as given, or drawn from the
// accent (it, then two neighbours on the color wheel).
const hsl = (h) => {
  const [r, g, b] = rgb(h).map((v) => v / 255);
  const mx = Math.max(r, g, b); const mn = Math.min(r, g, b); const l = (mx + mn) / 2;
  if (mx === mn) return [0, 0, l];
  const d = mx - mn; const s = l > 0.5 ? d / (2 - mx - mn) : d / (mx + mn);
  const hh = mx === r ? (g - b) / d + (g < b ? 6 : 0) : mx === g ? (b - r) / d + 2 : (r - g) / d + 4;
  return [hh * 60, s, l];
};
// An accent's hue (0-360), saturation and lightness (0-1), for checks on a palette.
export const hslOf = (h) => (HEX.test(String(h)) ? hsl(String(h).toLowerCase()) : [0, 0, 0]);
const fromHsl = (hh, s, l) => {
  const k = (n) => (n + hh / 30) % 12; const a = s * Math.min(l, 1 - l);
  return hex([0, 8, 4].map((n) => 255 * (l - a * Math.max(-1, Math.min(k(n) - 3, 9 - k(n), 1)))));
};
export function gradientOf(d, accent) {
  if (Array.isArray(d?.gradient) && d.gradient.length >= 2 && d.gradient.every((c) => typeof c === 'string' && HEX.test(c))) {
    const g = d.gradient.slice(0, 3).map((c) => c.toLowerCase());
    return g.length === 3 ? g : [g[0], g[1], mix(g[0], g[1], 0.5)];
  }
  const [hh, s, l] = hsl(accent);
  const sat = Math.min(0.74, Math.max(0.5, s));
  const lit = Math.min(0.6, Math.max(0.5, l));
  // Warm accents lean to pink and gold (a sunset), cool ones to violet and cyan.
  const warm = hh < 60 || hh >= 300;
  const [a, b] = warm ? [325, 28] : [45, 330];
  return [fromHsl(hh, sat, lit), fromHsl((hh + a) % 360, sat, lit), fromHsl((hh + b) % 360, sat, Math.min(0.7, lit + 0.08))];
}
const words = (v, n, d) => (typeof v === 'string' && v.trim() ? v.trim().replace(/\s+/g, ' ').slice(0, n) : d);

// A design as given (by the model, a person or a preset), made whole and
// readable: unknown tokens fall back to Console's, colors are checked.
export function checkDesign(d, base = PRESETS[0]) {
  const scheme = pick(d?.scheme, 'scheme', base.scheme);
  const light = palette(d?.light || d?.dark, base.light, false);
  const dark = palette(d?.dark || (scheme === 'dark' ? d?.light : null), base.dark, true);
  return {
    name: words(d?.name, 24, base.name),
    about: words(d?.about, 90, ''),
    scheme,
    light: scheme === 'dark' ? dark : light,
    dark: scheme === 'light' ? light : dark,
    font: pick(d?.font, 'font', base.font),
    titles: pick(d?.titles, 'titles', base.titles),
    type: typeOf({ titles: pick(d?.titles, 'titles', base.titles), type: d?.type }, `${d?.name || ''} ${d?.about || ''}`),
    corners: pick(d?.corners, 'corners', base.corners),
    density: pick(d?.density, 'density', base.density),
    surface: pick(d?.surface, 'surface', base.surface),
    nav: pick(d?.nav, 'nav', base.nav),
    width: pick(d?.width, 'width', base.width),
    motion: pick(d?.motion, 'motion', base.motion || 'calm'),
    look: pick(d?.look, 'look', base.look || 'native'),
    gradient: gradientOf(d?.gradient ? d : base.gradient && !d?.light && !d?.dark ? base : { gradient: null }, (d?.light || base.light)?.accent || base.light.accent),
  };
}

// ---- CSS

const CORNERS = { round: { card: '16px', control: '999px', field: '9px', small: '8px' }, soft: { card: '10px', control: '7px', field: '7px', small: '6px' }, square: { card: '4px', control: '4px', field: '4px', small: '3px' } };
const DENSITY = { airy: { text: '15px', space: '20px', pad: '20px', row: '12px', control: '38px' }, regular: { text: '14px', space: '16px', pad: '16px', row: '9px', control: '32px' }, compact: { text: '13px', space: '12px', pad: '12px', row: '6px', control: '28px' } };
const WIDTH = { narrow: '760px', regular: '1040px', wide: '1280px' };
const STATUS = { light: { ok: '#18794e', warn: '#b4560a', bad: '#c4302b', info: '#2563c9' }, dark: { ok: '#3fcf8e', warn: '#f2a33a', bad: '#ff8a80', info: '#7cb4ff' } };

// Secondary text: the design's grey, darkened (or lightened) until it reads
// at 4.5:1 on the page and on a card (WCAG AA), however pale the design.
export function mutedOf(p, start = 55) {
  for (let k = start; k <= 100; k++) {
    const m = mix(p.bg, p.fg, k / 100);
    if (contrast(m, p.bg) >= 4.6 && contrast(m, p.card) >= 4.6) return m;
  }
  return p.fg;
}

// The color pair of the bold look, from the accent: deep (a band and the
// main buttons, white text on it at 8:1) and pop (a light tint of the same
// hue, for big words on the band and for its button, at 5:1 on deep); wash
// is the page, tinted by the accent.
export function boldColors(p, isDark) {
  const deep = readable(p.accent, '#ffffff', 8, '#000000');
  const [hh, s] = hsl(p.accent);
  let pop = fromHsl(hh, Math.min(0.85, Math.max(0.55, s)), 0.74);
  for (let l = 0.76; contrast(pop, deep) < 5 && l <= 0.96; l += 0.02) pop = fromHsl(hh, Math.min(0.85, Math.max(0.55, s)), l);
  const wash = isDark ? p.bg : mix(p.bg, p.accent, 0.06);
  return { deep, deepFg: onColor(deep), pop, wash };
}

// A design's colors on the native look's page and cells.
// The design's own page and cards (its tint is the app's), with cards that
// stand off a page of their own color by a step of grey.
export function nativePalette(p, isDark) {
  const bg = contrast(p.bg, p.card) < 1.03 ? (isDark ? '#000000' : mix(p.card, p.fg, 0.045)) : p.bg;
  const card = p.card;
  const ink = isDark ? '#ffffff' : '#000000';
  const fg = contrast(p.fg, bg) >= 7 && contrast(p.fg, card) >= 7 ? p.fg : readable(p.fg, isDark ? card : bg, 7, ink);
  const accent = contrast(p.accent, card) >= 4.5 && contrast(p.accent, bg) >= 4.5 ? p.accent : readable(p.accent, isDark ? card : bg, 4.5, fg);
  return { bg, card, fg, accent };
}

// The variables of a design in one scheme.
export function designVars(input, which = 'light') {
  const d = checkDesign(input);
  const isDark = lum(d[which].bg) < 0.2;
  // The native look: the phone's own grouped page and white (or dark grey)
  // cells; an app is told apart by its tint, the accent, kept readable on them.
  const native = d.look === 'native';
  const p = native ? nativePalette(d[which], isDark) : d[which];
  const k = CORNERS[d.corners];
  const n = DENSITY[d.density];
  // Cards that would vanish on their page (same color, grouped) get a hairline.
  const flat = contrast(p.bg, p.card) < 1.04;
  const border = d.surface === 'outlined' || flat ? 'var(--line)' : isDark ? 'rgba(255,255,255,.06)' : 'transparent';
  const shadow = isDark ? 'none' : d.surface === 'raised' ? '0 1px 2px rgba(16,24,40,.05), 0 8px 24px rgba(16,24,40,.07)' : d.surface === 'outlined' ? '0 1px 2px rgba(16,24,40,.05)' : flat ? 'none' : '0 1px 2px rgba(16,24,40,.04)';
  const s = STATUS[isDark ? 'dark' : 'light'];
  const b = boldColors(p, isDark);
  const bold = d.look === 'bold';
  return {
    '--bg': bold ? b.wash : p.bg, '--card': p.card, '--fg': p.fg, '--accent': p.accent, '--accent-fg': onColor(p.accent),
    '--ok': s.ok, '--warn': s.warn, '--bad': s.bad, '--info': s.info,
    '--font': d.font === 'system' && TYPES[d.type].body ? TYPES[d.type].body : FONTS[d.font], '--font-title': TYPES[d.type].title, '--title-weight': String(TYPES[d.type].weight), '--title-track': TYPES[d.type].track, '--type': d.type,
    '--radius': k.card, '--radius-control': k.control, '--radius-field': k.field, '--radius-small': k.small,
    '--text': n.text, '--space': n.space, '--pad': n.pad, '--row': n.row, '--control': n.control, '--page': WIDTH[d.width],
    '--card-border': border, '--card-shadow': shadow, '--field-border': 'var(--line-2)',
    '--field-bg': isDark ? 'color-mix(in srgb, var(--fg) 3%, var(--card))' : 'var(--card)',
    '--muted': mutedOf(p, isDark ? 58 : 55),
    '--line': `color-mix(in srgb, var(--fg) ${isDark ? 11 : 10}%, transparent)`,
    '--line-2': `color-mix(in srgb, var(--fg) ${isDark ? 19 : 17}%, transparent)`,
    '--soft': `color-mix(in srgb, var(--fg) ${isDark ? 6 : 4}%, transparent)`,
    '--fill': `color-mix(in srgb, var(--fg) ${isDark ? 8 : 5}%, var(--card))`,
    '--fill-solid': mix(p.card, p.fg, isDark ? 0.1 : 0.06),
    '--head': `color-mix(in srgb, var(--fg) ${isDark ? 3 : 2.5}%, var(--card))`,
    '--accent-soft': `color-mix(in srgb, var(--accent) ${isDark ? 20 : 12}%, transparent)`,
    '--seg-on': isDark ? 'color-mix(in srgb, var(--fg) 14%, var(--card))' : 'var(--card)',
    '--shadow': shadow,
    '--g1': d.gradient[0], '--g2': d.gradient[1], '--g3': d.gradient[2],
    '--slow': d.motion === 'lively' ? '.6' : '1', '--play': d.motion === 'still' ? 'paused' : 'running', '--rv': d.motion === 'still' ? '0' : '1',
    '--look': d.look, '--deep': b.deep, '--deep-fg': b.deepFg, '--pop': b.pop, '--wash': b.wash,
  };
}

const block = (v) => Object.entries(v).map(([k, x]) => `${k}: ${x};`).join(' ');
// A design as CSS for a selector (":root" in a view's frame; the stage around
// the frames in the studio).
export function designCss(input, sel = ':root') {
  const d = checkDesign(input);
  if (d.scheme !== 'auto') return `${sel} { ${block(designVars(d, d.scheme))} color-scheme: ${d.scheme}; }`;
  return `${sel} { ${block(designVars(d, 'light'))} color-scheme: light; }\n@media (prefers-color-scheme: dark) { ${sel} { ${block(designVars(d, 'dark'))} color-scheme: dark; } }`;
}

// Three presets for an app with no designs of its own yet, picked by its id
// so each app gets its own three, always one of each navigation.
export function presetDesigns(seed = '') {
  let hsh = 0;
  for (const ch of String(seed)) hsh = (hsh * 31 + ch.charCodeAt(0)) >>> 0;
  const by = (nav) => PRESETS.filter((p) => p.nav === nav);
  // the one in use at first is a light one (a dark page is a choice, not a default)
  return ['segmented', 'tabs', 'sidebar'].map((nav, i) => { const list = by(nav).filter((p) => i > 0 || p.scheme !== 'dark'); return checkDesign(list[(hsh + i) % list.length]); });
}
