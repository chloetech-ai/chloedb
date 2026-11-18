// How each widget draws as a card under its tool call, and in the live
// pane: the terminal gets text art, the other surfaces an SVG chart or map.
import type { Elements, RenderElement } from 'claude-code'

import type { Card, CartLine, Flight, Shop, Stock } from '../types'
import { bars, brailleMap, flightSvg, stockSvg } from './charts'

// The elements every surface has, and Svg where the surface has it.
export type Kit = Pick<Elements['desktop'], 'Box' | 'Text' | 'Button'> & { Svg?: Elements['desktop']['Svg'] }

export type Actions = {
  isPinned: boolean
  pin: () => void
  unpin: () => void
  ask: (text: string) => void
  add: (id: number) => void
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
  if (d.widget === 'stock') return `${d.symbol} ${money(d.price, d.currency)}`
  if (d.widget === 'flight') return d.callsign
  return d.mode === 'cart' ? 'Cart' : `Shop: ${d.query}`
}

export function drawCard(t: Kit, card: Card, place: Place, act: Actions): RenderElement {
  const d = card.data
  if (d.widget === 'stock') return stock(t, d, place, act)
  if (d.widget === 'flight') return flight(t, d, place, act)
  return shop(t, d, place, act)
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
