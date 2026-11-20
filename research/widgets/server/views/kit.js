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
  // A made app's page says so: its host keeps what it keeps (a view of its own keeps nothing).
  const deliver = drawing(draw, () => connected, Boolean(document.querySelector('meta[name="kit-keeps"]')))
  app.ontoolinput = params => draw.input?.(params.arguments ?? {})
  app.ontoolresult = result => {
    if (result.isError) {
      draw.error?.(result.content?.find(c => c.type === 'text')?.text ?? 'The tool failed.')
      return
    }
    deliver(result.structuredContent ?? {})
  }
  app.onhostcontextchanged = theme
  if (window.matchMedia?.('(prefers-color-scheme: dark)').matches) document.documentElement.dataset.theme = 'dark'
  connected = app.connect().then(() => app)
  await connected
  // The frame can't print itself (TOM-5): the page prints this app.
  globalThis.print = () => print(app)
  theme(app.getHostContext())
  report()
  return app
}

// Hands each tool result to the app's draw, in order, with what the app
// kept in this browser as data.kept, so an app starts from it and puts the
// request's values over it. The first result waits for the reads an app
// makes as it opens (await Kit.kept(app, key) right after connect): an
// app written to copy kept values in then would otherwise copy them over
// the values just asked for.
//
// A result marked again (the app opened again, changed or undone, with
// what it was last asked) carries old values: what was kept since is
// newer, so a value the app kept under the same name is not handed in.
const tick = () => new Promise(resolve => setTimeout(resolve, 0))
function names(value, depth = 0, out = new Set()) {
  if (!value || typeof value !== 'object' || Array.isArray(value) || depth > 2) return out
  for (const [k, v] of Object.entries(value)) {
    out.add(k.toLowerCase())
    names(v, depth + 1, out)
  }
  return out
}
function replayed(data, kept) {
  if (!data?.again || !data.values || typeof data.values !== 'object') return data
  const have = names(kept)
  if (!have.size) return data
  const values = Object.fromEntries(Object.entries(data.values).filter(([k]) => !have.has(k.toLowerCase())))
  return { ...data, values }
}
let sharedKept = false
let keptWhere = ''
let role = null
// Draws the last data again from what is kept: after a keep the link can't
// make, so a helper's screen doesn't show an item gone that is still there.
let redraw = null
let restored = 0
let redrawn = []
function drawing(draw, connection, keeps = true) {
  let queue = Promise.resolve()
  let first = true
  let last
  const run = data =>
    (queue = queue.then(async () => {
      const app = await connection()
      if (first) {
        first = false
        await tick()
        for (let i = 0; reads.size && i < 20; i++) await Promise.race([Promise.allSettled([...reads]), new Promise(resolve => setTimeout(resolve, 3000))])
        await tick()
      }
      let kept = {}
      let gone = false
      try {
        if (keeps) kept = (await globalThis.Kit.call(app, 'app_kept', { all: true }))?.values ?? {}
      } catch (error) {
        // An app whose host keeps nothing gets none; a stopped link says so.
        if (/stopped/i.test(error?.message ?? '')) {
          banner(error.message)
          gone = true
        }
      }
      // A stopped link shows the notice and none of the app's own content (PRIYA-2, TOM-7).
      if (gone) return
      // Shared by a link: what it keeps is not in this browser alone, and its words say so.
      // Not private: it is kept with the app on the site, not in this browser,
      // and its words say so (DEV-1).
      const where = data?.private ? '' : data?.shared ? 'for everyone with its link' : 'with this app, not only in this browser'
      if (where !== keptWhere) {
        keptWhere = where
        sharedKept = Boolean(data?.shared)
        if (document.body) textForm(document.body)
      }
      role = data?.role ?? role
      if (role === 'viewer') banner('Shared with you to look at: what you change here is not kept.')
      const start = performance.now()
      try {
        draw({ ...replayed(data, kept), kept })
      } finally {
        slow(performance.now() - start)
      }
    }).catch(error => broke(error?.message ?? error)))
  redraw = () => last !== undefined && run(last)
  return data => {
    last = data
    return run(data)
  }
}

// Calls one of the server's tools from the view and returns its data.
async function call(app, name, args) {
  const result = await app.callServerTool({ name, arguments: args })
  if (result.isError) throw new Error(result.content?.find(c => c.type === 'text')?.text ?? `${name} failed`)
  return result.structuredContent
}

// A pattern the person types can run without end on some texts ((a+)+$ on a
// run of a's) and freeze the page with the app (DEV-1). In the chat, the page
// runs it apart, in a worker it stops after a second, and the text never
// leaves the browser; elsewhere a pattern that repeats a repeat is refused
// and the text is cut to 20,000 characters. It never throws: it answers
// { matches: [{ index, text, groups, named }], error, timedOut }.
const RUNAWAY = /\((?:[^()\\]|\\.)*(?:[+*]|\{\d*,\d*\})(?:[^()\\]|\\.)*\)\s*(?:[+*]|\{\d*,)/
function matchAll(pattern, flags, text) {
  try {
    const re = new RegExp(pattern, flags.includes('g') ? flags : `${flags}g`)
    const matches = []
    let m
    while ((m = re.exec(text)) && matches.length < 1000) {
      matches.push({ index: m.index, text: m[0], groups: m.slice(1), named: m.groups ?? null })
      if (m[0] === '') re.lastIndex++
    }
    return { matches }
  } catch (error) {
    return { matches: [], error: error.message }
  }
}
async function regex(app, pattern, flags = '', text = '') {
  const asked = { pattern: String(pattern ?? ''), flags: String(flags ?? ''), text: String(text ?? '').slice(0, 200_000) }
  if (app?.getHostVersion?.()?.name === 'instant-apps') {
    try {
      return await call(app, 'app_regex', asked)
    } catch (error) {
      return { matches: [], error: error.message }
    }
  }
  if (RUNAWAY.test(asked.pattern)) return { matches: [], error: 'This pattern repeats a repeat, such as (a+)+, which can run without end on some texts and freeze the page: write it as a+.' }
  return matchAll(asked.pattern, asked.flags, asked.text.slice(0, 20_000))
}

// An app's own code can still run for seconds (a loop on what was typed)
// and freeze the page with it. The kit times the app's event handlers and its
// draws, and one that ran over two seconds is told to the chat, which names
// the app and opens it next time without what it kept (DEV-1).
const SLOW = 2000
function slow(ms) {
  if (ms < SLOW || !connected) return
  connected.then(app => app.getHostVersion?.()?.name === 'instant-apps' && app.callServerTool({ name: 'app_froze', arguments: { ms: Math.round(ms) } })).catch(() => {})
}
// The page draws a shared app again when someone else keeps something in it,
// but not while the person is typing in it: the kit says when they type (MAYA-3).
let typedSaid = 0
addEventListener('input', () => {
  if (!connected || Date.now() - typedSaid < 1500) return
  typedSaid = Date.now()
  connected.then(app => app.getHostVersion?.()?.name === 'instant-apps' && app.callServerTool({ name: 'app_busy', arguments: {} })).catch(() => {})
}, true)
const timedListeners = new WeakMap()
const addListener = EventTarget.prototype.addEventListener
const removeListener = EventTarget.prototype.removeEventListener
EventTarget.prototype.addEventListener = function (type, listener, options) {
  if (typeof listener !== 'function') return addListener.call(this, type, listener, options)
  let timed = timedListeners.get(listener)
  if (!timed) {
    timed = function (...args) {
      const start = performance.now()
      try {
        return listener.apply(this, args)
      } finally {
        slow(performance.now() - start)
      }
    }
    timedListeners.set(listener, timed)
  }
  return addListener.call(this, type, timed, options)
}
EventTarget.prototype.removeEventListener = function (type, listener, options) {
  return removeListener.call(this, type, timedListeners.get(listener) ?? listener, options)
}

// Puts a message in the conversation as the person's, for "Ask Claude".
function ask(app, text) {
  return app.sendMessage({ role: 'user', content: [{ type: 'text', text }] })
}

// Tells the conversation what the person now sees or picked in the view
// ("Selected: Pasta Fresca at 7:30"), so a later question can refer to it.
// What the page shows goes with it (below).
let toldWords = ''
let seenWords = ''
function tell(app, text) {
  toldWords = String(text ?? '').slice(0, 600)
  return context(app)
}
function context(app) {
  const parts = [toldWords, seenWords && `On screen now:\n${seenWords}`].filter(Boolean)
  return app.updateModelContext({ content: [{ type: 'text', text: parts.join('\n') }] }).catch(() => {})
}

// What the person sees, told to the conversation a moment after the page
// settles: its text and the values in its fields. The chat answers from
// it, so the words under an app repeat the app's own numbers rather than
// working them out again, and a question about the app is about this.
function fieldsText() {
  const out = []
  for (const f of document.querySelectorAll('input, select, textarea')) {
    if (/^(hidden|password|submit|button|file)$/.test(f.type) || f.closest('[hidden]')) continue
    const label = (f.labels?.[0]?.innerText || f.getAttribute('aria-label') || f.placeholder || f.name || f.id || '').replace(/\s+/g, ' ').trim().slice(0, 60)
    const value = f.type === 'checkbox' || f.type === 'radio' ? (f.checked ? 'on' : 'off') : f.tagName === 'SELECT' ? f.selectedOptions?.[0]?.text ?? f.value : f.value
    if (label || value) out.push(`${label || 'field'}: ${String(value ?? '').slice(0, 120)}`)
    if (out.length >= 30) break
  }
  return out.join('; ')
}
// The photos it shows, by what they show: innerText has none of them.
function photosText() {
  const out = []
  for (const img of document.images) {
    if (!img.complete || !img.naturalWidth || img.closest('[hidden]') || img.width < 24) continue
    out.push((img.alt || img.title || 'a photo').replace(/\s+/g, ' ').trim().slice(0, 80))
    if (out.length >= 20) break
  }
  return out.length ? `Photos shown (${out.length}): ${out.join('; ')}` : ''
}
function onScreen() {
  const text = (document.body?.innerText ?? '').replace(/[ \t]+/g, ' ').replace(/\s*\n\s*/g, '\n').trim().slice(0, 5000)
  const fields = fieldsText()
  const photos = photosText()
  return `${text}${fields ? `\nFields: ${fields}` : ''}${photos ? `\n${photos}` : ''}`
}
// Told once the page is still for 700 ms, and at least every 2 s while it
// keeps changing (an animation's readout), so the chat is never left waiting.
let seenTimer = null
let seenSince = 0
function report() {
  clearTimeout(seenTimer)
  seenSince ||= Date.now()
  seenTimer = setTimeout(async () => {
    seenSince = 0
    if (!connected) return
    const now = onScreen()
    if (!now || now === seenWords) return
    seenWords = now
    context(await connected)
  }, Math.max(0, Math.min(700, 2000 - (Date.now() - seenSince))))
}
addEventListener('input', report, true)
addEventListener('change', report, true)
addEventListener('load', e => e.target instanceof HTMLImageElement && report(), true)

// What an app keeps from one opening to the next (a chore chart's stars,
// prices typed into a tracker, the rate the person set): stored with the
// app for this browser (the chat's id for it), in the same place as the
// app. The frame itself keeps nothing (no storage, cookies or IndexedDB).
// A value is anything JSON holds.
// A keep the link can't make (a view link, or a helper link taking something
// out) is said on the app itself, so nobody thinks it was kept.
async function keep(app, key, value) {
  try {
    await globalThis.Kit.call(app, 'app_keep', { key: String(key), value: value ?? null })
    // Kept: a "Not kept" said earlier no longer stands (LENA-6).
    const box = document.getElementById('kit-banner')
    if (box && /^Not kept/.test(box.textContent)) box.remove()
    return true
  } catch (error) {
    banner(error.message)
    // A viewer may try things on screen; a helper sees what is kept again
    // (at most every few seconds, so an app that keeps as it draws can't loop).
    // Every refusal is drawn at once, but an app that keeps as it draws can't loop: five in ten seconds at most.
    const now = Date.now()
    redrawn = redrawn.filter(t => now - t < 10000)
    if (role !== 'viewer' && redraw && redrawn.length < 5) {
      redrawn.push(now)
      restored = now
      redraw()
    }
    return false
  }
}
function banner(text) {
  if (!document.body) return
  let box = document.getElementById('kit-banner')
  if (!box) {
    box = document.createElement('div')
    box.id = 'kit-banner'
    box.setAttribute('role', 'status')
    box.style.cssText = 'position:sticky;top:0;z-index:9;margin:-14px -16px 10px;padding:8px 16px;background:var(--bg-2);border-bottom:1px solid var(--line);color:var(--ink);font-size:13px'
    document.body.prepend(box)
  }
  box.textContent = text
}

// Prints the app, all of it (the page does it: a frame can't print).
function print(app) {
  return globalThis.Kit.call(app, 'app_print', {}).catch(() => {})
}

// A file for the person to take elsewhere (a CSV for a spreadsheet, a file
// for their accounts): the page downloads it, as the frame can't.
function download(app, name, text, type = 'text/csv') {
  return globalThis.Kit.call(app, 'app_download', { name: String(name), text: String(text ?? ''), type: String(type) })
}
const reads = new Set() // Kit.kept calls not answered yet
async function kept(app, key) {
  const read = globalThis.Kit.call(app, 'app_kept', { key: String(key) }).then(data => data?.value ?? null)
  reads.add(read)
  read.then(() => reads.delete(read), () => reads.delete(read))
  return read
}

// Opens a web address through the host (a view's iframe may not open one
// itself); anything but http(s) is left alone.
function link(app, url) {
  return /^https?:\/\//i.test(String(url ?? '')) ? app.openLink({ url: String(url) }).catch(() => {}) : Promise.resolve()
}

// Money with its cents, always: a total of $1,726.45 is not $1,726.
const money = (value, currency = 'USD') => new Intl.NumberFormat(undefined, { style: 'currency', currency, minimumFractionDigits: 2, maximumFractionDigits: 2 }).format(value)
// A number as people type it: "2,50" and "2.50" are 2.5, "1.234,56" and
// "1,234.56" are 1234.56, "1,500" is 1500, "7,5 %" is 7.5, "€ 3,50" is 3.5.
// NaN when it is not a number. Read every number field with it.
function parse(text) {
  if (typeof text === 'number') return text
  let t = String(text ?? '').replace(/[\s\u00a0\u202f']/g, '').replace(/^[^\d.,+-]+|[^\d.,]+$/g, '')
  if (!/^[+-]?[\d.,]*\d[\d.,]*$/.test(t)) return NaN
  const comma = t.lastIndexOf(','), dot = t.lastIndexOf('.')
  if (comma >= 0 && dot >= 0) {
    const decimal = comma > dot ? ',' : '.'
    t = t.split(decimal === ',' ? '.' : ',').join('').replace(decimal, '.')
  } else if (comma >= 0) {
    const parts = t.split(',')
    // One comma and three digits after it is a thousands separator (1,500); else a decimal one (2,50).
    t = parts.length > 2 || (parts.length === 2 && parts[1].length === 3 && parts[0].replace(/^[+-]/, '').length > 0 && parts[0] !== '0') ? parts.join('') : parts.join('.')
  } else if ((t.match(/\./g) ?? []).length > 1) {
    t = t.split('.').join('')
  }
  const n = Number(t)
  return Number.isFinite(n) ? n : NaN
}
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
    if (keptWhere && /kept in this browser/i.test(node.data)) node.data = node.data.replace(/(k)ept in this browser/gi, (_, k) => `${k}ept ${keptWhere}`)
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
  report()
}).observe(document.documentElement, { childList: true, subtree: true, characterData: true })
if (document.body) textForm(document.body)

globalThis.Kit = { connect, drawing, replayed, call, ask, tell, keep, kept, download, print, link, money, number, parse, regex, ago, esc, math: globalThis.KitMath, plot: globalThis.KitPlot }
