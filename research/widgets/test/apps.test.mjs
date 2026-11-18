// Instant apps in an MCP host, through a real MCP client and a real
// chloedb-server: the host's model finds a saved app or, with the kit,
// makes one; the app view gets its source and may call only the app's own
// tools; a saved app is found again for a request like the one it served;
// a change is a new version and undo goes back; and Your apps lists them.
//
//   node --test test/apps.test.mjs     (needs chloedb-server: CHLOEDB_SERVER_BIN, or a build in the checkout)
import assert from 'node:assert/strict'
import { mkdtempSync, rmSync } from 'node:fs'
import os from 'node:os'
import path from 'node:path'
import { after, before, test } from 'node:test'

import { Client } from '../server/node_modules/@modelcontextprotocol/sdk/dist/esm/client/index.js'
import { InMemoryTransport } from '../server/node_modules/@modelcontextprotocol/sdk/dist/esm/inMemory.js'

process.env.WIDGETS_SIMULATE = '1'
const { build } = await import('../server/index.mjs')
const { useStore } = await import('../server/apps.mjs')
const { Apps } = await import('../server/store.mjs')
const { findServerBinary } = await import('../../../chloedb-apps/engine/strata.mjs')

const bin = process.env.CHLOEDB_SERVER_BIN || findServerBinary(new URL('../../../', import.meta.url).pathname)
const skip = bin ? false : 'no chloedb-server binary (set CHLOEDB_SERVER_BIN)'
if (bin) process.env.CHLOEDB_SERVER_BIN = bin

// An app as the host's model writes it from app_kit: products side by side.
const COMPARE = `<style>.cols{display:grid;grid-template-columns:repeat(auto-fit,minmax(160px,1fr));gap:10px}</style>
<div id="root" class="empty">Loading…</div>
<script type="module">
const { connect, call, esc, money } = globalThis.Kit
const root = document.getElementById('root')
let app
function draw(d) {
  root.className = 'cols'
  root.innerHTML = d.items.slice(0, 2).map(i => '<div class="card"><b>' + esc(i.title) + '</b> ' + money(i.price) + ' <button class="btn" data-id="' + i.id + '">Add to cart</button></div>').join('')
}
root.onclick = async e => { const b = e.target.closest('button'); if (b) { await call(app, 'cart_add', { id: Number(b.dataset.id) }); b.textContent = 'In cart' } }
app = await connect('compare', draw)
</script>`

let dir
let apps
let client

before(async () => {
  if (skip) return
  dir = mkdtempSync(path.join(os.tmpdir(), 'instant-apps-mcp-'))
  apps = await Apps.open({ dir, shared: false })
  useStore(apps)
  const [a, b] = InMemoryTransport.createLinkedPair()
  await build().connect(a)
  client = new Client({ name: 'apps-test', version: '0.0.0' })
  await client.connect(b)
})

after(async () => {
  await client?.close()
  apps?.close()
  if (dir) rmSync(dir, { recursive: true, force: true })
})

const call = async (name, args = {}) => client.callTool({ name, arguments: args })
const ok = async (name, args) => {
  const result = await call(name, args)
  assert.notEqual(result.isError, true, `${name}: ${JSON.stringify(result.content)}`)
  return result
}
const text = result => result.content.map(c => c.text).join('\n')

test('the app view draws any app, and only the view sees its two tools', { skip }, async () => {
  const { tools } = await client.listTools()
  const by = Object.fromEntries(tools.map(t => [t.name, t]))
  for (const name of ['app_open', 'app_make', 'app_change']) assert.equal(by[name]._meta.ui.resourceUri, 'ui://widgets/app.html', name)
  assert.equal(by.my_apps._meta.ui.resourceUri, 'ui://widgets/apps.html')
  for (const name of ['app_source', 'app_call']) assert.deepEqual(by[name]._meta.ui.visibility, ['app'], name)
  const { contents } = await client.readResource({ uri: 'ui://widgets/app.html' })
  // Its pictures may come from any tool's data, and it reaches no network.
  assert.match(contents[0].text, /img-src data: [^;]*https:\/\/cdn\.dummyjson\.com[^;]*https:\/\/www\.themealdb\.com/)
  assert.match(contents[0].text, /connect-src 'none'/)
  assert.match(contents[0].text, /app_source/)
})

test('a saved app is found first for a request like what it does', { skip }, async () => {
  const found = await ok('apps_find', { request: 'how is apple stock doing this month' })
  assert.equal(found.structuredContent.apps[0].id, 'stock-chart')
  assert.match(text(found), /app_open/)
  // Opening it fills it with this request's arguments.
  const opened = await ok('app_open', { id: 'stock-chart', args: { symbol: 'AAPL' }, request: 'how is apple stock doing this month' })
  assert.equal(opened.structuredContent.instant.how, 'recalled')
  assert.equal(opened.structuredContent.symbol, 'AAPL')
  // A view made by hand is mounted as a fragment too, with no kit of its own.
  const { structuredContent } = await ok('app_source', { id: 'stock-chart' })
  assert.match(structuredContent.source, /<script type="module">/)
  assert.doesNotMatch(structuredContent.source, /kit:(css|sdk|js)|<head>|<html/)
})

test('a new app: the kit, then made, its calls held to its tools, saved and found again', { skip }, async () => {
  const kit = await ok('app_kit', { tool: 'shop_search', args: { query: 'blender' } })
  assert.match(text(kit), /Kit\.connect\(name, draw\)/)
  assert.match(text(kit), /Pass the fragment to app_make as html/)
  assert.match(text(kit), /- cart_add:/)
  assert.match(text(kit), /"items"/)

  const bad = await call('app_make', { name: 'Broken', about: 'x', kind: 'info', tool: 'shop_search', args: {}, tools: [], html: '<div>no script here at all, nothing</div>', request: 'x' })
  assert.equal(bad.isError, true)

  const request = 'put the Boxed Blender and the Hand Blender side by side'
  const made = await ok('app_make', { name: 'Product comparison', about: 'Puts chosen products side by side: price, rating, stock.', kind: 'info', tool: 'shop_search', args: { query: 'blender' }, tools: ['shop_search', 'cart_add'], html: '```html\n' + COMPARE + '\n```', request })
  const app = made.structuredContent.instant
  assert.equal(app.how, 'made')
  assert.equal(app.saved, false)
  assert.ok(made.structuredContent.items.length > 0)
  assert.match(text(made), /draft until it is saved/)
  // Claude Code hands the model structuredContent: the guidance is there
  // too, and what the app was filled with, which a pinned card refreshes by.
  assert.match(app.note, /draft until it is saved/)
  assert.deepEqual(app.args, { query: 'blender' })
  assert.equal((await ok('app_source', { id: app.id })).structuredContent.source, COMPARE, 'the fence is taken off')

  // Its buttons reach its own tools and nothing else; an action says so.
  const added = await ok('app_call', { id: app.id, tool: 'cart_add', args: { id: made.structuredContent.items[0].id } })
  assert.equal(added.structuredContent.acted, true)
  const refused = await call('app_call', { id: app.id, tool: 'table_book', args: {} })
  assert.equal(refused.isError, true)
  assert.match(text(refused), /not one of this app/)

  // A draft is not found; a saved one is, for another request like it.
  assert.ok(!(await ok('apps_find', { request: 'compare two headphones side by side' })).structuredContent.apps.some(a => a.id === app.id))
  await ok('app_save', { id: app.id })
  const found = await ok('apps_find', { request: 'compare two headphones side by side' })
  assert.equal(found.structuredContent.apps[0].id, app.id)
  const again = await ok('app_open', { id: app.id, args: { query: 'headphones' }, request: 'compare two headphones side by side' })
  assert.equal(again.structuredContent.instant.how, 'recalled')
  assert.equal(again.structuredContent.query, 'headphones')

  // A change is a new version, and undo goes back.
  const changed = await ok('app_change', { id: app.id, html: COMPARE.replace('Add to cart', 'Buy'), change: 'say Buy' })
  assert.equal(changed.structuredContent.instant.version, 2)
  assert.equal(changed.structuredContent.query, 'headphones', 'it shows what it showed last')
  assert.match((await ok('app_source', { id: app.id })).structuredContent.source, /Buy/)
  await ok('app_undo', { id: app.id })
  assert.equal((await ok('app_source', { id: app.id })).structuredContent.version, 1)

  // Your apps lists it with the others, and taking it out keeps it from being found.
  const mine = await ok('my_apps')
  assert.ok(mine.structuredContent.apps.some(a => a.id === app.id && a.kind === 'info'))
  await ok('app_save', { id: app.id, saved: false })
  assert.ok(!(await ok('my_apps')).structuredContent.apps.some(a => a.id === app.id))
})
