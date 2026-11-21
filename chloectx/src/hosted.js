// The hosted beta's pages (src/hosted.rs): sign-in and sign-up, and the
// account page. No libraries; the server sends JSON.
"use strict";
const $ = (s) => document.querySelector(s);
const esc = (s) => String(s ?? "").replace(/[&<>"']/g, (c) => ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;", "'": "&#39;" })[c]);
async function post(url, body, raw) {
  const r = await fetch(url, { method: "POST", headers: raw ? {} : { "content-type": "application/json" }, body: raw ? body : JSON.stringify(body || {}) });
  const j = await r.json().catch(() => ({}));
  if (!r.ok) throw new Error(j.error || `${r.status}`);
  return j;
}
function say(el, text, kind) {
  el.textContent = text;
  el.className = "msg" + (kind ? " " + kind : "");
}

for (const id of ["signin", "signup"]) {
  const f = document.getElementById(id);
  if (!f) continue;
  f.addEventListener("submit", async (e) => {
    e.preventDefault();
    const msg = f.querySelector(".msg");
    const body = Object.fromEntries(new FormData(f).entries());
    try {
      say(msg, id === "signup" ? "Making your memory…" : "Signing in…");
      await post("/" + id, body);
      location.href = "/account";
    } catch (err) {
      say(msg, err.message, "bad");
    }
  });
}

const n = (k, w) => `${k} ${w}${k === 1 ? "" : "s"}`;
const when = (ms) => (ms ? new Date(ms).toLocaleString() : "");
let polling = null;

async function load() {
  const r = await fetch("/account/state");
  if (r.status === 401) return (location.href = "/");
  const s = await r.json();
  $("#connector").textContent = s.connector;
  $("#address").textContent = s.address;
  const src = s.status.by_source || [];
  $("#sources").innerHTML = src.length
    ? "<h3>What it holds</h3><table>" + src.map((x) => `<tr><td>${esc(x.label)}</td><td>${n(x.chats, "chat")}</td><td>${n(x.messages, "message")}</td></tr>`).join("") + "</table>"
    : '<p class="small">Nothing yet.</p>';
  const lines = s.card || [];
  $("#lines").innerHTML = lines.length
    ? lines
        .map(
          (l) => `<div class="line"><span class="t">${esc(l.text)} <span class="tag">${esc(l.section)}</span>${l.confirmed ? "" : ' <span class="tag">suggested</span>'}
<span class="s">${(l.sources || []).slice(0, 3).map((x) => esc(`${x.source}: ${x.title || ""}`)).join(" · ")}</span></span>
${l.confirmed ? "" : `<button data-id="${esc(l.id)}" data-action="confirm">Keep</button>`}<button class="ghost" data-id="${esc(l.id)}" data-action="drop">Drop</button></div>`
        )
        .join("")
    : '<p class="small">No lines yet.</p>';
  const a = s.assistants || [];
  $("#assistants").innerHTML = a.length
    ? "<table>" + a.map((x) => `<tr><td>${esc(x.client)}</td><td>${x.scope && x.scope.includes("write") ? "reads and suggests" : "reads"}</td><td>since ${esc(when(x.since))}</td></tr>`).join("") + "</table>"
    : '<p class="small">None yet.</p>';
  const log = s.log || [];
  $("#log").innerHTML = log.length
    ? "<table>" + log.map((x) => `<tr><td>${esc(when(x.ts))}</td><td>${esc(x.client)}</td><td>${esc(x.tool)}</td><td>${esc(Object.values(x.args || {}).join(" "))}</td></tr>`).join("") + "</table>"
    : '<p class="small">Nothing read yet.</p>';
  $("#capture-key").textContent = s.capture_key ? "Make a new capture key" : "Make a capture key";
  const imp = s.import;
  const msg = $("#import-msg");
  const bar = $("#progress");
  if (imp && imp.state === "running") {
    bar.hidden = false;
    bar.max = Math.max(imp.chats || 1, 1);
    bar.value = imp.done || 0;
    say(msg, imp.chats ? `Importing ${imp.file}: ${imp.done} of ${imp.chats} chats, ${imp.stored} new messages.` : `Reading ${imp.file}…`);
    if (!polling) polling = setInterval(load, 2000);
  } else {
    if (polling) clearInterval(polling), (polling = null);
    if (imp && imp.state === "done") {
      bar.hidden = true;
      say(msg, `Imported ${imp.file}: ${imp.stored} new messages, ${imp.known} already kept. Next, ask Claude to draft your persona card.`, "ok");
    } else if (imp && imp.state === "failed") {
      bar.hidden = true;
      say(msg, `${imp.file}: ${imp.error}`, "bad");
    }
  }
  return s;
}

if ($("#connector")) {
  let part = 3 << 20;
  load().then((s) => (part = s.part || part));
  document.addEventListener("click", async (e) => {
    const b = e.target.closest("button[data-copy]");
    if (b) {
      await navigator.clipboard.writeText($("#" + b.dataset.copy).textContent);
      b.textContent = "Copied";
      setTimeout(() => (b.textContent = "Copy"), 1500);
    }
    const c = e.target.closest("button[data-action]");
    if (c) {
      c.disabled = true;
      await post("/account/card", { id: c.dataset.id, action: c.dataset.action }).catch((err) => alert(err.message));
      load();
    }
  });
  $("#file").addEventListener("change", async (e) => {
    const f = e.target.files[0];
    if (!f) return;
    const msg = $("#import-msg");
    const bar = $("#progress");
    bar.hidden = false;
    bar.max = f.size || 1;
    try {
      for (let off = 0; off < f.size || off === 0; off += part) {
        const last = off + part >= f.size;
        const q = `name=${encodeURIComponent(f.name)}&offset=${off}${last ? "&final=1" : ""}`;
        say(msg, `Uploading ${f.name}: ${Math.round((100 * off) / (f.size || 1))}%`);
        bar.value = off;
        await post("/account/upload?" + q, f.slice(off, off + part), true);
        if (last) break;
      }
      e.target.value = "";
      load();
    } catch (err) {
      say(msg, err.message, "bad");
    }
  });
  $("#signout").addEventListener("click", async (e) => {
    e.preventDefault();
    await post("/signout");
    location.href = "/";
  });
  $("#signout-assistants").addEventListener("click", async () => {
    await post("/account/signout-assistants");
    load();
  });
  $("#capture-key").addEventListener("click", async () => {
    const r = await post("/account/capture-key");
    say($("#key-msg"), `Your capture key, shown once: ${r.capture_key}. Paste it into the extension's options with the address above. A new key replaces the old one.`, "ok");
    load();
  });
  $("#delete").addEventListener("click", async () => {
    try {
      await post("/account/delete", { passcode: $("#delete-pass").value });
      location.href = "/";
    } catch (err) {
      say($("#delete-msg"), err.message, "bad");
    }
  });
}
