// Outside services as the person: their Spotify (a fake of its accounts
// and API here), and a service found in the MCP registry (a fake registry
// pointing at a real MCP server on this machine), with a scripted model and
// a real chloedb-server.
//
//   node --test test/connect.test.mjs     (needs chloedb-server: CHLOEDB_SERVER_BIN, or a build in the checkout)
import assert from 'node:assert/strict'
import { mkdtempSync, rmSync } from 'node:fs'
import { createServer } from 'node:http'
import os from 'node:os'
import path from 'node:path'
import { after, before, test } from 'node:test'

import { McpServer } from '../server/node_modules/@modelcontextprotocol/sdk/dist/esm/server/mcp.js'
import { StreamableHTTPServerTransport } from '../server/node_modules/@modelcontextprotocol/sdk/dist/esm/server/streamableHttp.js'
import { z } from '../server/node_modules/zod/index.js'

const { findServerBinary } = await import('../../../chloedb-apps/engine/strata.mjs')
const bin = process.env.CHLOEDB_SERVER_BIN || findServerBinary(new URL('../../../', import.meta.url).pathname)
const skip = bin ? false : 'no chloedb-server binary (set CHLOEDB_SERVER_BIN)'
if (bin) process.env.CHLOEDB_SERVER_BIN = bin

// The fakes: Spotify's accounts and API, the registry, and an MCP server.
const seen = { tokens: [], made: [], items: [], auth: [] }
let fakes
function weather() {
  const server = new McpServer({ name: 'weather', version: '1.0.0' })
  server.registerTool('forecast', { description: 'The forecast for a city', inputSchema: { city: z.string() }, annotations: { readOnlyHint: true } }, async ({ city }) => ({
    content: [{ type: 'text', text: JSON.stringify({ city, days: [{ day: 'Fri', high: 31 }, { day: 'Sat', high: 33 }] }) }],
  }))
  server.registerTool('alert_me', { description: 'Sends an alert', inputSchema: { city: z.string() } }, async () => ({ content: [{ type: 'text', text: 'sent' }] }))
  return server
}
async function fakeServer(req, res) {
  const url = new URL(req.url, 'http://x')
  const body = await new Promise(resolve => {
    let b = ''
    req.on('data', c => (b += c))
    req.on('end', () => resolve(b))
  })
  const json = (v, status = 200) => res.writeHead(status, { 'content-type': 'application/json' }).end(JSON.stringify(v))
  if (url.pathname === '/mcp') {
    const transport = new StreamableHTTPServerTransport({ sessionIdGenerator: undefined, enableJsonResponse: true })
    const server = weather()
    res.on('close', () => (transport.close(), server.close()))
    await server.connect(transport)
    return transport.handleRequest(req, res, body ? JSON.parse(body) : undefined)
  }
  if (url.pathname === '/v0/servers') {
    const port = fakes.address().port
    return json({ servers: [
      { server: { name: 'example/keyed-weather', description: 'Weather with a key', remotes: [{ type: 'streamable-http', url: 'https://keyed.example/mcp', headers: [{ name: 'Authorization', isRequired: true }] }] } },
      { server: { name: 'example/weather', title: 'Weather', description: 'Forecasts for any city', remotes: [{ type: 'streamable-http', url: `http://127.0.0.1:${port}/mcp` }] }, _meta: { 'io.modelcontextprotocol.registry/official': { status: 'active' } } },
    ] })
  }
  if (url.pathname === '/api/token') {
    const form = new URLSearchParams(body)
    seen.tokens.push(Object.fromEntries(form))
    if (form.get('grant_type') === 'authorization_code' && form.get('code') !== 'good') return json({ error: 'invalid_grant' }, 400)
    return json({ access_token: 'secret-access', refresh_token: 'secret-refresh', expires_in: 3600, scope: 'x' })
  }
  if (!url.pathname.startsWith('/v1/')) return json({ error: 'no' }, 404)
  seen.auth.push(req.headers.authorization)
  if (req.headers.authorization !== 'Bearer secret-access') return json({ error: { status: 401, message: 'bad token' } }, 401)
  if (url.pathname === '/v1/me') return json({ id: 'rk', display_name: 'Roberto' })
  if (url.pathname === '/v1/search') {
    const q = url.searchParams.get('q')
    if (/nowhere/i.test(q)) return json({ tracks: { items: [] } })
    return json({ tracks: { items: [{ name: q.split(' ')[0], artists: [{ name: 'Someone' }], album: { name: 'An album', images: [{ url: 'https://i.scdn.co/image/abc', width: 300 }] }, uri: 'spotify:track:6habFhsOp2NvshLv26DqMb', external_urls: { spotify: 'https://open.spotify.com/track/6habFhsOp2NvshLv26DqMb' }, duration_ms: 200000 }] } })
  }
  if (url.pathname === '/v1/me/playlists' && req.method === 'POST') {
    seen.made.push(JSON.parse(body))
    return json({ id: 'p1', name: JSON.parse(body).name, uri: 'spotify:playlist:p1', external_urls: { spotify: 'https://open.spotify.com/playlist/p1' } }, 201)
  }
  if (url.pathname === '/v1/playlists/p1/items' && req.method === 'POST') {
    seen.items.push(JSON.parse(body))
    return json({ snapshot_id: 's' }, 201)
  }
  return json({ error: { status: 404, message: 'no' } }, 404)
}

const FRAGMENT = `<div id="root"></div><script type="module">Kit.connect('t', d => { document.getElementById('root').textContent = d.summary })</script>`
const model = {
  async route(input) {
    if (/playlist/i.test(input.request)) return { mode: 'make', kind: 'tool', tool: 'spotify_search', args: { songs: ['Midnight City - M83', 'Nowhere Song - Nobody'] }, why: 'no playlist app yet', make: { name: 'Playlist maker', about: 'Songs for a moment, saved to Spotify.', look: 'Songs with covers; Save.', tools: ['spotify_search', 'spotify_playlist_make', 'spotify_play'] } }
    if (/weather/i.test(input.request)) return { mode: 'make', kind: 'info', tool: 'connector', args: { service: 'weather' }, why: 'a weather service', make: { name: 'Weather', about: 'A forecast.', look: 'Days.', tools: ['connector', 'connector_call'] }, connected: input.connected }
    return { mode: 'chat', reply: 'Just text.' }
  },
  async connector(input) {
    seen.servers = input.servers
    return { servers: input.servers.map((_, i) => i) }
  },
  async connectorTool(input) {
    seen.tools = input.tools
    return { tool: 'forecast', arguments: { city: 'Indio' } }
  },
  async make() {
    return FRAGMENT
  },
  async reply() {
    return 'ok'
  },
}

let chat
let dir
const ME = 'me-0123456789abcdef'
const OTHER = 'other-0123456789abcdef'
before(async () => {
  if (skip) return
  fakes = createServer(fakeServer)
  await new Promise(resolve => fakes.listen(0, '127.0.0.1', resolve))
  const at = `http://127.0.0.1:${fakes.address().port}`
  Object.assign(process.env, { SPOTIFY_CLIENT_ID: 'test-client', SPOTIFY_ACCOUNTS: at, SPOTIFY_API: `${at}/v1`, MCP_REGISTRY: at, MCP_ALLOW_LOCAL: '1' })
  const { start } = await import('../chat/server.mjs')
  dir = mkdtempSync(path.join(os.tmpdir(), 'instant-apps-connect-'))
  chat = await start({ port: 0, data: dir, model, log: () => {}, shared: false })
})
after(() => {
  chat?.close()
  fakes?.close()
  fakes?.closeAllConnections()
  if (dir) rmSync(dir, { recursive: true, force: true })
})

const turn = async (text, self = ME) => {
  const res = await fetch(`${chat.url}api/turn`, { method: 'POST', headers: { 'x-self': self }, body: JSON.stringify({ text, history: [], screen: [], talk: 'talk-0123456789abcdef' }) })
  return (await res.text()).trim().split('\n').map(line => JSON.parse(line))
}
const call = async (body, self = ME) => (await fetch(`${chat.url}api/call`, { method: 'POST', headers: { 'x-self': self }, body: JSON.stringify(body) })).json()

test('Spotify is the person’s own: a sign-in first, sealed tokens, then songs and a playlist as them', { skip }, async () => {
  // Not connected: the chat offers the sign-in and makes nothing.
  let evs = await turn('make me a playlist for the drive to Coachella')
  const connect = evs.find(e => e.type === 'connect')
  assert.ok(connect, 'a sign-in is offered')
  assert.equal(connect.name, 'Spotify')
  assert.ok(!evs.some(e => e.type === 'making' || e.type === 'app'), 'no app before the sign-in')
  // The sign-in: to Spotify with PKCE and a state, and back.
  const go = await fetch(new URL(connect.url, chat.url), { redirect: 'manual' })
  assert.equal(go.status, 302)
  const to = new URL(go.headers.get('location'))
  assert.equal(to.pathname, '/authorize')
  assert.equal(to.searchParams.get('code_challenge_method'), 'S256')
  assert.equal(to.searchParams.get('client_id'), 'test-client')
  const bad = await fetch(`${chat.url}api/connect/spotify/done?code=good&state=forged`)
  assert.equal(bad.status, 400, 'a state this chat did not start is refused')
  const done = await fetch(`${chat.url}api/connect/spotify/done?code=good&state=${to.searchParams.get('state')}`)
  assert.equal(done.status, 200)
  assert.match(await done.text(), /Roberto/)
  assert.ok(seen.tokens.at(-1).code_verifier, 'the code is exchanged with its verifier, no secret')
  // Kept sealed: the database holds no token.
  const [row] = await chat.apps.db.get('ia_connections', [`${ME}|spotify`])
  assert.ok(row, 'kept')
  assert.doesNotMatch(row.value, /secret-access|secret-refresh/)
  const listed = await (await fetch(`${chat.url}api/connections`, { headers: { 'x-self': ME } })).json()
  assert.deepEqual(listed.connected.map(c => c.service), ['spotify'])
  // The same request again: the songs found as them, the ones not found said.
  evs = await turn('make me a playlist for the drive to Coachella')
  const app = evs.find(e => e.type === 'app')
  assert.ok(app, 'the app is made once connected')
  assert.equal(app.result.structuredContent.items.length, 1)
  assert.deepEqual(app.result.structuredContent.missing, ['Nowhere Song - Nobody'])
  assert.equal(app.result.structuredContent.items[0].image, 'https://i.scdn.co/image/abc')
  assert.match(app.html, /i\.scdn\.co/, 'covers may load in the app')
  assert.doesNotMatch(JSON.stringify(app), /secret-access/, 'no token reaches the page')
  // A playlist made from the app's button, in their account.
  const made = await call({ app: app.app.id, me: ME, self: ME, name: 'spotify_playlist_make', arguments: { name: 'Coachella drive', uris: ['spotify:track:6habFhsOp2NvshLv26DqMb'] } })
  assert.ok(!made.isError, JSON.stringify(made))
  assert.equal(made.structuredContent.made.url, 'https://open.spotify.com/playlist/p1')
  assert.equal(made.acted, true)
  assert.deepEqual(seen.items.at(-1).uris, ['spotify:track:6habFhsOp2NvshLv26DqMb'])
  // Another browser has no Spotify of its own: it is asked to connect, never given this one's.
  const theirs = await call({ app: app.app.id, me: OTHER, self: OTHER, name: 'spotify_search', arguments: { query: 'M83' } }, OTHER)
  assert.equal(theirs.isError, true)
  assert.equal(theirs.connect.name, 'Spotify')
  // Disconnecting takes it away.
  await fetch(`${chat.url}api/disconnect`, { method: 'POST', headers: { 'x-self': ME }, body: JSON.stringify({ service: 'spotify' }) })
  const gone = await call({ app: app.app.id, me: ME, self: ME, name: 'spotify_search', arguments: { query: 'M83' } })
  assert.equal(gone.connect?.service, 'spotify')
})

test('a service found in the MCP registry is connected on the fly, read only until a tap', { skip }, async () => {
  const evs = await turn('what is the weather in Indio this weekend')
  const app = evs.find(e => e.type === 'app')
  assert.ok(app, JSON.stringify(evs.filter(e => e.type === 'reply' || e.type === 'error')))
  assert.deepEqual(seen.servers.map(s => s.name), ['example/weather'], 'a server that wants a key of its own is left out')
  const data = app.result.structuredContent
  assert.equal(data.tool, 'forecast')
  assert.deepEqual(data.result.days.map(d => d.high), [31, 33])
  assert.deepEqual(data.tools.map(t => [t.name, t.reads]), [['forecast', true], ['alert_me', false]])
  // Remembered for this browser, so the next request goes straight there.
  const listed = await (await fetch(`${chat.url}api/connections`, { headers: { 'x-self': ME } })).json()
  assert.ok(listed.connected.some(c => c.service === 'mcp' && c.url === data.server.url))
  // The app's buttons reach its own server, and no other.
  const more = await call({ app: app.app.id, me: ME, self: ME, name: 'connector_call', arguments: { server: data.server.url, tool: 'forecast', arguments: { city: 'Palm Springs' } } })
  assert.equal(more.structuredContent.result.city, 'Palm Springs')
  const elsewhere = await call({ app: app.app.id, me: ME, self: ME, name: 'connector_call', arguments: { server: 'https://evil.example/mcp', tool: 'x' } })
  assert.equal(elsewhere.isError, true)
})

test('a server on a private address is never connected to', { skip }, async () => {
  const { publicUrl } = await import('../server/widgets/connector.mjs')
  for (const url of ['https://169.254.169.254/mcp', 'https://10.0.0.1/mcp', 'http://example.com/mcp', 'https://kubernetes.default.svc/mcp']) await assert.rejects(publicUrl(url), url)
})
