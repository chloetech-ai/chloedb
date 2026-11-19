// Reads a web page for the chat's research step, as an MCP server with one
// tool, read_page: the page's title and text, so Claude can take the facts
// from the page itself (a review's measurements, a shop's price) rather
// than from a search result's snippet.
//
// It reads the public web and nothing else. Claude Code's own WebFetch
// would reach whatever the machine reaches, which on the cluster is the
// chloedb-server beside the chat and the services around it, and a page
// Claude reads can ask it to fetch anything. So an address is read only
// over http(s) on its usual port, every address its name resolves to must
// be public, the connection goes to the address that was checked (a name
// that resolves again elsewhere changes nothing), and each redirect is
// checked the same way.
//
//   node server/reader.mjs      (stdio; the chat names it in --mcp-config)
import { lookup } from 'node:dns/promises'
import http from 'node:http'
import https from 'node:https'
import net from 'node:net'
import { fileURLToPath } from 'node:url'
import { resolve } from 'node:path'

const MAX_BYTES = 3_000_000
const MAX_TEXT = 15_000
const TIMEOUT_MS = 12_000

// Whether an address is on the public internet: not this machine, a
// private network, a link (the cloud's metadata service), shared address
// space, multicast or reserved.
export function publicAddress(ip) {
  if (net.isIPv4(ip)) {
    const [a, b] = ip.split('.').map(Number)
    if (a === 0 || a === 10 || a === 127 || a >= 224) return false
    if (a === 100 && b >= 64 && b <= 127) return false
    if (a === 169 && b === 254) return false
    if (a === 172 && b >= 16 && b <= 31) return false
    if (a === 192 && (b === 168 || (b === 0 && ip.startsWith('192.0.0.')) || (b === 0 && ip.startsWith('192.0.2.')))) return false
    if (a === 198 && (b === 18 || b === 19)) return false
    return true
  }
  if (net.isIPv6(ip)) {
    const v = ip.toLowerCase()
    // Every address whose first 96 bits are zero or ::ffff (IPv4 inside,
    // dotted or in hex: ::ffff:a00:1 is 10.0.0.1), and 0:..., is not.
    if (v.startsWith('::') || /^0+:/.test(v)) return false
    if (/^f[cd]/.test(v) || /^fe[89ab]/.test(v) || v.startsWith('ff')) return false
    if (v.startsWith('64:ff9b:') || v.startsWith('2001:db8')) return false
    return true
  }
  return false
}

// The address to connect to for a URL, once it is checked; or why not.
async function checked(url) {
  let u
  try {
    u = new URL(url)
  } catch {
    throw new Error('not an address')
  }
  if (u.protocol !== 'https:' && u.protocol !== 'http:') throw new Error('only http and https pages are read')
  if (u.username || u.password) throw new Error('addresses with a user or password are not read')
  if (u.port && u.port !== '80' && u.port !== '443') throw new Error('only the usual ports are read')
  const host = u.hostname.replace(/^\[|\]$/g, '')
  if (/^localhost$|\.(localhost|local|internal|svc|cluster\.local|home|lan)$/i.test(host)) throw new Error('not a public address')
  const found = net.isIP(host) ? [{ address: host, family: net.isIP(host) }] : await lookup(host, { all: true, verbatim: true })
  if (!found.length || !found.every(a => publicAddress(a.address))) throw new Error('not a public address')
  return { u, address: found[0] }
}

function get(u, address) {
  const lib = u.protocol === 'https:' ? https : http
  return new Promise((resolve, reject) => {
    const req = lib.request(
      u,
      {
        method: 'GET',
        headers: { 'user-agent': 'Mozilla/5.0 (compatible; instant-apps-reader/0.1)', accept: 'text/html,text/plain;q=0.9,*/*;q=0.1', 'accept-language': 'en' },
        // To the address checked, whatever the name resolves to now; the
        // name still goes in the TLS handshake and the Host header.
        lookup: (_host, options, done) => (options?.all ? done(null, [address]) : done(null, address.address, address.family)),
        timeout: TIMEOUT_MS,
      },
      res => resolve(res),
    )
    req.on('timeout', () => req.destroy(new Error('the page took too long')))
    req.on('error', reject)
    req.end()
  })
}

// The page at a public address: its final address, title and text.
export async function readPage(url, { redirects = 4 } = {}) {
  let at = url
  for (let hop = 0; hop <= redirects; hop++) {
    const { u, address } = await checked(at)
    const res = await get(u, address)
    if (res.statusCode >= 300 && res.statusCode < 400 && res.headers.location) {
      res.resume()
      at = new URL(res.headers.location, u).href
      continue
    }
    if (res.statusCode >= 400) {
      res.resume()
      throw new Error(`the page answered ${res.statusCode}`)
    }
    const type = String(res.headers['content-type'] ?? '')
    if (type && !/text\/html|text\/plain|application\/xhtml/i.test(type)) {
      res.resume()
      throw new Error(`not a page of text (${type.split(';')[0]})`)
    }
    const chunks = []
    let size = 0
    for await (const chunk of res) {
      size += chunk.length
      if (size > MAX_BYTES) {
        res.destroy()
        break
      }
      chunks.push(chunk)
    }
    const body = Buffer.concat(chunks).toString('utf8')
    return { url: u.href, ...text(body, /html/i.test(type) || /<html|<body/i.test(body.slice(0, 2000))) }
  }
  throw new Error('too many redirects')
}

const ENTITIES = { amp: '&', lt: '<', gt: '>', quot: '"', apos: "'", nbsp: ' ', mdash: '—', ndash: '–', hellip: '…', rsquo: '’', lsquo: '‘', rdquo: '”', ldquo: '“', pound: '£', euro: '€', copy: '©', reg: '®', trade: '™', deg: '°' }
const decode = s =>
  s.replace(/&(#x[0-9a-f]+|#\d+|[a-z]+);/gi, (m, e) => {
    if (e[0] === '#') {
      const code = e[1].toLowerCase() === 'x' ? parseInt(e.slice(2), 16) : parseInt(e.slice(1), 10)
      return Number.isFinite(code) && code > 0 && code < 0x110000 ? String.fromCodePoint(code) : m
    }
    return ENTITIES[e.toLowerCase()] ?? m
  })

// A page's title and readable text: no scripts, styles, menus or forms;
// a line for each block, and tables as rows of cells.
export function text(body, html = true) {
  if (!html) return { title: null, text: body.replace(/\s+\n/g, '\n').trim().slice(0, MAX_TEXT) }
  const title = decode((/<title[^>]*>([\s\S]*?)<\/title>/i.exec(body)?.[1] ?? '').replace(/\s+/g, ' ').trim()) || null
  // The page's main part when it has one that holds most of its text.
  const whole = lines(body)
  const main = /<(main|article)[\s>][\s\S]*<\/\1>/i.exec(body)?.[0]
  const part = main ? lines(main) : null
  const chosen = part && part.join('\n').length > whole.join('\n').length * 0.4 ? part : whole
  return { title, text: chosen.join('\n').slice(0, MAX_TEXT) }
}

function lines(html) {
  const out = html
    .replace(/<!--[\s\S]*?-->/g, ' ')
    .replace(/<(script|style|noscript|svg|nav|footer|header|aside|form|iframe|template|button|select)\b[\s\S]*?<\/\1>/gi, ' ')
    .replace(/<\/(td|th)>/gi, ' | ')
    .replace(/<(br|\/p|\/div|\/li|\/tr|\/h[1-6]|\/section|\/blockquote|\/dd|\/dt)\b[^>]*>/gi, '\n')
    .replace(/<li\b[^>]*>/gi, '\n- ')
    .replace(/<h([1-6])\b[^>]*>/gi, (m, n) => `\n${'#'.repeat(Number(n))} `)
    .replace(/<[^>]+>/g, ' ')
  return decode(out)
    .split('\n')
    .map(line => line.replace(/[ \t\f\v ]+/g, ' ').trim())
    .filter((line, i, all) => line && line !== all[i - 1])
}

async function main() {
  const { McpServer } = await import('@modelcontextprotocol/sdk/server/mcp.js')
  const { StdioServerTransport } = await import('@modelcontextprotocol/sdk/server/stdio.js')
  const { z } = await import('zod')
  const server = new McpServer({ name: 'reader', version: '0.1.0' })
  server.registerTool(
    'read_page',
    {
      title: 'Read a web page',
      description: 'Reads a public web page (one a search found) and returns its title and text, up to about 15,000 characters, to take facts from the page itself.',
      inputSchema: { url: z.string().url().max(2000).describe('The page’s address, https://...') },
      annotations: { readOnlyHint: true, openWorldHint: true },
    },
    async ({ url }) => {
      try {
        const page = await readPage(url)
        return { content: [{ type: 'text', text: `${page.title ? `# ${page.title}\n` : ''}${page.url}\n\n${page.text}` }] }
      } catch (error) {
        return { content: [{ type: 'text', text: `Could not read ${url}: ${error.message}` }], isError: true }
      }
    },
  )
  await server.connect(new StdioServerTransport())
}

if (process.argv[1] && fileURLToPath(import.meta.url) === resolve(process.argv[1])) {
  main().catch(error => {
    console.error(error)
    process.exit(1)
  })
}
