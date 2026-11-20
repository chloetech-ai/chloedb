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

function get(u, address, { accept = 'text/html,text/plain;q=0.9,*/*;q=0.1', timeout = TIMEOUT_MS } = {}) {
  const lib = u.protocol === 'https:' ? https : http
  return new Promise((resolve, reject) => {
    const req = lib.request(
      u,
      {
        method: 'GET',
        headers: { 'user-agent': 'Mozilla/5.0 (compatible; instant-apps-reader/0.1)', accept, 'accept-language': 'en' },
        // To the address checked, whatever the name resolves to now; the
        // name still goes in the TLS handshake and the Host header.
        lookup: (_host, options, done) => (options?.all ? done(null, [address]) : done(null, address.address, address.family)),
        timeout,
      },
      res => resolve(res),
    )
    req.on('timeout', () => req.destroy(new Error('it took too long')))
    req.on('error', reject)
    req.end()
  })
}

// What a public address holds, when it is one of `types`: its final
// address, type and bytes, up to maxBytes (cut there, or refused when
// `whole` and it is longer).
async function fetchPublic(url, { types, accept, maxBytes = MAX_BYTES, whole = false, timeout, redirects = 4 }) {
  let at = url
  for (let hop = 0; hop <= redirects; hop++) {
    const { u, address } = await checked(at)
    const res = await get(u, address, { accept, timeout })
    if (res.statusCode >= 300 && res.statusCode < 400 && res.headers.location) {
      res.resume()
      at = new URL(res.headers.location, u).href
      continue
    }
    if (res.statusCode >= 400) {
      res.resume()
      throw new Error(`it answered ${res.statusCode}`)
    }
    const type = String(res.headers['content-type'] ?? '').split(';')[0].trim().toLowerCase()
    if (!types.test(type)) {
      res.resume()
      throw new Error(`not what was asked for (${type || 'no type'})`)
    }
    if (whole && Number(res.headers['content-length']) > maxBytes) {
      res.resume()
      throw new Error('too big')
    }
    const chunks = []
    let size = 0
    for await (const chunk of res) {
      size += chunk.length
      if (size > maxBytes) {
        res.destroy()
        if (whole) throw new Error('too big')
        break
      }
      chunks.push(chunk)
    }
    return { url: u.href, type, bytes: Buffer.concat(chunks) }
  }
  throw new Error('too many redirects')
}

// The page at a public address: its final address, title, text and the
// pictures on it.
export async function readPage(url, { timeout } = {}) {
  const got = await fetchPublic(url, { types: /^(text\/html|text\/plain|application\/xhtml\+xml|)$/, timeout })
  const body = got.bytes.toString('utf8')
  return { url: got.url, ...text(body, /html/.test(got.type) || /<html|<body/i.test(body.slice(0, 2000)), got.url) }
}

// A picture at a public address (a photo a page shows), when it is a
// JPEG, PNG, WebP, GIF or AVIF of at most maxBytes.
export async function fetchPicture(url, { maxBytes = 400_000, timeout = 6000 } = {}) {
  const got = await fetchPublic(url, { types: /^image\/(jpeg|png|webp|gif|avif)$/, accept: 'image/avif,image/webp,image/jpeg,image/png,image/gif;q=0.9', maxBytes, whole: true, timeout })
  return { url: got.url, type: got.type, bytes: got.bytes }
}

// JSON at a public address (an open API such as Wikipedia's).
export async function fetchJson(url, { timeout = 6000 } = {}) {
  const got = await fetchPublic(url, { types: /^application\/(json|[a-z.+-]*\+json)$/, accept: 'application/json', maxBytes: 1_000_000, whole: true, timeout })
  return JSON.parse(got.bytes.toString('utf8'))
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
// a line for each block, and tables as rows of cells; and its pictures.
export function text(body, html = true, url = null) {
  if (!html) return { title: null, text: body.replace(/\s+\n/g, '\n').trim().slice(0, MAX_TEXT), pictures: [] }
  const title = decode((/<title[^>]*>([\s\S]*?)<\/title>/i.exec(body)?.[1] ?? '').replace(/\s+/g, ' ').trim()) || null
  // The page's main part when it has one that holds most of its text.
  const whole = lines(body)
  const main = /<(main|article)[\s>][\s\S]*<\/\1>/i.exec(body)?.[0]
  const part = main ? lines(main) : null
  const chosen = part && part.join('\n').length > whole.join('\n').length * 0.4 ? part : whole
  return { title, text: chosen.join('\n').slice(0, MAX_TEXT), pictures: url ? pictures(body, main && chosen === part ? main : body, url) : [] }
}

const attr = (tag, name) => {
  const m = new RegExp(`\\s${name}\\s*=\\s*(?:"([^"]*)"|'([^']*)'|([^\\s>]+))`, 'i').exec(tag)
  return m ? decode(m[1] ?? m[2] ?? m[3] ?? '') : null
}
const NOT_A_PHOTO = /logo|icon|sprite|avatar|badge|pixel|spacer|blank|placeholder|loading|1x1|gravatar|emoji|flag|button|banner-ad|doubleclick|\/ads?\//i

// The pictures a page shows, its own first (og:image, twitter:image),
// then the photos in its main part with their alt text; no icons, logos
// or tracking pixels, at most 8.
export function pictures(head, part, base) {
  const out = []
  const add = (src, alt) => {
    if (!src || /^data:/i.test(src)) return
    let href
    try {
      href = new URL(src.trim(), base)
    } catch {
      return
    }
    if (!/^https?:$/.test(href.protocol) || /\.svg(\?|$)/i.test(href.pathname) || NOT_A_PHOTO.test(href.pathname)) return
    if (!out.some(p => p.url === href.href)) out.push({ url: href.href, alt: alt ? alt.replace(/\s+/g, ' ').trim().slice(0, 120) : null })
  }
  for (const tag of head.match(/<meta\b[^>]*>/gi) ?? []) {
    if (/^(og:image(:secure_url|:url)?|twitter:image(:src)?)$/i.test(attr(tag, 'property') ?? attr(tag, 'name') ?? '')) add(attr(tag, 'content'), null)
  }
  for (const tag of part.match(/<img\b[^>]*>/gi) ?? []) {
    const w = Number(attr(tag, 'width')), h = Number(attr(tag, 'height'))
    if ((w && w < 120) || (h && h < 90)) continue
    const set = attr(tag, 'srcset') ?? attr(tag, 'data-srcset')
    const best = set
      ?.split(',')
      .map(s => s.trim().split(/\s+/))
      .map(([u, d]) => ({ u, n: parseFloat(d) || 1 }))
      .filter(c => c.n <= 1200)
      .sort((a, b) => b.n - a.n)[0]?.u
    add(attr(tag, 'data-src') ?? best ?? attr(tag, 'src'), attr(tag, 'alt'))
    if (out.length >= 8) break
  }
  return out.slice(0, 8)
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
      description: 'Reads a public web page (one a search found) and returns its title and text, up to about 15,000 characters, to take facts from the page itself, and the addresses of the pictures it shows.',
      inputSchema: { url: z.string().url().max(2000).describe('The page’s address, https://...') },
      annotations: { readOnlyHint: true, openWorldHint: true },
    },
    async ({ url }) => {
      try {
        const page = await readPage(url)
        const pics = page.pictures.length ? `\n\nPictures on the page:\n${page.pictures.map(p => `- ${p.url}${p.alt ? ` (${p.alt})` : ''}`).join('\n')}` : ''
        return { content: [{ type: 'text', text: `${page.title ? `# ${page.title}\n` : ''}${page.url}\n\n${page.text}${pics}` }] }
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
