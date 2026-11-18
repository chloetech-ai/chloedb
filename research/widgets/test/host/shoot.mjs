// Screenshots of each widget's view in the test host, and a check that each
// one drew and that its buttons reach the server and the conversation.
//
//   node test/host/host.mjs 8790 &   (WIDGETS_SIMULATE=1 for made-up data)
//   node test/host/shoot.mjs http://127.0.0.1:8790 OUT_DIR
import { mkdirSync } from 'node:fs'
import { createRequire } from 'node:module'

const require = createRequire(import.meta.url)
const { chromium } = require('playwright')

const [base = 'http://127.0.0.1:8790', out = 'shots'] = process.argv.slice(2)
mkdirSync(out, { recursive: true })

const CASES = [
  { name: 'stock', tool: 'stock_quote', args: { symbol: 'NVDA' }, prompt: 'how is nvidia doing today?', reply: 'NVIDIA is down a little today; the chart shows where it slid.', wait: '.big', press: '#why' },
  { name: 'flight', tool: 'flight_track', args: { flight: process.env.FLIGHT ?? 'LH 773' }, prompt: 'is my lufthansa flight in the air yet?', reply: 'Yes: it is cruising, and the map shows where.', wait: '.map svg, .empty' },
  { name: 'shop', tool: 'shop_search', args: { query: 'phone', maxPrice: 900 }, prompt: 'I need a new phone, under $900', reply: 'Here are the phones in that range; the Add to cart buttons work.', wait: '.grid', press: '[data-add]' },
]

const browser = await chromium.launch()
let failed = 0
for (const theme of ['light', 'dark']) {
  for (const width of [760, 390]) {
    const page = await browser.newPage({ viewport: { width, height: 900 }, deviceScaleFactor: 2 })
    for (const c of CASES) {
      const q = new URLSearchParams({ tool: c.tool, args: JSON.stringify(c.args), prompt: c.prompt, reply: c.reply, theme })
      await page.goto(`${base}/?${q}`)
      const frame = page.frameLocator('iframe')
      try {
        await frame.locator(c.wait).first().waitFor({ timeout: 20_000 })
        await page.waitForTimeout(c.name === 'shop' ? 2500 : 600)
        if (c.press && theme === 'light' && width === 760) {
          await frame.locator(c.press).first().click()
          await page.waitForTimeout(1200)
          const events = await page.evaluate(() => window.events.map(e => e[0]))
          const wanted = c.name === 'shop' ? 'tools/call' : 'ui/message'
          if (!events.includes(wanted)) throw new Error(`${c.name}: pressing ${c.press} sent no ${wanted} (${events})`)
        }
        const file = `${out}/${c.name}-${theme}-${width === 390 ? 'phone' : 'desk'}.png`
        await page.locator('main').screenshot({ path: file })
        console.log('ok', file)
      } catch (error) {
        failed++
        console.log('FAIL', c.name, theme, width, error.message)
      }
    }
    await page.close()
  }
}
await browser.close()
process.exit(failed ? 1 : 0)
