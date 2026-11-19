// The widgets mod's state: what each widget tool answered (by the call's
// tool_use_id), the widgets pinned to the live pane and their latest data,
// and the person's saved apps for the Your apps pane.

export type Kind = 'stock_quote' | 'flight_track' | 'shop_search' | 'cart_add' | 'cart_view' | 'app_open' | 'app_make' | 'app_change' | 'show_findings'

// The app an instant-app tool (app_open, app_make, app_change) answered with.
export type Instant = {
  id: string
  name: string
  about: string
  kind: 'info' | 'action' | 'discover'
  tool: string
  tools: string[]
  version: number
  saved: boolean
  how: string
  // What it was filled with, and what the model is told to do next.
  args?: { [key: string]: unknown }
  note?: string
}

// A made app's data that no card here knows (recipes, tables): drawn as a
// list of what it holds.
export type Made = {
  widget: 'app'
  summary: string
  instant: Instant
  [field: string]: unknown
}

// A saved app, as Your apps lists it.
export type Shelved = { id: string; name: string; about: string; kind: Instant['kind']; uses?: number }

export type Stock = {
  widget: 'stock'
  summary: string
  symbol: string
  name: string
  exchange: string
  currency: string
  range: string
  price: number
  base: number
  change: number
  changePct: number
  high: number
  low: number
  volume: number | null
  asOf: number
  points: [number, number][]
  source: string
  note?: string
}

export type Flight = {
  widget: 'flight'
  summary: string
  found: boolean
  unavailable?: boolean
  query: string
  callsign: string
  airline: string | null
  icao24?: string
  registeredIn?: string
  lat?: number
  lon?: number
  over?: string
  altitudeFt?: number
  speedKts?: number
  heading?: number
  climbFpm?: number
  onGround?: boolean
  trail?: [number, number][]
  airborne?: string[]
  asOf: number
  source: string
  note?: string
}

export type Product = {
  id: number
  title: string
  brand: string | null
  price: number
  rating: number
  category: string
  discountPct: number
  stock: number | null
}

export type CartLine = { id: number; title: string; price: number; qty: number }

export type Shop = {
  widget: 'shop'
  summary: string
  mode: 'results' | 'cart'
  query?: string
  maxPrice?: number | null
  total?: number
  items: Product[]
  cart: CartLine[]
  count?: number
  source?: string
  note?: string
}

// What the Claude of the conversation found itself (a web search, pages it
// read), shaped to what the person wants and handed to an app.
export type FactValue = string | number | boolean | null

export type Found = {
  n: number
  title: string
  url: string | null
  source: string | null
  date: string | null
  kind: string | null
  summary: string
  quotes: string[]
  facts: { [name: string]: FactValue }
}

export type FoundItem = {
  n: number
  name: string
  about: string | null
  verdict: string | null
  facts: { [name: string]: FactValue }
  pros: string[]
  cons: string[]
  says: { doc: number; text: string }[]
  when: string | null
  url: string | null
}

export type Findings = {
  widget: 'findings'
  summary: string
  query: string
  goal: 'pick' | 'answer' | 'overview' | 'how-to' | 'news'
  answer: { text: string; cites: number[] } | null
  needs: { need: string; pick: string; item: number | null; why: string | null; cites: number[] }[]
  criteria: string[]
  items: FoundItem[]
  documents: Found[]
  facts: string[]
  notes: string | null
  source?: string
}

export type Data = ((Stock | Flight | Shop | Findings) & { instant?: Instant }) | Made

// One widget the model opened: which tool on which server, with what
// arguments, and what it answered.
export type Card = {
  kind: Kind
  server: string
  args: { [key: string]: string | number | boolean | null }
  data: Data
  at: number
}

export type Pin = Card & { id: string }

declare module 'claude-code' {
  interface PluginState {
    widgets: {
      cards: Shaped<{ [toolUseId: string]: Card }>
      pins: Shaped<Pin[]>
      cart: Shaped<CartLine[]>
      shelf: Shaped<{ server: string; apps: Shelved[]; at: number }>
    }
  }
}
