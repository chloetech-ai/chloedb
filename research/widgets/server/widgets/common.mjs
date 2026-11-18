// What every widget shares: fetching JSON with a deadline, the simulated
// mode the tests and screenshots use, and a seeded random source for it.

// WIDGETS_SIMULATE=1 answers every widget from made-up data, labelled
// "simulated", and never touches the network.
export const SIMULATE = /^(1|true|yes)$/i.test(process.env.WIDGETS_SIMULATE ?? '')

export async function fetchJson(url, { timeoutMs = 8000, headers = {} } = {}) {
  const response = await fetch(url, {
    signal: AbortSignal.timeout(timeoutMs),
    headers: { 'user-agent': 'Mozilla/5.0 (widgets-mcp)', accept: 'application/json', ...headers },
  })
  if (!response.ok) {
    throw new Error(`${new URL(url).host} answered ${response.status}`)
  }
  return response.json()
}

export function hash(text) {
  let h = 2166136261
  for (const ch of text) {
    h ^= ch.codePointAt(0)
    h = Math.imul(h, 16777619)
  }
  return h >>> 0
}

// mulberry32: small, fast and the same on every machine.
export function random(seed) {
  let a = seed >>> 0
  return () => {
    a = (a + 0x6d2b79f5) >>> 0
    let t = a
    t = Math.imul(t ^ (t >>> 15), t | 1)
    t ^= t + Math.imul(t ^ (t >>> 7), t | 61)
    return ((t ^ (t >>> 14)) >>> 0) / 4294967296
  }
}

export const round = (value, digits = 2) => Math.round(value * 10 ** digits) / 10 ** digits
