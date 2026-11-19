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

import { described, fragment, kitGuide, toolList } from './guide.mjs'
import { Apps } from './store.mjs'

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

const KIND = z.enum(['info', 'action', 'discover'])
const ARGS = z.record(z.string(), z.any())

// What the view and the mod are told about the app they show.
const meta = (app, how) => ({ id: app.id, name: app.name, about: app.about, kind: app.kind, tool: app.tool, tools: app.tools, version: app.version, saved: app.saved, how })

function sample(data) {
  const text = JSON.stringify(data, null, 1)
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
        'things to compare side by side, something to book or buy, things to browse for ideas, or what you found yourself (pages from a web search, documents ' +
        'you read) for the person to look through. It searches the person’s saved apps by words and meaning ' +
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
        kind: KIND.describe('info (looking something up), action (getting something done: book, buy) or discover (browsing for ideas)'),
        tool: TOOL.describe('Its first tool, which fills it'),
        args: ARGS.describe('That tool’s arguments for this request'),
        tools: z.array(TOOL).max(8).describe('Every tool it calls: the first and those its buttons need'),
        html: z.string().min(40).max(60000).describe('The app’s fragment, written as app_kit says: a <style>, the markup and one <script type="module">'),
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
        html: z.string().min(40).max(60000).describe('The app’s whole new fragment'),
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
      inputSchema: { id: z.string().min(1).max(64), tool: TOOL, args: ARGS.optional() },
      annotations: { openWorldHint: true },
      _meta: { ui: { resourceUri: RUNNER, visibility: ['app'] } },
    },
    async ({ id, tool, args }) => {
      const apps = await store()
      const app = await apps.get(id)
      if (!app) return fail(`No app ${id}.`)
      if (!app.tools.includes(tool)) return fail(`${tool} is not one of this app’s tools`)
      const data = await callTool(tool, args ?? {})
      // A tool that acts (a booking, the cart): the view tells the model.
      const acted = TOOLS.find(t => t.name === tool)?.acts === true
      return { content: [{ type: 'text', text: data.summary }], structuredContent: acted ? { ...data, acted: true } : data }
    },
  )
}
