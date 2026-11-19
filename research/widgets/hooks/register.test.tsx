// The widgets mod against the engine's own test kit: a widget tool's answer
// becomes a card under its row on every surface, the model reads one
// sentence, the shop's Add reaches the server, and Pin opens the live pane.
//
//   claude plugin test research/widgets
import { expect, test } from 'claude-code/testing'

const STOCK = {
  widget: 'stock',
  summary: 'NVIDIA Corporation (NVDA) 151.94 USD, -3.52 (-2.26%) over 1d.',
  symbol: 'NVDA',
  name: 'NVIDIA Corporation',
  exchange: 'NasdaqGS',
  currency: 'USD',
  range: '1d',
  price: 151.94,
  base: 155.46,
  change: -3.52,
  changePct: -2.26,
  high: 159.7,
  low: 150.85,
  volume: 222_200_000,
  asOf: 1_795_026_000_000,
  points: Array.from({ length: 60 }, (_, i) => [1_795_000_000_000 + i * 300_000, 159 - i * 0.12 + (i % 5) * 0.3]),
  source: 'Yahoo Finance',
}

const FLIGHT = {
  widget: 'flight',
  summary: 'DLH773 (Lufthansa) is airborne over Myanmar (Burma).',
  found: true,
  query: 'LH773',
  callsign: 'DLH773',
  airline: 'Lufthansa',
  icao24: '3c4b26',
  registeredIn: 'Germany',
  lat: 17.07,
  lon: 96.13,
  over: 'Myanmar (Burma)',
  altitudeFt: 36200,
  speedKts: 501,
  heading: 319,
  climbFpm: 0,
  onGround: false,
  trail: [
    [16.5, 96.6],
    [17.07, 96.13],
  ],
  asOf: 1_795_026_000_000,
  source: 'OpenSky Network',
}

const SHOP = {
  widget: 'shop',
  summary: '3 products match "headphones".',
  mode: 'results',
  query: 'headphones',
  maxPrice: null,
  total: 2,
  items: [
    { id: 101, title: 'Apple AirPods Max Silver', brand: 'Apple', price: 549.99, rating: 3.5, category: 'audio', discountPct: 14, stock: 5 },
    { id: 104, title: 'Beats Flex Wireless Earphones', brand: 'Beats', price: 49.99, rating: 4.7, category: 'audio', discountPct: 6, stock: 9 },
  ],
  cart: [],
  source: 'DummyJSON demo store',
}

const SURFACES = ['terminal', 'desktop', 'vscode', 'mobile'] as const
const NOW = 1_795_026_030_000

// Stands in for the engine beneath the plugin, before the test's first call
// on $: a widget tool answers the JSON Claude Code makes of its
// structuredContent, and the server's cart_add answers a cart.
function engine(on: any, data: object) {
  const seen = { id: '', mcp: [] as unknown[], opened: [] as string[] }
  const json = JSON.stringify(data)
  on('tool.call', (_: unknown, e: { tool_use_id: string }) => {
    seen.id = e.tool_use_id
    return { result: json, text: json }
  })
  on('clock.now', () => ({ value: NOW }))
  on('ui.toast', () => ({ value: undefined }))
  on('ui.open', (_: unknown, e: { id: string }) => {
    seen.opened.push(e.id)
    return { value: { isPlaced: true } }
  })
  on('mcp.call', (_: unknown, e: { server: string; tool: string; args: unknown }) => {
    seen.mcp.push([e.server, e.tool, e.args])
    const cart = { widget: 'shop', summary: 'Cart: 1 item(s), $49.99.', mode: 'cart', items: [], cart: [{ id: 104, title: 'Beats Flex Wireless Earphones', price: 49.99, qty: 1 }], note: 'Added 1 × Beats Flex Wireless Earphones.' }
    return { value: { content: [{ type: 'text', text: JSON.stringify(cart) }], isError: false } }
  })
  return seen
}

const row = (id: string, tool: string) => ({
  plugin: 'widgets',
  component: 'ToolUse',
  requestId: id,
  viewport: { columns: 100, rows: 40 },
  props: { tool_use_id: id, tool, input: {}, isRunning: false, isErrored: false, isInterrupted: false },
})

declare function setTimeout(fn: () => void, ms: number): unknown
const settle = () => new Promise<void>(r => setTimeout(r, 50))

test('a stock answer is a card on every surface, and the model reads one sentence', async ($, on) => {
  const seen = engine(on, STOCK)
  const ran = await $.tool.call({ tool: 'mcp__widgets__stock_quote', symbol: 'NVDA' } as any)
  expect((ran as any).result).toBe(STOCK.summary)

  for (const surface of SURFACES) {
    const ui = await $.ui.mount({ ...row(seen.id, 'mcp__widgets__stock_quote'), surface } as any)
    expect(await ui.find({ type: 'Text', text: /NVIDIA Corporation/ })).toBeDefined()
    expect(await ui.find({ type: 'Text', text: /▼ -3\.52 \(-2\.26%\)/ })).toBeDefined()
    if (surface === 'terminal') {
      expect(await ui.find({ type: 'Text', text: /[▁▂▃▄▅▆▇█]{20}/ })).toBeDefined()
    } else {
      expect(String((await ui.find({ type: 'Svg' }))?.props.alt)).toMatch(/NVDA today/)
    }
    expect(await ui.find({ key: 'pin' })).toBeDefined()
    await ui.unmount()
  }
})

test('a flight is a map: braille in the terminal, SVG elsewhere', async ($, on) => {
  const seen = engine(on, FLIGHT)
  const tool = 'mcp__plugin_widgets_widgets__flight_track'
  await $.tool.call({ tool, flight: 'LH773' } as any)
  const term = await $.ui.mount({ ...row(seen.id, tool), surface: 'terminal' } as any)
  expect(await term.find({ type: 'Text', text: /^↖$/ })).toBeDefined()
  expect(await term.find({ type: 'Text', text: /[⠁-⣿]{10}/ })).toBeDefined()
  expect(await term.find({ type: 'Text', text: /36,200 ft · 501 kt · heading 319°/ })).toBeDefined()
  await term.unmount()

  const desk = await $.ui.mount({ ...row(seen.id, tool), surface: 'desktop' } as any)
  expect(String((await desk.find({ type: 'Svg' }))?.props.source)).toMatch(/rotate\(319\)/)
  await desk.unmount()
})

test("the shop's Add puts the product in the cart through the server", async ($, on) => {
  const seen = engine(on, SHOP)
  await $.tool.call({ tool: 'mcp__widgets__shop_search', query: 'headphones' } as any)
  const ui = await $.ui.mount({ ...row(seen.id, 'mcp__widgets__shop_search'), surface: 'terminal' } as any)
  await ui.press({ key: 'add-104' })
  await settle()
  expect(seen.mcp).toEqual([['widgets', 'cart_add', { id: 104 }]])
  await ui.unmount()

  const again = await $.ui.mount({ ...row(seen.id, 'mcp__widgets__shop_search'), surface: 'terminal' } as any)
  expect(await again.find({ type: 'Text', text: /1 × Beats Flex Wireless Earphones/ })).toBeDefined()
  expect(await again.find({ key: 'add-104' })).toBeDefined()
  await again.unmount()
})

test('Pin keeps a widget in the live pane', async ($, on) => {
  const seen = engine(on, STOCK)
  await $.tool.call({ tool: 'mcp__widgets__stock_quote', symbol: 'NVDA' } as any)
  const card = await $.ui.mount({ ...row(seen.id, 'mcp__widgets__stock_quote'), surface: 'desktop' } as any)
  await card.press({ key: 'pin' })
  await settle()
  expect(seen.opened).toEqual(['widgets'])
  await card.unmount()

  const pane = await $.ui.mount({
    plugin: 'widgets',
    component: 'Pane',
    requestId: 'widgets',
    surface: 'terminal',
    props: { title: 'Live widgets', isFocused: false, bodyColumns: 50, placement: 'dock', scroll: { offset: 0, bodyRows: 30 }, view: {} },
  } as any)
  expect(await pane.find({ type: 'Text', text: /NVIDIA Corporation/ })).toBeDefined()
  expect(await pane.find({ key: 'unpin' })).toBeDefined()
  await pane.unmount()
})

test('a folded run of calls keeps its line, with the widget cards under it', async ($, on) => {
  const seen = engine(on, FLIGHT)
  on('ui.render', ($: any, e: any) => {
    const { Text } = $.ui.resolve(e)
    return <Text>Called plugin:widgets:widgets</Text>
  })
  const tool = 'mcp__plugin_widgets_widgets__flight_track'
  await $.tool.call({ tool, flight: 'LH773' } as any)
  const group = (calls: object[]) => ({
    plugin: 'widgets',
    component: 'ToolGroup',
    requestId: 'group-1',
    surface: 'terminal',
    props: { calls, isActive: false, isExpanded: false },
  })
  const call = (id: string, name: string) => ({ tool_use_id: id, tool: name, input: {}, isRunning: false, isErrored: false, isInterrupted: false })
  const ui = await $.ui.mount(group([call('r', 'Read'), call(seen.id, tool)]) as any)
  expect(await ui.find({ type: 'Text', text: /Called plugin:widgets:widgets/ })).toBeDefined()
  expect(await ui.find({ type: 'Text', text: /36,200 ft/ })).toBeDefined()
  await ui.unmount()
  const other = await $.ui.mount(group([call('r', 'Read')]) as any)
  expect(await other.find({ type: 'Text', text: /36,200 ft/ })).toBeUndefined()
  await other.unmount()
})

test('other tools keep their own rows', async ($, on) => {
  on('ui.render', ($: any, e: any) => {
    const { Text } = $.ui.resolve(e)
    return <Text>engine row</Text>
  })
  const ui = await $.ui.mount({ ...row('toolu_other', 'Bash'), surface: 'terminal' } as any)
  expect((await ui.find({ type: 'Text', text: /engine row/ }))?.text).toBe('engine row')
  await ui.unmount()
})

// An instant app made from the shop's data, as app_make answers it.
const APP = { id: 'product-comparison-fbbbf8', name: 'Product comparison', about: 'Puts chosen products side by side.', kind: 'info', tool: 'shop_search', tools: ['shop_search', 'cart_add'], version: 1, saved: false, how: 'made', args: { query: 'blender' } }
const MADE = { ...SHOP, instant: { ...APP, note: 'Made for this request. It is a draft until it is saved.' } }
// One no card knows: a list of recipes.
const RECIPES = { summary: '2 recipes.', meals: [{ id: 1, name: 'Shakshuka', area: 'Tunisian' }, { id: 2, name: 'Dal', area: 'Indian' }], instant: { ...APP, id: 'recipe-ideas-01ab23', name: 'Recipe ideas', tool: 'recipes_browse', kind: 'discover', how: 'recalled', saved: true } }
const SHELF = { summary: '1 saved app.', widget: 'apps', apps: [{ id: APP.id, name: APP.name, about: APP.about, kind: 'info', uses: 1 }] }

// The engine for app tools: app_save and my_apps answer as the server does.
function appEngine(on: any, data: object) {
  const seen = { id: '', mcp: [] as unknown[], opened: [] as string[], toasts: [] as string[], filled: [] as string[] }
  const json = JSON.stringify(data)
  on('tool.call', (_: unknown, e: { tool_use_id: string; tool: string }) => {
    seen.id = e.tool_use_id
    const text = e.tool.endsWith('__my_apps') ? JSON.stringify(SHELF) : json
    return { result: text, text }
  })
  on('clock.now', () => ({ value: NOW }))
  on('ui.toast', (_: unknown, e: { text: string }) => {
    seen.toasts.push(e.text)
    return { value: undefined }
  })
  on('prompt.fill', (_: unknown, e: { text: string }) => {
    seen.filled.push(e.text)
    return { isFilled: true, text: e.text, cursor: e.text.length }
  })
  on('ui.open', (_: unknown, e: { id: string }) => {
    seen.opened.push(e.id)
    return { value: { isPlaced: true } }
  })
  on('mcp.call', (_: unknown, e: { server: string; tool: string; args: unknown }) => {
    seen.mcp.push([e.server, e.tool, e.args])
    const answer = e.tool === 'my_apps' ? SHELF : { summary: 'Saved.', id: APP.id, saved: true }
    return { value: { content: [{ type: 'text', text: JSON.stringify(answer) }], isError: false } }
  })
  return seen
}

test('a made app is its card under a line with its name and Save, and the model is told what next', async ($, on) => {
  const seen = appEngine(on, MADE)
  const tool = 'mcp__widgets__app_make'
  const ran = await $.tool.call({ tool, name: APP.name, html: '<div>…</div>' } as any)
  expect((ran as any).result).toBe(`${SHOP.summary}\n\n${MADE.instant.note}`)

  for (const surface of SURFACES) {
    const ui = await $.ui.mount({ ...row(seen.id, tool), surface } as any)
    expect(await ui.find({ type: 'Text', text: /Product comparison/ })).toBeDefined()
    expect(await ui.find({ type: 'Text', text: /new app/ })).toBeDefined()
    expect(await ui.find({ type: 'Text', text: /AirPods Max/ })).toBeDefined()
    expect(await ui.find({ key: 'save' })).toBeDefined()
    await ui.unmount()
  }

  const ui = await $.ui.mount({ ...row(seen.id, tool), surface: 'terminal' } as any)
  await ui.press({ key: 'save' })
  await settle()
  expect(seen.mcp).toEqual([
    ['widgets', 'app_save', { id: APP.id }],
    ['widgets', 'my_apps', {}],
  ])
  expect(seen.toasts.some(t => /Saved Product comparison/.test(t))).toBe(true)
  await ui.unmount()
  const again = await $.ui.mount({ ...row(seen.id, tool), surface: 'terminal' } as any)
  expect(await again.find({ key: 'save' })).toBeUndefined()
  expect(await again.find({ type: 'Text', text: /saved/ })).toBeDefined()
  await again.unmount()

  // Your apps now lists it, and Open asks for it in the prompt.
  const pane = await $.ui.mount({
    plugin: 'widgets',
    component: 'Pane',
    requestId: 'apps',
    surface: 'desktop',
    props: { title: 'Your apps', isFocused: false, bodyColumns: 60, placement: 'dock', scroll: { offset: 0, bodyRows: 30 }, view: {} },
  } as any)
  expect(await pane.find({ type: 'Text', text: /LOOK THINGS UP/ })).toBeDefined()
  await pane.press({ key: `open-${APP.id}` })
  await settle()
  expect(seen.filled).toEqual(['Open my "Product comparison" app'])
  await pane.unmount()
})

test('an app of data no card knows lists what it holds', async ($, on) => {
  const seen = appEngine(on, RECIPES)
  const tool = 'mcp__widgets__app_open'
  await $.tool.call({ tool, id: RECIPES.instant.id } as any)
  const ui = await $.ui.mount({ ...row(seen.id, tool), surface: 'terminal' } as any)
  expect(await ui.find({ type: 'Text', text: /Recipe ideas/ })).toBeDefined()
  expect(await ui.find({ type: 'Text', text: /from your apps/ })).toBeDefined()
  expect(await ui.find({ type: 'Text', text: /Shakshuka/ })).toBeDefined()
  expect(await ui.find({ key: 'save' })).toBeUndefined()
  await ui.unmount()
})

test("the model's my_apps opens Your apps beside the chat", async ($, on) => {
  const seen = appEngine(on, SHELF)
  const ran = await $.tool.call({ tool: 'mcp__widgets__my_apps' } as any)
  expect((ran as any).result).toBe(SHELF.summary)
  expect(seen.opened).toEqual(['apps'])
})

test('the other app tools answer the model in the sentence the server wrote', async ($, on) => {
  const found = { summary: 'The person’s saved apps that may fit "compare two headphones", best first: …', apps: [{ id: APP.id, score: 0.05 }] }
  appEngine(on, found)
  const ran = await $.tool.call({ tool: 'mcp__plugin_widgets_widgets__apps_find', request: 'compare two headphones' } as any)
  expect((ran as any).result).toBe(found.summary)
})

// What Claude found on the web, shown by the saved Search results app.
const FOUND = {
  widget: 'findings',
  summary: '2 document(s) for "quiet headphones".',
  query: 'quiet headphones',
  documents: [
    { n: 1, title: 'The best noise-cancelling headphones', url: 'https://example.com/best', source: 'example.com', date: '2026-10-02', kind: 'review', summary: 'Sony cancels the most noise.', quotes: ['The XM6 cut more rumble than anything else.'], facts: { price: 449 } },
    { n: 2, title: 'A field note without an address', url: null, source: 'notes.md', date: null, kind: null, summary: 'Bose is the most comfortable.', quotes: [], facts: {} },
  ],
  facts: ['price'],
  notes: 'Both put Sony first for quiet.',
  instant: { ...APP, id: 'search-results', name: 'Search results', tool: 'findings', tools: ['findings'], how: 'recalled', saved: true, args: { query: 'quiet headphones' } },
}

test('what Claude found is a list of its documents with their links, and asks about them', async ($, on) => {
  const seen = appEngine(on, FOUND)
  const tool = 'mcp__widgets__app_open'
  await $.tool.call({ tool, id: 'search-results' } as any)
  for (const surface of SURFACES) {
    const ui = await $.ui.mount({ ...row(seen.id, tool), surface } as any)
    expect(await ui.find({ type: 'Text', text: /Search results/ })).toBeDefined()
    expect(await ui.find({ type: 'Link', href: 'https://example.com/best' })).toBeDefined()
    expect(await ui.find({ type: 'Text', text: /A field note without an address/ })).toBeDefined()
    expect(await ui.find({ type: 'Text', text: /example\.com · 2026-10-02 · review/ })).toBeDefined()
    expect(await ui.find({ type: 'Text', text: /Both put Sony first/ })).toBeDefined()
    await ui.unmount()
  }
  const ui = await $.ui.mount({ ...row(seen.id, tool), surface: 'desktop' } as any)
  await ui.press({ key: 'compare' })
  await settle()
  expect(seen.filled).toEqual(['Compare what these sources say about quiet headphones.'])
  await ui.unmount()
})

test('after a web search the model is told it can show what it found in an app', async ($, on) => {
  on('tool.call', () => ({ result: { query: 'quiet headphones', results: [] } }))
  const ran: any = await $.tool.call({ tool: 'WebSearch', query: 'quiet headphones' } as any)
  expect(ran.context.at(-1)).toMatch(/mcp__plugin_widgets_widgets__app_open with id "search-results"/)
  const other: any = await $.tool.call({ tool: 'Read', file_path: '/x' } as any)
  expect(other.context ?? []).toEqual([])
})
