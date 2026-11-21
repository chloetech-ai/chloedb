// Sends the content scripts' captures to the ChloeCtx endpoint with the
// capture key, and keeps each site's last result for the options page.
const DEFAULTS = { endpoint: "http://127.0.0.1:7781", key: "", sites: {}, status: {} };

async function post(payload) {
  const o = await chrome.storage.local.get(DEFAULTS);
  if (!o.key || !o.sites[payload.source]) return;
  let result;
  try {
    const r = await fetch(o.endpoint.replace(/\/+$/, "") + "/capture", {
      method: "POST",
      headers: { "content-type": "application/json", authorization: "Bearer " + o.key },
      body: JSON.stringify(payload),
    });
    const j = await r.json().catch(() => ({}));
    result = r.ok ? { ok: true, at: Date.now(), stored: j.stored || 0, title: payload.title } : { ok: false, at: Date.now(), error: j.error || "HTTP " + r.status };
  } catch (e) {
    result = { ok: false, at: Date.now(), error: "ChloeCtx is not reachable at " + o.endpoint + " (is `chloectx me serve` running?)" };
  }
  const status = (await chrome.storage.local.get({ status: {} })).status;
  status[payload.source] = result;
  await chrome.storage.local.set({ status });
}

chrome.runtime.onMessage.addListener((msg) => {
  if (msg && msg.type === "capture" && msg.payload) post(msg.payload);
});
