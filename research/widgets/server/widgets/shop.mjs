// The shopping widget: products that match a request, with prices, ratings
// and pictures, and a cart. The catalog is DummyJSON's demo store (no key),
// a stand-in for a real shop's search API; the cart lives in this process.
import { z } from 'zod'

import { SIMULATE, fetchJson, round } from './common.mjs'

const FIELDS = 'title,price,rating,brand,thumbnail,category,discountPercentage,stock,description'

// Used when the demo store is unreachable or WIDGETS_SIMULATE is set.
const OFFLINE = [
  [1, 'Trail running shoes', 'Northpeak', 89.99, 4.6, 'mens-shoes', 12],
  [2, 'Road running shoes', 'Fleet', 129.0, 4.4, 'mens-shoes', 8],
  [3, 'Waterproof hiking boots', 'Northpeak', 149.5, 4.7, 'mens-shoes', 15],
  [4, 'Wireless earbuds', 'Sonar', 59.99, 4.2, 'mobile-accessories', 20],
  [5, 'Noise cancelling headphones', 'Sonar', 199.0, 4.8, 'mobile-accessories', 10],
  [6, 'Smart watch', 'Pulse', 249.0, 4.3, 'mobile-accessories', 5],
  [7, 'Espresso machine', 'Brava', 329.0, 4.5, 'kitchen-accessories', 18],
  [8, 'Burr coffee grinder', 'Brava', 119.0, 4.6, 'kitchen-accessories', 0],
  [9, 'Laptop backpack', 'Carry', 74.0, 4.5, 'mens-bags', 10],
  [10, 'Mechanical keyboard', 'Keyline', 139.0, 4.7, 'laptops', 7],
].map(([id, title, brand, price, rating, category, discount]) => ({
  id, title, brand, price, rating, category, discountPct: discount, stock: 25, thumbnail: null, description: '',
}))

const cart = new Map()
const known = new Map()

function tidy(p) {
  const item = {
    id: p.id,
    title: p.title,
    brand: p.brand ?? null,
    price: p.price,
    rating: round(p.rating ?? 0, 1),
    category: p.category,
    discountPct: Math.round(p.discountPercentage ?? p.discountPct ?? 0),
    stock: p.stock ?? null,
    thumbnail: p.thumbnail ?? null,
    description: (p.description ?? '').slice(0, 160),
  }
  known.set(item.id, item)
  return item
}

async function search(query) {
  if (!SIMULATE) {
    try {
      const find = async q => (await fetchJson(`https://dummyjson.com/products/search?q=${encodeURIComponent(q)}&limit=40&select=${FIELDS}`)).products
      // The store matches the words as one phrase, and "laptops" finds no
      // "laptop": with none for the whole query, its words in the singular,
      // then the last word ("headphones" of "noise cancelling headphones")
      // and each of the others, each as written and in the singular.
      const one = w => (w.length > 3 && /[^s]s$/i.test(w) ? w.slice(0, -1) : w)
      const words = query.split(/\s+/).filter(Boolean)
      const tries = [query, words.map(one).join(' ')]
      if (words.length > 1) for (const w of words.filter(w => w.length > 2).reverse()) tries.push(w, one(w))
      let products = []
      for (const q of new Set(tries)) {
        products = await find(q)
        if (products.length) break
      }
      return { items: products.map(tidy), source: 'DummyJSON demo store' }
    } catch (error) {
      return { items: offline(query), source: 'offline sample catalog', note: `Store unreachable (${error.message}).` }
    }
  }
  return { items: offline(query), source: 'offline sample catalog' }
}

function offline(query) {
  const words = query.toLowerCase().split(/\s+/).filter(Boolean)
  const hits = OFFLINE.filter(p => words.some(w => `${p.title} ${p.brand} ${p.category}`.toLowerCase().includes(w.replace(/s$/, ''))))
  return (hits.length ? hits : OFFLINE).map(tidy)
}

export function cartView(note) {
  const lines = [...cart.values()]
  const total = round(lines.reduce((sum, l) => sum + l.price * l.qty, 0))
  return { widget: 'shop', mode: 'cart', items: [], cart: lines, total, count: lines.reduce((n, l) => n + l.qty, 0), note }
}

export const shopSearch = {
  name: 'shop_search',
  title: 'Shop',
  view: 'shop',
  description:
    'Opens a shopping widget in the conversation: products that match, with price, rating, picture and an Add to cart button. ' +
    'Call it whenever the person wants to buy, compare or find a product ("I need running shoes under $100", ' +
    '"good headphones for flights?"), before answering.',
  input: {
    query: z.string().min(1).max(80).describe('What to look for, in a few words: "running shoes", "headphones"'),
    maxPrice: z.number().positive().optional().describe('Highest price in USD, when the person gave one'),
    sort: z.enum(['relevance', 'price', 'rating']).optional().describe('Order of the results; relevance when left out'),
  },
  output: z.object({ widget: z.literal('shop') }).passthrough(),
  async run({ query, maxPrice, sort = 'relevance' }) {
    const { items, source, note } = await search(query)
    let shown = items.filter(i => maxPrice == null || i.price <= maxPrice)
    if (sort === 'price') shown = [...shown].sort((a, b) => a.price - b.price)
    if (sort === 'rating') shown = [...shown].sort((a, b) => b.rating - a.rating)
    return {
      widget: 'shop',
      mode: 'results',
      query,
      maxPrice: maxPrice ?? null,
      sort,
      total: shown.length,
      items: shown.slice(0, 8),
      cart: [...cart.values()],
      source,
      note,
    }
  },
  summary(d) {
    if (!d.items.length) return `No products match "${d.query}"${d.maxPrice ? ` under $${d.maxPrice}` : ''} (${d.source}).`
    const top = d.items.slice(0, 3).map(i => `${i.title} $${i.price} (${i.rating}★, id ${i.id})`).join('; ')
    return `${d.total} products match "${d.query}"${d.maxPrice ? ` under $${d.maxPrice}` : ''}. Top: ${top}. Source: ${d.source}. The widget lists them with an Add to cart button; answer in a sentence or two.`
  },
}

export const cartAdd = {
  name: 'cart_add',
  title: 'Add to cart',
  view: 'shop',
  acts: true,
  description: 'Adds a product from the shopping widget to the cart and shows the cart. Use the product id the widget or shop_search gave.',
  input: {
    id: z.number().int().positive().describe('Product id from shop_search'),
    qty: z.number().int().positive().max(20).optional().describe('How many; 1 when left out'),
  },
  output: z.object({ widget: z.literal('shop') }).passthrough(),
  async run({ id, qty = 1 }) {
    let item = known.get(id)
    if (!item && !SIMULATE) {
      try {
        item = tidy(await fetchJson(`https://dummyjson.com/products/${id}?select=${FIELDS}`))
      } catch {
        item = null
      }
    }
    if (!item) return cartView(`No product ${id}.`)
    const line = cart.get(id) ?? { id, title: item.title, price: item.price, thumbnail: item.thumbnail, qty: 0 }
    line.qty += qty
    cart.set(id, line)
    return cartView(`Added ${qty} × ${item.title}.`)
  },
  summary(d) {
    return `${d.note ?? ''} Cart: ${d.count} item(s), $${d.total}. This is a demo cart; nothing is ordered or charged.`.trim()
  },
}

export const cartShow = {
  name: 'cart_view',
  title: 'Cart',
  view: 'shop',
  description: 'Shows the shopping cart widget: what is in it and the total. Call it when the person asks what is in their cart.',
  input: {},
  output: z.object({ widget: z.literal('shop') }).passthrough(),
  async run() {
    return cartView()
  },
  summary: cartAdd.summary,
}
