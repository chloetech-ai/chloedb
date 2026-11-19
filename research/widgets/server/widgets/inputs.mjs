// The app's own inputs: for an app that works everything out in the page
// (a calculator, a function plotted with its derivative, a converter, a
// simulation, a game), which needs no data from anywhere. Its data is what
// the request gives it, by name, and the app computes the rest; a later
// request fills it again with new values ("now do cos(x)").
//
// Like findings, no model calls it as a tool of its own: app_open and
// app_make take its arguments, and this only checks and passes them on.
import { z } from 'zod'

const VALUE = z.union([z.string().max(2000), z.number(), z.boolean(), z.null()])

export const INPUT = {
  request: z.string().min(1).max(500).describe('What the person asked for, in a few words'),
  values: z
    .record(z.string().max(60), VALUE)
    .optional()
    .describe('The values the request gives, by name, for the app to start from: {"f": "x^2 sin(x)"}, {"amount": 600000, "rate": 6}; {} when it gives none'),
}

export const inputs = {
  name: 'inputs',
  title: 'The app’s own inputs',
  view: null,
  given: true,
  domains: [],
  description:
    'Not a source: the starting values for an app that works everything out itself in the page and needs no data from anywhere (a calculator, a ' +
    'function plotted with its derivative or integral, a unit or loan calculator, a simulation, a timer, a small game). Give the request and the ' +
    'values it names; the app computes the rest and lets the person change them.',
  input: INPUT,
  example: { request: 'animate the derivative of x^2 sin(x)', values: { f: 'x^2 sin(x)' } },
  async run({ request, values = {} }) {
    return { widget: 'inputs', request, values }
  },
  summary(data) {
    const named = Object.entries(data.values ?? {}).map(([k, v]) => `${k} = ${v}`)
    return `Opened for ${data.request}${named.length ? `, with ${named.join(', ')}` : ''}.`
  },
}
