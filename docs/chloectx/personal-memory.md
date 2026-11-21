# Personal memory: one memory, many models

ChloeCtx keeps a memory of your own beside the memories of your projects: the chats you had with ChatGPT, Claude.ai and Gemini, and a persona card of what they say about you, each line linked to the chats it came from. It is one ChloeDB file on your machine, and any assistant you connect reads it through the same tools: ChatGPT as a connector, Claude.ai as a custom connector, Claude Code and any MCP client. A slice of it can be shared with a key of its own.

```
 ChatGPT export ─┐                                 ┌─ ChatGPT (connector)
 Claude.ai zip ──┤  chloectx import                ├─ Claude.ai (custom connector)
 Gemini Takeout ─┤ ─────────────┐                  ├─ Claude Code, any MCP client
 browser capture ┤              ▼                  │
 API proxy ──────┘   ┌──────────────────────┐  MCP │
                     │ me: chats + persona  │──────┤  /mcp, OAuth sign-in
                     │ one ChloeDB file     │      │
                     └──────────────────────┘      └─ shares: /s/<key>/mcp
                                                       signed, read-only, expiring
```

Why not a file of notes about yourself? A plain file has no sources (you cannot tell where "vegetarian" came from or whether it is still true), no search (an assistant reads all of it or none), no history (an edit loses what it replaced), and no scope (you share the whole file or nothing). Here every line of the card says which chat it came from, an assistant asks for what bears on its question and gets those records with their chats, a changed line keeps the old one as history, and a share holds only the sections and topics you picked, signed, logged and revocable.

## On a Mac with Claude Desktop

The shortest way in needs no terminal. Each release has `chloectx-claude-desktop-macos.mcpb`, a Claude Desktop extension that holds chloectx and its model. Open it in Claude Desktop (double-click it, or drag it onto Settings, Extensions) and press Install. Then ask Claude:

1. "Import my ChatGPT export." Claude finds the exports in Downloads, Desktop and Documents (or says how to ask for one) and imports the one you pick.
2. "Draft my persona card." Claude reads your own messages a part at a time and proposes lines, each naming the messages it came from. They show as suggested.
3. "Show me the card." Claude goes through the suggested lines with you, and confirms, rewrites or drops each.

It is the same memory as the commands below use, so `chloectx me serve` can later serve it to ChatGPT as well. [Its README](../../chloectx/claude-desktop/README.md) lists its tools and how it is built and checked.

## Import your chats

Ask each service for an export, then give ChloeCtx the file as it came:

| from | where to ask | what you get |
|---|---|---|
| ChatGPT | Settings, Data controls, Export data | a zip with `conversations.json` (and your custom instructions) |
| Claude.ai | Settings, Privacy, Export data | a zip with `conversations.json` and `memories.json` |
| Gemini | Google Takeout, My Activity, Gemini Apps | `MyActivity.json` (pick JSON) or `MyActivity.html` |

```bash
chloectx import ~/Downloads/chatgpt-export.zip --dry-run   # what it holds, nothing written
chloectx import ~/Downloads/chatgpt-export.zip
chloectx import ~/Downloads/claude-export.zip
chloectx import ~/Downloads/takeout/                       # a folder: every export found in it
chloectx me                                                # what the memory holds, by source
```

Each message is kept once, so importing a newer export of the same account adds only what is new. Every record says which assistant and which chat it came from, with the chat's title and address. ChatGPT's custom instructions and Claude.ai's memories come in as chats of their own. Any other source can be written as JSON lines, one message each, and imported with `--from jsonl`:

```json
{"source": "chatgpt", "conversation": "c-1", "title": "Kyoto trip", "role": "user", "text": "I'm vegetarian.", "ts": 1763600000000}
```

The memory is the `me` folder in ChloeCtx's data folder (`~/.local/share/chloectx/me`). The usual commands read it with `--me`: `chloectx --me search kyoto`, `chloectx --me ask "what do I eat?"`.

## The persona card

The card is what an assistant reads first: short lines about you, in sections (About me, Work, Health & diet, Travel, and so on), each with the chats it came from.

```bash
chloectx me card build            # drafts the card from your own messages with a model (claude -p, --model sonnet)
chloectx me card                  # the card, each line with its source chats
chloectx me card edit             # in $EDITOR; a line you change or keep becomes yours
chloectx me card add "Health & diet" "Vegetarian; avoids mushrooms."
chloectx me card confirm <id>     # make a drafted line yours
chloectx me card drop <id>        # take it off (kept as history)
```

`build` reads only what you wrote, never the assistants' answers, and keeps a drafted line only when it names the messages it came from. Building again replaces the drafted lines and keeps every line you wrote or confirmed. A line an assistant adds through `remember` shows as suggested, by that assistant, until you confirm it.

The assistant you connect can draft it instead, with no model of ChloeCtx's own: `persona_draft` gives it your messages a part at a time with the same rules, and `persona_suggest` adds the lines it proposes, each with the ids of the messages it came from, as suggested. A line that names no message of yours is left out. `persona_review` confirms, changes or drops lines as you tell it.

## Connect ChatGPT, Claude.ai and other assistants

`chloectx me serve` serves the memory over MCP with OAuth sign-in: the assistant registers itself, sends you to a consent page that asks for your passcode, and gets a token for 24 hours (renewed for 90 days). The passcode and a capture key are made and printed once on the first run; `chloectx me connect --new-passcode` makes a new one.

```bash
chloectx me serve                                    # http://127.0.0.1:7781
cloudflared tunnel --url http://127.0.0.1:7781      # or: tailscale funnel 7781
chloectx me serve --public-url https://<that address>
```

ChatGPT and Claude.ai reach a connector at a public HTTPS address, so put a tunnel in front of it and pass the tunnel's address as `--public-url`. Then:

- **ChatGPT**: in Settings, under Apps & Connectors, turn on developer mode, then create a connector with `https://<address>/mcp` and OAuth.
- **Claude.ai**: Settings, Connectors, Add custom connector, with `https://<address>/mcp`.
- **Claude Code**: `claude mcp add --transport http me https://<address>/mcp` (or the local address).

The consent page asks whether the assistant may also write. It gets these tools:

| tool | what |
|---|---|
| `persona` | the card, with each line's sources |
| `search` | records that answer a query: id, title (the assistant and the chat), address, text |
| `fetch` | one record in full: the exchange it belongs to, or a card line with its sources |
| `ask` | the context for a question, within a budget |
| `remember` | (with write) a line for the card, shown as suggested, or a note |
| `persona_draft`, `persona_suggest`, `persona_review` | (with write) the card drafted by the assistant, and the person's review of it |

`search` and `fetch` answer as ChatGPT's connectors expect. Every call is written to an access log, `chloectx me log`, with the tool, the query and who asked, never what came back. `chloectx me connect` lists the assistants signed in, and `--signout` signs them all out.

## Keep it current

**The browser extension** (`chloectx/extension/`, for Chrome and other Chromium browsers) records your chats on chatgpt.com, claude.ai and gemini.google.com as you have them, into the server's `/capture`. It is off for every site until you turn that site on, and it sends only the messages of the chat open in that tab, to the address you give it, with the capture key. [Its README](../../chloectx/extension/README.md) says how to load it.

**The API proxy** records what apps send to OpenAI's and Anthropic's APIs and what they get back. Point an app's base URL at the server, with the capture key in the path, and keep its own API key:

```bash
export OPENAI_BASE_URL=http://127.0.0.1:7781/p/<capture key>/openai/v1
export ANTHROPIC_BASE_URL=http://127.0.0.1:7781/p/<capture key>/anthropic
```

Requests go through unchanged, streamed or not, and the exchange is recorded after it ends. A request with `x-chloectx-record: off` is not recorded; `x-chloectx-conversation` and `x-chloectx-title` name the chat it belongs to (by default, its first message).

## Share a slice

A share is a signed, read-only copy of part of the memory, with a key of its own:

```bash
chloectx me share create "diet and travel" --sections "Health & diet,Travel" --topic kyoto --expires 30d
chloectx me share list
chloectx me share log <id>        # every call made with its key
chloectx me share revoke <id>
```

It holds the card's lines in the sections named (every section by default, none with `--no-card`), the records that answer each `--topic`, and with `--sources` the messages the card's lines were drawn from. Whoever has the key connects to `https://<address>/s/<key>/mcp` with no sign-in and sees only that slice; the key stops working when it expires or is revoked. The copy is a [context capsule](capsules.md) signed with your key (Ed25519), so `--out DIR` also writes it for `chloectx mount`, which checks the signature and names the signer.

## On a ChloeDB cluster

`chloectx app` serves a space of the cluster the same way, for a memory that should live in a cluster rather than on a laptop ([ChloeCtx on a cluster](app.md)):

```bash
CHLOECTX_CONNECT_PASSCODE="<at least 10 letters and digits>" CHLOECTX_CONNECT_CAPTURE_KEY="ctxc_<a long secret>" \
  chloectx app --server http://127.0.0.1:7700 --port 7760 --connect-url https://me.example.com
```

The space (`--connect-space`, `me` by default) is made if it is not there, and the sign-in, the tools, `/capture` and the proxy are served at the app's address beside its own API, which keeps its token. `--connect-state FILE` keeps the sign-ins across restarts. Shares are served by `chloectx me serve` only.
