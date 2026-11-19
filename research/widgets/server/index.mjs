#!/usr/bin/env node
// Widgets: one MCP server whose tools open small apps in a conversation.
//
// Each tool returns a short text for the model and the data as
// structuredContent. Its `_meta.ui.resourceUri` names an HTML view that hosts
// with MCP Apps (Claude on the web and Claude Desktop, VS Code Copilot,
// ChatGPT, Goose) draw inline; Claude Code reads the text, and the mod in
// ../hooks draws the same data as a native card there.
//
//   node index.mjs               stdio, as Claude Code and Claude Desktop start it
//   node index.mjs --http 8787   Streamable HTTP at /mcp, for a remote connector
import { readFileSync } from 'node:fs'
import { createServer } from 'node:http'
import { createRequire } from 'node:module'
import { resolve } from 'node:path'
import { fileURLToPath } from 'node:url'

import { McpServer } from '@modelcontextprotocol/sdk/server/mcp.js'
import { StdioServerTransport } from '@modelcontextprotocol/sdk/server/stdio.js'
import { StreamableHTTPServerTransport } from '@modelcontextprotocol/sdk/server/streamableHttp.js'
import { RESOURCE_MIME_TYPE, registerAppResource, registerAppTool } from '@modelcontextprotocol/ext-apps/server'

import { z } from 'zod'

import { RUNNER, SHELF, registerApps } from './apps.mjs'
import { tableBook, tableSearch } from './widgets/booking.mjs'
import { findings } from './widgets/findings.mjs'
import { inputs } from './widgets/inputs.mjs'
import { exchangeRate } from './widgets/rates.mjs'
import flight from './widgets/flight.mjs'
import { recipeGet, recipesBrowse } from './widgets/recipes.mjs'
import { cartAdd, cartShow, shopSearch } from './widgets/shop.mjs'
import stock from './widgets/stock.mjs'

// The tools with a view of their own, which any MCP Apps host draws.
export const WIDGETS = [stock, flight, shopSearch, cartAdd, cartShow]
// Every tool an app can get its data from or act through: the widgets',
// sources that only made apps draw (booking a table, recipes), and what the
// Claude of the conversation found itself (findings), which it hands to an
// app rather than calls, and the starting values of an app that works
// everything out itself (inputs).
export const TOOLS = [...WIDGETS, tableSearch, tableBook, recipesBrowse, recipeGet, exchangeRate, findings, inputs]

export const VIEWS = {
  stock: { title: 'Stock widget', domains: [] },
  flight: { title: 'Flight tracker widget', domains: [] },
  shop: { title: 'Shopping widget', domains: ['https://cdn.dummyjson.com'] },
}

// The views of instant apps (apps.mjs): the one any app draws in, which may
// show the pictures of every tool's data, and the person's apps.
const ALL = [...new Set([...Object.values(VIEWS).flatMap(v => v.domains), ...TOOLS.flatMap(t => t.domains ?? [])])]
export const APP_VIEWS = {
  app: { uri: RUNNER, title: 'Instant app', domains: ALL },
  apps: { uri: SHELF, title: 'Your apps', domains: [] },
}

export const INSTRUCTIONS = [
  'These tools open small live apps (widgets) in the conversation, and keep the ones the person likes.',
  'When a request would be answered better by an app than by text (live data such as a price or a flight,',
  'things to compare side by side, something to book or buy, things to browse for ideas), call apps_find first,',
  'even if the person did not ask for an app. Open the saved app that does the job with app_open; when none does,',
  'read app_kit, write one and show it with app_make. After an app is shown, answer in one or two sentences: it',
  'already shows the details. A change the person asks for in an app is app_change; app_save keeps it when they',
  'ask, and my_apps shows what they kept. The tools stock_quote, flight_track and shop_search open the apps made',
  'by hand directly.',
  'What you find yourself is data for an app too: after a web search, or pages or files you read, for a request',
  'to find, research, compare or pick things, call show_findings before you answer, shaped to what the person',
  'wants: for the best headphones to buy, the headphones compared, the pick for each need and what each source',
  'says about each, not a list of pages.',
  'An app that works things out itself (a calculator, a function plotted with its derivative, a converter, a simulation, a',
  'game) needs no data: make it with the tool inputs, whose args are the request and the values it names.',
].join(' ')

const here = path => new URL(path, import.meta.url)
const require = createRequire(import.meta.url)

// The MCP Apps client, inlined so a view needs no network to start: its
// closing `export {...}` becomes a global the view's own script reads.
function sdk() {
  const source = readFileSync(require.resolve('@modelcontextprotocol/ext-apps/app-with-deps'), 'utf8')
  const match = source.match(/export\s*\{([^}]*)\}\s*;?\s*$/)
  if (!match) throw new Error('ext-apps app-with-deps.js no longer ends in one export list')
  const names = match[1].split(',').map(part => {
    const [local, exported = local] = part.trim().split(/\s+as\s+/)
    return `${JSON.stringify(exported)}:${local}`
  })
  return `${source.slice(0, match.index)}\nglobalThis.McpApps={${names.join(',')}};`
}

let SDK
export function html(view) {
  return page(readFileSync(here(`./views/${view}.html`), 'utf8'), (VIEWS[view] ?? APP_VIEWS[view])?.domains ?? [])
}

// An app's source as the app view mounts it: a made app's fragment as it
// is, and a view made by hand as one too (its styles and its body, the kit
// being the view's own).
// For a change (prompt), the world's outlines stay a marker: the page
// that draws it puts them in.
export function fragmentOf(source, { prompt = false } = {}) {
  if (!source.startsWith('view:')) return source
  const doc = readFileSync(here(`./views/${source.slice(5)}.html`), 'utf8')
  const head = /<head>([\s\S]*?)<\/head>/i.exec(doc)?.[1] ?? ''
  const styles = head.match(/<style>[\s\S]*?<\/style>/gi) ?? []
  const body = (/<body>([\s\S]*?)<\/body>/i.exec(doc)?.[1] ?? '').replace(/<!-- kit:\w+ -->/g, '')
  const fragment = [...styles, body].join('\n')
  return prompt ? fragment : fragment.replace('/* kit:world */', () => `globalThis.WORLD=${readFileSync(here('./assets/world.json'), 'utf8')};`)
}

// A made app is a fragment (its <style>, markup and <script type="module">)
// that this page holds, after the kit it uses.
const FRAME = `<!doctype html>
<html>
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<meta name="kit-keeps" content="app">
<!-- kit:css -->
<!-- kit:sdk -->
<!-- kit:js -->
</head>
<body>
<!-- app -->
</body>
</html>`

// An app's page from its source: a whole view (a page of its own, as the
// widgets' are) or a made app's fragment. Its scripts may load nothing and
// reach nothing but the host: pictures come only from `domains`, and data
// only through the host's tools (Kit.call).
export function render(source, domains = []) {
  if (source.startsWith('view:')) return html(source.slice(5))
  const doc = /^\s*<!doctype/i.test(source) ? source : FRAME.replace('<!-- app -->', () => source)
  return page(doc, domains)
}

function page(doc, domains) {
  SDK ??= sdk()
  const csp = `default-src 'none'; script-src 'unsafe-inline'; style-src 'unsafe-inline'; img-src data: ${domains.join(' ')}; font-src data:; connect-src 'none'`
  // Replaced with functions: the SDK's text holds `$&` and the like.
  return doc
    .replace(/<head>/i, () => `<head>\n<meta http-equiv="Content-Security-Policy" content="${csp}">`)
    .replace('<!-- kit:css -->', () => `<style>${readFileSync(here('./views/kit.css'), 'utf8')}</style>`)
    .replace('<!-- kit:sdk -->', () => `<script type="module">${SDK.replaceAll('</script', '<\\/script')}</script>`)
    .replace('<!-- kit:js -->', () => `<script type="module">${readFileSync(here('./views/math.js'), 'utf8')}\n${readFileSync(here('./views/kit.js'), 'utf8')}</script>`)
    .replace('/* kit:world */', () => `globalThis.WORLD=${readFileSync(here('./assets/world.json'), 'utf8')};`)
}

export function build() {
  const server = new McpServer({ name: 'widgets', version: '0.1.0' }, { instructions: INSTRUCTIONS })

  for (const widget of WIDGETS) {
    registerAppTool(
      server,
      widget.name,
      {
        title: widget.title,
        description: widget.description,
        inputSchema: widget.input,
        annotations: { readOnlyHint: !widget.name.startsWith('cart_add'), openWorldHint: true },
        _meta: { ui: { resourceUri: `ui://widgets/${widget.view}.html` } },
      },
      async args => {
        const data = await widget.run(args ?? {})
        const summary = widget.summary(data)
        // Claude Code hands the model structuredContent as JSON in place of
        // the text, so the summary rides in it too, first; the mod trims the
        // model's copy back to it.
        return { content: [{ type: 'text', text: summary }], structuredContent: { summary, ...data } }
      },
    )
  }

  // The sources without a view of their own answer as plain tools; what
  // Claude found is only ever handed to an app.
  for (const tool of TOOLS.filter(t => !t.view && !t.given)) {
    server.registerTool(
      tool.name,
      { title: tool.title, description: tool.description, inputSchema: tool.input, annotations: { readOnlyHint: tool.name !== 'table_book', openWorldHint: true } },
      async args => {
        const data = await tool.run(args ?? {})
        const summary = tool.summary(data)
        return { content: [{ type: 'text', text: summary }], structuredContent: { summary, ...data } }
      },
    )
  }

  registerApps(server, { TOOLS, callTool, fragmentOf })

  for (const [view, { title, domains, uri = `ui://widgets/${view}.html` }] of Object.entries({ ...VIEWS, ...APP_VIEWS })) {
    registerAppResource(server, title, uri, { description: `The view the ${view} tools draw in` }, async () => ({
      contents: [
        {
          uri,
          mimeType: RESOURCE_MIME_TYPE,
          text: html(view),
          _meta: { ui: { prefersBorder: true, csp: { resourceDomains: domains, connectDomains: [] } } },
        },
      ],
    }))
  }
  return server
}

// The picture domains an app that uses these tools may show.
export function domainsOf(names) {
  const out = new Set()
  for (const name of names) {
    const tool = TOOLS.find(t => t.name === name)
    for (const d of tool?.domains ?? VIEWS[tool?.view]?.domains ?? []) out.add(d)
  }
  return [...out]
}

// Runs a tool as the MCP server would answer it: the data, with the
// sentence for the model first in it.
export async function callTool(name, args) {
  const tool = TOOLS.find(t => t.name === name)
  if (!tool) throw new Error(`no tool ${name}`)
  // What the model hands an app comes through app_open and app_make's
  // loose args, so its own schema checks it here.
  if (tool.given) {
    const checked = z.object(tool.input).safeParse(args ?? {})
    if (!checked.success) throw new Error(`${name}'s args: ${checked.error.issues.map(i => `${i.path.join('.') || 'args'} ${i.message}`).join('; ')}`)
    args = checked.data
  }
  const data = await tool.run(args ?? {})
  return { summary: tool.summary(data), ...data }
}

async function main() {
  const at = process.argv.indexOf('--http')
  if (at === -1) {
    await build().connect(new StdioServerTransport())
    return
  }
  const port = Number(process.argv[at + 1] ?? 8787)
  createServer(async (req, res) => {
    if (!req.url.startsWith('/mcp')) {
      res.writeHead(404).end()
      return
    }
    // Stateless: a server and a transport per request. The cart is the
    // process's, so it is shared by every connection.
    const server = build()
    const transport = new StreamableHTTPServerTransport({ sessionIdGenerator: undefined, enableJsonResponse: true })
    res.on('close', () => {
      transport.close()
      server.close()
    })
    await server.connect(transport)
    await transport.handleRequest(req, res)
  }).listen(port, '127.0.0.1', () => console.error(`widgets MCP server on http://127.0.0.1:${port}/mcp`))
}

if (process.argv[1] && fileURLToPath(import.meta.url) === resolve(process.argv[1])) {
  main().catch(error => {
    console.error(error)
    process.exit(1)
  })
}
