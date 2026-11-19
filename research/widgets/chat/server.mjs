#!/usr/bin/env node
// Instant apps: a chat whose answers come with small apps, and a rail of the
// apps the person kept. A request that needs an app either brings back a
// saved one (found in ChloeDB by words and meaning, then picked by the
// model) or has the model make one; a made app can be changed by asking and
// saved to the rail, and from then on it opens by itself for requests like
// the ones it served.
//
// Every app is an MCP App: an HTML view the page draws in a sandboxed
// iframe through the MCP Apps SDK's AppBridge, filled by a tool of
// ../server, which is also the MCP server Claude Desktop and claude.ai use.
//
//   node chat/server.mjs [--port 8800] [--host 127.0.0.1] [--data DIR]
//
// CHLOEDB_URL uses a running chloedb-server instead of starting one.
import { mkdirSync, readFileSync } from 'node:fs'
import { randomBytes } from 'node:crypto'
import { createServer } from 'node:http'
import { createRequire } from 'node:module'
import os from 'node:os'
import path from 'node:path'
import { fileURLToPath } from 'node:url'

import { described } from '../server/guide.mjs'
import { TOOLS, callTool, domainsOf, fragmentOf, render } from '../server/index.mjs'
import { KEEPING, keeping, takesOut } from '../server/apps.mjs'
import { claude } from './model.mjs'
import { Apps, valueNames } from '../server/store.mjs'

const require = createRequire(new URL('../server/', import.meta.url))

// The host half of the MCP Apps SDK, bundled once for the page.
async function bridge() {
  const esbuild = require('esbuild')
  const out = await esbuild.build({
    stdin: {
      contents: "export { AppBridge } from '@modelcontextprotocol/ext-apps/app-bridge'\nexport { PostMessageTransport } from '@modelcontextprotocol/ext-apps'\n",
      resolveDir: new URL('../server/', import.meta.url).pathname,
    },
    bundle: true,
    format: 'esm',
    write: false,
    platform: 'browser',
  })
  return out.outputFiles[0].text
}

const KNOWN = new Set(TOOLS.map(t => t.name))

// How many requests that call Claude one visitor may make in an hour, on a
// site others use (WIDGETS_PER_IP_HOUR): by the address the proxy in front
// says, or the socket's.
function limiter(perHour) {
  const seen = new Map()
  return req => {
    if (!perHour) return false
    const ip = String(req.headers['x-forwarded-for'] ?? '').split(',')[0].trim() || req.socket.remoteAddress
    const now = Date.now()
    const recent = (seen.get(ip) ?? []).filter(t => now - t < 3_600_000)
    if (recent.length >= perHour) return true
    seen.set(ip, [...recent, now])
    return false
  }
}

export async function start({ port = 8800, host = '127.0.0.1', data, model = claude(), apps, log = console.error, shared = true, perHour = Number(process.env.WIDGETS_PER_IP_HOUR || 0) } = {}) {
  const limited = limiter(perHour)
  const tooMany = 'You’ve asked a lot this hour. Please try again in a little while.'
  apps ??= await Apps.open({ dir: data ?? path.join(os.tmpdir(), 'instant-apps'), log, shared })
  const page = readFileSync(new URL('./index.html', import.meta.url), 'utf8')
  const bridgeJs = await bridge()
  // What each app shown in this process last drew, so a change redraws it
  // with the same data.
  const shown = new Map()
  // What each turn's answer is written from, once the page says what its
  // app shows (/api/say): by a one-time token the turn hands the page, so
  // the answer is part of the turn and not a request of its own.
  const sayable = new Map()
  const sayToken = (request, summary) => {
    const token = randomBytes(12).toString('hex')
    sayable.set(token, { request, summary, at: Date.now() })
    for (const [t, v] of sayable) if (Date.now() - v.at > 600_000) sayable.delete(t)
    return token
  }
  // A browser's own ids (its conversation, its kept values): random, from the page.
  const own = value => (typeof value === 'string' && /^[A-Za-z0-9-]{16,64}$/.test(value) ? value : '')
  // Whose kept values a browser reaches for an app, and what it may do with
  // them: its own ("own"), or a share link's ("s-<token>"): the link that
  // made it changes everything ("full"), a helper link adds and ticks but
  // takes nothing out ("helper"), a view link only looks ("viewer").
  const LINKS = '-links-'
  async function owning(appId, me) {
    const id = own(me)
    if (!id) return null
    if (!id.startsWith('s-')) return { owner: id, role: 'own' }
    const link = await apps.kept(appId, `link:${id.slice(2)}`, LINKS).catch(() => null)
    return link ? { owner: link.owner, role: link.role } : { owner: id, role: 'full' }
  }
  // What a helper or view link may not keep: a view link changes nothing, a
  // helper link adds and changes but takes nothing out (an entry, a field, a
  // value cleared).
  async function refusal(who, appId, args) {
    if (who.role === 'viewer') return 'Not kept: this link can only look. Whoever shared it can change it.'
    if (who.role !== 'helper') return ''
    const before = await apps.kept(appId, String(args?.key ?? ''), who.owner).catch(() => null)
    return takesOut(before, args?.value) ? 'Not kept: this link can add and tick, but not take anything out. Whoever shared it can.' : ''
  }
  // Each app sent to a page with a conversation is recorded as shown in it,
  // with what it was filled with, so it opens again as it was.
  function recording(talk, send) {
    const writes = []
    const out = event => {
      if (event.type === 'app' && talk) {
        event.shown = `${Date.now().toString(36)}-${randomBytes(6).toString('hex')}`
        writes.push(apps.show({ id: event.shown, talk, app: event.app.id, version: event.app.version, tool: event.tool, args: event.args, how: event.how }).catch(log))
      }
      send(event)
    }
    out.written = () => Promise.all(writes)
    return out
  }

  // An app as the page draws it.
  const view = app => ({
    id: app.id, name: app.name, about: app.about, kind: app.kind, tool: app.tool, tools: app.tools,
    saved: app.saved, version: app.version, uses: app.uses,
  })
  const htmlOf = app => render(app.source, domainsOf(app.tools))

  async function run(tool, args) {
    try {
      const data = await callTool(tool, args)
      return { result: { content: [{ type: 'text', text: data.summary }], structuredContent: data }, data }
    } catch (error) {
      return { result: { content: [{ type: 'text', text: error.message }], isError: true }, data: { summary: `${tool} failed: ${error.message}` } }
    }
  }

  // What the apps in the conversation show now, as the page tells it: what
  // each was last filled with, what was done in it, and what it said it
  // shows. The model reads it to answer about them ("which is quickest?").
  // Every app on the page, newest first: the three newest in full, the
  // older ones in short, so "the regex tester" or "the headphones you found"
  // is always one the chat can see.
  const onScreen = screen =>
    (Array.isArray(screen) ? screen : []).slice(0, 12).map((w, i) => ({
      id: String(w.id ?? '').slice(0, 64),
      shown: String(w.shown ?? '').slice(0, 64),
      version: Number(w.version) || 1,
      name: String(w.name ?? '').slice(0, 60),
      folded: Boolean(w.folded),
      shows: String(w.shows ?? '').slice(0, i < 3 ? 900 : 300),
      did: (Array.isArray(w.did) ? w.did : []).slice(-3).map(d => String(d).slice(0, 300)),
      told: String(w.told ?? '').slice(0, i === 0 ? 6000 : i < 3 ? 2500 : 400),
    }))

  // An entry the person asked the chat to put into an app (values.add):
  // each gets an id, so an app that opens again with it adds it once.
  function entries(args) {
    const values = args?.values
    if (!values || typeof values !== 'object' || values.add == null) return args
    const add = (Array.isArray(values.add) ? values.add : [values.add]).filter(e => e && typeof e === 'object').map(e => ({ id: String(e.id ?? randomBytes(5).toString('hex')), ...e }))
    return { ...args, values: { ...values, add } }
  }
  // Whether a query may be about what a kept answer found: it shares a
  // word that says what it is about with the answer's query or name. A
  // question about General Mills is not about loan rates.
  const STOP = new Set('the and for with from that this what which who how why when where about again show find open kept keep answer answers saved one ones please can you your mine our are was were has have had any all its into than then them they there here just more most some much many very like want need give tell look see best good new now'.split(' '))
  function sameSearch(asked, ...kept) {
    const words = text => new Set((String(text ?? '').toLowerCase().match(/[\p{L}\p{N}]{3,}/gu) ?? []).filter(w => !STOP.has(w)))
    const a = words(asked)
    if (!a.size) return true
    const b = words(kept.join(' '))
    for (const w of a) if (b.has(w) || b.has(w.replace(/s$/, '')) || b.has(`${w}s`)) return true
    return false
  }
  // What an app opens with next time: not the entries asked for once.
  function lasting(args) {
    const values = args?.values
    if (!values || typeof values !== 'object' || (values.add == null && values.edit == null)) return args
    const { add, edit, ...rest } = values
    return { ...args, values: rest }
  }

  async function turn({ text, history = [], lastApp, screen, talk }, send) {
    send({ type: 'status', text: 'Looking through your apps' })
    const candidates = await apps.recall(text, 8)
    send({ type: 'recall', candidates: candidates.map(c => ({ id: c.id, name: c.name, score: c.score })) })
    const last = lastApp ? await apps.get(lastApp.id) : null
    const plan = await model.route({
      request: text,
      history: history.slice(-6),
      candidates,
      last: last && { ...last, args: lastApp.args ?? last.args, reads: last.tool === 'inputs' ? valueNames(last.source, lastApp.args ?? last.args) : undefined },
      screen: onScreen(screen),
      tools: described(TOOLS),
    })
    send({ type: 'route', mode: plan.mode, kind: plan.kind, why: plan.why })
    // A change asked in words is the same change as the app's Change box,
    // and an undo the same as its version button: done, never just said.
    if ((plan.mode === 'change' || plan.mode === 'undo') && plan.app && (await apps.get(plan.app))) {
      // The app as the page shows it, newest first: its own data.
      const target = { shownId: onScreen(screen).find(w => w.id === plan.app)?.shown, talk: own(talk) }
      if (plan.mode === 'undo') {
        const was = await apps.get(plan.app)
        if (was.version <= 1) {
          send({ type: 'reply', text: `${was.name} has no earlier version to go back to.` })
          return
        }
        await undo(plan.app, send, target)
        send({ type: 'reply', text: `${was.name} is back to version ${was.version - 1}. Its Redo button brings the change back.` })
        return
      }
      const what = String(plan.what || text)
      const { app: changed, copied, data } = await change(plan.app, what, send, target)
      // Said from what the changed app shows, not taken for granted.
      const copy = copied ? ` It is a copy of that answer, kept in the person's apps as "${changed.name}"; other answers are as they were.` : ''
      send({ type: 'say', token: sayToken(text, `${changed.name} was rewritten as version ${changed.version} for: ${what}.${copy} Its Undo button takes the change back. What it was filled with: ${data?.summary ?? ''}`) })
      return
    }
    if (plan.mode === 'chat' || !plan.tool || !KNOWN.has(plan.tool)) {
      send({ type: 'reply', text: plan.reply || 'I could not find a way to answer that with an app.' })
      return
    }
    let app = plan.mode === 'use' && plan.use ? await apps.get(plan.use) : null
    let args = entries(plan.args ?? {})
    // A kept answer opens with what it found then; a new search is Search
    // results', even when the router took a kept answer for it.
    if (app?.id.startsWith('answer-') && plan.tool === 'findings' && !sameSearch(plan.args?.query, app.args?.query, app.args?.request, app.name)) app = await apps.get('search-results')
    const keptAnswer = app?.id.startsWith('answer-')
    if (keptAnswer) args = app.args
    // An app of what was found: the web searched first, and what it found
    // is the tool's arguments.
    if (plan.tool === 'findings' && !keptAnswer) {
      send({ type: 'status', text: 'Searching the web' })
      try {
        // A follow-up starts from what the app shown just before found.
        const found = last?.tool === 'findings' ? (lastApp.args ?? last.args) : null
        // What the person changed in that answer holds (a place taken out stays out).
        const changes = found ? await apps.changesOf(last.id).catch(() => []) : []
        args = await model.research({ request: text, query: String(args.query || text), history: history.slice(-6), found, changes })
      } catch (error) {
        send({ type: 'reply', text: `I could not search the web just now: ${error.message}` })
        return
      }
      if (!args.documents.length) {
        // In the person's words: the model's own query may be anything.
        send({ type: 'reply', text: `I couldn’t find anything on the web for “${text}”. Try again, or put it another way.` })
        return
      }
    }
    if (app) {
      send({ type: 'status', text: `Opening ${app.name}` })
      const tool = app.tools.includes(plan.tool) && !keptAnswer ? plan.tool : app.tool
      const { result, data } = await run(tool, args)
      // The app just shown, filled again for a follow-up, is an update.
      const how = lastApp?.id === app.id ? 'updated' : app.saved ? 'recalled' : 'again'
      send({ type: 'app', app: view(app), html: htmlOf(app), tool, args, result, how, why: plan.why })
      shown.set(app.id, { args, result, data, tool })
      await apps.used(app.id, text, lasting(args), how)
      // The answer is written once the page tells what the app shows (/api/say).
      send({ type: 'say', token: sayToken(text, data.summary) })
      return
    }
    // A new app: its data first, then the model writes it, and the answer
    // is written meanwhile.
    const make = plan.make ?? { name: 'New app', about: text, look: '', tools: [plan.tool] }
    const tools = [...new Set([plan.tool, ...(make.tools ?? [])])].filter(t => KNOWN.has(t))
    send({ type: 'making', name: make.name, kind: plan.kind, why: plan.why })
    const { result, data } = await run(plan.tool, args)
    const source = await model.make({
      request: text,
      plan: make,
      tool: plan.tool,
      args,
      data,
      tools: described(TOOLS.filter(t => tools.includes(t.name))),
    })
    app = await apps.put({ name: make.name, about: make.about, kind: plan.kind, tool: plan.tool, tools, source, args, examples: text, saved: false }, `made for: ${text}`)
    await apps.used(app.id, text, args, 'made')
    shown.set(app.id, { args, result, data, tool: plan.tool })
    send({ type: 'app', app: view(app), html: htmlOf(app), tool: plan.tool, args, result, how: 'made', why: plan.why })
    send({ type: 'say', token: sayToken(text, data.summary) })
  }

  // What an app was filled with where the page shows it (its shown row),
  // else what it showed last here, else its own arguments.
  async function filling(app, { shownId, talk } = {}) {
    const row = shownId && talk ? await apps.shownOf(shownId, talk) : null
    if (row && row.app === app.id) {
      const tool = app.tools.includes(row.tool) ? row.tool : app.tool
      const args = lasting(row.args)
      const { result, data } = await run(tool, args)
      return { args, result, data, tool }
    }
    const was = shown.get(app.id)
    if (was) return was
    const { result, data } = await run(app.tool, app.args)
    return { args: app.args, result, data, tool: app.tool }
  }

  // Changes the app a widget shows, with that widget's data. An answer of
  // what was found is one of many answers of the same app (Search
  // results), so its change is made to a copy of its own, kept in the
  // person's apps, and the other answers stay as they were.
  async function change(appId, text, send, at = {}) {
    let app = await apps.get(appId)
    const was = await filling(app, at)
    let copied = false
    if (app.tool === 'findings' && !app.id.startsWith('answer-')) {
      const topic = String(was.args?.query || text).replace(/\s+/g, ' ').trim().slice(0, 50)
      const when = new Date().toLocaleDateString('en-US', { month: 'short', day: 'numeric' })
      app = await apps.keepAnswer(app.id, { name: `${topic.charAt(0).toUpperCase()}${topic.slice(1)} · ${when}`, request: topic, args: was.args })
      copied = true
    }
    send({ type: 'status', text: `Changing ${app.name}` })
    const source = await model.edit({
      app,
      // A page made by hand is changed from its own code, so what it does (side by side, its sources) stays.
      source: fragmentOf(app.source, { prompt: true }),
      change: text,
      tool: was.tool,
      args: was.args,
      data: was.data,
      tools: described(TOOLS.filter(t => app.tools.includes(t.name))),
    })
    const next = await apps.change(app.id, source, text)
    // The widget's place in the conversation now shows the changed app.
    if (at.inPlace && at.shownId && at.talk) await apps.show({ id: at.shownId, talk: at.talk, app: next.id, version: next.version, tool: was.tool, args: was.args, how: 'changed' }).catch(log)
    send({ type: 'app', app: view(next), html: htmlOf(next), tool: was.tool, args: was.args, result: was.result, how: 'changed', newer: false, copied })
    return { app: next, copied, data: was.data }
  }

  // Back one version, as the version button does, with the widget's data.
  async function undo(appId, send, at = {}) {
    const app = await apps.undo(appId)
    const was = await filling(app, at)
    send({ type: 'app', app: view(app), html: htmlOf(app), tool: was.tool, args: was.args, result: was.result, how: 'undone', newer: true })
  }

  // Opens an app from the rail with what it last showed.
  async function open(appId, send) {
    const app = await apps.get(appId)
    const { result, data } = await run(app.tool, app.args)
    shown.set(app.id, { args: app.args, result, data, tool: app.tool })
    send({ type: 'app', app: view(app), html: htmlOf(app), tool: app.tool, args: app.args, result, how: 'opened' })
  }

  // An app shown earlier in a conversation, filled again with what it showed then.
  async function reopen(shownId, talk) {
    const row = await apps.shownOf(shownId, talk)
    if (!row) return null
    const app = await apps.get(row.app)
    if (!app) return { removed: true }
    const tool = app.tools.includes(row.tool) ? row.tool : app.tool
    // An entry asked for then is in what the app kept: it is not asked for again.
    const args = lasting(row.args)
    const { result, data } = await run(tool, args)
    shown.set(app.id, { args, result, data, tool })
    return { type: 'app', app: view(app), html: htmlOf(app), tool, args, result, how: 'again', shown: row.id }
  }

  const body = req =>
    new Promise((resolve, reject) => {
      let text = ''
      req.on('data', chunk => (text += chunk))
      req.on('end', () => {
        try {
          resolve(text ? JSON.parse(text) : {})
        } catch (error) {
          reject(error)
        }
      })
    })

  // A stream of events, one JSON a line, ending with done or error.
  async function stream(res, work) {
    res.writeHead(200, { 'content-type': 'application/x-ndjson', 'cache-control': 'no-store' })
    const send = event => res.write(`${JSON.stringify(event)}\n`)
    try {
      await work(send)
      send({ type: 'done' })
    } catch (error) {
      log(error)
      send({ type: 'error', text: error.message })
    }
    res.end()
  }

  const json = (res, value, code = 200) => res.writeHead(code, { 'content-type': 'application/json' }).end(JSON.stringify(value))

  const server = createServer(async (req, res) => {
    const url = new URL(req.url, 'http://localhost')
    const parts = url.pathname.split('/').filter(Boolean)
    try {
      if (req.method === 'GET' && url.pathname === '/') return res.writeHead(200, { 'content-type': 'text/html; charset=utf-8' }).end(page)
      if (req.method === 'GET' && url.pathname === '/healthz') return json(res, { ok: true, spend: model.budget?.status() })
      if (req.method === 'GET' && url.pathname === '/bridge.js') return res.writeHead(200, { 'content-type': 'text/javascript' }).end(bridgeJs)
      if (req.method === 'GET' && url.pathname === '/api/apps') return json(res, { apps: (await apps.saved()).map(view) })
      if (req.method === 'POST' && url.pathname === '/api/turn') {
        const input = await body(req)
        if (limited(req)) return stream(res, () => Promise.reject(new Error(tooMany)))
        return stream(res, async send => {
          const out = recording(own(input.talk), send)
          await turn(input, out)
          await out.written()
        })
      }
      // A conversation of this browser, read back after a reload or the next day.
      if (req.method === 'GET' && url.pathname === '/api/talk') {
        const talk = own(url.searchParams.get('talk'))
        return json(res, { turns: talk ? await apps.talk(talk) : [] })
      }
      if (req.method === 'POST' && url.pathname === '/api/talk') {
        const { talk, request, reply, shown: was = [] } = await body(req)
        if (!own(talk)) return json(res, { error: 'no conversation' }, 400)
        const tidyShown = (Array.isArray(was) ? was : []).slice(0, 8).map(w => ({
          id: String(w?.id ?? '').slice(0, 64), app: String(w?.app ?? '').slice(0, 64), name: String(w?.name ?? '').slice(0, 80),
          kind: String(w?.kind ?? '').slice(0, 12), how: String(w?.how ?? '').slice(0, 12), shows: String(w?.shows ?? '').slice(0, 400), in: Boolean(w?.in),
        }))
        await apps.talked({ talk: own(talk), request: String(request ?? '').slice(0, 2000), reply: String(reply ?? '').slice(0, 4000), shown: tidyShown })
        return json(res, { ok: true })
      }
      if (req.method === 'GET' && parts[0] === 'api' && parts[1] === 'shown' && parts[2]) {
        const event = await reopen(decodeURIComponent(parts[2]), own(url.searchParams.get('talk')))
        if (!event) return json(res, { error: 'not in this conversation' }, 404)
        return json(res, event)
      }
      // A tool an app calls from its view: only the tools it was made with.
      if (req.method === 'POST' && url.pathname === '/api/call') {
        const { app: appId, name, arguments: args, me } = await body(req)
        const app = await apps.get(appId)
        if (app && KEEPING.includes(name)) {
          const who = await owning(app.id, me)
          const refused = who && name === 'app_keep' ? await refusal(who, app.id, args) : ''
          if (refused) return json(res, { content: [{ type: 'text', text: refused }], isError: true })
          return json(res, await keeping(apps, app.id, name, args, who?.owner ?? ''))
        }
        // An app that books can cancel what it booked.
        const its = app && (app.tools.includes(name) || (name === 'table_cancel' && app.tools.includes('table_book')))
        if (!its) return json(res, { content: [{ type: 'text', text: `${name} is not one of this app's tools` }], isError: true })
        const acting = TOOLS.find(t => t.name === name)?.acts === true
        const who = acting ? await owning(app.id, me) : null
        if (who?.role === 'viewer') return json(res, { content: [{ type: 'text', text: 'Not done: this link can only look. Whoever shared it can.' }], isError: true })
        const { result, data } = await run(name, args)
        // What is done in an app stays with it (TOM-5): each booking, and
        // its cancelling, kept as bookings, which the app gets in data.kept.
        if (who && !result.isError && data?.code && (name === 'table_book' || name === 'table_cancel')) {
          const bookings = (await apps.kept(app.id, 'bookings', who.owner).catch(() => null)) ?? []
          const at = bookings.findIndex(b => b?.code === data.code)
          if (name === 'table_book' && at < 0) bookings.push({ id: data.code, code: data.code, name: data.name, restaurant: data.restaurant, date: data.date, time: data.time, party: data.party, guest: data.guest, booked: new Date().toISOString(), demo: true })
          if (name === 'table_cancel' && at >= 0) bookings[at] = { ...bookings[at], cancelled: new Date().toISOString() }
          await apps.keep(app.id, 'bookings', bookings.slice(-50), who.owner).catch(() => {})
        }
        // A call of its first tool (another range, another search) is what
        // it shows now; an action's answer (a booking) is not.
        if (!result.isError && name === app.tool) shown.set(app.id, { args, result, data, tool: name })
        // A tool that acts (a booking, the cart): the page has the chat say so.
        const acted = !result.isError && TOOLS.find(t => t.name === name)?.acts === true
        return json(res, acted ? { ...result, acted: true } : result)
      }
      // What the chat says under an app, from what the app shows on screen.
      if (req.method === 'POST' && url.pathname === '/api/say') {
        const { token, shows } = await body(req)
        const turned = sayable.get(String(token ?? ''))
        if (!turned) return json(res, { error: 'nothing to say' }, 404)
        sayable.delete(String(token))
        const { request, summary } = turned
        const said = await model.reply({ request: String(request ?? '').slice(0, 500), summary: String(summary ?? '').slice(0, 4000), shows: String(shows ?? '').slice(0, 6000) }).catch(() => '')
        return json(res, { text: said || String(summary ?? '') })
      }
      // What the chat says after something was done in an app.
      // A link that opens one app, with what it keeps, on another phone or
      // computer: whoever has it sees and changes the same kept values,
      // under the link's own key. What this browser kept is copied there
      // once; the link is as private as the person keeps it.
      if (req.method === 'POST' && url.pathname === '/api/share') {
        const { app: appId, me } = await body(req)
        const app = appId ? await apps.get(String(appId)) : null
        const who = app && (await owning(app.id, me))
        if (!who) return json(res, { error: 'no such app' }, 404)
        if (who.role === 'helper' || who.role === 'viewer') return json(res, { error: 'Only whoever shared it can share it further.' }, 403)
        let token = who.role === 'full' ? who.owner.slice(2) : ''
        if (!token) {
          token = randomBytes(16).toString('hex')
          for (const [key, value] of Object.entries(await apps.keptAll(app.id, who.owner))) await apps.keep(app.id, key, value, `s-${token}`)
        }
        // Its helper and view links, made once and the same each time after.
        let links = await apps.kept(app.id, '-links', `s-${token}`)
        if (!links) {
          links = { helper: randomBytes(16).toString('hex'), viewer: randomBytes(16).toString('hex') }
          await apps.keep(app.id, `link:${links.helper}`, { owner: `s-${token}`, role: 'helper' }, LINKS)
          await apps.keep(app.id, `link:${links.viewer}`, { owner: `s-${token}`, role: 'viewer' }, LINKS)
          await apps.keep(app.id, '-links', links, `s-${token}`)
        }
        return json(res, { token, ...links })
      }
      if (req.method === 'POST' && url.pathname === '/api/note') {
        const { app: appId, summary, history = [] } = await body(req)
        const app = await apps.get(appId)
        if (!app) return json(res, { error: 'no such app' }, 404)
        if (limited(req)) return json(res, { text: String(summary ?? '') })
        const said = model.note ? await model.note({ app: view(app), summary: String(summary ?? ''), history: history.slice(-6) }).catch(() => '') : ''
        return json(res, { text: said || String(summary ?? '') })
      }
      if (req.method === 'POST' && parts[0] === 'api' && parts[1] === 'apps' && parts[2]) {
        const appId = decodeURIComponent(parts[2])
        const input = await body(req)
        if (!(await apps.get(appId))) return json(res, { error: 'no such app' }, 404)
        if (parts[3] === 'change') return stream(res, send => (limited(req) ? Promise.reject(new Error(tooMany)) : change(appId, String(input.text ?? ''), send, { shownId: String(input.shown ?? ''), talk: own(input.talk), inPlace: true })))
        if (parts[3] === 'open') {
          return stream(res, async send => {
            const out = recording(own(input.talk), send)
            await open(appId, out)
            await out.written()
          })
        }
        // An answer of what was found, kept as its own app with these findings.
        if (parts[3] === 'keep-answer') {
          const row = await apps.shownOf(String(input.shown ?? ''), own(input.talk))
          const args = row?.args ?? shown.get(appId)?.args
          if (!args) return json(res, { error: 'that answer is not here any more' }, 404)
          const name = String(input.name ?? '').trim().slice(0, 60) || 'Kept answer'
          const kept = await apps.keepAnswer(appId, { name, request: String(input.request ?? name).slice(0, 300), args })
          return json(res, { app: view(kept) })
        }
        if (parts[3] === 'save') return json(res, { app: view(await apps.save(appId, true, input.name)) })
        if (parts[3] === 'unsave') return json(res, { app: view(await apps.save(appId, false)) })
        if (parts[3] === 'undo' || parts[3] === 'redo') {
          const app = parts[3] === 'undo' ? await apps.undo(appId) : await apps.redo(appId)
          return json(res, { app: view(app), html: htmlOf(app), newer: await apps.hasNewer(appId, app.version) })
        }
        if (parts[3] === 'remove') {
          await apps.remove(appId)
          return json(res, { removed: appId })
        }
      }
      res.writeHead(404).end()
    } catch (error) {
      log(error)
      json(res, { error: error.message }, 500)
    }
  })
  await new Promise(resolve => server.listen(port, host, resolve))
  return {
    url: `http://${host === '0.0.0.0' ? '127.0.0.1' : host}:${server.address().port}/`,
    apps,
    close() {
      server.close()
      apps.close()
    },
  }
}

function flag(name, fallback) {
  const at = process.argv.indexOf(name)
  return at === -1 ? fallback : process.argv[at + 1]
}

if (process.argv[1] && fileURLToPath(import.meta.url) === path.resolve(process.argv[1])) {
  const data = flag('--data', path.join(os.homedir(), '.local/share/instant-apps'))
  mkdirSync(data, { recursive: true })
  start({ port: Number(flag('--port', 8800)), host: flag('--host', '127.0.0.1'), data })
    .then(({ url }) => console.error(`Instant apps on ${url} (apps kept in ${data})`))
    .catch(error => {
      console.error(error.message)
      process.exit(1)
    })
}
