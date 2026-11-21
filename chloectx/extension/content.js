// Reads the open conversation when it settles and hands it to the
// background worker, which sends it to ChloeCtx. Nothing happens on a site
// until it is turned on in the extension's options.
(() => {
  const site = chloectxSiteOf(location.hostname);
  if (!site) return;
  const S = CHLOECTX_SITES[site];
  let enabled = false;
  let last = "";
  let timer = null;

  chrome.storage.local.get({ sites: {} }, (o) => {
    enabled = !!o.sites[site];
    if (enabled) schedule();
  });
  chrome.storage.onChanged.addListener((c) => {
    if (c.sites) {
      enabled = !!(c.sites.newValue || {})[site];
      if (enabled) schedule();
    }
  });

  function schedule() {
    clearTimeout(timer);
    timer = setTimeout(capture, 3000);
  }

  function capture() {
    if (!enabled) return;
    const conversation = S.conversation(location.pathname);
    if (!conversation) return;
    if (S.busy(document)) return schedule(); // still answering
    const messages = S.messages(document)
      .filter((m) => (m.role === "user" || m.role === "assistant") && m.text && m.text.trim())
      .map((m) => ({ role: m.role, text: m.text.trim().slice(0, 100000) }));
    if (!messages.length) return;
    // The last 80 messages: ChloeCtx keeps each message once, so what it
    // has already is not stored again.
    const tail = messages.slice(-80);
    const sig = conversation + "|" + messages.length + "|" + tail[tail.length - 1].text.length + "|" + tail[tail.length - 1].text.slice(-64);
    if (sig === last) return;
    last = sig;
    chrome.runtime.sendMessage({
      type: "capture",
      payload: { source: site, conversation, title: S.title(document) || "Untitled chat", url: location.origin + location.pathname, messages: tail },
    });
  }

  new MutationObserver(() => enabled && schedule()).observe(document.documentElement, { childList: true, subtree: true, characterData: true });
  addEventListener("popstate", () => enabled && schedule());
})();
