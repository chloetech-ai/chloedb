// What every widget view shares: the connection to the host, its theme, and
// formatting. Views read it as globalThis.Kit once this module has run.
const { App, applyDocumentTheme, applyHostStyleVariables, applyHostFonts } = globalThis.McpApps

function theme(context) {
  if (!context) return
  if (context.theme) {
    applyDocumentTheme(context.theme)
    document.documentElement.dataset.theme = context.theme
  }
  if (context.styles?.variables) applyHostStyleVariables(context.styles.variables)
  if (context.styles?.css?.fonts) applyHostFonts(context.styles.css.fonts)
}

// Connects to the host and hands every tool result to `draw`. Returns the
// App, whose callServerTool and sendMessage the views use for their buttons.
async function connect(name, draw) {
  const app = new App({ name, version: '0.1.0' })
  app.ontoolinput = params => draw.input?.(params.arguments ?? {})
  app.ontoolresult = result => {
    if (result.isError) {
      draw.error?.(result.content?.find(c => c.type === 'text')?.text ?? 'The tool failed.')
      return
    }
    draw(result.structuredContent ?? {})
  }
  app.onhostcontextchanged = theme
  if (window.matchMedia?.('(prefers-color-scheme: dark)').matches) document.documentElement.dataset.theme = 'dark'
  await app.connect()
  theme(app.getHostContext())
  return app
}

// Calls one of the server's tools from the view and returns its data.
async function call(app, name, args) {
  const result = await app.callServerTool({ name, arguments: args })
  if (result.isError) throw new Error(result.content?.find(c => c.type === 'text')?.text ?? `${name} failed`)
  return result.structuredContent
}

// Puts a message in the conversation as the person's, for "Ask Claude".
function ask(app, text) {
  return app.sendMessage({ role: 'user', content: [{ type: 'text', text }] })
}

const money = (value, currency = 'USD') =>
  new Intl.NumberFormat(undefined, { style: 'currency', currency, maximumFractionDigits: value >= 1000 ? 0 : 2 }).format(value)
const number = (value, digits = 0) => new Intl.NumberFormat(undefined, { maximumFractionDigits: digits }).format(value)
const ago = ms => {
  const s = Math.max(0, Math.round((Date.now() - ms) / 1000))
  return s < 60 ? `${s}s ago` : s < 3600 ? `${Math.round(s / 60)} min ago` : new Date(ms).toLocaleString()
}
const esc = text => String(text ?? '').replace(/[&<>"']/g, c => `&#${c.charCodeAt(0)};`)

globalThis.Kit = { connect, call, ask, money, number, ago, esc }
