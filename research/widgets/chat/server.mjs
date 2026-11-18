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
//   node chat/server.mjs [--port 8800] [--data DIR]
import { mkdirSync, readFileSync } from 'node:fs'
import { createServer } from 'node:http'
import { createRequire } from 'node:module'
import os from 'node:os'
import path from 'node:path'
import { fileURLToPath } from 'node:url'

import { TOOLS, callTool, domainsOf, render } from '../server/index.mjs'
import { claude } from './model.mjs'
import { Apps } from './store.mjs'

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

// Each tool as the model reads it: its name, what it does and its arguments.
function described(tools) {
  return tools.map(t => ({
    name: t.name,
    description: t.description,
    args: Object.fromEntries(Object.entries(t.input).map(([k, v]) => [k, `${v.isOptional() ? 'optional ' : ''}${typeName(v)}: ${v.description ?? ''}`])),
  }))
}

// By the schema's own type name: the tools' zod is the server package's.
function typeName(schema) {
  const inner = schema._def.typeName === 'ZodOptional' ? schema._def.innerType : schema
  if (inner._def.typeName === 'ZodEnum') return `one of ${inner._def.values.join(', ')}`
  if (inner._def.typeName === 'ZodNumber') return 'number'
  return 'text'
}

const KNOWN = new Set(TOOLS.map(t => t.name))

export async function start({ port = 8800, data, model = claude(), apps, log = console.error } = {}) {
  apps ??= await Apps.open({ dir: data ?? path.join(os.tmpdir(), 'instant-apps'), log })
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

  async function turn({ text, history = [], lastApp }, send) {
    send({ type: 'status', text: 'Looking through your apps' })
    const candidates = await apps.recall(text, 8)
    send({ type: 'recall', candidates: candidates.map(c => ({ id: c.id, name: c.name, score: c.score })) })
    const last = lastApp ? await apps.get(lastApp.id) : null
    const plan = await model.route({
      request: text,
      history: history.slice(-6),
      candidates,
      last: last && { ...last, args: lastApp.args ?? last.args },
      tools: described(TOOLS),
    })
    send({ type: 'route', mode: plan.mode, kind: plan.kind, why: plan.why })
    if (plan.mode === 'chat' || !plan.tool || !KNOWN.has(plan.tool)) {
      send({ type: 'reply', text: plan.reply || 'I could not find a way to answer that with an app.' })
      return
    }
    let app = plan.mode === 'use' && plan.use ? await apps.get(plan.use) : null
    const args = plan.args ?? {}
    if (app) {
      send({ type: 'status', text: `Opening ${app.name}` })
      const { result, data } = await run(app.tools.includes(plan.tool) ? plan.tool : app.tool, args)
      const how = app.saved ? 'recalled' : 'again'
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
      if (req.method === 'GET' && url.pathname === '/bridge.js') return res.writeHead(200, { 'content-type': 'text/javascript' }).end(bridgeJs)
      if (req.method === 'GET' && url.pathname === '/api/apps') return json(res, { apps: (await apps.saved()).map(view) })
      if (req.method === 'POST' && url.pathname === '/api/turn') {
        const input = await body(req)
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
        return json(res, result)
      }
      if (req.method === 'POST' && parts[0] === 'api' && parts[1] === 'apps' && parts[2]) {
        const appId = decodeURIComponent(parts[2])
        const input = await body(req)
        if (!(await apps.get(appId))) return json(res, { error: 'no such app' }, 404)
        if (parts[3] === 'change') return stream(res, send => change(appId, String(input.text ?? ''), send))
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
  await new Promise(resolve => server.listen(port, '127.0.0.1', resolve))
  return {
    url: `http://127.0.0.1:${server.address().port}/`,
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
  start({ port: Number(flag('--port', 8800)), data })
    .then(({ url }) => console.error(`Instant apps on ${url} (apps kept in ${data})`))
    .catch(error => {
      console.error(error.message)
      process.exit(1)
    })
}
