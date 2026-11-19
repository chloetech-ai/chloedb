// The model's jobs in the chat, each one `claude -p` call: route a request
// (open a saved app, make a new one, or just answer), look things up on the
// web for an app that shows what was found, make an app, change one as
// asked, and write the short answer under it.
//
// WIDGETS_CLAUDE names the claude binary, WIDGETS_FAST_MODEL the model that
// routes and answers, WIDGETS_MAKE_MODEL the one that writes apps. On a
// site others use, WIDGETS_DAILY_USD and WIDGETS_TOTAL_USD cap what Claude
// may spend (kept in WIDGETS_SPEND_FILE), and WIDGETS_MODEL_SLOTS how many
// calls run at once.
import { spawn } from 'node:child_process'
import { readFileSync } from 'node:fs'
import { mkdtemp, rm, writeFile } from 'node:fs/promises'
import os from 'node:os'
import path from 'node:path'

import { KINDS, checkFragment, fragment, kitGuide, toolList } from '../server/guide.mjs'
import { clip } from '../server/widgets/findings.mjs'

export { fragment }

// What a nested session must not inherit from the one that runs this: its
// session, project and memory.
const PRIVATE = /^(CLAUDECODE|CLAUDE_CODE_(SESSION_ID|REMOTE_SESSION_ID|CHILD_SESSION|PROJECTS_SESSION|ENTRYPOINT|ADDITIONAL_DIRECTORIES_CLAUDE_MD|SYNC_SESSION_REFS|SESSION_ATTENDED)|CLAUDE_MEMORY_STORES|CLAUDE_COWORK_MEMORY_PATH_OVERRIDE|CLAUDE_ADDITIONAL_DIRECTORIES)$/

// What Claude has cost here, as claude -p reports each call's cost: by UTC
// day and in all, kept in a file when one is named. A call past a cap is
// refused before it starts.
export class Budget {
  constructor({ daily = 0, total = 0, file = null } = {}) {
    this.daily = daily
    this.total = total
    this.file = file
    this.spent = { total: 0, day: '', today: 0 }
    if (file) {
      try {
        this.spent = { ...this.spent, ...JSON.parse(readFileSync(file, 'utf8')) }
      } catch {
        // None spent yet.
      }
    }
  }

  roll() {
    const day = new Date().toISOString().slice(0, 10)
    if (this.spent.day !== day) this.spent = { ...this.spent, day, today: 0 }
  }

  check() {
    this.roll()
    if (this.daily && this.spent.today >= this.daily) throw new Error('Today’s budget for Claude on this site is spent. It starts again at 4 PM Pacific time.')
    if (this.total && this.spent.total >= this.total) throw new Error('This site’s budget for Claude is spent.')
  }

  async add(usd) {
    if (!(usd > 0)) return
    this.roll()
    this.spent = { ...this.spent, total: this.spent.total + usd, today: this.spent.today + usd }
    if (this.file) await writeFile(this.file, JSON.stringify(this.spent)).catch(() => {})
  }

  status() {
    this.roll()
    return { ...this.spent, daily: this.daily, cap: this.total }
  }
}

// At most `n` calls at once; the others wait their turn.
function slots(n) {
  let busy = 0
  const waiting = []
  return async work => {
    if (busy >= n) await new Promise(resolve => waiting.push(resolve))
    busy++
    try {
      return await work()
    } finally {
      busy--
      waiting.shift()?.()
    }
  }
}

const num = name => Number(process.env[name] || 0)

export function claude({
  bin = process.env.WIDGETS_CLAUDE || 'claude',
  fast = process.env.WIDGETS_FAST_MODEL || 'claude-sonnet-5-5',
  strong = process.env.WIDGETS_MAKE_MODEL || 'claude-opus-5-5',
  budget = new Budget({ daily: num('WIDGETS_DAILY_USD'), total: num('WIDGETS_TOTAL_USD'), file: process.env.WIDGETS_SPEND_FILE || null }),
  at = num('WIDGETS_MODEL_SLOTS') || 4,
} = {}) {
  const turn = slots(at)
  const run = input => {
    budget.check()
    return turn(() => call(input))
  }
  // `tools` are Claude Code's own tools the call may use: none, or the web
  // search alone for research (never WebFetch, which would reach the
  // services beside this one); `mcp` the MCP servers it may use, such as
  // the reader of public pages.
  async function call({ system, prompt, model, schema, tools = [], mcp = null, turns = 0, timeoutMs = 240_000 }) {
    budget.check()
    const dir = await mkdtemp(path.join(os.tmpdir(), 'widgets-claude-'))
    try {
      const systemFile = path.join(dir, 'system.txt')
      await writeFile(systemFile, system, { mode: 0o600 })
      const args = ['-p', '--output-format', 'json', '--model', model, '--system-prompt-file', systemFile, '--tools', tools.join(','),
        '--setting-sources', '', '--strict-mcp-config', '--disable-slash-commands', '--no-session-persistence']
      const allowed = [...tools, ...Object.keys(mcp ?? {}).map(name => `mcp__${name}`)]
      if (mcp) args.push('--mcp-config', JSON.stringify({ mcpServers: mcp }))
      if (allowed.length) args.push('--allowedTools', allowed.join(','))
      if (turns) args.push('--max-turns', String(turns))
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
      await budget.add(Number(answer.total_cost_usd))
      if (answer.is_error) throw new Error(`the model failed: ${String(answer.result).slice(0, 300)}`)
      return schema ? answer.structured_output ?? JSON.parse(answer.result) : String(answer.result ?? '')
    } finally {
      await rm(dir, { recursive: true, force: true }).catch(() => {})
    }
  }

  // An app as the model writes it, checked before anyone sees it: a
  // script that does not run goes back once with what is wrong.
  async function written(prompt) {
    const system = await makeSystem()
    const ask = text => run({ system, prompt: text, model: strong, timeoutMs: 480_000 })
    let answer = await ask(prompt)
    let problem
    let source
    try {
      source = fragment(answer)
      problem = await checkFragment(source)
    } catch (error) {
      problem = error.message
    }
    if (!problem) return source
    answer = await ask(`${prompt}\n\nYou wrote this:\n\`\`\`html\n${source ?? answer}\n\`\`\`\n\nIt does not run: ${problem}\nAnswer with the whole fixed fragment, changing nothing else.`)
    source = fragment(answer)
    const still = await checkFragment(source)
    if (still) throw new Error(`the app Claude wrote does not run: ${still}`)
    return source
  }

  return {
    budget,
    async route(input) {
      return run({ system: ROUTE, prompt: routePrompt(input), model: fast, schema: ROUTE_SCHEMA, timeoutMs: 90_000 })
    },
    // The web searched for a request, and what was found as the findings
    // tool takes it.
    async research(input) {
      const found = await run({ system: RESEARCH, prompt: researchPrompt(input), model: fast, schema: RESEARCH_SCHEMA, tools: ['WebSearch'], mcp: READER, turns: 14, timeoutMs: 240_000 })
      return clip({ ...found, query: found.query || input.query })
    },
    async make(input) {
      return written(makePrompt(input))
    },
    async edit(input) {
      return written(editPrompt(input))
    },
    async reply(input) {
      return (await run({ system: REPLY, prompt: replyPrompt(input), model: fast, timeoutMs: 90_000 })).trim()
    },
    async note(input) {
      return (await run({ system: NOTE, prompt: notePrompt(input), model: fast, timeoutMs: 90_000 })).trim()
    },
  }
}

const ROUTE = `You run a chat in which answers can come with small apps: interactive views shown in the conversation. An app is for ${KINDS}. The person keeps the apps they like in a rail beside the chat, and a saved app opens again by itself when a later request needs it.

For each request you decide one of three things:
- "use": a saved app (or the app shown just before) does the job the request asks for, even with other inputs (another stock, another city, another dish). Give its id in "use" and, in "tool" and "args", the tool call that fills it for this request. Prefer a saved app whenever it does the same job: that is the point of saving it. Sharing a tool is not enough: comparing products side by side is another job than a shop's list, and a recipe to cook tonight another than browsing ideas.
- "make": no saved app fits, and the request is better served by an app than by text: live data, things to compare, something to book or buy, things to browse. Describe the new app in "make" and give its first tool call in "tool" and "args".
- "chat": the request needs no app (an explanation, an opinion, small talk, a question about what an app in the conversation already shows, something no tool has data for and no app could work out). Write the answer in "reply", in at most three sentences.

An app can also work things out itself, with no data from anywhere: a calculator of any kind (calculus, loans, units, statistics), a function plotted with its derivative or integral and animated, a simulation (physics, growth, a queue), a visual explanation, a timer, a small game, a drawing or music toy. When a request asks for one ("make me", "build", "a calculator that", "show me how ... changes", "animate"), it is "make" (or "use" of a saved app that does the same job) with tool "inputs" and args {"request": the request in a few words, "values": the values it names, such as {"f": "x^2 sin(x)"} or {"amount": 600000, "rate": 6, "years": 30}; {} when none}, and kind "tool". Its "look" says what it draws and every control it has (a field for the formula, sliders, play and pause, presets), as an expert would build it, in up to 6 sentences.

A follow-up that changes what the app shown just before should show (another time, fewer people, not spicy, a cheaper one) is "use" of that app with the new arguments: it is filled again in the conversation.

A request to find, research or look up things on the web (the best of something to buy or visit, reviews, what people say, news, papers, how to do something, a question the tools have no data for) uses the tool "findings" with args {"query": the request in a few words}: the chat searches the web for it first and fills the app with what it found, shaped to the request (for a pick, the options compared and which one for which need, with the sources behind each claim). The saved app "Search results" shows it; make another app on findings only when the request needs another way to show it (a reading list, a trip plan). A follow-up to a "Search results" app in the conversation that changes what it should show (a need of the person's, a budget, a narrower question, "which should I get if I fly a lot") is "use" of that app with tool "findings", which starts from what it shows; a question it already answers is "chat".

For "make": name it in 2 or 3 words that fit later requests of the same kind ("Stock chart", "Product comparison", "Table booking", "Dinner ideas", "Derivative explorer"; never one stock, product, city or formula). "about" is one sentence on what it does, also general. "look" says what it shows and which controls it has, in 2 to 4 sentences that fit the tools' data. "tools" lists every tool it may call: its first one and those its buttons need (booking needs the search and the booking tool; a shop needs the cart tools; recipes the recipe tool).

The tools and their arguments are listed in the request; use only those, with arguments that fit them. "kind" is info, action, discover or tool. "why" is a few words the chat shows beside the app: for "use", why that saved app fits ("your stock chart, for another stock"); for "make", what is new ("no saved app compares products yet").`

const ROUTE_SCHEMA = {
  type: 'object',
  properties: {
    mode: { type: 'string', enum: ['use', 'make', 'chat'] },
    kind: { type: 'string', enum: ['info', 'action', 'discover', 'tool'] },
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


function screenText(screen = []) {
  if (!screen.length) return 'No app is in the conversation yet.'
  return screen
    .map(w => `- "${w.name}" shows: ${w.shows || '(nothing yet)'}${w.did.length ? ` Done in it: ${w.did.join(' ')}` : ''}${w.told ? ` It says the person now sees: ${w.told}` : ''}`)
    .join('\n')
}

function routePrompt({ request, history, candidates, last, screen, tools }) {
  const saved = candidates.length
    ? candidates.map(a => `- id ${a.id}: "${a.name}" (${a.kind}), ${a.about} Its first tool: ${a.tool}. Asked for before with: ${a.examples || 'nothing yet'}.`).join('\n')
    : '(none yet)'
  const called = JSON.stringify(last?.args ?? {})
  const shown = last ? `The app shown just before: id ${last.id}, "${last.name}", tools ${last.tools.join(', ')}, called with ${called.length > 600 ? `${called.slice(0, 600)}... (cut)` : called}.` : 'No app was shown just before.'
  const said = history.length ? history.map(h => `${h.role}: ${h.text}`).join('\n') : '(nothing yet)'
  return `Today is ${new Date().toDateString()}.

The conversation so far:
${said}

${shown}

The apps in the conversation, newest first, and what they show now (use this to answer questions about them, such as "which one is quickest?" or "is the first one any good?", in "chat" mode):
${screenText(screen)}

The person's saved apps, best matches first (ranked by words and meaning):
${saved}

The tools apps get their data from and act through:
${toolList(tools)}

The request: ${request}`
}

let MAKE
async function makeSystem() {
  MAKE ??= await kitGuide()
  return MAKE
}

// A photo handed in with the data is shown as one, not as its bytes.
const photo = (k, v) => (typeof v === 'string' && v.startsWith('data:image/') ? `${v.slice(0, v.indexOf(',') + 1)}... (a photo, ready for <img src>)` : v)

function sample(data) {
  const text = JSON.stringify(data, photo, 1)
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

const RESEARCH = `You look things up on the web for a chat, and what you find is shown to the person as an app that answers their request: not a list of pages, but what they want to know, with the pages behind every claim. For "the best headphones to buy", that is the headphones themselves: which one to get, which one for which need, each one's facts (price, battery, how well it cancels noise, comfort...), pros and cons, and what each source says about it.

Search the web (at most 4 searches), then read 3 or 4 of the most useful pages, from different sites, with read_page (a test with measurements, a review, a shop or the maker's page for prices and specs): search results alone are too thin for facts. Use 4 to 8 sources in all: what you read, and the search results that add something. Search or read again for what is missing that matters (current prices, specs, dates, the newest models). Use only what the pages and results say; never invent a page, an address, a date, a price or a quote. Prefer sources that tested or know first hand, recent ones, and several that disagree over many that agree.

Answer with:
- "goal": pick (choosing among things to buy, visit or use, or browsing things to see: breeds, places, dishes), answer (a question), overview (a subject), how-to (steps) or news (what happened).
- "answer": the bottom line in 1 to 3 sentences, as a friend who read everything would say it ("Get the X unless you fly a lot: then Y"), with "cites", the numbers of the documents behind it.
- "items": for a pick, the 3 to 6 options themselves, best first, each with "name", "verdict" (one sentence: who it is for and the catch), "facts", "pros" and "cons" (a few words each) and "says" ({doc, text}: what each source says about it, quoted or close); for an answer or an overview, its 3 to 6 points ("name" a short heading, "about" the point, "says" its evidence); for a how-to, its steps in order; for news, its events with "when".
- "facts": the same 3 to 6 names for every item, short values that compare at a glance (449, "30 h", "best tested", "250 g"), in the same units for every item; a price as a number in dollars when there is one. Leave a fact out of an item when the sources do not give it; never write that it is unknown.
- "needs": for a pick, the best item for 2 to 4 kinds of need ("most people", "long flights", "on a budget"), by the item's exact name, with "why" and "cites".
- "criteria": the facts' names exactly as in the items, most important for this request first.
- "documents": the 4 to 8 sources used, numbered from 1 in this order (doc and cites use those numbers): title, url, source (the site or publisher), date only when the results give one, kind (review, test, article, news, forum, shop...), a summary of what each says for this request, and quotes passed exactly.
- "image" for an item: the address of a photo of the item itself from the "Pictures on the page" of a page you read (a product photo whose description names it, the place, the animal, the dish), never a logo, a person or a picture of something else, and never an address you did not see. "url" for an item: its own page (the maker's, the place's, its Wikipedia article) when you saw it; its photo is taken from there when you give no image. Every item of a pick gets a photo this way when one can be found, and when the person asks to see pictures, give every item one.
- "query": the request in a few words, and "notes": what the sources disagree on or leave out, and what to check, in a sentence or two.

When what was found before is given and the request follows it up (a need of the person's, a budget, a narrower question), start from it: re-rank, add or drop items and update the answer and needs for them, and search only for what is missing.`

// The reader of public web pages (../server/reader.mjs), for the facts a
// search result's snippet leaves out.
const READER = { reader: { command: process.execPath, args: [new URL('../server/reader.mjs', import.meta.url).pathname] } }

const FACT = { type: ['string', 'number', 'boolean', 'null'] }
const CITES = { type: 'array', items: { type: 'integer' } }
const RESEARCH_SCHEMA = {
  type: 'object',
  properties: {
    query: { type: 'string' },
    goal: { type: 'string', enum: ['pick', 'answer', 'overview', 'how-to', 'news'] },
    answer: { type: 'object', properties: { text: { type: 'string' }, cites: CITES }, required: ['text'] },
    needs: {
      type: 'array',
      items: { type: 'object', properties: { need: { type: 'string' }, pick: { type: 'string' }, why: { type: 'string' }, cites: CITES }, required: ['need', 'pick'] },
    },
    criteria: { type: 'array', items: { type: 'string' } },
    items: {
      type: 'array',
      items: {
        type: 'object',
        properties: {
          name: { type: 'string' },
          about: { type: 'string' },
          verdict: { type: 'string' },
          facts: { type: 'object', additionalProperties: FACT },
          pros: { type: 'array', items: { type: 'string' } },
          cons: { type: 'array', items: { type: 'string' } },
          says: { type: 'array', items: { type: 'object', properties: { doc: { type: 'integer' }, text: { type: 'string' } }, required: ['doc', 'text'] } },
          when: { type: 'string' },
          url: { type: 'string' },
          image: { type: 'string' },
        },
        required: ['name'],
      },
    },
    documents: {
      type: 'array',
      items: {
        type: 'object',
        properties: {
          title: { type: 'string' },
          url: { type: 'string' },
          source: { type: 'string' },
          date: { type: 'string' },
          kind: { type: 'string' },
          summary: { type: 'string' },
          quotes: { type: 'array', items: { type: 'string' } },
        },
        required: ['title', 'url', 'summary'],
      },
    },
    notes: { type: 'string' },
  },
  required: ['query', 'goal', 'answer', 'items', 'documents'],
}

// What was found before, as the research step reads it: what the app shows,
// without the numbering and the fields the view adds.
function before(found) {
  if (!found?.documents?.length) return ''
  const { query, goal, answer, needs, criteria, items, documents, notes } = found
  const text = JSON.stringify({ query, goal, answer, needs, criteria, items, documents, notes })
  return `What was found before in this conversation:\n${text.length > 24000 ? `${text.slice(0, 24000)}... (cut)` : text}\n\n`
}

function researchPrompt({ request, query, history = [], found }) {
  const said = history.length ? `The conversation so far:\n${history.map(h => `${h.role}: ${h.text}`).join('\n')}\n\n` : ''
  return `Today is ${new Date().toDateString()}.\n\n${said}${before(found)}Look up on the web: ${query}\n\n(The person asked: ${request})`
}

const REPLY = `You answer in a chat. An app with the details is shown above your answer, so answer the request in one or two short sentences, at most 45 words: the answer itself, not every number or option, and nothing about the app or its buttons (the person can see them). If the data is made up for a demo, say so in a few words. The data you get is what the app's first call returned; when the request asks about several things (three stocks, two cities), the app loads the others itself, so answer from what you have and never say that the rest is missing.`

const NOTE = `You are the assistant in a chat in which answers come with small apps. The person just did something in one of the apps (booked, added to a cart). Write what you say about it in the chat: one or two short sentences, at most 35 words, in plain words. The person did it themselves, so do not say you did it: confirm what happened in their terms ("Your table at ... is booked", "The whisk is in your cart"; if it is a demo and nothing real happened, say so in a few words), then offer one useful next step you could help with. No markdown, no lists.`

function notePrompt({ app, summary, history }) {
  const said = history.length ? history.map(h => `${h.role}: ${h.text}`).join('\n') : '(nothing yet)'
  return `The conversation so far:\n${said}\n\nIn the app "${app.name}" (${app.about}), the person just did this. The app's answer: ${summary}`
}

function replyPrompt({ request, summary }) {
  return `The request: ${request}\n\nThe data the app shows, in short: ${summary}`
}
