// Any service with an MCP server, connected when a request needs it: the
// public MCP registry (registry.modelcontextprotocol.io) is searched for
// the service, a remote server of it (Streamable HTTP or SSE) is connected
// to, and its tools fill an app and act for its buttons. A server that
// wants a sign-in gets one (OAuth as MCP specifies it: the server's
// authorization server, a client registered on the fly, PKCE), kept per
// browser and sealed (connections.mjs). What a server answers is data for
// the app, never instructions to the chat.
import { lookup } from 'node:dns/promises'
import net from 'node:net'
import { randomBytes } from 'node:crypto'

import { Client } from '@modelcontextprotocol/sdk/client/index.js'
import { SSEClientTransport } from '@modelcontextprotocol/sdk/client/sse.js'
import { StreamableHTTPClientTransport } from '@modelcontextprotocol/sdk/client/streamableHttp.js'
import { UnauthorizedError, auth } from '@modelcontextprotocol/sdk/client/auth.js'
import { z } from 'zod'

import { publicAddress } from '../reader.mjs'
import { NeedsConnect } from './spotify.mjs'

const REGISTRY = () => process.env.MCP_REGISTRY || 'https://registry.modelcontextprotocol.io'
const LOCAL = () => /^(1|true|yes)$/i.test(process.env.MCP_ALLOW_LOCAL ?? '')
const TIMEOUT = 15_000

// Only the public internet: a server address of this machine or a private
// network (the cloud's metadata service, the cluster) is never reached.
export async function publicUrl(url) {
  const u = new URL(url)
  if (LOCAL() && /^(127\.0\.0\.1|localhost)$/.test(u.hostname)) return u
  if (u.protocol !== 'https:') throw new Error('only https servers are connected to')
  if (u.username || u.password) throw new Error('addresses with a user or password are not connected to')
  const host = u.hostname.replace(/^\[|\]$/g, '')
  if (/^localhost$|\.(localhost|local|internal|svc|cluster\.local|home|lan)$/i.test(host)) throw new Error('not a public address')
  const found = net.isIP(host) ? [{ address: host }] : await lookup(host, { all: true, verbatim: true })
  if (!found.length || !found.every(a => publicAddress(a.address))) throw new Error('not a public address')
  return u
}
const guarded = async (url, init) => {
  await publicUrl(String(url instanceof Request ? url.url : url))
  return fetch(url, { ...init, signal: init?.signal ?? AbortSignal.timeout(TIMEOUT) })
}

const STOP = new Set('mcp server servers the and for with from that this what which my your our app apps service services connect connector data'.split(' '))

// Remote servers in the registry for a service ("notion", "weather"):
// each one's newest version, only those reachable over the web with no
// key of their own to fill in (a sign-in is fine; an API key header is not).
export async function registrySearch(service, limit = 12) {
  const words = [service, ...(String(service).toLowerCase().match(/[a-z0-9]{3,}/g) ?? []).filter(w => !STOP.has(w))]
  const seen = new Map()
  for (const word of [...new Set(words)].slice(0, 4)) {
    let body
    try {
      const res = await fetch(`${REGISTRY()}/v0/servers?${new URLSearchParams({ search: word, limit: '50', version: 'latest' })}`, { signal: AbortSignal.timeout(TIMEOUT), headers: { accept: 'application/json' } })
      if (!res.ok) continue
      body = await res.json()
    } catch {
      continue
    }
    for (const entry of body?.servers ?? []) {
      const s = entry.server ?? entry
      const meta = entry._meta?.['io.modelcontextprotocol.registry/official']
      if (meta?.status && meta.status !== 'active') continue
      for (const r of s.remotes ?? []) {
        if (!['streamable-http', 'sse'].includes(r.type) || !r.url || /[{}]/.test(r.url)) continue
        if ((r.headers ?? []).some(h => h.isRequired) || (r.variables && Object.keys(r.variables).length)) continue
        if (!seen.has(r.url)) seen.set(r.url, { name: s.name, title: s.title || s.name.split('/').pop(), about: String(s.description ?? '').slice(0, 300), url: r.url, type: r.type, website: s.websiteUrl ?? null })
      }
    }
    if (seen.size >= limit) break
  }
  return [...seen.values()].slice(0, limit)
}

// A person's sign-in to one server, as the MCP SDK reads and keeps it.
class SignIn {
  constructor(account, redirect, onState) {
    this.account = account
    this.redirect = redirect
    this.onState = onState
    this.authUrl = null
    this.cache = undefined
  }
  async all() {
    this.cache ??= (await this.account.get()) ?? {}
    return this.cache
  }
  async save(part) {
    this.cache = { ...(await this.all()), ...part }
    await this.account.set(this.cache, this.cache.title ?? this.cache.url)
  }
  get redirectUrl() {
    return this.redirect
  }
  get clientMetadata() {
    return { client_name: 'Instant apps', redirect_uris: [this.redirect], grant_types: ['authorization_code', 'refresh_token'], response_types: ['code'], token_endpoint_auth_method: 'none' }
  }
  async state() {
    const state = randomBytes(16).toString('hex')
    this.onState?.(state)
    return state
  }
  async clientInformation() {
    return (await this.all()).client
  }
  async saveClientInformation(client) {
    await this.save({ client })
  }
  async tokens() {
    return (await this.all()).tokens
  }
  async saveTokens(tokens) {
    await this.save({ tokens, signedIn: true })
  }
  redirectToAuthorization(url) {
    this.authUrl = String(url)
  }
  async saveCodeVerifier(verifier) {
    await this.save({ verifier })
  }
  async codeVerifier() {
    return (await this.all()).verifier
  }
  async invalidateCredentials(scope) {
    if (scope === 'all') this.cache = {}
    else if (scope === 'tokens') this.cache = { ...(await this.all()), tokens: undefined }
    else if (scope === 'client') this.cache = { ...(await this.all()), client: undefined }
    else if (scope === 'verifier') this.cache = { ...(await this.all()), verifier: undefined }
    await this.account.set(this.cache, this.cache.title ?? this.cache.url)
  }
}

const clients = new Map() // `${self}|${url}` → { client, at }
setInterval(() => {
  for (const [key, c] of clients) if (Date.now() - c.at > 300_000) (clients.delete(key), c.client.close().catch(() => {}))
}, 60_000).unref?.()

// Every client closed, as a chat that stops does.
export function closeClients() {
  for (const [key, c] of clients) (clients.delete(key), c.client.close().catch(() => {}))
}

const within = (promise, ms, what) => Promise.race([promise, new Promise((_, no) => setTimeout(() => no(new Error(`${what} took too long`)), ms).unref?.())])

// A client of one server as this person: their sign-in when the server
// wants one. A server that wants a sign-in not made yet throws NeedsConnect
// with the address to sign in at.
export async function clientFor(ctx, server) {
  const url = typeof server === 'string' ? server : server.url
  await publicUrl(url)
  const key = `${ctx?.self ?? ''}|${url}`
  const had = clients.get(key)
  if (had) {
    had.at = Date.now()
    return had.client
  }
  const account = ctx?.account?.(`mcp:${url}`)
  const signIn = account && ctx.redirect ? new SignIn(account, ctx.redirect, state => ctx.pending?.(state, { url, title: server.title ?? null })) : null
  if (signIn && server.title) signIn.cache = { ...((await account.get()) ?? {}), url, title: server.title }
  const order = server.type === 'sse' ? [SSEClientTransport, StreamableHTTPClientTransport] : [StreamableHTTPClientTransport, SSEClientTransport]
  let last
  for (const Transport of order) {
    const client = new Client({ name: 'instant-apps', version: '0.1.0' })
    const transport = new Transport(new URL(url), { ...(signIn ? { authProvider: signIn } : {}), fetch: guarded })
    try {
      await within(client.connect(transport), TIMEOUT, 'connecting')
      clients.set(key, { client, at: Date.now() })
      if (signIn) await signIn.save({ url, title: server.title ?? (await signIn.all()).title ?? null, connected: Date.now() }).catch(() => {})
      return client
    } catch (error) {
      await client.close().catch(() => {})
      if (signIn?.authUrl) throw new NeedsConnect(`mcp:${url}`, server.title ?? new URL(url).hostname, { url: signIn.authUrl })
      if (error instanceof UnauthorizedError || /\b401\b|unauthori[sz]ed/i.test(error.message)) throw new Error(`${server.title ?? url} wants a sign-in this chat could not start`)
      last = error
    }
  }
  throw last ?? new Error('could not connect')
}

// The code the server's sign-in page sent back, as tokens kept for the person.
export async function finishSignIn(ctx, url, code) {
  const account = ctx.account(`mcp:${url}`)
  const signIn = new SignIn(account, ctx.redirect)
  const done = await auth(signIn, { serverUrl: url, authorizationCode: code, fetchFn: guarded })
  clients.delete(`${ctx.self}|${url}`)
  return done
}

// A server's tools, short enough to show the model and an app.
export async function toolsOf(client) {
  const listed = await within(client.listTools(), TIMEOUT, 'listing its tools')
  return (listed.tools ?? []).slice(0, 60).map(t => ({
    name: t.name,
    about: String(t.description ?? t.title ?? '').slice(0, 400),
    takes: t.inputSchema ?? { type: 'object' },
    reads: t.annotations?.readOnlyHint === true || (t.annotations?.readOnlyHint == null && /^(get|list|search|find|read|fetch|query|lookup|describe|show|current|forecast)/i.test(t.name)),
  }))
}

const MAX = 60_000
// What a tool answered, as data: its structured answer, else its text
// (read as JSON when it is), never more than an app needs.
export function resultOf(out) {
  const text = (out?.content ?? []).filter(c => c.type === 'text').map(c => c.text).join('\n')
  if (out?.isError) throw new Error(text.slice(0, 500) || 'the tool failed')
  if (out?.structuredContent) return out.structuredContent
  let value = text
  try {
    value = JSON.parse(text)
  } catch {}
  const json = JSON.stringify(value)
  return json.length > MAX ? (typeof value === 'string' ? `${value.slice(0, MAX)}… (cut)` : { cut: true, text: json.slice(0, MAX) }) : value
}

export async function callOn(ctx, server, tool, args) {
  const client = await clientFor(ctx, server)
  return resultOf(await within(client.callTool({ name: tool, arguments: args ?? {} }), 30_000, tool))
}

const SERVER = z.object({ name: z.string().max(200), title: z.string().max(200).nullish(), url: z.string().max(2000), about: z.string().max(400).nullish(), type: z.string().max(40).nullish(), website: z.string().max(2000).nullish() }).passthrough()

export const connector = {
  name: 'connector',
  title: 'Connected service',
  view: null,
  given: true,
  domains: [],
  description:
    'What a service outside the chat answered through its MCP server, found in the public MCP registry and connected when the request ' +
    'needed it: data.server ({name, title, url, about}), data.tool and data.arguments (what was called last for the request; data.steps, the calls before it that it needed, such as a city\'s coordinates), data.result (its ' +
    'answer: structured data or text, whatever the service gives), data.tools (every tool of that server: name, about, takes (its JSON ' +
    'schema) and reads (true when it only reads)), and data.note. An app on it shows data.result as the request wants it, drawn from the ' +
    "result's real fields, and its controls call connector_call with {server: data.server.url, tool, arguments} for more: another page, " +
    'another search, or an action the person taps (a tool whose reads is false), never on its own.',
  input: {
    request: z.string().max(2000).optional(),
    service: z.string().max(200).optional(),
    server: SERVER,
    tool: z.string().max(200).nullish(),
    arguments: z.record(z.any()).nullish(),
    result: z.any(),
    tools: z.array(z.any()).max(60).optional(),
    note: z.string().max(600).nullish(),
    steps: z.array(z.any()).max(3).optional(),
  },
  example: { service: 'weather', server: { name: 'example/weather', title: 'Weather', url: 'https://example.com/mcp' }, tool: 'forecast', arguments: { city: 'Indio' }, result: { days: [] }, tools: [] },
  async run(args) {
    return { widget: 'connector', ...args }
  },
  summary(d) {
    const answer = d.result == null ? 'nothing yet' : JSON.stringify(d.result).slice(0, 800)
    return `${d.server.title ?? d.server.name} (an MCP server${d.server.about ? `: ${d.server.about}` : ''}) answered ${d.tool ?? 'nothing'}${d.arguments ? ` ${JSON.stringify(d.arguments).slice(0, 200)}` : ''}: ${answer}${d.note ? ` ${d.note}` : ''}`
  },
}

export const connectorCall = {
  name: 'connector_call',
  title: 'Call a connected service',
  view: null,
  service: 'mcp',
  domains: [],
  description:
    'Calls one tool of the MCP server an app on "connector" was made with, as the person who taps (their own sign-in): {server: ' +
    'data.server.url, tool: one of data.tools by name, arguments: what its takes asks for}. Answers {result}, as data.result is.',
  input: {
    server: z.string().max(2000).describe('data.server.url'),
    tool: z.string().max(200),
    arguments: z.record(z.any()).optional(),
  },
  example: { server: 'https://example.com/mcp', tool: 'forecast', arguments: { city: 'Indio' } },
  async run({ server, tool, arguments: args }, ctx) {
    return { widget: 'connector', server: { url: server }, tool, arguments: args ?? {}, result: await callOn(ctx, { url: server }, tool, args) }
  },
  summary: d => `${d.tool} answered: ${JSON.stringify(d.result).slice(0, 800)}`,
}
