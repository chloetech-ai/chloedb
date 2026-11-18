// The apps, kept in ChloeDB: every app the chat made or keeps (its source,
// each version, what it was asked for), and the search that brings a saved
// app back when a later request needs it, by words (BM25) and by meaning
// (a static embedding of its name, what it does and the requests it served)
// fused by rank.
import { randomBytes } from 'node:crypto'
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

// The apps "made before", so the rail is not empty on the first start: the
// three widgets, whose views were written by hand.
export const SEEDS = [
  { id: 'stock-chart', name: 'Stock chart', kind: 'info', tool: 'stock_quote', tools: ['stock_quote'], source: 'view:stock', args: { symbol: 'NVDA' },
    about: 'The price of a stock, ETF, index or crypto now, its change and a chart over a range.', examples: 'how is nvidia doing today' },
  { id: 'flight-tracker', name: 'Flight tracker', kind: 'info', tool: 'flight_track', tools: ['flight_track'], source: 'view:flight', args: { flight: 'LH773' },
    about: 'Where a flight is right now on a map, with its altitude, speed and heading.', examples: 'is lufthansa 773 in the air right now' },
  { id: 'shop', name: 'Shop', kind: 'action', tool: 'shop_search', tools: ['shop_search', 'cart_add', 'cart_view'], source: 'view:shop', args: { query: 'headphones' },
    about: 'Products that match a search, with pictures, prices and ratings, and a cart to add them to.', examples: 'wireless earphones under 100 dollars' },
]

const id = name => `${name.toLowerCase().replace(/[^a-z0-9]+/g, '-').replace(/^-|-$/g, '').slice(0, 24)}-${randomBytes(3).toString('hex')}`
const now = () => Date.now()

export class Apps {
  constructor(db, { meaning }) {
    this.db = db
    this.meaning = meaning
  }

  // Opens the store at CHLOEDB_URL, or starts chloedb-server on `dir`.
  static async open({ dir, url = process.env.CHLOEDB_URL, log = () => {} } = {}) {
    let db
    let child = null
    if (url) db = new Strata(url, process.env.CHLOEDB_KEY)
    else {
      const bin = process.env.CHLOEDB_SERVER_BIN || findServerBinary(REPO)
      if (!bin) throw new Error('no chloedb-server: build it (cd chloedb-server && cargo build --release), or set CHLOEDB_SERVER_BIN or CHLOEDB_URL')
      const port = await freePort()
      const started = await startServer({ bin, db: path.join(dir, 'apps.strata'), listen: `127.0.0.1:${port}`, log })
      db = started.client
      child = started.child
    }
    const have = new Set((await db.tables()).map(t => t.name ?? t))
    let meaning = have.has('ia_apps') ? undefined : null
    if (!have.has('ia_apps')) {
      try {
        await db.sql(APPS.replace('MEANING', `, meaning VECTOR OPTIONS(embed = 'name, about, examples', model = '${MODEL}')`))
        meaning = true
      } catch (error) {
        // No embedding model here (chloedb-server fetch-model fetches it):
        // recall by words alone.
        log(`apps by words only: ${error.message}`)
        await db.sql(APPS.replace('MEANING', ''))
        meaning = false
      }
      await db.sql(VERSIONS)
      await db.sql(TURNS)
    }
    if (meaning === undefined) {
      const schema = await db.req('GET', '/v1/tables/ia_apps').catch(() => null)
      meaning = JSON.stringify(schema ?? {}).includes('"meaning"')
    }
    const apps = new Apps(db, { meaning })
    apps.child = child
    if (!(await db.query('SELECT id FROM ia_apps LIMIT 1')).length) {
      for (const seed of SEEDS) await apps.put({ ...seed, saved: true, uses: 1 }, 'made by hand')
    }
    return apps
  }

  close() {
    this.child?.kill()
  }

  // The saved apps that may fit a request, best first.
  async recall(text, limit = 8) {
    const fused = this.meaning
      ? 'rrf(bm25(name, :q), bm25(about, :q), bm25(examples, :q), similarity(meaning, :q))'
      : 'rrf(bm25(name, :q), bm25(about, :q), bm25(examples, :q))'
    const rows = await this.db.query(
      `SELECT id, name, about, kind, tool, tools, examples, uses, score() AS score FROM ia_apps WHERE saved = true ORDER BY ${fused} DESC LIMIT ${limit}`,
      { q: text },
    )
    return rows.map(tidy)
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
    return tidy(row)
  }

  async change(appId, source, change) {
    const app = await this.get(appId)
    return this.put({ ...app, source, version: app.version + 1 }, change)
  }

  async undo(appId) {
    const app = await this.get(appId)
    if (app.version <= 1) return app
    const [before] = await this.db.get('ia_versions', [`${appId}@${app.version - 1}`])
    await this.db.write([{ op: 'delete', table: 'ia_versions', key: `${appId}@${app.version}` }])
    return this.put({ ...app, source: before.source, version: app.version - 1 }, null)
  }

  async save(appId, saved = true, name) {
    const app = await this.get(appId)
    return this.put({ ...app, saved, name: name || app.name }, null)
  }

  // An app served a request: counted, its arguments kept for the rail, and
  // the request added to those it is recalled by.
  async used(appId, request, args, how) {
    const app = await this.get(appId)
    const examples = [request, ...app.examples.split(' | ').filter(e => e && e !== request)].slice(0, 8).join(' | ')
    await this.db.write([
      { op: 'upsert', table: 'ia_apps', doc: { ...untidy(app), examples, args: JSON.stringify(args ?? {}), uses: app.uses + 1, used: now() } },
      { op: 'upsert', table: 'ia_turns', doc: { id: `${now()}-${randomBytes(3).toString('hex')}`, request, app: appId, how, at: now() } },
    ])
  }
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
