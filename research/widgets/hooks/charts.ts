// Pictures of a widget's data: text art for the terminal (bars of block
// characters, a braille map) and SVG for the surfaces that draw it (the
// desktop app, VS Code, the phone). SVG colors are fixed, not the theme's:
// a surface draws the markup as an image.
import { WORLD } from './world'

const BLOCKS = ' ▁▂▃▄▅▆▇█'

// Resamples `values` to `width` columns, each the mean of what falls in it.
function resample(values: number[], width: number): number[] {
  if (values.length < width) {
    // Stretched: each point takes the columns that fall on it.
    return Array.from({ length: width }, (_, c) => values[Math.floor((c * values.length) / width)]!)
  }
  const out: number[] = []
  for (let c = 0; c < width; c++) {
    const from = Math.floor((c * values.length) / width)
    const to = Math.max(from + 1, Math.floor(((c + 1) * values.length) / width))
    const slice = values.slice(from, to)
    out.push(slice.reduce((a, b) => a + b, 0) / slice.length)
  }
  return out
}

// A bar chart `rows` characters tall, one column per point: the top line first.
export function bars(values: number[], width: number, rows = 3): string[] {
  const vs = resample(values, width)
  const lo = Math.min(...vs)
  const hi = Math.max(...vs)
  const span = hi - lo || 1
  const levels = vs.map(v => 1 + Math.round(((v - lo) / span) * (rows * 8 - 1)))
  const lines: string[] = []
  for (let r = rows - 1; r >= 0; r--) {
    lines.push(levels.map(l => BLOCKS[Math.max(0, Math.min(8, l - r * 8))]).join(''))
  }
  return lines
}

export const project = (lat: number, lon: number): [number, number] => [
  ((lon + 180) / 360) * WORLD.w,
  ((WORLD.top - lat) / WORLD.span) * WORLD.h,
]

type Edge = [number, number, number, number]
let edges: Edge[] | null = null

// The land's outline as edges, parsed once per load of the module.
function land(): Edge[] {
  if (edges) return edges
  edges = []
  for (const ring of WORLD.path.split('M')) {
    const pts = ring
      .replace(/Z/g, '')
      .split('L')
      .filter(Boolean)
      .map(p => p.split(',').map(Number) as [number, number])
    for (let i = 0; i < pts.length; i++) {
      const [x1, y1] = pts[i]!
      const [x2, y2] = pts[(i + 1) % pts.length]!
      if (y1 !== y2) edges.push([x1, y1, x2, y2])
    }
  }
  return edges
}

const ARROWS = ['↑', '↗', '→', '↘', '↓', '↙', '←', '↖']
const DOT = [
  [0x01, 0x08],
  [0x02, 0x10],
  [0x04, 0x20],
  [0x40, 0x80],
]

export type MapLine = { text: string; plane?: number }

// A braille map `cols` by `rows` characters around a point, land filled,
// the point drawn as an arrow along `heading`. Each line says where the
// arrow sits in it, if it does, so the drawing can color it.
export function brailleMap(lat: number, lon: number, heading: number, cols: number, rows: number): MapLine[] {
  const dotsX = cols * 2
  const dotsY = rows * 4
  const unitsPerDot = 260 / dotsX
  const [px, py] = project(lat, lon)
  const x0 = Math.min(WORLD.w - dotsX * unitsPerDot, Math.max(0, px - (dotsX * unitsPerDot) / 2))
  const y0 = Math.min(WORLD.h - dotsY * unitsPerDot, Math.max(0, py - (dotsY * unitsPerDot) / 2))
  const cells = Array.from({ length: rows }, () => new Array<number>(cols).fill(0))
  const all = land()
  for (let dy = 0; dy < dotsY; dy++) {
    const y = y0 + (dy + 0.5) * unitsPerDot
    const xs: number[] = []
    for (const [x1, y1, x2, y2] of all) {
      if (y1 > y !== y2 > y) xs.push(x1 + ((y - y1) * (x2 - x1)) / (y2 - y1))
    }
    xs.sort((a, b) => a - b)
    for (let i = 0; i + 1 < xs.length; i += 2) {
      const from = Math.max(0, Math.ceil((xs[i]! - x0) / unitsPerDot - 0.5))
      const to = Math.min(dotsX - 1, Math.floor((xs[i + 1]! - x0) / unitsPerDot - 0.5))
      for (let dx = from; dx <= to; dx++) {
        cells[dy >> 2]![dx >> 1]! |= DOT[dy & 3]![dx & 1]!
      }
    }
  }
  const pc = Math.max(0, Math.min(cols - 1, Math.floor((px - x0) / unitsPerDot / 2)))
  const pr = Math.max(0, Math.min(rows - 1, Math.floor((py - y0) / unitsPerDot / 4)))
  const arrow = ARROWS[Math.round(heading / 45) % 8]!
  return cells.map((row, r) => {
    const chars = row.map(bits => String.fromCharCode(0x2800 + bits))
    if (r !== pr) return { text: chars.join('') }
    chars[pc] = arrow
    return { text: chars.join(''), plane: pc }
  })
}

const UP = '#1a8a4a'
const DOWN = '#c8372d'

// The stock's area chart, `w` by `h` CSS pixels.
export function stockSvg(points: [number, number][], base: number | null, isUp: boolean, w = 560, h = 120): string {
  const ys = points.map(p => p[1])
  const lo = Math.min(...ys)
  const hi = Math.max(...ys)
  const pad = 4
  const x = (i: number) => pad + (i / Math.max(1, points.length - 1)) * (w - 2 * pad)
  const y = (v: number) => pad + (1 - (v - lo) / Math.max(1e-9, hi - lo)) * (h - 2 * pad)
  const line = points.map((p, i) => `${i ? 'L' : 'M'}${x(i).toFixed(1)},${y(p[1]).toFixed(1)}`).join('')
  const color = isUp ? UP : DOWN
  const baseLine =
    base != null && base >= lo && base <= hi
      ? `<line x1="0" x2="${w}" y1="${y(base).toFixed(1)}" y2="${y(base).toFixed(1)}" stroke="#9a9893" stroke-dasharray="3 4"/>`
      : ''
  return (
    `<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 ${w} ${h}" width="${w}" height="${h}">` +
    `<defs><linearGradient id="g" x1="0" x2="0" y1="0" y2="1"><stop offset="0" stop-color="${color}" stop-opacity=".25"/><stop offset="1" stop-color="${color}" stop-opacity="0"/></linearGradient></defs>` +
    baseLine +
    `<path d="${line}L${x(points.length - 1)},${h}L${x(0)},${h}Z" fill="url(#g)"/>` +
    `<path d="${line}" fill="none" stroke="${color}" stroke-width="2" stroke-linejoin="round"/>` +
    `</svg>`
  )
}

// The map around a flight, its trail and the plane along its heading.
export function flightSvg(lat: number, lon: number, heading: number, trail: [number, number][], w = 560, h = 210): string {
  const [px, py] = project(lat, lon)
  const vw = 300
  const vh = (vw * h) / w
  const vx = Math.min(WORLD.w - vw, Math.max(0, px - vw / 2))
  const vy = Math.min(WORLD.h - vh, Math.max(0, py - vh / 2))
  const points = trail.map(([a, b]) => project(a, b).map(n => n.toFixed(1)).join(',')).join(' ')
  return (
    `<svg xmlns="http://www.w3.org/2000/svg" viewBox="${vx.toFixed(1)} ${vy.toFixed(1)} ${vw} ${vh.toFixed(1)}" width="${w}" height="${h}">` +
    `<rect x="0" y="0" width="${WORLD.w}" height="${WORLD.h}" fill="#9a9893" fill-opacity=".08"/>` +
    `<path d="${WORLD.path}" fill="#9a9893" fill-opacity=".38" stroke="#9a9893" stroke-opacity=".6" stroke-width=".3"/>` +
    (trail.length > 1 ? `<polyline points="${points}" fill="none" stroke="#2f6fde" stroke-width="1" stroke-dasharray="2 2"/>` : '') +
    `<circle cx="${px.toFixed(1)}" cy="${py.toFixed(1)}" r="6" fill="#2f6fde" fill-opacity=".2"/>` +
    `<g transform="translate(${px.toFixed(1)} ${py.toFixed(1)}) rotate(${heading}) scale(.4)">` +
    `<path d="M0,-10 L2,-3 L10,2 L10,4 L2,2 L1.5,8 L4,10 L4,11 L0,10 L-4,11 L-4,10 L-1.5,8 L-2,2 L-10,4 L-10,2 L-2,-3Z" fill="#2f6fde" stroke="#ffffff" stroke-width=".8"/></g>` +
    `</svg>`
  )
}
