// The command center's number format, big answers and column health, in
// Node (tests/ui.rs runs it when node is on the PATH): the modules load
// with a little of a browser's globals, and their pure parts are checked.

import assert from "node:assert/strict";

const noop = () => {};
const el = () => ({ style: {}, dataset: {}, classList: { add: noop, remove: noop, toggle: noop }, append: noop, setAttribute: noop, addEventListener: noop, replaceChildren: noop, children: [] });
Object.assign(globalThis, {
  window: globalThis,
  location: { host: "x", pathname: "/", hash: "", search: "", href: "http://x/" },
  matchMedia: () => ({ matches: false, addEventListener: noop }),
  sessionStorage: { getItem: () => null, setItem: noop, removeItem: noop },
  localStorage: { getItem: () => null, setItem: noop, removeItem: noop },
  document: { addEventListener: noop, querySelector: () => null, querySelectorAll: () => [], documentElement: el(), body: el(), createElement: el, createElementNS: el, createTextNode: el },
});
const app = new URL("../ui/app/", import.meta.url);
const { fmt, ApiError } = await import(new URL("core.js", app));
const big = await import(new URL("bigresult.js", app));
const { flags } = await import(new URL("health.js", app));

// One format for a value of the data: the grid, a chart's tip, Sheets.
assert.equal(fmt.num(226159459.83), (226159459.83).toLocaleString(undefined, { maximumFractionDigits: 2 }));
assert.equal(fmt.num(4.1271186440678), (4.127119).toLocaleString(undefined, { maximumFractionDigits: 6 }));
assert.equal(fmt.num(0.00001234), "1.234e-5");
assert.equal(fmt.num(null), "–");
// Round ticks, labelled short: not 991.7M, 1.5B.
assert.deepEqual(fmt.ticks(0, 1983364195.78, 4), [0, 5e8, 1e9, 1.5e9, 2e9]);
assert.deepEqual(fmt.ticks(0, 1983364195.78, 4).map(fmt.tick), ["0", "500M", "1B", "1.5B", "2B"]);
assert.deepEqual(fmt.ticks(-3, 7.3, 4), [-5, 0, 5, 10]);

// The server's refusal of an answer for its size, in its two wordings.
assert.ok(big.tooBig(new ApiError(400, "query error: the query matches more than 10000 rows, the most a query returns; add a LIMIT (or read every row a page at a time: a scroll, page_size)")));
assert.ok(big.tooBig(new ApiError(400, "query error: LIMIT and OFFSET reach row 20000; a query returns at most 10000 rows")));
assert.ok(!big.tooBig(new ApiError(400, "unknown column x")));
assert.equal(big.SHOWN, 10000);
assert.equal(big.wholeTable("SELECT * FROM trips"), "trips");
assert.equal(big.wholeTable('select * from "my trips";'), "my trips");
assert.equal(big.wholeTable("SELECT * FROM trips WHERE fare > 3"), null);

// A column's flags: always NULL, mostly NULL, one value, a sum a few rows make.
assert.deepEqual(flags({ n: 50, nonNull: 0 }), [["bad", "always NULL"]]);
assert.deepEqual(flags({ n: 100, nonNull: 100, distinct: 1 }), [["warn", "one value"]]);
assert.deepEqual(flags({ n: 1000, nonNull: 1000, distinct: 900, sum: 100, top: 10, topSum: 80 }).map(([, t]) => t), ["10 rows make 80% of the sum"]);
assert.deepEqual(flags({ n: 1000, nonNull: 1000, distinct: 900, sum: 100, top: 10, topSum: 20 }), []);
console.log("ok");
