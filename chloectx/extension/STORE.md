# Chrome Web Store listing

What to paste into the Chrome Web Store's developer dashboard for ChloeCtx capture. Package: `sh pack.sh chloectx-capture.zip`.

## Store listing

- **Name:** ChloeCtx capture
- **Summary** (132 characters at most): Keep your ChatGPT, Claude and Gemini chats in your own ChloeCtx memory, which any assistant you connect can search.
- **Category:** Productivity
- **Language:** English
- **Description:**

  ChloeCtx keeps a memory of your own: your chats with ChatGPT, Claude and Gemini, and a persona card of what they say about you, each line linked to the chat it came from. Claude, ChatGPT and other assistants you connect read it, so you stop repeating yourself.

  This extension keeps that memory current as you chat. Turn it on for the sites you want (chatgpt.com, claude.ai, gemini.google.com); it is off for all of them until you do. A few seconds after a chat stops changing, it sends that chat's messages to your ChloeCtx address, and each message is kept once.

  It needs ChloeCtx: on a Mac, Claude Desktop's ChloeCtx extension or `chloectx me serve`; or a ChloeCtx server you signed up for. Set its address and capture key in the extension's options.

  Nothing goes anywhere but the address you set. No analytics, no ads.

- **Icon:** icon128.png
- **Screenshots** (1280x800): `store/options.png` (the options page).
- **Homepage:** https://chloetech-ai.github.io/chloedb/site/ctx/
- **Support:** https://github.com/chloetech-ai/chloedb/issues

## Privacy practices

- **Single purpose:** Keeps the person's own chats with AI assistants on chatgpt.com, claude.ai and gemini.google.com in their own ChloeCtx memory.
- **Permission justifications:**
  - `storage`: keeps the ChloeCtx address, the capture key, the sites turned on and each site's last result.
  - Host permissions for chatgpt.com, chat.openai.com, claude.ai and gemini.google.com: reads the open chat on those sites, only when the person turned that site on.
  - Host permissions for 127.0.0.1 and localhost: sends the chat to ChloeCtx running on the person's own computer.
  - Optional host permission `https://*/*`: asked for only when the person sets an https ChloeCtx address (a server they signed up for), and then only for that address's origin.
- **Remote code:** No. All code is in the package.
- **Data usage:** collects *website content* (the chats on the sites turned on) and *authentication information* (the capture key the person types). Sent only to the address the person sets. Not sold, not used or transferred for purposes unrelated to the single purpose, not used for creditworthiness or lending.
- **Privacy policy URL:** https://github.com/chloetech-ai/chloedb/blob/main/chloectx/extension/PRIVACY.md

## Steps (yours: the account is ChloeTech's)

1. Register as a Chrome Web Store developer (one-time fee) with the ChloeTech account, and verify the contact email.
2. New item: upload `chloectx-capture.zip`.
3. Paste the listing and privacy practices above; upload icon128.png and the screenshot.
4. Distribution: Public (or Unlisted while the hosted beta is small). Submit for review.
