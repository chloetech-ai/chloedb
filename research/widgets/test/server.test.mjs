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
  assert.deepEqual(names, ['app_call', 'app_change', 'app_kit', 'app_make', 'app_open', 'app_save', 'app_source', 'app_undo', 'apps_find', 'cart_add', 'cart_view', 'flight_track', 'my_apps', 'recipe_get', 'recipes_browse', 'shop_search', 'stock_quote', 'table_book', 'table_search'])
  for (const tool of tools) {
    if (/^(table|recipe)|^(apps_find|app_kit|app_save|app_undo)$/.test(tool.name)) {
      assert.equal(tool._meta?.ui, undefined, tool.name)
      continue
    }
    assert.match(tool._meta.ui.resourceUri, /^ui:\/\/widgets\/(stock|flight|shop|app|apps)\.html$/, tool.name)
    // No output schema: Claude Code would hand the model structuredContent
    // (the chart's every point) in place of the short text.
    assert.equal(tool.outputSchema, undefined, tool.name)
  }
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
