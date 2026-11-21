const SITES = ["chatgpt", "claude", "gemini"];
const $ = (id) => document.getElementById(id);

function ago(t) {
  const s = Math.round((Date.now() - t) / 1000);
  return s < 60 ? s + " s ago" : s < 3600 ? Math.round(s / 60) + " min ago" : Math.round(s / 3600) + " h ago";
}

async function load() {
  const o = await chrome.storage.local.get({ endpoint: "http://127.0.0.1:7781", key: "", sites: {}, status: {} });
  $("endpoint").value = o.endpoint;
  $("key").value = o.key;
  for (const s of SITES) {
    $("site-" + s).checked = !!o.sites[s];
    const st = o.status[s];
    const el = $("st-" + s);
    el.className = st ? (st.ok ? "ok" : "bad") : "";
    el.textContent = !st ? "" : st.ok ? "kept " + st.stored + " new, " + ago(st.at) : st.error;
  }
}

function say(t, ok) {
  $("msg").textContent = t;
  $("msg").className = ok ? "ok" : "bad";
}

async function save() {
  const endpoint = $("endpoint").value.trim().replace(/\/+$/, "") || "http://127.0.0.1:7781";
  // An address that is not this machine needs the browser's leave.
  if (!/^http:\/\/(127\.0\.0\.1|localhost)(:\d+)?$/.test(endpoint)) {
    if (!endpoint.startsWith("https://")) return say("The address is http://127.0.0.1:PORT or an https:// one.", false);
    const ok = await chrome.permissions.request({ origins: [new URL(endpoint).origin + "/*"] });
    if (!ok) return say("The browser did not allow that address.", false);
  }
  const sites = {};
  for (const s of SITES) sites[s] = $("site-" + s).checked;
  await chrome.storage.local.set({ endpoint, key: $("key").value.trim(), sites });
  say("Saved. Reload an open chat for it to take effect.", true);
}

async function test() {
  const o = await chrome.storage.local.get({ endpoint: "http://127.0.0.1:7781", key: "" });
  try {
    const r = await fetch(o.endpoint + "/capture/ping", { headers: { authorization: "Bearer " + o.key } });
    say(r.ok ? "Connected." : r.status === 401 ? "ChloeCtx answers, but the capture key is wrong." : "ChloeCtx answered HTTP " + r.status + ".", r.ok);
  } catch (e) {
    say("Nothing answers at " + o.endpoint + ": is `chloectx me serve` running?", false);
  }
}

$("save").addEventListener("click", save);
$("test").addEventListener("click", test);
chrome.storage.onChanged.addListener(load);
load();
