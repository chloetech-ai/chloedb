// Recipe amounts as one buys them: read from what a recipe says ("2/3 Cup",
// "450 grams Boneless skin", "4 - 6", "Halved"), scaled to the people asked
// for, in pounds and cups (or grams and millilitres), whole things rounded
// up to what a shop sells ("2 avocados"), and several dishes' amounts of one
// thing added up into one line of a shopping list (MAYA-7).

const FRACTIONS = { '½': 0.5, '⅓': 1 / 3, '⅔': 2 / 3, '¼': 0.25, '¾': 0.75, '⅛': 0.125 }
const NUMBER = String.raw`(\d+\s+\d+\/\d+|\d+\/\d+|\d+(?:[.,]\d+)?\s*[½⅓⅔¼¾⅛]?|[½⅓⅔¼¾⅛])`
// Each unit: its kind, and how much of the kind's base (grams, millilitres,
// or one of a thing) it is.
const UNITS = [
  [/^(kg|kgs|kilo(gram)?s?)\b/i, 'mass', 1000],
  [/^(g|gr|grams?|grammes?)\b/i, 'mass', 1],
  [/^(lbs?|pounds?)\b/i, 'mass', 453.6],
  [/^(oz|ounces?)\b/i, 'mass', 28.35],
  [/^(l|ltr|litres?|liters?)\b/i, 'volume', 1000],
  [/^(ml|millilitres?|milliliters?)\b/i, 'volume', 1],
  [/^(cl)\b/i, 'volume', 10],
  [/^(cups?|c)\b/i, 'volume', 240],
  [/^(tbsp|tbs|tblsp|tablespoons?|tbls?)\b/i, 'volume', 15],
  [/^(tsp|teaspoons?)\b/i, 'volume', 5],
  [/^(pints?)\b/i, 'volume', 473],
  [/^(fl\s*oz)\b/i, 'volume', 29.6],
]
// Units one buys in as they are: a can, a clove, a bunch.
const AS_SOLD = /^(cans?|tins?|cloves?|bunch(es)?|heads?|sticks?|sprigs?|slices?|packets?|packs?|jars?|bottles?|stalks?|leaves|leaf|fillets?|breasts?|thighs?|drumsticks?|rashers?|knobs?|handfull?s?)\b/i
const SIZES = /^(large|medium|small|big|whole|thick|thin|fresh)\b/i
// Things one buys by the piece; anything else without a number is "some".
const COUNTABLE = /\b(avocados?|cloves?|lemons?|limes?|oranges?|onions?|shallots?|challots?|tomato(es)?|peppers?|eggs?|carrots?|cucumbers?|apples?|bananas?|potato(es)?|zucchinis?|courgettes?|aubergines?|eggplants?|chickens?|chilli(es)?|chil(e|i)s?|buns?|tortillas?|pitta?s?|baguettes?|leeks?|celery|cabbages?|lettuces?|mangos?|mangoes|pears?|peaches|plums?|corn|cobs?|beetroots?|parsnips?|turnips?|squash|pumpkins?|fennel bulbs?|sweet potato(es)?)\b/i
// Spices, herbs and seasonings: a jar or a bunch, not a count.
const SEASONING = /\b(cumin|coriander|fennel seeds?|turmeric|paprika|cinnamon|nutmeg|cardamom|cloves? \(spice\)|allspice|ginger|oregano|thyme|basil|parsley|mint|dill|rosemary|sage|bay lea(f|ves)|chives|tarragon|saffron|chilli powder|chili powder|curry powder|garam masala|five spice|vanilla|fennel)\b/i
// Words that say how to cut it, not how much: one of the thing.
const PREP = /^(halved|chopped|sliced|diced|peeled|minced|grated|crushed|to serve|to garnish|garnish|for garnish|for frying|for serving|cubed|shredded|seeded|deseeded|julienned|quartered|whole|finely|roughly|thinly|beaten|melted|softened|juice of|zest of|boiled|cooked|steamed|fresh|dried|ground|to taste|optional)\b/i
const LITTLE = /^(pinch|dash|sprinkl|drizzle|splash|to taste|a little|some|as needed|as required|season)/i
// What most kitchens have: listed apart, to check rather than to buy.
const PANTRY = /^(salt|sea salt|black pepper|pepper|ground pepper|olive oil|vegetable oil|sunflower oil|oil|sugar|caster sugar|plain flour|flour|all purpose flour|butter for greasing|cooking spray)$/i
// Nobody buys these.
const NOTHING = /^(water|cold water|hot water|boiling water|warm water|ice|ice cubes)$/i

function number(text) {
  const t = text.trim().replace(',', '.')
  const mixed = /^(\d+)\s+(\d+)\/(\d+)$/.exec(t)
  if (mixed) return Number(mixed[1]) + Number(mixed[2]) / Number(mixed[3])
  const frac = /^(\d+)\/(\d+)$/.exec(t)
  if (frac) return Number(frac[1]) / Number(frac[2])
  const uni = /^(\d*(?:\.\d+)?)\s*([½⅓⅔¼¾⅛])$/.exec(t)
  if (uni) return Number(uni[1] || 0) + FRACTIONS[uni[2]]
  return Number(t)
}

// What a recipe's amount says: a quantity of a kind (mass, volume, count or
// a unit sold as is), and what is left over as a note.
export function parse(amount) {
  let text = String(amount ?? '').trim()
  if (!text) return { kind: 'count', qty: 1, unit: null, note: '' }
  if (LITTLE.test(text)) return { kind: 'little', qty: 0, unit: null, note: text.toLowerCase() }
  // "4 - 6", "2-3": the more of the two, since it is to buy.
  const range = new RegExp(`^${NUMBER}\\s*(?:-|–|to|or)\\s*${NUMBER}`, 'i').exec(text)
  let qty
  if (range) {
    qty = Math.max(number(range[1]), number(range[2]))
    text = text.slice(range[0].length).trim()
  } else {
    const lead = new RegExp(`^${NUMBER}`).exec(text)
    if (lead) {
      qty = number(lead[1])
      text = text.slice(lead[0].length).trim()
    }
  }
  // "1 (12 oz.)": that many packs of that size.
  const pack = new RegExp(`^\\(\\s*${NUMBER}\\s*([a-z. ]+?)\\.?\\s*\\)\\s*`, 'i').exec(text)
  if (pack) {
    const inner = parse(`${pack[1]} ${pack[2]}`)
    if (inner.kind === 'mass' || inner.kind === 'volume') return { kind: inner.kind, qty: inner.qty * (qty ?? 1), unit: null, note: text.slice(pack[0].length).trim() }
  }
  // "400g", "2tbsp": the unit right after the number.
  for (const [pattern, kind, base] of UNITS) {
    const m = pattern.exec(text)
    if (m) {
      const note = text.slice(m[0].length).replace(/^[\s.,/-]+/, '').trim()
      return { kind, qty: (qty ?? 1) * base, unit: null, note: PREP.test(note) || !note ? note.toLowerCase() : note }
    }
  }
  const size = SIZES.exec(text)
  if (size && qty != null) text = text.slice(size[0].length).trim()
  const sold = AS_SOLD.exec(text)
  if (sold) return { kind: 'unit', qty: qty ?? 1, unit: single(sold[1].toLowerCase()), note: text.slice(sold[0].length).trim() }
  // Only how to cut it ("Halved"): one of a thing, or some of things.
  if (qty == null) return { kind: 'count', qty: 1, unit: null, unsaid: true, note: PREP.test(text) ? text.toLowerCase() : text }
  return { kind: 'count', qty, unit: null, note: text }
}

const nice = n => {
  const quarters = Math.round(n * 4) / 4
  const whole = Math.floor(quarters)
  const part = { 0: '', 0.25: '¼', 0.5: '½', 0.75: '¾' }[quarters - whole]
  return whole ? `${whole}${part ? ` ${part}` : ''}` : part || '¼'
}
const up = (n, step) => Math.ceil(n / step - 1e-9) * step
const plural = (word, n) => (n > 1 && !/s$/.test(word) ? (/(sh|ch|x|ss)$/.test(word) ? `${word}es` : /[^aeiou]y$/.test(word) ? `${word.slice(0, -1)}ies` : `${word}s`) : word)
const single = word => (/^leaves$/.test(word) ? 'leaf' : /^handfull?s?$/.test(word) ? 'bunch' : /(ch|sh|x)es$/.test(word) ? word.slice(0, -2) : /oes$/.test(word) ? word.slice(0, -2) : /ies$/.test(word) ? `${word.slice(0, -3)}y` : word.replace(/([^s])s$/, '$1'))
// Juice is bought as the fruit.
const FRUIT = /^(lemon|lime|orange)s?\s+(juice|zest)$/i

// A quantity of a kind, in the units one buys it in.
export function shown({ kind, qty, unit }, units = 'us') {
  if (kind === 'little') return 'a little'
  if (kind === 'mass') {
    if (units === 'metric') return qty >= 1000 ? `${Math.round(up(qty, 100)) / 1000} kg` : `${Math.max(10, Math.round(up(qty, 10)))} g`
    const oz = qty / 28.35
    return oz >= 15.5 ? `${nice(up(oz / 16, 0.25))} lb` : `${Math.max(1, Math.ceil(oz))} oz`
  }
  if (kind === 'volume') {
    if (units === 'metric') return qty >= 1000 ? `${Math.round(up(qty, 100)) / 1000} l` : qty >= 30 ? `${Math.round(up(qty, 10))} ml` : qty >= 15 ? `${nice(qty / 15)} tbsp` : `${nice(qty / 5)} tsp`
    if (qty >= 60) return `${nice(up(qty / 240, 0.25))} cup${qty > 240 ? 's' : ''}`
    if (qty >= 15) return `${nice(up(qty / 15, 0.5))} tbsp`
    return `${nice(up(qty / 5, 0.25))} tsp`
  }
  if (kind === 'unit') {
    const n = Math.ceil(qty - 1e-9)
    return `${n} ${plural(unit, n)}`
  }
  // A count: whole things, rounded up to what a shop sells.
  return `${Math.max(1, Math.ceil(qty - 1e-9))}`
}

// The name of a thing as a list groups it: "Chicken Breasts" and "chicken
// breast" are one line.
export const keyOf = name =>
  String(name)
    .toLowerCase()
    .replace(/\(.*?\)/g, '')
    .replace(/[^a-z ]/g, ' ')
    .replace(/\s+/g, ' ')
    .trim()
    .split(' ')
    .map(w => (w.length > 3 ? w.replace(/(ies)$/, 'y').replace(/(oes|ches|shes|xes)$/, m => m.slice(0, -2)).replace(/([^s])s$/, '$1') : w))
    .join(' ')

// How many a dish feeds: what its steps say ("Serves 4"), else from its
// main meat or fish (about 150 g, or one breast or fillet, each), else four,
// and how that was found, since TheMealDB has no servings.
const SERVES = /\b(?:serves|serving|servings|feeds|for)\s*:?\s*(\d{1,2})(?:\s*(?:-|to)\s*(\d{1,2}))?\b(?!\s*(?:min|minutes|hours|seconds|degrees|°))(?:\s*(?:people|persons|portions|servings))?/i
const MAIN = /\b(chicken|beef|pork|lamb|mutton|veal|turkey|duck|salmon|cod|tuna|fish|prawns?|shrimps?|mince|steaks?|sausages?|haddock|tofu)\b/i
export function serves(recipe) {
  const said = SERVES.exec(recipe.instructions ?? '')
  if (said && /serv|feed|people|person|portion/i.test(said[0])) {
    const n = Math.max(Number(said[1]), Number(said[2] ?? 0))
    if (n >= 1 && n <= 16) return { serves: n, serves_note: 'as the recipe says' }
  }
  for (const a of recipe.amounts ?? []) {
    if (!MAIN.test(a.name)) continue
    const p = parse(a.amount)
    if (p.kind === 'mass' && p.qty >= 100) return { serves: Math.min(10, Math.max(1, Math.round(p.qty / 150))), serves_note: `about, from its ${a.name.toLowerCase()} (the recipe doesn't say)` }
    if ((p.kind === 'count' || p.kind === 'unit') && p.qty >= 1 && /breast|fillet|steak|chop|thigh|leg|sausage/i.test(`${a.name} ${p.unit ?? ''}`)) {
      const each = /thigh|sausage|drumstick/i.test(`${a.name} ${p.unit ?? ''}`) ? 2 : 1
      return { serves: Math.min(10, Math.max(1, Math.round(p.qty / each))), serves_note: `about, from its ${a.name.toLowerCase()} (the recipe doesn't say)` }
    }
  }
  return { serves: 4, serves_note: 'a guess: the recipe doesn\'t say' }
}

// Each dish's amounts for the people asked, and one list of all of them:
// a thing in several dishes once, its amounts added (by kind, so grams and
// cups of one thing are two amounts on its line), what each dish needs it
// for, and the pantry's staples apart.
export function shopping(recipes, people = 4, units = 'us') {
  const dishes = recipes.map(r => {
    const s = serves(r)
    const factor = people / s.serves
    return { id: r.id, title: r.title, day: r.day ?? null, ...s, factor: Math.round(factor * 100) / 100, amounts: (r.amounts ?? []).map(a => ({ name: a.name, amount: a.amount, for_people: forPeople(a, factor, units) })) }
  })
  const lines = new Map()
  for (const [i, r] of recipes.entries()) {
    for (const a of r.amounts ?? []) {
      let key = keyOf(a.name)
      if (!key || NOTHING.test(a.name.trim())) continue
      let p = parse(a.amount)
      // "2 cloves" of garlic and 6 "Garlic Clove" are one line, in cloves.
      const last = key.split(' ').at(-1)
      if (p.kind === 'count' && !p.unsaid && key.includes(' ') && /^cloves?$/.test(last)) p = { ...p, kind: 'unit', unit: single(last) }
      if (p.kind === 'unit' && !key.endsWith(` ${p.unit}`)) key = `${key} ${p.unit}`
      const fruit = FRUIT.exec(a.name.trim())
      const named = p.kind === 'unit' ? a.name.replace(new RegExp(`\\s+${p.unit}s?$`, 'i'), '') : a.name
      const line = lines.get(key) ?? { name: fruit && p.kind === 'count' ? fruit[1] : named, kinds: new Map(), dishes: [], notes: new Set(fruit ? [`for its ${fruit[2].toLowerCase()}`] : []) }
      const bucket = `${p.kind}:${p.unit ?? ''}`
      const was = line.kinds.get(bucket) ?? { ...p, qty: 0 }
      line.kinds.set(bucket, { ...was, qty: was.qty + p.qty * dishes[i].factor })
      const who = r.day ? `${r.day}` : r.title
      if (!line.dishes.includes(who)) line.dishes.push(who)
      if (p.note && PREP.test(p.note)) line.notes.add(p.note)
      lines.set(key, line)
    }
  }
  const items = []
  const pantry = []
  for (const line of lines.values()) {
    const buy = [...line.kinds.values()].map(p => shown(p, units))
    // "a little" beside a real amount adds nothing.
    const amounts = buy.length > 1 ? buy.filter(b => b !== 'a little') : buy
    const kinds = [...line.kinds.values()]
    const counted = kinds.every(p => p.kind === 'count')
    const n = Math.ceil(kinds.reduce((s, p) => s + p.qty, 0) - 1e-9)
    // Things with no number ("Grape Tomatoes, halved", "Boiled rice"), or
    // a seasoning counted ("1 1/2 Cumin"): some, unless bought by the piece.
    const some = counted && ((kinds.every(p => p.unsaid) && (/s$/.test(line.name.trim()) || !COUNTABLE.test(line.name))) || SEASONING.test(line.name) || !COUNTABLE.test(line.name))
    const name = counted && !some ? (n > 1 ? plural(line.name.toLowerCase(), n) : single(line.name.toLowerCase())) : line.name.toLowerCase()
    const amount = some ? 'some' : amounts.join(' + ')
    const out = { name, buy: `${amount} ${name}`, amount, for: line.dishes, ...(line.notes.size ? { note: [...line.notes].join(', ') } : {}) }
    ;(PANTRY.test(keyOf(line.name)) || PANTRY.test(line.name.trim()) ? pantry : items).push(out)
  }
  items.sort((a, b) => a.name.localeCompare(b.name))
  return { people, units, dishes, items, pantry }
}

// One amount for the people asked, as it is bought.
export function forPeople(a, factor, units = 'us') {
  const p = parse(a.amount)
  if (p.kind === 'little') return a.amount || 'a little'
  const scaled = { ...p, qty: p.qty * factor }
  return `${shown(scaled, units)}${p.note && !PREP.test(p.note) ? ` ${p.note}` : ''}`
}
