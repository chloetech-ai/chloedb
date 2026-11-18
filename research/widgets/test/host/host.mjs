// A small MCP Apps host for trying the views in a browser: it starts the
// widgets server over stdio, calls a tool, and draws the tool's view in a
// sandboxed iframe through the SDK's own AppBridge (what Claude, VS Code and
// the SDK's basic-host use), under a made-up conversation turn.
//
//   node test/host/host.mjs [port]     then open http://127.0.0.1:8790/?tool=stock_quote&args={"symbol":"NVDA"}
import { readFileSync } from 'node:fs'
import { createServer } from 'node:http'
import { createRequire } from 'node:module'

const server = new URL('../../server/', import.meta.url)
const require = createRequire(server)
const { Client } = require('@modelcontextprotocol/sdk/client/index.js')
const { StdioClientTransport } = require('@modelcontextprotocol/sdk/client/stdio.js')
const esbuild = require('esbuild')

const port = Number(process.argv[2] ?? 8790)

const client = new Client({ name: 'widgets-test-host', version: '0.0.0' })
await client.connect(
  new StdioClientTransport({ command: process.execPath, args: [new URL('index.mjs', server).pathname], env: process.env }),
)

// The host side of the SDK, bundled for the page.
const bridge = await esbuild.build({
  stdin: {
    contents:
      "export { AppBridge } from '@modelcontextprotocol/ext-apps/app-bridge'\n" +
      "export { PostMessageTransport } from '@modelcontextprotocol/ext-apps'\n",
    resolveDir: server.pathname,
  },
  bundle: true,
  format: 'esm',
  write: false,
  platform: 'browser',
})

const page = readFileSync(new URL('./host.html', import.meta.url), 'utf8')

const body = req =>
  new Promise(resolve => {
    let text = ''
    req.on('data', chunk => (text += chunk))
    req.on('end', () => resolve(text ? JSON.parse(text) : {}))
  })

createServer(async (req, res) => {
  const url = new URL(req.url, `http://${req.headers.host}`)
  try {
    if (url.pathname === '/') {
      res.writeHead(200, { 'content-type': 'text/html' }).end(page)
    } else if (url.pathname === '/bridge.js') {
      res.writeHead(200, { 'content-type': 'text/javascript' }).end(bridge.outputFiles[0].text)
    } else if (url.pathname === '/tools') {
      res.writeHead(200, { 'content-type': 'application/json' }).end(JSON.stringify(await client.listTools()))
    } else if (url.pathname === '/call') {
      const result = await client.callTool(await body(req))
      res.writeHead(200, { 'content-type': 'application/json' }).end(JSON.stringify(result))
    } else if (url.pathname === '/resource') {
      const result = await client.readResource({ uri: url.searchParams.get('uri') })
      res.writeHead(200, { 'content-type': 'application/json' }).end(JSON.stringify(result))
    } else {
      res.writeHead(404).end()
    }
  } catch (error) {
    res.writeHead(500, { 'content-type': 'text/plain' }).end(String(error?.stack ?? error))
  }
}).listen(port, '127.0.0.1', () => console.log(`MCP Apps test host on http://127.0.0.1:${port}/`))
