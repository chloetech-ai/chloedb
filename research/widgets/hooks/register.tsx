// Widgets in Claude Code: when the model calls one of the widgets server's
// tools (stock_quote, flight_track, shop_search, cart_*), its answer is drawn
// as a card under the tool call, on every surface; the model reads one
// sentence instead of the data. A card's Pin keeps it live in a pane beside
// the chat, refreshed every 20 seconds.
//
// The server is ../server (an MCP server any MCP Apps host can draw as
// HTML); this module is how Claude Code, which draws no HTML, shows the same
// widgets.
import { atom, read, update } from 'claude-code'
import type { EngineInterface, Register } from 'claude-code'

import type { Card, CartLine, Data, Kind, Pin } from '../types'
import { drawCard, title } from './cards'
import type { Actions, Kit } from './cards'

const PANE = 'widgets'
const KINDS: Kind[] = ['stock_quote', 'flight_track', 'shop_search', 'cart_add', 'cart_view']
const MAX_CARDS = 40
const REFRESH_MS = 20_000

// A new tag when a value's fields change, so a reload declines the old.
const cards = atom({ plugin: 'widgets', key: 'cards' } as const, {}, { shape: 'cards-1' })
const pins = atom({ plugin: 'widgets', key: 'pins' } as const, [], { shape: 'pins-1' })
const cart = atom({ plugin: 'widgets', key: 'cart' } as const, [], { shape: 'cart-1' })

// `mcp__<server>__<tool>` for one of the widget tools, whatever the server
// is called: `widgets` configured by hand, `plugin_widgets_widgets` when
// this plugin starts it.
export function widgetOf(tool: string): { server: string; kind: Kind } | null {
  const m = /^mcp__(.+)__([a-z_]+)$/.exec(tool)
  if (!m || !KINDS.includes(m[2] as Kind)) return null
  return { server: m[1]!, kind: m[2] as Kind }
}

// Claude Code hands on a tool's structuredContent as JSON text.
export function parse(text: unknown): Data | null {
  if (typeof text !== 'string' || !text.startsWith('{')) return null
  try {
    const data = JSON.parse(text)
    return data && typeof data === 'object' && ['stock', 'flight', 'shop'].includes(data.widget) ? (data as Data) : null
  } catch {
    return null
  }
}

function newest<T extends { at: number }>(all: { [id: string]: T }, keep: number): { [id: string]: T } {
  return Object.fromEntries(
    Object.entries(all)
      .sort((a, b) => b[1].at - a[1].at)
      .slice(0, keep),
  )
}

type Args = Card['args']
function argsOf(e: { [key: string]: unknown }): Args {
  const args: Args = {}
  for (const [key, value] of Object.entries(e)) {
    if (key === 'tool' || key === 'tool_use_id') continue
    if (value === null || ['string', 'number', 'boolean'].includes(typeof value)) args[key] = value as Args[string]
  }
  return args
}

async function callTool($: EngineInterface, server: string, kind: Kind, args: Args): Promise<Data | null> {
  const result = await $.mcp.call(server, kind, args)
  const block = result.content.find(c => c.type === 'text')
  return result.isError ? null : parse(block?.text ?? result.structuredContent)
}

function remember($: EngineInterface, data: Data) {
  if (data.widget === 'shop') return update($, cart, () => data.cart as CartLine[])
}

async function refresh($: EngineInterface) {
  const list = await read($, pins)
  for (const pin of list) {
    const data = await callTool($, pin.server, pin.kind, pin.args).catch(() => null)
    if (!data) continue
    await update($, pins, now => now.map(p => (p.id === pin.id ? { ...p, data, at: Date.now() } : p)))
  }
}

export const register: Register = on => {
  on('session.start', async ($, e, next) => {
    await $.command.register({ name: 'widgets', description: 'Show the widgets pinned live beside the chat' })
    $.clock.every(REFRESH_MS, () => void refresh($))
    if ((await read($, pins)).length) void $.ui.open({ id: PANE, title: 'Live widgets' })
    return next(e)
  })

  on('command.run', { command: 'widgets' }, async $ => {
    await $.ui.open({ id: PANE, title: 'Live widgets' })
    const count = (await read($, pins)).length
    return { text: count ? `${count} widget(s) pinned.` : 'No widgets pinned yet: press Pin on a widget card.' }
  })

  // The widget tools sit in the model's list from the start rather than
  // behind ToolSearch, so a request that needs one opens it in one step.
  on('tool.describe', async ($, e, next) => {
    const described = await next(e)
    return widgetOf(e.tool) ? { ...described, isDeferred: false } : described
  })

  on('tool.call', async ($, e, next) => {
    const widget = widgetOf(e.tool)
    if (!widget) return next(e)
    const ran = await next(e)
    if (ran.deny !== undefined || ran.isError) return ran
    const data = parse(ran.text ?? ran.result)
    if (!data) return ran
    const card: Card = { kind: widget.kind, server: widget.server, args: argsOf(e), data, at: Date.now() }
    await update($, cards, all => newest({ ...all, [e.tool_use_id]: card }, MAX_CARDS))
    await remember($, data)
    // The card shows the data; the model reads the sentence the server wrote.
    return data.summary ? { result: data.summary } : ran
  }).catch(($, e, next) => next(e))

  // A run of calls the transcript folds into one line keeps its line, and
  // the cards of the widgets in it draw under it.
  on('ui.render', { component: 'ToolGroup' }, async ($, e, next) => {
    const line = await next(e)
    if (e.props.isExpanded) return line
    const all = await read($, cards)
    const ids = e.props.calls.filter(c => widgetOf(c.tool) && c.tool_use_id && all[c.tool_use_id]).map(c => c.tool_use_id!)
    if (!ids.length) return line
    const { Box } = $.ui.resolve(e)
    const drawn = await Promise.all(ids.map(id => cardBox($, e, id, all[id]!)))
    return (
      <Box flexDirection="column">
        {line}
        {drawn}
      </Box>
    )
  })

  // The card under a tool call's own row (a call not folded into a run, or
  // a run unfolded by ctrl+o).
  on('ui.render', { component: 'ToolUse' }, async ($, e, next) => {
    if (!widgetOf(e.props.tool)) return next(e)
    const row = await next(e)
    const card = (await read($, cards))[e.props.tool_use_id]
    if (!card) return row
    const { Box } = $.ui.resolve(e)
    const drawn = await cardBox($, e, e.props.tool_use_id, card)
    // The phone app draws a card under the row when the Box holding both
    // has a border.
    return e.surface === 'mobile' ? (
      <Box borderStyle="round" flexDirection="column">
        {row}
        {drawn}
      </Box>
    ) : (
      <Box flexDirection="column">
        {row}
        {drawn}
      </Box>
    )
  })

  // The sentence the model read is the card's to show: the row under a
  // widget call draws nothing of its own, unless the whole transcript is
  // asked for (ctrl+o).
  on('ui.render', { component: 'ToolResult' }, async ($, e, next) => {
    if (!widgetOf(e.props.tool) || e.props.isExpanded || e.props.isErrored) return next(e)
    const { Box } = $.ui.resolve(e)
    return <Box />
  })

  // The pane of pinned widgets, newest first.
  on('ui.render', { component: 'Pane', requestId: PANE }, async ($, e) => {
    const { Box, Text } = $.ui.resolve(e)
    const list = await read($, pins)
    const columns = e.props.bodyColumns ?? 60
    const now = await $.clock.now()
    const shopCart = await read($, cart)
    if (!list.length) {
      return <Text dimColor>Nothing pinned yet. Press Pin live on a stock card, or Follow live on a flight.</Text>
    }
    return (
      <Box flexDirection="column" gap={1}>
        {[...list].reverse().map(pin => (
          <Box key={pin.id} borderStyle="round" borderDimColor flexDirection="column" paddingX={1}>
            {drawCard(kit($, e), pin, { columns: columns - 4, compact: true, cart: shopCart, now }, actions($, pin.id, pin, true))}
          </Box>
        ))}
        <Text dimColor>Refreshed every {REFRESH_MS / 1000}s.</Text>
      </Box>
    )
  })
}

type Site = Parameters<EngineInterface['ui']['resolve']>[0] & { viewport?: { columns: number } }

// One widget's card, in a rounded border on the surfaces that draw one.
async function cardBox($: EngineInterface, e: Site, id: string, card: Card) {
  const { Box } = $.ui.resolve(e)
  const pinned = await read($, pins)
  const drawn = drawCard(
    kit($, e),
    card,
    { columns: (e.viewport?.columns ?? 80) - 6, compact: false, cart: await read($, cart), now: await $.clock.now() },
    actions($, id, card, pinned.some(p => p.id === id)),
  )
  return e.surface === 'mobile' ? (
    <Box key={id} flexDirection="column">
      {drawn}
    </Box>
  ) : (
    <Box key={id} borderStyle="round" borderDimColor flexDirection="column" paddingX={1} marginLeft={2}>
      {drawn}
    </Box>
  )
}

// The terminal draws text art; every other surface has Svg. (A table is
// completed with every element name, an unknown one drawing nothing, so
// the surface is what tells.)
function kit($: EngineInterface, e: Parameters<EngineInterface['ui']['resolve']>[0]): Kit {
  const table = $.ui.resolve(e)
  const svg = e.surface !== 'terminal' && 'Svg' in table ? table.Svg : undefined
  return { Box: table.Box, Text: table.Text, Button: table.Button, Svg: svg }
}

function actions($: EngineInterface, id: string, card: Card, isPinned: boolean): Actions {
  return {
    isPinned,
    pin: () =>
      void (async () => {
        const pin: Pin = { ...card, id }
        await update($, pins, list => [...list.filter(p => p.id !== id), pin].slice(-6))
        await $.ui.open({ id: PANE, title: 'Live widgets' })
        $.ui.toast(`Pinned ${title(card)}: it refreshes every ${REFRESH_MS / 1000}s`)
      })(),
    unpin: () => void update($, pins, list => list.filter(p => p.id !== id)),
    ask: text => void $.prompt.fill({ text }),
    add: productId =>
      void (async () => {
        const data = await callTool($, card.server, 'cart_add', { id: productId }).catch(() => null)
        if (!data || data.widget !== 'shop') {
          $.ui.toast('Could not add it to the cart')
          return
        }
        await remember($, data)
        $.ui.toast(data.note ?? 'Added to the cart')
      })(),
  }
}
