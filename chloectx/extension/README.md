# ChloeCtx capture (browser extension)

Records the chats you have on chatgpt.com, claude.ai and gemini.google.com into your own ChloeCtx personal memory, as you have them. It is for Chrome and other Chromium browsers (Manifest V3), and it is off for every site until you turn that site on.

## Load it

1. Start the server: `chloectx me serve`. Its first run prints the passcode and the capture key once (`chloectx me connect --new-capture-key` makes a new key).
2. In Chrome, open `chrome://extensions`, turn on Developer mode, choose **Load unpacked** and pick this folder.
3. Open the extension's options (its toolbar button), give it the server's address (`http://127.0.0.1:7781` by default) and the capture key, tick the sites to record, and press **Test connection**.

An https address (a tunnel, or a server on a cluster) asks for the browser's permission to reach it when you save.

## What it sends

- Only on the sites you turned on, and only the chat open in that tab: its id, title, address and its last 80 messages, a few seconds after the page stops changing (never while an answer is still being written).
- Only to the address you gave, with the capture key; nowhere else. The server keeps each message once, so sending the same chat again adds only what is new.
- The options page shows, per site, when it last sent and what the server said.

The pages of these sites change; the selectors it reads them with are in `sites.js`, one entry per site.

## Test

`test.mjs` starts `chloectx me serve` on a store of its own, loads the extension into Chromium (Playwright), sets it up through its options page, serves a copy of each site's page in place of the real site, and checks that each chat arrives in the store once and that a site that is off sends nothing:

```bash
cd chloectx && cargo build --profile ci --bin chloectx
CHLOECTX_BIN=target/ci/chloectx NODE_PATH=$(npm root -g) node extension/test.mjs
```
