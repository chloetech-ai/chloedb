// Instant apps in any MCP host: the Claude of the conversation finds a
// saved app for a request (apps_find) and opens it (app_open), or, when none
// fits, reads the kit (app_kit), writes one and shows it (app_make).
// app_change makes a new version, app_save keeps an app in the person's
// apps, and my_apps shows them. Every app draws in one view,
// ui://widgets/app.html, which fetches the app's source (app_source) and
// sends the app's calls through app_call, which runs only the tools the app
// was made with. The apps are rows in ChloeDB (store.mjs), in the folder
// the chat uses too, so an app made in one place opens in the others.
import os from 'node:os'
import path from 'node:path'

import { registerAppTool } from '@modelcontextprotocol/ext-apps/server'
import { z } from 'zod'

import { checkFragment, described, fragment, kitGuide, toolList } from './guide.mjs'
import { Apps } from './store.mjs'
import { INPUT as FOUND } from './widgets/findings.mjs'

export const RUNNER = 'ui://widgets/app.html'
export const SHELF = 'ui://widgets/apps.html'
export const DATA = process.env.WIDGETS_DATA || path.join(os.homedir(), '.local/share/instant-apps')

// The store, opened on first use and kept for the process.
let opened = null
export function store() {
  opened ??= Apps.open({ dir: DATA, log: message => console.error(message) }).catch(error => {
    opened = null
    throw error
  })
  return opened
}

// For the tests: another store in place of the folder's.
export function useStore(apps) {
  opened = Promise.resolve(apps)
}

const KIND = z.enum(['info', 'action', 'discover', 'tool'])
const ARGS = z.record(z.string(), z.any())

// Kit.keep and Kit.kept, which every app may call: what it keeps between
// openings, stored with it for one person (owner: the chat's browser; one
// person alone in Claude Desktop or Claude Code).
export const KEEPING = ['app_keep', 'app_kept']

// What a helper link may not do to what an app keeps (INES-2, LENA-4): take
// out or change what is there. It may add (an entry, a field that was
// empty) and tick (false to true), and a time an app writes as it keeps
// ("updated") may move.
const blank = v => v == null || v === '' || (Array.isArray(v) && !v.length) || (typeof v === 'object' && !Array.isArray(v) && !Object.keys(v).length)
const STAMP = /(updated|modified|saved|changed|edited|synced|^last|_at$|At$)/
export function changesWhatIsThere(before, after, depth = 0, key = '') {
  if (blank(before)) return false
  // A tick may be made, not taken back: a helper un-booking a night is a change (TOM-2).
  if (typeof before === 'boolean') return before ? after !== true : typeof after !== 'boolean' && after != null
  if (blank(after)) return true
  if (depth > 6) return false
  if (Array.isArray(before)) {
    if (!Array.isArray(after)) return true
    const ids = before.filter(e => e && typeof e === 'object' && e.id != null)
    if (ids.length === before.length) {
      return before.some(e => {
        const now = after.find(x => x && typeof x === 'object' && String(x.id) === String(e.id))
        return !now || changesWhatIsThere(e, now, depth + 1)
      })
    }
    // A list without ids: each entry still there, as it was or ticked, in any order.
    if (before.length > 500) return after.length < before.length
    const left = [...after]
    return before.some(e => {
      const at = left.findIndex(x => !changesWhatIsThere(e, x, depth + 1))
      if (at < 0) return true
      left.splice(at, 1)
      return false
    })
  }
  if (typeof before === 'object') {
    if (typeof after !== 'object' || Array.isArray(after)) return true
    return Object.entries(before).some(([k, v]) => changesWhatIsThere(v, after[k], depth + 1, k))
  }
  if (typeof after === 'object' || typeof after === 'boolean') return true
  if (STAMP.test(key) && /^\d{4}-\d\d-\d\d/.test(String(before)) && /^\d{4}-\d\d-\d\d/.test(String(after))) return false
  return String(before) !== String(after)
}

// A name from "view:" on (a filter, a sort, a tab) is how one person looks
// at the app: kept with their own browser even when the app is shared, so
// one crew lead's filter isn't everyone's (MARCUS-3).
export const isView = key => String(key).startsWith('view:')

// A link for one person may show only part of an app (one crew's jobs):
// its lists hold only the entries whose field has that value (MARCUS-4).
const inPart = (entry, only) => entry && typeof entry === 'object' && String(entry[only.field] ?? '').trim().toLowerCase() === String(only.value).trim().toLowerCase()
const parted = (value, only) => Array.isArray(value) && value.some(e => e && typeof e === 'object' && only.field in e)
export function partOf(value, only) {
  if (!only?.field || !parted(value, only)) return value
  return value.filter(e => inPart(e, only))
}
// What a person's link keeps, put back into the whole: entries it added
// say who added them (by) and belong to its part, and the entries it never
// saw (the other crews') stay as they were, where they were (INES-5, MARCUS-4).
export function withPart(before, after, { by, only } = {}) {
  if (!Array.isArray(after)) return after
  const was = new Set((Array.isArray(before) ? before : []).map(e => e?.id).filter(id => id != null))
  // An entry the link can't see (another crew's) is not its to write: an
  // app that saw none of its starting entries and wrote them again doesn't
  // put a second one under the same id (DEV-1).
  const unseen = new Set(only?.field && parted(before, only) ? before.filter(e => e?.id != null && !inPart(e, only)).map(e => e.id) : [])
  const added = after
    .filter(e => !(e && typeof e === 'object' && e.id != null && unseen.has(e.id)))
    .map(e => (e && typeof e === 'object' && !Array.isArray(e) && (e.id == null || !was.has(e.id)) ? { ...e, ...(by && blank(e.by) ? { by } : {}), ...(only?.field && blank(e[only.field]) ? { [only.field]: only.value } : {}) } : e))
  if (!only?.field || !parted(before, only)) return added
  const theirs = new Map(added.filter(e => e?.id != null).map(e => [e.id, e]))
  const out = []
  const placed = new Set()
  for (const e of before) {
    if (!inPart(e, only)) out.push(e)
    else if (e?.id != null && theirs.has(e.id)) {
      out.push(theirs.get(e.id))
      placed.add(e.id)
    }
  }
  for (const e of added) if (e?.id == null || !placed.has(e.id)) out.push(e)
  return out
}
// What a helper link added or filled that wasn't there: its own, which it
// may go on changing and take out again. A note typed one key at a time is
// kept at each key, and the second key changes what the first one filled
// (PRIYA-1, TOM-1). Each is a path: /field, [id] for an entry of a list
// with ids, #n for an entry put at the end of a list without.
const ided = v => Array.isArray(v) && v.length > 0 && v.every(e => e && typeof e === 'object' && e.id != null)
export function madeBy(before, after, path = '', out = [], depth = 0) {
  if (after === undefined || depth > 6) return out
  if (blank(before)) {
    if (!blank(after)) out.push(path)
    return out
  }
  if (Array.isArray(before) && Array.isArray(after)) {
    if (ided(before)) {
      const was = new Map(before.map(e => [String(e.id), e]))
      for (const e of after) {
        if (!e || typeof e !== 'object' || e.id == null) continue
        const old = was.get(String(e.id))
        if (!old) out.push(`${path}[${e.id}]`)
        else madeBy(old, e, `${path}[${e.id}]`, out, depth + 1)
      }
    } else for (let i = before.length; i < after.length; i++) out.push(`${path}#${i}`)
    return out
  }
  if (typeof before === 'object' && !Array.isArray(before) && after && typeof after === 'object' && !Array.isArray(after)) {
    for (const [k, v] of Object.entries(after)) madeBy(before[k], v, `${path}/${k}`, out, depth + 1)
  }
  return out
}
// What is there without what the link made itself: the helper rule is
// checked against this, so what it made it may change.
export function withoutMade(value, made, path = '', depth = 0) {
  if (!made?.size || depth > 6) return value
  if (made.has(path)) return Array.isArray(value) ? [] : value && typeof value === 'object' ? {} : ''
  if (Array.isArray(value)) {
    if (ided(value)) return value.filter(e => !made.has(`${path}[${e.id}]`)).map(e => withoutMade(e, made, `${path}[${e.id}]`, depth + 1))
    return value.filter((e, i) => !made.has(`${path}#${i}`))
  }
  if (value && typeof value === 'object') return Object.fromEntries(Object.entries(value).map(([k, v]) => [k, withoutMade(v, made, `${path}/${k}`, depth + 1)]))
  return value
}

// Fields a person's link never gets (a crew lead doesn't see prices): taken
// out of every entry it reads, and put back from what is there when it
// keeps, so leaving them out takes nothing away (MARCUS-1).
const hidden = (key, hide) => hide.some(h => String(h).toLowerCase() === String(key).toLowerCase())
export function hideIn(value, hide, depth = 0) {
  if (!hide?.length || depth > 6 || !value || typeof value !== 'object') return value
  if (Array.isArray(value)) return value.map(e => hideIn(e, hide, depth + 1))
  return Object.fromEntries(Object.entries(value).filter(([k]) => !hidden(k, hide)).map(([k, v]) => [k, hideIn(v, hide, depth + 1)]))
}
export function withHidden(before, after, hide, depth = 0) {
  if (!hide?.length || depth > 6 || !after || typeof after !== 'object' || !before || typeof before !== 'object') return after
  if (Array.isArray(after)) {
    if (!Array.isArray(before)) return after
    const was = new Map(before.filter(e => e && typeof e === 'object' && e.id != null).map(e => [String(e.id), e]))
    return after.map((e, i) => (e && typeof e === 'object' && e.id != null ? withHidden(was.get(String(e.id)), e, hide, depth + 1) : ided(before) ? e : withHidden(before[i], e, hide, depth + 1)))
  }
  if (Array.isArray(before)) return after
  const out = Object.fromEntries(Object.entries(after).map(([k, v]) => [k, withHidden(before[k], v, hide, depth + 1)]))
  for (const [k, v] of Object.entries(before)) if (hidden(k, hide)) out[k] = v
  return out
}
export async function keeping(apps, appId, name, args = {}, owner = '', self = '') {
  // All of it at once, for the kit to hand draw as data.kept.
  if (name === 'app_kept' && args?.all === true) {
    try {
      // Names from "-" on are the chat's own (a share's links), never the app's.
      const values = Object.fromEntries(Object.entries(await apps.keptAll(appId, owner)).filter(([k]) => !k.startsWith('-') && (!self || self === owner || !isView(k))))
      if (self && self !== owner) Object.assign(values, Object.fromEntries(Object.entries(await apps.keptAll(appId, self)).filter(([k]) => isView(k))))
      return { content: [{ type: 'text', text: 'What this app kept.' }], structuredContent: { values } }
    } catch (error) {
      return { content: [{ type: 'text', text: error.message }], isError: true }
    }
  }
  const key = String(args?.key ?? '').slice(0, 80)
  if (!key || key.startsWith('-')) return { content: [{ type: 'text', text: 'Say what to keep it as (a key that does not start with "-").' }], isError: true }
  if (self && isView(key)) owner = self
  try {
    if (name === 'app_keep') {
      await apps.keep(appId, key, args.value, owner)
      return { content: [{ type: 'text', text: `Kept as ${key}.` }], structuredContent: { key, kept: true } }
    }
    const value = await apps.kept(appId, key, owner)
    return { content: [{ type: 'text', text: value == null ? `Nothing kept as ${key} yet.` : `What was kept as ${key}.` }], structuredContent: { key, value } }
  } catch (error) {
    return { content: [{ type: 'text', text: error.message }], isError: true }
  }
}

// What the view and the mod are told about the app they show.
const meta = (app, how) => ({ id: app.id, name: app.name, about: app.about, kind: app.kind, tool: app.tool, tools: app.tools, version: app.version, saved: app.saved, how })

// A photo handed in with the data is shown as one, not as its bytes.
const photo = (k, v) => (typeof v === 'string' && v.startsWith('data:image/') ? `${v.slice(0, v.indexOf(',') + 1)}... (a photo, ready for <img src>)` : v)

function sample(data) {
  const text = JSON.stringify(data, photo, 1)
  return text.length > 6000 ? `${text.slice(0, 6000)}\n... (cut)` : text
}

const fail = text => ({ content: [{ type: 'text', text }], isError: true })

export function registerApps(server, { TOOLS, callTool, fragmentOf }) {
  const names = TOOLS.map(t => t.name)
  const TOOL = z.enum(names)
  const known = new Set(names)

  // Fills an app with its first tool and answers as every app tool does:
  // a sentence for the model, and the data with the app's details for the
  // view (and for Claude Code's mod).
  async function show(apps, app, args, request, how, note, data = null) {
    try {
      data ??= await callTool(app.tool, args)
    } catch (error) {
      return fail(error.message)
    }
    await apps.used(app.id, request ?? '', args, how)
    const saved = app.saved ? '' : ` It is a draft until it is saved (its Save button, or app_save when the person asks), and only saved apps come back by themselves.`
    const said = `${note} The app "${app.name}" (id ${app.id}) shows this in the conversation, so answer in a sentence or two and leave the details to it.${saved}`
    // The guidance rides in structuredContent too, for the hosts (Claude
    // Code) that hand the model that in place of the text; and the
    // arguments, so a pinned app refreshes by its tool alone.
    return { content: [{ type: 'text', text: `${data.summary}\n\n${said}` }], structuredContent: { ...data, instant: { ...meta(app, how), args: args ?? {}, note: said } } }
  }

  server.registerTool(
    'apps_find',
    {
      title: 'Find an app',
      description:
        'Call this first when a request would be answered better by a small live app in the conversation than by text: live data (a price, where a flight is), ' +
        'things to compare side by side, something to book or buy, or things to browse for ideas (what you found yourself on the web or in documents goes to ' +
        'show_findings). It searches the person’s saved apps by words and meaning ' +
        'and lists those that may fit, best first, with the tool and arguments that fill each. When one does the job the request asks for (even with other ' +
        'inputs: another stock, city or dish), call app_open with its id and the arguments for this request. When none does, call app_kit, then app_make to ' +
        'make one: sharing a tool is not enough (comparing products side by side is another job than a shop’s list, a recipe to cook tonight another than ' +
        'browsing ideas). Not for questions that text answers as well.',
      inputSchema: { request: z.string().min(1).max(500).describe('The person’s request, in their words') },
      annotations: { readOnlyHint: true },
    },
    async ({ request }) => {
      const apps = await store()
      const found = await apps.recall(request, 5)
      const lines = found.map((a, i) => {
        const tool = described(TOOLS.filter(t => t.name === a.tool))[0]
        return `${i + 1}. "${a.name}" (id ${a.id}, ${a.kind}): ${a.about} Opens with ${a.tool} ${JSON.stringify(tool?.args ?? {})}. Asked for before with: ${a.examples || 'nothing yet'}.`
      })
      const text = found.length
        ? `The person’s saved apps that may fit "${request}", best first:\n${lines.join('\n')}\n\nIf one does the job, call app_open with its id and this request’s arguments; if none does, call app_kit, then app_make.`
        : `The person has no saved apps yet. If an app would serve "${request}" better than text, call app_kit, then app_make.`
      return { content: [{ type: 'text', text }], structuredContent: { summary: text, apps: found.map(a => ({ id: a.id, name: a.name, about: a.about, kind: a.kind, tool: a.tool, score: a.score })) } }
    },
  )

  registerAppTool(
    server,
    'app_open',
    {
      title: 'Open an app',
      description:
        'Opens one of the person’s apps in the conversation, filled by its first tool with the arguments for this request: an id from apps_find or my_apps. ' +
        'Leave args out to show what it showed last.',
      inputSchema: {
        id: z.string().min(1).max(64).describe('The app’s id'),
        args: ARGS.optional().describe('Arguments for the app’s first tool (apps_find says which), for this request'),
        request: z.string().max(500).optional().describe('The person’s request, which is added to what finds this app later'),
      },
      annotations: { readOnlyHint: true, openWorldHint: true },
      _meta: { ui: { resourceUri: RUNNER } },
    },
    async ({ id, args, request }) => {
      const apps = await store()
      const app = await apps.get(id)
      if (!app) return fail(`No app ${id}: apps_find lists them.`)
      return show(apps, app, args ?? app.args, request, app.saved ? 'recalled' : 'opened', app.saved ? 'Opened from the person’s apps.' : 'Opened again.')
    },
  )

  // What the model found itself, typed so its host checks it: shown in the
  // person's app for what Claude found that fits the request best (Search
  // results, or one they made on `findings`).
  registerAppTool(
    server,
    'show_findings',
    {
      title: 'Show what you found',
      description:
        'After you searched the web or read pages or files for a request to find, research, compare or pick something, show what you found as an app in ' +
        'the conversation, before you answer. Shape it to what the person wants, not as a list of pages: for "the best headphones to buy", the headphones ' +
        'themselves compared on what matters, which one for which need, and what each source says about each, with the pages behind every claim; for a ' +
        'question, the answer and the points behind it; for a how-to, the steps; for news, what happened when. Search again first if you lack facts that ' +
        'matter (prices, specs, dates). It opens the person’s app for what Claude found; then answer in two or three sentences, since it shows the details.',
      inputSchema: { request: z.string().min(1).max(500).describe('The person’s request, in their words'), ...FOUND },
      annotations: { readOnlyHint: true },
      _meta: { ui: { resourceUri: RUNNER } },
    },
    async ({ request, ...found }) => {
      const apps = await store()
      const fit = (await apps.recall(request, 5)).find(a => a.tool === 'findings')
      const app = await apps.get(fit?.id ?? 'search-results')
      if (!app) return fail('The person has no app for what Claude found: make one with app_kit and app_make on the tool findings.')
      return show(apps, app, found, request, app.saved ? 'recalled' : 'opened', `Shown in "${app.name}".`)
    },
  )

  server.registerTool(
    'app_kit',
    {
      title: 'How to make an app',
      description:
        'Before app_make: how an app is written (a fragment of HTML against a small kit), the tools an app can get its data from and act through, and a ' +
        'sample of what the app’s first tool returns for this request. Read it, write the app, then call app_make.',
      inputSchema: {
        tool: TOOL.describe('The tool that fills the app first'),
        args: ARGS.optional().describe('Its arguments for this request'),
      },
      annotations: { readOnlyHint: true, openWorldHint: true },
    },
    async ({ tool, args }) => {
      const source = TOOLS.find(t => t.name === tool)
      const data = await callTool(tool, args ?? source.example ?? {})
      const guide = await kitGuide({ answer: ' Pass the fragment to app_make as html.' })
      const text = `${guide}\n\nThe tools an app may call (list in app_make's tools the ones it calls; its first tool fills it, the others are for its buttons):\n${toolList(described(TOOLS))}\n\nWhat ${tool} returns for ${JSON.stringify(args ?? source.example ?? {})}, which the app's draw(data) gets:\n${sample(data)}`
      return { content: [{ type: 'text', text }] }
    },
  )

  registerAppTool(
    server,
    'app_make',
    {
      title: 'Make an app',
      description:
        'Makes a new app and shows it in the conversation, filled by its first tool: for a request no saved app does (apps_find), after reading app_kit. ' +
        'It is a draft until the person saves it.',
      inputSchema: {
        name: z.string().min(1).max(40).describe('2 or 3 words that fit later requests of the same kind ("Product comparison", "Table booking"; never one product or city)'),
        about: z.string().min(1).max(300).describe('One sentence on what it does, as general as the name'),
        kind: KIND.describe('info (looking something up), action (getting something done: book, buy), discover (browsing for ideas) or tool (working something out: a calculator, a plot, a simulation)'),
        tool: TOOL.describe('Its first tool, which fills it'),
        args: ARGS.describe('That tool’s arguments for this request'),
        tools: z.array(TOOL).max(8).describe('Every tool it calls: the first and those its buttons need'),
        html: z.string().min(40).max(120000).describe('The app’s fragment, written as app_kit says: a <style>, the markup and one <script type="module">'),
        request: z.string().max(500).describe('The person’s request, which is the first of those that find this app later'),
      },
      annotations: { openWorldHint: true },
      _meta: { ui: { resourceUri: RUNNER } },
    },
    async ({ name, about, kind, tool, args, tools, html, request }) => {
      let source
      try {
        source = fragment(html)
      } catch (error) {
        return fail(`${error.message}: write it as app_kit says.`)
      }
      const broken = await checkFragment(source)
      if (broken) return fail(`The app's script does not run: ${broken}\nFix it and call app_make again.`)
      // Filled first, so arguments its tool refuses make no app.
      let data
      try {
        data = await callTool(tool, args)
      } catch (error) {
        return fail(error.message)
      }
      const apps = await store()
      const allowed = [...new Set([tool, ...tools])].filter(t => known.has(t))
      const app = await apps.put({ name, about, kind, tool, tools: allowed, source, args, examples: request, saved: false }, `made for: ${request}`)
      return show(apps, app, args, request, 'made', `Made the new app "${name}".`, data)
    },
  )

  registerAppTool(
    server,
    'app_change',
    {
      title: 'Change an app',
      description:
        'Changes an app as the person asks ("put the best rated first", "show the price in euros"): give its whole new fragment, written from its source ' +
        '(app_source) as app_kit says. It is a new version, shown in the conversation with what it showed last; app_undo goes back.',
      inputSchema: {
        id: z.string().min(1).max(64).describe('The app’s id'),
        html: z.string().min(40).max(120000).describe('The app’s whole new fragment'),
        change: z.string().min(1).max(300).describe('What was asked, in a few words'),
        tools: z.array(TOOL).max(8).optional().describe('Its tools, when the change needs others'),
      },
      annotations: { openWorldHint: true },
      _meta: { ui: { resourceUri: RUNNER } },
    },
    async ({ id, html, change, tools }) => {
      const apps = await store()
      const app = await apps.get(id)
      if (!app) return fail(`No app ${id}.`)
      let source
      try {
        source = fragment(html)
      } catch (error) {
        return fail(`${error.message}: write it as app_kit says.`)
      }
      const broken = await checkFragment(source)
      if (broken) return fail(`The app's script does not run: ${broken}\nFix it and call app_change again.`)
      let next = await apps.change(id, source, change)
      if (tools) next = await apps.put({ ...next, tools: [...new Set([next.tool, ...tools])].filter(t => known.has(t)) }, null)
      return show(apps, next, next.args, '', 'changed', `Changed "${next.name}" (version ${next.version}): ${change}.`)
    },
  )

  server.registerTool(
    'app_save',
    {
      title: 'Save an app',
      description: 'Keeps an app in the person’s apps, so it opens by itself for requests like those it served; saved false takes it out. When the person asks, or from the app’s Save button.',
      inputSchema: {
        id: z.string().min(1).max(64).describe('The app’s id'),
        saved: z.boolean().optional().describe('false to take it out of the person’s apps'),
        name: z.string().min(1).max(40).optional().describe('Another name for it'),
      },
    },
    async ({ id, saved = true, name }) => {
      const apps = await store()
      if (!(await apps.get(id))) return fail(`No app ${id}.`)
      const app = await apps.save(id, saved, name)
      const text = saved ? `Saved "${app.name}" to the person’s apps: it opens by itself for requests like the ones it served.` : `Took "${app.name}" out of the person’s apps.`
      return { content: [{ type: 'text', text }], structuredContent: { summary: text, instant: meta(app, saved ? 'saved' : 'removed') } }
    },
  )

  server.registerTool(
    'app_undo',
    {
      title: 'Undo a change',
      description: 'Takes an app back to its version before the last change. Open it again (app_open) to show it.',
      inputSchema: { id: z.string().min(1).max(64).describe('The app’s id') },
    },
    async ({ id }) => {
      const apps = await store()
      const was = await apps.get(id)
      if (!was) return fail(`No app ${id}.`)
      const app = await apps.undo(id)
      const text = app.version < was.version ? `"${app.name}" is back to version ${app.version}.` : `"${app.name}" has no earlier version.`
      return { content: [{ type: 'text', text }], structuredContent: { summary: text, instant: meta(app, 'undone') } }
    },
  )

  registerAppTool(
    server,
    'my_apps',
    {
      title: 'Your apps',
      description: 'Shows the person’s saved apps in the conversation, grouped as Look things up, Get things done and Discover, to open one or take it out. When they ask what apps they have.',
      inputSchema: {},
      annotations: { readOnlyHint: true },
      _meta: { ui: { resourceUri: SHELF } },
    },
    async () => {
      const apps = await store()
      const saved = await apps.saved()
      const text = saved.length
        ? `The person’s ${saved.length} saved apps, shown in the conversation: ${saved.map(a => `"${a.name}" (id ${a.id})`).join(', ')}.`
        : 'The person has no saved apps yet.'
      return { content: [{ type: 'text', text }], structuredContent: { widget: 'apps', summary: text, apps: saved.map(a => meta(a, 'saved')).map((a, i) => ({ ...a, uses: saved[i].uses })) } }
    },
  )

  // The two the view calls, and nothing else should: the app's source, and
  // a call of one of its own tools.
  registerAppTool(
    server,
    'app_source',
    {
      title: 'An app’s source',
      description: 'Only an app’s view calls this: the fragment of HTML it draws.',
      inputSchema: { id: z.string().min(1).max(64) },
      annotations: { readOnlyHint: true },
      _meta: { ui: { resourceUri: RUNNER, visibility: ['app'] } },
    },
    async ({ id }) => {
      const apps = await store()
      const app = await apps.get(id)
      if (!app) return fail(`No app ${id}.`)
      return { content: [{ type: 'text', text: `The source of "${app.name}", version ${app.version}.` }], structuredContent: { id, version: app.version, source: fragmentOf(app.source) } }
    },
  )

  registerAppTool(
    server,
    'app_call',
    {
      title: 'An app’s call',
      description: 'Only an app’s view calls this: one of the tools the app was made with, for its buttons.',
      inputSchema: { id: z.string().min(1).max(64), tool: z.union([TOOL, z.enum(KEEPING)]), args: ARGS.optional() },
      annotations: { openWorldHint: true },
      _meta: { ui: { resourceUri: RUNNER, visibility: ['app'] } },
    },
    async ({ id, tool, args }) => {
      const apps = await store()
      const app = await apps.get(id)
      if (!app) return fail(`No app ${id}.`)
      if (KEEPING.includes(tool)) return keeping(apps, id, tool, args)
      if (!app.tools.includes(tool)) return fail(`${tool} is not one of this app’s tools`)
      const data = await callTool(tool, args ?? {})
      // A tool that acts (a booking, the cart): the view tells the model.
      const acted = TOOLS.find(t => t.name === tool)?.acts === true
      return { content: [{ type: 'text', text: data.summary }], structuredContent: acted ? { ...data, acted: true } : data }
    },
  )
}
