// What a model needs to write an app: the kit every app page has, how an
// app is written against it, and one made by hand to show it. The chat's
// model reads it (chat/model.mjs), and so does the Claude of a conversation
// in Claude Desktop, claude.ai or Claude Code, through app_kit (apps.mjs).
import { readFile } from 'node:fs/promises'

export const KINDS = 'info (looking something up: a stock chart, products compared, where a flight is), action (getting something done: booking a table, buying), or discover (browsing for ideas: dishes to cook, products to explore)'

const VIEWS = new URL('./views/', import.meta.url)

// `answer` says how to hand the app back: in a fence, or as a tool's input.
export async function kitGuide({ answer = ' Answer with the fragment alone in one \`\`\`html fence.' } = {}) {
  const example = await readFile(new URL('stock.html', VIEWS), 'utf8')
  return `You write one small app shown inside a chat conversation, in a sandboxed iframe 320 to 760 pixels wide. An app is for ${KINDS}.

Write only the app's fragment: one <style> element, the markup, and one <script type="module"> with its code. No <html>, <head> or <body>, no other scripts, no stylesheets or fonts from anywhere, no fetch or XMLHttpRequest: the page can load nothing. Pictures may come only from image URLs in the tools' data.${answer}

The page already has a kit, globalThis.Kit:
- Kit.connect(name, draw) connects to the chat and returns a promise of the connection, app. draw(data) gets the data of the app's tool each time there is some: when it opens, and when the chat fills it again for a later request. Set draw.input = args => ... to show what is being loaded, and draw.error = message => ... for a tool that failed. Call connect once, at the end of the script.
- await Kit.call(app, toolName, args) calls one of the app's tools and returns its data, the same shape as draw's: for buttons that fetch more or act (Book, Add to cart, Show the recipe, another range). Show that it is working while it runs, and the error in place if it throws.
- Kit.ask(app, text) puts a message in the chat as the person's, for a follow-up ("Why did it move?", "What goes with this?").
- Kit.tell(app, text) tells the conversation, in one sentence, what the person now sees or picked ("Selected: Huawei Matebook X Pro, $1,399"), so a later question can refer to it. Call it when they pick, select or filter something, not on every draw.
- Kit.link(app, url) asks the host to open a web address (a document's link) in the browser; use it for links, since the page cannot navigate.
- Kit.money(n, currency = 'USD'), Kit.number(n, digits), Kit.ago(ms), and Kit.esc(text), which escapes text for innerHTML: escape every text that comes from data.

The kit's CSS follows the chat's light or dark theme: variables --bg (the page), --bg-2 (a raised surface), --ink, --ink-2 (secondary text), --ink-3 (faint), --line (borders), --accent, --up and --down (green and red, with --up-bg and --down-bg), --radius, --font, --mono; classes .row (a flex row), .between, .wrap, .muted, .faint, .num, .title, .big (a big number), .chip (with .up and .down), .tabs with .tab buttons (aria-pressed="true" for the chosen one), .btn and .btn.primary, .live (a pulsing dot), .foot (a footer line), .empty (a centered message). body has the font and 14px 16px of padding already.

Make it good to use, not decorated: the answer first, big and clear; the details after; one or two actions that matter. Controls go in one compact row (chips, tabs or a small select), never a stack of form fields: the results take most of the room. About 300 to 600 pixels tall; a list scrolls inside a box rather than growing without end. Draw from the data's real fields (a sample of the first tool's data is in the request), cope with an empty result and with fields missing, and show data.note when there is one: demo data says so. For an action (booking, buying), make it plain what will happen and put the confirmation in place of the button once the tool answers. Never use alert(), confirm() or prompt(). Keep it under 250 lines.

An app made by hand, to show the kit in use (a whole page, where yours is a fragment; its kit:* comments are where the page puts the kit):

${example}`
}

// The app's fragment from a model's answer: the fenced block if there is
// one, which must have its module script.
export function fragment(text) {
  const fenced = /```(?:html)?\s*\n([\s\S]*?)\n```/.exec(text)
  const body = (fenced ? fenced[1] : text).trim()
  if (!/<script[^>]*type=["']?module/i.test(body)) throw new Error('the made app has no <script type="module">')
  return body
}

// Each tool as a model reads it: its name, what it does and its arguments.
export function described(tools) {
  return tools.map(t => ({
    name: t.name,
    description: t.description,
    args: Object.fromEntries(Object.entries(t.input).map(([k, v]) => [k, `${v.isOptional() ? 'optional ' : ''}${typeName(v)}: ${v.description ?? ''}`])),
  }))
}

// By the schema's own type name: the tools' zod is this package's.
function typeName(schema) {
  const inner = schema._def.typeName === 'ZodOptional' ? schema._def.innerType : schema
  if (inner._def.typeName === 'ZodEnum') return `one of ${inner._def.values.join(', ')}`
  if (inner._def.typeName === 'ZodNumber') return 'number'
  if (inner._def.typeName === 'ZodArray') return 'list'
  if (inner._def.typeName === 'ZodBoolean') return 'true or false'
  return 'text'
}

export function toolList(tools) {
  return tools
    .map(t => `- ${t.name}: ${t.description}\n  arguments: ${JSON.stringify(t.args)}`)
    .join('\n')
}
