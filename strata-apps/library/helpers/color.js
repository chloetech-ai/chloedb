// title: Colour
// description: Colour arithmetic for charts, tags and avatars: parse and print colours, mix, lighten and darken, the contrast between two colours, the text colour that reads on a background, a stable colour for any name, and palettes.
// api: parse('#3b82f6' | '#38f' | 'rgb(59,130,246)') -> {r, g, b} or null; hex({r, g, b}) -> '#3b82f6';
//   mix(a, b, t = 0.5) -> hex; lighten(c, amount = 0.2) and darken(c, amount = 0.2) -> hex;
//   contrast(a, b) -> WCAG ratio, 1 to 21 (4.5 or more reads well); readable(bg) -> '#111111' or '#ffffff';
//   forName(text) -> a hex colour, always the same for the same text (avatars, tags);
//   palette(n, {saturation = 0.62, lightness = 0.52, start = 210}) -> n hex colours spread round the wheel;
//   scale(value, min, max, from = '#dbeafe', to = '#1d4ed8') -> hex between the two (heatmaps)
function helper() {
  const clamp = (x) => Math.max(0, Math.min(255, Math.round(x)));
  function parse(c) {
    if (c && typeof c === 'object' && 'r' in c) return { r: clamp(c.r), g: clamp(c.g), b: clamp(c.b) };
    const s = String(c || '').trim().toLowerCase();
    let m = s.match(/^#?([0-9a-f]{3})$/);
    if (m) return { r: parseInt(m[1][0] + m[1][0], 16), g: parseInt(m[1][1] + m[1][1], 16), b: parseInt(m[1][2] + m[1][2], 16) };
    m = s.match(/^#?([0-9a-f]{6})([0-9a-f]{2})?$/);
    if (m) return { r: parseInt(m[1].slice(0, 2), 16), g: parseInt(m[1].slice(2, 4), 16), b: parseInt(m[1].slice(4, 6), 16) };
    m = s.match(/^rgba?\(\s*(\d+)[\s,]+(\d+)[\s,]+(\d+)/);
    if (m) return { r: clamp(+m[1]), g: clamp(+m[2]), b: clamp(+m[3]) };
    return null;
  }
  const hex = (c) => { const p = parse(c); return p ? `#${[p.r, p.g, p.b].map((x) => x.toString(16).padStart(2, '0')).join('')}` : null; };
  function mix(a, b, t = 0.5) {
    const x = parse(a); const y = parse(b);
    if (!x || !y) return hex(x || y);
    return hex({ r: x.r + (y.r - x.r) * t, g: x.g + (y.g - x.g) * t, b: x.b + (y.b - x.b) * t });
  }
  const lighten = (c, amount = 0.2) => mix(c, '#ffffff', amount);
  const darken = (c, amount = 0.2) => mix(c, '#000000', amount);
  function luminance(c) {
    const p = parse(c);
    if (!p) return 0;
    const ch = (v) => { const s = v / 255; return s <= 0.03928 ? s / 12.92 : ((s + 0.055) / 1.055) ** 2.4; };
    return 0.2126 * ch(p.r) + 0.7152 * ch(p.g) + 0.0722 * ch(p.b);
  }
  function contrast(a, b) {
    const x = luminance(a); const y = luminance(b);
    return Math.round(((Math.max(x, y) + 0.05) / (Math.min(x, y) + 0.05)) * 100) / 100;
  }
  const readable = (bg) => (contrast(bg, '#111111') >= contrast(bg, '#ffffff') ? '#111111' : '#ffffff');
  function fromHsl(h, s, l) {
    const k = (n) => (n + h / 30) % 12;
    const a = s * Math.min(l, 1 - l);
    const f = (n) => l - a * Math.max(-1, Math.min(k(n) - 3, 9 - k(n), 1));
    return hex({ r: f(0) * 255, g: f(8) * 255, b: f(4) * 255 });
  }
  function forName(text) {
    let x = 2166136261;
    for (const ch of String(text || '')) { x ^= ch.codePointAt(0); x = Math.imul(x, 16777619) >>> 0; }
    return fromHsl(x % 360, 0.55, 0.48);
  }
  function palette(n, { saturation = 0.62, lightness = 0.52, start = 210 } = {}) {
    const out = [];
    for (let i = 0; i < n; i++) out.push(fromHsl((start + i * 137.508) % 360, saturation, lightness));
    return out;
  }
  function scale(value, min, max, from = '#dbeafe', to = '#1d4ed8') {
    const t = max > min ? (Number(value) - min) / (max - min) : 0;
    return mix(from, to, Math.max(0, Math.min(1, t)));
  }
  return { parse, hex, mix, lighten, darken, contrast, readable, forName, palette, scale };
}
