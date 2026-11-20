// Today's exchange rate between two currencies, for an app that converts
// money (a currency converter, a trip budget in another currency): live
// from Yahoo Finance's chart endpoint (no key), simulated when that is
// unreachable or WIDGETS_SIMULATE is set, and labelled so. Without it an
// app could only bake in a rate the model remembered.
import { z } from 'zod'

import { SIMULATE, fetchJson } from './common.mjs'

// Rough rates for a demo or when Yahoo is unreachable: never shown as live.
const USD_PER = { USD: 1, EUR: 1.1, GBP: 1.3, JPY: 0.0066, CAD: 0.72, AUD: 0.65, CHF: 1.2, MXN: 0.055, BRL: 0.18, CLP: 0.00105, INR: 0.012, CNY: 0.14 }

const CODE = z.string().regex(/^[A-Za-z]{3}$/, 'a three-letter currency code')

async function live(from, to) {
  const symbol = `${from}${to}=X`
  const body = await fetchJson(`https://query1.finance.yahoo.com/v8/finance/chart/${encodeURIComponent(symbol)}?range=5d&interval=1d`)
  const meta = body?.chart?.result?.[0]?.meta
  const rate = meta?.regularMarketPrice
  if (typeof rate !== 'number' || !(rate > 0)) throw new Error(body?.chart?.error?.description ?? `no rate for ${from} to ${to}`)
  return { rate, asOf: (meta.regularMarketTime ?? Date.now() / 1000) * 1000, source: 'Yahoo Finance' }
}

function simulated(from, to) {
  const a = USD_PER[from]
  const b = USD_PER[to]
  if (!a || !b) throw new Error(`no simulated rate for ${from} to ${to}`)
  return { rate: a / b, asOf: Date.now(), source: 'simulated' }
}

export const exchangeRate = {
  name: 'exchange_rate',
  title: 'Exchange rate',
  view: null,
  domains: [],
  description:
    "Today's exchange rate between two currencies (1 EUR = 1.09 USD), with its time and source. An app that converts money (a currency " +
    'converter, a trip or shopping budget in another currency) calls it when it opens and when the person picks other currencies, and ' +
    'shows the rate, its time and its source; never a rate written into the app.',
  input: {
    from: CODE.describe('The currency converted from: EUR'),
    to: CODE.describe('The currency converted to: USD'),
    amount: z.number().optional().describe('An amount to convert, if any'),
  },
  example: { from: 'EUR', to: 'USD', amount: 80 },
  async run({ from, to, amount }) {
    from = String(from).toUpperCase()
    to = String(to).toUpperCase()
    let got
    if (from === to) got = { rate: 1, asOf: Date.now(), source: 'same currency' }
    else if (!SIMULATE) {
      try {
        got = await live(from, to)
      } catch (error) {
        got = { ...simulated(from, to), note: `Live rate unavailable (${error.message}); simulated.` }
      }
    }
    got ??= simulated(from, to)
    const out = { widget: 'rate', from, to, ...got, rate: Math.round(got.rate * 1e6) / 1e6 }
    if (typeof amount === 'number') out.converted = Math.round(amount * out.rate * 100) / 100
    return out
  },
  summary(d) {
    const when = new Date(d.asOf).toISOString().slice(0, 16).replace('T', ' ')
    const sum = d.converted != null ? ` (so ${d.converted} ${d.to})` : ''
    return `1 ${d.from} = ${d.rate} ${d.to}${sum}. Source: ${d.source}, as of ${when} UTC.${d.note ? ` ${d.note}` : ''}`
  },
}
