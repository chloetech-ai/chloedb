// How each site's page shows a conversation. The sites change their pages
// without notice: when capture stops on one, these selectors are what to
// update (and `chloectx/extension/test.mjs` checks them on a copy of each page).
// Shared by the content script and the test, so it defines one global.
var CHLOECTX_SITES = {
  chatgpt: {
    hosts: ["chatgpt.com", "chat.openai.com"],
    // /c/<id>, or /g/<gpt>/c/<id>.
    conversation: (path) => (path.match(/\/c\/([A-Za-z0-9-]{6,})/) || [])[1],
    messages: (doc) =>
      [...doc.querySelectorAll("[data-message-author-role]")].map((el) => ({
        role: el.getAttribute("data-message-author-role"),
        text: el.innerText,
      })),
    busy: (doc) => !!doc.querySelector('[data-testid="stop-button"], .result-streaming'),
    title: (doc) => doc.title.replace(/\s*[|-]\s*ChatGPT\s*$/, ""),
  },
  claude: {
    hosts: ["claude.ai"],
    conversation: (path) => (path.match(/\/chat\/([A-Za-z0-9-]{6,})/) || [])[1],
    messages: (doc) =>
      [...doc.querySelectorAll('[data-testid="user-message"], .font-claude-response, [data-testid="assistant-message"]')]
        // A reply's own pieces are inside it: only the outermost counts.
        .filter((el, _, all) => !all.some((o) => o !== el && o.contains(el)))
        .map((el) => ({ role: el.matches('[data-testid="user-message"]') ? "user" : "assistant", text: el.innerText })),
    busy: (doc) => !!doc.querySelector('[data-is-streaming="true"]'),
    title: (doc) => doc.title.replace(/\s*[|-]\s*Claude\s*$/, ""),
  },
  gemini: {
    hosts: ["gemini.google.com"],
    conversation: (path) => (path.match(/\/app\/([A-Za-z0-9_-]{6,})/) || [])[1],
    messages: (doc) =>
      [...doc.querySelectorAll("user-query, model-response")].map((el) => ({
        role: el.tagName.toLowerCase() === "user-query" ? "user" : "assistant",
        text: (el.querySelector(".query-text, message-content") || el).innerText,
      })),
    busy: (doc) => !!doc.querySelector("model-response [aria-busy='true'], .pending-response"),
    title: (doc) => doc.title.replace(/\s*[|-]\s*Gemini\s*$/, ""),
  },
};

function chloectxSiteOf(host) {
  for (const [name, s] of Object.entries(CHLOECTX_SITES)) if (s.hosts.includes(host)) return name;
  return null;
}
