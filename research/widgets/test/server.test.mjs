// The MCP server, through a real MCP client over stdio: every tool is listed
// with its view, answers with text and structured data, and every view is an
// MCP App resource. WIDGETS_SIMULATE keeps it off the network.
//
//   cd server && npm install && npm test
import assert from 'node:assert/strict'
import { after, before, test } from 'node:test'

import { Client } from '../server/node_modules/@modelcontextprotocol/sdk/dist/esm/client/index.js'
import { StdioClientTransport } from '../server/node_modules/@modelcontextprotocol/sdk/dist/esm/client/stdio.js'

const client = new Client({ name: 'widgets-test', version: '0.0.0' })

before(async () => {
  await client.connect(
    new StdioClientTransport({
      command: process.execPath,
      args: [new URL('../server/index.mjs', import.meta.url).pathname],
      env: { ...process.env, WIDGETS_SIMULATE: '1' },
    }),
  )
})
after(() => client.close())

const call = async (name, args) => {
  const result = await client.callTool({ name, arguments: args })
  assert.equal(result.isError, undefined, JSON.stringify(result.content))
  return result
}

test('the server says when to open an app', () => {
  assert.match(client.getInstructions(), /call apps_find first/)
})

test('every widget tool names its view, and the sources are plain tools', async () => {
  const { tools } = await client.listTools()
  const names = tools.map(t => t.name).sort()
  assert.deepEqual(names, ['app_call', 'app_change', 'app_kit', 'app_make', 'app_open', 'app_save', 'app_source', 'app_undo', 'apps_find', 'cart_add', 'cart_view', 'exchange_rate', 'flight_track', 'my_apps', 'recipe_get', 'recipes_browse', 'shop_search', 'show_findings', 'stock_quote', 'table_book', 'table_cancel', 'table_search'])
  for (const tool of tools) {
    if (/^(table|recipe)|^(apps_find|app_kit|app_save|app_undo|exchange_rate)$/.test(tool.name)) {
      assert.equal(tool._meta?.ui, undefined, tool.name)
      continue
    }
    assert.match(tool._meta.ui.resourceUri, /^ui:\/\/widgets\/(stock|flight|shop|app|apps)\.html$/, tool.name)
    // No output schema: Claude Code would hand the model structuredContent
    // (the chart's every point) in place of the short text.
    assert.equal(tool.outputSchema, undefined, tool.name)
  }
})

test('exchange_rate answers a rate with its time and source, simulated and labelled so here', async () => {
  const r = await call('exchange_rate', { from: 'eur', to: 'USD', amount: 80 })
  const d = r.structuredContent
  assert.equal(d.from, 'EUR')
  assert.equal(d.to, 'USD')
  assert.equal(d.source, 'simulated')
  assert.equal(d.converted, Math.round(80 * d.rate * 100) / 100)
  assert.match(r.content[0].text, /^1 EUR = [\d.]+ USD \(so [\d.]+ USD\)\. Source: simulated/)
})

test('stock_quote answers a sentence and the chart data', async () => {
  const r = await call('stock_quote', { symbol: 'nvda', range: '5d' })
  const d = r.structuredContent
  assert.equal(d.widget, 'stock')
  assert.equal(d.symbol, 'NVDA')
  assert.equal(d.source, 'simulated')
  assert.ok(d.points.length > 20)
  assert.equal(d.price, d.points.at(-1)[1])
  assert.match(r.content[0].text, /NVIDIA Corporation \(NVDA\)/)
})

test('flight_track finds a flight by its IATA number', async () => {
  const r = await call('flight_track', { flight: 'UA 857' })
  const d = r.structuredContent
  assert.equal(d.callsign, 'UAL857')
  assert.equal(d.airline, 'United Airlines')
  assert.ok(d.found && d.trail.length > 1)
  assert.match(r.content[0].text, /^UAL857 \(United Airlines\) is airborne over /)
})

test('shop_search filters by price and the cart adds up', async () => {
  const r = await call('shop_search', { query: 'running shoes', maxPrice: 100 })
  const d = r.structuredContent
  assert.ok(d.items.length > 0)
  assert.ok(d.items.every(i => i.price <= 100))
  const added = (await call('cart_add', { id: d.items[0].id, qty: 2 })).structuredContent
  assert.equal(added.count, 2)
  assert.equal(added.total, Math.round(d.items[0].price * 2 * 100) / 100)
  const cart = (await call('cart_view', {})).structuredContent
  assert.equal(cart.mode, 'cart')
  assert.equal(cart.cart[0].qty, 2)
})

test('every view is an MCP App resource that carries its own client', async () => {
  for (const view of ['stock', 'flight', 'shop', 'app', 'apps']) {
    const { contents } = await client.readResource({ uri: `ui://widgets/${view}.html` })
    assert.equal(contents[0].mimeType, 'text/html;profile=mcp-app')
    assert.match(contents[0].text, /globalThis\.McpApps=\{/)
    assert.doesNotMatch(contents[0].text, /kit:(css|sdk|js)|kit:world/)
  }
})

test('the sources answer a sentence and their data: a table to book, recipes to browse', async () => {
  const found = await call('table_search', { where: 'North Beach, San Francisco', date: '2026-11-20', time: '8pm', party: 4 })
  assert.equal(found.structuredContent.time, '20:00')
  assert.ok(found.structuredContent.restaurants.length > 0)
  assert.match(found.content[0].text, /made up/)
  const place = found.structuredContent.restaurants.find(r => r.slots.length)
  const booked = await call('table_book', { restaurant: place.id, name: place.name, date: '2026-11-20', time: place.slots[0], party: 4 })
  assert.equal(booked.structuredContent.confirmed, true)
  assert.match(booked.structuredContent.code, /^DEMO-/)
  const dishes = await call('recipes_browse', { cuisine: 'Thai' })
  assert.equal(dishes.structuredContent.recipes[0].cuisine, 'Thai')
  const dish = await call('recipe_get', { id: dishes.structuredContent.recipes[0].id })
  assert.ok(dish.structuredContent.amounts.length > 0)
})

test('stock_quote never makes up a stock: three tickers in one, or text that is not a ticker, are refused', async () => {
  for (const symbol of ['SBUX,GIS,MDLZ', 'SBUX GIS']) {
    const r = await client.callTool({ name: 'stock_quote', arguments: { symbol } })
    assert.equal(r.isError, true, symbol)
    assert.match(r.content[0].text, /One ticker a call/)
  }
  const odd = await client.callTool({ name: 'stock_quote', arguments: { symbol: 'Mondelez!' } })
  assert.equal(odd.isError, true)
  assert.match(odd.content[0].text, /not a ticker/)
})

test('an app asked for several corrections at once gets them all; a plan keeps every day', async () => {
  const { INPUT } = await import('../server/widgets/inputs.mjs')
  const two = { add: [{ item: 'lunch', amount: 4.8 }], edit: [{ find: { client: 'Johnson' }, set: { day: 'Tuesday' } }, { find: { client: 'Patel' }, set: { time: '10:00' } }] }
  assert.ok(INPUT.values.safeParse(two).success, 'two edits in one request (MARCUS-1)')
  assert.ok(INPUT.values.safeParse({ edit: { find: { item: 'groceries' }, set: { amount: 32.4 } } }).success)
  const { clip } = await import('../server/widgets/findings.mjs')
  const days = Array.from({ length: 15 }, (_, i) => ({ name: `Apr ${6 + i}`, about: 'a day' }))
  assert.equal(clip({ query: 'trip', documents: [{ title: 'Guide', summary: 'x' }], items: days }).items.length, 15, 'a 14-day plan keeps all its days (TOM-2)')
})

// The kit's own helpers, read from its source as the page runs them.
const kit = (await import('node:fs')).readFileSync(new URL('../server/views/kit.js', import.meta.url), 'utf8')
const fn = name => new Function(`${kit.match(new RegExp(`function names\\([\\s\\S]*?\\n}`))?.[0] ?? ''}\n${kit.match(new RegExp(`function ${name}\\([\\s\\S]*?\\n}`))[0]}\nreturn ${name}`)()

test('the kit reads numbers as people type them, and money keeps its cents', () => {
  const parse = fn('parse')
  for (const [typed, n] of [['2,50', 2.5], ['2.50', 2.5], ['1.234,56', 1234.56], ['1,234.56', 1234.56], ['1,500', 1500], ['7,5 %', 7.5], ['€ 3,50', 3.5], ['0,500', 0.5], ['-3,2', -3.2], [' 680 ', 680], ['12.5 hrs', 12.5]]) assert.equal(parse(typed), n, typed)
  assert.ok(Number.isNaN(parse('abc')))
  assert.ok(Number.isNaN(parse('')))
  const money = new Function(`${kit.match(/const money = [^\n]*/)[0]}\nreturn money`)()
  assert.match(money(1726.45), /1,726\.45/, 'LENA-5: a total over $1,000 keeps its cents')
  assert.match(money(3703), /3,703\.00/)
})

test('an app opened again keeps what was typed since over what it was last asked', () => {
  const replayed = fn('replayed')
  const asked = { again: true, values: { budget: 650, month: 'Nov', f: 'tan(x)' } }
  // INES-1: 680 typed in the budget box, then the app opened again.
  assert.deepEqual(replayed(asked, { state: { budget: 680 } }).values, { month: 'Nov', f: 'tan(x)' })
  assert.deepEqual(replayed(asked, { Budget: 680, month: 'Oct' }).values, { f: 'tan(x)' })
  // Nothing kept here (another browser): the request's values are all there.
  assert.deepEqual(replayed(asked, {}).values, asked.values)
  // A new request's values win, as before.
  assert.deepEqual(replayed({ values: { budget: 700 } }, { budget: 680 }).values, { budget: 700 })
})

test('a dish’s time is an estimate from its own words, and says so (MAYA-4)', async () => {
  const { estimate } = await import('../server/widgets/recipes.mjs')
  const meal = (name, steps, n = 6) => ({ strMeal: name, strInstructions: steps, ...Object.fromEntries(Array.from({ length: n }, (_, i) => [`strIngredient${i + 1}`, `thing ${i}`])) })
  assert.deepEqual(estimate(meal('15-minute chicken & halloumi burgers', 'Fry for 3-4 mins on each side.\nServe.')), { minutes: 15, minutes_note: 'from its name' })
  // 3.5 min on each side is 7, and 11 to get 6 things ready: 20.
  assert.deepEqual(estimate(meal('Burgers', 'Shape the patties and season them well.\nFry for 3-4 mins on each side, until golden.')), { minutes: 20, minutes_note: 'estimated from its steps' })
  const marinated = estimate(meal('Shawarma', 'Mix the spices into the yoghurt.\nMarinate overnight or up to 24 hours.\nGrill for 5 minutes on each side.'))
  assert.equal(marinated.minutes, 500)
  assert.equal(marinated.minutes_note, 'estimated from its steps, 8 h of it waiting')
  // "every 5 minutes" is not five more minutes, and a step that cooks without a time is ten.
  assert.equal(estimate(meal('Stew', 'Simmer for 30 minutes, stirring every 5 minutes.\nBake in the oven until golden.')).minutes, 50)
  assert.equal(estimate(meal('Salad', 'Chop everything finely.\nToss it all together and serve.')).minutes_note, 'a rough guess: the recipe gives no times')
})

test('a typed pattern that runs without end is refused outside the chat, and matches are found (DEV-1)', () => {
  const runaway = new Function(`${kit.match(/const RUNAWAY = [^\n]*/)[0]}\nreturn RUNAWAY`)()
  for (const p of ['(a+)+$', '(a*)*', '(\\w+\\s?)+$', '(x+x+)+y', '([a-z]+)*']) assert.ok(runaway.test(p), p)
  for (const p of ['a+$', '(ab)+', '\\d{3}-\\d{4}', '(foo|bar)+', '^(\\w+)@(\\w+)\\.com$']) assert.ok(!runaway.test(p), p)
  const matchAll = fn('matchAll')
  assert.deepEqual(matchAll('(\\d+)-(\\d+)', '', 'a 12-34 b 5-6').matches.map(m => [m.index, m.text, m.groups]), [[2, '12-34', ['12', '34']], [10, '5-6', ['5', '6']]])
  assert.equal(matchAll('a*', '', 'bab').matches.length, 4, 'an empty match moves on')
  assert.match(matchAll('(', '', 'x').error, /Invalid|Unterminated|group/i)
})

test('what a helper link may keep: adding and ticking, not changing or taking out (INES-2, LENA-4)', async () => {
  const { changesWhatIsThere: changes } = await import('../server/apps.mjs')
  const list = [{ id: 1, item: 'milk', done: false }, { id: 2, item: 'eggs' }]
  assert.equal(changes(list, [{ ...list[0], done: true }, list[1], { id: 3, item: 'bread' }]), false, 'ticking and adding')
  assert.equal(changes(list, [{ ...list[0], note: 'semi-skimmed' }, list[1]]), false, 'a field that was empty filled in')
  assert.equal(changes(list, [list[0]]), true, 'an entry taken out')
  assert.equal(changes(list, []), true, 'the list cleared')
  assert.equal(changes(list, [{ id: 1, done: true }, list[1]]), true, 'a field of an entry taken out')
  assert.equal(changes([{ id: 'a', amount: 98.75 }], [{ id: 'a', amount: 9.75 }]), true, 'an invoice amount changed')
  assert.equal(changes({ budget: 650 }, { budget: 900 }), true, 'a budget changed')
  assert.equal(changes({ budget: 650 }, { budget: 650, Dec: [] }), false, 'a month added')
  assert.equal(changes({ Nov: [{ id: 'x' }] }, { Dec: [] }), true, 'a month taken out')
  assert.equal(changes(['a', 'b'], ['c', 'a', 'b']), false, 'added at the top')
  assert.equal(changes(['a', 'b'], ['a']), true)
  assert.equal(changes([{ item: 'milk', done: false }], [{ item: 'milk', done: true }, { item: 'tea' }]), false, 'a list without ids ticked')
  assert.equal(changes({ rows: [{ id: 1 }], updated: '2026-11-19T08:00:00Z' }, { rows: [{ id: 1 }, { id: 2 }], updated: '2026-11-19T09:00:00Z' }), false, 'the time it was kept moves')
  assert.equal(changes(null, []), false, 'nothing was there')
  assert.equal(changes('note', ''), true)
  assert.equal(changes(false, true), false, 'a tick')
  assert.equal(changes(true, false), true, 'a tick taken back, a night un-booked (TOM-2)')
})
