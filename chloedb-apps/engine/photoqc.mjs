// Whether a photo is good enough to fill the width of a page (a hero, a
// band): big enough, neither too dark nor washed out, with some contrast,
// and sharp. Cheap stats on the fetched bytes: the size from the file's
// header (JPEG, PNG, WebP) always; the light, contrast and sharpness from
// the pixels, decoded in a headless browser when the engine has one
// (playwright), at 256 px across. A photo that fails is not used full-bleed:
// the next result, then the built-in set, take its place (photos.mjs).

import { loadPlaywright } from './checks.mjs';

// The pixel size written in an image file's header, or null.
export function imageSize(bytes) {
  const b = Buffer.isBuffer(bytes) ? bytes : Buffer.from(bytes || []);
  if (b.length > 24 && b[0] === 0x89 && b.toString('latin1', 1, 4) === 'PNG') return { width: b.readUInt32BE(16), height: b.readUInt32BE(20), type: 'png' };
  if (b.length > 30 && b.toString('latin1', 0, 4) === 'RIFF' && b.toString('latin1', 8, 12) === 'WEBP') {
    const kind = b.toString('latin1', 12, 16);
    if (kind === 'VP8 ') return { width: b.readUInt16LE(26) & 0x3fff, height: b.readUInt16LE(28) & 0x3fff, type: 'webp' };
    if (kind === 'VP8L') { const n = b.readUInt32LE(21); return { width: (n & 0x3fff) + 1, height: ((n >> 14) & 0x3fff) + 1, type: 'webp' }; }
    if (kind === 'VP8X') return { width: 1 + b.readUIntLE(24, 3), height: 1 + b.readUIntLE(27, 3), type: 'webp' };
  }
  if (b.length > 4 && b[0] === 0xff && b[1] === 0xd8) {
    let i = 2;
    while (i + 9 < b.length) {
      if (b[i] !== 0xff) { i++; continue; }
      const m = b[i + 1];
      if (m === 0xd8 || m === 0x01 || (m >= 0xd0 && m <= 0xd7)) { i += 2; continue; }
      const len = b.readUInt16BE(i + 2);
      // a start of frame (not DHT C4, JPG C8, DAC CC): height, then width
      if (m >= 0xc0 && m <= 0xcf && m !== 0xc4 && m !== 0xc8 && m !== 0xcc) return { width: b.readUInt16BE(i + 7), height: b.readUInt16BE(i + 5), type: 'jpeg' };
      i += 2 + len;
    }
  }
  return null;
}

// The bars a full-width photo must clear.
// (maxSharp: a scene so full of edges that words over it cannot be read and
// the page's calm is lost; the built-in set, chosen by eye, stays under 45)
export const FULL_BLEED = { minWidth: 1200, maxAspect: 3.2, minMean: 0.12, maxMean: 0.86, minContrast: 0.08, minSharp: 5, maxSharp: 80 };

// Light (mean luminance, 0 to 1), contrast (its standard deviation) and
// sharpness (the Laplacian's mean square, x1000) of an image at 256 px
// across, or null with no browser.
let browserP = null;
let idle = null;
async function browser() {
  const pw = loadPlaywright();
  if (!pw) return null;
  if (!browserP) browserP = pw.chromium.launch().catch(() => null);
  clearTimeout(idle);
  // closed a while after its last use, so a build's photos share one
  idle = setTimeout(() => { const p = browserP; browserP = null; p?.then((b) => b?.close()).catch(() => {}); }, 20_000);
  idle.unref?.();
  return browserP;
}
export async function pixelStats(bytes, type = 'jpeg') {
  const b = await browser();
  if (!b) return null;
  const page = await b.newPage();
  try {
    return await page.evaluate(async (src) => {
      const img = new Image();
      img.src = src;
      await img.decode();
      const W = 256;
      const H = Math.max(1, Math.round((img.naturalHeight / img.naturalWidth) * W));
      const c = new OffscreenCanvas(W, H);
      const g = c.getContext('2d');
      g.drawImage(img, 0, 0, W, H);
      const d = g.getImageData(0, 0, W, H).data;
      const L = new Float32Array(W * H);
      for (let i = 0; i < W * H; i++) L[i] = (0.2126 * d[4 * i] + 0.7152 * d[4 * i + 1] + 0.0722 * d[4 * i + 2]) / 255;
      let s = 0;
      for (const v of L) s += v;
      const mean = s / L.length;
      let q = 0;
      for (const v of L) q += (v - mean) ** 2;
      let lap = 0;
      let n = 0;
      for (let y = 1; y < H - 1; y++) for (let x = 1; x < W - 1; x++) { const i = y * W + x; const v = 4 * L[i] - L[i - 1] - L[i + 1] - L[i - W] - L[i + W]; lap += v * v; n++; }
      // colour: how colourful (Hasler and Suesstrunk, 0 to about 150) and
      // the hue its colour leans to (the chroma-weighted mean, with how much
      // of the colour agrees on it, 0 to 1)
      let rgS = 0, ybS = 0, rgQ = 0, ybQ = 0, hx = 0, hy = 0, cw = 0;
      for (let i = 0; i < W * H; i++) {
        const r = d[4 * i], gg = d[4 * i + 1], b = d[4 * i + 2];
        const rg = r - gg, yb = 0.5 * (r + gg) - b;
        rgS += rg; ybS += yb; rgQ += rg * rg; ybQ += yb * yb;
        const mx = Math.max(r, gg, b), mn = Math.min(r, gg, b), c = mx - mn;
        if (c < 24) continue;
        let hdeg = mx === r ? ((gg - b) / c) % 6 : mx === gg ? (b - r) / c + 2 : (r - gg) / c + 4;
        hdeg *= 60;
        hx += c * Math.cos((hdeg * Math.PI) / 180); hy += c * Math.sin((hdeg * Math.PI) / 180); cw += c;
      }
      const N = W * H;
      const sd = (S, Q) => Math.sqrt(Math.max(0, Q / N - (S / N) ** 2));
      const colorful = Math.sqrt(sd(rgS, rgQ) ** 2 + sd(ybS, ybQ) ** 2) + 0.3 * Math.sqrt((rgS / N) ** 2 + (ybS / N) ** 2);
      const hue = cw ? ((Math.atan2(hy, hx) * 180) / Math.PI + 360) % 360 : null;
      const agree = cw ? Math.hypot(hx, hy) / cw : 0;
      return { mean, contrast: Math.sqrt(q / L.length), sharp: (1000 * lap) / Math.max(1, n), colorful, hue, agree };
    }, `data:image/${type};base64,${Buffer.from(bytes).toString('base64')}`);
  } catch { return null; } finally { await page.close().catch(() => {}); }
}

// What is wrong with a photo for a full-width place, in words, or [] when
// nothing is (or nothing could be measured).
// The design's own hue (its art direction's accent) is given as hue: a
// loud photo whose colour leans far from it would fight the page, and is
// refused for a full-width place.
export const CLASH = { colorful: 50, agree: 0.25, degrees: 70 };
export function clashes(st, hue, bars = CLASH) {
  if (!st || hue == null || st.hue == null || !(st.colorful >= bars.colorful) || !(st.agree >= bars.agree)) return false;
  const dd = Math.abs(((st.hue - hue + 540) % 360) - 180);
  return dd > bars.degrees;
}
export async function fullBleedProblems(bytes, { bars = FULL_BLEED, stats = pixelStats, hue = null } = {}) {
  const out = [];
  const size = imageSize(bytes);
  if (size) {
    if (size.width < bars.minWidth) out.push(`only ${size.width} px wide`);
    if (size.width / Math.max(1, size.height) > bars.maxAspect) out.push('a strip, far wider than tall');
  }
  if (out.length) return out;
  const st = await stats(bytes, size?.type || 'jpeg').catch(() => null);
  if (!st) return out;
  if (st.mean < bars.minMean) out.push('too dark');
  if (st.mean > bars.maxMean) out.push('washed out');
  if (st.contrast < bars.minContrast) out.push('flat');
  if (st.sharp < bars.minSharp) out.push('blurred');
  if (bars.maxSharp && st.sharp > bars.maxSharp) out.push('too busy to carry words');
  if (clashes(st, hue)) out.push("its loud colours clash with the design's");
  return out;
}
