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
  assert.match(tools.find(t => t.name === 'apps_find').description, /show_findings/)

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
  assert.match(text(opened), /From 2 source\(s\): example\.com, somewhere\./)
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

// For a pick, what was found answers it: the bottom line, the pick for each
// need, the options with what each source says.
const PICK = {
  request: 'which noise cancelling headphones should I get',
  query: 'best noise cancelling headphones',
  goal: 'pick',
  answer: { text: 'Get the Sony unless you fly a lot: then the Bose.', cites: [1, 2, 9] },
  needs: [{ need: 'long flights', pick: 'bose qc ultra 2', why: 'Most comfortable', cites: [2] }, { need: 'on a budget', pick: 'Something not listed' }],
  criteria: ['price', 'weight'],
  items: [
    { name: 'Sony WH-1000XM6', verdict: 'Best all-round.', facts: { price: 449, battery: '30 h', 'best for': 'most people' }, says: [{ doc: 1, text: 'Quietest we tried.' }, { doc: 7, text: 'A source that is not there.' }] },
    { name: 'Bose QC Ultra 2', verdict: 'Most comfortable.', facts: { price: 429, battery: '24 h', comfort: 'best' }, says: [{ doc: 2, text: 'Best for flights.' }], url: 'ftp://example.com/x' },
  ],
  documents: [
    { title: 'The best noise-cancelling headphones', url: 'https://example.com/best', summary: 'Sony first.' },
    { title: 'Bose QC Ultra 2 review', url: 'https://www.example.org/bose', summary: 'Bose for flights.' },
  ],
}

test('show_findings answers a pick in the app for what Claude found, its claims held to its sources', { skip }, async () => {
  const { tools } = await client.listTools()
  const tool = tools.find(t => t.name === 'show_findings')
  assert.equal(tool._meta.ui.resourceUri, 'ui://widgets/app.html')
  assert.ok(tool.inputSchema.properties.items && tool.inputSchema.properties.needs, 'its host checks the shape')

  const shown = await ok('show_findings', PICK)
  const d = shown.structuredContent
  assert.equal(d.instant.id, 'search-results')
  assert.equal(d.instant.saved, true)
  assert.equal(d.instant.how, 'recalled')
  assert.equal(d.goal, 'pick')
  assert.deepEqual(d.answer.cites, [1, 2], 'a cite of a source that is not there is dropped')
  assert.deepEqual(d.needs.map(x => x.item), [2, null], 'a pick names its item, whatever its case')
  assert.deepEqual(d.items[0].says.map(x => x.doc), [1])
  assert.equal(d.items[1].url, null)
  assert.equal(d.documents[1].source, 'example.org')
  // The criteria named that the items give, then the other facts, most given first.
  assert.deepEqual(d.criteria, ['price', 'battery', 'comfort'])
  assert.match(text(shown), /^For "best noise cancelling headphones": Get the Sony unless you fly a lot: then the Bose\. Best for: long flights, bose qc ultra 2; on a budget, Something not listed\. Options: Sony WH-1000XM6; Bose QC Ultra 2\. From 2 source\(s\): example\.com, example\.org\./)
  assert.match(text(shown), /shows this in the conversation/)

  const bad = await call('show_findings', { ...PICK, goal: 'shopping' })
  assert.equal(bad.isError, true)
})

test('the research step reads public pages only', async () => {
  const { readPage, publicAddress, text } = await import('../server/reader.mjs')
  for (const url of ['http://127.0.0.1/', 'http://localhost/', 'http://169.254.169.254/latest/meta-data/', 'http://10.1.2.3/', 'http://[::1]/', 'http://[::ffff:10.0.0.1]/', 'file:///etc/passwd', 'https://example.com:8443/', 'http://user:pw@example.com/', 'http://db.instant.svc.cluster.local/']) {
    await assert.rejects(readPage(url), /public address|only|user or password/, url)
  }
  assert.deepEqual(['8.8.8.8', '100.64.0.1', '172.20.0.1', '192.168.1.1', '2606:4700::1111', 'fd00::1', 'fe80::1'].map(publicAddress), [true, false, false, false, true, false, false])
  const page = text('<html><head><title>A &amp; B</title><script>evil()</script></head><body><nav>Menu</nav><main><h1>Best</h1><p>Sony&nbsp;first.</p><table><tr><td>Battery</td><td>30 h</td></tr></table></main></body></html>')
  assert.equal(page.title, 'A & B')
  assert.equal(page.text, '# Best\nSony first.\nBattery | 30 h |')
})

test('a page\'s pictures are its own photo and the photos in its main part, no logos or icons; and with made-up data no photo is fetched', async () => {
  const { text } = await import('../server/reader.mjs')
  const html = `<html><head><title>Best</title><meta content="/img/hero.jpg" property="og:image"><meta name="twitter:image" content="https://cdn.example.com/img/hero.jpg"></head>
    <body><header><img src="/logo.png" alt="Site"></header><main><h1>Best</h1><p>${'Long enough text. '.repeat(20)}</p>
    <img src="/icons/star.png" alt="star"><img src="/a.svg" alt="drawing"><img src="/tiny.jpg" width="40" height="40" alt="tiny"><img src="data:image/gif;base64,R0lGOD" alt="pixel">
    <img data-src="/img/sony.jpg" src="/img/blank.gif" alt="Sony WH-1000XM6 over-ear headphones">
    <img srcset="/img/bose-400.jpg 400w, /img/bose-800.jpg 800w, /img/bose-2400.jpg 2400w" src="/img/bose-400.jpg" alt="Bose QC Ultra">
    </main></body></html>`
  const page = text(html, true, 'https://reviews.example.com/best/headphones')
  assert.deepEqual(page.pictures, [
    { url: 'https://reviews.example.com/img/hero.jpg', alt: null },
    { url: 'https://cdn.example.com/img/hero.jpg', alt: null },
    { url: 'https://reviews.example.com/img/sony.jpg', alt: 'Sony WH-1000XM6 over-ear headphones' },
    { url: 'https://reviews.example.com/img/bose-800.jpg', alt: 'Bose QC Ultra' },
  ])

  const { picturesFor } = await import('../server/pictures.mjs')
  assert.deepEqual(await picturesFor([{ name: 'Sony', image: 'https://reviews.example.com/img/sony.jpg' }], { wiki: true }), [null], 'nothing is fetched with made-up data')
  const { findings } = await import('../server/widgets/findings.mjs')
  const found = await findings.run({ query: 'q', goal: 'pick', items: [{ name: 'Sony', image: 'https://reviews.example.com/img/sony.jpg' }], documents: [{ title: 't', summary: 's' }] })
  assert.equal(found.items[0].image, null)
  assert.equal(found.items[0].imageFrom, null)
})

test('an app that works things out itself: made on its own inputs, a script that does not run refused, and found again', { skip }, async () => {
  const kit = await ok('app_kit', { tool: 'inputs', args: { request: 'animate the derivative of x^2 sin(x)', values: { f: 'x^2 sin(x)' } } })
  assert.match(text(kit), /Kit\.math\.parse\(text\)/)
  assert.match(text(kit), /Kit\.plot\(canvas, view\)/)
  assert.match(text(kit), /An app on the tool inputs works everything out in the page/)

  const args = { request: 'animate the derivative of x^2 sin(x)', values: { f: 'x^2 sin(x)' } }
  const base = { name: 'Derivative explorer', about: 'Plots a function and its derivative, with the tangent sliding along it.', kind: 'tool', tool: 'inputs', args, tools: ['inputs'], request: 'a calculus calculator that animates derivatives' }
  const broken = await call('app_make', { ...base, html: '<canvas></canvas><script type="module">const f = (x => x * x;\nKit.connect("d", data => {})</script>' })
  assert.equal(broken.isError, true)
  assert.match(text(broken), /does not run: SyntaxError/)

  const html = '<canvas id="c" style="width:100%;height:240px"></canvas><script type="module">const { connect, math, plot } = globalThis.Kit\nconnect("d", data => { const e = math.parse(data.values.f); plot(document.getElementById("c"), { x: [-4, 4], curves: [{ f: e.f }, { f: e.derive().f }] }) })</script>'
  const made = await ok('app_make', { ...base, html })
  assert.equal(made.structuredContent.instant.kind, 'tool')
  assert.deepEqual(made.structuredContent.values, { f: 'x^2 sin(x)' })
  assert.match(text(made), /x = |f = x\^2 sin\(x\)/)
  await ok('app_save', { id: made.structuredContent.instant.id })
  const found = await ok('apps_find', { request: 'show me the derivative of cos(3x) animated' })
  assert.equal(found.structuredContent.apps[0].id, made.structuredContent.instant.id)
  const refused = await call('app_open', { id: made.structuredContent.instant.id, args: { values: { f: 'x' } } })
  assert.equal(refused.isError, true, 'inputs needs the request')
})

test('the math kit reads formulas as typed and takes exact derivatives', async () => {
  await import('../server/views/math.js')
  const M = globalThis.KitMath
  const near = (a, b) => Math.abs(a - b) < 1e-4 * (1 + Math.abs(b))
  const derivatives = {
    'x^2 sin x': '2x·sin(x) + x^2·cos(x)',
    'sin(x)/x': '(x·cos(x) - sin(x))/x^2',
    'e^(-x^2)': '-2x·e^(-x^2)',
    'ln(x^2)': '2/x',
    'sqrt(1-x^2)': '-x/√(1 - x^2)',
    'x^3 - 3x + 1': '3x^2 - 3',
    'x^(1/3)': '1/(3x^(2/3))',
    'y = 3x^4 - 2x': '12x^3 - 2',
    '(x+1)/(x-1)': '-2/(x - 1)^2',
    'sin^-1 x': '1/√(1 - x^2)',
  }
  for (const [formula, want] of Object.entries(derivatives)) {
    const e = M.parse(formula)
    const d = e.derive()
    assert.equal(d.text, want, formula)
    for (const x of [0.3, 0.7]) assert.ok(near(d.f(x), M.slope(e.f, x)), `${formula} at ${x}`)
  }
  for (const formula of ['x^x', 'tan x', 'atan(x)', '|x|', 'cos(3x)', 'e^x cos x', '2^x', 'log(x)', 'sinx cosx', 'x(x+1)']) {
    const e = M.parse(formula)
    assert.ok(near(e.derive().f(0.7), M.slope(e.f, 0.7)), formula)
  }
  // A corner has no slope; a point where the formula alone is 0/0 but both
  // sides agree has one.
  assert.ok(Number.isNaN(M.parse('|x|').derive().f(0)), '|x| at its corner')
  assert.equal(M.parse('|x|').derive().f(-2), -1)
  assert.equal(M.parse('x|x|').derive().f(0), 0)
  assert.ok(Number.isNaN(M.parse('x|x|').derive().derive().f(0)), "x|x|'' at 0")
  assert.ok(Number.isNaN(M.parse('|x^2 - 1|').derive().f(1)))
  assert.equal(M.parse('x^2').html, '<i>x</i><sup>2</sup>')
  assert.equal(M.parse('x^(1/3)').f(-8), -2)
  assert.deepEqual(M.parse('a x^2 + t').vars, ['x', 'a', 't'])
  assert.equal(M.parse('a x^2 + t').eval({ x: 2, a: 3, t: 1 }), 13)
  assert.ok(near(M.integrate(Math.sin, 0, Math.PI), 2))
  assert.deepEqual(M.zeros(M.parse('x^3 - 3x + 1').f, -3, 3).map(z => z.toFixed(4)), ['-1.8794', '0.3473', '1.5321'])
  assert.ok(Number.isNaN(M.parse('ln(x)').f(-1)))
  for (const [bad, said] of [['sin(x', /Missing \)/], ['x +', /ends too soon/], ['x $ 2', /can't read "\$"/], ['', /Type a formula/], ['sin', /needs something to work on/]]) {
    assert.throws(() => M.parse(bad), said, bad)
  }
})

test("a saved app's value names are read from its script, so new numbers land in its own fields; TAP's flight numbers find TAP's callsigns", async () => {
  const { valueNames } = await import('../server/store.mjs')
  const script = "connect('d', data => { const v = data.values ?? {}; fixed.value = v.fixed ?? 9800; price.value = v['price'] ?? 4.25; const { cost: c = 2.9, days } = data.values; draw(values.margin) })"
  assert.deepEqual(valueNames(script, { values: { fixed: 9800, costPerUnit: 1.1 } }).sort(), ['cost', 'costPerUnit', 'days', 'fixed', 'margin', 'price'])
  assert.deepEqual(valueNames('', {}), [])
  const { callsignOf } = await import('../server/widgets/flight.mjs')
  assert.equal(callsignOf('TP 244'), 'TAP244')
  assert.equal(callsignOf('tp210'), 'TAP210')
  assert.equal(callsignOf('TAP244'), 'TAP244')
  assert.equal(callsignOf('UA 857'), 'UAL857')
})

test('a made app that uses what the frame forbids is refused with what to use instead', async () => {
  const { checkFragment } = await import('../server/guide.mjs')
  const app = code => `<div id="root"></div><script type="module">${code}</script>`
  assert.match(await checkFragment(app("localStorage.setItem('stars', '3')")), /Kit\.keep\(app, key, value\)/)
  assert.match(await checkFragment(app("const w = new Worker('x.js')")), /cannot start workers/)
  assert.match(await checkFragment(app("const f = new Function('x', 'return x')")), /Kit\.math\.parse/)
  assert.match(await checkFragment(app("import * as d3 from 'https://cdn.jsdelivr.net/npm/d3@7/+esm'")), /loads nothing from the network/)
  assert.match(await checkFragment(`<script src="https://unpkg.com/x"></script>${app('1')}`), /loads nothing from the network/)
  assert.equal(await checkFragment(app("const { connect, keep, kept } = globalThis.Kit; connect('t', async () => keep(1, 'k', await kept(1, 'k')))")), null)
})

test('a second flight radar answers when OpenSky does not, and made tools keep numbers and show all of a result (AD: TOM-1, DEV-5, DEV-6, MARCUS-9)', async () => {
  const { second } = await import('../server/widgets/flight.mjs')
  const real = globalThis.fetch
  let asked = ''
  globalThis.fetch = async url => {
    asked = String(url)
    return new Response(JSON.stringify({ now: 1_795_000_000_000, ac: [{ hex: '4951c2', flight: 'TAP201  ', r: 'CS-TUB', lat: 41.2, lon: -8.6, alt_baro: 35000, alt_geom: 36210, gs: 455.3, track: 12.4, geom_rate: -64, seen_pos: 2.5 }] }), { status: 200, headers: { 'content-type': 'application/json' } })
  }
  try {
    const found = await second('TP 201')
    assert.equal(asked, 'https://api.adsb.lol/v2/callsign/TAP201', "TAP's flight number is asked by its callsign")
    assert.equal(found.found, true)
    assert.equal(found.source, 'adsb.lol')
    assert.equal(found.over, 'Portugal')
    assert.equal(found.altitudeFt, 36210)
    assert.equal(found.speedKts, 455)
    assert.equal(found.registration, 'CS-TUB')
    assert.equal(found.asOf, 1_795_000_000_000 - 2500, 'the time of the position, not of the answer')
    globalThis.fetch = async () => new Response(JSON.stringify({ now: 1, ac: [] }), { status: 200 })
    assert.equal((await second('TP 201')).found, false, 'no aircraft is not found, not an error')
  } finally {
    globalThis.fetch = real
  }
  const guide = (await import('node:fs')).readFileSync(new URL('../server/guide.mjs', import.meta.url), 'utf8')
  assert.match(guide, /past 9007199254740991/, 'pasted numbers are never rounded')
  assert.match(guide, /run the tool in your head on the sample/, 'a made tool is tried on its sample')
  assert.match(guide, /repeat\(7, minmax\(0, 1fr\)\)/, 'a week fits its seven days')
})
