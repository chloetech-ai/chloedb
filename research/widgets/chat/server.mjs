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
import { AsyncLocalStorage } from 'node:async_hooks'
import { randomBytes } from 'node:crypto'
import { createServer } from 'node:http'
import { createRequire } from 'node:module'
import os from 'node:os'
import path from 'node:path'
import { fileURLToPath } from 'node:url'

import { described } from '../server/guide.mjs'
import { addFound, takeOut } from '../server/widgets/findings.mjs'
import { TOOLS, callTool, domainsOf, fragmentOf, render } from '../server/index.mjs'
import { KEEPING, keeping, changesWhatIsThere, isView, partOf, withPart, madeBy, withoutMade, hideIn, withHidden, mergeKeep, signed } from '../server/apps.mjs'
import { claude } from './model.mjs'
import { Apps, RUNNING, valueNames } from '../server/store.mjs'
import { Connections, selfOf } from '../server/connections.mjs'
import { NeedsConnect, SPOTIFY, spotifyFinish, spotifyStart } from '../server/widgets/spotify.mjs'
import { callOn, clientFor, closeClients, finishSignIn, registrySearch, toolsOf } from '../server/widgets/connector.mjs'

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
  const worker = readFileSync(new URL('./sw.js', import.meta.url), 'utf8')
  const bridgeJs = await bridge()
  // What each browser connected (its Spotify, MCP servers it signed in to),
  // and the sign-ins started and not finished yet, by their state.
  const connections = apps.db ? await Connections.open(apps.db, data ?? path.join(os.tmpdir(), 'instant-apps')).catch(error => (log(`no connections: ${error.message}`), null)) : null
  const signIns = new Map()
  const signingIn = (state, value) => {
    signIns.set(state, { ...value, at: Date.now() })
    for (const [k, v] of signIns) if (Date.now() - v.at > 900_000) signIns.delete(k)
  }
  // Who a request is for: the browser's own id (the page sends it as
  // x-self), and where this chat is reached from, for a sign-in's way back.
  const caller = new AsyncLocalStorage()
  const originOf = req => process.env.WIDGETS_ORIGIN || `${String(req.headers['x-forwarded-proto'] ?? 'http').split(',')[0]}://${req.headers['x-forwarded-host'] ?? req.headers.host}`
  const ctxOf = (self, origin) => {
    const me = selfOf(self)
    return {
      self: me,
      origin,
      account: service => (connections && me ? connections.account(me, service) : null),
      redirect: `${origin}/api/connect/mcp/done`,
      pending: (state, info) => signingIn(state, { self: me, kind: 'mcp', ...info }),
    }
  }
  const connectUrl = (c, ctx) => (c.service === 'spotify' ? `/api/connect/spotify?self=${ctx.self}` : c.url)
  // What each app shown in this process last drew, so a change redraws it
  // with the same data.
  const shown = new Map()
  // What each turn's answer is written from, once the page says what its
  // app shows (/api/say): by a one-time token the turn hands the page, so
  // the answer is part of the turn and not a request of its own.
  const sayable = new Map()
  const sayToken = (request, summary, before = '', kept = null) => {
    const token = randomBytes(12).toString('hex')
    sayable.set(token, { request, summary, before, kept, at: Date.now() })
    for (const [t, v] of sayable) if (Date.now() - v.at > 600_000) sayable.delete(t)
    return token
  }
  // What an app keeps for the person asking, all of it, not only what fits
  // on screen: the chat answers from it (round 10 TOM-4, LENA-2).
  const ownKeys = values => Object.fromEntries(Object.entries(values ?? {}).filter(([k]) => !/^(-|view:|made:|link:|args@|gone:)/.test(k)))
  async function keptFor(appId, links) {
    const token = links?.[appId]
    const owner = typeof token === 'string' && /^[0-9a-f]{32}$/.test(token) ? (await owning(appId, `s-${token}`).catch(() => null))?.owner : caller.getStore()?.self
    if (!owner) return null
    return { owner, values: ownKeys(await apps.keptAll(appId, owner).catch(() => ({}))) }
  }
  // What a turn changed in what an app keeps, in words: entries added,
  // taken out and changed, by name, or "nothing".
  function keptChange(before = {}, after = {}) {
    const label = e => (e && typeof e === 'object' ? String(Object.entries(e).find(([k, v]) => !/^(id|by|at)$/i.test(k) && (typeof v === 'string' ? v.trim() : typeof v === 'number'))?.[1] ?? e.id ?? '') : String(e ?? '')).slice(0, 40)
    const out = []
    for (const k of new Set([...Object.keys(before), ...Object.keys(after)])) {
      const a = before[k], b = after[k]
      if (JSON.stringify(a) === JSON.stringify(b)) continue
      if (Array.isArray(a) || Array.isArray(b)) {
        const key = e => (e && typeof e === 'object' && e.id != null ? `#${e.id}` : JSON.stringify(e))
        const was = new Map((a ?? []).map(e => [key(e), e])), now = new Map((b ?? []).map(e => [key(e), e]))
        const added = [...now.keys()].filter(x => !was.has(x)).map(x => label(now.get(x)))
        const gone = [...was.keys()].filter(x => !now.has(x)).map(x => label(was.get(x)))
        const edited = [...now.keys()].filter(x => was.has(x) && JSON.stringify(was.get(x)) !== JSON.stringify(now.get(x))).map(x => label(now.get(x)))
        if (added.length) out.push(`${k}: added ${added.slice(0, 8).join(', ')}`)
        if (gone.length) out.push(`${k}: took out ${gone.slice(0, 8).join(', ')}`)
        if (edited.length) out.push(`${k}: changed ${edited.slice(0, 8).join(', ')}`)
      } else out.push(`${k}: ${JSON.stringify(a ?? null).slice(0, 60)} to ${JSON.stringify(b ?? null).slice(0, 60)}`)
    }
    return out.join('; ').slice(0, 1500) || 'nothing'
  }
  // A turn's answer, written once from its token: from what the app shows
  // (the page says), what it was filled with and what the request changed
  // in what it keeps; kept with the turn too, so it is there on reopening.
  async function say(token, shows) {
    const turned = sayable.get(token)
    if (!turned) return null
    sayable.delete(token)
    const { request, summary, before, kept, turn: turnId } = turned
    const changed = kept ? keptChange(kept.before, ownKeys(await apps.keptAll(kept.app, kept.owner).catch(() => ({})))) : ''
    const said = await model.reply({ request: String(request ?? '').slice(0, 500), summary: String(summary ?? '').slice(0, 4000), shows: String(shows ?? '').slice(0, 6000), before: String(before ?? '').slice(0, 6000), changed }).catch(() => '')
    const text = said || String(summary ?? '')
    if (turnId && text) await apps.retalk(turnId, { reply: text.slice(0, 4000) }).catch(log)
    return { text }
  }
  // A browser's own ids (its conversation, its kept values): random, from the page.
  const own = value => (typeof value === 'string' && /^[A-Za-z0-9-]{16,64}$/.test(value) ? value : '')
  // Whose kept values a browser reaches for an app, and what it may do with
  // them: its own ("own"), or a share link's ("s-<token>"): the link that
  // made it changes everything ("full"), a helper link adds and ticks but
  // takes nothing out ("helper"), a view link only looks ("viewer"); a
  // link whose sharing was stopped reaches nothing ("gone").
  const LINKS = '-links-'
  // An entry by its first words ("Thu Apr 15 Évora"), for what a phone is told.
  const entryName = e => Object.entries(e ?? {}).filter(([k, v]) => !/^(id|by|at|date)$/.test(k) && typeof v === 'string' && v.trim()).slice(0, 2).map(([, v]) => v).join(' ') || String(e?.id ?? '')
  async function owning(appId, me) {
    const id = own(me)
    if (!id) return null
    if (!id.startsWith('s-')) return { owner: id, role: 'own' }
    const link = await apps.kept(appId, `link:${id.slice(2)}`, LINKS).catch(() => null)
    return link ? { owner: link.owner, role: link.role, token: id.slice(2), ...(link.name ? { name: link.name } : {}), ...(link.marks ? { marks: true } : {}), ...(link.only?.field ? { only: link.only } : {}), ...(link.hide?.length ? { hide: link.hide } : {}) } : { owner: id, role: 'full' }
  }
  const STOPPED = 'This link was stopped by whoever shared it: it opens nothing any more.'
  // What a request's links let it do to an app itself (change it, undo,
  // save, remove): only its owner, or a link that changes everything. A
  // page opened only by helper or view links (a guest) changes no app.
  async function mayChange(appId, input) {
    const token = input?.links?.[appId]
    if (typeof token === 'string' && /^[0-9a-f]{32}$/.test(token)) {
      const who = await owning(appId, `s-${token}`)
      if (who?.role === 'gone') return STOPPED
      if (who?.role === 'helper' || who?.role === 'viewer') return 'This link can’t change the app itself. Whoever shared it can.'
      return ''
    }
    return input?.guest ? 'This page has only what was shared with you, and can’t change apps.' : ''
  }
  // What a helper or view link may not keep: a view link changes nothing, a
  // helper link adds and ticks but changes and takes out nothing (an entry,
  // a field, a number).
  async function refusal(who, appId, args) {
    if (who.role === 'gone') return STOPPED
    if (isView(args?.key ?? '')) return ''
    if (who.role === 'viewer') return 'Not kept: this link can only look. Whoever shared it can change it.'
    if (who.role !== 'helper') return ''
    const key = String(args?.key ?? '')
    const before = hideIn(partOf(await apps.kept(appId, key, who.owner).catch(() => null), who.only), who.hide)
    // What the link added or filled itself it may go on changing (PRIYA-1, TOM-1).
    if (who.marks && marksMore(before, args?.value)) return 'Not kept: this link can only mark what is there done, not add or change it. Whoever shared it can.'
    const made = new Set((await madeOf(appId, who))[key] ?? [])
    return changesWhatIsThere(withoutMade(before, made), args?.value) ? 'Not kept: this link can add and tick, but not change or take out what is there. Whoever shared it can.' : ''
  }
  // What a "mark done only" link may not do beyond a helper's: add an entry,
  // or fill in anything but a mark (done, a status, a note on why) (round 10 MARCUS-6).
  const MARK = /(done|check|tick|status|mark|complete|finish|state|reason|why|note|comment|rain|late|by$|_at$|At$|^at$|^when$)/i
  function marksMore(before, after, depth = 0, key = '') {
    if (depth > 6 || after == null) return false
    if (Array.isArray(after)) {
      const was = Array.isArray(before) ? before : []
      const ids = new Map(was.filter(e => e && typeof e === 'object' && e.id != null).map(e => [String(e.id), e]))
      if (after.length > was.length && !(ids.size && after.every(e => e && typeof e === 'object' && ids.has(String(e.id))))) return true
      return after.some((e, i) => (e && typeof e === 'object' && e.id != null ? !ids.has(String(e.id)) || marksMore(ids.get(String(e.id)), e, depth + 1) : marksMore(was[i], e, depth + 1, key)))
    }
    if (typeof after === 'object') return Object.entries(after).some(([k, v]) => marksMore(before && typeof before === 'object' ? before[k] : undefined, v, depth + 1, k))
    const empty = before == null || before === '' || before === false
    return empty && after !== '' && after !== false && !MARK.test(key)
  }
  const made_ = async (appId, token) => (token ? ((await apps.kept(appId, `made:${token}`, LINKS).catch(() => null)) ?? {}) : {})
  // What a person's links made, whichever link it was (MARCUS-3).
  const madeName = name => `made:name:${String(name).trim().toLowerCase()}`
  async function madeOf(appId, who) {
    const out = { ...(await made_(appId, who.token)) }
    if (who.name) for (const [k, v] of Object.entries((await apps.kept(appId, madeName(who.name), LINKS).catch(() => null)) ?? {})) out[k] = [...new Set([...(out[k] ?? []), ...v])]
    return out
  }
  // A link for part of an app (one crew) adds only to its part, and is
  // told so, rather than having it put in its own part unasked (MARCUS-1).
  async function outOfPart(who, appId, args) {
    if (!who.only?.field || who.only.field === '*' || isView(args?.key ?? '')) return ''
    const before = await apps.kept(appId, String(args?.key ?? ''), who.owner).catch(() => null)
    const value = args?.value
    if (!Array.isArray(value) || !Array.isArray(before)) return ''
    const was = new Set(before.map(e => e?.id).filter(id => id != null).map(String))
    const want = String(who.only.value).trim().toLowerCase()
    const other = value.find(e => e && typeof e === 'object' && (e.id == null || !was.has(String(e.id))) && e[who.only.field] != null && String(e[who.only.field]).trim() !== '' && String(e[who.only.field]).trim().toLowerCase() !== want)
    return other ? `Not kept: this link adds only to ${who.only.value}, not to ${other[who.only.field]}. Whoever shared it can.` : ''
  }
  const unsigned = v => (Array.isArray(v) ? v.map(unsigned) : v && typeof v === 'object' ? Object.fromEntries(Object.entries(v).filter(([k]) => k !== 'by').map(([k, x]) => [k, unsigned(x)])) : v)
  // Two entries with one number, added on two phones: kept with the app
  // (not with either phone) until the list has one of them again (LENA-2).
  async function keepClashes(appId, owner, key, said) {
    const both = said.filter(c => c.both && c.number)
    if (!both.length) return
    const was = (await apps.kept(appId, `clash:${owner}`, LINKS).catch(() => null)) ?? []
    const next = [...was.filter(c => !both.some(b => b.number === c.number && b.value === c.value && key === c.key)), ...both.map(b => ({ key, number: b.number, value: b.value, at: Date.now() }))].slice(-20)
    await apps.keep(appId, `clash:${owner}`, next, LINKS)
  }
  async function clashesOf(appId, owner, values, reader = '') {
    const was = (await apps.kept(appId, `clash:${owner}`, LINKS).catch(() => null)) ?? []
    if (!was.length) return []
    const found = (v, c, out = [], depth = 0) => {
      if (depth > 5 || !v || typeof v !== 'object') return out
      if (!Array.isArray(v) && v[c.number] != null && String(v[c.number]).trim() === c.value) out.push(v)
      for (const x of Object.values(v)) found(x, c, out, depth + 1)
      return out
    }
    const money = n => (typeof n === 'number' ? n.toFixed(2) : String(n))
    const what = e => {
      const amount = Object.entries(e).find(([k, x]) => typeof x === 'number' && /amount|price|total|cost|sum|value/i.test(k))?.[1]
      // Both sides named: an entry with no name is the owner's (round 9, the clash note).
      return `${amount != null ? money(amount) : nameOfEntry(e)} by ${e.by || reader || 'the owner'}`
    }
    const open = []
    for (const c of was) {
      const twice = found(values[c.key], c)
      if (twice.length >= 2) open.push({ ...c, text: `${c.value} is in ${c.key} ${twice.length} times (${twice.map(what).join(', ')}): check which is right`, choices: twice.filter(e => e.id != null).map(e => ({ id: String(e.id), label: what(e) })) })
    }
    if (open.length !== was.length) await apps.keep(appId, `clash:${owner}`, was.filter(c => open.some(o => o.number === c.number && o.value === c.value && o.key === c.key)), LINKS).catch(() => {})
    return open
  }
  // Names of a person the app wrote itself on entries it put in or
  // changed (by, ticked_by), taken off.
  function unsignAuto(before, value, name, depth = 0) {
    if (depth > 4 || !value || typeof value !== 'object') return value
    if (Array.isArray(value)) {
      const was = new Map((Array.isArray(before) ? before : []).filter(e => e && e.id != null).map(e => [String(e.id), e]))
      return value.map(e => {
        if (!e || typeof e !== 'object' || Array.isArray(e)) return e
        const old = e.id != null ? was.get(String(e.id)) : undefined
        const out = { ...e }
        for (const k of Object.keys(out)) if ((k === 'by' || /_by$/i.test(k)) && out[k] === name && old?.[k] !== name) delete out[k]
        return out
      })
    }
    return Object.fromEntries(Object.entries(value).map(([k, v]) => [k, unsignAuto(before && typeof before === 'object' ? before[k] : undefined, v, name, depth + 1)]))
  }
  const nameOfEntry = e => (e && typeof e === 'object' ? String(Object.entries(e).find(([k, v]) => !/^(id|by|at)$/i.test(k) && typeof v === 'string' && v.trim() && !/^\d{4}-\d\d-\d\d/.test(v))?.[1] ?? e.id ?? '') : String(e ?? '')).slice(0, 40)
  // Keeps of one value go one after another, in the order they came: the
  // keys of a word typed fast arrive close together (PRIYA-1).
  const queues = new Map()
  function inTurn(name, fn) {
    const run = (queues.get(name) ?? Promise.resolve()).then(fn, fn)
    const tail = run.catch(() => {})
    queues.set(name, tail)
    tail.then(() => queues.get(name) === tail && queues.delete(name))
    return run
  }
  // Pages that have a shared app open hear when someone else keeps
  // something in it, and draw it again (MARCUS-4, LENA-5).
  const watchers = new Map()
  const told = (appId, owner, self, said = []) => {
    for (const w of watchers.get(`${appId}|${owner}`) ?? []) w({ app: appId, self, ...(said.length ? { said } : {}) })
  }
  // A new version of an app reaches every phone that has it open by a link,
  // whoever's it is: it draws the new one and says so (MAYA-3).
  apps.onput = (appId, version, change) => {
    for (const [k, set] of watchers) if (k.startsWith(`${appId}|`)) for (const w of set) w({ app: appId, version, change: String(change ?? 'an earlier version brought back').replace(/^(made for|changed|change):\s*/i, '').slice(0, 120) })
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
    const ctx = caller.getStore()
    try {
      const data = await callTool(tool, args, ctx)
      return { result: { content: [{ type: 'text', text: data.summary }], structuredContent: data }, data }
    } catch (error) {
      // The person's own account, not connected yet: the page offers the sign-in.
      if (error instanceof NeedsConnect) {
        const connect = { ...error.connect, url: connectUrl(error.connect, ctx ?? {}) }
        return { result: { content: [{ type: 'text', text: error.message }], isError: true, connect }, data: { summary: `${tool}: ${error.message}` }, connect }
      }
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
      ...(w.unshareable ? { unshareable: true } : {}),
    }))

  // An entry the person asked the chat to put into an app (values.add):
  // each gets an id, so an app that opens again with it adds it once.
  function entries(args) {
    const values = args?.values
    if (!values || typeof values !== 'object' || values.add == null) return args
    // A number goes in as it was typed: "MM-21240", never "#MM-21240", so
    // the app's own check for the same number finds it (round 10 LENA-1).
    const NUMBERED = /(number|^no$|^num|invoice|^ref|reference|code|order|receipt|bill)/i
    const plainNumbers = e => Object.fromEntries(Object.entries(e).map(([k, v]) => [k, NUMBERED.test(k) && typeof v === 'string' ? v.replace(/^\s*(#|no\.?\s|nº\s?)\s*/i, '') : v]))
    const add = (Array.isArray(values.add) ? values.add : [values.add]).filter(e => e && typeof e === 'object').map(e => plainNumbers({ id: String(e.id ?? randomBytes(5).toString('hex')), ...e }))
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
  // A request that points at a kept list or plan, and one that asks for
  // something to be looked up for it.
  const KEPT_REF = /\b(kept|shortlist|(this|my|our|the) (list|plan|answer)|keep (the|my|our|everyone'?s|all the) notes)\b/i
  const LOOKUP = /\b(look(ing)? up|search for|check|store price|prices?|in stock|add|more options?|another|latest|current|compare|include)\b/i
  // What an app opens with next time: not the entries asked for once, nor
  // one taken out once (LENA-1: a take-out kept here ran on every open).
  function lasting(args) {
    const values = args?.values
    if (!values || typeof values !== 'object' || (values.add == null && values.edit == null && values.remove == null)) return args
    const { add, edit, remove, ...rest } = values
    return { ...args, values: rest }
  }

  // Not connected yet: the person gets the sign-in, and the page asks again
  // once it is done.
  function needsConnect(connect, text, send) {
    send({ type: 'connect', service: connect.service, name: connect.name, url: connect.url, request: text, ...(connect.thirdParty ? { thirdParty: true, title: connect.title, host: connect.host, asked: connect.asked } : {}) })
    send({ type: 'reply', text: connect.thirdParty
      ? `I found no connector made by ${connect.asked ?? "the service"} itself, only one made by someone else (${connect.title ?? connect.name}). If you'd rather not sign in to it, paste or type what you need into an app instead, and I'll work from that.`
      : `This needs your ${connect.name}. Connect it with the button above, and I'll pick up from there.` })
  }

  async function turn({ text, history = [], lastApp, screen, talk, links, guest }, send) {
    // A page opened only by helper or view links has no chat: what was
    // shared is all it gets (INES-1, LENA-5).
    if (guest) {
      send({ type: 'reply', text: 'This page has only what was shared with you. Use the app itself; whoever shared it can do the rest.' })
      return
    }
    send({ type: 'status', text: 'Looking through your apps' })
    const candidates = await apps.recall(text, 8)
    send({ type: 'recall', candidates: candidates.map(c => ({ id: c.id, name: c.name, score: c.score })) })
    const last = lastApp ? await apps.get(lastApp.id) : null
    let plan = await model.route({
      request: text,
      history: history.slice(-6),
      candidates,
      last: last && { ...last, args: lastApp.args ?? last.args, reads: last.tool === 'inputs' ? valueNames(last.source, lastApp.args ?? last.args) : undefined },
      // An app this browser shared is kept for everyone with its link, so
      // the chat can say so (MAYA-4, MARCUS-2).
      screen: await Promise.all(onScreen(screen).map(async (w, i) => {
        // The three newest apps carry all they keep, also what is scrolled
        // out of sight; a private one carries nothing (round 10 TOM-4).
        const holds = i < 3 && !/^\(private/.test(w.shows) ? await keptFor(w.id, links) : null
        const all = holds && Object.keys(holds.values).length ? JSON.stringify(holds.values).slice(0, i === 0 ? 12000 : 4000) : ''
        return { ...w, ...(links?.[w.id] ? { shared: true } : {}), ...(w.id.startsWith('answer-') || (await apps.get(w.id).catch(() => null))?.saved ? { kept: true } : {}), ...(all ? { holds: all } : {}) }
      })),
      tools: described(TOOLS),
      connected: await connectedOf(caller.getStore()?.self),
    })
    send({ type: 'route', mode: plan.mode, kind: plan.kind, why: plan.why })
    // A change asked in words is the same change as the app's Change box,
    // and an undo the same as its version button: done, never just said.
    if ((plan.mode === 'change' || plan.mode === 'undo') && plan.app && (await apps.get(plan.app))) {
      const refused = await mayChange(plan.app, { links })
      if (refused) {
        send({ type: 'reply', text: refused })
        return
      }
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
      // What it showed before the change, so the answer compares the two
      // and says "not done" only when what was asked is the same in both,
      // and names what the change left out (TOM-3, TOM-4).
      const before = onScreen(screen).find(w => w.id === plan.app)?.told
      const { app: changed, copied, data } = await change(plan.app, what, send, target)
      // Said from what the changed app shows, not taken for granted.
      const copy = copied ? ` It is a copy of that answer, kept in the person's apps as "${changed.name}"; other answers are as they were.` : ''
      send({ type: 'say', token: sayToken(text, `${changed.name} was rewritten as version ${changed.version} for: ${what}.${copy} Its Undo button takes the change back. What it was filled with: ${data?.summary ?? ''}`, before) })
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
    // But something to look up for that answer ("find its store price") is for it (PRIYA-4).
    const forIt = LOOKUP.test(text) && /\b(its|it|this|that|these|them|those|kept)\b/i.test(text)
    if (app?.id.startsWith('answer-') && plan.tool === 'findings' && !forIt && !sameSearch(plan.args?.query, app.args?.query, app.args?.request, app.name)) app = await apps.get('search-results')
    // A request about a kept list on screen ("change this kept list", "keep
    // the notes") is about that list, not a new answer beside it (PRIYA-1, TOM-3).
    if (plan.mode === 'use' && plan.tool === 'findings' && !app?.id.startsWith('answer-') && KEPT_REF.test(text)) {
      const there = onScreen(screen).find(w => w.id.startsWith('answer-'))
      if (there && !(await mayChange(there.id, { links }))) app = (await apps.get(there.id)) ?? app
    }
    const keptAnswer = app?.id.startsWith('answer-')
    // Something to find for a kept answer (a store price, two more options)
    // is looked up, and the kept answer itself takes it: its notes, links
    // and name stay, and every phone with it open gets it (PRIYA-1, PRIYA-4).
    // What was found is added to the answer, never put in its place: no
    // option, price or source kept drops off (round 9 PRIYA-1, PRIYA-3).
    // An option taken off a kept answer by asking goes, in place, as a new
    // version Undo brings back; one it can't tell is said (round 9 PRIYA-4).
    const TAKE = /\b(take|taking|remove|drop|cross|delete|get rid of)\b[^.?!]*\b(off|out|from)\b|\b(remove|delete|drop)\b/i
    const taking = keptAnswer && TAKE.test(text) && !(await mayChange(app.id, { links })) ? takeOut(app.args, text) : null
    if (taking && !taking.gone.length && /\b(off|out of)\b/i.test(text)) {
      send({ type: 'reply', text: `I couldn't tell which option of ${app.name} to take off: say its name as the list shows it.` })
      return
    }
    if (taking?.gone.length) {
      app = await apps.put({ ...app, args: taking.kept, version: app.version + 1 }, `took off: ${taking.gone.join(', ')}`.slice(0, 300))
      plan = { ...plan, tool: 'findings', mode: 'use' }
    } else if (keptAnswer && plan.tool === 'findings' && LOOKUP.test(text) && !(await mayChange(app.id, { links }))) {
      send({ type: 'status', text: 'Searching the web' })
      try {
        const found = await model.research({ request: text, query: String(app.args?.query || text), history: history.slice(-6), found: app.args, changes: await apps.changesOf(app.id).catch(() => []) })
        if (found.documents?.length || found.items?.length) app = await apps.put({ ...app, args: addFound(app.args, found, { adding: /\b(more|another|other|alternatives?|new options?|add (an?|another|more)|also (find|look))\b/i.test(text) }), version: app.version + 1 }, `found again for: ${text}`.slice(0, 300))
      } catch (error) {
        send({ type: 'reply', text: `I could not search the web just now: ${error.message}` })
        return
      }
    }
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
    // A service outside the chat: an MCP server found for it and
    // connected, and what its tool answered is the app's data.
    if (plan.tool === 'connector' && !keptAnswer) {
      const step = await connecting({ text, service: String(args.service || text), server: args.server, history }, send)
      if (step.connect) return needsConnect(step.connect, text, send)
      if (step.reply) return send({ type: 'reply', text: step.reply })
      args = step.args
    }
    if (app) {
      send({ type: 'status', text: `Opening ${app.name}` })
      // What it keeps before this request, to say afterwards what it changed.
      const asked = args?.values && ['add', 'edit', 'remove'].some(k => args.values[k] != null)
      const keptNow = asked && !keptAnswer ? await keptFor(app.id, links) : null
      const tool = app.tools.includes(plan.tool) && !keptAnswer ? plan.tool : app.tool
      const { result, data, connect } = await run(tool, args)
      if (connect) return needsConnect(connect, text, send)
      // The app just shown, filled again for a follow-up, is an update.
      const how = lastApp?.id === app.id ? 'updated' : app.saved ? 'recalled' : 'again'
      send({ type: 'app', app: view(app), html: htmlOf(app), tool, args, result, how, why: plan.why })
      shown.set(app.id, { args, result, data, tool })
      // Filled by a helper's or a viewer's link: on their screen only, never
      // what the app opens with for everyone (MAYA-2).
      if (!(await mayChange(app.id, { links }))) await apps.used(app.id, text, lasting(args), how)
      // The answer is written once the page tells what the app shows (/api/say),
      // with what the request changed in what it keeps (round 10 LENA-2, INES-4).
      send({ type: 'say', token: sayToken(text, data.summary, '', keptNow ? { app: app.id, owner: keptNow.owner, before: keptNow.values } : null) })
      return
    }
    // A new app: its data first, then the model writes it, and the answer
    // is written meanwhile.
    const make = plan.make ?? { name: 'New app', about: text, look: '', tools: [plan.tool] }
    const tools = [...new Set([plan.tool, ...(make.tools ?? [])])].filter(t => KNOWN.has(t))
    // A tool of the person's own account runs before the app is made, so a
    // sign-in comes first.
    const first = await run(plan.tool, args)
    if (first.connect) return needsConnect(first.connect, text, send)
    const { result, data } = first
    send({ type: 'making', name: make.name, kind: plan.kind, why: plan.why })
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

  // What a browser connected, as the router reads it: names and addresses, no tokens.
  async function connectedOf(self) {
    if (!connections || !self) return []
    return (await connections.list(self).catch(() => [])).map(c => ({ service: c.service.startsWith('mcp:') ? 'mcp' : c.service, name: c.label, url: c.service.startsWith('mcp:') ? c.service.slice(4) : undefined }))
  }

  // The connector's step: servers for the service (the person's own first,
  // then the registry's), the ones that fit put in order by the model, the
  // first that connects asked with one tool that only reads.
  async function connecting({ text, service, server, history }, send) {
    const ctx = caller.getStore() ?? {}
    send({ type: 'status', text: `Looking for a connector for ${service}` })
    const mine = (await connectedOf(ctx.self)).filter(c => c.service === 'mcp' && c.url)
    const found = await registrySearch(service).catch(() => [])
    const all = [...(server?.url ? [{ name: server.name ?? server.url, title: server.title ?? server.name ?? null, url: server.url }] : []), ...mine.map(c => ({ name: c.name, title: c.name, url: c.url, about: 'connected by this person before' })), ...found]
    const servers = [...new Map(all.map(s => [s.url, s])).values()].slice(0, 15)
    if (!servers.length) return { reply: `I couldn't find a connector for ${service} in the public MCP registry.` }
    const picked = await model.connector({ request: text, service, history: history.slice(-4), servers: servers.map(({ name, title, about, url }) => ({ name, title, about, url })) })
    const order = (picked.servers ?? []).map(i => servers[i]).filter(Boolean)
    if (!order.length) return { reply: picked.reply || `None of the connectors I found for ${service} fits this.` }
    let last = ''
    // One that wants a sign-in waits while the next ones that fit are
    // tried: a forecast needs no account, a person's own notes do.
    let signIn = null
    for (const s of order.slice(0, 3)) {
      send({ type: 'status', text: `Connecting to ${s.title ?? s.name}` })
      let client
      try {
        client = await clientFor(ctx, s)
      } catch (error) {
        if (error instanceof NeedsConnect) {
          // Who makes it, when not the service itself: a stranger's server
          // is never offered as if it were the service's own (round 9 DEV-4).
          if (!signIn) {
            const host = (() => { try { return new URL(s.url).hostname } catch { return '' } })()
            // By the site's own name, not a part before it: github.robinsonaisystems.com is Robinson's, not GitHub's (round 10 DEV-2).
            const site = host.toLowerCase().split('.').slice(-2)[0] ?? ''
            const own_ = site && String(service).toLowerCase().split(/[^a-z0-9]+/).some(w => w.length > 2 && site.includes(w))
            signIn = { ...error.connect, host, asked: service, ...(own_ ? {} : { thirdParty: true, title: s.title ?? s.name }) }
          }
          continue
        }
        last = `${s.title ?? s.name}: ${error.message}`
        log(`connector ${s.url}: ${error.message}`)
        continue
      }
      const tools = await toolsOf(client).catch(error => ((last = error.message), []))
      if (!tools.length) continue
      // Kept, so the next request about it goes straight there.
      if (ctx.account && !(await ctx.account(`mcp:${s.url}`)?.get())) await ctx.account(`mcp:${s.url}`)?.set({ url: s.url, title: s.title ?? s.name, open: true }, s.title ?? s.name).catch(() => {})
      // Up to three calls that only read, each seeing what the ones before
      // answered: a city's coordinates, then its forecast.
      const steps = []
      let result = null
      let tool = null
      let note = null
      for (let n = 0; n < 3; n++) {
        const done = steps.map(x => ({ tool: x.tool, arguments: x.arguments, result: JSON.stringify(x.result).slice(0, 4000) }))
        const use = await model.connectorTool({ request: text, history: history.slice(-4), server: s, tools, done })
        const next = tools.find(t => t.name === use.tool)
        if (!next) {
          note = steps.length ? note : (use.note ?? null)
          break
        }
        if (!next.reads) {
          note = `${next.name} changes something, so it waits for a tap in the app.`
          break
        }
        if (steps.some(x => x.tool === next.name && JSON.stringify(x.arguments) === JSON.stringify(use.arguments ?? {}))) break
        send({ type: 'status', text: `Asking ${s.title ?? s.name}` })
        try {
          result = await callOn(ctx, s, next.name, use.arguments ?? {})
          tool = next
          steps.push({ tool: next.name, arguments: use.arguments ?? {}, result })
        } catch (error) {
          if (error instanceof NeedsConnect) return { connect: error.connect }
          note = `${next.name} failed: ${error.message}`
          break
        }
      }
      // A server none of whose tools answers this (another city's weather
      // only) gives way to the next one that fits.
      if (!steps.length && !/waits for a tap/.test(note ?? '') && s !== order.slice(0, 3).at(-1)) {
        last = `${s.title ?? s.name}: ${note ?? 'no tool fits'}`
        continue
      }
      return { args: { request: text, service, server: { name: s.name, title: s.title ?? null, url: s.url, about: s.about ?? null, type: s.type ?? null, website: s.website ?? null }, tool: tool?.name ?? null, arguments: steps.at(-1)?.arguments ?? null, result, ...(steps.length > 1 ? { steps: steps.slice(0, -1).map(x => ({ tool: x.tool, arguments: x.arguments, result: x.result })) } : {}), tools: tools.map(({ name, about, takes, reads }) => ({ name, about, takes, reads })), note } }
    }
    if (signIn) return { connect: signIn }
    return { reply: `I couldn't connect to a server for ${service}${last ? ` (${last})` : ''}.` }
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
  async function open(appId, send, who = null) {
    const app = await apps.get(appId)
    // A link for part of it opens with only that part of what it was made
    // with too: a phone with no signal draws it from there (round 9 MARCUS-1).
    const args = who?.only || who?.hide ? forLink(app.args, who) : app.args
    const { result, data } = await run(app.tool, args)
    if (args === app.args) shown.set(app.id, { args, result, data, tool: app.tool })
    send({ type: 'app', app: view(app), html: htmlOf(app), tool: app.tool, args, result, how: 'opened' })
  }
  // What a link for part of an app may get of its values: its part of each
  // list, no field it is kept from, and, for "what is shared with them",
  // none of the owner's own figures (a budget) (round 9 INES-5, MARCUS-1).
  function forLink(args, who) {
    const values = args?.values
    if (!values || typeof values !== 'object' || Array.isArray(values)) return args
    return { ...args, values: seenBy(values, who) }
  }
  function seenBy(values, who) {
    const figures = v => typeof v === 'number' || (v && typeof v === 'object' && !Array.isArray(v) && Object.values(v).length > 0 && Object.values(v).every(x => typeof x === 'number'))
    return Object.fromEntries(Object.entries(values)
      .filter(([k, v]) => !(who.only?.field === '*' && figures(v) && !isView(k)))
      .filter(([k]) => !(who.hide ?? []).some(h => String(h).toLowerCase() === String(k).toLowerCase()))
      .map(([k, v]) => [k, hideIn(partOf(v, who.only), who.hide)]))
  }

  // An app shown earlier in a conversation, filled again with what it showed then.
  async function reopen(shownId, talk) {
    const row = await apps.shownOf(shownId, talk)
    if (!row) return null
    const app = await apps.get(row.app)
    if (!app) return { removed: true }
    const tool = app.tools.includes(row.tool) ? row.tool : app.tool
    // An entry asked for then is in what the app kept: it is not asked for
    // again. A kept answer opens as it is now (found again in place, PRIYA-4).
    const args = app.id.startsWith('answer-') ? app.args : lasting(row.args)
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

  // A page the sign-in ends on: it tells the chat's tab, which asks again.
  const signedIn = (res, ok, text) =>
    res.writeHead(ok ? 200 : 400, { 'content-type': 'text/html; charset=utf-8', 'cache-control': 'no-store' }).end(`<!doctype html><meta name="viewport" content="width=device-width,initial-scale=1"><title>${ok ? 'Connected' : 'Not connected'}</title>
<body style="font:16px/1.5 system-ui,sans-serif;max-width:32rem;margin:15vh auto;padding:0 1rem;color:#222"><h1 style="font-size:1.3rem">${ok ? 'Connected' : 'Not connected'}</h1><p>${text.replace(/[&<>"]/g, c => `&#${c.charCodeAt(0)};`)}</p><p><a href="/">Back to the chat</a></p>
<script>try{new BroadcastChannel('instant-apps-connect').postMessage({ok:${ok ? 'true' : 'false'}})}catch{};try{localStorage.setItem('instant-apps-connected',String(Date.now()))}catch{};${ok ? 'setTimeout(()=>{try{window.close()}catch{}},1200)' : ''}</script>`)

  async function connectRoutes(req, res, url) {
    const ctx = caller.getStore()
    if (!connections) return json(res, { error: 'connections are not kept here' }, 503)
    if (req.method === 'GET' && url.pathname === '/api/connections') return json(res, { connected: await connectedOf(ctx.self), spotify: Boolean(SPOTIFY.client()) })
    if (req.method === 'POST' && url.pathname === '/api/disconnect') {
      const { service } = await body(req)
      if (!ctx.self || typeof service !== 'string') return json(res, { error: 'nothing to disconnect' }, 400)
      await connections.remove(ctx.self, service.slice(0, 2100))
      return json(res, { ok: true })
    }
    if (req.method === 'GET' && url.pathname === '/api/connect/spotify') {
      if (!SPOTIFY.client()) return signedIn(res, false, 'Spotify is not set up on this chat yet: it needs a Spotify app’s client id (SPOTIFY_CLIENT_ID).')
      if (!ctx.self) return signedIn(res, false, 'Open this from the chat.')
      const start = spotifyStart(`${ctx.origin}/api/connect/spotify/done`)
      signingIn(start.state, { kind: 'spotify', self: ctx.self, verifier: start.verifier })
      return res.writeHead(302, { location: start.url, 'cache-control': 'no-store' }).end()
    }
    if (req.method === 'GET' && url.pathname === '/api/connect/spotify/done') {
      const state = url.searchParams.get('state') ?? ''
      const pending = signIns.get(state)
      signIns.delete(state)
      if (!pending || pending.kind !== 'spotify') return signedIn(res, false, 'This sign-in ran out. Try again from the chat.')
      if (url.searchParams.get('error')) return signedIn(res, false, `Spotify said: ${url.searchParams.get('error')}.`)
      try {
        const token = await spotifyFinish(url.searchParams.get('code') ?? '', pending.verifier, `${ctx.origin}/api/connect/spotify/done`)
        await connections.set(pending.self, 'spotify', token, `Spotify (${token.label})`)
        return signedIn(res, true, `Your Spotify (${token.label}) is connected to the chat. You can close this tab.`)
      } catch (error) {
        return signedIn(res, false, error.message)
      }
    }
    if (req.method === 'GET' && url.pathname === '/api/connect/mcp/done') {
      const state = url.searchParams.get('state') ?? ''
      const pending = signIns.get(state)
      signIns.delete(state)
      if (!pending || pending.kind !== 'mcp') return signedIn(res, false, 'This sign-in ran out. Try again from the chat.')
      if (url.searchParams.get('error')) return signedIn(res, false, `${pending.title ?? 'The service'} said: ${url.searchParams.get('error_description') ?? url.searchParams.get('error')}.`)
      try {
        await finishSignIn({ ...ctxOf(pending.self, ctx.origin) }, pending.url, url.searchParams.get('code') ?? '')
        return signedIn(res, true, `${pending.title ?? new URL(pending.url).hostname} is connected to the chat. You can close this tab.`)
      } catch (error) {
        return signedIn(res, false, error.message)
      }
    }
    return json(res, { error: 'not found' }, 404)
  }

  const handle = async (req, res) => {
    const url = new URL(req.url, 'http://localhost')
    const parts = url.pathname.split('/').filter(Boolean)
    try {
      if (req.method === 'GET' && url.pathname === '/') return res.writeHead(200, { 'content-type': 'text/html; charset=utf-8' }).end(page)
      if (req.method === 'GET' && url.pathname === '/sw.js') return res.writeHead(200, { 'content-type': 'text/javascript; charset=utf-8', 'cache-control': 'no-cache' }).end(worker)
      if (req.method === 'GET' && url.pathname === '/healthz') return json(res, { ok: true, spend: model.budget?.status() })
      if (req.method === 'GET' && url.pathname === '/bridge.js') return res.writeHead(200, { 'content-type': 'text/javascript' }).end(bridgeJs)
      if (req.method === 'GET' && url.pathname === '/api/apps') return json(res, { apps: (await apps.saved()).map(view) })
      // The person's own accounts: what this browser connected, its sign-ins, and taking one away.
      if (url.pathname.startsWith('/api/connect') || url.pathname === '/api/connections' || url.pathname === '/api/disconnect') return connectRoutes(req, res, url)
      if (req.method === 'POST' && url.pathname === '/api/turn') {
        const input = await body(req)
        if (limited(req)) return stream(res, () => Promise.reject(new Error(tooMany)))
        // A browser closed before the answer came: the turn goes on, and
        // what it answered is kept in the conversation, so the request is
        // there when it opens again (round 9 LENA-3).
        let left = false
        res.on('close', () => { if (!res.writableEnded) left = true })
        return stream(res, async send => {
          let reply = ''
          let saying = null
          const shownHere = []
          // The request is in the conversation from the start, as still
          // running, so a page opened meanwhile shows it (round 10 INES-6,
          // LENA-4, MARCUS-7); the page's own record fills the same row.
          const talk = own(input.talk)
          const turnId = talk ? await apps.talked({ talk, request: String(input.text ?? '').slice(0, 2000), reply: RUNNING, shown: [] }).catch(log) : null
          if (turnId) send({ type: 'turn', id: turnId })
          const out = recording(talk, event => {
            if (event.type === 'reply') reply = String(event.text ?? '')
            if (event.type === 'app') shownHere.push({ id: event.shown ?? '', app: event.app?.id ?? '', name: event.app?.name ?? '', kind: event.app?.kind ?? '', how: event.how ?? '', shows: '', in: false })
            if (event.type === 'say') {
              saying = event.token
              const v = sayable.get(event.token)
              if (v) v.turn = turnId
            }
            if (!left) send(event)
          })
          try {
            await turn(input, out)
            await out.written()
          } finally {
            // A page closed before the answer was written: it is written here,
            // from what the app was filled with and what the request changed.
            if (left && saying && !reply) reply = (await say(saying, '').catch(() => null))?.text ?? ''
            // Closed just after (the answer not asked for yet): written here a little later.
            else if (saying) setTimeout(() => sayable.has(saying) && say(saying, '').catch(log), 20_000).unref?.()
            if (turnId) await apps.retalk(turnId, { reply: reply.slice(0, 4000), shown: shownHere.slice(0, 8) }).catch(log)
          }
        })
      }
      // A conversation of this browser, read back after a reload or the next day.
      if (req.method === 'GET' && url.pathname === '/api/talk') {
        const talk = own(url.searchParams.get('talk'))
        return json(res, { turns: talk ? await apps.talk(talk) : [] })
      }
      if (req.method === 'POST' && url.pathname === '/api/talk') {
        const { talk, turn: turnId, request, reply, shown: was = [] } = await body(req)
        if (!own(talk)) return json(res, { error: 'no conversation' }, 400)
        const tidyShown = (Array.isArray(was) ? was : []).slice(0, 8).map(w => ({
          id: String(w?.id ?? '').slice(0, 64), app: String(w?.app ?? '').slice(0, 64), name: String(w?.name ?? '').slice(0, 80),
          kind: String(w?.kind ?? '').slice(0, 12), how: String(w?.how ?? '').slice(0, 12), shows: String(w?.shows ?? '').slice(0, 400), in: Boolean(w?.in),
        }))
        const turned = { talk: own(talk), request: String(request ?? '').slice(0, 2000), reply: String(reply ?? '').slice(0, 4000), shown: tidyShown }
        // The turn the server kept as it began is filled, not kept twice.
        const filled = typeof turnId === 'string' && turnId.startsWith(`${own(talk)}:`) && (await apps.retalk(turnId, turned).catch(() => false))
        if (!filled) await apps.talked(turned)
        return json(res, { ok: true })
      }
      if (req.method === 'GET' && parts[0] === 'api' && parts[1] === 'shown' && parts[2]) {
        const event = await reopen(decodeURIComponent(parts[2]), own(url.searchParams.get('talk')))
        if (!event) return json(res, { error: 'not in this conversation' }, 404)
        return json(res, event)
      }
      // What someone else kept in a shared app, as it happens: the page draws
      // it again (MARCUS-4). Only that something changed is said; the page
      // reads it through its own link.
      if (req.method === 'GET' && url.pathname === '/api/watch') {
        const pairs = String(url.searchParams.get('apps') ?? '').split(',').map(x => x.split('|')).filter(([a, o]) => a && /^s-[0-9a-f]{32}$/.test(o ?? '')).slice(0, 20)
        const named = []
        for (const [a, o] of pairs) {
          const who = await owning(a, o)
          if (who && who.role !== 'gone') named.push(`${a}|${who.owner}`)
        }
        res.writeHead(200, { 'content-type': 'text/event-stream', 'cache-control': 'no-store', connection: 'keep-alive' })
        res.write(': watching\n\n')
        const send = event => res.write(`data: ${JSON.stringify(event)}\n\n`)
        for (const n of named) watchers.set(n, new Set([...(watchers.get(n) ?? []), send]))
        const ping = setInterval(() => res.write(': ping\n\n'), 25000)
        req.on('close', () => {
          clearInterval(ping)
          for (const n of named) {
            const set = watchers.get(n)
            set?.delete(send)
            if (set && !set.size) watchers.delete(n)
          }
        })
        return
      }
      // A tool an app calls from its view: only the tools it was made with.
      if (req.method === 'POST' && url.pathname === '/api/call') {
        const input = await body(req)
        const { app: appId, name, me, self } = input
        let args = input.arguments
        // A keep the app made by itself, with nobody touching it (a new
        // version putting what it had in its own shape): nobody's doing, so
        // it carries no name and is no link's own (TOM-2).
        const auto = name === 'app_keep' && args?.auto === true
        // Kept right after the chat asked for something: what it took out was asked for.
        const askedFor = name === 'app_keep' && args?.asked === true
        if (args && typeof args === 'object' && ('auto' in args || 'asked' in args)) {
          const { auto: _, asked: __, ...rest } = args
          args = rest
        }
        const app = await apps.get(appId)
        if (app && KEEPING.includes(name)) {
          const who = await owning(app.id, me)
          // How each person looks at it (view: names) stays with their own browser.
          const mine = own(self) && !own(self).startsWith('s-') ? own(self) : ''
          const key = String(args?.key ?? '')
          const work = async () => {
            // What the page saw when it kept (base): the keep goes on top of
            // what the others kept since, instead of over it (INES-1, MAYA-2).
            const said = []
            let mergedIn = false
            if (name === 'app_keep' && 'base' in input && !isView(key)) {
              const stored = await apps.kept(app.id, key, who?.owner ?? '').catch(() => null)
              const seen = who ? hideIn(partOf(stored, who.only), who.hide) : stored
              if (JSON.stringify(seen ?? null) !== JSON.stringify(input.base ?? null)) {
                const value = mergeKeep(input.base ?? null, args?.value ?? null, seen ?? null, said, `/${key}`)
                mergedIn = JSON.stringify(value) !== JSON.stringify(args?.value ?? null)
                args = { ...args, value }
              }
            }
            // What was taken out stays out (round 9 TOM-2, TOM-3): an entry
            // taken out comes back only from the browser that took it out
            // (its own undo), never from a phone still showing the old list.
            const goneSlot = `gone:${who?.owner ?? ''}:${key}`
            const listed = name === 'app_keep' && !isView(key) && Array.isArray(args?.value)
            const stored_ = listed ? await apps.kept(app.id, key, who?.owner ?? '').catch(() => null) : null
            if (listed) {
              const gone = (await apps.kept(app.id, goneSlot, LINKS).catch(() => null)) ?? {}
              const there = new Set((Array.isArray(stored_) ? stored_ : []).map(e => String(e?.id)))
              const back = args.value.filter(e => e && typeof e === 'object' && e.id != null && !there.has(String(e.id)) && gone[String(e.id)] && gone[String(e.id)] !== `${me ?? ''}|${self ?? ''}`)
              if (back.length) {
                args = { ...args, value: args.value.filter(e => !back.includes(e)) }
                for (const e of back) said.push({ field: key, stayedOut: entryName(e) })
                mergedIn = true
              }
            }
            const refused = who?.role === 'gone' ? STOPPED : who && name === 'app_keep' ? (await refusal(who, app.id, args)) || (await outOfPart(who, app.id, args)) : ''
            if (refused) return { content: [{ type: 'text', text: refused }], isError: true }
            // A person's link: what it adds says who added it, it sees and
            // keeps only its part, and what it doesn't see stays (INES-5, MARCUS-4, MARCUS-1).
            let asked = args
            const own_ = name === 'app_keep' && who && !isView(key)
            const before = own_ ? await apps.kept(app.id, key, who.owner).catch(() => null) : null
            const name_ = auto ? '' : who?.name
            // A keep nobody made (a new version drawing first on a link's
            // phone, putting in what the owner asked the chat for) can't
            // sign as that person, whatever the app wrote (round 9 TOM-4).
            if (auto && who?.name && name === 'app_keep' && !isView(key)) args = { ...args, value: unsignAuto(stored_ ?? (await apps.kept(app.id, key, who.owner).catch(() => null)), args?.value, who.name) }
            if (own_ && (who.name || who.only || who.hide)) asked = { ...args, value: withHidden(before, withPart(before, args?.value, { by: name_, only: who.only }), who.hide) }
            // A link made for a person signs with their name, whatever the app wrote (P).
            if (own_ && name_) asked = { ...asked, value: signed(before, asked?.value, name_) }
            const out = await keeping(apps, app.id, name, asked, who?.owner ?? '', mine, auto ? '' : who?.name ?? (who && who.role !== 'own' && who.role !== 'full' ? 'someone with a link' : ''))
            // Who took each entry out, for the rule above.
            const removedHere = []
            if (listed && !out.isError) {
              const now = await apps.kept(app.id, key, who?.owner ?? '').catch(() => null)
              const ids = new Set((Array.isArray(now) ? now : []).map(e => String(e?.id)))
              const gone = (await apps.kept(app.id, goneSlot, LINKS).catch(() => null)) ?? {}
              let changed = false
              for (const e of Array.isArray(stored_) ? stored_ : []) if (e?.id != null && !ids.has(String(e.id))) {
                gone[String(e.id)] = `${me ?? ''}|${self ?? ''}`
                changed = true
                // A new version that took an entry out by itself says so on
                // the screen, so nobody loses it without a word (TOM-2, TOM-6).
                if (auto && !askedFor) said.push({ field: key, tookOut: entryName(e) })
                // Taken out by hand here: this page offers Undo beside it (round 10 INES-8).
                else if (!auto) removedHere.push(entryName(e))
              }
              for (const id of ids) if (gone[id]) { delete gone[id]; changed = true }
              if (changed) await apps.keep(app.id, goneSlot, Object.fromEntries(Object.entries(gone).slice(-2000)), LINKS).catch(() => {})
            }
            if (own_ && !out.isError) {
              if (who.role === 'helper' && who.token && !auto) {
                const added = madeBy(hideIn(partOf(before, who.only), who.hide), args?.value)
                // Kept by the link and by the person's name, so a new link
                // for them can change what the old one marked (MARCUS-3).
                for (const slot of [`made:${who.token}`, ...(who.name ? [madeName(who.name)] : [])]) {
                  const made = (await apps.kept(app.id, slot, LINKS).catch(() => null)) ?? {}
                  const now = new Set([...(made[key] ?? []), ...added])
                  if (now.size !== (made[key] ?? []).length) await apps.keep(app.id, slot, { ...made, [key]: [...now].slice(-2000) }, LINKS)
                }
              }
              await keepClashes(app.id, who.owner, key, said)
              told(app.id, who.owner, String(self ?? ''), said)
            }
            // What the link may not put there (another crew, a price it never
            // gets) is not kept, and its screen shows what was (MARCUS-1).
            const seenNow = own_ && !out.isError && (who.only || who.hide) ? hideIn(partOf(asked?.value, who.only), who.hide) : undefined
            const altered = seenNow !== undefined && JSON.stringify(unsigned(seenNow)) !== JSON.stringify(unsigned(args?.value ?? null))
            if (name === 'app_keep' && !out.isError && (mergedIn || said.length || altered)) out.structuredContent = { ...out.structuredContent, merged: true, value: altered ? seenNow : args.value, ...(said.length ? { said } : {}) }
            if (name === 'app_keep' && !out.isError && removedHere.length && (!who || who.role === 'own' || who.role === 'full')) out.structuredContent = { ...out.structuredContent, removed: removedHere.slice(0, 5) }
            // The app knows who this link is, so it never asks for a name (P).
            // Its owner, and a link that changes everything, get no who: they
            // may change and take out anything, whoever added it (DEV-2).
            if (who && who.role !== 'own' && who.role !== 'full' && name === 'app_kept' && args?.all && out.structuredContent) out.structuredContent.who = { role: who.role, ...(who.name ? { name: who.name } : {}), ...(who.marks ? { marks: true } : {}), ...(who.only ? { only: who.only } : {}), ...(who.hide ? { hidden: who.hide } : {}) }
            // Two entries with the same number from two phones are said to
            // every phone that opens it, until one is fixed (LENA-2).
            if (who && name === 'app_kept' && args?.all && out.structuredContent?.values) {
              const clashes = await clashesOf(app.id, who.owner, out.structuredContent.values, who.role === 'own' ? 'you' : '')
              if (clashes.length) out.structuredContent.clashes = clashes
            }
            if ((who?.only || who?.hide) && name === 'app_kept' && out.structuredContent) {
              const sc = out.structuredContent
              const seen = v => hideIn(partOf(v, who.only), who.hide)
              out.structuredContent = sc.values ? { ...sc, values: seenBy(sc.values, who), ...(who.hide ? { hidden: who.hide } : {}) } : { ...sc, value: Object.keys(seenBy({ [key]: sc.value }, who)).length ? seen(sc.value) : null }
            }
            return out
          }
          return json(res, name === 'app_keep' ? await inTurn(`${app.id}|${who?.owner ?? ''}|${key}`, work) : await work())
        }
        // An app that books can cancel what it booked.
        const its = app && (app.tools.includes(name) || (name === 'table_cancel' && app.tools.includes('table_book')))
        if (!its) return json(res, { content: [{ type: 'text', text: `${name} is not one of this app's tools` }], isError: true })
        const acting = TOOLS.find(t => t.name === name)?.acts === true
        const who = acting ? await owning(app.id, me) : null
        if (who?.role === 'gone') return json(res, { content: [{ type: 'text', text: STOPPED }], isError: true })
        if (who?.role === 'viewer') return json(res, { content: [{ type: 'text', text: 'Not done: this link can only look. Whoever shared it can.' }], isError: true })
        // An app on a connected service calls only the server it was made with.
        if (name === 'connector_call' && String(args?.server ?? '') !== String(app.args?.server?.url ?? '')) return json(res, { content: [{ type: 'text', text: 'This app reaches only the service it was made with.' }], isError: true })
        const { result, data, connect } = await run(name, args)
        if (connect) return json(res, result)
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
        const out = await say(String(token ?? ''), shows)
        if (!out) return json(res, { error: 'nothing to say' }, 404)
        return json(res, out)
      }
      // What the chat says after something was done in an app.
      // A link that opens one app, with what it keeps, on another phone or
      // computer: whoever has it sees and changes the same kept values,
      // under the link's own key. What this browser kept is copied there
      // once; the link is as private as the person keeps it.
      if (req.method === 'POST' && url.pathname === '/api/share') {
        const input = await body(req)
        const { app: appId, me } = input
        const app = appId ? await apps.get(String(appId)) : null
        const who = app && (await owning(app.id, me))
        if (!who) return json(res, { error: 'no such app' }, 404)
        if (who.role === 'gone') return json(res, { error: STOPPED }, 403)
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
        // A link for one person: named, so what they add says it was them,
        // and maybe for part of the app only (one crew's jobs) (INES-5, MARCUS-4).
        const { person } = input
        if (person && typeof person === 'object') {
          const name = String(person.name ?? '').replace(/\s+/g, ' ').trim().slice(0, 30)
          const role = person.role === 'viewer' ? 'viewer' : 'helper'
          // A link that only marks what is there done: no adding (round 10 MARCUS-6).
          const marks = person.role === 'marker'
          const field = String(person.only?.field ?? '').trim().slice(0, 40)
          const only = field && person.only?.value != null && String(person.only.value).trim() ? { field, value: String(person.only.value).trim().slice(0, 60) } : null
          // Fields their link never gets, such as prices (MARCUS-1).
          const hide = (Array.isArray(person.hide) ? person.hide : []).map(h => String(h ?? '').trim().slice(0, 40)).filter(h => h && !/^(id|by)$/.test(h)).slice(0, 12)
          if (!name) return json(res, { error: 'Give the person a name.' }, 400)
          const link = { role, name, ...(marks ? { marks } : {}), ...(only ? { only } : {}), ...(hide.length ? { hide } : {}) }
          // The same person with the same link again: theirs, not a second one (round 10 INES-10).
          const alike = (links.people ?? []).find(p => p.name.toLowerCase() === name.toLowerCase() && JSON.stringify({ ...p, token: undefined, name: undefined }) === JSON.stringify({ ...link, name: undefined }))
          if (alike) return json(res, { token, helper: links.helper, viewer: links.viewer, people: links.people ?? [], same: alike.token })
          const tok = randomBytes(16).toString('hex')
          await apps.keep(app.id, `link:${tok}`, { owner: `s-${token}`, ...link }, LINKS)
          links = { ...links, people: [...(links.people ?? []), { token: tok, ...link }] }
          await apps.keep(app.id, '-links', links, `s-${token}`)
        }
        return json(res, { token, helper: links.helper, viewer: links.viewer, people: links.people ?? [] })
      }
      // Stop sharing (DEV-2): every link of it opens nothing from now on, and
      // what it kept comes back to the browser that stops it, its own again.
      if (req.method === 'POST' && url.pathname === '/api/unshare') {
        const { app: appId, me, self, person } = await body(req)
        const app = appId ? await apps.get(String(appId)) : null
        const who = app && (await owning(app.id, me))
        const mine = own(self) && !own(self).startsWith('s-') ? own(self) : ''
        if (!who || !mine || !who.owner.startsWith('s-')) return json(res, { error: 'not shared' }, 404)
        if (who.role !== 'full') return json(res, { error: 'Only whoever shared it can stop sharing it.' }, 403)
        const token = who.owner.slice(2)
        const values = await apps.keptAll(app.id, who.owner)
        const links = values['-links']
        // One person's link only: it opens nothing from now on, and the app
        // stays shared with the others.
        if (typeof person === 'string' && (links?.people ?? []).some(p => p.token === person)) {
          await apps.keep(app.id, `link:${person}`, { owner: who.owner, role: 'gone' }, LINKS)
          const people = links.people.filter(p => p.token !== person)
          await apps.keep(app.id, '-links', { ...links, people }, who.owner)
          // A tab that has it open now is told, and shows it stopped (DEV-7).
          told(app.id, who.owner, '')
          return json(res, { stopped: true, people })
        }
        for (const [key, value] of Object.entries(values)) if (!key.startsWith('-')) await apps.keep(app.id, key, value, mine)
        for (const tok of [token, links?.helper, links?.viewer, ...(links?.people ?? []).map(p => p.token)].filter(Boolean)) await apps.keep(app.id, `link:${tok}`, { owner: who.owner, role: 'gone' }, LINKS)
        for (const key of Object.keys(values)) await apps.keep(app.id, key, null, who.owner)
        told(app.id, who.owner, '')
        return json(res, { stopped: true })
      }
      // What an app held before, and bringing it back (LENA-1, INES-1): for
      // whoever owns it, or a link that changes everything.
      if (req.method === 'POST' && (url.pathname === '/api/history' || url.pathname === '/api/restore')) {
        const input = await body(req)
        const app = input.app ? await apps.get(String(input.app)) : null
        const who = app && (await owning(app.id, input.me))
        if (!who || who.role === 'gone') return json(res, { error: who ? STOPPED : 'no such app' }, who ? 403 : 404)
        if (who.role !== 'own' && who.role !== 'full') return json(res, { error: 'Only whoever shared it can bring back what it kept.' }, 403)
        // A private app keeps nothing with the chat, nor what it held before.
        if (input.forget) {
          if (who.role !== 'own') return json(res, { error: 'Only whoever owns it can.' }, 403)
          await apps.forgetHistory(app.id, who.owner)
          return json(res, { forgot: true })
        }
        const rows = await apps.history(app.id, who.owner)
        // As it was just before the change at t: each value as held until its next change.
        const at = t => {
          const out = {}
          for (const r of [...rows].reverse()) if (r.at >= t && !(r.name in out)) out[r.name] = r.value
          return out
        }
        if (url.pathname === '/api/history') {
          // Every copy kept is a moment, the one just before a change that took
          // something out too, and each says what changed after it (MAYA-1, LENA-3, INES-9).
          const times = [...new Set(rows.map(r => r.at))].slice(0, 40)
          const now = await apps.keptAll(app.id, who.owner)
          const mine = values => Object.fromEntries(Object.entries(values).filter(([k]) => !/^(-|view:|made:|link:)/.test(k)))
          const says = values => Object.entries(mine(values)).map(([k, v]) => (Array.isArray(v) ? `${v.length} in ${k}` : v && typeof v === 'object' ? Object.entries(v).filter(([, x]) => Array.isArray(x)).map(([n, x]) => `${x.length} in ${n}`).join(', ') || k : `${k} ${String(v).slice(0, 20)}`)).filter(Boolean).join(', ').slice(0, 120)
          // An entry by what tells it apart: its name and its number, not its
          // date ("MS-2320 Miller & Sons", not "2026-11-17") (LENA-4).
          const NAMED = /^(name|title|item|what|text|description|desc|supplier|vendor|customer|client|job|task|case|place|dish|thing|label)$/i
          const NUMBER = /^(number|no|num|invoice|invoice_?(no|num|number|id)|invoiceNumber|ref|reference|code)$/i
          const label = e => {
            if (!e || typeof e !== 'object') return String(e ?? '').slice(0, 30)
            const fields = Object.entries(e).filter(([k, v]) => !/^(id|by|at|done|checked|ticked)$/i.test(k) && (typeof v === 'string' ? v.trim() : typeof v === 'number'))
            const number = fields.find(([k]) => NUMBER.test(k.replace(/\s+/g, '_')))?.[1]
            const named = fields.find(([k, v]) => NAMED.test(k) && typeof v === 'string')?.[1] ?? fields.find(([, v]) => typeof v === 'string' && !/^\d{4}-\d\d-\d\d/.test(v) && v !== number)?.[1]
            const out = [number, named].filter(x => x != null && String(x).trim()).join(' ')
            return (out || String(fields.find(([, v]) => typeof v === 'string')?.[1] ?? e.id ?? '')).slice(0, 40)
          }
          const lists = (v, path = '') => (Array.isArray(v) ? [[path, v]] : v && typeof v === 'object' ? Object.entries(v).flatMap(([k, x]) => lists(x, path ? `${path} ${k}` : k)) : [])
          const MONEY = /amount|price|total|cost|sum|paid|owed|balance/i
          const short = (v, f = '') => (v == null || v === '' ? 'empty' : typeof v === 'object' ? 'changed' : typeof v === 'number' && MONEY.test(f) ? v.toFixed(2) : String(v).slice(0, 24))
          const TICK = /^(done|checked|ticked|paid|bought|got|packed|booked|complete[d]?)$/i
          let named = false
          let unnamed = false
          const changed = (before, after) => {
            const out = []
            for (const k of new Set([...Object.keys(mine(before)), ...Object.keys(mine(after))])) {
              if (JSON.stringify(before[k]) === JSON.stringify(after[k])) continue
              const a = new Map(lists(before[k], k)), b = new Map(lists(after[k], k))
              let said = false
              for (const path of new Set([...a.keys(), ...b.keys()])) {
                const was = a.get(path) ?? [], now = b.get(path) ?? []
                const ided = [...was, ...now].every(e => e && typeof e === 'object' && e.id != null)
                const keyOf = e => (ided ? String(e.id) : label(e))
                const then = new Map(was.map(e => [keyOf(e), e])), next = new Map(now.map(e => [keyOf(e), e]))
                const gone = was.filter(e => !next.has(keyOf(e))).map(label).filter(Boolean)
                // Each entry added says who added it, when it carries a name:
                // a batch that came in together is not all one person's (INES-3, MAYA-8).
                const addedOnes = now.filter(e => !then.has(keyOf(e)))
                const byOf = e => (e && typeof e === 'object' && typeof e.by === 'string' && e.by.trim() ? e.by.trim() : '')
                if (addedOnes.some(byOf)) named = true
                const groups = new Map()
                for (const e of addedOnes) {
                  const l = label(e)
                  if (!l) continue
                  groups.set(byOf(e), [...(groups.get(byOf(e)) ?? []), l])
                }
                const added = [...groups.values()].flat()
                // What changed in an entry still there: a tick, an amount (LENA-4, MARCUS-8).
                const edits = []
                for (const e of now) {
                  const old = then.get(keyOf(e))
                  if (!old || JSON.stringify(old) === JSON.stringify(e) || typeof e !== 'object') continue
                  for (const f of new Set([...Object.keys(old), ...Object.keys(e)])) {
                    if (/^(id|at|by)$/.test(f) || JSON.stringify(old[f]) === JSON.stringify(e[f])) continue
                    edits.push(TICK.test(f) && typeof (e[f] ?? false) === 'boolean' ? `${label(e)} ${e[f] ? '' : 'un'}${f === 'done' || f === 'checked' ? 'ticked' : f}` : `${label(e)} ${f} ${short(old[f], f)} to ${short(e[f], f)}`)
                  }
                }
                if (gone.length || edits.length) unnamed = true
                if (gone.length) out.push(`${gone.slice(0, 3).join(', ')} taken out of ${path}`)
                if (added.length) {
                  if (!addedOnes.some(byOf)) out.push(`${added.slice(0, 3).join(', ')} added to ${path}`)
                  else for (const [by, ls] of groups) out.push(`${ls.slice(0, 3).join(', ')} added to ${path}${by ? ` by ${by}` : ''}`)
                }
                if (edits.length) out.push(edits.slice(0, 3).join(', ') + (edits.length > 3 ? ` and ${edits.length - 3} more` : ''))
                if (gone.length || added.length || edits.length) said = true
              }
              if (!said) unnamed = true
              if (!said) out.push(typeof before[k] === 'object' || typeof after[k] === 'object' ? `${k} changed` : `${k} ${short(before[k])} to ${short(after[k])}`)
            }
            return out.join('; ').slice(0, 200)
          }
          // Who made the change, when it came through a person's link.
          const signers = await apps.signers(app.id, who.owner)
          const whoIn = (from, to) => [...new Set(signers.filter(s => s.at >= from && (to == null || s.at < to)).map(s => s.by))]
          const states = times.map(t => ({ ...now, ...at(t) }))
          return json(res, {
            now: says(now),
            // When this version was made: Undo takes back a change to what it
            // keeps that came after it, before going back a version (round 10 MARCUS-1).
            versionAt: await apps.versionMade(app.id, app.version),
            moments: times.map((t, i) => {
              named = false
              unnamed = false
              const then = changed(states[i], i ? states[i - 1] : now)
              // Entries that say who added them name each one; the moment's
              // signers are said only for what carries no name (INES-3).
              const by = whoIn(t, i ? times[i - 1] : null)
              return { at: t, says: says(states[i]), then, by: named && !unnamed ? [] : by }
            }),
          })
        }
        const t = Number(input.at)
        if (!Number.isFinite(t)) return json(res, { error: 'when?' }, 400)
        const values = at(t)
        for (const [key, value] of Object.entries(values)) await apps.keep(app.id, key, value, who.owner)
        told(app.id, who.owner, String(input.self ?? ''))
        return json(res, { restored: Object.keys(values) })
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
        // A helper's or a viewer's phone changes no app, and removes none (MAYA-3, LENA-1).
        if (parts[3] !== 'open' && parts[3] !== 'now') {
          const refused = await mayChange(appId, input)
          if (refused) return json(res, { error: refused }, 403)
        }
        if (parts[3] === 'change') return stream(res, send => (limited(req) ? Promise.reject(new Error(tooMany)) : change(appId, String(input.text ?? ''), send, { shownId: String(input.shown ?? ''), talk: own(input.talk), inPlace: true })))
        if (parts[3] === 'open') {
          // A stopped link opens nothing: not the app, not what it was made with (PRIYA-2, TOM-7).
          const token = input?.links?.[appId]
          if (typeof token === 'string' && /^[0-9a-f]{32}$/.test(token) && (await owning(appId, `s-${token}`))?.role === 'gone') {
            return stream(res, async send => send({ type: 'stopped', app: appId, text: STOPPED }))
          }
          const linkWho = typeof token === 'string' && /^[0-9a-f]{32}$/.test(token) ? await owning(appId, `s-${token}`) : null
          return stream(res, async send => {
            const out = recording(own(input.talk), send)
            await open(appId, out, linkWho)
            await out.written()
          })
        }
        // An answer of what was found, kept as its own app with these findings.
        if (parts[3] === 'keep-answer') {
          const row = await apps.shownOf(String(input.shown ?? ''), own(input.talk))
          // Kept once: that place in the conversation is the kept answer from
          // then on, after a reload too, and a second press gives the same one (PRIYA-3).
          if (row?.app?.startsWith('answer-')) {
            const there = await apps.get(row.app)
            if (there) return json(res, { app: view(there) })
          }
          const args = row?.args ?? shown.get(appId)?.args
          if (!args) return json(res, { error: 'that answer is not here any more' }, 404)
          const name = String(input.name ?? '').trim().slice(0, 60) || 'Kept answer'
          const kept = await apps.keepAnswer(appId, { name, request: String(input.request ?? name).slice(0, 300), args })
          if (row) await apps.show({ id: row.id, talk: own(input.talk), app: kept.id, version: kept.version, tool: row.tool, args: row.args, how: 'kept' }).catch(log)
          // The notes on it come with it (PRIYA-2).
          const who = input.me ? await owning(appId, input.me) : null
          if (who && who.role !== 'gone') {
            const key = `notes ${String(args.query ?? '').slice(0, 70)}`
            const notes = await apps.kept(appId, key, who.owner).catch(() => null)
            if (notes != null) await apps.keep(kept.id, key, notes, who.owner)
          }
          return json(res, { app: view(kept) })
        }
        // The app as it is now, for a phone told of a new version (R).
        if (parts[3] === 'now') return json(res, { app: view(await apps.get(appId)), html: htmlOf(await apps.get(appId)) })
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
  }
  const server = createServer((req, res) => caller.run(ctxOf(req.headers['x-self'] ?? new URL(req.url, 'http://localhost').searchParams.get('self'), originOf(req)), () => handle(req, res)))
  await new Promise(resolve => server.listen(port, host, resolve))
  return {
    url: `http://${host === '0.0.0.0' ? '127.0.0.1' : host}:${server.address().port}/`,
    apps,
    close() {
      server.close()
      server.closeAllConnections?.()
      closeClients()
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
