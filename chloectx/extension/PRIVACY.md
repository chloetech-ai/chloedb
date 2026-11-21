# ChloeCtx capture: privacy policy

ChloeCtx capture is a browser extension that keeps the chats you have on chatgpt.com, claude.ai and gemini.google.com in your own ChloeCtx memory.

**What it reads.** On the sites you turn on in its options, and only there, it reads the chat open in the tab: the chat's id, title and address, and its messages (yours and the assistant's). On a site that is off, it reads nothing. It reads no other site.

**Where it sends it.** Only to the ChloeCtx address you type in its options: your own `chloectx me serve` (on your computer, `http://127.0.0.1:7781` by default) or a ChloeCtx server you signed up for. Each request carries the capture key you gave it. It sends nothing to ChloeTech or to anyone else, and it has no analytics.

**What it keeps in the browser.** The address, the capture key, which sites are on, and when each site last sent, in the browser's extension storage. Removing the extension removes them.

**What the server keeps.** Your ChloeCtx memory keeps each message once, with the chat it came from. On your own computer that is a file you own (`chloectx me` says where). You can delete chats, or the whole memory, at any time.

**No selling, no ads.** The data is not sold, not used for advertising and not used to train models.

Questions: https://github.com/chloetech-ai/chloedb/issues
