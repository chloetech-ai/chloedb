// Recipes to browse: dishes with photos by name, cuisine, kind or main
// ingredient, and one dish in full. From TheMealDB's free API (its test key,
// "1"); a small sample without photos when it is unreachable or
// WIDGETS_SIMULATE is set, labelled so.
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
    minutes: estimate(meal),
  }
}

// TheMealDB gives no times: a rough one from the steps and ingredients.
function estimate(meal) {
  const steps = (meal.strInstructions ?? '').split(/\r?\n|\. /).filter(s => s.trim().length > 12).length
  return Math.min(180, Math.max(15, Math.round((steps * 4 + ingredients(meal).length * 2) / 5) * 5))
}

const OFFLINE = [
  ['52772', 'Teriyaki Chicken Casserole', 'Japanese', 'Chicken', ['soy sauce', 'chicken breasts', 'rice', 'stir-fry vegetables']],
  ['52959', 'Baked salmon with fennel & tomatoes', 'British', 'Seafood', ['fennel', 'salmon', 'tomatoes', 'olive oil']],
  ['52814', 'Thai Green Curry', 'Thai', 'Chicken', ['chicken', 'green curry paste', 'coconut milk', 'basil']],
  ['52771', 'Spicy Arrabiata Penne', 'Italian', 'Vegetarian', ['penne', 'olive oil', 'garlic', 'chopped tomatoes']],
  ['52855', 'Banana Pancakes', 'American', 'Dessert', ['banana', 'eggs', 'baking powder', 'vanilla']],
  ['52977', 'Corba', 'Turkish', 'Side', ['lentils', 'onion', 'carrots', 'tomato puree']],
].map(([id, title, cuisine, kind, items]) => ({ id, title, cuisine, kind, tags: [], image: null, ingredients: items, minutes: 30 }))

function offline(words) {
  const w = words.toLowerCase()
  const hits = OFFLINE.filter(r => !w || `${r.title} ${r.cuisine} ${r.kind} ${r.ingredients.join(' ')}`.toLowerCase().includes(w))
  return hits.length ? hits : OFFLINE
}

export const recipesBrowse = {
  name: 'recipes_browse',
  title: 'Recipes',
  view: null,
  domains: ['https://www.themealdb.com'],
  description:
    'Dishes to cook, with a photo, cuisine, kind and main ingredients: by words in the name ("curry"), by cuisine ' +
    '(Thai, Italian, Mexican, Japanese, Indian, French...), by kind (Vegetarian, Vegan, Seafood, Dessert, Pasta, ' +
    'Chicken, Beef, Breakfast) or by a main ingredient ("salmon"); a few at random when given nothing.',
  input: {
    query: z.string().max(40).optional().describe('Words in the dish’s name'),
    cuisine: z.string().max(20).optional().describe('A cuisine, "Thai"'),
    kind: z.string().max(20).optional().describe('A kind of dish, "Vegetarian", "Dessert"'),
    ingredient: z.string().max(30).optional().describe('A main ingredient, "salmon"'),
  },
  example: { cuisine: 'Thai' },
  async run({ query, cuisine, kind, ingredient }) {
    const asked = { query: query ?? null, cuisine: cuisine ?? null, kind: kind ?? null, ingredient: ingredient ?? null }
    if (SIMULATE) return { widget: 'recipes', mode: 'browse', ...asked, recipes: offline(query ?? cuisine ?? kind ?? ingredient ?? ''), source: 'offline sample' }
    try {
      let meals
      if (query) meals = (await get(`search.php?s=${encodeURIComponent(query)}`)).meals ?? []
      else {
        const filter = cuisine ? `a=${encodeURIComponent(cuisine)}` : kind ? `c=${encodeURIComponent(kind)}` : ingredient ? `i=${encodeURIComponent(ingredient.replace(/\s+/g, '_'))}` : null
        if (filter) {
          const ids = ((await get(`filter.php?${filter}`)).meals ?? []).slice(0, 12).map(m => m.idMeal)
          meals = (await Promise.all(ids.map(id => get(`lookup.php?i=${id}`).then(b => b.meals?.[0]).catch(() => null)))).filter(Boolean)
        } else {
          const seen = new Set()
          meals = (await Promise.all(Array.from({ length: 10 }, () => fetchJson(`${API}/random.php`).then(b => b.meals?.[0]).catch(() => null))))
            .filter(m => m && !seen.has(m.idMeal) && seen.add(m.idMeal))
        }
      }
      return { widget: 'recipes', mode: 'browse', ...asked, recipes: meals.slice(0, 12).map(card), source: 'TheMealDB' }
    } catch (error) {
      return { widget: 'recipes', mode: 'browse', ...asked, recipes: offline(query ?? cuisine ?? kind ?? ingredient ?? ''), source: 'offline sample', note: `TheMealDB unreachable (${error.message}).` }
    }
  },
  summary(d) {
    const what = [d.query, d.cuisine, d.kind, d.ingredient].filter(Boolean).join(', ') || 'a few at random'
    if (!d.recipes.length) return `No recipes for ${what} (${d.source}).`
    return `${d.recipes.length} recipes for ${what}: ${d.recipes.slice(0, 6).map(r => `${r.title} (${r.minutes ? `${r.minutes} min, ` : ''}id ${r.id})`).join('; ')}. Source: ${d.source}.`
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
    return `${d.title} (${d.cuisine ?? ''} ${d.kind ?? ''}): ${d.amounts.length} ingredients, ${d.steps.length} steps. Source: ${d.source}.`
  },
}
