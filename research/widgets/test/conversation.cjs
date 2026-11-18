// An ongoing conversation with the widgets in it, recorded: saved apps come
// back, a follow-up updates one, an action in one gets the assistant's reply,
// and a question is answered from what a widget shows. Run it from an empty
// folder against a running chat (URL=, default http://127.0.0.1:8800/); with
// Table booking and Dinner ideas saved, every app opens from the rail.
const { chromium } = require('playwright')
const URL = process.env.URL || 'http://127.0.0.1:8800/'
const log = (...a) => console.log(new Date().toISOString().slice(11, 19), ...a)

;(async () => {
  const b = await chromium.launch()
  const ctx = await b.newContext({ viewport: { width: 1280, height: 820 }, deviceScaleFactor: 1, recordVideo: { dir: 'video', size: { width: 1280, height: 820 } } })
  const p = await ctx.newPage()
  p.on('pageerror', e => log('pageerror', e.message))
  p.on('console', m => { if (m.type() === 'error') log('console', m.text().slice(0, 200)) })
  await p.goto(URL + '?theme=light')
  await p.waitForSelector('.app')
  await p.waitForTimeout(1200)
  const count = type => p.evaluate(t => window.events.filter(e => e.type === t).length, type)
  let n = 0
  const shot = async name => {
    n++
    await p.screenshot({ path: `c${n}-${name}.png` })
  }

  async function say(text, name) {
    const done = await count('done')
    log('say', text)
    await p.locator('#text').click()
    await p.locator('#text').pressSequentially(text, { delay: 14 })
    await p.waitForTimeout(300)
    await p.keyboard.press('Enter')
    await p.waitForFunction(d => window.events.filter(e => e.type === 'done').length > d, done, { timeout: 300000 })
    await p.waitForTimeout(2500)
    const evs = await p.evaluate(() => window.events.slice(-4).map(e => ({ ...e, html: undefined, result: undefined, candidates: undefined, app: e.app?.name })))
    for (const e of evs) if (e.type !== 'status' && e.type !== 'done') log(' ', JSON.stringify(e).slice(0, 300))
    await shot(name)
  }

  // Clicks in the newest widget: a button, then a confirming one if the app asks.
  async function act(button, name, confirm, nth = 0) {
    const notes = await count('note')
    const frame = p.locator('.widget:not(.folded)').last().frameLocator('iframe')
    const target = frame.getByRole('button', { name: button }).nth(nth)
    await target.scrollIntoViewIfNeeded()
    await p.waitForTimeout(800)
    log('click', String(button))
    await target.click()
    if (confirm) {
      await p.waitForTimeout(1200)
      const yes = frame.getByRole('button', { name: confirm }).first()
      if (await yes.count()) {
        await yes.scrollIntoViewIfNeeded()
        log('click', String(confirm))
        await yes.click()
      }
    }
    await p.waitForFunction(k => window.events.filter(e => e.type === 'note').length > k, notes, { timeout: 120000 })
    await p.waitForTimeout(2500)
    log('  note:', await p.evaluate(() => window.events.filter(e => e.type === 'note').at(-1).text))
    await shot(name)
  }

  await say('My parents are visiting this weekend. Can you find us a table for 4 on Friday around 7 in North Beach?', 'table')
  await say("My dad doesn't like spicy food, and 7:30 would work better for them", 'updated')
  await act(/7:30/, 'booked', /book|reserve|confirm/i)
  await say('Perfect. Any ideas for something Italian I could cook for them on Saturday?', 'cook')
  await say('Which of those is the quickest to make?', 'quickest')
  await say('And a small gift for my mom under $50? Something for her kitchen', 'gift')
  await act(/Add to cart/, 'added', null, 2)
  await p.waitForTimeout(1500)
  // The whole conversation in one picture: the page grows to fit it.
  await p.addStyleTag({ content: '.shell{height:auto!important;grid-template-rows:auto!important}.log{overflow:visible!important}.rail{align-self:start;position:sticky;top:0}' })
  // Chromium paints a sandboxed frame only inside the viewport, so the viewport grows to the page.
  const tall = await p.evaluate(() => document.documentElement.scrollHeight)
  await p.setViewportSize({ width: 1280, height: tall })
  await p.waitForTimeout(2500)
  await p.screenshot({ path: 'whole-conversation.png' })
  await ctx.close()
  await b.close()
  log('video', require('fs').readdirSync('video').join(' '))
})().catch(e => { log('failed', e); process.exit(1) })
