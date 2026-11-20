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
const { Apps } = await import('../server/store.mjs')
const { findServerBinary } = await import('../../../chloedb-apps/engine/strata.mjs')

const bin = process.env.CHLOEDB_SERVER_BIN || findServerBinary(new URL('../../../', import.meta.url).pathname)
const skip = bin ? false : 'no chloedb-server binary (set CHLOEDB_SERVER_BIN)'
if (bin) process.env.CHLOEDB_SERVER_BIN = bin

const FRAGMENT = (label) => `<style>.x{color:var(--ink)}</style><div id="root" class="empty">${label}</div>
<script type="module">const { connect, esc } = globalThis.Kit; const root = document.getElementById('root'); connect('t', d => { root.textContent = d.summary })</script>`

// Routes as a model would: the saved app whose first tool fits, else a new
// one; counts what it was asked to do.
function scripted() {
  const calls = { route: [], make: 0, edit: 0, reply: 0, research: [] }
  return {
    calls,
    async route(input) {
      calls.route.push(input)
      const change = /^(make|undo) it/i.exec(input.request)
      if (change) {
        const app = input.screen[0]?.id
        return change[1].toLowerCase() === 'undo' ? { mode: 'undo', kind: 'action', app, why: 'take it back' } : { mode: 'change', kind: 'action', app, what: input.request.slice(8), why: 'a change' }
      }
      if (/ off my list/i.test(input.request)) return { mode: 'use', kind: 'info', use: this.pick, tool: 'findings', args: { query: 'quiet headphones' }, why: 'your kept list' }
      if (/^slowly/i.test(input.request)) {
        await new Promise(r => setTimeout(r, 400))
        return { mode: 'chat', kind: 'info', tool: null, args: {}, reply: 'Took a while.', why: 'slow' }
      }
      const invoice = /^add invoice (\S+) ([\d.]+)/i.exec(input.request)
      if (invoice) return { mode: 'use', kind: 'tool', use: input.screen[0]?.id, tool: 'inputs', args: { request: 'invoices', values: { add: { invoice_no: invoice[1], amount: Number(invoice[2]) } } }, why: 'your ledger' }
      const entry = /^add (\w+) ([\d.,]+)/i.exec(input.request)
      if (entry) return { mode: 'use', kind: 'tool', use: input.screen[0]?.id, tool: 'inputs', args: { request: 'budget', values: { month: 'Nov', add: { item: entry[1], amount: Number(entry[2].replace(',', '.')) } } }, why: 'your budget' }
      const out = /^take (\w+) out/i.exec(input.request)
      if (out) return { mode: 'use', kind: 'tool', use: input.screen[0]?.id, tool: 'inputs', args: { request: 'budget', values: { month: 'Nov', remove: { find: { item: out[1] } } } }, why: 'your budget' }
      if (/^find /i.test(input.request)) {
        const saved = input.candidates.find(c => c.id === (this.pick ?? 'search-results')) ?? { id: this.pick }
        return { mode: 'use', kind: 'info', use: saved.id, tool: 'findings', args: { query: input.request.slice(5) }, why: 'your search results' }
      }
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
    async research(input) {
      calls.research.push(input)
      return {
        query: input.query,
        documents: /nothing/.test(input.query)
          ? []
          : /camera/.test(input.query)
            ? [{ title: 'Cameras compared', url: 'https://example.com/cameras', source: 'example.com', date: '2026-11-02', summary: 'Fuji first.', quotes: ['Best colour.'], facts: { pick: 'Fuji' } }]
            : input.found
              ? [{ title: 'Sony store', url: 'https://example.com/sony-store', source: 'example.com', date: '2026-11-20', summary: 'Sony at $399.', quotes: ['$399.00'] }]
              : [{ title: 'Quiet headphones, tested', url: 'https://example.com/quiet', source: 'example.com', date: '2026-11-01', summary: 'Sony first.', quotes: ['Quietest we tried.'], facts: { pick: 'Sony' } }],
        items: /quiet/.test(input.query) ? (input.found ? [{ name: 'Sony WH-1000XM5 headphones', facts: { store_price: 399 }, says: [{ doc: 1, text: '$399.00' }] }] : [{ name: 'Sony WH-1000XM5', verdict: 'Quietest.', says: [{ doc: 1, text: 'Quietest we tried.' }] }, { name: 'Bose QuietComfort Ultra', verdict: 'Comfiest.' }, { name: 'AirPods Max', verdict: 'For iPhones.' }]) : undefined,
        notes: 'One source.',
      }
    },
    async make() {
      calls.make++
      return FRAGMENT('v1')
    },
    async edit({ change, source }) {
      calls.edit++
      calls.editSource = source
      return FRAGMENT(`v2: ${change}`)
    },
    async reply({ summary, shows, before, changed }) {
      calls.reply++
      calls.shows = shows
      calls.before = before
      calls.changed = changed
      return shows ? `On screen: ${shows.slice(0, 30)}` : summary.slice(0, 40)
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
  chat = await start({ port: 0, data: dir, model, log: () => {}, shared: false })
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
  assert.deepEqual((await rail()).map(a => a.name).sort(), ['Flight tracker', 'Search results', 'Shop', 'Stock chart'])

  // No saved app books tables: one is made, as a draft.
  const made = await events('/api/turn', { text: 'Book a table for 4 on Friday at 8 in North Beach' })
  assert.deepEqual(made.map(e => e.type).filter(t => t !== 'status'), ['recall', 'route', 'making', 'app', 'say', 'done'])
  // The answer is written from what the app shows, once the page says, and only once.
  const say = made.find(e => e.type === 'say')
  assert.deepEqual(Object.keys(say).sort(), ['token', 'type'], 'the page cannot choose what the answer is written from')
  const said = await post('/api/say', { token: say.token, shows: 'On screen now:\nTrattoria Lucca 20:15' })
  assert.equal(said.text, 'On screen: On screen now:\nTrattoria Lucca')
  assert.equal((await fetch(chat.url + 'api/say', { method: 'POST', body: JSON.stringify({ token: say.token }) })).status, 404)
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
  assert.equal(booked.acted, true, 'a booking acts, so the chat says so')
  const noted = await post('/api/note', { app: app.app.id, summary: booked.content[0].text })
  assert.match(noted.text, /Booked/)
  const refused = await post('/api/call', { app: app.app.id, name: 'stock_quote', arguments: { symbol: 'NVDA' } })
  assert.equal(refused.isError, true)

  // A change is a new version; undo goes back, and redo forward again.
  const changed = await events(`/api/apps/${app.app.id}/change`, { text: 'best rated first' })
  const v2 = changed.find(e => e.type === 'app')
  assert.equal(v2.app.version, 2)
  assert.match(v2.html, /v2: best rated first/)
  const undone = await post(`/api/apps/${app.app.id}/undo`)
  assert.equal(undone.app.version, 1)
  assert.match(undone.html, />v1</)
  assert.equal(undone.newer, true)
  const redone = await post(`/api/apps/${app.app.id}/redo`)
  assert.equal(redone.app.version, 2)
  assert.match(redone.html, /v2: best rated first/)
  assert.equal(redone.newer, false)
  // A new change after an undo replaces the version the undo left.
  await post(`/api/apps/${app.app.id}/undo`)
  const other = (await events(`/api/apps/${app.app.id}/change`, { text: 'cheapest first' })).find(e => e.type === 'app')
  assert.equal(other.app.version, 2)
  assert.match(other.html, /v2: cheapest first/)
  assert.equal((await post(`/api/apps/${app.app.id}/redo`)).app.version, 2, 'nothing newer to go to')

  // Asked in words, a change is made for real, and so is an undo.
  const screenNow = [{ id: app.app.id, version: 2, name: 'Table booking', shows: 'Places', did: [], told: 'On screen now: Lagos 3 nights, Porto 4 nights' }]
  const worded = await events('/api/turn', { text: 'make it show the price', screen: screenNow })
  const wordedApp = worded.find(e => e.type === 'app')
  assert.equal(wordedApp.how, 'changed')
  assert.equal(wordedApp.app.version, 3)
  assert.match(wordedApp.html, /v2: show the price/)
  // The answer is written from what the changed app shows, never taken for granted.
  assert.ok(!worded.some(e => e.type === 'reply'))
  const saidChange = await post('/api/say', { token: worded.find(e => e.type === 'say').token, shows: '' })
  assert.match(saidChange.text, /^Table booking was rewritten as version 3/)
  // TOM-4: the answer compares what it showed before with what it shows now.
  assert.equal(model.calls.before, 'On screen now: Lagos 3 nights, Porto 4 nights')
  const back = await events('/api/turn', { text: 'undo it', screen: [{ ...screenNow[0], version: 3 }] })
  const backApp = back.find(e => e.type === 'app')
  assert.equal(backApp.how, 'undone')
  assert.equal(backApp.app.version, 2)
  assert.equal(backApp.newer, true)
  assert.match(back.find(e => e.type === 'reply').text, /back to version 2/)

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

  // A follow-up about the app just shown fills it again: an update.
  const followed = await events('/api/turn', { text: 'a table at 7:30 instead', lastApp: { id: app.app.id, args: {} }, screen: [{ name: 'Table booking', shows: 'Trattoria Lucca free at 19:30', did: ['Booked'] }] })
  assert.equal(followed.find(e => e.type === 'app').how, 'updated')
  assert.match(model.calls.route.at(-1).screen[0].shows, /Trattoria Lucca/, 'the model reads what the widgets show')

  // From the rail, with what it showed last.
  const opened = await events(`/api/apps/${app.app.id}/open`, {})
  assert.equal(opened.find(e => e.type === 'app').how, 'opened')

  // A request that needs no app is just answered.
  const plain = await events('/api/turn', { text: 'what is a mutex' })
  assert.deepEqual(plain.filter(e => e.type === 'reply').map(e => e.text), ['Just text.'])
  assert.ok(!plain.some(e => e.type === 'app'))

  // The router sees every app on the page, with its id and what it shows.
  const many = Array.from({ length: 14 }, (_, i) => ({ id: `a${i}`, version: 1, name: `App ${i}`, folded: i > 0, shows: 'x', did: [], told: `On screen now:\n${'y'.repeat(3000)}` }))
  await events('/api/turn', { text: 'what is a mutex', screen: many })
  const seen = model.calls.route.at(-1).screen
  assert.equal(seen.length, 12)
  assert.equal(seen[1].folded, true)
  // The one used last in full, the next two in part, the rest in short.
  assert.equal(seen[0].told.length, 3000 + 'On screen now:\n'.length)
  assert.equal(seen[1].told.length, 2500)
  assert.equal(seen[5].told.length, 400)
})

test('what an app keeps is kept for one browser, and goes when the app is removed', { skip }, async () => {
  const made = (await events('/api/turn', { text: 'dinner for two tonight' })).find(e => e.type === 'app')
  const id = made.app.id
  const me = 'a'.repeat(32)
  const friend = 'b'.repeat(32)
  const kept = await post('/api/call', { app: id, me, name: 'app_keep', arguments: { key: 'stars', value: { mon: 3 } } })
  assert.equal(kept.isError, undefined)
  assert.deepEqual((await post('/api/call', { app: id, me, name: 'app_kept', arguments: { key: 'stars' } })).structuredContent.value, { mon: 3 })
  assert.equal((await post('/api/call', { app: id, me: friend, name: 'app_kept', arguments: { key: 'stars' } })).structuredContent.value, null, 'another browser sees none of it')
  // A paste of a few hundred KB is kept (DEV-4); past 1 MB it is not.
  assert.equal((await post('/api/call', { app: id, me, name: 'app_keep', arguments: { key: 'paste', value: 'x'.repeat(325_000) } })).isError, undefined)
  const big = await post('/api/call', { app: id, me, name: 'app_keep', arguments: { key: 'big', value: 'x'.repeat(1_000_001) } })
  assert.equal(big.isError, true)
  await post(`/api/apps/${id}/save`)
  assert.ok((await rail()).some(a => a.id === id))
  await post(`/api/apps/${id}/remove`)
  assert.ok(!(await rail()).some(a => a.id === id), 'gone from the rail')
  assert.equal((await fetch(`${chat.url}api/apps/${id}/open`, { method: 'POST', body: '{}' })).status, 404)
  assert.equal((await chat.apps.kept(id, 'stars', me)), null, 'what it kept went with it')
})

test('a share link opens an app with what it kept, for everyone who has the link', { skip }, async () => {
  const me = '1'.repeat(32)
  const friend = '2'.repeat(32)
  const list = await chat.apps.put({ name: 'Grocery list', about: 'What to buy.', kind: 'tool', tool: 'inputs', tools: ['inputs'], source: FRAGMENT('list'), args: { request: 'list' }, saved: true }, 'made by hand')
  await post('/api/call', { app: list.id, me, name: 'app_keep', arguments: { key: 'items', value: ['milk', 'eggs'] } })
  const { token } = await post('/api/share', { app: list.id, me })
  assert.match(token, /^[0-9a-f]{32}$/)
  const link = `s-${token}`
  assert.deepEqual((await post('/api/call', { app: list.id, me: link, name: 'app_kept', arguments: { all: true } })).structuredContent.values, { items: ['milk', 'eggs'] }, 'the link starts with what it kept')
  // A friend with the link changes it, and the one who shared it sees that.
  await post('/api/call', { app: list.id, me: link, name: 'app_keep', arguments: { key: 'items', value: ['milk', 'eggs', 'bread'] } })
  assert.deepEqual((await post('/api/call', { app: list.id, me: link, name: 'app_kept', arguments: { key: 'items' } })).structuredContent.value, ['milk', 'eggs', 'bread'])
  assert.deepEqual((await post('/api/share', { app: list.id, me: link })).token, token, 'sharing it again gives the same link')
  assert.deepEqual((await post('/api/call', { app: list.id, me: friend, name: 'app_kept', arguments: { all: true } })).structuredContent.values, {}, 'without the link, a browser has none of it')
  assert.equal((await fetch(`${chat.url}api/share`, { method: 'POST', body: JSON.stringify({ app: 'no-such-app', me }) })).status, 404)
})

test('a conversation is kept for its browser, and each app in it opens again as it was', { skip }, async () => {
  const talk = 'c'.repeat(32)
  const first = await events('/api/turn', { text: 'find reviews of quiet headphones', talk })
  const answer = first.find(e => e.type === 'app')
  assert.ok(answer.shown, 'an app shown in a conversation is recorded')
  await post('/api/talk', { talk, turn: first.find(e => e.type === 'turn').id, request: 'find reviews of quiet headphones', reply: 'Sony first.', shown: [{ id: answer.shown, app: answer.app.id, name: answer.app.name, kind: 'info', how: answer.how, shows: 'x' }] })
  const secondTurn = await events('/api/turn', { text: 'find good cameras', talk })
  const second = secondTurn.find(e => e.type === 'app')
  assert.notEqual(second.shown, answer.shown)
  // The page's record fills the turn the server kept as it began: one turn, not two.
  await post('/api/talk', { talk, turn: secondTurn.find(e => e.type === 'turn').id, request: 'find good cameras', reply: '', shown: [] })

  const { turns } = await (await fetch(`${chat.url}api/talk?talk=${talk}`)).json()
  assert.deepEqual(turns.map(t => t.request), ['find reviews of quiet headphones', 'find good cameras'])
  assert.equal(turns[0].shown[0].id, answer.shown)
  assert.deepEqual((await (await fetch(`${chat.url}api/talk?talk=${'d'.repeat(32)}`)).json()).turns, [], 'another browser reads none of it')

  // The first answer opens again with what it found then, though the same
  // app showed another search since.
  const again = await (await fetch(`${chat.url}api/shown/${answer.shown}?talk=${talk}`)).json()
  assert.equal(again.app.id, 'search-results')
  assert.equal(again.args.documents[0].title, 'Quiet headphones, tested')
  assert.equal((await fetch(`${chat.url}api/shown/${answer.shown}?talk=${'d'.repeat(32)}`)).status, 404, 'only in its own conversation')

  // Kept as its own app, it keeps those findings when searches follow, and
  // the notes on it come with it (S: PRIYA-2).
  const noter = '7'.repeat(32)
  const notesKey = `notes ${String(again.args.query ?? '').slice(0, 70)}`
  await post('/api/call', { app: 'search-results', me: noter, name: 'app_keep', arguments: { key: notesKey, value: [{ id: 'n1', on: 'Quiet headphones', text: 'too heavy' }] } })
  const keptApp = (await post('/api/apps/search-results/keep-answer', { talk, shown: answer.shown, name: 'Quiet headphones · Nov 19', request: 'find reviews of quiet headphones', me: noter })).app
  assert.equal((await post('/api/call', { app: keptApp.id, me: noter, name: 'app_kept', arguments: { key: notesKey } })).structuredContent.value[0].text, 'too heavy')
  // Shared, a person's link adds a No under their own name.
  const kshare = await post('/api/share', { app: keptApp.id, me: noter })
  const sam = (await post('/api/share', { app: keptApp.id, me: `s-${kshare.token}`, person: { name: 'Sam', role: 'helper' } })).people.at(-1)
  const had = (await post('/api/call', { app: keptApp.id, me: `s-${sam.token}`, name: 'app_kept', arguments: { key: notesKey } })).structuredContent.value
  assert.equal((await post('/api/call', { app: keptApp.id, me: `s-${sam.token}`, name: 'app_keep', arguments: { key: notesKey, value: [...had, { id: 'n2', on: 'Quiet headphones', text: 'No', no: true, by: 'helper' }] } })).isError, undefined)
  const both = (await post('/api/call', { app: keptApp.id, me: `s-${kshare.token}`, name: 'app_kept', arguments: { key: notesKey } })).structuredContent.value
  assert.deepEqual(both.map(n => [n.text, n.by]), [['too heavy', undefined], ['No', 'Sam']])
  assert.match(keptApp.id, /^answer-/)
  assert.equal(keptApp.saved, true)
  assert.ok((await rail()).some(a => a.id === keptApp.id))
  await chat.apps.used(keptApp.id, 'find other things', { query: 'other', documents: [] }, 'recalled')
  assert.equal((await chat.apps.get(keptApp.id)).args.documents[0].title, 'Quiet headphones, tested')
  const opened = (await events(`/api/apps/${keptApp.id}/open`, { talk })).find(e => e.type === 'app')
  assert.equal(opened.result.structuredContent.documents[0].title, 'Quiet headphones, tested')
  // Asked for again, it opens with what it found then, and no search.
  const searches = model.calls.research.length
  model.pick = keptApp.id
  const asked = (await events('/api/turn', { text: 'find that headphones answer I kept', talk })).find(e => e.type === 'app')
  delete model.pick
  assert.equal(asked.app.id, keptApp.id)
  assert.equal(asked.result.structuredContent.documents[0].title, 'Quiet headphones, tested')
  assert.equal(model.calls.research.length, searches)
  // A new question the router took the kept answer for is a new search (LENA-2).
  model.pick = keptApp.id
  const other = (await events('/api/turn', { text: 'find why General Mills stock fell this year', talk })).find(e => e.type === 'app')
  delete model.pick
  assert.equal(other.app.id, 'search-results')
  assert.equal(model.calls.research.length, searches + 1)
})

test('an app gets all it kept with its data, and an entry asked for in words is sent once, with an id', { skip }, async () => {
  const me = 'e'.repeat(32)
  const budget = await chat.apps.put({ name: 'Budget tracker', about: 'Expenses by month.', kind: 'tool', tool: 'inputs', tools: ['inputs'], source: FRAGMENT('budget'), args: { request: 'budget', values: { month: 'Nov' } }, saved: true }, 'made by hand')
  await post('/api/call', { app: budget.id, me, name: 'app_keep', arguments: { key: 'rows', value: [{ item: 'rent', amount: 600 }] } })
  await post('/api/call', { app: budget.id, me, name: 'app_keep', arguments: { key: 'month', value: 'Oct' } })
  const all = await post('/api/call', { app: budget.id, me, name: 'app_kept', arguments: { all: true } })
  assert.deepEqual(all.structuredContent.values, { rows: [{ item: 'rent', amount: 600 }], month: 'Oct' })
  assert.deepEqual((await post('/api/call', { app: budget.id, me: 'f'.repeat(32), name: 'app_kept', arguments: { all: true } })).structuredContent.values, {}, 'another browser has none')

  const talk = '7'.repeat(32)
  const added = (await events('/api/turn', { text: 'add lunch 4,80', talk, screen: [{ id: budget.id, version: 1, name: 'Budget tracker', shows: '', did: [] }] })).find(e => e.type === 'app')
  assert.equal(added.app.id, budget.id)
  assert.ok(!added.result.isError, 'the app’s own inputs take entries')
  assert.match(added.result.content[0].text, /add = \[\{"id"/)
  const [entry] = added.args.values.add
  assert.equal(entry.item, 'lunch')
  assert.equal(entry.amount, 4.8)
  assert.match(entry.id, /^[0-9a-f]{10}$/, 'each entry has an id, so the app adds it once')
  assert.deepEqual((await chat.apps.get(budget.id)).args.values, { month: 'Nov' }, 'the app opens next time without the entry')
  const again = await (await fetch(`${chat.url}api/shown/${added.shown}?talk=${talk}`)).json()
  assert.deepEqual(again.args.values, { month: 'Nov' }, 'and so does that place in the conversation: the entry is in what it kept')

  // A take-out asked once is sent once, and never again when the app opens (LENA-1).
  const out = (await events('/api/turn', { text: 'take rent out', talk, screen: [{ id: budget.id, version: 1, name: 'Budget tracker', shows: '', did: [] }] })).find(e => e.type === 'app')
  assert.deepEqual(out.args.values.remove, { find: { item: 'rent' } }, 'the app gets the take-out this time')
  assert.deepEqual((await chat.apps.get(budget.id)).args.values, { month: 'Nov' }, 'the app opens next time without it')
  const shownOut = await (await fetch(`${chat.url}api/shown/${out.shown}?talk=${talk}`)).json()
  assert.deepEqual(shownOut.args.values, { month: 'Nov' }, 'and that place in the conversation opens without it too')
})

test('a change is made to what that widget shows, and a web answer changes as a copy of its own', { skip }, async () => {
  const talk = '9'.repeat(32)
  const first = (await events('/api/turn', { text: 'find reviews of quiet headphones', talk })).find(e => e.type === 'app')
  await events('/api/turn', { text: 'find good cameras', talk })
  const before = await chat.apps.get('search-results')
  const changed = (await events('/api/apps/search-results/change', { text: 'bigger text', shown: first.shown, talk })).find(e => e.type === 'app')
  assert.equal(changed.copied, true)
  assert.match(changed.app.id, /^answer-/)
  assert.match(changed.app.name, /^Reviews of quiet headphones · /)
  assert.equal(changed.args.documents[0].title, 'Quiet headphones, tested', 'that answer’s own findings, not the latest search’s')
  assert.match(changed.html, /v2: bigger text/)
  // The change starts from the answer's own page, so what it does stays (PRIYA-1).
  assert.doesNotMatch(model.calls.editSource, /a page made by hand/)
  assert.match(model.calls.editSource, /Kit\.connect|connect\(/)
  const after = await chat.apps.get('search-results')
  assert.equal(after.version, before.version, 'Search results itself is unchanged')
  assert.equal(after.source, before.source)
  // That place in the conversation opens again as the changed copy.
  const again = await (await fetch(`${chat.url}api/shown/${first.shown}?talk=${talk}`)).json()
  assert.equal(again.app.id, changed.app.id)
  // Another answer kept under the same name gets an id of its own.
  const twin = await chat.apps.keepAnswer('search-results', { name: changed.app.name, request: 'x', args: first.args })
  assert.notEqual(twin.id, changed.app.id)
  // What the person changed in it holds when a search follows it up (INES-2).
  await events(`/api/apps/${changed.app.id}/change`, { text: 'take the Sony out, it is discontinued', shown: first.shown, talk })
  assert.deepEqual(await chat.apps.changesOf(changed.app.id), ['bigger text', 'take the Sony out, it is discontinued'])
  await events('/api/turn', { text: 'find which of these is lightest', talk, lastApp: { id: changed.app.id, args: changed.args } })
  assert.deepEqual(model.calls.research.at(-1).changes, ['bigger text', 'take the Sony out, it is discontinued'])
})

test('a saved app is recalled by meaning, with no word in common', { skip }, async () => {
  assert.ok(chat.apps.meaning, 'the embedding model is there (chloedb-server fetch-model potion-retrieval-32M-int8)')
  assert.equal((await chat.apps.recall('microsoft shares this week'))[0].id, 'stock-chart')
  assert.equal((await chat.apps.recall('where is my plane'))[0].id, 'flight-tracker')
})

test('a store opened again keeps its apps, and recall by meaning', { skip }, async () => {
  const again = mkdtempSync(path.join(os.tmpdir(), 'instant-apps-reopen-'))
  try {
    const first = await Apps.open({ dir: again, shared: false })
    const app = await first.put({ name: 'Table booking', about: 'Find a restaurant with a free table and book it.', kind: 'action', tool: 'table_search', tools: ['table_search', 'table_book'], source: 'view:stock', args: {}, examples: 'book a table', saved: true }, 'made')
    first.close()
    await new Promise(resolve => setTimeout(resolve, 500))
    const second = await Apps.open({ dir: again, shared: false })
    try {
      assert.ok((await second.saved()).some(a => a.id === app.id), 'the saved app is still there')
      assert.equal(second.meaning, first.meaning, 'it still recalls by meaning')
      assert.equal((await second.recall('reserve dinner for two'))[0].id, app.id)
    } finally {
      second.close()
    }
  } finally {
    rmSync(again, { recursive: true, force: true })
  }
})

test('every process with one folder of apps shares one chloedb-server, started once', { skip }, async () => {
  const dir = mkdtempSync(path.join(os.tmpdir(), 'instant-apps-shared-'))
  try {
    // Two at once, as two Claude Code sessions starting the MCP server.
    const [a, b] = await Promise.all([Apps.open({ dir }), Apps.open({ dir })])
    assert.equal(a.db.url, b.db.url)
    const app = await a.put({ name: 'Product comparison', about: 'Puts products side by side.', kind: 'info', tool: 'shop_search', tools: ['shop_search'], source: '<div></div>', args: {}, saved: true }, 'made')
    assert.ok((await b.saved()).some(x => x.id === app.id), 'one sees what the other wrote')
    a.close()
    b.close()
    // Closing them leaves it running for the next one.
    const c = await Apps.open({ dir })
    assert.equal(c.db.url, a.db.url)
    c.close()
  } finally {
    Apps.stop(dir)
    rmSync(dir, { recursive: true, force: true })
  }
})

// On a site others use: Claude's spend is capped and kept across restarts,
// and one visitor's requests in an hour are limited.
test('the spend cap refuses calls past it, and remembers what was spent', async () => {
  const { claude, Budget } = await import('../chat/model.mjs')
  const tmp = mkdtempSync(path.join(os.tmpdir(), 'instant-apps-budget-'))
  try {
    // A claude that answers every call for 4 cents.
    const fake = path.join(tmp, 'claude')
    ;(await import('node:fs')).writeFileSync(fake, '#!/bin/sh\ncat > /dev/null\necho \'{"result":"Hi.","total_cost_usd":0.04}\'\n', { mode: 0o755 })
    const file = path.join(tmp, 'spend.json')
    const model = claude({ bin: fake, budget: new Budget({ daily: 0.05, file }) })
    assert.equal(await model.reply({ request: 'hi', summary: 'x' }), 'Hi.')
    assert.equal(await model.reply({ request: 'hi', summary: 'x' }), 'Hi.')
    await assert.rejects(model.reply({ request: 'hi', summary: 'x' }), /budget for Claude/)
    // Started again, it still knows.
    const again = claude({ bin: fake, budget: new Budget({ daily: 0.05, file }) })
    await assert.rejects(again.reply({ request: 'hi', summary: 'x' }), /budget for Claude/)
    assert.ok(Math.abs(again.budget.status().today - 0.08) < 1e-9)
  } finally {
    rmSync(tmp, { recursive: true, force: true })
  }
})

test('a web search that brings back no sources is asked for once more', async () => {
  const { claude, Budget } = await import('../chat/model.mjs')
  const tmp = mkdtempSync(path.join(os.tmpdir(), 'instant-apps-research-'))
  try {
    // A claude whose first search fills the shape with nothing, as one did
    // on a Mac ("dummy"), and whose second finds a page.
    const fake = path.join(tmp, 'claude')
    const none = JSON.stringify({ structured_output: { query: 'dummy', goal: 'answer', answer: { text: 'dummy' }, items: [], documents: [] }, total_cost_usd: 0.01 })
    const some = JSON.stringify({ structured_output: { query: '42 inch TVs at lider.cl', goal: 'pick', answer: { text: 'The LG.' }, items: [{ name: 'LG 43LR6000' }], documents: [{ title: 'Televisores', url: 'https://www.lider.cl/tv', summary: 'TVs.' }] }, total_cost_usd: 0.01 })
    ;(await import('node:fs')).writeFileSync(fake, `#!/bin/sh\ncat > /dev/null\nif [ -f "${tmp}/once" ]; then echo '${some}'; else touch "${tmp}/once"; echo '${none}'; fi\n`, { mode: 0o755 })
    const model = claude({ bin: fake, budget: new Budget({ daily: 1 }) })
    const said = []
    const found = await model.research({ request: 'Compare 42” tvs from líder.cl', query: 'Compare 42” tvs from líder.cl', history: [] }, line => said.push(line))
    assert.equal(found.documents.length, 1)
    assert.equal(found.query, '42 inch TVs at lider.cl')
    assert.match(said[0], /0 source\(s\), 0 item\(s\), query "dummy"/)
    assert.match(said[1], /^research again .* 1 source\(s\)/)
  } finally {
    rmSync(tmp, { recursive: true, force: true })
  }
})

test('one visitor’s requests in an hour are limited', { skip }, async () => {
  const dir = mkdtempSync(path.join(os.tmpdir(), 'instant-apps-limit-'))
  const chat = await start({ port: 0, data: dir, model: scripted(), log: () => {}, shared: false, perHour: 1 })
  try {
    const turn = async () => (await fetch(new URL('/api/turn', chat.url), { method: 'POST', headers: { 'x-forwarded-for': '203.0.113.7' }, body: JSON.stringify({ text: 'hello' }) })).text()
    assert.match(await turn(), /"Just text\."/)
    assert.match(await turn(), /asked a lot this hour/)
    assert.equal((await fetch(new URL('/healthz', chat.url))).status, 200)
  } finally {
    chat.close()
    rmSync(dir, { recursive: true, force: true })
  }
})

test('a request to find things searches the web first, and the Search results app shows what was found', { skip }, async () => {
  const usesBefore = (await rail()).find(a => a.id === 'search-results').uses
  const found = await events('/api/turn', { text: 'find reviews of quiet headphones' })
  assert.ok(found.some(e => e.type === 'status' && e.text === 'Searching the web'))
  assert.deepEqual(model.calls.research.at(-1).query, 'reviews of quiet headphones')
  const app = found.find(e => e.type === 'app')
  assert.equal(app.app.id, 'search-results')
  assert.equal(app.how, 'recalled')
  const data = app.result.structuredContent
  assert.equal(data.widget, 'findings')
  assert.equal(data.documents[0].url, 'https://example.com/quiet')
  assert.deepEqual(data.facts, ['pick'])
  assert.match(app.html, /Found by Claude/)
  // Kept with the app, so it shows them again when opened from the rail.
  assert.equal((await rail()).find(a => a.id === 'search-results').uses, usesBefore + 1)

  // A follow-up starts from what it found.
  await events('/api/turn', { text: 'find which of those is best for flights', lastApp: { id: 'search-results', args: app.args } })
  assert.equal(model.calls.research.at(-1).found.documents[0].title, 'Quiet headphones, tested')

  // Nothing found is said in words, with no app.
  const none = await events('/api/turn', { text: 'find nothing at all' })
  assert.ok(!none.some(e => e.type === 'app'))
  assert.match(none.find(e => e.type === 'reply').text, /couldn’t find anything on the web for “find nothing at all”/)
})

test('a helper link adds and ticks but changes and takes out nothing, and a view link only looks', { skip }, async () => {
  const me = '3'.repeat(32)
  const list = await chat.apps.put({ name: 'Shared list', about: 'What to buy.', kind: 'tool', tool: 'inputs', tools: ['inputs'], source: FRAGMENT('list'), args: { request: 'list' }, saved: true }, 'made by hand')
  const items = [{ id: 'a', name: 'milk', done: false }, { id: 'b', name: 'eggs', done: false }]
  await post('/api/call', { app: list.id, me, name: 'app_keep', arguments: { key: 'items', value: items } })
  const share = await post('/api/share', { app: list.id, me })
  assert.match(share.helper, /^[0-9a-f]{32}$/)
  assert.match(share.viewer, /^[0-9a-f]{32}$/)
  const keep = (who, value) => post('/api/call', { app: list.id, me: `s-${who}`, name: 'app_keep', arguments: { key: 'items', value } })
  const read = async who => (await post('/api/call', { app: list.id, me: `s-${who}`, name: 'app_kept', arguments: { all: true } })).structuredContent.values
  assert.deepEqual(await read(share.viewer), { items }, 'every link sees the same, and none of the links themselves')
  // MAYA-12: her husband ticks milk and adds bread.
  const ticked = [{ ...items[0], done: true }, items[1], { id: 'c', name: 'bread', done: false }]
  assert.equal((await keep(share.helper, ticked)).isError, undefined)
  assert.deepEqual((await read(share.token)).items, ticked)
  // But can't take eggs out, or clear the list.
  const out = await keep(share.helper, [ticked[0], ticked[2]])
  assert.equal(out.isError, true)
  assert.match(out.content[0].text, /can add and tick, but not change or take out what is there/)
  assert.equal((await keep(share.helper, [])).isError, true)
  // Nor change what is there (INES-2, LENA-4).
  assert.equal((await keep(share.helper, [{ ...ticked[0], name: 'oat milk' }, ticked[1], ticked[2]])).isError, true)
  assert.deepEqual((await read(share.token)).items, ticked, 'nothing was taken out')
  // A view link changes nothing; the full link can take things out.
  assert.match((await keep(share.viewer, ticked)).content[0].text, /can only look/)
  assert.equal((await keep(share.token, [ticked[0]])).isError, undefined)
  // Only a full link shares further, and sharing again gives the same links.
  assert.equal((await fetch(`${chat.url}api/share`, { method: 'POST', body: JSON.stringify({ app: list.id, me: `s-${share.helper}` }) })).status, 403)
  assert.deepEqual(await post('/api/share', { app: list.id, me: `s-${share.token}` }), share)
  // An app can't read or write the share's own records.
  assert.equal((await post('/api/call', { app: list.id, me: `s-${share.token}`, name: 'app_kept', arguments: { key: '-links' } })).isError, true)
})

test('a booking stays with the app that made it, and can be cancelled (TOM-5)', { skip }, async () => {
  const me = '4'.repeat(32)
  const app = await chat.apps.put({ name: 'Table booking', about: 'Book a table.', kind: 'action', tool: 'table_search', tools: ['table_search', 'table_book'], source: FRAGMENT('book'), args: { where: 'Lisbon' }, saved: true }, 'made by hand')
  const booked = await post('/api/call', { app: app.id, me, name: 'table_book', arguments: { restaurant: '7', name: 'Pier 9', date: '2027-04-12', time: '19:30', party: 2 } })
  assert.equal(booked.acted, true)
  const code = booked.structuredContent.code
  const kept = async () => (await post('/api/call', { app: app.id, me, name: 'app_kept', arguments: { all: true } })).structuredContent.values.bookings
  assert.deepEqual((await kept()).map(b => [b.code, b.name, b.party, b.cancelled ?? null]), [[code, 'Pier 9', 2, null]])
  const cancelled = await post('/api/call', { app: app.id, me, name: 'table_cancel', arguments: { code } })
  assert.equal(cancelled.isError, undefined, 'an app that books can cancel')
  assert.ok((await kept())[0].cancelled, 'and the booking says it was cancelled')
  assert.equal((await post('/api/call', { app: app.id, me: '5'.repeat(32), name: 'app_kept', arguments: { all: true } })).structuredContent.values.bookings, undefined, 'another browser has none')
})

test('a helper or view link changes no app, a guest has no chat, and sharing can be stopped (round 4)', { skip }, async () => {
  const me = '6'.repeat(32)
  const app = await chat.apps.put({ name: 'Week plan', about: 'Dinners for the week.', kind: 'tool', tool: 'inputs', tools: ['inputs'], source: FRAGMENT('plan'), args: { request: 'plan' }, saved: true }, 'made by hand')
  await post('/api/call', { app: app.id, me, name: 'app_keep', arguments: { key: 'days', value: [{ id: 'mon', dish: 'burgers' }] } })
  const share = await post('/api/share', { app: app.id, me })
  const status = async (route, body) => (await fetch(chat.url + route.slice(1), { method: 'POST', body: JSON.stringify(body) })).status
  // MAYA-3, LENA-1: neither the helper's nor the viewer's phone removes it, undoes it or saves it.
  assert.equal(await status(`/api/apps/${app.id}/remove`, { links: { [app.id]: share.helper } }), 403)
  assert.equal(await status(`/api/apps/${app.id}/unsave`, { links: { [app.id]: share.viewer } }), 403)
  assert.equal(await status(`/api/apps/${app.id}/remove`, { guest: true }), 403, 'nor any app, from a guest page')
  assert.ok((await chat.apps.get(app.id)).saved, 'still in Your apps')
  // INES-1, MAYA-2: a page opened only by links has no chat.
  const events = await (await fetch(`${chat.url}api/turn`, { method: 'POST', body: JSON.stringify({ text: 'what did I spend on coffee', guest: true, links: { [app.id]: share.helper } }) })).text()
  assert.match(events, /only what was shared with you/)
  // MAYA-4, MARCUS-2: the chat is told an app is shared, so it never says it can't reach his phone.
  await (await fetch(`${chat.url}api/turn`, { method: 'POST', body: JSON.stringify({ text: 'will he see it on his phone?', links: { [app.id]: share.token }, screen: [{ id: app.id, name: 'Week plan', version: 1, shows: 'plan' }, { id: 'other', name: 'Other', version: 1 }] }) })).text()
  const told = model.calls.route.at(-1).screen
  assert.equal(told[0].shared, true)
  assert.equal(told[1].shared, undefined, 'an app not shared is not marked')
  // MARCUS-3: a filter is one person's own, even in a shared app.
  const browser = '7'.repeat(32)
  await post('/api/call', { app: app.id, me: `s-${share.helper}`, self: browser, name: 'app_keep', arguments: { key: 'view:crew', value: 'A' } })
  assert.equal((await post('/api/call', { app: app.id, me: `s-${share.token}`, self: me, name: 'app_kept', arguments: { all: true } })).structuredContent.values['view:crew'], undefined, 'the owner does not get the helper’s filter')
  assert.equal((await post('/api/call', { app: app.id, me: `s-${share.helper}`, self: browser, name: 'app_kept', arguments: { all: true } })).structuredContent.values['view:crew'], 'A')
  // DEV-2: stopping it gives what it kept back to the browser that stops it, and every link opens nothing.
  assert.equal((await post('/api/unshare', { app: app.id, me: `s-${share.helper}`, self: browser })).error, 'Only whoever shared it can stop sharing it.')
  assert.equal((await post('/api/unshare', { app: app.id, me: `s-${share.token}`, self: me })).stopped, true)
  assert.deepEqual((await post('/api/call', { app: app.id, me, name: 'app_kept', arguments: { all: true } })).structuredContent.values.days, [{ id: 'mon', dish: 'burgers' }])
  for (const token of [share.token, share.helper, share.viewer]) {
    const out = await post('/api/call', { app: app.id, me: `s-${token}`, name: 'app_kept', arguments: { all: true } })
    assert.match(out.content[0].text, /stopped/, 'a stopped link reads nothing')
    assert.match((await post('/api/call', { app: app.id, me: `s-${token}`, name: 'app_keep', arguments: { key: 'days', value: [] } })).content[0].text, /stopped/)
  }
})

test('a link for one person says who added what, can show only their part, and stops alone (INES-5, MARCUS-4)', { skip }, async () => {
  const me = '8'.repeat(32)
  const app = await chat.apps.put({ name: 'Crew week', about: 'Jobs by crew.', kind: 'tool', tool: 'inputs', tools: ['inputs'], source: FRAGMENT('week'), args: { request: 'week' }, saved: true }, 'made by hand')
  const jobs = [{ id: 'j1', client: 'Smith', crew: 'A', done: false }, { id: 'j2', client: 'Lee', crew: 'B', done: false }, { id: 'j3', client: 'Patel', crew: 'A', done: false }]
  await post('/api/call', { app: app.id, me, name: 'app_keep', arguments: { key: 'jobs', value: jobs } })
  const share = await post('/api/share', { app: app.id, me })
  const owner = `s-${share.token}`
  const joana = (await post('/api/share', { app: app.id, me: owner, person: { name: 'Joana', role: 'helper' } })).people.at(-1)
  const dave = (await post('/api/share', { app: app.id, me: owner, person: { name: 'Dave', role: 'helper', only: { field: 'crew', value: 'A' } } })).people.at(-1)
  assert.equal(dave.only.value, 'A')
  assert.equal((await post('/api/share', { app: app.id, me: `s-${joana.token}`, person: { name: 'X' } })).error, 'Only whoever shared it can share it further.')
  // INES-5: what Joana adds says it was her.
  await post('/api/call', { app: app.id, me: `s-${joana.token}`, name: 'app_keep', arguments: { key: 'jobs', value: [...jobs, { id: 'j4', client: 'Gomes', crew: 'B', done: false }] } })
  const read = async who => (await post('/api/call', { app: app.id, me: who, name: 'app_kept', arguments: { all: true } })).structuredContent.values.jobs
  assert.equal((await read(owner)).find(j => j.id === 'j4').by, 'Joana')
  assert.equal((await read(owner)).find(j => j.id === 'j1').by, undefined, 'what was there is as it was')
  // MARCUS-4: Dave sees crew A's jobs only, ticks one and adds one; crew B's are as they were.
  const his = await read(`s-${dave.token}`)
  assert.deepEqual(his.map(j => j.id), ['j1', 'j3'])
  const out = await post('/api/call', { app: app.id, me: `s-${dave.token}`, name: 'app_keep', arguments: { key: 'jobs', value: [{ ...his[0], done: true }, his[1], { id: 'j5', client: 'Kim', done: false }] } })
  assert.ok(!out.isError, out.content?.[0]?.text)
  const all = await read(owner)
  assert.deepEqual(all.map(j => j.id), ['j1', 'j2', 'j3', 'j4', 'j5'], 'nothing of crew B taken out, in its place')
  assert.equal(all.find(j => j.id === 'j1').done, true)
  assert.deepEqual([all.find(j => j.id === 'j5').crew, all.find(j => j.id === 'j5').by], ['A', 'Dave'], 'his new job is crew A, by him')
  // His link still can't take out or change crew A's jobs.
  assert.ok((await post('/api/call', { app: app.id, me: `s-${dave.token}`, name: 'app_keep', arguments: { key: 'jobs', value: [] } })).isError)
  // Stopping Joana's link leaves Dave's and the others working.
  const stopped = await post('/api/unshare', { app: app.id, me: owner, self: me, person: joana.token })
  assert.deepEqual(stopped.people.map(p => p.name), ['Dave'])
  assert.match((await post('/api/call', { app: app.id, me: `s-${joana.token}`, name: 'app_kept', arguments: { all: true } })).content[0].text, /stopped/)
  assert.equal((await read(`s-${dave.token}`)).length, 3)
  assert.equal((await read(`s-${share.helper}`)).length, 5)
})

test('a helper types a whole word, one key at a time, and still can’t change what the owner kept (PRIYA-1, TOM-1, INES-4)', { skip }, async () => {
  const me = 'a'.repeat(32)
  const app = await chat.apps.put({ name: 'Notes list', about: 'Rooms.', kind: 'tool', tool: 'inputs', tools: ['inputs'], source: FRAGMENT('notes'), args: { request: 'notes' }, saved: true }, 'made by hand')
  const rooms = [{ id: 'r1', name: 'Hall', note: '' }, { id: 'r2', name: 'Kitchen', note: 'old' }]
  await post('/api/call', { app: app.id, me, name: 'app_keep', arguments: { key: 'rooms', value: rooms } })
  const share = await post('/api/share', { app: app.id, me })
  const keep = value => post('/api/call', { app: app.id, me: `s-${share.helper}`, name: 'app_keep', arguments: { key: 'rooms', value } })
  const read = async () => (await post('/api/call', { app: app.id, me: `s-${share.token}`, name: 'app_kept', arguments: { key: 'rooms' } })).structuredContent.value
  for (const text of ['N', 'No', 'No h']) {
    const out = await keep([{ ...rooms[0], note: text }, rooms[1]])
    assert.equal(out.isError, undefined, `"${text}" is kept`)
    assert.equal((await read())[0].note, text)
  }
  // What was there stays the owner's.
  assert.equal((await keep([rooms[0], { ...rooms[1], note: 'new' }])).isError, true, 'the owner’s note is not the helper’s to change')
  // And an entry the helper added, they go on editing.
  const withNew = [{ ...rooms[0], note: 'No h' }, rooms[1], { id: 'r3', name: 'Loft' }]
  assert.equal((await keep(withNew)).isError, undefined)
  assert.equal((await keep([withNew[0], withNew[1], { id: 'r3', name: 'Loft', note: 'x' }])).isError, undefined)
  assert.equal((await keep([withNew[0], withNew[1], { id: 'r3', name: 'Loft 2', note: 'xy' }])).isError, undefined)
})

test('what an app held before comes back: a keep that drops entries is undone from its history (LENA-1, INES-1)', { skip }, async () => {
  const me = 'b'.repeat(32)
  const app = await chat.apps.put({ name: 'Visits log', about: 'Visits.', kind: 'tool', tool: 'inputs', tools: ['inputs'], source: FRAGMENT('visits'), args: { request: 'visits' }, saved: true }, 'made by hand')
  const three = [{ id: 'a', t: 1 }, { id: 'b', t: 2 }, { id: 'c', t: 3 }]
  const call = (name, args, who = me) => post('/api/call', { app: app.id, me: who, name, arguments: args })
  await call('app_keep', { key: 'visits', value: three })
  await call('app_keep', { key: 'visits', value: [three[0]] }) // the wipe: 3 became 1
  assert.equal((await call('app_kept', { key: 'visits' })).structuredContent.value.length, 1)
  const hist = await post('/api/history', { app: app.id, me, self: me })
  assert.ok(hist.moments.length >= 1, 'a moment before the wipe is listed')
  assert.match(hist.now, /1 in visits/)
  assert.match(hist.moments[0].says, /3 in visits/)
  const done = await post('/api/restore', { app: app.id, me, self: me, at: hist.moments[0].at })
  assert.deepEqual(done.restored, ['visits'])
  assert.deepEqual((await call('app_kept', { key: 'visits' })).structuredContent.value, three, 'all three are back')
  // Only the owner or a full link brings things back; a helper can't.
  const share = await post('/api/share', { app: app.id, me })
  assert.equal((await fetch(`${chat.url}api/restore`, { method: 'POST', body: JSON.stringify({ app: app.id, me: `s-${share.helper}`, at: hist.moments[0].at }) })).status, 403)
  assert.ok((await post('/api/history', { app: app.id, me: `s-${share.token}` })).moments, 'a full link may')
  // Making it private forgets what it held before.
  assert.equal((await post('/api/history', { app: app.id, me, self: me, forget: true })).forgot, true)
  assert.equal((await post('/api/history', { app: app.id, me, self: me })).moments.length, 0)
})

test('a link can hide fields such as prices, and a stopped link opens nothing (MARCUS-1, PRIYA-2, TOM-7)', { skip }, async () => {
  const me = 'c'.repeat(32)
  const app = await chat.apps.put({ name: 'Quotes', about: 'Jobs and prices.', kind: 'tool', tool: 'inputs', tools: ['inputs'], source: FRAGMENT('quotes'), args: { request: 'quotes' }, saved: true }, 'made by hand')
  const jobs = [{ id: 'j1', client: 'Smith', price: 900 }, { id: 'j2', client: 'Lee', price: 450 }]
  await post('/api/call', { app: app.id, me, name: 'app_keep', arguments: { key: 'jobs', value: jobs } })
  const share = await post('/api/share', { app: app.id, me })
  const lead = (await post('/api/share', { app: app.id, me: `s-${share.token}`, person: { name: 'Lead', role: 'helper', hide: ['price'] } })).people.at(-1)
  assert.deepEqual(lead.hide, ['price'])
  const read = async () => (await post('/api/call', { app: app.id, me: `s-${lead.token}`, name: 'app_kept', arguments: { key: 'jobs' } })).structuredContent.value
  assert.deepEqual(await read(), [{ id: 'j1', client: 'Smith' }, { id: 'j2', client: 'Lee' }], 'prices are not sent')
  const out = await post('/api/call', { app: app.id, me: `s-${lead.token}`, name: 'app_keep', arguments: { key: 'jobs', value: [{ id: 'j1', client: 'Smith', done: true }, { id: 'j2', client: 'Lee' }] } })
  assert.ok(!out.isError, out.content?.[0]?.text)
  const owner = (await post('/api/call', { app: app.id, me: `s-${share.token}`, name: 'app_kept', arguments: { key: 'jobs' } })).structuredContent.value
  assert.deepEqual(owner.map(j => j.price), [900, 450], 'prices stay as they were')
  assert.equal(owner[0].done, true)
  // Stopped: the open says so and sends no app.
  await post('/api/unshare', { app: app.id, me: `s-${share.token}`, self: me })
  const text = await (await fetch(`${chat.url}api/apps/${app.id}/open`, { method: 'POST', body: JSON.stringify({ talk: 't', links: { [app.id]: share.helper } }) })).text()
  assert.match(text, /"type":"stopped"/)
  assert.doesNotMatch(text, /"type":"app"/)
})

test('a page watching a shared app hears when another phone keeps something (MARCUS-4, LENA-5)', { skip }, async () => {
  const me = 'd'.repeat(32)
  const app = await chat.apps.put({ name: 'Live list', about: 'Items.', kind: 'tool', tool: 'inputs', tools: ['inputs'], source: FRAGMENT('live'), args: { request: 'live' }, saved: true }, 'made by hand')
  const share = await post('/api/share', { app: app.id, me })
  const res = await fetch(`${chat.url}api/watch?apps=${app.id}|s-${share.token}`)
  const reader = res.body.getReader()
  await reader.read() // ": watching"
  await post('/api/call', { app: app.id, me: `s-${share.helper}`, self: 'other', name: 'app_keep', arguments: { key: 'items', value: [{ id: 'x' }] } })
  const got = await Promise.race([reader.read(), new Promise(r => setTimeout(() => r(null), 5000))])
  await reader.cancel()
  assert.ok(got && /"app":"/.test(new TextDecoder().decode(got.value)), 'told of the keep')
})

test('two phones keeping at once: each keep goes on top of what the other kept, and a clash is said (K: INES-1, MAYA-2, TOM-3, LENA-1)', { skip }, async () => {
  const me = 'e'.repeat(32)
  const app = await chat.apps.put({ name: 'Two phones', about: 'A list.', kind: 'tool', tool: 'inputs', tools: ['inputs'], source: FRAGMENT('two'), args: { request: 'two' }, saved: true }, 'made by hand')
  const base = [{ id: 'a', item: 'milk', done: false }, { id: 'b', item: 'garlic', done: false }]
  await post('/api/call', { app: app.id, me, name: 'app_keep', arguments: { key: 'items', value: base } })
  const share = await post('/api/share', { app: app.id, me })
  const joana = (await post('/api/share', { app: app.id, me: `s-${share.token}`, person: { name: 'Joana', role: 'helper' } })).people.at(-1)
  const keep = (who, value, from, key = 'items') => post('/api/call', { app: app.id, me: who, name: 'app_keep', arguments: { key, value }, base: from })
  const read = async (key = 'items') => (await post('/api/call', { app: app.id, me: `s-${share.token}`, name: 'app_kept', arguments: { key } })).structuredContent.value
  // He ticks garlic; she, from the same list, adds paper: both stay.
  assert.equal((await keep(`s-${joana.token}`, [base[0], { ...base[1], done: true }], base)).isError, undefined)
  const out = await keep(`s-${share.token}`, [...base, { id: 'c', item: 'paper', done: false }], base)
  assert.equal(out.structuredContent.merged, true)
  assert.deepEqual((await read()).map(e => [e.item, e.done]), [['milk', false], ['garlic', true], ['paper', false]])
  // The same hotel box typed on both: the first stays, and the other is said.
  await keep(`s-${share.token}`, { hotel: '' }, undefined, 'stay')
  await keep(`s-${share.token}`, { hotel: 'Umbrella' }, { hotel: '' }, 'stay')
  const clash = await keep(`s-${share.token}`, { hotel: 'Ribeira' }, { hotel: '' }, 'stay')
  assert.deepEqual(clash.structuredContent.said, [{ field: 'stay/hotel', mine: 'Ribeira', theirs: 'Umbrella' }])
  assert.deepEqual(await read('stay'), { hotel: 'Umbrella' })
  // The same invoice added on both phones goes in once.
  const now = await read()
  await keep(`s-${share.token}`, [...now, { id: 'x1', item: 'flour', amount: 1150 }], now)
  const twice = await keep(`s-${joana.token}`, [...now, { id: 'y1', item: 'flour', amount: 1150 }], now)
  assert.equal((await read()).filter(e => e.item === 'flour').length, 1)
  assert.equal(twice.structuredContent.said[0].twice, 'flour')
  // A keep with no base (an app from before) replaces, as it always did.
  assert.equal((await keep(`s-${share.token}`, [base[0]])).structuredContent.merged, undefined)
})

test('a helper can never take out what the owner added, a part keeps its crew, and earlier versions say what changed (L, M: MAYA-1, MARCUS-3, LENA-3)', { skip }, async () => {
  const me = 'f'.repeat(32)
  const app = await chat.apps.put({ name: 'Extras', about: 'A list.', kind: 'tool', tool: 'inputs', tools: ['inputs'], source: FRAGMENT('extras'), args: { request: 'extras' }, saved: true }, 'made by hand')
  const call = (who, value) => post('/api/call', { app: app.id, me: who, name: 'app_keep', arguments: { key: 'extras', value } })
  await call(me, ['milk'])
  const share = await post('/api/share', { app: app.id, me })
  const chidi = (await post('/api/share', { app: app.id, me: `s-${share.token}`, person: { name: 'Chidi', role: 'helper' } })).people.at(-1)
  assert.equal((await call(`s-${chidi.token}`, ['milk', 'cheerios'])).isError, undefined)
  // Maya puts bread where his cheerios were.
  assert.equal((await call(`s-${share.token}`, ['milk', 'bread', 'cheerios'])).isError, undefined)
  assert.equal((await call(`s-${chidi.token}`, ['milk', 'cheerios'])).isError, true, 'his link can’t take her bread out')
  assert.equal((await call(`s-${chidi.token}`, ['milk', 'bread'])).isError, undefined, 'but can take out his own')
  // Earlier versions: the copy just before a change, and what changed then.
  const hist = await post('/api/history', { app: app.id, me: `s-${share.token}` })
  assert.ok(hist.moments.some(m => /cheerios taken out of extras/.test(m.then)), JSON.stringify(hist.moments))
  // MARCUS-3, MARCUS-1: a crew B lead adding a job marked crew A is told
  // his link adds only to crew B, and nothing goes in; one with no crew lands in his.
  const week = await chat.apps.put({ name: 'Crews', about: 'Jobs.', kind: 'tool', tool: 'inputs', tools: ['inputs'], source: FRAGMENT('crews'), args: { request: 'crews' }, saved: true }, 'made by hand')
  await post('/api/call', { app: week.id, me, name: 'app_keep', arguments: { key: 'jobs', value: [{ id: 1, crew: 'A' }, { id: 2, crew: 'B' }] } })
  const s2 = await post('/api/share', { app: week.id, me })
  const darnell = (await post('/api/share', { app: week.id, me: `s-${s2.token}`, person: { name: 'Darnell', role: 'helper', only: { field: 'crew', value: 'B' } } })).people.at(-1)
  const other = await post('/api/call', { app: week.id, me: `s-${darnell.token}`, name: 'app_keep', arguments: { key: 'jobs', value: [{ id: 2, crew: 'B' }, { id: 3, crew: 'A', client: 'Okafor' }] } })
  assert.equal(other.isError, true)
  assert.match(other.content[0].text, /adds only to B, not to A/)
  const mine = await post('/api/call', { app: week.id, me: `s-${darnell.token}`, name: 'app_keep', arguments: { key: 'jobs', value: [{ id: 2, crew: 'B' }, { id: 4, client: 'Lopez' }] } })
  assert.equal(mine.structuredContent.merged, true, 'his screen is told what was kept')
  assert.equal(mine.structuredContent.value.find(j => j.id === 4).crew, 'B')
  const jobs = (await post('/api/call', { app: week.id, me: `s-${s2.token}`, name: 'app_kept', arguments: { key: 'jobs' } })).structuredContent.value
  assert.equal(jobs.find(j => j.id === 3), undefined)
  assert.equal(jobs.find(j => j.id === 4).crew, 'B')
})

test('a link made for a person signs with their name, the app knows who it is, and earlier versions say who (P: MAYA-9, LENA-2, MARCUS-2, LENA-4)', { skip }, async () => {
  const me = '9'.repeat(32)
  const app = await chat.apps.put({ name: 'Signed', about: 'Jobs.', kind: 'tool', tool: 'inputs', tools: ['inputs'], source: FRAGMENT('signed'), args: { request: 'signed' }, saved: true }, 'made by hand')
  const share = await post('/api/share', { app: app.id, me })
  const owner = `s-${share.token}`
  const keep = (who, key, value) => post('/api/call', { app: app.id, me: who, name: 'app_keep', arguments: { key, value } })
  const read = async key => (await post('/api/call', { app: app.id, me: owner, name: 'app_kept', arguments: { key } })).structuredContent.value
  await keep(owner, 'jobs', [{ id: 'kim', client: 'Kim', done: false }])
  const ray = (await post('/api/share', { app: app.id, me: owner, person: { name: 'Ray', role: 'helper' } })).people.at(-1)
  // The app wrote "helper" and "Luis"; the server writes Ray.
  assert.equal((await keep(`s-${ray.token}`, 'jobs', [{ id: 'kim', client: 'Kim', done: true, doneBy: 'Luis' }, { id: 'n1', client: 'Patel', by: 'helper' }])).isError, undefined)
  const jobs = await read('jobs')
  assert.equal(jobs.find(j => j.id === 'kim').doneBy, 'Ray', 'a tick is signed by the link')
  assert.equal(jobs.find(j => j.id === 'kim').by, undefined, 'the owner’s job stays the owner’s')
  assert.equal(jobs.find(j => j.id === 'n1').by, 'Ray', 'what he adds is his, by name')
  // Lists inside a week are signed too.
  await keep(owner, 'week', { mon: [{ id: 'm1', item: 'milk' }] })
  await keep(`s-${ray.token}`, 'week', { mon: [{ id: 'm1', item: 'milk' }, { id: 'm2', item: 'eggs', by: 'guest' }] })
  assert.equal((await read('week')).mon.find(e => e.id === 'm2').by, 'Ray')
  // The app is told who the link is, so it never asks.
  const all = await post('/api/call', { app: app.id, me: `s-${ray.token}`, name: 'app_kept', arguments: { all: true } })
  assert.deepEqual(all.structuredContent.who, { role: 'helper', name: 'Ray' })
  assert.equal((await post('/api/call', { app: app.id, me, name: 'app_kept', arguments: { all: true } })).structuredContent.who, undefined)
  // Earlier versions say what changed in an entry and who did it.
  const hist = await post('/api/history', { app: app.id, me: owner })
  const moment = hist.moments.find(m => /Kim ticked/.test(m.then))
  assert.ok(moment, JSON.stringify(hist.moments))
  assert.deepEqual(moment.by, ['Ray'])
})

test('names stay true: the app’s own keeps are nobody’s, history names each entry’s person, the owner is no link, and a new link is the same person (U, W: TOM-2, INES-3, DEV-2, MARCUS-3, LENA-2)', { skip }, async () => {
  const me = '6'.repeat(32)
  const app = await chat.apps.put({ name: 'Names', about: 'Jobs.', kind: 'tool', tool: 'inputs', tools: ['inputs'], source: FRAGMENT('names'), args: { request: 'names' }, saved: true }, 'made by hand')
  const share = await post('/api/share', { app: app.id, me })
  const owner = `s-${share.token}`
  const keep = (who, key, value, extra = {}) => post('/api/call', { app: app.id, me: who, name: 'app_keep', arguments: { key, value, ...extra }, ...(extra.base !== undefined ? { base: extra.base } : {}) })
  const read = async key => (await post('/api/call', { app: app.id, me: owner, name: 'app_kept', arguments: { key } })).structuredContent.value
  await keep(owner, 'days', [{ id: 'd7', day: 'Apr 7' }])
  const ruth = (await post('/api/share', { app: app.id, me: owner, person: { name: 'Ruth', role: 'helper' } })).people.at(-1)
  // A new version, drawn first on Ruth's phone, puts Tom's dinner in its
  // place by itself: nobody's name on it, and not hers to take out (TOM-2).
  assert.equal((await keep(`s-${ruth.token}`, 'days', [{ id: 'd7', day: 'Apr 7', dinner: 'Trattoria Lucca' }], { auto: true })).isError, undefined)
  const days = await read('days')
  assert.equal(days[0].dinner, 'Trattoria Lucca')
  assert.equal(days[0].by, undefined)
  assert.equal(JSON.stringify(days).includes('Ruth'), false, 'no name of hers anywhere on it')
  assert.equal((await keep(`s-${ruth.token}`, 'days', [{ id: 'd7', day: 'Apr 7' }])).isError, true, 'her link can’t take his dinner out')

  // The owner, sharing, is no link: the app gets no who, so it lets them change anything (DEV-2).
  assert.equal((await post('/api/call', { app: app.id, me: owner, name: 'app_kept', arguments: { all: true } })).structuredContent.who, undefined)

  // Earlier versions name each entry's own person: only the detergente was Joana's (INES-3).
  await keep(owner, 'expenses', [{ id: 'r', item: 'rent', amount: 600 }])
  const joana = (await post('/api/share', { app: app.id, me: owner, person: { name: 'Joana', role: 'helper' } })).people.at(-1)
  await keep(`s-${joana.token}`, 'expenses', [{ id: 'r', item: 'rent', amount: 600 }, { id: 'j', item: 'detergente', amount: 4.6 }])
  await keep(owner, 'expenses', [{ id: 'r', item: 'rent', amount: 600 }, { id: 'j', item: 'detergente', amount: 4.6, by: 'Joana' }, { id: 'p', item: 'pastel de nata', amount: 1.3 }, { id: 'b', item: 'bilhete comboio', amount: 2.35 }])
  const hist = await post('/api/history', { app: app.id, me: owner })
  const moment = hist.moments.find(m => /detergente/.test(m.then))
  assert.ok(moment, JSON.stringify(hist.moments))
  assert.match(moment.then, /detergente added to expenses by Joana/)
  // The owner's own entries, a moment later, are a moment of their own,
  // never told as Joana's (round 9 DEV-2, INES-4).
  assert.doesNotMatch(moment.then, /pastel de nata/)
  const owners = hist.moments.find(m => /pastel de nata/.test(m.then))
  assert.ok(owners, JSON.stringify(hist.moments))
  assert.match(owners.then, /pastel de nata, bilhete comboio added to expenses(;|$)/)
  assert.deepEqual(owners.by, [], 'not Joana’s')

  // The same invoice number from two phones is said to every phone that
  // opens it, with each one's amount and who, until one is taken out (LENA-2).
  await keep(owner, 'invoices', [])
  await keep(`s-${joana.token}`, 'invoices', [{ id: 'i1', number: 'MS-2320', amount: 545.2 }], { base: [] })
  await keep(owner, 'invoices', [{ id: 'i2', number: 'MS-2320', amount: 545.02 }], { base: [] })
  const opened = await post('/api/call', { app: app.id, me: owner, name: 'app_kept', arguments: { all: true } })
  assert.equal(opened.structuredContent.clashes?.length, 1, JSON.stringify(opened.structuredContent))
  assert.match(opened.structuredContent.clashes[0].text, /MS-2320 is in invoices 2 times \(545\.20 by Joana, 545\.02 by the owner\): check which is right/)
  // Each can be kept with a button: the note says which entry is which.
  assert.deepEqual(opened.structuredContent.clashes[0].choices.map(c => c.id), ['i1', 'i2'])
  const both = await read('invoices')
  await keep(owner, 'invoices', both.filter(i => i.id !== 'i2'))
  assert.equal((await post('/api/call', { app: app.id, me: owner, name: 'app_kept', arguments: { all: true } })).structuredContent.clashes, undefined, 'fixed, it is said no more')

  // A new link for the same person can change what the old one marked (MARCUS-3).
  await keep(owner, 'jobs', [{ id: 's', client: 'Smith', status: '' }])
  const jose = (await post('/api/share', { app: app.id, me: owner, person: { name: 'Jose', role: 'helper' } })).people.at(-1)
  assert.equal((await keep(`s-${jose.token}`, 'jobs', [{ id: 's', client: 'Smith', status: 'running late' }])).isError, undefined)
  await post('/api/unshare', { app: app.id, me: owner, self: me, person: jose.token })
  const jose2 = (await post('/api/share', { app: app.id, me: owner, person: { name: 'Jose', role: 'helper' } })).people.at(-1)
  assert.equal((await keep(`s-${jose2.token}`, 'jobs', [{ id: 's', client: 'Smith', status: 'done' }])).isError, undefined, 'Jose is Jose')
  assert.equal((await read('jobs'))[0].status, 'done')
  const sam = (await post('/api/share', { app: app.id, me: owner, person: { name: 'Sam', role: 'helper' } })).people.at(-1)
  assert.equal((await keep(`s-${sam.token}`, 'jobs', [{ id: 's', client: 'Smith', status: 'late' }])).isError, true, 'but not another person')

  // A link kept from prices adds a job with no price, and its screen is told (MARCUS-1).
  const lead = (await post('/api/share', { app: app.id, me: owner, person: { name: 'Darnell', role: 'helper', hide: ['price'] } })).people.at(-1)
  const added = await keep(`s-${lead.token}`, 'jobs', [{ id: 's', client: 'Smith', status: 'done' }, { id: 'n', client: 'Brown', price: 999 }])
  assert.equal(added.structuredContent.merged, true)
  assert.equal(added.structuredContent.value.find(j => j.id === 'n').price, undefined)
  assert.equal((await read('jobs')).find(j => j.id === 'n').price, undefined)
})

test('what was taken out stays out: a phone still showing the old list can’t bring it back (Z: round 9 TOM-2, TOM-3)', { skip }, async () => {
  const me = '7'.repeat(32)
  const app = await chat.apps.put({ name: 'Trip', about: 'Nights.', kind: 'tool', tool: 'inputs', tools: ['inputs'], source: FRAGMENT('trip'), args: { request: 'trip' }, saved: true }, 'made by hand')
  const share = await post('/api/share', { app: app.id, me })
  const owner = `s-${share.token}`
  const margaret = (await post('/api/share', { app: app.id, me: owner, person: { name: 'Margaret', role: 'helper' } })).people.at(-1)
  const keep = (who, value, base, self) => post('/api/call', { app: app.id, me: who, self, name: 'app_keep', arguments: { key: 'nights', value }, ...(base !== undefined ? { base } : {}) })
  const read = async () => (await post('/api/call', { app: app.id, me: owner, name: 'app_kept', arguments: { key: 'nights' } })).structuredContent.value
  const n14 = { id: 'n14', night: 'Wed Apr 14', town: 'Porto' }, n15 = { id: 'n15', night: 'Thu Apr 15', town: 'Évora' }, p15 = { id: 'p15', night: 'Thu Apr 15', town: 'Porto' }
  await keep(owner, [n14, n15], undefined, 'laptop')
  // Moving Porto a day later takes Évora's first night out.
  await keep(owner, [n14, p15], [n14, n15], 'laptop')
  // Margaret's iPad still had the old list, and keeps it as if it were new.
  const stale = await keep(`s-${margaret.token}`, [n14, p15, n15], [n14, p15])
  assert.deepEqual((await read()).map(n => n.id), ['n14', 'p15'], 'Évora’s night stays out')
  assert.equal(stale.structuredContent.merged, true)
  assert.match(JSON.stringify(stale.structuredContent.said), /stayedOut.*Thu Apr 15/)
  // A new version drawn first on Margaret's iPad puts in the dinner Tom
  // asked for, the app writing her name on it: it goes in with no name
  // (round 9 TOM-4).
  const dinner = await post('/api/call', { app: app.id, me: `s-${margaret.token}`, name: 'app_keep', arguments: { key: 'dinners', value: [{ id: 'ramiro', place: 'Cervejaria Ramiro', by: 'Margaret' }], auto: true }, base: null })
  assert.equal(dinner.isError, undefined)
  const dinners = (await post('/api/call', { app: app.id, me: owner, name: 'app_kept', arguments: { key: 'dinners' } })).structuredContent.value
  assert.equal(dinners[0].by, undefined, JSON.stringify(dinners))
  // The one who took it out can put it back (an undo).
  await keep(owner, [n14, p15, n15], [n14, p15], 'laptop')
  assert.deepEqual((await read()).map(n => n.id), ['n14', 'p15', 'n15'])
})

test('a request sent just before the browser closes is answered and kept in the conversation (AB: round 9 LENA-3)', { skip }, async () => {
  const talk = '4'.repeat(32)
  const gone = new AbortController()
  const sent = fetch(`${chat.url}api/turn`, { method: 'POST', body: JSON.stringify({ text: 'slowly, what is due?', talk }), signal: gone.signal }).catch(() => null)
  await new Promise(r => setTimeout(r, 100))
  gone.abort()
  await sent
  let turns = []
  for (let i = 0; i < 30 && !turns.some(t => t.reply); i++) {
    await new Promise(r => setTimeout(r, 100))
    turns = (await (await fetch(`${chat.url}api/talk?talk=${talk}`)).json()).turns
  }
  assert.equal(turns.at(-1)?.request, 'slowly, what is due?', JSON.stringify(turns))
  assert.equal(turns.at(-1)?.reply, 'Took a while.')
})

test('a link gets only what it may see: what is marked shared, none of the owner’s figures, and only its part of what the app was made with (AC: round 9 INES-3, INES-5, MARCUS-1)', { skip }, async () => {
  const me = '3'.repeat(32)
  const app = await chat.apps.put({ name: 'Budget', about: 'Spending.', kind: 'tool', tool: 'inputs', tools: ['inputs'], source: FRAGMENT('budget'), args: { request: 'budget', values: { jobs: [{ id: 'j1', client: 'Johnson', crew: 'Crew 1', price: 120 }, { id: 'j2', client: 'Patel', crew: 'Crew 2', price: 90 }] } }, saved: true }, 'made by hand')
  const share = await post('/api/share', { app: app.id, me })
  const owner = `s-${share.token}`
  await post('/api/call', { app: app.id, me: owner, name: 'app_keep', arguments: { key: 'expenses', value: [{ id: 'c', item: 'coffee', amount: 1.2 }, { id: 'p', item: 'Pingo Doce', amount: 31.8, shared: true, paid: 'me' }] } })
  await post('/api/call', { app: app.id, me: owner, name: 'app_keep', arguments: { key: 'budgets', value: { '2026-11': 650 } } })
  await post('/api/call', { app: app.id, me: owner, name: 'app_keep', arguments: { key: 'currency', value: 'EUR' } })
  const joana = (await post('/api/share', { app: app.id, me: owner, person: { name: 'Joana', role: 'helper', only: { field: '*', value: 'Joana' } } })).people.at(-1)
  const seen = (await post('/api/call', { app: app.id, me: `s-${joana.token}`, name: 'app_kept', arguments: { all: true } })).structuredContent.values
  assert.deepEqual(seen.expenses.map(e => e.item), ['Pingo Doce'], 'what is marked shared is shared with her, her coffee-free')
  assert.equal(seen.budgets, undefined, 'the owner’s budget never reaches her phone')
  assert.equal(seen.currency, 'EUR')
  // A crew lead's link kept from prices opens with only his crew and no price,
  // even in what the app was first made with (what an offline phone draws).
  const luis = (await post('/api/share', { app: app.id, me: owner, person: { name: 'Luis', role: 'helper', only: { field: 'crew', value: 'Crew 2' }, hide: ['price'] } })).people.at(-1)
  const opened = (await events(`/api/apps/${app.id}/open`, { talk: '2'.repeat(32), links: { [app.id]: luis.token } })).find(e => e.type === 'app')
  assert.deepEqual(opened.args.values.jobs, [{ id: 'j2', client: 'Patel', crew: 'Crew 2' }])
  assert.doesNotMatch(JSON.stringify(opened.result), /Johnson|120/)
})

test('an answer is kept once, and stays kept after a reload (Y: PRIYA-3)', { skip }, async () => {
  const talk = '5'.repeat(32)
  const answer = (await events('/api/turn', { text: 'find reviews of quiet headphones', talk })).find(e => e.type === 'app')
  const once = (await post('/api/apps/search-results/keep-answer', { talk, shown: answer.shown, name: 'Quiet headphones · Nov 20', request: 'quiet headphones' })).app
  const twice = (await post('/api/apps/search-results/keep-answer', { talk, shown: answer.shown, name: 'Quiet headphones · Nov 20', request: 'quiet headphones' })).app
  assert.equal(twice.id, once.id, 'a second press is the same kept answer')
  const reloaded = await (await fetch(`${chat.url}api/shown/${answer.shown}?talk=${talk}`)).json()
  assert.equal(reloaded.app.id, once.id, 'that place in the conversation opens as the kept answer')
  // Something to look up for it is searched, and the kept answer itself
  // takes it: the same app, a new version, its notes with it (PRIYA-4).
  const searches = model.calls.research.length
  const asked = (await chat.apps.get(once.id)).args.query
  model.pick = once.id
  const priced = (await events('/api/turn', { text: 'find its store price', talk })).find(e => e.type === 'app')
  delete model.pick
  assert.equal(model.calls.research.length, searches + 1, 'it looked it up')
  assert.equal(priced.app.id, once.id)
  assert.equal(priced.app.version, once.version + 1)
  // What it found is added: every option kept stays, in its order, the
  // one it priced takes the price, and both sources are there (round 9
  // PRIYA-1, PRIYA-3).
  const kept = (await chat.apps.get(once.id)).args
  assert.deepEqual(kept.items.map(i => i.name), ['Sony WH-1000XM5', 'Bose QuietComfort Ultra', 'AirPods Max'])
  assert.equal(kept.items[0].facts.store_price, 399)
  assert.deepEqual(kept.items[0].says.map(s => s.doc), [1, 2])
  assert.deepEqual(kept.documents.map(d => d.url), ['https://example.com/quiet', 'https://example.com/sony-store'])
  assert.equal(kept.query, asked, 'the question, and so the title, stays')
  // An option taken off by asking goes from the kept answer itself, as a
  // new version (round 9 PRIYA-4).
  model.pick = once.id
  const off = (await events('/api/turn', { text: 'take the AirPods Max off my list', talk })).find(e => e.type === 'app')
  delete model.pick
  assert.equal(off?.app.id, once.id)
  assert.deepEqual((await chat.apps.get(once.id)).args.items.map(i => i.name), ['Sony WH-1000XM5', 'Bose QuietComfort Ultra'])
})

test('open phones keep up: a new version is told with what changed, and a stopped link is told (R: MAYA-3, DEV-7)', { skip }, async () => {
  const me = '8'.repeat(32)
  const app = await chat.apps.put({ name: 'Dinners', about: 'A week.', kind: 'tool', tool: 'inputs', tools: ['inputs'], source: FRAGMENT('dinners'), args: { request: 'dinners' }, saved: true }, 'made by hand')
  const share = await post('/api/share', { app: app.id, me })
  const femi = (await post('/api/share', { app: app.id, me: `s-${share.token}`, person: { name: 'Femi', role: 'helper' } })).people.at(-1)
  const res = await fetch(`${chat.url}api/watch?apps=${app.id}|s-${femi.token}`)
  const reader = res.body.getReader()
  await reader.read() // ": watching"
  const next = async () => {
    const got = await Promise.race([reader.read(), new Promise(r => setTimeout(() => r(null), 5000))])
    return got ? JSON.parse(new TextDecoder().decode(got.value).replace(/^data: /, '')) : null
  }
  await chat.apps.change(app.id, FRAGMENT('dinners for four'), 'changed: amounts for four')
  const ev = await next()
  assert.equal(ev.version, 2)
  assert.match(ev.change, /amounts for four/)
  // His phone gets the new version, though his link can't change the app.
  const now = await post(`/api/apps/${app.id}/now`, { links: { [app.id]: femi.token } })
  assert.equal(now.app.version, 2)
  assert.ok(now.html.length > 0)
  // Stopping his link tells his open tab, which then shows it stopped.
  await post('/api/unshare', { app: app.id, me: `s-${share.token}`, self: me, person: femi.token })
  assert.equal((await next())?.app, app.id)
  await reader.cancel()
  const kept = await post('/api/call', { app: app.id, me: `s-${femi.token}`, name: 'app_kept', arguments: { all: true } })
  assert.equal(kept.isError, true)
})

test('Undo takes back the last change to what an app keeps, after its version; a hand take-out is said with Undo (AE: round 10 MARCUS-1, LENA-8, INES-8)', { skip }, async () => {
  const me = 'e'.repeat(32)
  const app = await chat.apps.put({ name: 'Crew week', about: 'Jobs.', kind: 'tool', tool: 'inputs', tools: ['inputs'], source: FRAGMENT('jobs'), args: { request: 'jobs' }, saved: true }, 'made by hand')
  const call = (name, args, who = me) => post('/api/call', { app: app.id, me: who, name, arguments: args })
  const jobs = [{ id: 'j1', job: 'Johnson', day: 'Mon' }, { id: 'j2', job: 'Garcia', day: 'Thu' }]
  await call('app_keep', { key: 'jobs', value: jobs })
  // The version it is at is dated: a change kept later than it is the one Undo takes back.
  await new Promise(r => setTimeout(r, 20))
  const moved = [{ ...jobs[0], day: 'Tue' }, jobs[1]]
  const kept = await call('app_keep', { key: 'jobs', value: moved })
  assert.equal(kept.structuredContent?.removed, undefined, 'a move takes nothing out')
  const hist = await post('/api/history', { app: app.id, me, self: me })
  assert.equal(typeof hist.versionAt, 'number')
  const last = hist.moments.find(m => m.then)
  assert.match(last.then, /Johnson day Mon to Tue/)
  await post('/api/restore', { app: app.id, me, self: me, at: last.at })
  assert.equal((await call('app_kept', { key: 'jobs' })).structuredContent.value[0].day, 'Mon', 'the move is taken back, not the app')
  assert.equal((await chat.apps.get(app.id)).version, 1)
  // Taking an entry out by hand is said to the page that did it, for its Undo.
  const out = await call('app_keep', { key: 'jobs', value: [jobs[0]] })
  assert.match(String(out.structuredContent?.removed), /^Garcia/)
  // A link that only adds and ticks gets no such Undo.
  const share = await post('/api/share', { app: app.id, me })
  const helper = await call('app_keep', { key: 'jobs', value: [jobs[0], { id: 'j3', job: 'Baker' }] }, `s-${share.helper}`)
  assert.equal(helper.structuredContent?.removed, undefined)
})

test('a request writes only what it means: numbers as typed, a lookup for kept options adds none and keeps models apart (AH: round 10 LENA-1, PRIYA-1, PRIYA-2)', { skip }, async () => {
  const ledger = await chat.apps.put({ name: 'Invoice ledger', about: 'Supplier invoices.', kind: 'tool', tool: 'inputs', tools: ['inputs'], source: FRAGMENT('ledger'), args: { request: 'invoices' }, saved: true }, 'made by hand')
  const added = (await events('/api/turn', { text: 'add invoice #MM-21240 120.5', talk: '8'.repeat(32), screen: [{ id: ledger.id, version: 1, name: 'Invoice ledger', shows: '', did: [] }] })).find(e => e.type === 'app')
  assert.equal(added.args.values.add[0].invoice_no, 'MM-21240', 'no "#" in front of the number')
  const { addFound } = await import('../server/widgets/findings.mjs')
  const kept = { items: [{ name: 'Bose QuietComfort Ultra Headphones', facts: { price: '$429' } }, { name: 'Sony WH-1000XM5' }] }
  const priced = addFound(kept, { items: [{ name: 'Bose QuietComfort Headphones', facts: { price: '$169' } }, { name: 'Sony WH1000XM5', facts: { price: '$329' } }] }, { adding: false })
  assert.equal(priced.items.length, 2, 'a lookup adds no option of its own')
  assert.equal(priced.items[0].facts.price, '$429', 'the plain QuietComfort’s price is not the Ultra’s')
  assert.equal(priced.items[1].facts.price, '$329', 'WH-1000XM5 and WH1000XM5 are one')
  assert.equal(addFound(kept, { items: [{ name: 'Apple AirPods Max' }] }).items.length, 3, 'asked for more, an option is added')
})

test('the answer says only what the request changed, the chat sees all an app keeps, and a request from before a page closed is there on reopening (AF: round 10 TOM-4, LENA-2, LENA-4, INES-6, MARCUS-7)', { skip }, async () => {
  const me = 'a'.repeat(32)
  const talk = 'b'.repeat(32)
  const budget = await chat.apps.put({ name: 'Budget', about: 'Spending.', kind: 'tool', tool: 'inputs', tools: ['inputs'], source: FRAGMENT('budget'), args: { request: 'budget' }, saved: true }, 'made by hand')
  const call = (name, args) => fetch(chat.url + 'api/call', { method: 'POST', headers: { 'x-self': me }, body: JSON.stringify({ app: budget.id, me, name, arguments: args }) }).then(r => r.json())
  await call('app_keep', { key: 'items', value: [{ id: 'i1', item: 'rent', amount: 900 }, { id: 'i9', item: 'gym', amount: 30 }] })
  const turn = async text => (await (await fetch(chat.url + 'api/turn', { method: 'POST', headers: { 'x-self': me }, body: JSON.stringify({ text, talk, screen: [{ id: budget.id, version: 1, name: 'Budget', shows: 'rent 900', did: [] }] }) })).text()).trim().split('\n').map(l => JSON.parse(l))
  // The chat is given all the app keeps, also what is out of sight (gym).
  const said = await turn('add coffee 3.5')
  assert.match(model.calls.route.at(-1).screen[0].holds, /gym/)
  // The page drew it and kept the coffee; the answer is told what changed.
  await call('app_keep', { key: 'items', value: [{ id: 'i1', item: 'rent', amount: 900 }, { id: 'i9', item: 'gym', amount: 30 }, { id: 'c1', item: 'coffee', amount: 3.5 }] })
  const turnId = said.find(e => e.type === 'turn')?.id
  assert.ok(turnId?.startsWith(`${talk}:`), 'the turn is kept from its start')
  await post('/api/say', { token: said.find(e => e.type === 'say').token, shows: '' })
  assert.equal(model.calls.changed, 'items: added coffee')
  // An app that took none of it: the answer is told nothing changed.
  const none = await turn('add tea 2')
  await post('/api/say', { token: none.find(e => e.type === 'say').token, shows: '' })
  assert.equal(model.calls.changed, 'nothing')
  // The answer is kept with its turn even when the page never records it.
  let turns = (await (await fetch(`${chat.url}api/talk?talk=${talk}`)).json()).turns
  assert.equal(turns.find(t => t.id === turnId)?.request, 'add coffee 3.5')
  assert.ok(turns.find(t => t.id === turnId)?.reply, 'the answer is with the turn')
  // The page's own record fills the same turn, not a second one.
  await post('/api/talk', { talk, turn: turnId, request: 'add coffee 3.5', reply: 'Coffee went in.', shown: [] })
  turns = (await (await fetch(`${chat.url}api/talk?talk=${talk}`)).json()).turns
  assert.equal(turns.filter(t => t.request === 'add coffee 3.5').length, 1)
  assert.equal(turns.find(t => t.id === turnId).reply, 'Coffee went in.')
  // A request still running when the page opens again is there, as running.
  const other = 'af'.repeat(16)
  const gone = new AbortController()
  const sent = fetch(`${chat.url}api/turn`, { method: 'POST', body: JSON.stringify({ text: 'slowly, what is left?', talk: other }), signal: gone.signal }).catch(() => null)
  await new Promise(r => setTimeout(r, 120))
  gone.abort()
  await sent
  const running = (await (await fetch(`${chat.url}api/talk?talk=${other}`)).json()).turns
  assert.equal(running.at(-1)?.request, 'slowly, what is left?')
  assert.equal(running.at(-1)?.running, true)
  let done = []
  for (let i = 0; i < 30 && !done.some(t => t.reply); i++) {
    await new Promise(r => setTimeout(r, 100))
    done = (await (await fetch(`${chat.url}api/talk?talk=${other}`)).json()).turns
  }
  assert.equal(done.length, 1, JSON.stringify(done))
  assert.equal(done[0].reply, 'Took a while.')
  assert.equal(done[0].running, undefined)
})
