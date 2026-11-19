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
let connected = null // the connection, once a view or app made it

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
  connected = app.connect().then(() => app)
  await connected
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

// Tells the conversation what the person now sees or picked in the view
// ("Selected: Pasta Fresca at 7:30"), so a later question can refer to it.
function tell(app, text) {
  return app.updateModelContext({ content: [{ type: 'text', text }] }).catch(() => {})
}

// Opens a web address through the host (a view's iframe may not open one
// itself); anything but http(s) is left alone.
function link(app, url) {
  return /^https?:\/\//i.test(String(url ?? '')) ? app.openLink({ url: String(url) }).catch(() => {}) : Promise.resolve()
}

const money = (value, currency = 'USD') =>
  new Intl.NumberFormat(undefined, { style: 'currency', currency, maximumFractionDigits: value >= 1000 ? 0 : 2 }).format(value)
const number = (value, digits = 0) => new Intl.NumberFormat(undefined, { maximumFractionDigits: digits }).format(value)
const ago = ms => {
  const s = Math.max(0, Math.round((Date.now() - ms) / 1000))
  return s < 60 ? `${s}s ago` : s < 3600 ? `${Math.round(s / 60)} min ago` : new Date(ms).toLocaleString()
}
const esc = text => String(text ?? '').replace(/[&<>"']/g, c => `&#${c.charCodeAt(0)};`)

// Hosts draw views in frames sandboxed with allow-scripts alone, where a
// form is never submitted: the browser drops the submission before its
// submit event. So a submit button's click and Enter in a form's field
// send the submit event here instead, and nothing is sent anywhere (a
// handler reads the fields). It behaves the same in a frame that may
// submit forms.
function submitHere(form, submitter, e) {
  if (!form || form.method === 'dialog') return
  e.preventDefault()
  const init = { bubbles: true, cancelable: true }
  const event = typeof SubmitEvent === 'function' ? new SubmitEvent('submit', { ...init, submitter: submitter ?? null }) : new Event('submit', init)
  form.dispatchEvent(event)
}
document.addEventListener(
  'click',
  e => {
    const b = e.target instanceof Element ? e.target.closest('button, input[type=submit], input[type=image]') : null
    if (b && b.type === 'submit' && !b.disabled && b.form) submitHere(b.form, b, e)
  },
  true,
)
document.addEventListener(
  'keydown',
  e => {
    const f = e.target
    if (e.key !== 'Enter' || e.isComposing || e.defaultPrevented || !(f instanceof HTMLInputElement) || !f.form) return
    if (/^(button|submit|reset|checkbox|radio|file|image|color|range)$/.test(f.type)) return
    submitHere(f.form, null, e)
  },
  true,
)

// An app that hits an error says so in place and offers to be fixed: the
// message goes to the conversation, where Claude changes the app (the
// chat sends it straight to a change of this app).
const seen = new Set()
function broke(message) {
  message = String(message ?? '').replace(/^Uncaught\s+/, '').slice(0, 300)
  if (!message || /ResizeObserver loop/.test(message) || seen.has(message)) return
  seen.add(message)
  let bar = document.getElementById('kit-broke')
  if (!bar) {
    bar = document.createElement('div')
    bar.id = 'kit-broke'
    bar.className = 'kit-broke'
    document.body.append(bar)
  }
  bar.innerHTML = `<span>This app hit an error: ${esc(message)}</span><button class="btn" type="button">Fix it</button>`
  bar.querySelector('button').onclick = async () => {
    bar.querySelector('button').disabled = true
    bar.querySelector('button').textContent = 'Asked'
    const app = await (connected ?? connect('app', () => {}))
    ask(app, `Fix this app: it stopped with "${message}". Keep everything else as it is.`)
  }
}
addEventListener('error', e => broke(e.error?.message ?? e.message))
addEventListener('unhandledrejection', e => broke(e.reason?.message ?? e.reason))

// Apple's devices draw some symbols as emoji pictures (▶ as a blue box, and
// ↗ ⏸ ☀ ✔ ✖ ⚠ alike) unless the text asks for the text form, so every
// text the page shows asks for it: U+FE0E after each such symbol. Emoji
// that are pictures in every form (a headphones emoji) stay as they are;
// the guide asks apps not to use them.
const SYMBOL = /(?!\p{Emoji_Presentation})(\p{Extended_Pictographic})(?![\uFE0E\uFE0F])/gu
const SKIP = /^(SCRIPT|STYLE|TEXTAREA)$/
function textForm(node) {
  if (node.nodeType === Node.TEXT_NODE) {
    const parent = node.parentElement
    if (!parent || SKIP.test(parent.nodeName) || parent.isContentEditable) return
    SYMBOL.lastIndex = 0
    if (!SYMBOL.test(node.data)) return
    SYMBOL.lastIndex = 0
    node.data = node.data.replace(SYMBOL, '$1\uFE0E')
    return
  }
  if (node.nodeType !== Node.ELEMENT_NODE || SKIP.test(node.nodeName)) return
  const walk = document.createTreeWalker(node, NodeFilter.SHOW_TEXT)
  const found = []
  for (let n; (n = walk.nextNode()); ) found.push(n)
  found.forEach(textForm)
}
new MutationObserver(changes => {
  for (const c of changes) {
    if (c.type === 'characterData') textForm(c.target)
    else c.addedNodes.forEach(textForm)
  }
}).observe(document.documentElement, { childList: true, subtree: true, characterData: true })
if (document.body) textForm(document.body)

globalThis.Kit = { connect, call, ask, tell, link, money, number, ago, esc, math: globalThis.KitMath, plot: globalThis.KitPlot }
