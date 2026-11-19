// What a model needs to write an app: the kit every app page has, how an
// app is written against it, and one made by hand to show it. The chat's
// model reads it (chat/model.mjs), and so does the Claude of a conversation
// in Claude Desktop, claude.ai or Claude Code, through app_kit (apps.mjs).
import { spawn } from 'node:child_process'
import { mkdtemp, readFile, rm, writeFile } from 'node:fs/promises'
import os from 'node:os'
import path from 'node:path'

export const KINDS = 'info (looking something up: a stock chart, products compared, where a flight is), action (getting something done: booking a table, buying), discover (browsing for ideas: dishes to cook, products to explore), or tool (working something out in the page: a calculator, a function plotted with its derivative, a converter, a simulation, a game)'

const VIEWS = new URL('./views/', import.meta.url)

// `answer` says how to hand the app back: in a fence, or as a tool's input.
export async function kitGuide({ answer = ' Answer with the fragment alone in one \`\`\`html fence.' } = {}) {
  const example = await readFile(new URL('stock.html', VIEWS), 'utf8')
  return `You write one small app shown inside a chat conversation, in a sandboxed iframe 320 to 760 pixels wide. An app is for ${KINDS}.

Write only the app's fragment: one <style> element, the markup, and one <script type="module"> with its code. No <html>, <head> or <body>, no other scripts, no stylesheets or fonts from anywhere, no fetch or XMLHttpRequest: the page can load nothing. Pictures come only from the tools' data: photos handed in ready to show (data: addresses), and image addresses from the tools' own sources.${answer}

The page already has a kit, globalThis.Kit:
- Kit.connect(name, draw) connects to the chat and returns a promise of the connection, app. draw(data) gets the data of the app's tool each time there is some: when it opens, and when the chat fills it again for a later request. Set draw.input = args => ... to show what is being loaded, and draw.error = message => ... for a tool that failed. Call connect once, at the end of the script.
- await Kit.call(app, toolName, args) calls one of the app's tools and returns its data, the same shape as draw's: for buttons that fetch more or act (Book, Add to cart, Show the recipe, another range). Show that it is working while it runs, and the error in place if it throws.
- Kit.ask(app, text) puts a message in the chat as the person's, for a follow-up ("Why did it move?", "What goes with this?").
- Kit.tell(app, text) tells the conversation, in one sentence, what the person now sees or picked ("Selected: Huawei Matebook X Pro, $1,399"), so a later question can refer to it. Call it when they pick, select or filter something, not on every draw.
- Kit.link(app, url) asks the host to open a web address (a document's link) in the browser; use it for links, since the page cannot navigate.
- Kit.money(n, currency = 'USD'), Kit.number(n, digits), Kit.ago(ms), and Kit.esc(text), which escapes text for innerHTML: escape every text that comes from data.
- Kit.math.parse(text) reads a formula as a person types it ("x^2 sin x", "e^(-x)", "ln(x)/x", "sqrt(1-x^2)", "2x+1", "|x|", "y = 3x^4", "sin^2 x"): it returns an expression e with e.text and e.html (written back neatly; e.html has <sup> for powers and is safe for innerHTML), e.f(x) (its value; NaN outside its domain), e.eval({ x, t, a }) for several variables, e.vars (['x']) and e.derive('x'), its exact derivative as another expression (so e.derive().derive() is the second). It throws an Error whose message says what is wrong ("Missing ) after sin(… at the end"): show it in place while the person types, and keep the last good one drawn. Also Kit.math.slope(f, x) (a numerical derivative), Kit.math.integrate(f, a, b), Kit.math.zeros(f, a, b) (where f crosses zero) and Kit.math.format(n) (a short number, 1/3 as a fraction).
- Kit.plot(canvas, view) draws a grid, axes with numbers, and curves on a <canvas> in the theme's colors, at the canvas's CSS size and sharp on any screen: view = { x: [min, max], y: [min, max] (left out, it fits the curves), curves: [{ f, color, width, dash: [4, 4], area: [a, b] (shades under it), alpha }], lines: [{ through: [x, y], slope }, { from: [x, y], to: [x, y] }, each with color, width, dash], points: [{ x, y, color, label, r }] }. Colors are CSS colors or the kit's variables by name ('--accent', '--down', '--up', '--ink-2'). It returns { px(x, y) → [left, top], x(left), y(top), ctx, width, height, view } to draw more on the canvas's 2d context or to read the pointer (x(e.offsetX)). To animate (a tangent sliding along a curve, a secant closing in on it, a point moving), call it again in requestAnimationFrame with the new lines and points: it clears and redraws in a few milliseconds.

The kit's CSS follows the chat's light or dark theme: variables --bg (the page), --bg-2 (a raised surface), --ink, --ink-2 (secondary text), --ink-3 (faint), --line (borders), --accent, --up and --down (green and red, with --up-bg and --down-bg), --radius, --font, --mono; classes .row (a flex row), .between, .wrap, .muted, .faint, .num, .title, .big (a big number), .chip (with .up and .down), .tabs with .tab buttons (aria-pressed="true" for the chosen one), .btn and .btn.primary, .live (a pulsing dot), .foot (a footer line), .empty (a centered message). body has the font and 14px 16px of padding already.

Make it good to use, not decorated: the answer first, big and clear; the details after; one or two actions that matter. Controls go in one compact row (chips, tabs or a small select), never a stack of form fields: the results take most of the room. About 300 to 600 pixels tall; a list scrolls inside a box rather than growing without end. Draw from the data's real fields (a sample of the first tool's data is in the request), cope with an empty result and with fields missing, and show data.note when there is one: demo data says so. No emoji anywhere, not as pictures, icons, bullets or decoration: most screens draw them as colored cartoons, which looks like a toy. A picture comes only from the data: an image address, or a photo handed in ready to show (a data: address, such as each found item's "image"), in an <img> with object-fit: cover; when there is none, leave the picture out (no placeholder, no letter in a circle). An icon (play, pause, an arrow, a check) is a small inline <svg> in currentColor, or just a word. The one exception is an app whose content is the symbols themselves (a game's pieces, an emoji picker). For an action (booking, buying), make it plain what will happen and put the confirmation in place of the button once the tool answers. Never use alert(), confirm() or prompt(). A form's submit handler runs (the kit sends it, as the frame may not submit forms) but nothing is sent anywhere: read the fields there and call e.preventDefault(). Keep it under 250 lines.

An app on the tool inputs works everything out in the page: its data is { request, values }, the values the request named ({ f: 'x^2 sin(x)' }). Start from them, let the person change them in place (a field for the formula, sliders, play and pause), recompute as they type with no tool call, and when draw is called again with new values, put every one in its field (a value left out keeps what is shown). Read each value by the name you were given; when a later call names a value you have no field for, say so in place in a short line ("Not used: unit cost") rather than showing an answer that ignores it. Such an app may be as rich as it needs to be: canvas drawings, animation with requestAnimationFrame, keyboard and pointer controls, presets to try; up to 600 lines. Check your arithmetic and edge cases (a formula that fails to read, a value outside its domain, a vertical asymptote, an empty field): the app must never stop on one.

An app made by hand, to show the kit in use (a whole page, where yours is a fragment; its kit:* comments are where the page puts the kit):

${example}`
}

// Whether a fragment's scripts run: each module script is checked by
// node --check (it does not run), so a syntax error comes back as a
// sentence and a line, for the model to fix before anyone sees it.
export async function checkFragment(html) {
  const scripts = [...String(html).matchAll(/<script\b[^>]*>([\s\S]*?)<\/script>/gi)].map(m => m[1])
  if (!scripts.length) return 'it has no <script type="module">'
  const dir = await mkdtemp(path.join(os.tmpdir(), 'widgets-check-'))
  try {
    for (const [i, code] of scripts.entries()) {
      const file = path.join(dir, `app-${i}.mjs`)
      await writeFile(file, code)
      const { status, stderr } = await new Promise(resolve => {
        const child = spawn(process.execPath, ['--check', file], { stdio: ['ignore', 'ignore', 'pipe'] })
        let err = ''
        child.stderr.on('data', d => (err += d))
        child.on('close', code => resolve({ status: code, stderr: err }))
        child.on('error', () => resolve({ status: 0, stderr: '' }))
      })
      if (status !== 0) {
        const lines = stderr.split('\n')
        const at = lines.findIndex(l => /Error\b/.test(l))
        const where = /:(\d+)$/.exec(lines[0] ?? '')?.[1]
        return `${at >= 0 ? lines[at].trim() : 'a syntax error'}${where ? ` at line ${where} of the script` : ''}${lines[1] ? `, near: ${lines[1].trim().slice(0, 160)}` : ''}`
      }
    }
    return null
  } finally {
    await rm(dir, { recursive: true, force: true }).catch(() => {})
  }
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
  if (inner._def.typeName === 'ZodRecord') return 'named values'
  return 'text'
}

export function toolList(tools) {
  return tools
    .map(t => `- ${t.name}: ${t.description}\n  arguments: ${JSON.stringify(t.args)}`)
    .join('\n')
}
