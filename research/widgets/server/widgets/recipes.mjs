// Recipes to browse: dishes with photos by name, cuisine, kind or main
// ingredient, a page at a time and not only the first ones from A to Z, with
// the amounts each one needs and how long it takes about (an estimate:
// TheMealDB gives no times), and one dish in full. From TheMealDB's free API
// (its test key, "1"); a small sample without photos when it is unreachable
// or WIDGETS_SIMULATE is set, labelled so.
import { z } from 'zod'

import { SIMULATE, fetchJson } from './common.mjs'

const API = 'https://www.themealdb.com/api/json/v1/1'
const cache = new Map()

async function get(path) {
  if (cache.has(path)) return cache.get(path)
  const body = await fetchJson(`${API}/${path}`)
  cache.set(path, body)
  return body
}

function ingredients(meal) {
  const out = []
  for (let i = 1; i <= 20; i++) {
    const name = meal[`strIngredient${i}`]?.trim()
    if (name) out.push({ name, amount: meal[`strMeasure${i}`]?.trim() ?? '' })
  }
  return out
}

function card(meal) {
  return {
    id: meal.idMeal,
    title: meal.strMeal,
    cuisine: meal.strArea ?? null,
    kind: meal.strCategory ?? null,
    tags: (meal.strTags ?? '').split(',').map(t => t.trim()).filter(Boolean),
    image: meal.strMealThumb ? `${meal.strMealThumb}/medium` : null,
    ingredients: ingredients(meal).map(i => i.name),
    amounts: ingredients(meal),
    ...estimate(meal),
  }
}

// A stock, broth or bouillon of an ingredient doesn't make it a main one:
// a dish made with chicken stock is not a chicken dish, nor vegetarian.
const ONLY_FLAVOUR = /\b(stock|broth|bouillon|cube|gravy|powder|fat)\b/i
const MEAT = /\b(chicken|beef|pork|lamb|mutton|veal|bacon|ham|pancetta|chorizo|sausages?|turkey|duck|goose|fish|anchov(y|ies)|prawns?|shrimps?|salmon|tuna|cod|crab|lobster|mussels|clams|squid|gelatine?|lard|suet)\b/i
// "ground beef" is "minced beef" too: each word of an ingredient, with the
// other names it goes by.
const SAME = { ground: '(ground|minced?)', minced: '(ground|minced?)', mince: '(ground|minced?)', prawns: 'prawns?|shrimps?', shrimp: 'prawns?|shrimps?', courgette: 'courgettes?|zucchinis?', zucchini: 'courgettes?|zucchinis?', aubergine: 'aubergines?|eggplants?', eggplant: 'aubergines?|eggplants?' }
const named = word => {
  const words = String(word).toLowerCase().trim().split(/\s+/).filter(Boolean)
  const tests = words.map(w => new RegExp(`\\b(${SAME[w] ?? w.replace(/[^a-z]/g, '').replace(/e?s$/, '')}(e?s)?)\\b`, 'i'))
  return name => tests.every(t => t.test(name))
}
function has(meal, word) {
  const is = named(word)
  return ingredients(meal).some(i => is(i.name) && !ONLY_FLAVOUR.test(i.name))
}
// Every dish with an ingredient the word names ("chicken": chicken, its
// breasts, thighs, wings...), and those of its kind when it is one.
const KINDS = ['Beef', 'Breakfast', 'Chicken', 'Dessert', 'Goat', 'Lamb', 'Miscellaneous', 'Pasta', 'Pork', 'Seafood', 'Side', 'Starter', 'Vegan', 'Vegetarian']
const OTHER_FOOD = /\b(tomato|garlic|onion|pepper|lettuce)/i
async function withIngredient(word) {
  const is = named(word)
  const names = ((await get('list.php?i=list')).meals ?? []).map(m => m.strIngredient).filter(n => n && is(n) && !ONLY_FLAVOUR.test(n) && !(OTHER_FOOD.test(n) && !OTHER_FOOD.test(word)))
  const kind = KINDS.find(k => k.toLowerCase() === String(word).toLowerCase().trim())
  const lists = await Promise.all([
    ...names.slice(0, 12).map(n => get(`filter.php?i=${encodeURIComponent(n.replace(/\s+/g, '_'))}`).then(b => b.meals ?? []).catch(() => [])),
    kind ? get(`filter.php?c=${kind}`).then(b => b.meals ?? []).catch(() => []) : [],
  ])
  return [...new Set(lists.flat().map(m => m.idMeal))]
}
function matches(meal, { query, cuisine, kind, ingredient }) {
  const same = (a, b) => String(a ?? '').toLowerCase() === String(b).toLowerCase().trim()
  if (ingredient && !has(meal, ingredient)) return false
  if (query && !String(meal.strMeal).toLowerCase().includes(query.toLowerCase().trim()) && !has(meal, query)) return false
  if (cuisine && !same(meal.strArea, cuisine)) return false
  if (kind && !same(meal.strCategory, kind)) return false
  // TheMealDB files some dishes made with chicken stock as vegetarian.
  if (kind && /^(vegetarian|vegan)$/i.test(kind.trim()) && ingredients(meal).some(i => MEAT.test(i.name))) return false
  return true
}

// TheMealDB gives no times, so a dish's time is always an estimate, and says
// so: from its name ("15-minute burgers"), else the minutes and hours its
// steps name, added up (the middle of a range; "on each side" twice), with 10
// minutes for each step that cooks without saying how long and a few to get
// the ingredients ready, and the waiting in it (marinating, resting,
// overnight) said apart; else a rough guess from its steps that cook.
const WORDS = { a: 1, an: 1, one: 1, two: 2, three: 3, four: 4, five: 5, six: 6, seven: 7, eight: 8, nine: 9, ten: 10, twelve: 12, fifteen: 15, twenty: 20, thirty: 30, forty: 40, 'half an': 0.5, 'half a': 0.5 }
const AMOUNT = '(\\d+(?:[.,]\\d+)?|half an?|an?|one|two|three|four|five|six|seven|eight|nine|ten|twelve|fifteen|twenty|thirty|forty)'
const DURATION = new RegExp(`(\\bevery\\s+|\\b(?:to|or|up to)\\s+)?${AMOUNT}(?:\\s*(?:-|–|to|or)\\s*${AMOUNT})?\\s*(?:and a half\\s*)?(hours?|hrs?|minutes?|mins?)\\b(\\s*(?:on|per)\\s*(?:each|every)?\\s*side)?|\\b(overnight)\\b`, 'gi')
const COOKS = /\b(boil|simmer|bake|roast|fry|saut[eé]|grill|broil|cook|oven|stew|braise|caramel[iy]ze|toast|steam|poach|reduce)/i
const WAITS = /\b(marinat|rest|chill|refrigerat|fridge|freez|soak|prove|proof|rise|dry out|cool|set aside|leave)/i
const amountOf = text => WORDS[text.toLowerCase()] ?? Number(text.replace(',', '.'))
const hours = m => (m >= 120 ? `${Math.round(m / 60)} h` : `${m} min`)
export function estimate(meal) {
  const named = /\b(\d{1,3})[\s-]*min(?:ute)?s?\b/i.exec(meal.strMeal ?? '')
  if (named) return { minutes: Number(named[1]), minutes_note: 'from its name' }
  const steps = (meal.strInstructions ?? '').split(/\r?\n+|(?<=\.)\s+(?=[A-Z])/).map(s => s.trim()).filter(s => s.length > 12)
  let cooking = 0
  let waiting = 0
  let untimed = 0
  for (const step of steps) {
    let found = false
    for (const m of step.matchAll(DURATION)) {
      // "every 5 minutes", and the far end of "30 minutes to one hour".
      if (m[1]) continue
      let took
      if (m[6]) took = 8 * 60
      else {
        const low = amountOf(m[2])
        const high = m[3] ? amountOf(m[3]) : low
        if (!Number.isFinite(low) || !Number.isFinite(high)) continue
        const half = /and a half/i.test(m[0]) ? 0.5 : 0
        took = ((low + high) / 2 + half) * (/^h/i.test(m[4]) ? 60 : 1) * (m[5] ? 2 : 1)
      }
      if (WAITS.test(step) && !COOKS.test(step.slice(Math.max(0, m.index - 30), m.index))) waiting += took
      else cooking += took
      found = true
    }
    if (!found && COOKS.test(step)) untimed++
  }
  const ready = 5 + ingredients(meal).length
  const round = n => Math.max(10, Math.round(n / 5) * 5)
  if (cooking || waiting) {
    const wait = round(Math.min(waiting, 24 * 60))
    return { minutes: round(Math.min(cooking, 12 * 60) + untimed * 10 + ready + (waiting ? wait : 0)), minutes_note: waiting ? `estimated from its steps, ${hours(wait)} of it waiting` : 'estimated from its steps' }
  }
  return { minutes: round(Math.max(1, untimed) * 10 + ready), minutes_note: 'a rough guess: the recipe gives no times' }
}

const OFFLINE = [
  ['52772', 'Teriyaki Chicken Casserole', 'Japanese', 'Chicken', ['soy sauce', 'chicken breasts', 'rice', 'stir-fry vegetables']],
  ['52959', 'Baked salmon with fennel & tomatoes', 'British', 'Seafood', ['fennel', 'salmon', 'tomatoes', 'olive oil']],
  ['52814', 'Thai Green Curry', 'Thai', 'Chicken', ['chicken', 'green curry paste', 'coconut milk', 'basil']],
  ['52771', 'Spicy Arrabiata Penne', 'Italian', 'Vegetarian', ['penne', 'olive oil', 'garlic', 'chopped tomatoes']],
  ['52855', 'Banana Pancakes', 'American', 'Dessert', ['banana', 'eggs', 'baking powder', 'vanilla']],
  ['52977', 'Corba', 'Turkish', 'Side', ['lentils', 'onion', 'carrots', 'tomato puree']],
].map(([id, title, cuisine, kind, items]) => ({ id, title, cuisine, kind, tags: [], image: null, ingredients: items, amounts: items.map(name => ({ name, amount: '' })), minutes: 30, minutes_note: 'a rough guess: the recipe gives no times' }))

function offline(words) {
  const w = words.toLowerCase()
  const hits = OFFLINE.filter(r => !w || `${r.title} ${r.cuisine} ${r.kind} ${r.ingredients.join(' ')}`.toLowerCase().includes(w))
  return hits.length ? hits : OFFLINE
}

// A list in an order of its own for each search, the same every time, so
// a page is not only the dishes from A to C.
function spread(ids, key) {
  const hash = text => {
    let h = 2166136261
    for (const c of text) h = Math.imul(h ^ c.charCodeAt(0), 16777619) >>> 0
    return h
  }
  return [...ids].sort((a, b) => hash(`${key}:${a}`) - hash(`${key}:${b}`))
}
const PAGE = 24
// The most dishes looked up in full for one search: when a search needs more
// than the list gives (a second filter, a time, quickest first). Every list
// TheMealDB has is smaller, so a total is the total, and each dish is looked
// up once (the cache keeps it): about five seconds the first time.
const LOOKED = 600
async function lookup(ids) {
  const out = []
  for (let i = 0; i < ids.length; i += 20) out.push(...(await Promise.all(ids.slice(i, i + 20).map(id => get(`lookup.php?i=${id}`).then(b => b.meals?.[0]).catch(() => null)))))
  return out.filter(Boolean)
}
// With nothing but a time or an order, dishes of the usual kinds of dinner.
const DINNERS = ['Chicken', 'Pasta', 'Vegetarian', 'Beef', 'Seafood', 'Pork']

export const recipesBrowse = {
  name: 'recipes_browse',
  title: 'Recipes',
  view: null,
  domains: ['https://www.themealdb.com'],
  description:
    'Dishes to cook, with a photo, cuisine, kind, ingredients with their amounts and about how long each takes (an estimate: the source gives no times), ' +
    'a page of 24 at a time with the total that match: by words in the name ("curry"), by cuisine ' +
    '(Thai, Italian, Mexican, Japanese, Indian, French...), by kind (Vegetarian, Vegan, Seafood, Dessert, Pasta, ' +
    'Chicken, Beef, Breakfast), by a main ingredient ("salmon"), within a time ("under 30 minutes": max_minutes 30), quickest first; a few at random when given nothing.',
  input: {
    query: z.string().max(40).optional().describe('Words in the dish’s name'),
    cuisine: z.string().max(20).optional().describe('A cuisine, "Thai"'),
    kind: z.string().max(20).optional().describe('A kind of dish, "Vegetarian", "Dessert"'),
    ingredient: z.string().max(30).optional().describe('A main ingredient, "salmon"'),
    max_minutes: z.number().int().min(5).max(600).optional().describe('Only dishes that take about this long or less, by their estimated time'),
    sort: z.enum(['quickest']).optional().describe('"quickest": the quickest dishes first'),
    page: z.number().int().min(1).max(20).optional().describe('Which page of 24, from 1: the next one shows more'),
  },
  example: { cuisine: 'Thai' },
  async run({ query, cuisine, kind, ingredient, max_minutes: max, sort, page = 1 }) {
    const asked = { query: query ?? null, cuisine: cuisine ?? null, kind: kind ?? null, ingredient: ingredient ?? null, max_minutes: max ?? null, sort: sort ?? null }
    const paged = (all, total, extra = {}) => {
      const pages = Math.max(1, Math.ceil(total / PAGE))
      return { recipes: all.slice((page - 1) * PAGE, page * PAGE).map(card), total, page, pages, more: page < pages, ...extra }
    }
    if (SIMULATE) {
      const recipes = offline(query ?? cuisine ?? kind ?? ingredient ?? '')
      return { widget: 'recipes', mode: 'browse', ...asked, recipes, total: recipes.length, page: 1, pages: 1, more: false, source: 'offline sample' }
    }
    try {
      // Every filter given applies together ("chicken and rice" is dishes
      // with chicken whose name or ingredients have rice): the narrowest
      // list first, then the dishes in full, kept when they match the rest.
      const filters = [ingredient && 'ingredient', query && 'query', cuisine && 'cuisine', kind && 'kind'].filter(Boolean)
      const timed = meal => (max == null || card(meal).minutes <= max)
      const order = meals => (sort === 'quickest' ? [...meals].sort((a, b) => card(a).minutes - card(b).minutes) : meals)
      const key = JSON.stringify(asked)
      let out
      if (filters[0] === 'query') {
        const meals = order(((await get(`search.php?s=${encodeURIComponent(query)}`)).meals ?? []).filter(meal => matches(meal, { query, cuisine, kind, ingredient }) && timed(meal)))
        out = paged(meals, meals.length)
      } else if (filters.length || max != null || sort) {
        let ids
        if (filters[0] === 'ingredient') ids = await withIngredient(ingredient)
        else if (filters.length) {
          const filter = filters[0] === 'cuisine' ? `a=${encodeURIComponent(cuisine)}` : `c=${encodeURIComponent(kind)}`
          ids = ((await get(`filter.php?${filter}`)).meals ?? []).map(m => m.idMeal)
        } else {
          ids = [...new Set((await Promise.all(DINNERS.map(c => get(`filter.php?c=${c}`).then(b => b.meals ?? []).catch(() => [])))).flat().map(m => m.idMeal))]
        }
        ids = spread(ids, key)
        if (filters.length > 1 || max != null || sort) {
          const meals = order((await lookup(ids.slice(0, LOOKED))).filter(meal => matches(meal, { query, cuisine, kind, ingredient }) && timed(meal)))
          out = paged(meals, meals.length, ids.length > LOOKED ? { note: `Looked through ${LOOKED} of the ${ids.length} dishes.` } : {})
        } else {
          const pageIds = ids.slice((page - 1) * PAGE, page * PAGE)
          out = { ...paged([], ids.length), recipes: (await lookup(pageIds)).map(card) }
        }
      } else {
        const seen = new Set()
        const meals = (await Promise.all(Array.from({ length: 10 }, () => fetchJson(`${API}/random.php`).then(b => b.meals?.[0]).catch(() => null))))
          .filter(m => m && !seen.has(m.idMeal) && seen.add(m.idMeal))
        out = { recipes: meals.map(card), total: meals.length, page: 1, pages: 1, more: false }
      }
      return { widget: 'recipes', mode: 'browse', ...asked, ...out, times: 'estimated: TheMealDB gives no cooking times', source: 'TheMealDB' }
    } catch (error) {
      const recipes = offline(query ?? cuisine ?? kind ?? ingredient ?? '')
      return { widget: 'recipes', mode: 'browse', ...asked, recipes, total: recipes.length, page: 1, pages: 1, more: false, source: 'offline sample', note: `TheMealDB unreachable (${error.message}).` }
    }
  },
  summary(d) {
    const what = [d.query, d.cuisine, d.kind, d.ingredient, d.max_minutes != null && `about ${d.max_minutes} min or less`, d.sort === 'quickest' && 'quickest first'].filter(Boolean).join(', ') || 'a few at random'
    if (!d.recipes.length) return `No recipes for ${what} (${d.source}).`
    const shown = d.total > d.recipes.length ? `${d.recipes.length} of ${d.total} recipes (page ${d.page} of ${d.pages})` : `${d.recipes.length} recipes`
    return `${shown} for ${what}: ${d.recipes.slice(0, 6).map(r => `${r.title} (about ${r.minutes} min, ${r.minutes_note}; id ${r.id})`).join('; ')}. Times are estimates: the source gives none. Source: ${d.source}.`
  },
}

export const recipeGet = {
  name: 'recipe_get',
  title: 'Recipe',
  view: null,
  domains: ['https://www.themealdb.com'],
  description: 'One dish in full: its photo, ingredients with amounts, steps and a video link when there is one. Use an id from recipes_browse.',
  input: { id: z.string().min(1).max(12).describe('A recipe id from recipes_browse') },
  example: { id: '52814' },
  async run({ id }) {
    if (!SIMULATE) {
      try {
        const meal = (await get(`lookup.php?i=${encodeURIComponent(id)}`)).meals?.[0]
        if (meal) {
          return {
            widget: 'recipes',
            mode: 'recipe',
            ...card(meal),
            amounts: ingredients(meal),
            steps: (meal.strInstructions ?? '').split(/\r?\n+/).map(s => s.replace(/^(step\s*)?\d+[.):]?\s*/i, '').trim()).filter(s => s.length > 3),
            video: meal.strYoutube || null,
            link: meal.strSource || null,
            source: 'TheMealDB',
          }
        }
      } catch {
        /* the sample below */
      }
    }
    const r = OFFLINE.find(x => x.id === id) ?? OFFLINE[0]
    return { widget: 'recipes', mode: 'recipe', ...r, amounts: r.ingredients.map(name => ({ name, amount: '' })), steps: ['Cook it as you like.'], video: null, link: null, source: 'offline sample' }
  },
  summary(d) {
    // The amounts too: an answer written from this summary must never say the source has none.
    const amounts = d.amounts.map(a => (a.amount ? `${a.amount} ${a.name}` : a.name)).join('; ')
    return `${d.title} (${d.cuisine ?? ''} ${d.kind ?? ''}): ${d.amounts.length} ingredients (${amounts}), ${d.steps.length} steps. Source: ${d.source}.`
  },
}
