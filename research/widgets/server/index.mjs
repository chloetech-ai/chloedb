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

import { tableBook, tableSearch } from './widgets/booking.mjs'
import flight from './widgets/flight.mjs'
import { recipeGet, recipesBrowse } from './widgets/recipes.mjs'
import { cartAdd, cartShow, shopSearch } from './widgets/shop.mjs'
import stock from './widgets/stock.mjs'

// The tools with a view of their own, which any MCP Apps host draws.
export const WIDGETS = [stock, flight, shopSearch, cartAdd, cartShow]
// Every tool an app can get its data from or act through: the widgets', and
// sources that only made apps draw (booking a table, recipes).
export const TOOLS = [...WIDGETS, tableSearch, tableBook, recipesBrowse, recipeGet]

export const VIEWS = {
  stock: { title: 'Stock widget', domains: [] },
  flight: { title: 'Flight tracker widget', domains: [] },
  shop: { title: 'Shopping widget', domains: ['https://cdn.dummyjson.com'] },
}

export const INSTRUCTIONS = [
  'These tools open small live apps (widgets) in the conversation: stock_quote for a stock, ETF, index or',
  'crypto price; flight_track for where a flight is; shop_search, cart_add and cart_view for finding and',
  'buying products. When a request is about one of these, call the tool first, even if the person did not',
  'ask for a widget, then answer in one or two sentences: the widget already shows the details.',
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
  return page(readFileSync(here(`./views/${view}.html`), 'utf8'), VIEWS[view]?.domains ?? [])
}

// A made app is a fragment (its <style>, markup and <script type="module">)
// that this page holds, after the kit it uses.
const FRAME = `<!doctype html>
<html>
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
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
    .replace('<!-- kit:js -->', () => `<script type="module">${readFileSync(here('./views/kit.js'), 'utf8')}</script>`)
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

  // The sources without a view of their own answer as plain tools.
  for (const tool of TOOLS.filter(t => !t.view)) {
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

  for (const [view, { title, domains }] of Object.entries(VIEWS)) {
    const uri = `ui://widgets/${view}.html`
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
