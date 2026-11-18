// The widgets mod's state: what each widget tool answered (by the call's
// tool_use_id), the widgets pinned to the live pane and their latest data.

export type Kind = 'stock_quote' | 'flight_track' | 'shop_search' | 'cart_add' | 'cart_view'

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

export type Data = Stock | Flight | Shop

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
    }
  }
}
