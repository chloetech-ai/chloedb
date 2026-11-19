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

// What the Claude of the conversation found itself (a web search, pages it
// read), handed to the saved Search results app.
const FOUND = {
  query: 'quiet headphones for flights',
  documents: [
    {
      title: 'The best noise-cancelling headphones',
      url: 'https://example.com/best',
      date: '2026-10-02',
      kind: 'review',
      summary: 'Sony cancels the most noise.',
      quotes: ['The XM6 cut more rumble than anything else.'],
      facts: { pick: 'Sony WH-1000XM6', price: 449 },
    },
    { title: 'A page with an odd address', url: 'javascript:alert(1)', source: 'somewhere', summary: 'Bose is the most comfortable.', facts: { pick: 'Bose QC Ultra 2', price: 429 } },
  ],
  notes: 'Both put Sony first for quiet.',
}

test('what Claude found fills the Search results app, and is never a tool of its own', { skip }, async () => {
  const { tools } = await client.listTools()
  assert.ok(!tools.some(t => t.name === 'findings'), 'findings is only handed to apps')
  assert.match(tools.find(t => t.name === 'apps_find').description, /found yourself/)

  const found = await ok('apps_find', { request: 'find reviews of quiet headphones' })
  assert.equal(found.structuredContent.apps[0].id, 'search-results')
  assert.match(text(found), /Opens with findings .*"documents":"list/)
  // A word of another app's ("flights") still lists it, for the model to pick.
  const near = await ok('apps_find', { request: 'find reviews of quiet headphones for flights' })
  assert.ok(near.structuredContent.apps.some(a => a.id === 'search-results'))

  const opened = await ok('app_open', { id: 'search-results', args: FOUND, request: 'find reviews of quiet headphones for flights' })
  const d = opened.structuredContent
  assert.equal(d.widget, 'findings')
  assert.equal(d.instant.how, 'recalled')
  assert.equal(d.documents.length, 2)
  // Its source from the address, and an address that is not the web dropped.
  assert.equal(d.documents[0].source, 'example.com')
  assert.equal(d.documents[1].url, null)
  assert.deepEqual(d.facts, ['pick', 'price'])
  assert.match(text(opened), /2 document\(s\) for "quiet headphones for flights": The best noise-cancelling headphones \(example\.com, 2026-10-02\)/)
  const { structuredContent } = await ok('app_source', { id: 'search-results' })
  assert.match(structuredContent.source, /Kit|link\(app, d\.url\)/)

  // Opened again with nothing new, it shows what it showed last.
  const again = await ok('app_open', { id: 'search-results' })
  assert.equal(again.structuredContent.documents[0].title, 'The best noise-cancelling headphones')

  // What its schema refuses makes no app and says why.
  const bad = await call('app_make', { name: 'Reading list', about: 'Articles to read later.', kind: 'info', tool: 'findings', args: { query: 'x', documents: [{ url: 'https://example.com' }] }, tools: ['findings'], html: COMPARE, request: 'keep these to read' })
  assert.equal(bad.isError, true)
  assert.match(text(bad), /findings's args: documents\.0\.title/)
  assert.ok(!(await apps.recall('Reading list articles to read later', 5)).some(a => a.name === 'Reading list'))

  // A made app can be filled by what Claude found too, and the kit shows it.
  const kit = await ok('app_kit', { tool: 'findings' })
  assert.match(text(kit), /Kit\.link\(app, url\)/)
  assert.match(text(kit), /"widget": "findings"/)
})

test('a folder from before a seed was added gets it once, and keeps it out once taken out', { skip }, async () => {
  await apps.save('search-results', false)
  const reopened = await Apps.open({ url: apps.db.url })
  assert.equal((await reopened.get('search-results')).saved, false)
  await apps.db.write([{ op: 'delete', table: 'ia_apps', key: 'search-results' }])
  const again = await Apps.open({ url: apps.db.url })
  assert.equal((await again.get('search-results')).saved, true)
})
