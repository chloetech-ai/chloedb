// The stock widget: a ticker's price, its change and a chart of the range.
// Live from Yahoo Finance's chart endpoint (no key); simulated when that
// is unreachable or WIDGETS_SIMULATE is set, and labelled so.
import { z } from 'zod'

import { SIMULATE, fetchJson, hash, random, round } from './common.mjs'

const RANGES = ['1d', '5d', '1mo', '6mo', '1y', '5y']
// Daily for a year, so its range is the 52-week range people know (from
// each day's high and low, not from weekly closes).
const INTERVAL = { '1d': '5m', '5d': '30m', '1mo': '1d', '6mo': '1d', '1y': '1d', '5y': '1wk' }
const SPAN_MS = { '1d': 6.5 * 3600e3, '5d': 5 * 86400e3, '1mo': 30 * 86400e3, '6mo': 182 * 86400e3, '1y': 365 * 86400e3, '5y': 5 * 365 * 86400e3 }
const POINTS = { '1d': 78, '5d': 65, '1mo': 22, '6mo': 126, '1y': 52, '5y': 60 }

const KNOWN = {
  AAPL: ['Apple Inc.', 228], NVDA: ['NVIDIA Corporation', 182], MSFT: ['Microsoft Corporation', 512],
  TSLA: ['Tesla, Inc.', 345], AMZN: ['Amazon.com, Inc.', 221], GOOGL: ['Alphabet Inc.', 284],
  META: ['Meta Platforms, Inc.', 702], NFLX: ['Netflix, Inc.', 1180], AMD: ['Advanced Micro Devices, Inc.', 238],
  SPY: ['SPDR S&P 500 ETF Trust', 662], QQQ: ['Invesco QQQ Trust', 598], 'BTC-USD': ['Bitcoin USD', 91200],
}

const Output = z.object({ widget: z.literal('stock') }).passthrough()

async function live(symbol, range) {
  const url = `https://query1.finance.yahoo.com/v8/finance/chart/${encodeURIComponent(symbol)}?range=${range}&interval=${INTERVAL[range]}&includePrePost=false`
  const body = await fetchJson(url)
  const result = body?.chart?.result?.[0]
  if (!result) {
    throw new Error(body?.chart?.error?.description ?? `no chart for ${symbol}`)
  }
  const { meta } = result
  const closes = result.indicators?.quote?.[0]?.close ?? []
  const points = (result.timestamp ?? [])
    .map((t, i) => [t * 1000, closes[i]])
    .filter(([, close]) => typeof close === 'number')
    .map(([t, close]) => [t, round(close)])
  // The range's high and low are the highest and lowest trades in it,
  // as quote pages give them, not the highest and lowest close.
  const quote = result.indicators?.quote?.[0] ?? {}
  const numbers = list => (list ?? []).filter(v => typeof v === 'number')
  const highs = numbers(quote.high)
  const lows = numbers(quote.low)
  const price = meta.regularMarketPrice ?? points.at(-1)?.[1]
  const base = range === '1d' ? meta.chartPreviousClose ?? meta.previousClose ?? points[0]?.[1] : points[0]?.[1]
  return {
    symbol: meta.symbol ?? symbol,
    name: meta.longName ?? meta.shortName ?? KNOWN[symbol]?.[0] ?? symbol,
    exchange: meta.fullExchangeName ?? meta.exchangeName ?? '',
    currency: meta.currency ?? 'USD',
    price: round(price),
    base: round(base),
    // The day's range for a day, else the range's own.
    high: round((range === '1d' && meta.regularMarketDayHigh) || Math.max(...(highs.length ? highs : points.map(p => p[1])))),
    low: round((range === '1d' && meta.regularMarketDayLow) || Math.min(...(lows.length ? lows : points.map(p => p[1])))),
    volume: meta.regularMarketVolume ?? null,
    asOf: (meta.regularMarketTime ?? Date.now() / 1000) * 1000,
    points,
    source: 'Yahoo Finance',
  }
}

function simulated(symbol, range) {
  const [name, start] = KNOWN[symbol] ?? [symbol, 20 + (hash(symbol) % 580)]
  const day = Math.floor(Date.now() / 86400e3)
  const next = random(hash(`${symbol}:${range}:${day}`))
  const n = POINTS[range]
  const end = Date.now()
  const step = SPAN_MS[range] / (n - 1)
  const drift = (next() - 0.45) * 0.004
  const vol = range === '1d' ? 0.0025 : range === '5d' ? 0.006 : 0.015
  let price = start
  const points = []
  for (let i = 0; i < n; i++) {
    price *= 1 + drift + (next() - 0.5) * 2 * vol
    points.push([Math.round(end - (n - 1 - i) * step), round(price)])
  }
  const closes = points.map(p => p[1])
  return {
    symbol,
    name,
    exchange: 'NASDAQ',
    currency: 'USD',
    price: closes.at(-1),
    base: range === '1d' ? round(start) : closes[0],
    high: round(Math.max(...closes)),
    low: round(Math.min(...closes)),
    volume: Math.round(2e7 + next() * 8e7),
    asOf: end,
    points,
    source: 'simulated',
  }
}

export default {
  name: 'stock_quote',
  title: 'Stock quote',
  view: 'stock',
  description:
    'Opens a live stock widget in the conversation: price, change and a chart. ' +
    'Call it whenever the person asks how a stock, ETF, index or crypto pair is doing, ' +
    'its price, or its chart, even in passing ("how is NVDA today?", "AAPL over the last month"). ' +
    'Call it once per symbol, before answering.',
  input: {
    symbol: z.string().min(1).max(15).describe('Ticker as the exchange lists it: NVDA, AAPL, BRK-B, ^GSPC, BTC-USD'),
    range: z.enum(RANGES).optional().describe('How far back the chart goes; 1d when left out'),
  },
  output: Output,
  async run({ symbol, range = '1d' }) {
    const ticker = symbol.trim().toUpperCase()
    // One ticker a call: "SBUX,GIS,MDLZ" is three, never a stock of its own.
    const several = ticker.split(/[\s,;/&+]+/).filter(Boolean)
    if (several.length > 1) throw new Error(`One ticker a call: ask for ${several.join(', ')} one at a time`)
    if (!/^[A-Z0-9^.=-]{1,15}$/.test(ticker)) throw new Error(`"${symbol}" is not a ticker as an exchange lists it (NVDA, BRK-B, ^GSPC)`)
    let data
    if (!SIMULATE) {
      try {
        data = await live(ticker, range)
      } catch (error) {
        // A ticker that isn't listed is said so; made-up prices are only for a quote service that can't be reached.
        if (/answered 404|No data found|delisted/i.test(error.message)) throw new Error(`No stock, fund or index is listed as ${ticker}`)
        data = { ...simulated(ticker, range), note: `Live quote unavailable (${error.message}); simulated.` }
      }
    }
    data ??= simulated(ticker, range)
    const change = round(data.price - data.base)
    const changePct = data.base ? round((change / data.base) * 100) : 0
    return { widget: 'stock', range, change, changePct, ...data }
  },
  summary(d) {
    const sign = d.change >= 0 ? '+' : ''
    const when = new Date(d.asOf).toISOString().slice(0, 16).replace('T', ' ')
    return (
      `${d.name} (${d.symbol}) ${d.price} ${d.currency}, ${sign}${d.change} (${sign}${d.changePct}%) over ${d.range}; ` +
      `range low ${d.low}, high ${d.high}. Source: ${d.source}, as of ${when} UTC. ` +
      'The widget shows the chart; answer in a sentence or two without repeating every number.'
    )
  },
}
