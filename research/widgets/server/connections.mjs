// What a person connected: their Spotify, an MCP server they signed in to.
// Each browser's own (the page's random id), kept in ChloeDB's table
// ia_connections and sealed with AES-256-GCM, so a row read from the
// database is no token. Never handed to an app, a link or the model: the
// server calls the service for the person, and the app gets the answer.
import { createCipheriv, createDecipheriv, createHash, randomBytes } from 'node:crypto'
import { mkdirSync, readFileSync, writeFileSync } from 'node:fs'
import path from 'node:path'

const TABLE = `CREATE TABLE ia_connections (id KEYWORD PRIMARY KEY, self KEYWORD, service KEYWORD, label TEXT, value TEXT OPTIONS(indexed = false), at INT)`

// A browser's own id, as the page makes it.
export const selfOf = value => (typeof value === 'string' && /^[A-Za-z0-9-]{16,64}$/.test(value) && !value.startsWith('s-') ? value : '')

// The sealing key: WIDGETS_SEAL_KEY, else one made once in the data folder
// (readable by its owner only). A lost key only means people connect again.
function sealKey(dir) {
  if (process.env.WIDGETS_SEAL_KEY) return createHash('sha256').update(process.env.WIDGETS_SEAL_KEY).digest()
  const file = path.join(dir, 'seal.key')
  try {
    return Buffer.from(readFileSync(file, 'utf8').trim(), 'hex')
  } catch {}
  const key = randomBytes(32)
  mkdirSync(dir, { recursive: true })
  writeFileSync(file, key.toString('hex'), { mode: 0o600, flag: 'wx' })
  return Buffer.from(readFileSync(file, 'utf8').trim(), 'hex')
}

const quote = text => `'${String(text).replace(/'/g, "''")}'`

export class Connections {
  constructor(db, key) {
    this.db = db
    this.key = key
  }

  static async open(db, dir) {
    const tables = new Set((await db.tables()).map(t => t.table ?? t.name ?? t))
    if (!tables.has('ia_connections'))
      await db.sql(TABLE).catch(error => {
        if (!/already exists/i.test(error.message)) throw error
      })
    return new Connections(db, sealKey(dir))
  }

  seal(value) {
    const iv = randomBytes(12)
    const cipher = createCipheriv('aes-256-gcm', this.key, iv)
    const body = Buffer.concat([cipher.update(JSON.stringify(value), 'utf8'), cipher.final()])
    return [iv, cipher.getAuthTag(), body].map(b => b.toString('base64')).join('.')
  }

  open(text) {
    try {
      const [iv, tag, body] = String(text).split('.').map(b => Buffer.from(b, 'base64'))
      const decipher = createDecipheriv('aes-256-gcm', this.key, iv)
      decipher.setAuthTag(tag)
      return JSON.parse(Buffer.concat([decipher.update(body), decipher.final()]).toString('utf8'))
    } catch {
      return null
    }
  }

  async get(self, service) {
    if (!selfOf(self)) return null
    const [row] = await this.db.get('ia_connections', [`${self}|${service}`])
    return row ? this.open(row.value) : null
  }

  async set(self, service, value, label = service) {
    if (!selfOf(self)) throw new Error('no browser to connect for')
    await this.db.write([{ op: 'upsert', table: 'ia_connections', doc: { id: `${self}|${service}`, self, service, label: String(label).slice(0, 200), value: this.seal(value), at: Date.now() } }])
  }

  async remove(self, service) {
    if (!selfOf(self)) return
    await this.db.write([{ op: 'delete', table: 'ia_connections', key: `${self}|${service}` }])
  }

  // What this browser connected: service and label, never the tokens.
  async list(self) {
    if (!selfOf(self)) return []
    const rows = await this.db.query(`SELECT service, label, at FROM ia_connections WHERE self = ${quote(self)} LIMIT 100`)
    return rows.map(({ service, label, at }) => ({ service, label, at }))
  }

  // One service of one browser, as a tool reads and keeps it.
  account(self, service) {
    return { get: () => this.get(self, service), set: (value, label) => this.set(self, service, value, label), remove: () => this.remove(self, service) }
  }
}
