# ChloeCtx for Claude Desktop (Mac)

A Claude Desktop extension (a `.mcpb` file) that installs ChloeCtx's personal memory with a double-click: no terminal, no Rust, no tunnel. Everything is inside the file: `chloectx` for the Mac (one universal binary, Apple silicon and Intel) and the embedding model it searches with.

Once it is installed, the person talks to Claude:

1. "Import my ChatGPT export." Claude calls `find_exports`, which lists the exports in Downloads, Desktop and Documents (or says how to ask ChatGPT, Claude.ai or Google Takeout for one), then `import`.
2. "Draft my persona card." Claude reads the person's own messages a part at a time (`persona_draft`, at most five parts of 24,000 characters) and proposes lines with `persona_suggest`. A line is kept only when it names messages the person wrote, and it shows as suggested until the person confirms it.
3. "Show me the card." Claude lists the suggested lines and confirms, rewrites or drops each as the person says (`persona_review`).

The memory is the same `me` store as the command line's (`~/.local/share/chloectx/me`), so `chloectx me serve` can later serve it to ChatGPT too.

## Tools

| tool | what |
|---|---|
| `persona` | the card, with each line's sources |
| `search`, `fetch`, `ask` | the memory, as for any connected assistant |
| `remember` | a line for the card (suggested) or a note |
| `persona_draft` | a part of the person's own messages, with the rules for a line |
| `persona_suggest` | lines for the card, each with the ids of its source messages |
| `persona_review` | confirm, change or drop lines |
| `find_exports` | the chat exports on this Mac |
| `import` | an export (a zip, a folder or a file) into the memory |

The server is `chloectx me mcp --client Claude`: MCP over standard input and output, as the owner with write access. Its calls go to the access log (`chloectx me log`). `src/remote.rs` has a test that the manifest lists the tools it serves, in order.

## Build and check

The release workflow (`.github/workflows/server-release.yml`, job `claude-desktop`) makes it on a Mac from the release's two macOS builds of chloectx: `lipo` joins them, `codesign --sign -` signs the result ad hoc, and `build.sh` packs it with the model. Then `check.py` runs it as Claude Desktop does, and the file is attested and published with the release as `chloectx-claude-desktop-macos.mcpb`.

```bash
chloectx fetch-model                                   # the model, into ~/.local/share/chloectx/models
sh build.sh path/to/chloectx ~/.local/share/chloectx/models/potion-retrieval-32M 0.3.10 out.mcpb
python3 check.py out.mcpb
```

`check.py` unpacks the file and starts its command with the manifest's arguments and environment. The home folder it uses is a new one, with a small ChatGPT export in Downloads. Over MCP it finds that export, imports it, searches it, drafts a line of the card and reads the card back. On Linux, build it with a Linux chloectx: the manifest's platform is `darwin`, and the check does not look at the binary's.

## Install

In Claude Desktop, open the `.mcpb` file (double-click it, or drag it onto Settings, Extensions) and press Install. The first time it reads Downloads, macOS asks whether Claude may.
