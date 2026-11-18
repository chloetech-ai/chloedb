// Booking a table: restaurants near a place with the times they have free,
// and a booking. A stand-in for a reservations API (OpenTable, Resy,
// SevenRooms): the restaurants and their free tables are made up, the same
// for the same request, and a booking books nothing. Every answer says so.
import { z } from 'zod'

import { hash, random, round } from './common.mjs'

const NAMES = {
  italian: ['Trattoria Lucca', 'Osteria del Ponte', 'Pasta Fresca', 'Nonna Rosa', 'Vicolo'],
  japanese: ['Kaito', 'Sushi Hama', 'Izakaya Moku', 'Ramen Kuro', 'Tsubaki'],
  thai: ['Baan Suan', 'Krua Thai', 'Lemongrass House', 'Siam Garden', 'Jasmine'],
  mexican: ['Casa Verde', 'La Milpa', 'Taqueria Sol', 'Comal', 'Agave'],
  french: ['Le Petit Zinc', 'Bistro Lune', 'Maison Claire', 'Chez Mireille', 'La Fourchette'],
  indian: ['Saffron', 'Tandoor House', 'Masala Lane', 'Chaat Corner', 'Kesar'],
  american: ['The Gilded Fork', 'Oak & Ember', 'Harbor Grill', 'Corner Diner', 'Smoke Yard'],
  seafood: ['Pier 9 Oyster Bar', 'Salt & Brine', 'The Tide', 'Anchor', 'Mussel Beach'],
}
const NOTES = ['Window tables', 'Patio seating', 'Chef’s counter', 'Quiet, good for talking', 'Lively bar', 'Tasting menu', 'Family run', 'Natural wine list']

const pad = n => String(n).padStart(2, '0')

// "tomorrow", "friday", "2026-11-21" or nothing (today) as YYYY-MM-DD.
function dayOf(text, now = new Date()) {
  const t = String(text ?? '').trim().toLowerCase()
  const d = new Date(now)
  if (/^\d{4}-\d{2}-\d{2}$/.test(t)) return t
  if (t === 'tomorrow') d.setDate(d.getDate() + 1)
  const days = ['sunday', 'monday', 'tuesday', 'wednesday', 'thursday', 'friday', 'saturday']
  const i = days.findIndex(name => t.startsWith(name.slice(0, 3)))
  if (i >= 0) d.setDate(d.getDate() + ((i - d.getDay() + 7) % 7 || 7))
  return `${d.getFullYear()}-${pad(d.getMonth() + 1)}-${pad(d.getDate())}`
}

// "8pm", "20:00", "7:30 pm" as HH:MM, 19:30 when it says nothing.
function timeOf(text) {
  const m = /^(\d{1,2})(?::(\d{2}))?\s*(am|pm)?$/i.exec(String(text ?? '').trim())
  if (!m) return '19:30'
  let h = Number(m[1])
  if (m[3]?.toLowerCase() === 'pm' && h < 12) h += 12
  if (!m[3] && h < 11) h += 12
  return `${pad(Math.min(23, h))}:${m[2] ?? '00'}`
}

const minutes = hm => Number(hm.slice(0, 2)) * 60 + Number(hm.slice(3))
const clock = m => `${pad(Math.floor(m / 60))}:${pad(m % 60)}`

function restaurants({ where, date, time, party, cuisine }) {
  const kinds = cuisine && NAMES[cuisine.toLowerCase()] ? [cuisine.toLowerCase()] : Object.keys(NAMES)
  const rand = random(hash(`${where}|${date}|${kinds.join(',')}`))
  const wanted = minutes(time)
  const list = []
  for (let i = 0; i < 6; i++) {
    const kind = kinds[Math.floor(rand() * kinds.length)]
    const names = NAMES[kind]
    let name = names[Math.floor(rand() * names.length)]
    if (list.some(r => r.name === name)) name = names.find(n => !list.some(r => r.name === n))
    if (!name) continue // one cuisine has five places
    const slots = []
    for (let step = -4; step <= 4; step++) {
      const at = wanted + step * 15
      if (at < 17 * 60 || at > 22 * 60 + 30) continue
      if (rand() < 0.45 + party * 0.04) continue
      slots.push(clock(at))
    }
    list.push({
      id: `${hash(`${where}|${name}`) % 100000}`,
      name,
      cuisine: kind[0].toUpperCase() + kind.slice(1),
      area: where,
      rating: round(3.9 + rand() * 1.0, 1),
      reviews: 80 + Math.floor(rand() * 2400),
      price: '$'.repeat(2 + Math.floor(rand() * 3)),
      walkMin: 3 + Math.floor(rand() * 25),
      note: NOTES[Math.floor(rand() * NOTES.length)],
      slots,
    })
  }
  return list.sort((a, b) => b.slots.length - a.slots.length || b.rating - a.rating)
}

const Area = z.string().min(1).max(60).describe('Where: a city or neighbourhood, "North Beach, San Francisco"')

export const tableSearch = {
  name: 'table_search',
  title: 'Find a table',
  view: null,
  domains: [],
  description:
    'Restaurants near a place with the times they have a free table for the party, around the time asked. ' +
    'Demo data: the restaurants and tables are made up.',
  input: {
    where: Area,
    date: z.string().max(20).optional().describe('YYYY-MM-DD, "tomorrow" or a weekday; today when left out'),
    time: z.string().max(10).optional().describe('"8pm" or "20:00"; 19:30 when left out'),
    party: z.number().int().min(1).max(12).optional().describe('How many people; 2 when left out'),
    cuisine: z.string().max(20).optional().describe('italian, japanese, thai, mexican, french, indian, american or seafood'),
  },
  example: { where: 'North Beach, San Francisco', date: 'friday', time: '8pm', party: 2 },
  async run({ where, date, time, party = 2, cuisine }) {
    const day = dayOf(date)
    const at = timeOf(time)
    return {
      widget: 'booking',
      mode: 'search',
      where,
      date: day,
      time: at,
      party,
      cuisine: cuisine ?? null,
      restaurants: restaurants({ where, date: day, time: at, party, cuisine }),
      source: 'Demo reservations (made up)',
      note: 'Demo: these restaurants and tables are made up, and booking books nothing.',
    }
  },
  summary(d) {
    const open = d.restaurants.filter(r => r.slots.length)
    // The free times nearest the one asked for, in order.
    const near = slots => [...slots].sort((a, b) => Math.abs(minutes(a) - minutes(d.time)) - Math.abs(minutes(b) - minutes(d.time))).slice(0, 3).sort()
    const top = open.slice(0, 4).map(r => `${r.name} (${r.cuisine}, ${r.rating}★, free at ${near(r.slots).join(', ')}, id ${r.id})`).join('; ')
    return `${open.length} of ${d.restaurants.length} places near ${d.where} have a table for ${d.party} on ${d.date} around ${d.time}: ${top}. ${d.note}`
  },
}

export const tableBook = {
  name: 'table_book',
  title: 'Book a table',
  view: null,
  domains: [],
  description: 'Books a table from table_search: the restaurant id, the time and the party. Demo: nothing is booked.',
  input: {
    restaurant: z.string().min(1).max(20).describe('The restaurant id from table_search'),
    name: z.string().max(60).optional().describe('The restaurant name, to show on the confirmation'),
    date: z.string().max(20).optional().describe('YYYY-MM-DD as table_search gave it'),
    time: z.string().max(10).describe('One of the restaurant’s free times, "20:15"'),
    party: z.number().int().min(1).max(12).describe('How many people'),
    guest: z.string().max(60).optional().describe('Whose name the table is under'),
  },
  example: { restaurant: '41823', name: 'Trattoria Lucca', date: '2026-11-20', time: '20:15', party: 2 },
  async run({ restaurant, name, date, time, party, guest }) {
    const code = `DEMO-${(hash(`${restaurant}|${date}|${time}|${party}`) % 0xfffff).toString(16).toUpperCase().padStart(5, '0')}`
    return {
      widget: 'booking',
      mode: 'booked',
      confirmed: true,
      code,
      restaurant,
      name: name ?? null,
      date: dayOf(date),
      time: timeOf(time),
      party,
      guest: guest ?? null,
      source: 'Demo reservations (made up)',
      note: 'Demo: nothing was booked.',
    }
  },
  summary(d) {
    return `Booked (demo) ${d.name ?? `restaurant ${d.restaurant}`} for ${d.party} on ${d.date} at ${d.time}, code ${d.code}. ${d.note}`
  },
}
