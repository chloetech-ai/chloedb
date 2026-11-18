// The chat's flow with a scripted model and a real chloedb-server: a request
// no saved app fits makes one, its view may call only its own tools, a
// change is a new version and undo goes back, a saved app joins the rail,
// and a later request like it brings it back without making another.
//
//   node --test test/chat.test.mjs     (needs chloedb-server: CHLOEDB_SERVER_BIN, or a build in the checkout)
import assert from 'node:assert/strict'
import { mkdtempSync, rmSync } from 'node:fs'
import os from 'node:os'
import path from 'node:path'
import { after, before, test } from 'node:test'

process.env.WIDGETS_SIMULATE = '1'
const { start } = await import('../chat/server.mjs')
const { findServerBinary } = await import('../../../chloedb-apps/engine/strata.mjs')

const bin = process.env.CHLOEDB_SERVER_BIN || findServerBinary(new URL('../../../', import.meta.url).pathname)
const skip = bin ? false : 'no chloedb-server binary (set CHLOEDB_SERVER_BIN)'
if (bin) process.env.CHLOEDB_SERVER_BIN = bin

const FRAGMENT = (label) => `<style>.x{color:var(--ink)}</style><div id="root" class="empty">${label}</div>
<script type="module">const { connect, esc } = globalThis.Kit; const root = document.getElementById('root'); connect('t', d => { root.textContent = d.summary })</script>`

// Routes as a model would: the saved app whose first tool fits, else a new
// one; counts what it was asked to do.
function scripted() {
  const calls = { route: [], make: 0, edit: 0, reply: 0 }
  return {
    calls,
    async route(input) {
      calls.route.push(input)
      const booking = /table|dinner/i.test(input.request)
      if (!booking) return { mode: 'chat', kind: 'info', tool: null, args: {}, reply: 'Just text.', why: 'no app needed' }
      const saved = input.candidates.find(c => c.tool === 'table_search')
      const args = { where: 'North Beach, San Francisco', date: 'friday', time: '8pm', party: 4 }
      if (saved) return { mode: 'use', kind: 'action', use: saved.id, tool: 'table_search', args, why: 'your table booking app' }
      return {
        mode: 'make', kind: 'action', use: null, tool: 'table_search', args, why: 'no saved app books tables yet',
        make: { name: 'Table booking', about: 'Find a restaurant with a free table and book it.', look: 'Places with their free times; Book.', tools: ['table_search', 'table_book', 'no_such_tool'] },
      }
    },
    async make() {
      calls.make++
      return FRAGMENT('v1')
    },
    async edit({ change }) {
      calls.edit++
      return FRAGMENT(`v2: ${change}`)
    },
    async reply({ summary }) {
      calls.reply++
      return summary.slice(0, 40)
    },
  }
}

let chat
let model
let dir

before(async () => {
  if (skip) return
  dir = mkdtempSync(path.join(os.tmpdir(), 'instant-apps-test-'))
  model = scripted()
  chat = await start({ port: 0, data: dir, model, log: () => {} })
})

after(() => {
  chat?.close()
  if (dir) rmSync(dir, { recursive: true, force: true })
})

async function events(route, body) {
  const res = await fetch(chat.url + route.slice(1), { method: 'POST', body: JSON.stringify(body) })
  return (await res.text()).trim().split('\n').map(line => JSON.parse(line))
}
const post = async (route, body = {}) => (await fetch(chat.url + route.slice(1), { method: 'POST', body: JSON.stringify(body) })).json()
const rail = async () => (await (await fetch(`${chat.url}api/apps`)).json()).apps

test('the flow: make, act, change, undo, save, and recall', { skip }, async () => {
  assert.deepEqual((await rail()).map(a => a.name).sort(), ['Flight tracker', 'Shop', 'Stock chart'])

  // No saved app books tables: one is made, as a draft.
  const made = await events('/api/turn', { text: 'Book a table for 4 on Friday at 8 in North Beach' })
  assert.deepEqual(made.map(e => e.type).filter(t => t !== 'status'), ['recall', 'route', 'making', 'app', 'reply', 'done'])
  const app = made.find(e => e.type === 'app')
  assert.equal(app.how, 'made')
  assert.equal(app.app.saved, false)
  assert.deepEqual(app.app.tools, ['table_search', 'table_book'], 'a tool that does not exist is dropped')
  assert.match(app.html, /Content-Security-Policy/)
  assert.match(app.html, /globalThis\.Kit/)
  assert.equal(app.result.structuredContent.widget, 'booking')
  assert.equal(model.calls.make, 1)
  assert.ok(!(await rail()).some(a => a.id === app.app.id), 'a draft is not in the rail')

  // Its view books through its own tools, and only those.
  const booked = await post('/api/call', { app: app.app.id, name: 'table_book', arguments: { restaurant: '1', time: '20:15', party: 4 } })
  assert.equal(booked.structuredContent.confirmed, true)
  const refused = await post('/api/call', { app: app.app.id, name: 'stock_quote', arguments: { symbol: 'NVDA' } })
  assert.equal(refused.isError, true)

  // A change is a new version; undo goes back.
  const changed = await events(`/api/apps/${app.app.id}/change`, { text: 'best rated first' })
  const v2 = changed.find(e => e.type === 'app')
  assert.equal(v2.app.version, 2)
  assert.match(v2.html, /v2: best rated first/)
  const undone = await post(`/api/apps/${app.app.id}/undo`)
  assert.equal(undone.app.version, 1)
  assert.match(undone.html, />v1</)

  // Saved, it joins the rail.
  await post(`/api/apps/${app.app.id}/save`)
  assert.ok((await rail()).some(a => a.id === app.app.id && a.kind === 'action'))

  // A later request like it: ChloeDB finds it, and it opens with no app made.
  const again = await events('/api/turn', { text: 'get me a table for two tomorrow in the Mission' })
  const candidates = again.find(e => e.type === 'recall').candidates
  assert.equal(candidates[0].id, app.app.id, 'the saved app ranks first')
  const recalled = again.find(e => e.type === 'app')
  assert.equal(recalled.how, 'recalled')
  assert.equal(recalled.app.id, app.app.id)
  assert.equal(model.calls.make, 1)

  // From the rail, with what it showed last.
  const opened = await events(`/api/apps/${app.app.id}/open`, {})
  assert.equal(opened.find(e => e.type === 'app').how, 'opened')

  // A request that needs no app is just answered.
  const plain = await events('/api/turn', { text: 'what is a mutex' })
  assert.deepEqual(plain.filter(e => e.type === 'reply').map(e => e.text), ['Just text.'])
  assert.ok(!plain.some(e => e.type === 'app'))
})

test('a saved app is recalled by meaning, with no word in common', { skip }, async () => {
  assert.ok(chat.apps.meaning, 'the embedding model is there (chloedb-server fetch-model potion-retrieval-32M-int8)')
  assert.equal((await chat.apps.recall('microsoft shares this week'))[0].id, 'stock-chart')
  assert.equal((await chat.apps.recall('where is my plane'))[0].id, 'flight-tracker')
})
