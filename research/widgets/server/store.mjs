// The apps, kept in ChloeDB: every app the chat made or keeps (its source,
// each version, what it was asked for), and the search that brings a saved
// app back when a later request needs it, by words (BM25) and by meaning
// (a static embedding of its name, what it does and the requests it served)
// fused by rank.
import { spawn } from 'node:child_process'
import { randomBytes } from 'node:crypto'
import { closeSync, mkdirSync, openSync, readFileSync, rmSync, statSync, writeFileSync } from 'node:fs'
import net from 'node:net'
import path from 'node:path'

import { Strata, findServerBinary, startServer } from '../../../chloedb-apps/engine/strata.mjs'

const MODEL = process.env.WIDGETS_EMBED_MODEL || 'potion-retrieval-32M-int8'
const REPO = new URL('../../../', import.meta.url).pathname

const APPS = `CREATE TABLE ia_apps (id KEYWORD PRIMARY KEY, name TEXT, about TEXT, examples TEXT, kind KEYWORD,
  tool KEYWORD, tools KEYWORD[], saved BOOL, version INT, uses INT, made INT, used INT,
  args TEXT OPTIONS(indexed = false), source TEXT OPTIONS(indexed = false)MEANING)`
const VERSIONS = `CREATE TABLE ia_versions (id KEYWORD PRIMARY KEY, app KEYWORD, version INT, change TEXT, made INT,
  source TEXT OPTIONS(indexed = false))`
const TURNS = `CREATE TABLE ia_turns (id KEYWORD PRIMARY KEY, request TEXT, app KEYWORD, how KEYWORD, at INT)`
// What an app keeps between openings (Kit.keep), by the browser that kept
// it (its owner), the app and the key: one friend's prices are not another's.
const STATE = `CREATE TABLE ia_state (id KEYWORD PRIMARY KEY, owner KEYWORD, app KEYWORD, name KEYWORD, value TEXT OPTIONS(indexed = false), at INT)`
// Each app shown in a conversation, with what it was filled with, so a
// reload or a folded answer opens it again as it was.
const SHOWN = `CREATE TABLE ia_shown (id KEYWORD PRIMARY KEY, talk KEYWORD, app KEYWORD, version INT, tool KEYWORD, args TEXT OPTIONS(indexed = false), how KEYWORD, at INT)`
// A conversation's turns: what was asked, which apps it showed, what was
// said. A conversation is one browser's (its talk id is that browser's
// secret), so it is read back only by that id.
// What an app held before each change, one a minute (and before any change
// that takes entries out), so what it kept yesterday can come back
// (LENA-1, INES-1).
const HISTORY = `CREATE TABLE ia_history (id KEYWORD PRIMARY KEY, owner KEYWORD, app KEYWORD, name KEYWORD, value TEXT OPTIONS(indexed = false), at INT)`
// Who made each change through a link made for a person, so Earlier
// versions says who did what (LENA-4, MARCUS-8).
const SIGNED = `CREATE TABLE ia_by (id KEYWORD PRIMARY KEY, owner KEYWORD, app KEYWORD, by KEYWORD, at INT)`
const TALK = `CREATE TABLE ia_talk (id KEYWORD PRIMARY KEY, talk KEYWORD, at INT, request TEXT OPTIONS(indexed = false), reply TEXT OPTIONS(indexed = false), shown TEXT OPTIONS(indexed = false))`

// The apps "made before", so the rail is not empty on the first start: the
// three widgets, and the list of what Claude found, whose views were
// written by hand.
export const SEEDS = [
  { id: 'stock-chart', name: 'Stock chart', kind: 'info', tool: 'stock_quote', tools: ['stock_quote'], source: 'view:stock', args: { symbol: 'NVDA' },
    about: 'The price of a stock, ETF, index or crypto now, its change and a chart over a range.', examples: 'how is nvidia doing today' },
  { id: 'flight-tracker', name: 'Flight tracker', kind: 'info', tool: 'flight_track', tools: ['flight_track'], source: 'view:flight', args: { flight: 'LH773' },
    about: 'Where a flight is right now on a map, with its altitude, speed and heading.', examples: 'is lufthansa 773 in the air right now' },
  { id: 'shop', name: 'Shop', kind: 'action', tool: 'shop_search', tools: ['shop_search', 'cart_add', 'cart_view'], source: 'view:shop', args: { query: 'headphones' },
    about: 'Products that match a search, with pictures, prices and ratings, and a cart to add them to.', examples: 'wireless earphones under 100 dollars' },
  { id: 'search-results', name: 'Search results', kind: 'info', tool: 'findings', tools: ['findings'], source: 'view:findings', args: { query: 'Nothing yet', documents: [] },
    about: 'What Claude found for a question on the web or in documents: each page with its source, date, a summary and the passages that matter, side by side.',
    examples: 'find reviews of noise cancelling headphones | research what people say about | look up articles on' },
]

const id = name => `${name.toLowerCase().replace(/[^a-z0-9]+/g, '-').replace(/^-|-$/g, '').slice(0, 24)}-${randomBytes(3).toString('hex')}`
const now = () => Date.now()
const stateId = (owner, appId, key) => `${owner || '-'}|${appId}|${key}`

export class Apps {
  constructor(db, { meaning }) {
    this.db = db
    this.meaning = meaning
  }

  // Opens the store at CHLOEDB_URL, or the chloedb-server of `dir`: the one
  // every process with that folder shares (`shared`, started if none
  // runs), or one of this process's own, which `close` stops.
  static async open({ dir, url = process.env.CHLOEDB_URL, log = () => {}, shared = true } = {}) {
    let db
    let child = null
    if (url) db = new Strata(url, process.env.CHLOEDB_KEY)
    else {
      const bin = process.env.CHLOEDB_SERVER_BIN || findServerBinary(REPO)
      if (!bin) throw new Error('no chloedb-server: build it (cd chloedb-server && cargo build --release), or set CHLOEDB_SERVER_BIN or CHLOEDB_URL')
      if (shared) db = await sharedServer({ bin, dir, log })
      else {
        const port = await freePort()
        const started = await startServer({ bin, db: path.join(dir, 'apps.strata'), listen: `127.0.0.1:${port}`, log })
        db = started.client
        child = started.child
      }
    }
    // The listing names each table under `table`.
    const listed = await db.tables()
    const have = new Set(listed.map(t => t.table ?? t.name ?? t))
    let meaning = have.has('ia_apps') ? JSON.stringify(listed.find(t => (t.table ?? t.name) === 'ia_apps')).includes('"meaning"') : null
    if (!have.has('ia_apps')) {
      // Two processes opening a new folder at once both create the tables:
      // the second finds them there.
      const create = sql => db.sql(sql).catch(error => {
        if (!/already exists/i.test(error.message)) throw error
      })
      try {
        await db.sql(APPS.replace('MEANING', `, meaning VECTOR OPTIONS(embed = 'name, about, examples', model = '${MODEL}')`))
        meaning = true
      } catch (error) {
        if (/already exists/i.test(error.message)) meaning = JSON.stringify((await db.tables()).find(t => (t.table ?? t.name) === 'ia_apps')).includes('"meaning"')
        else {
          // No embedding model here (chloedb-server fetch-model fetches it):
          // recall by words alone.
          log(`apps by words only: ${error.message}`)
          await create(APPS.replace('MEANING', ''))
          meaning = false
        }
      }
      await create(VERSIONS)
      await create(TURNS)
    }
    // Tables added later: a folder from before them gets them.
    const create = sql => db.sql(sql).catch(error => {
      if (!/already exists/i.test(error.message)) throw error
    })
    const tables = new Set((await db.tables()).map(t => t.table ?? t.name ?? t))
    for (const [name, sql] of [['ia_state', STATE], ['ia_shown', SHOWN], ['ia_talk', TALK], ['ia_history', HISTORY], ['ia_by', SIGNED]]) if (!tables.has(name)) await create(sql)
    const apps = new Apps(db, { meaning })
    apps.child = child
    // Each seed once, by its id: a folder from before a seed was added gets
    // it, and a seed the person took out stays out.
    const seeded = new Set((await db.get('ia_apps', SEEDS.map(seed => seed.id))).filter(Boolean).map(row => row.id))
    for (const seed of SEEDS) if (!seeded.has(seed.id)) await apps.put({ ...seed, saved: true, uses: 1 }, 'made by hand')
    return apps
  }

  close() {
    this.child?.kill()
  }

  // Stops the shared chloedb-server of `dir`, if one runs.
  static stop(dir) {
    const file = path.join(dir, 'server.json')
    try {
      const { pid } = JSON.parse(readFileSync(file, 'utf8'))
      process.kill(pid)
    } catch {
      // None runs.
    }
    rmSync(file, { force: true })
  }

  // The saved apps that may fit a request, best first.
  async recall(text, limit = 8) {
    const fused = this.meaning
      ? 'rrf(bm25(name, :q), bm25(about, :q), bm25(examples, :q), similarity(meaning, :q))'
      : 'rrf(bm25(name, :q), bm25(about, :q), bm25(examples, :q))'
    const rows = await this.db.query(
      `SELECT id, name, about, kind, tool, tools, examples, uses, args, source, score() AS score FROM ia_apps WHERE saved = true ORDER BY ${fused} DESC LIMIT ${limit}`,
      { q: text },
    )
    // An app that works things out in the page reads its values by name:
    // the router is told those names, so new numbers land in the app's own
    // fields and not under a name it ignores.
    return rows.map(tidy).map(({ source, args, ...app }) => (app.tool === 'inputs' ? { ...app, reads: valueNames(source, args) } : app))
  }

  // The rail: saved apps, the last used first.
  async saved() {
    const rows = await this.db.query('SELECT id, name, about, kind, tool, tools, uses, used, version FROM ia_apps WHERE saved = true ORDER BY used DESC LIMIT 100')
    return rows.map(tidy)
  }

  async get(appId) {
    const [row] = await this.db.get('ia_apps', [appId])
    return row ? tidy(row) : null
  }

  // A new app, or a new version of one.
  async put(app, change) {
    const row = {
      id: app.id ?? id(app.name),
      name: app.name,
      about: app.about,
      examples: app.examples ?? '',
      kind: app.kind,
      tool: app.tool,
      tools: app.tools,
      saved: Boolean(app.saved),
      version: app.version ?? 1,
      uses: app.uses ?? 0,
      made: app.made ?? now(),
      used: now(),
      args: JSON.stringify(app.args ?? {}),
      source: app.source,
    }
    const ops = [{ op: 'upsert', table: 'ia_apps', doc: row }]
    // A version row for each new source, none for a save or an undo.
    if (change != null) {
      ops.push({ op: 'upsert', table: 'ia_versions', doc: { id: `${row.id}@${row.version}`, app: row.id, version: row.version, change, made: now(), source: row.source } })
    }
    await this.db.write(ops)
    // A new version (or an undo) is told to every phone that has it open (R).
    try { this.onput?.(row.id, row.version, change) } catch {}
    return tidy(row)
  }

  async change(appId, source, change) {
    const app = await this.get(appId)
    // A change after an undo replaces the versions the undo left.
    const later = Array.from({ length: 50 }, (_, i) => `${appId}@${app.version + 2 + i}`)
    const stale = (await this.db.get('ia_versions', later)).filter(Boolean)
    if (stale.length) await this.db.write(stale.map(v => ({ op: 'delete', table: 'ia_versions', key: v.id })))
    return this.put({ ...app, source, version: app.version + 1 }, change)
  }

  // Back one version, keeping the newer one for redo until the next change
  // replaces it.
  async undo(appId) {
    const app = await this.get(appId)
    if (app.version <= 1) return app
    const [before] = await this.db.get('ia_versions', [`${appId}@${app.version - 1}`])
    if (!before) return app
    return this.put({ ...app, source: before.source, version: app.version - 1 }, null)
  }

  // The changes that made the version it is at now, oldest first ("take
  // the Berardo museum out"), so a follow-up keeps them.
  async changesOf(appId) {
    const app = await this.get(appId)
    if (!app || app.version <= 1) return []
    const ids = Array.from({ length: Math.min(app.version - 1, 20) }, (_, i) => `${appId}@${app.version - i}`)
    const rows = (await this.db.get('ia_versions', ids)).filter(Boolean)
    return rows.sort((a, b) => a.version - b.version).map(v => String(v.change ?? '').slice(0, 300)).filter(Boolean)
  }

  // Forward again to the version an undo left.
  async redo(appId) {
    const app = await this.get(appId)
    const [after] = await this.db.get('ia_versions', [`${appId}@${app.version + 1}`])
    if (!after) return app
    return this.put({ ...app, source: after.source, version: app.version + 1 }, null)
  }

  // Whether an undo left a newer version to go forward to.
  async hasNewer(appId, version) {
    const [after] = await this.db.get('ia_versions', [`${appId}@${version + 1}`])
    return Boolean(after)
  }

  // Takes an app out of the person's apps and out of recall; its versions
  // and what it kept go with it.
  async remove(appId) {
    const app = await this.get(appId)
    if (!app) return null
    const versions = Array.from({ length: app.version + 50 }, (_, i) => `${appId}@${i + 1}`)
    const there = (await this.db.get('ia_versions', versions)).filter(Boolean)
    const ops = [{ op: 'delete', table: 'ia_apps', key: appId }, ...there.map(v => ({ op: 'delete', table: 'ia_versions', key: v.id }))]
    const state = await this.db.query('SELECT id FROM ia_state WHERE app = :app LIMIT 1000', { app: appId }).catch(() => [])
    for (const row of state) ops.push({ op: 'delete', table: 'ia_state', key: row.id })
    const history = await this.db.query('SELECT id FROM ia_history WHERE app = :app LIMIT 2000', { app: appId }).catch(() => [])
    for (const row of history) ops.push({ op: 'delete', table: 'ia_history', key: row.id })
    const signed = await this.db.query('SELECT id FROM ia_by WHERE app = :app LIMIT 2000', { app: appId }).catch(() => [])
    for (const row of signed) ops.push({ op: 'delete', table: 'ia_by', key: row.id })
    await this.db.write(ops)
    return app
  }

  async save(appId, saved = true, name) {
    const app = await this.get(appId)
    return this.put({ ...app, saved, name: name || app.name }, null)
  }

  // Kit.keep and Kit.kept: what an app keeps for one browser (its owner),
  // kept across the app's versions.
  async keep(appId, key, value, owner = '', by = '') {
    const text = JSON.stringify(value ?? null)
    // A pasted file of a few hundred KB is kept (DEV-4); its earlier
    // versions only while it is small.
    if (text.length > 1_000_000) throw new Error('That is too much to keep (1 MB at most).')
    const id = stateId(owner, appId, key)
    const at = now()
    const ops = [{ op: 'upsert', table: 'ia_state', doc: { id, owner, app: appId, name: key, value: text, at } }]
    // The chat's own records (links, what a link made) and how each person
    // looks at it (view:) have no history.
    if (!/^(-|link:|made:|view:)/.test(key) && !String(owner).startsWith('-') && text.length <= 200_000) {
      const [row] = await this.db.get('ia_state', [id])
      if (row && row.value !== text) {
        const was = JSON.parse(row.value)
        const count = v => (Array.isArray(v) ? v.length : v && typeof v === 'object' ? Object.values(v).reduce((n, x) => n + (Array.isArray(x) ? x.length : 0), 0) : 0)
        const minute = `${id}|${Math.floor(at / 60_000)}`
        const [had] = await this.db.get('ia_history', [minute])
        const doc = { owner, app: appId, name: key, value: row.value, at }
        if (!had) ops.push({ op: 'upsert', table: 'ia_history', doc: { id: minute, ...doc } })
        else if (count(value) < count(was)) ops.push({ op: 'upsert', table: 'ia_history', doc: { id: `${id}|t${at}`, ...doc } })
        if (by) ops.push({ op: 'upsert', table: 'ia_by', doc: { id: `${id}|t${at}`, owner, app: appId, by: String(by).slice(0, 40), at } })
      }
    }
    await this.db.write(ops)
  }

  async forgetHistory(appId, owner = '') {
    const mine = stateId(owner, appId, '')
    const rows = await this.db.query('SELECT id FROM ia_history WHERE app = :app LIMIT 2000', { app: appId }).catch(() => [])
    const ops = rows.filter(r => r.id.startsWith(mine)).map(r => ({ op: 'delete', table: 'ia_history', key: r.id }))
    const signed = await this.db.query('SELECT id FROM ia_by WHERE app = :app LIMIT 2000', { app: appId }).catch(() => [])
    ops.push(...signed.filter(r => r.id.startsWith(mine)).map(r => ({ op: 'delete', table: 'ia_by', key: r.id })))
    if (ops.length) await this.db.write(ops)
  }

  // What an app held before each change, for one owner, newest first.
  async history(appId, owner = '') {
    const mine = stateId(owner, appId, '')
    const rows = await this.db.query('SELECT id, name, value, at FROM ia_history WHERE app = :app LIMIT 2000', { app: appId }).catch(() => [])
    const out = rows.filter(r => r.id.startsWith(mine)).sort((a, b) => b.at - a.at)
    // The oldest go once there are many.
    if (out.length > 600) await this.db.write(out.slice(600).map(r => ({ op: 'delete', table: 'ia_history', key: r.id }))).catch(() => {})
    return out.slice(0, 600).map(r => ({ name: r.name, value: JSON.parse(r.value), at: r.at }))
  }

  // Who changed it through a person's link, and when, newest first.
  async signers(appId, owner = '') {
    const mine = stateId(owner, appId, '')
    const rows = await this.db.query('SELECT id, by, at FROM ia_by WHERE app = :app LIMIT 2000', { app: appId }).catch(() => [])
    return rows.filter(r => r.id.startsWith(mine)).map(r => ({ by: r.by, at: r.at })).sort((a, b) => b.at - a.at)
  }

  async kept(appId, key, owner = '') {
    const [row] = await this.db.get('ia_state', [stateId(owner, appId, key)])
    return row ? JSON.parse(row.value) : null
  }

  // Everything an app kept for one browser, as { key: value }: what its
  // draw gets as data.kept.
  async keptAll(appId, owner = '') {
    const mine = stateId(owner, appId, '')
    const rows = await this.db.query('SELECT id, name, value FROM ia_state WHERE app = :app LIMIT 1000', { app: appId }).catch(() => [])
    return Object.fromEntries(rows.filter(r => r.id.startsWith(mine)).map(r => [r.name, JSON.parse(r.value)]))
  }

  // A copy of an answer of what was found, kept as its own app: it opens
  // with the same findings, which a later search does not replace.
  async keepAnswer(appId, { name, request, args }) {
    const app = await this.get(appId)
    if (!app) return null
    // Its own id: another answer kept under the same name stays as it is.
    let key = `answer-${id(name)}`
    for (let n = 2; await this.get(key); n++) key = `answer-${id(name)}-${n}`
    return this.put({ ...app, id: key, name, about: `What was found for: ${request}`.slice(0, 200), examples: request, args, saved: true, uses: 0, version: 1, made: now() }, 'kept answer')
  }

  // An app shown in a conversation, with what it was filled with.
  async show({ id: shownId, talk, app, version, tool, args, how }) {
    await this.db.write([{ op: 'upsert', table: 'ia_shown', doc: { id: shownId, talk, app, version: version ?? 1, tool: tool ?? '', args: JSON.stringify(args ?? {}), how: how ?? '', at: now() } }])
  }

  async shownOf(shownId, talk) {
    const [row] = await this.db.get('ia_shown', [shownId])
    if (!row || row.talk !== talk) return null
    return { ...row, args: JSON.parse(row.args || '{}') }
  }

  // A turn of a conversation, and the conversation read back in order.
  async talked({ talk, request, reply, shown }) {
    await this.db.write([{ op: 'upsert', table: 'ia_talk', doc: { id: `${talk}:${now()}-${randomBytes(2).toString('hex')}`, talk, at: now(), request: request ?? '', reply: reply ?? '', shown: JSON.stringify(shown ?? []) } }])
  }

  async talk(talk, limit = 200) {
    const rows = await this.db.query(`SELECT id, at, request, reply, shown FROM ia_talk WHERE talk = :talk ORDER BY at DESC LIMIT ${limit}`, { talk })
    return rows.reverse().map(r => ({ at: r.at, request: r.request, reply: r.reply, shown: JSON.parse(r.shown || '[]') }))
  }

  // An app served a request: counted, its arguments kept for the rail, and
  // the request added to those it is recalled by. A kept answer keeps its
  // own findings.
  async used(appId, request, args, how) {
    const app = await this.get(appId)
    const examples = [request, ...app.examples.split(' | ').filter(e => e !== request)].filter(Boolean).slice(0, 8).join(' | ')
    await this.db.write([
      { op: 'upsert', table: 'ia_apps', doc: { ...untidy(app), examples, args: JSON.stringify(appId.startsWith('answer-') ? app.args : args ?? {}), uses: app.uses + 1, used: now() } },
      { op: 'upsert', table: 'ia_turns', doc: { id: `${now()}-${randomBytes(3).toString('hex')}`, request, app: appId, how, at: now() } },
    ])
  }
}

const sleep = ms => new Promise(resolve => setTimeout(resolve, ms))

// The shared server of a folder, if server.json names one that answers.
async function running(file) {
  let url
  try {
    ;({ url } = JSON.parse(readFileSync(file, 'utf8')))
  } catch {
    return null
  }
  const client = new Strata(url)
  try {
    await Promise.race([client.health(), sleep(2000).then(() => Promise.reject(new Error('no answer')))])
    return client
  } catch {
    return null
  }
}

// One chloedb-server per folder of apps, shared by every process that opens
// it: the chat, and each MCP server that Claude Desktop, a claude.ai
// connector or a Claude Code session starts. The first starts it on its own
// (it outlives that process, its output in chloedb-server.log) and writes
// where it listens to server.json; the others find it there. A lock file
// keeps two from starting it at once. Apps.stop(dir) ends it.
async function sharedServer({ bin, dir, log }) {
  mkdirSync(dir, { recursive: true })
  const file = path.join(dir, 'server.json')
  const lock = path.join(dir, 'server.lock')
  for (let i = 0; i < 300; i++) {
    const found = await running(file)
    if (found) return found
    let fd
    try {
      fd = openSync(lock, 'wx')
    } catch {
      // Another process is starting it; a lock older than 30 s is a crash's.
      try {
        if (Date.now() - statSync(lock).mtimeMs > 30000) rmSync(lock, { force: true })
      } catch {
        // Gone already.
      }
      await sleep(100)
      continue
    }
    try {
      const again = await running(file)
      if (again) return again
      const listen = `127.0.0.1:${await freePort()}`
      log(`starting ${bin} for the apps in ${dir} on ${listen}`)
      const out = openSync(path.join(dir, 'chloedb-server.log'), 'a')
      const child = spawn(bin, ['--db', path.join(dir, 'apps.strata'), '--seq-no', '--listen', listen, '--grpc', 'off'], { detached: true, stdio: ['ignore', out, out] })
      closeSync(out)
      child.unref()
      let exited = null
      child.on('exit', code => (exited = code))
      const client = new Strata(`http://${listen}`)
      for (let j = 0; j < 300 && exited === null; j++) {
        try {
          await client.health()
          writeFileSync(file, JSON.stringify({ url: client.url, pid: child.pid, started: Date.now() }))
          return client
        } catch {
          await sleep(100)
        }
      }
      child.kill()
      throw new Error(`chloedb-server for the apps did not start (exit ${exited}); see ${path.join(dir, 'chloedb-server.log')}`)
    } finally {
      closeSync(fd)
      rmSync(lock, { force: true })
    }
  }
  throw new Error(`chloedb-server for the apps in ${dir} did not start in time`)
}

function freePort() {
  return new Promise((resolve, reject) => {
    const s = net.createServer()
    s.on('error', reject)
    s.listen(0, '127.0.0.1', () => {
      const { port } = s.address()
      s.close(() => resolve(port))
    })
  })
}

// The value names an app's script reads from its data's values: values.cost,
// values['cost'], { cost, price } = data.values, and v.cost after
// const v = data.values; with the names it was last called with.
export function valueNames(source = '', args = {}) {
  const names = new Set()
  const add = name => {
    if (/^[A-Za-z_$][\w$]{0,40}$/.test(name) && !['length', 'map', 'forEach', 'filter', 'keys', 'entries'].includes(name)) names.add(name)
  }
  const code = String(source ?? '')
  for (const m of code.matchAll(/\bvalues\??\.([A-Za-z_$][\w$]*)/g)) add(m[1])
  for (const m of code.matchAll(/\bvalues\[\s*['"]([^'"]+)['"]\s*\]/g)) add(m[1])
  for (const m of code.matchAll(/\{([^{}]{1,400})\}\s*=\s*(?:[\w$.?]*\.)?values\b/g)) {
    for (const part of m[1].split(',')) add(part.split(/[:=]/)[0].trim())
  }
  for (const m of code.matchAll(/\b(?:const|let|var)\s+([A-Za-z_$][\w$]*)\s*=\s*(?:[\w$.?]*\.)?values\b(?!\s*[.[])/g)) {
    const alias = m[1].replace(/\$/g, '\\$')
    for (const n of code.matchAll(new RegExp(`\\b${alias}\\??\\.([A-Za-z_$][\\w$]*)`, 'g'))) add(n[1])
    for (const n of code.matchAll(new RegExp(`\\b${alias}\\[\\s*['"]([^'"]+)['"]\\s*\\]`, 'g'))) add(n[1])
  }
  for (const name of Object.keys(args?.values ?? {})) add(name)
  return [...names].slice(0, 24)
}

function tidy(row) {
  const out = { ...row }
  if (typeof out.args === 'string') out.args = JSON.parse(out.args || '{}')
  if (typeof out.tools === 'string') out.tools = [out.tools]
  out.tools ??= out.tool ? [out.tool] : []
  delete out.meaning
  return out
}

function untidy(app) {
  return { ...app, args: JSON.stringify(app.args ?? {}) }
}
