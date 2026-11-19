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

1. **Recall.** The apps are rows in ChloeDB (`server/store.mjs`, table
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

## Widgets in an ongoing conversation

An app is a widget inside the assistant's reply, not a page of its own: a
line says what was done ("Opened Budget from your apps · sent: add lunch
4.8", "Made a new app", "Filled Stock chart again"), never what was meant to
come of it; the widget has a bar with its name, Save, Change, Undo change
and Redo; and the written answer follows it, written from what the app
shows on screen (`POST /api/say`), and marked as said about an earlier
version once the app changes in place. The conversation and the widgets keep each
other informed:

- **The assistant sees what the widgets show.** Each turn sends, for the
  newest widgets, what each shows (its first call's summary), what was done
  in it ("Booked Maison Claire for 4 at 19:30") and what the app told the
  model itself (`Kit.tell(app, text)`, the SDK's `updateModelContext`). So
  "which of those is the quickest?" is answered in text from the dishes the
  widget shows, without opening anything.
- **A follow-up updates the widget.** "7:30 would work better" after a booking
  app opens the same app again with the new inputs, marked Updated, and the
  older widget of that app folds to its bar ("updated below").
- **An app keeps what is typed into it, in this browser.** `Kit.keep` and
  `Kit.kept` store values per app and per browser (`ia_state`), and each
  draw gets all of them as `data.kept`, so an app starts from them and the
  values a request names win over kept ones.
- **Entries go in by asking.** "Add lunch 4,80 to my budget" opens the
  budget app with `values.add` (each entry with an id, so it is added once)
  and "the groceries were 32.40" with `values.edit`; the app shows whether
  it took them, and the answer says so.
- **A change is made to what that widget shows.** Change and Undo use the
  widget's own data, and a change to one web answer is made to a copy of
  it, kept in the person's apps, so other answers stay as they were.
- **Conversations come back.** Each browser's conversations are kept
  (`ia_talk`, `ia_shown`); New chat asks first, and Earlier chats opens one
  from before.
- **An action in a widget gets a reply.** A tool marked `acts: true` (booking
  a table, adding to the cart) is done by the person, so after it the
  assistant writes a sentence or two in the chat (`POST /api/note`): what
  happened in their terms, and one next step.

`test/conversation.cjs` plays such a conversation in Chromium against a
running chat and records it (a video, a picture after each turn and one of the
whole conversation, in the folder it is run from, and the seconds into the
video where an app was being made, to cut it): a table found, moved to 7:30
and booked, dinner ideas, a question answered from the widget, a gift added
to a cart, then two products put side by side, which no saved app does, so a
Product comparison is made on the fly, saved to the rail and asked about. With
the other apps saved, it takes about two minutes, half a minute of it making
the new app.

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

## Inside Claude

The MCP server (`server/`) does what the chat does, with the Claude of the
conversation as the model: Claude Desktop, claude.ai and Claude Code make,
save and reopen apps through it, and an app made in one opens in the others.

| tool | what it does |
|---|---|
| `apps_find` | searches the saved apps for the request (words and meaning, as above); the server's instructions tell Claude to call it first when an app would answer better than text |
| `app_open` | opens a saved app with this request's arguments |
| `app_kit` | the kit's guide, the tools an app may call and a sample of its first tool's data, for Claude to write a new app from |
| `app_make` | shows the fragment Claude wrote, as a draft |
| `app_change`, `app_undo` | a new version of an app, and back |
| `app_save` | keeps an app in the person's apps (its Save button calls it too), or takes it out |
| `my_apps` | the saved apps, grouped as the rail groups them; clicking one asks Claude to open it |
| `app_source`, `app_call` | for the view only (`visibility: ['app']`): an app's source, and its calls, which run only the tools it was made with |

Every app draws in one view, `ui://widgets/app.html`: the tool result names
the app (`structuredContent.instant`), the view fetches its source, mounts it
and hands it the data, with a bar above it (its name, how it came, Save and
Change). Your apps is `ui://widgets/apps.html`. The apps are rows in ChloeDB,
in `~/.local/share/instant-apps` (`WIDGETS_DATA`), the folder the chat uses
too. Each Claude session starts its own MCP server, and they share one
chloedb-server per folder (`server.json` there names it; the first to start
starts it). It is this checkout's build, one on the `PATH`, or
`CHLOEDB_SERVER_BIN`.

- **Claude Desktop**: add the server to `claude_desktop_config.json` as
  `{"command": "node", "args": ["/path/to/research/widgets/server/index.mjs"]}`.
- **claude.ai**: `node server/index.mjs --http 8787` serves Streamable HTTP at
  `/mcp`; behind a public HTTPS address it can be added as a custom connector.
- **Claude Code**: `claude --plugin-dir research/widgets` (after the
  `npm install` above). Claude Code draws no HTML, so the plugin's mod
  (`hooks/`) draws each app as the card of its data under the tool call, with
  a line above it (its name, how it came and Save), on the terminal, the
  desktop app, VS Code and the phone; data no card knows (recipes, tables)
  shows as a list. `/apps`, or Claude's `my_apps`, opens Your apps in a pane,
  and each app's Open asks for it. A Pin keeps a stock or a flight live in a
  pane. The model reads the server's sentences, not the data.

A run with Claude Code (`claude -p --plugin-dir research/widgets`, a fresh
folder, four sessions one after the other, 2026-11-18): asked to put two
blenders side by side and save it, Claude called `apps_find` (no fit),
`app_kit`, `app_make` (a 6 KB comparison table) and `app_save`. A new
session asked to compare two headphones called `apps_find`, which ranked the
saved comparison first, and `app_open` with `{"query": "headphones"}`; a
third did the same for smartphones, and "Which apps have I saved?" called
`my_apps`.

### What Claude found

What Claude finds itself (a web search, pages or files it read) fills an app
too, shaped to what the person wants rather than as a list of pages. For "the
best headphones to buy" that is the headphones themselves: the bottom line,
which one for which need, each one's facts, pros and cons and what each
source says about it, with the pages behind every claim. A question gets its
answer and the points behind it, a how-to its steps, news its events.

`show_findings` is the tool Claude calls with it, typed so its host checks
the shape (`server/widgets/findings.mjs`: `query`, `goal` (pick, answer,
overview, how-to, news), `answer` with the documents it cites, `needs`,
`criteria`, `items` with `facts`, `pros`, `cons` and `says` per source, and
the numbered `documents`). It opens the person's app for what Claude found
that fits the request, the saved **Search results** unless they made
another on the tool `findings`, which `app_open` and `app_make` also take as
arguments. The server keeps only web addresses and drops a citation of a
source that is not there. Search results (`server/views/findings.html`)
shows the bottom line with its sources, a button per need that picks out its
option, the options as cards or side by side, "What matters to you?", which
asks Claude to pick again for that, and the sources, which every citation
opens to. The server's instructions tell Claude to call it after finding
things, and in Claude Code the mod adds a line after each WebSearch and
WebFetch result saying how, which Claude follows (without it Claude answered
in text); it draws the result as a card with the pick per need, the options
and the sources as links.

A run (Claude Code, `claude -p --plugin-dir research/widgets` with WebSearch
allowed, 2026-11-19): asked "what are the best noise cancelling headphones
to buy right now?", Claude searched three times (the guides, a test of the
top three, their prices) and called `show_findings` with six headphones, a
pick for five needs and ten sources, RTINGS' measured attenuation among the
facts; 65 s, $0.63.

## On instant.chloetech.ai

The chat runs for friends at https://instant.chloetech.ai, behind a password
that the site's Caddy checks (namespace `web`). `deploy/k8s.yaml` is the pod:
the chat, fetched from this branch with Claude Code at start, beside its own
chloedb-server, on the `protos` node, with the apps on the node's disk. Claude
is capped at $20 a day and $100 in all (`/healthz` says what was spent), 40
requests a visitor an hour and 3 calls at once. Everyone who signs in shares
the same saved apps.

A request to find or research something is routed to `findings`: the chat
first has Claude look it up (`claude -p` on the fast model, under the same
caps, with WebSearch and `server/reader.mjs`, an MCP server that reads public
pages: http(s) on the usual ports, every address the name resolves to
public, the connection pinned to the address checked, each redirect checked
again; Claude Code's own WebFetch would reach the chloedb-server beside the
chat and the cluster's services). It searches up to four times, reads three
or four pages for the facts, and returns the shape above, which fills Search
results. A follow-up to it ("I fly a lot") starts from what was found. One
pick took 28 s and $0.11.

## Tests

```bash
cd research/widgets/server && npm test      # the MCP server and its app tools, and the chat's flow with a scripted model, on a real chloedb-server
claude plugin test research/widgets          # the Claude Code mod on every surface
node test/host/host.mjs                      # a small MCP Apps host to look at the views: ?tool=app_open&args={"id":"stock-chart"}
(mkdir -p rec && cd rec && node ../test/conversation.cjs)   # record a conversation against the running chat (real models)
```

## What it is not yet

- The reservations service is made up and the store is DummyJSON's demo:
  nothing is booked, bought or charged, and the apps say so.
- A made app reaches data only through the tools here and what Claude hands
  it (`findings`). A connector per source (a real reservations API, a shop's
  catalog) would give it more.
- What Claude found is kept as it was found: Search results opened later
  shows the same documents until Claude looks again.
- One person, one machine: no accounts, and apps are not shared.
