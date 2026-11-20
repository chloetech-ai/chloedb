// The flight widget: where a flight is now, how high and how fast, over
// which country, with the trail of positions this server has seen. Live from
// the OpenSky Network's public state vectors (anonymous, no key).
import { readFileSync } from 'node:fs'

import { z } from 'zod'

import { SIMULATE, fetchJson, hash, random, round } from './common.mjs'

const WORLD = JSON.parse(readFileSync(new URL('../assets/world.json', import.meta.url), 'utf8'))

// IATA code -> ICAO code and name, so "UA 857" finds callsign UAL857.
const AIRLINES = {
  UA: ['UAL', 'United Airlines'], AA: ['AAL', 'American Airlines'], DL: ['DAL', 'Delta Air Lines'],
  WN: ['SWA', 'Southwest Airlines'], B6: ['JBU', 'JetBlue'], AS: ['ASA', 'Alaska Airlines'],
  AC: ['ACA', 'Air Canada'], BA: ['BAW', 'British Airways'], LH: ['DLH', 'Lufthansa'],
  AF: ['AFR', 'Air France'], KL: ['KLM', 'KLM'], IB: ['IBE', 'Iberia'], LA: ['LAN', 'LATAM Airlines'],
  AV: ['AVA', 'Avianca'], CM: ['CMP', 'Copa Airlines'], AM: ['AMX', 'Aeroméxico'], EK: ['UAE', 'Emirates'],
  QR: ['QTR', 'Qatar Airways'], SQ: ['SIA', 'Singapore Airlines'], CX: ['CPA', 'Cathay Pacific'],
  NH: ['ANA', 'All Nippon Airways'], JL: ['JAL', 'Japan Airlines'], QF: ['QFA', 'Qantas'],
  TK: ['THY', 'Turkish Airlines'], FR: ['RYR', 'Ryanair'], U2: ['EZY', 'easyJet'], LX: ['SWR', 'Swiss'],
  EY: ['ETD', 'Etihad Airways'], VS: ['VIR', 'Virgin Atlantic'], NZ: ['ANZ', 'Air New Zealand'],
  AZ: ['ITY', 'ITA Airways'], SK: ['SAS', 'SAS'], AY: ['FIN', 'Finnair'], EI: ['EIN', 'Aer Lingus'],
  TP: ['TAP', 'TAP Air Portugal'], UX: ['AEA', 'Air Europa'], VY: ['VLG', 'Vueling'], OS: ['AUA', 'Austrian'],
  SN: ['BEL', 'Brussels Airlines'], LO: ['LOT', 'LOT Polish Airlines'], A3: ['AEE', 'Aegean'], W6: ['WZZ', 'Wizz Air'],
  DY: ['NOZ', 'Norwegian'], FI: ['ICE', 'Icelandair'], AT: ['RAM', 'Royal Air Maroc'], MS: ['MSR', 'EgyptAir'],
  ET: ['ETH', 'Ethiopian Airlines'], SA: ['SAA', 'South African Airways'], KE: ['KAL', 'Korean Air'],
  OZ: ['AAR', 'Asiana Airlines'], CI: ['CAL', 'China Airlines'], BR: ['EVA', 'EVA Air'], CA: ['CCA', 'Air China'],
  MU: ['CES', 'China Eastern'], CZ: ['CSN', 'China Southern'], AI: ['AIC', 'Air India'], TG: ['THA', 'Thai Airways'],
  MH: ['MAS', 'Malaysia Airlines'], GA: ['GIA', 'Garuda Indonesia'], PR: ['PAL', 'Philippine Airlines'],
  VA: ['VOZ', 'Virgin Australia'], WS: ['WJA', 'WestJet'], F9: ['FFT', 'Frontier'], NK: ['NKS', 'Spirit'],
  HA: ['HAL', 'Hawaiian Airlines'], G3: ['GLO', 'Gol'], AD: ['AZU', 'Azul'], AR: ['ARG', 'Aerolíneas Argentinas'],
  H2: ['SKU', 'Sky Airline'], JA: ['JAT', 'JetSMART'], Y4: ['VOI', 'Volaris'], SV: ['SVA', 'Saudia'],
}
const BY_ICAO = Object.fromEntries(Object.values(AIRLINES).map(([icao, name]) => [icao, name]))

export function callsignOf(flight) {
  const text = flight.toUpperCase().replace(/[\s-]+/g, '')
  const iata = text.match(/^([A-Z0-9]{2})(\d{1,4}[A-Z]?)$/)
  if (iata && AIRLINES[iata[1]] && !/^[A-Z]{3}/.test(text)) {
    return AIRLINES[iata[1]][0] + iata[2]
  }
  return text
}

// Equirectangular, as world.json's outlines are drawn.
export const project = (lat, lon) => [((lon + 180) / 360) * WORLD.w, ((WORLD.top - lat) / WORLD.span) * WORLD.h]

function polygons(d) {
  return d
    .split('M')
    .filter(Boolean)
    .map(ring => ring.replace(/Z/g, '').split('L').map(pair => pair.split(',').map(Number)))
}
const SHAPES = Object.entries(WORLD.countries).map(([code, c]) => ({ code, name: c.n, box: c.b, rings: polygons(c.d) }))

function inside([x, y], ring) {
  let hit = false
  for (let i = 0, j = ring.length - 1; i < ring.length; j = i++) {
    const [xi, yi] = ring[i]
    const [xj, yj] = ring[j]
    if (yi > y !== yj > y && x < ((xj - xi) * (y - yi)) / (yj - yi) + xi) hit = !hit
  }
  return hit
}

export function countryAt(lat, lon) {
  const p = project(lat, lon)
  for (const shape of SHAPES) {
    const [x0, y0, x1, y1] = shape.box
    if (p[0] < x0 || p[0] > x1 || p[1] < y0 || p[1] > y1) continue
    if (shape.rings.some(ring => inside(p, ring))) return shape.name
  }
  return null
}

// One fetch of every state vector serves every widget for ten seconds.
let cache = { at: 0, states: null }
async function states() {
  if (Date.now() - cache.at < 10_000 && cache.states) return cache.states
  // OpenSky's anonymous feed fails now and then (a 503, a reset): once more before giving up.
  let body
  try {
    body = await fetchJson('https://opensky-network.org/api/states/all', { timeoutMs: 20_000 })
  } catch {
    await new Promise(resolve => setTimeout(resolve, 1000))
    body = await fetchJson('https://opensky-network.org/api/states/all', { timeoutMs: 20_000 })
  }
  cache = { at: Date.now(), states: body.states ?? [] }
  return cache.states
}

// The positions this server has seen per aircraft, so a refresh draws a trail.
const trails = new Map()
function remember(icao24, lat, lon) {
  const trail = trails.get(icao24) ?? []
  const last = trail.at(-1)
  if (!last || last[0] !== lat || last[1] !== lon) trail.push([lat, lon])
  trails.set(icao24, trail.slice(-240))
  return trails.get(icao24)
}

function fromState(s, query) {
  const [icao24, callsign, originCountry, , lastContact, lon, lat, baroAlt, onGround, velocity, track, verticalRate, , geoAlt] = s
  const cs = (callsign ?? '').trim()
  return {
    found: true,
    query,
    callsign: cs,
    icao24,
    airline: BY_ICAO[cs.slice(0, 3)] ?? null,
    registeredIn: originCountry,
    lat: round(lat, 4),
    lon: round(lon, 4),
    over: countryAt(lat, lon) ?? 'open water',
    altitudeFt: Math.round(((geoAlt ?? baroAlt ?? 0) * 3.28084) / 10) * 10,
    speedKts: Math.round((velocity ?? 0) * 1.94384),
    heading: Math.round(track ?? 0),
    climbFpm: Math.round((verticalRate ?? 0) * 196.85),
    onGround: Boolean(onGround),
    asOf: (lastContact ?? Date.now() / 1000) * 1000,
    trail: remember(icao24, round(lat, 4), round(lon, 4)),
    source: 'OpenSky Network',
  }
}

async function live(query) {
  const callsign = callsignOf(query)
  const all = await states()
  // Exactly, else with a letter the airline adds on the radar (TAP210R
  // for TP 210).
  const flying = all.filter(s => s[5] != null)
  const state =
    flying.find(s => (s[1] ?? '').trim() === callsign) ??
    flying.find(s => { const cs = (s[1] ?? '').trim(); return cs.length === callsign.length + 1 && cs.startsWith(callsign) && /[A-Z]$/.test(cs) })
  if (state) return fromState(state, query)
  const prefix = callsign.slice(0, 3)
  const airborne = all
    .filter(s => (s[1] ?? '').startsWith(prefix) && s[5] != null && !s[8])
    .map(s => s[1].trim())
    .slice(0, 8)
  return {
    found: false,
    query,
    callsign,
    airline: BY_ICAO[prefix] ?? null,
    airborne,
    asOf: Date.now(),
    source: 'OpenSky Network',
  }
}

// A made-up flight on a great-circle-ish line, moving with the clock.
function simulated(query) {
  const callsign = callsignOf(query)
  const next = random(hash(callsign))
  const routes = [
    [[37.62, -122.38], [35.55, 139.78]], [[50.03, 8.56], [40.64, -73.78]], [[-33.39, -70.79], [40.47, -3.56]],
    [[51.47, -0.45], [25.25, 55.36]], [[1.36, 103.99], [-33.94, 151.18]],
  ]
  const [[lat0, lon0], [lat1, lon1]] = routes[hash(callsign) % routes.length]
  const t = ((Date.now() / 60000) % 600) / 600
  const at = f => [lat0 + (lat1 - lat0) * f + Math.sin(f * Math.PI) * 8, lon0 + (lon1 - lon0) * f]
  const trail = Array.from({ length: 40 }, (_, i) => at((t * i) / 39)).map(([a, b]) => [round(a, 3), round(b, 3)])
  const [lat, lon] = trail.at(-1)
  const [plat, plon] = trail.at(-2)
  return {
    found: true,
    query,
    callsign,
    icao24: (hash(callsign) % 0xffffff).toString(16).padStart(6, '0'),
    airline: BY_ICAO[callsign.slice(0, 3)] ?? null,
    registeredIn: 'United States',
    lat,
    lon,
    over: countryAt(lat, lon) ?? 'open water',
    altitudeFt: 34000 + Math.round(next() * 6) * 1000,
    speedKts: 470 + Math.round(next() * 60),
    heading: Math.round(((Math.atan2(lon - plon, lat - plat) * 180) / Math.PI + 360) % 360),
    climbFpm: 0,
    onGround: false,
    asOf: Date.now(),
    trail,
    source: 'simulated',
  }
}

// A second radar, adsb.lol (anonymous, no key, open data), asked when
// OpenSky fails or doesn't see the flight (round 9 TOM-1).
export async function second(query) {
  const callsign = callsignOf(query)
  const body = await fetchJson(`https://api.adsb.lol/v2/callsign/${encodeURIComponent(callsign)}`, { timeoutMs: 12_000 })
  const a = (body.ac ?? []).find(a => a.lat != null && a.lon != null)
  if (!a) return { found: false, query, callsign, airline: BY_ICAO[callsign.slice(0, 3)] ?? null, airborne: [], asOf: body.now ?? Date.now(), source: 'adsb.lol' }
  const lat = round(a.lat, 4)
  const lon = round(a.lon, 4)
  const ground = a.alt_baro === 'ground'
  const feet = Number.isFinite(a.alt_geom) ? a.alt_geom : Number.isFinite(a.alt_baro) ? a.alt_baro : 0
  return {
    found: true,
    query,
    callsign: (a.flight ?? callsign).trim(),
    icao24: a.hex,
    airline: BY_ICAO[callsign.slice(0, 3)] ?? null,
    registeredIn: null,
    registration: a.r ?? null,
    lat,
    lon,
    over: countryAt(lat, lon) ?? 'open water',
    altitudeFt: Math.round(feet / 10) * 10,
    speedKts: Math.round(a.gs ?? 0),
    heading: Math.round(a.track ?? a.true_heading ?? 0),
    climbFpm: Math.round(a.geom_rate ?? a.baro_rate ?? 0),
    onGround: ground,
    asOf: (body.now ?? Date.now()) - (a.seen_pos ?? 0) * 1000,
    trail: remember(a.hex, lat, lon),
    source: 'adsb.lol',
  }
}

export default {
  name: 'flight_track',
  title: 'Flight tracker',
  view: 'flight',
  description:
    'Opens a live flight tracker widget in the conversation: where a flight is now on a map, ' +
    'its altitude, speed and heading, and the country below it. Call it whenever the person asks ' +
    'where a flight is, whether it has taken off, or how it is going ("where is UA 857?", "is LH400 in the air?").',
  input: {
    flight: z.string().min(2).max(10).describe('Flight number or callsign: "UA 857", "LH400", "BAW117"'),
  },
  output: z.object({ widget: z.literal('flight') }).passthrough(),
  async run({ flight }) {
    let data
    if (!SIMULATE) {
      let failed = null
      try {
        data = await live(flight)
      } catch (error) {
        failed = error
      }
      // OpenSky down, or not seeing it: the second radar (TOM-1).
      if (!data?.found) {
        try {
          const other = await second(flight)
          if (other.found || failed) data = other.found ? other : { ...other, airborne: data?.airborne ?? [], source: 'OpenSky Network and adsb.lol' }
        } catch (error) {
          if (failed) data = { found: false, unavailable: true, query: flight, callsign: callsignOf(flight), airborne: [], asOf: Date.now(), source: 'OpenSky Network and adsb.lol', note: 'Both flight radars we ask (OpenSky and adsb.lol) are busy or down right now, so nothing can say where it is.' }
        }
      }
    }
    data ??= simulated(flight)
    return { widget: 'flight', ...data }
  },
  summary(d) {
    if (d.unavailable) return `Live flight positions are unavailable right now: ${d.note} Say so; do not guess where ${d.callsign} is.`
    if (!d.found) {
      const others = d.airborne?.length ? ` In the air now for that airline: ${d.airborne.join(', ')}.` : ''
      return `${d.callsign} is not broadcasting a position right now (${d.source}).${d.note ? ' ' + d.note : ''}${others}`
    }
    return (
      `${d.callsign}${d.airline ? ` (${d.airline})` : ''} is ${d.onGround ? 'on the ground' : 'airborne'} over ${d.over} ` +
      `at ${d.lat}, ${d.lon}: ${d.altitudeFt} ft, ${d.speedKts} kt, heading ${d.heading}°. Source: ${d.source}. ` +
      'The widget shows the map; answer in a sentence or two.'
    )
  },
}
