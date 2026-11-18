// The model's four jobs in the chat, each one `claude -p` call: route a
// request (open a saved app, make a new one, or just answer), make an app,
// change one as asked, and write the short answer under it.
//
// WIDGETS_CLAUDE names the claude binary, WIDGETS_FAST_MODEL the model that
// routes and answers, WIDGETS_MAKE_MODEL the one that writes apps.
import { spawn } from 'node:child_process'
import { mkdtemp, readFile, rm, writeFile } from 'node:fs/promises'
import os from 'node:os'
import path from 'node:path'

const VIEWS = new URL('../server/views/', import.meta.url)

// What a nested session must not inherit from the one that runs this: its
// session, project and memory.
const PRIVATE = /^(CLAUDECODE|CLAUDE_CODE_(SESSION_ID|REMOTE_SESSION_ID|CHILD_SESSION|PROJECTS_SESSION|ENTRYPOINT|ADDITIONAL_DIRECTORIES_CLAUDE_MD|SYNC_SESSION_REFS|SESSION_ATTENDED)|CLAUDE_MEMORY_STORES|CLAUDE_COWORK_MEMORY_PATH_OVERRIDE|CLAUDE_ADDITIONAL_DIRECTORIES)$/

export function claude({
  bin = process.env.WIDGETS_CLAUDE || 'claude',
  fast = process.env.WIDGETS_FAST_MODEL || 'claude-sonnet-5-5',
  strong = process.env.WIDGETS_MAKE_MODEL || 'claude-opus-5-5',
} = {}) {
  async function run({ system, prompt, model, schema, timeoutMs = 240_000 }) {
    const dir = await mkdtemp(path.join(os.tmpdir(), 'widgets-claude-'))
    try {
      const systemFile = path.join(dir, 'system.txt')
      await writeFile(systemFile, system, { mode: 0o600 })
      const args = ['-p', '--output-format', 'json', '--model', model, '--system-prompt-file', systemFile, '--tools', '',
        '--setting-sources', '', '--strict-mcp-config', '--disable-slash-commands', '--no-session-persistence']
      if (schema) args.push('--json-schema', JSON.stringify(schema))
      const env = Object.fromEntries(Object.entries(process.env).filter(([k]) => !PRIVATE.test(k)))
      const out = await new Promise((resolve, reject) => {
        const child = spawn(bin, args, { env, cwd: dir, stdio: ['pipe', 'pipe', 'pipe'] })
        let stdout = ''
        let stderr = ''
        const timer = setTimeout(() => { child.kill(); reject(new Error(`the model took over ${timeoutMs / 1000}s`)) }, timeoutMs)
        child.stdout.on('data', d => (stdout += d))
        child.stderr.on('data', d => (stderr += d))
        child.on('error', e => { clearTimeout(timer); reject(new Error(`could not run ${bin}: ${e.message}`)) })
        child.on('close', code => {
          clearTimeout(timer)
          if (code !== 0 && !stdout) reject(new Error(`${bin} exited ${code}: ${stderr.slice(-400)}`))
          else resolve(stdout)
        })
        child.stdin.end(prompt)
      })
      const answer = JSON.parse(out)
      if (answer.is_error) throw new Error(`the model failed: ${String(answer.result).slice(0, 300)}`)
      return schema ? answer.structured_output ?? JSON.parse(answer.result) : String(answer.result ?? '')
    } finally {
      await rm(dir, { recursive: true, force: true }).catch(() => {})
    }
  }

  return {
    async route(input) {
      return run({ system: ROUTE, prompt: routePrompt(input), model: fast, schema: ROUTE_SCHEMA, timeoutMs: 90_000 })
    },
    async make(input) {
      return fragment(await run({ system: await makeSystem(), prompt: makePrompt(input), model: strong }))
    },
    async edit(input) {
      return fragment(await run({ system: await makeSystem(), prompt: editPrompt(input), model: strong }))
    },
    async reply(input) {
      return (await run({ system: REPLY, prompt: replyPrompt(input), model: fast, timeoutMs: 90_000 })).trim()
    },
  }
}

// The app's fragment from an answer that may hold it in a fence.
export function fragment(text) {
  const fenced = /```(?:html)?\s*\n([\s\S]*?)\n```/.exec(text)
  const body = (fenced ? fenced[1] : text).trim()
  if (!/<script[^>]*type=["']?module/i.test(body)) throw new Error('the made app has no <script type="module">')
  return body
}

const KINDS = 'info (looking something up: a stock chart, products compared, where a flight is), action (getting something done: booking a table, buying), or discover (browsing for ideas: dishes to cook, products to explore)'

const ROUTE = `You run a chat in which answers can come with small apps: interactive views shown in the conversation. An app is for ${KINDS}. The person keeps the apps they like in a rail beside the chat, and a saved app opens again by itself when a later request needs it.

For each request you decide one of three things:
- "use": a saved app (or the app shown just before) does the job the request asks for, even with other inputs (another stock, another city, another dish). Give its id in "use" and, in "tool" and "args", the tool call that fills it for this request. Prefer a saved app whenever it does the same job: that is the point of saving it. Sharing a tool is not enough: comparing products side by side is another job than a shop's list, and a recipe to cook tonight another than browsing ideas.
- "make": no saved app fits, and the request is better served by an app than by text: live data, things to compare, something to book or buy, things to browse. Describe the new app in "make" and give its first tool call in "tool" and "args".
- "chat": the request needs no app (an explanation, an opinion, small talk, something no tool has data for). Write the answer in "reply", in at most three sentences.

For "make": name it in 2 or 3 words that fit later requests of the same kind ("Stock chart", "Product comparison", "Table booking", "Dinner ideas"; never one stock, product or city). "about" is one sentence on what it does, also general. "look" says what it shows and which controls it has, in 2 to 4 sentences that fit the tools' data. "tools" lists every tool it may call: its first one and those its buttons need (booking needs the search and the booking tool; a shop needs the cart tools; recipes the recipe tool).

The tools and their arguments are listed in the request; use only those, with arguments that fit them. "kind" is info, action or discover. "why" is a few words the chat shows beside the app: for "use", why that saved app fits ("your stock chart, for another stock"); for "make", what is new ("no saved app compares products yet").`

const ROUTE_SCHEMA = {
  type: 'object',
  properties: {
    mode: { type: 'string', enum: ['use', 'make', 'chat'] },
    kind: { type: 'string', enum: ['info', 'action', 'discover'] },
    use: { type: ['string', 'null'] },
    make: {
      type: ['object', 'null'],
      properties: {
        name: { type: 'string' },
        about: { type: 'string' },
        look: { type: 'string' },
        tools: { type: 'array', items: { type: 'string' } },
      },
      required: ['name', 'about', 'look', 'tools'],
    },
    tool: { type: ['string', 'null'] },
    args: { type: 'object' },
    reply: { type: ['string', 'null'] },
    why: { type: 'string' },
  },
  required: ['mode', 'kind', 'tool', 'args', 'why'],
}

function toolList(tools) {
  return tools
    .map(t => `- ${t.name}: ${t.description}\n  arguments: ${JSON.stringify(t.args)}`)
    .join('\n')
}

function routePrompt({ request, history, candidates, last, tools }) {
  const saved = candidates.length
    ? candidates.map(a => `- id ${a.id}: "${a.name}" (${a.kind}), ${a.about} Its first tool: ${a.tool}. Asked for before with: ${a.examples || 'nothing yet'}.`).join('\n')
    : '(none yet)'
  const shown = last ? `The app shown just before: id ${last.id}, "${last.name}", tools ${last.tools.join(', ')}, called with ${JSON.stringify(last.args)}.` : 'No app was shown just before.'
  const said = history.length ? history.map(h => `${h.role}: ${h.text}`).join('\n') : '(nothing yet)'
  return `Today is ${new Date().toDateString()}.

The conversation so far:
${said}

${shown}

The person's saved apps, best matches first (ranked by words and meaning):
${saved}

The tools apps get their data from and act through:
${toolList(tools)}

The request: ${request}`
}

let MAKE
async function makeSystem() {
  if (MAKE) return MAKE
  const example = await readFile(new URL('stock.html', VIEWS), 'utf8')
  MAKE = `You write one small app shown inside a chat conversation, in a sandboxed iframe 320 to 760 pixels wide. An app is for ${KINDS}.

Write only the app's fragment: one <style> element, the markup, and one <script type="module"> with its code. No <html>, <head> or <body>, no other scripts, no stylesheets or fonts from anywhere, no fetch or XMLHttpRequest: the page can load nothing. Pictures may come only from image URLs in the tools' data. Answer with the fragment alone in one \`\`\`html fence.

The page already has a kit, globalThis.Kit:
- Kit.connect(name, draw) connects to the chat and returns a promise of the connection, app. draw(data) gets the data of the app's tool each time there is some: when it opens, and when the chat fills it again for a later request. Set draw.input = args => ... to show what is being loaded, and draw.error = message => ... for a tool that failed. Call connect once, at the end of the script.
- await Kit.call(app, toolName, args) calls one of the app's tools and returns its data, the same shape as draw's: for buttons that fetch more or act (Book, Add to cart, Show the recipe, another range). Show that it is working while it runs, and the error in place if it throws.
- Kit.ask(app, text) puts a message in the chat as the person's, for a follow-up ("Why did it move?", "What goes with this?").
- Kit.money(n, currency = 'USD'), Kit.number(n, digits), Kit.ago(ms), and Kit.esc(text), which escapes text for innerHTML: escape every text that comes from data.

The kit's CSS follows the chat's light or dark theme: variables --bg (the page), --bg-2 (a raised surface), --ink, --ink-2 (secondary text), --ink-3 (faint), --line (borders), --accent, --up and --down (green and red, with --up-bg and --down-bg), --radius, --font, --mono; classes .row (a flex row), .between, .wrap, .muted, .faint, .num, .title, .big (a big number), .chip (with .up and .down), .tabs with .tab buttons (aria-pressed="true" for the chosen one), .btn and .btn.primary, .live (a pulsing dot), .foot (a footer line), .empty (a centered message). body has the font and 14px 16px of padding already.

Make it good to use, not decorated: the answer first, big and clear; the details after; one or two actions that matter. Controls go in one compact row (chips, tabs or a small select), never a stack of form fields: the results take most of the room. About 300 to 600 pixels tall; a list scrolls inside a box rather than growing without end. Draw from the data's real fields (a sample of the first tool's data is in the request), cope with an empty result and with fields missing, and show data.note when there is one: demo data says so. For an action (booking, buying), make it plain what will happen and put the confirmation in place of the button once the tool answers. Never use alert(), confirm() or prompt(). Keep it under 250 lines.

An app made by hand, to show the kit in use (a whole page, where yours is a fragment; its kit:* comments are where the page puts the kit):

${example}`
  return MAKE
}

function sample(data) {
  const text = JSON.stringify(data, null, 1)
  return text.length > 6000 ? `${text.slice(0, 6000)}\n... (cut)` : text
}

function makePrompt({ request, plan, tool, args, data, tools }) {
  return `The request that needs this app: ${request}

The app: "${plan.name}", ${plan.about}
What it shows and does: ${plan.look}

Its first tool is ${tool}, called with ${JSON.stringify(args)}. That call returned:
${sample(data)}

The tools it may call:
${toolList(tools)}`
}

function editPrompt({ app, source, change, tool, args, data, tools }) {
  return `This app, "${app.name}" (${app.about}), is shown with ${tool} called with ${JSON.stringify(args)}, which returned:
${sample(data)}

The tools it may call:
${toolList(tools)}

Its fragment now:
\`\`\`html
${source.startsWith('view:') ? '(a page made by hand; rewrite it as a fragment)' : source}
\`\`\`

Change it as the person asks, keeping everything else as it is, and answer with the whole new fragment: ${change}`
}

const REPLY = `You answer in a chat. An app with the details is shown above your answer, so answer the request in one or two short sentences, at most 45 words: the answer itself, not every number or option, and nothing about the app or its buttons (the person can see them). If the data is made up for a demo, say so in a few words. The data you get is what the app's first call returned; when the request asks about several things (three stocks, two cities), the app loads the others itself, so answer from what you have and never say that the rest is missing.`

function replyPrompt({ request, summary }) {
  return `The request: ${request}\n\nThe data the app shows, in short: ${summary}`
}
