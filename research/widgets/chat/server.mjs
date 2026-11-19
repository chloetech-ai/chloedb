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
import { createServer } from 'node:http'
import { createRequire } from 'node:module'
import os from 'node:os'
import path from 'node:path'
import { fileURLToPath } from 'node:url'

import { described } from '../server/guide.mjs'
import { TOOLS, callTool, domainsOf, render } from '../server/index.mjs'
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
  const onScreen = screen =>
    (Array.isArray(screen) ? screen : []).slice(0, 4).map(w => ({
      name: String(w.name ?? '').slice(0, 60),
      shows: String(w.shows ?? '').slice(0, 900),
      did: (Array.isArray(w.did) ? w.did : []).slice(-3).map(d => String(d).slice(0, 300)),
      told: String(w.told ?? '').slice(0, 300),
    }))

  async function turn({ text, history = [], lastApp, screen }, send) {
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
    if (plan.mode === 'chat' || !plan.tool || !KNOWN.has(plan.tool)) {
      send({ type: 'reply', text: plan.reply || 'I could not find a way to answer that with an app.' })
      return
    }
    let app = plan.mode === 'use' && plan.use ? await apps.get(plan.use) : null
    let args = plan.args ?? {}
    // An app of what was found: the web searched first, and what it found
    // is the tool's arguments.
    if (plan.tool === 'findings') {
      send({ type: 'status', text: 'Searching the web' })
      try {
        // A follow-up starts from what the app shown just before found.
        const found = last?.tool === 'findings' ? (lastApp.args ?? last.args) : null
        args = await model.research({ request: text, query: String(args.query || text), history: history.slice(-6), found })
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
      const { result, data } = await run(app.tools.includes(plan.tool) ? plan.tool : app.tool, args)
      // The app just shown, filled again for a follow-up, is an update.
      const how = lastApp?.id === app.id ? 'updated' : app.saved ? 'recalled' : 'again'
      send({ type: 'app', app: view(app), html: htmlOf(app), args, result, how, why: plan.why })
      shown.set(app.id, { args, result, data, tool: plan.tool })
      await apps.used(app.id, text, args, how)
      send({ type: 'reply', text: await model.reply({ request: text, summary: data.summary }).catch(() => data.summary) })
      return
    }
    // A new app: its data first, then the model writes it, and the answer
    // is written meanwhile.
    const make = plan.make ?? { name: 'New app', about: text, look: '', tools: [plan.tool] }
    const tools = [...new Set([plan.tool, ...(make.tools ?? [])])].filter(t => KNOWN.has(t))
    send({ type: 'making', name: make.name, kind: plan.kind, why: plan.why })
    const { result, data } = await run(plan.tool, args)
    const replied = model.reply({ request: text, summary: data.summary }).catch(() => data.summary)
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
    send({ type: 'app', app: view(app), html: htmlOf(app), args, result, how: 'made', why: plan.why })
    send({ type: 'reply', text: await replied })
  }

  async function change(appId, text, send) {
    const app = await apps.get(appId)
    let was = shown.get(appId)
    if (!was) {
      const { result, data } = await run(app.tool, app.args)
      was = { args: app.args, result, data, tool: app.tool }
    }
    send({ type: 'status', text: `Changing ${app.name}` })
    const source = await model.edit({
      app,
      source: app.source,
      change: text,
      tool: was.tool,
      args: was.args,
      data: was.data,
      tools: described(TOOLS.filter(t => app.tools.includes(t.name))),
    })
    const next = await apps.change(appId, source, text)
    send({ type: 'app', app: view(next), html: htmlOf(next), args: was.args, result: was.result, how: 'changed' })
  }

  // Opens an app from the rail with what it last showed.
  async function open(appId, send) {
    const app = await apps.get(appId)
    const { result, data } = await run(app.tool, app.args)
    shown.set(app.id, { args: app.args, result, data, tool: app.tool })
    send({ type: 'app', app: view(app), html: htmlOf(app), args: app.args, result, how: 'opened' })
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
        return stream(res, send => turn(input, send))
      }
      // A tool an app calls from its view: only the tools it was made with.
      if (req.method === 'POST' && url.pathname === '/api/call') {
        const { app: appId, name, arguments: args } = await body(req)
        const app = await apps.get(appId)
        if (!app || !app.tools.includes(name)) return json(res, { content: [{ type: 'text', text: `${name} is not one of this app's tools` }], isError: true })
        const { result, data } = await run(name, args)
        // A call of its first tool (another range, another search) is what
        // it shows now; an action's answer (a booking) is not.
        if (!result.isError && name === app.tool) shown.set(app.id, { args, result, data, tool: name })
        // A tool that acts (a booking, the cart): the page has the chat say so.
        const acted = !result.isError && TOOLS.find(t => t.name === name)?.acts === true
        return json(res, acted ? { ...result, acted: true } : result)
      }
      // What the chat says after something was done in an app.
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
        if (parts[3] === 'change') return stream(res, send => (limited(req) ? Promise.reject(new Error(tooMany)) : change(appId, String(input.text ?? ''), send)))
        if (parts[3] === 'open') return stream(res, send => open(appId, send))
        if (parts[3] === 'save') return json(res, { app: view(await apps.save(appId, true, input.name)) })
        if (parts[3] === 'unsave') return json(res, { app: view(await apps.save(appId, false)) })
        if (parts[3] === 'undo') {
          const app = await apps.undo(appId)
          return json(res, { app: view(app), html: htmlOf(app) })
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
