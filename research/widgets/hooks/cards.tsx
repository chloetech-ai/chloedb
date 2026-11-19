// How each widget draws as a card under its tool call, and in the live
// pane: the terminal gets text art, the other surfaces an SVG chart or map.
import type { Elements, RenderElement } from 'claude-code'

import type { Card, CartLine, Findings, Flight, Instant, Made, Shop, Stock } from '../types'
import { bars, brailleMap, flightSvg, stockSvg } from './charts'

// The elements every surface has, and Svg where the surface has it.
export type Kit = Pick<Elements['desktop'], 'Box' | 'Text' | 'Button' | 'Link'> & { Svg?: Elements['desktop']['Svg'] }

export type Actions = {
  isPinned: boolean
  pin: () => void
  unpin: () => void
  ask: (text: string) => void
  add: (id: number) => void
  save: () => void
}

export type Place = {
  columns: number
  compact: boolean
  cart: CartLine[]
  now: number
}

const money = (value: number, currency = 'USD') => {
  const digits = Math.abs(value) >= 1000 ? 0 : 2
  const text = value.toLocaleString('en-US', { minimumFractionDigits: digits, maximumFractionDigits: digits })
  return currency === 'USD' ? `$${text}` : `${text} ${currency}`
}
const signed = (value: number, digits = 2) => `${value >= 0 ? '+' : ''}${value.toFixed(digits)}`
const ago = (ms: number, now: number) => {
  const s = Math.max(0, Math.round((now - ms) / 1000))
  return s < 60 ? `${s}s ago` : s < 3600 ? `${Math.round(s / 60)} min ago` : `${Math.round(s / 3600)} h ago`
}

export function title(card: Card): string {
  const d = card.data
  if (d.instant) return d.instant.name
  if (d.widget === 'stock') return `${d.symbol} ${money(d.price, d.currency)}`
  if (d.widget === 'flight') return d.callsign
  if (d.widget === 'findings') return `Found: ${d.query}`
  return d.mode === 'cart' ? 'Cart' : `Shop: ${d.query}`
}

export function drawCard(t: Kit, card: Card, place: Place, act: Actions): RenderElement {
  const d = card.data
  const body =
    d.widget === 'stock'
      ? stock(t, d, place, act)
      : d.widget === 'flight'
        ? flight(t, d, place, act)
        : d.widget === 'shop'
          ? shop(t, d, place, act)
          : d.widget === 'findings'
            ? findings(t, d, place, act)
            : made(t, d, place)
  if (!d.instant) return body
  const { Box } = t
  return (
    <Box flexDirection="column">
      {appBar(t, d.instant, place, act)}
      {body}
    </Box>
  )
}

const CHIP: { [how: string]: string } = { made: 'new app', recalled: 'from your apps', opened: 'opened again', changed: 'changed', saved: 'saved' }

// An instant app's line above its card: its name, how it came, and Save
// while it is a draft. Claude Code draws no HTML, so a made app shows here
// as the card of its data; Claude Desktop and claude.ai draw the app itself.
function appBar(t: Kit, app: Instant, place: Place, act: Actions): RenderElement {
  const { Box, Text, Button } = t
  const how = app.how
  return (
    <Box justifyContent="space-between" gap={1}>
      <Text>
        <Text bold color="suggestion">
          {app.name}
        </Text>{' '}
        <Text dimColor>
          {CHIP[how] ?? how}
          {app.version > 1 ? ` · v${app.version}` : ''}
        </Text>
      </Text>
      {!app.saved && !place.compact && <Button key="save" label="Save to your apps" onPress={act.save} />}
    </Box>
  )
}

// What a made app holds, when no card here knows its data: its sentence,
// and the first list in it, a line for each of the first few.
function made(t: Kit, d: Made, place: Place): RenderElement {
  const { Box, Text } = t
  const list = Object.values(d).find(v => Array.isArray(v) && v.length && typeof v[0] === 'object') as { [k: string]: unknown }[] | undefined
  const width = Math.max(20, place.columns - 4)
  const line = (row: { [k: string]: unknown }) => {
    const name = ['title', 'name', 'label'].map(k => row[k]).find(v => typeof v === 'string') as string | undefined
    const rest = Object.entries(row)
      .filter(([k, v]) => (typeof v === 'number' || (typeof v === 'string' && v.length < 24)) && !['id', 'title', 'name', 'label', 'thumbnail', 'image'].includes(k) && !/^https?:/.test(String(v)))
      .slice(0, 3)
      .map(([k, v]) => `${k} ${v}`)
      .join(' · ')
    const text = `${name ?? '?'}${rest ? `  ${rest}` : ''}`
    return text.length > width ? `${text.slice(0, width - 1)}…` : text
  }
  return (
    <Box flexDirection="column">
      <Text>{d.summary}</Text>
      {(list ?? []).slice(0, place.compact ? 3 : 6).map((row, i) => (
        <Text key={`row-${i}`}>
          <Text dimColor>{i + 1} </Text>
          {line(row)}
        </Text>
      ))}
    </Box>
  )
}

// What Claude found: the request, what the documents agree on, and each
// document's title (a link where it has an address), where and when it is
// from, what it says and its first passage. Ask about one from its button.
function findings(t: Kit, d: Findings, place: Place, act: Actions): RenderElement {
  const { Box, Text, Button, Link } = t
  const width = Math.max(24, place.columns - 6)
  const cut = (text: string, n: number) => (text.length > n ? `${text.slice(0, n - 1)}…` : text)
  const docs = d.documents.slice(0, place.compact ? 3 : 6)
  if (!d.documents.length) return <Text dimColor>Nothing found for “{d.query}” yet.</Text>
  return (
    <Box flexDirection="column">
      <Text>
        <Text bold>{d.query}</Text>
        <Text dimColor>
          {'  '}
          {d.documents.length} source{d.documents.length > 1 ? 's' : ''}, found by Claude
        </Text>
      </Text>
      {d.notes && !place.compact && <Text italic>{cut(d.notes, width * 2)}</Text>}
      {docs.map(doc => (
        <Box key={`doc-${doc.n}`} flexDirection="column" marginTop={place.compact ? 0 : 1}>
          <Text>
            <Text dimColor>{doc.n} </Text>
            {doc.url ? <Link href={doc.url} label={cut(doc.title, width)} /> : <Text bold>{cut(doc.title, width)}</Text>}
          </Text>
          {[doc.source, doc.date, doc.kind].some(Boolean) && <Text dimColor>{'  '}{[doc.source, doc.date, doc.kind].filter(Boolean).join(' · ')}</Text>}
          {!place.compact && <Text>{'  '}{cut(doc.summary, width * 2)}</Text>}
          {!place.compact && doc.quotes[0] && <Text dimColor>{'  '}“{cut(doc.quotes[0], width * 2)}”</Text>}
        </Box>
      ))}
      {d.documents.length > docs.length && <Text dimColor>and {d.documents.length - docs.length} more</Text>}
      {!place.compact && (
        <Box gap={1} marginTop={1}>
          <Button key="compare" label="Compare them" onPress={() => act.ask(`Compare what these sources say about ${d.query}.`)} />
          <Button key="more" label="Find more" onPress={() => act.ask(`Find more sources on ${d.query}.`)} />
        </Box>
      )}
    </Box>
  )
}

function pinButton(t: Kit, act: Actions, label: string): RenderElement {
  const { Button } = t
  return act.isPinned ? (
    <Button key="unpin" label="Unpin" onPress={act.unpin} />
  ) : (
    <Button key="pin" label={label} onPress={act.pin} />
  )
}

function stock(t: Kit, d: Stock, place: Place, act: Actions): RenderElement {
  const { Box, Text, Button, Svg } = t
  const isUp = d.change >= 0
  const color = isUp ? 'success' : 'error'
  const width = Math.max(20, Math.min(72, place.columns - 8))
  const closes = d.points.map(p => p[1])
  const when = d.range === '1d' ? 'today' : `over ${d.range}`
  return (
    <Box flexDirection="column">
      <Box justifyContent="space-between">
        <Text bold>
          {d.name} <Text dimColor>{d.symbol}{d.exchange ? ` · ${d.exchange}` : ''}</Text>
        </Text>
        <Text dimColor>{d.range.toUpperCase()}</Text>
      </Box>
      <Text>
        <Text bold>{money(d.price, d.currency)}</Text>{' '}
        <Text color={color}>
          {isUp ? '▲' : '▼'} {signed(d.change)} ({signed(d.changePct)}%)
        </Text>{' '}
        <Text dimColor>{when}</Text>
      </Text>
      {Svg ? (
        <Svg
          source={stockSvg(d.points, d.range === '1d' ? d.base : null, isUp, 560, place.compact ? 70 : 120)}
          alt={`${d.symbol} ${when}: from ${money(closes[0] ?? d.base, d.currency)} to ${money(d.price, d.currency)}`}
          width={Math.min(560, Math.max(240, place.columns * 7))}
        />
      ) : (
        <Box flexDirection="column">
          {bars(closes, width, place.compact ? 2 : 3).map(line => (
            <Text color={color}>{line}</Text>
          ))}
        </Box>
      )}
      {!place.compact && (
        <Text dimColor>
          low {money(d.low, d.currency)} · high {money(d.high, d.currency)} · {d.range === '1d' ? 'prev close' : 'start'}{' '}
          {money(d.base, d.currency)}
        </Text>
      )}
      <Box justifyContent="space-between" flexWrap="wrap">
        <Text dimColor>
          {d.source} · {ago(d.asOf, place.now)}
        </Text>
        <Box gap={1}>
          {pinButton(t, act, 'Pin live')}
          {!place.compact && (
            <Button key="ask" label="Why did it move?" onPress={() => act.ask(`Why did ${d.symbol} move ${when}?`)} />
          )}
        </Box>
      </Box>
    </Box>
  )
}

function flight(t: Kit, d: Flight, place: Place, act: Actions): RenderElement {
  const { Box, Text, Button, Svg } = t
  if (!d.found || d.lat == null || d.lon == null) {
    return (
      <Box flexDirection="column">
        <Text bold>
          {d.callsign}
          {d.airline ? <Text dimColor> · {d.airline}</Text> : null}
        </Text>
        <Text>{d.unavailable ? `Can't reach live positions right now. ${d.note ?? ''}` : 'Not in the air right now.'}</Text>
        {d.airborne && d.airborne.length > 0 && <Text dimColor>In the air now for that airline: {d.airborne.join(', ')}</Text>}
        <Text dimColor>{d.source}</Text>
      </Box>
    )
  }
  const heading = d.heading ?? 0
  const climb = (d.climbFpm ?? 0) > 300 ? 'climbing' : (d.climbFpm ?? 0) < -300 ? 'descending' : 'level'
  const cols = Math.max(24, Math.min(64, place.columns - 8))
  return (
    <Box flexDirection="column">
      <Box justifyContent="space-between">
        <Text bold>
          {d.callsign}
          {d.airline ? <Text dimColor> · {d.airline}</Text> : null}
        </Text>
        <Text color={d.onGround ? 'warning' : 'success'}>{d.onGround ? 'On the ground' : 'In the air'}</Text>
      </Box>
      <Text dimColor>
        over {d.over} · {d.lat.toFixed(2)}, {d.lon.toFixed(2)}
      </Text>
      {Svg ? (
        <Svg
          source={flightSvg(d.lat, d.lon, heading, d.trail ?? [], 560, place.compact ? 140 : 210)}
          alt={`${d.callsign} over ${d.over}, heading ${heading} degrees`}
          width={Math.min(560, Math.max(240, place.columns * 7))}
        />
      ) : (
        <Box flexDirection="column">
          {brailleMap(d.lat, d.lon, heading, cols, place.compact ? 5 : 8).map(line =>
            line.plane == null ? (
              <Text dimColor>{line.text}</Text>
            ) : (
              <Text>
                <Text dimColor>{line.text.slice(0, line.plane)}</Text>
                <Text color="suggestion" bold>
                  {line.text.slice(line.plane, line.plane + 1)}
                </Text>
                <Text dimColor>{line.text.slice(line.plane + 1)}</Text>
              </Text>
            ),
          )}
        </Box>
      )}
      <Text>
        {(d.altitudeFt ?? 0).toLocaleString('en-US')} ft · {d.speedKts} kt · heading {heading}° · {climb}
      </Text>
      <Box justifyContent="space-between" flexWrap="wrap">
        <Text dimColor>
          {d.source} · {ago(d.asOf, place.now)}
        </Text>
        <Box gap={1}>
          {pinButton(t, act, 'Follow live')}
          {!place.compact && (
            <Button key="ask" label="When does it land?" onPress={() => act.ask(`When does ${d.callsign} land, and is it on time?`)} />
          )}
        </Box>
      </Box>
    </Box>
  )
}

function shop(t: Kit, d: Shop, place: Place, act: Actions): RenderElement {
  const { Box, Text, Button } = t
  const cart = place.cart
  const total = cart.reduce((sum, l) => sum + l.price * l.qty, 0)
  const nameWidth = Math.max(14, Math.min(40, place.columns - 34))
  const cut = (text: string) => (text.length > nameWidth ? `${text.slice(0, nameWidth - 1)}…` : text.padEnd(nameWidth))
  return (
    <Box flexDirection="column">
      {d.mode === 'cart' ? (
        <Text bold>Your cart</Text>
      ) : (
        <Text bold>
          {d.query}
          {d.maxPrice ? ` under ${money(d.maxPrice).replace(/\.00$/, '')}` : ''}{' '}
          <Text dimColor>
            {d.total} result{d.total === 1 ? '' : 's'} · {d.source}
          </Text>
        </Text>
      )}
      {d.items.slice(0, place.compact ? 3 : 6).map((item, i) => (
        <Box key={`item-${item.id}`} justifyContent="space-between" gap={1}>
          <Text>
            <Text dimColor>{i + 1} </Text>
            {cut(item.title)} <Text bold>{money(item.price).padStart(9)}</Text> <Text color="warning">★{item.rating.toFixed(1)}</Text>
          </Text>
          <Button key={`add-${item.id}`} label={cart.some(l => l.id === item.id) ? 'Add another' : 'Add'} onPress={() => act.add(item.id)} />
        </Box>
      ))}
      {cart.length > 0 ? (
        <Text>
          <Text bold>Cart</Text> {cart.map(l => `${l.qty} × ${l.title}`).join(', ')} · <Text bold>{money(total)}</Text>{' '}
          <Text dimColor>(demo store: nothing is ordered)</Text>
        </Text>
      ) : (
        d.mode === 'cart' && <Text dimColor>The cart is empty.</Text>
      )}
    </Box>
  )
}
