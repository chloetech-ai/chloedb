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

// What each token may be, and what it means (the model is told this).
export const CHOICES = {
  scheme: { auto: 'follows the device: light by day, dark at night', light: 'always light', dark: 'always dark' },
  font: { system: 'San Francisco, neutral', rounded: 'SF Rounded, friendly', serif: 'New York, editorial', mono: 'SF Mono, technical' },
  titles: { system: 'San Francisco', rounded: 'SF Rounded', serif: 'New York', mono: 'SF Mono' },
  corners: { round: 'capsule buttons and 20px cards', soft: '10-14px corners', square: '6px corners, crisp' },
  density: { airy: 'large text, generous space', regular: 'balanced', compact: 'dense, for many rows' },
  surface: { grouped: 'white cards on a tinted page (iOS Settings)', raised: 'cards lifted by a soft shadow', outlined: 'hairline boxes on a plain page' },
  nav: { segmented: 'a centered segmented control', tabs: 'underlined tabs, left aligned', sidebar: 'a list on the left (for apps with many views)' },
  width: { narrow: '720px: reading and forms', regular: '980px', wide: '1240px: tables and boards' },
  motion: { calm: 'slow, quiet movement on front pages', lively: 'quicker, more movement', still: 'nothing moves by itself' },
};

const C = (light, dark) => ({ light, dark });
export const PRESETS = [
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
// readable: unknown tokens fall back to Classic's, colors are checked.
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
    corners: pick(d?.corners, 'corners', base.corners),
    density: pick(d?.density, 'density', base.density),
    surface: pick(d?.surface, 'surface', base.surface),
    nav: pick(d?.nav, 'nav', base.nav),
    width: pick(d?.width, 'width', base.width),
    motion: pick(d?.motion, 'motion', base.motion || 'calm'),
    gradient: gradientOf(d?.gradient ? d : base.gradient && !d?.light && !d?.dark ? base : { gradient: null }, (d?.light || base.light)?.accent || base.light.accent),
  };
}

// ---- CSS

const CORNERS = { round: { card: '22px', control: '999px', field: '12px', small: '10px' }, soft: { card: '14px', control: '10px', field: '10px', small: '8px' }, square: { card: '6px', control: '6px', field: '6px', small: '4px' } };
const DENSITY = { airy: { text: '16px', space: '22px', pad: '22px', row: '14px' }, regular: { text: '15px', space: '16px', pad: '18px', row: '11px' }, compact: { text: '14px', space: '12px', pad: '14px', row: '7px' } };
const WIDTH = { narrow: '720px', regular: '980px', wide: '1240px' };
const STATUS = { light: { ok: '#248a3d', warn: '#c93400', bad: '#d70015', info: '#0066cc' }, dark: { ok: '#30d158', warn: '#ff9f0a', bad: '#ff6961', info: '#64d2ff' } };

// The variables of a design in one scheme.
export function designVars(input, which = 'light') {
  const d = checkDesign(input);
  const p = d[which];
  const isDark = lum(p.bg) < 0.2;
  const k = CORNERS[d.corners];
  const n = DENSITY[d.density];
  // Cards that would vanish on their page (same color, grouped) get a hairline.
  const flat = contrast(p.bg, p.card) < 1.04;
  const border = d.surface === 'outlined' || flat ? 'var(--line)' : isDark && d.surface === 'raised' ? 'rgba(255,255,255,.07)' : 'transparent';
  const shadow = d.surface === 'raised' && !isDark ? '0 1px 2px rgba(0,0,0,.04), 0 8px 28px rgba(0,0,0,.07)' : d.surface === 'grouped' && !isDark && !flat ? '0 1px 1px rgba(0,0,0,.02)' : 'none';
  const s = STATUS[isDark ? 'dark' : 'light'];
  return {
    '--bg': p.bg, '--card': p.card, '--fg': p.fg, '--accent': p.accent, '--accent-fg': onColor(p.accent),
    '--ok': s.ok, '--warn': s.warn, '--bad': s.bad, '--info': s.info,
    '--font': FONTS[d.font], '--font-title': TITLE_FONTS[d.titles],
    '--radius': k.card, '--radius-control': k.control, '--radius-field': k.field, '--radius-small': k.small,
    '--text': n.text, '--space': n.space, '--pad': n.pad, '--row': n.row, '--page': WIDTH[d.width],
    '--card-border': border, '--card-shadow': shadow, '--field-border': d.surface === 'outlined' ? 'var(--line)' : 'transparent',
    '--muted': `color-mix(in srgb, var(--fg) ${isDark ? 62 : 56}%, var(--bg))`,
    '--line': `color-mix(in srgb, var(--fg) ${isDark ? 16 : 11}%, transparent)`,
    '--soft': `color-mix(in srgb, var(--fg) ${isDark ? 8 : 4.5}%, transparent)`,
    '--fill': `color-mix(in srgb, var(--fg) ${isDark ? 11 : 6}%, var(--card))`,
    '--accent-soft': `color-mix(in srgb, var(--accent) ${isDark ? 22 : 12}%, transparent)`,
    '--seg-on': isDark ? 'color-mix(in srgb, var(--fg) 20%, var(--card))' : 'var(--card)',
    '--shadow': shadow,
    '--g1': d.gradient[0], '--g2': d.gradient[1], '--g3': d.gradient[2],
    '--slow': d.motion === 'lively' ? '.6' : '1', '--play': d.motion === 'still' ? 'paused' : 'running', '--rv': d.motion === 'still' ? '0' : '1',
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
  return ['segmented', 'tabs', 'sidebar'].map((nav, i) => { const list = by(nav); return checkDesign(list[(hsh + i) % list.length]); });
}
