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
      const entry = /^add (\w+) ([\d.,]+)/i.exec(input.request)
      if (entry) return { mode: 'use', kind: 'tool', use: input.screen[0]?.id, tool: 'inputs', args: { request: 'budget', values: { month: 'Nov', add: { item: entry[1], amount: Number(entry[2].replace(',', '.')) } } }, why: 'your budget' }
      if (/^find /i.test(input.request)) {
        const saved = input.candidates.find(c => c.id === (this.pick ?? 'search-results'))
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
            : [{ title: 'Quiet headphones, tested', url: 'https://example.com/quiet', source: 'example.com', date: '2026-11-01', summary: 'Sony first.', quotes: ['Quietest we tried.'], facts: { pick: 'Sony' } }],
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
    async reply({ summary, shows }) {
      calls.reply++
      calls.shows = shows
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
  const screenNow = [{ id: app.app.id, version: 2, name: 'Table booking', shows: 'Places', did: [] }]
  const worded = await events('/api/turn', { text: 'make it show the price', screen: screenNow })
  const wordedApp = worded.find(e => e.type === 'app')
  assert.equal(wordedApp.how, 'changed')
  assert.equal(wordedApp.app.version, 3)
  assert.match(wordedApp.html, /v2: show the price/)
  // The answer is written from what the changed app shows, never taken for granted.
  assert.ok(!worded.some(e => e.type === 'reply'))
  const saidChange = await post('/api/say', { token: worded.find(e => e.type === 'say').token, shows: '' })
  assert.match(saidChange.text, /^Table booking was rewritten as version 3/)
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
  const big = await post('/api/call', { app: id, me, name: 'app_keep', arguments: { key: 'big', value: 'x'.repeat(100_001) } })
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
  await post('/api/talk', { talk, request: 'find reviews of quiet headphones', reply: 'Sony first.', shown: [{ id: answer.shown, app: answer.app.id, name: answer.app.name, kind: 'info', how: answer.how, shows: 'x' }] })
  const second = (await events('/api/turn', { text: 'find good cameras', talk })).find(e => e.type === 'app')
  assert.notEqual(second.shown, answer.shown)
  await post('/api/talk', { talk, request: 'find good cameras', reply: '', shown: [] })

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

  // Kept as its own app, it keeps those findings when searches follow.
  const keptApp = (await post('/api/apps/search-results/keep-answer', { talk, shown: answer.shown, name: 'Quiet headphones · Nov 19', request: 'find reviews of quiet headphones' })).app
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

test('a helper link adds and ticks but takes nothing out, and a view link only looks', { skip }, async () => {
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
  assert.match(out.content[0].text, /can add and tick, but not take anything out/)
  assert.equal((await keep(share.helper, [])).isError, true)
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
