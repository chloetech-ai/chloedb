# Instant apps

A prototype of answers that come with apps. You ask in a chat; when the
answer is better as an app than as text, one opens in the conversation: a
stock chart, products side by side, restaurants with free tables, dishes to
cook. You can change it by asking ("put the best rated first"), and save it to
a rail of your apps beside the chat. From then on it opens by itself when a
later request needs it, with that request's inputs.

Apps serve three kinds of need, and the rail groups them so:

- **Look things up** (information): a stock chart, where a flight is, products compared.
- **Get things done** (actions): book a table, buy something.
- **Discover** (inspiration): dishes to cook, things to browse.

## How a request is answered

1. **Recall.** The apps are rows in ChloeDB (`chat/store.mjs`, table
   `ia_apps`). The request is searched against the saved ones by words
   (BM25 over each app's name, what it does and the requests it served) and
   by meaning (a static embedding of the same text, `potion-retrieval-32M`),
   fused by rank (`rrf`).
2. **Route.** The model (`chat/model.mjs`) reads the request, the recent
   conversation, the best matches and the tools, and decides: open a saved
   app with this request's inputs, make a new app, or just answer in text.
3. **Fill.** The app's tool runs (`server/widgets/*.mjs`): live stock prices
   (Yahoo Finance), live flights (OpenSky Network), a demo store (DummyJSON),
   recipes (TheMealDB), and a made-up reservations service.
4. **Make, if needed.** The model writes the app as an HTML fragment against
   a small kit (`server/views/kit.js`), shown its first tool's real data.
   That takes 30 to 60 seconds; a saved app opens in about 5.
5. **Show.** The page draws the app in a sandboxed iframe through the
   MCP Apps SDK's `AppBridge`, with a content security policy that lets it
   load nothing and reach nothing but its own tools, through the chat.
6. **Keep.** A made app is a draft until saved. Each change is a new version
   (`ia_versions`), undo goes back, and each request an app serves is added
   to what it is recalled by, so recall gets better with use.

Every app, made or written by hand, is an MCP App: a view filled by a tool of
the MCP server in `server/`. The same server works in any host that draws MCP
Apps (Claude on the web and Claude Desktop, VS Code), and in Claude Code
through the plugin in this folder.

## Try the chat

It needs Node 20 or later, Claude Code signed in (`claude auth status`), and
chloedb-server with its small embedding model:

```bash
cd chloedb-server && cargo build --release && cd ..        # or set CHLOEDB_SERVER_BIN to a chloedb-server
chloedb-server/target/release/chloedb-server fetch-model potion-retrieval-32M-int8
cd research/widgets/server && npm install && cd ..
node chat/server.mjs                                       # http://127.0.0.1:8800/
```

The apps are kept in `~/.local/share/instant-apps` (`--data DIR` for
another). The first start saves three apps written by hand (Stock chart,
Flight tracker, Shop), so the rail has something in it. Then try:

- "How is Apple doing this month?" opens the saved stock chart.
- "Book a table for 4 on Friday at 8 in North Beach" makes a booking app;
  pick a time to book (a demo: nothing is booked), ask for a change, save it.
- "Can you get me a table for two tomorrow in the Mission?" brings it back.
- "Dinner ideas, something Thai" makes a recipe browser.

Settings: `WIDGETS_MAKE_MODEL` (the model that writes apps, default
`claude-opus-5-5`), `WIDGETS_FAST_MODEL` (routes and answers, default
`claude-sonnet-5-5`), `WIDGETS_CLAUDE` (the claude binary), `CHLOEDB_URL`
(a running chloedb-server instead of starting one), `WIDGETS_SIMULATE=1`
(made-up data for every tool, no network). In a container behind a proxy,
Node's `fetch` needs `NODE_USE_ENV_PROXY=1`.

## The same apps elsewhere

- **Claude Code**: `claude --plugin-dir research/widgets` (after the
  `npm install` above). Claude Code draws no HTML, so the plugin's mod
  (`hooks/`) draws the stock, flight and shop widgets as native cards under
  the tool call, on the terminal, the desktop app, VS Code and the phone, and
  a Pin keeps one live in a pane. The model reads one sentence, not the data.
- **Claude Desktop**: add the server to `claude_desktop_config.json` as
  `{"command": "node", "args": ["/path/to/research/widgets/server/index.mjs"]}`.
- **claude.ai**: `node server/index.mjs --http 8787` serves Streamable HTTP at
  `/mcp`; behind a public HTTPS address it can be added as a custom connector.

Made apps live in the chat for now; serving them from the MCP server too
(one `open_app` tool and a view that runs any saved app) is the next step.

## Tests

```bash
cd research/widgets/server && npm test      # the MCP server, and the chat's flow with a scripted model on a real chloedb-server
claude plugin test research/widgets          # the Claude Code mod on every surface
node test/host/host.mjs                      # a small MCP Apps host to look at the hand-written views
```

## What it is not yet

- The reservations service is made up and the store is DummyJSON's demo:
  nothing is booked, bought or charged, and the apps say so.
- A made app reaches data only through the tools here. A connector per
  source (a real reservations API, a shop's catalog) would give it more.
- One person, one machine: no accounts, and apps are not shared.
