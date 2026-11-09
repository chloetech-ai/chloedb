// What the command center says of a cluster that follows another
// (chloedb-server's crate::follow): the name a refused write gives
// ("this cluster follows east: ..."), and where to write instead. On the
// hub, the source is the connection of that name, or the one at the
// source's URL; elsewhere, the source's own command center at that URL,
// which the follow view says (it takes an admin key); else the Follow
// view itself.

import { h, api, can, icon, bus, fmt, connectionName, modal, seg, toast, debounce, clear } from "./core.js";

/** The cluster a refused write says this one follows, or null. */
export const followsWho = (msg) => /this cluster follows ([A-Za-z0-9._-]{1,64})[:\s]/.exec(String(msg || ""))?.[1] || null;

const trim = (u) => String(u || "").replace(/\/+$/, "");

/** How far behind its source a follower is, from its lag in ms: "caught up", "1.20 s behind". */
export const behindText = (ms) => (!ms ? "caught up" : `${fmt.ms(ms)} behind`);

/**
 * Where the clients of a cluster that follows `name` write instead:
 * `{href, label}`, `view` the page to open there.
 */
export async function whereToWrite(name, view = "sql") {
  let src = null;
  if (can("admin")) {
    const v = await api("/v1/admin/follow").catch(() => null);
    src = (v?.clusters || []).find((c) => c.name === name)?.status?.source || null;
  }
  const book = await api("/v1/admin/hub", { hub: true }).catch(() => null);
  const conn = (book?.connections || []).find((c) => c.name !== connectionName && (c.name === name || (src && [...(c.urls || []), c.cluster?.public_url].some((u) => u && trim(u) === trim(src)))));
  if (conn) return { href: `/c/${conn.name}#/${view}`, label: `Open ${conn.name}` };
  if (src) return { href: `${trim(src)}/#/${view}`, label: `Open ${(() => { try { return new URL(src).host; } catch { return src; } })()}` };
  return { href: "#/follow", label: "Follow" };
}

/**
 * A note that this cluster follows `name`, so its clients' writes are
 * refused: "write there", with a link to it once known. `view` is the
 * page to open on the source.
 */
export function writeThere(name, view = "sql", { banner = false } = {}) {
  const go = h("a", { class: "btn sm", href: "#/follow" }, icon("share"), "Follow");
  whereToWrite(name, view).then((w) => { go.href = w.href; go.replaceChildren(icon("ext"), w.label); }).catch(() => {});
  const text = h("span", {}, "This cluster follows ", h("b", {}, name), ": it refuses its clients' writes and takes ", h("b", {}, name), "'s. Write there.");
  return banner
    ? h("div", { class: "banner warn follow-note" }, icon("share", "lg"), h("div", { class: "grow" }, text), go)
    : h("div", { class: "note-warn follow-note small" }, icon("share"), h("span", { class: "grow" }, text), go);
}

/**
 * For a page that writes (SQL, Import): a banner on `el` while this
 * cluster follows another, known from its follow view (an admin key);
 * empty otherwise.
 */
export async function followBanner(el, view = "sql") {
  const draw = async () => {
    if (!can("admin")) return null;
    const v = await api("/v1/admin/follow").catch(() => null);
    if (!v?.following) { el.replaceChildren(); return null; }
    el.replaceChildren(writeThere(v.following, view, { banner: true }));
    return v.following;
  };
  // Who is signed in may arrive after the page: ask again then, once.
  const off = bus.on("whoami", () => { off(); if (el.isConnected) draw(); });
  return draw();
}

const START = [
  ["follow", "Follow", "From where the follower is: a copy the first time, then the source's changes as they come"],
  ["copy", "Copy afresh", "The source's rows over the follower's, and the keys the source lacks deleted: an old source rejoining after a promotion that was not planned"],
  ["reverse", "Follow back", "Back from the source's promotion from the follower, with no copy: refused if the follower took writes since"],
];

/**
 * Sets one cluster of the hub's book to follow another (the hub's
 * POST /v1/admin/hub/follow): says what it would do and why it cannot,
 * as asked, then does it on "Start following". `book` is the hub's book;
 * `ask` what is picked to begin with ({follower, source, start}).
 * Resolves the answer, or null when cancelled.
 */
export function startFollow(book, ask = {}) {
  return new Promise((resolve) => {
    const names = (book?.connections || []).map((c) => c.name);
    const st = { follower: ask.follower || names[0] || "", source: ask.source || names.find((n) => n !== (ask.follower || names[0])) || "", start: ask.start || "follow", name: "", source_url: "", key_secret: "", ca_secret: "" };
    let done = false, plan = null, seq = 0;
    const pick = (k) => h("select", { class: "select", "aria-label": k, onchange: (e) => { st[k] = e.target.value; replan(); } }, names.map((n) => h("option", { value: n, selected: n === st[k] }, n)));
    const field = (k, label, placeholder) => h("label", { class: "stack", style: { gap: "4px" } }, h("span", { class: "small dim" }, label),
      h("input", { class: "input", value: st[k], placeholder, autocomplete: "off", spellcheck: "false", "aria-label": label, oninput: (e) => { st[k] = e.target.value.trim(); replanSoon(); } }));
    const what = h("div", { class: "stack follow-plan", "aria-live": "polite" });
    const go = h("button", { class: "btn primary", disabled: true }, icon("share"), "Start following");
    const no = h("button", { class: "btn" }, "Cancel");
    const startSeg = seg(START.map(([v, l, t]) => [v, l, t]), st.start, (v) => { st.start = v; replan(); }, { "aria-label": "How it starts" });
    const body = h("div", { class: "stack" },
      h("p", { class: "dim", style: { margin: 0 } }, "The follower's operator runs a follow process that reads the source's feed with a key of the feed scope the hub makes there, and writes it to the follower. The follower refuses its own clients' writes meanwhile. The hub is not on the way: the follower reads the source itself."),
      h("div", { class: "row", style: { gap: "10px", flexWrap: "wrap" } },
        h("label", { class: "stack", style: { gap: "4px" } }, h("span", { class: "small dim" }, "This cluster"), pick("follower")),
        h("span", { class: "small dim", style: { alignSelf: "flex-end", paddingBottom: "8px" } }, "follows"),
        h("label", { class: "stack", style: { gap: "4px" } }, h("span", { class: "small dim" }, "this one"), pick("source"))),
      h("div", { class: "stack", style: { gap: "4px" } }, h("span", { class: "small dim" }, "How it starts"), startSeg),
      h("details", { class: "more" }, h("summary", {}, "Names, the source's address and Secrets"),
        h("div", { class: "stack", style: { marginTop: "8px" } },
          field("name", "The name the follower knows the source by (default: the source's connection name)", ""),
          field("source_url", "The source's URL as the follower's pods reach it (default: its public URL, or its address in the book)", "https://east.example:7700"),
          field("key_secret", "A Secret of the follower's namespace holding a feed key of the source (key: key); the hub makes one when left empty", ""),
          field("ca_secret", "A Secret with the source's CA certificate (key: ca.crt), for a self-signed one", ""))),
      what);
    const m = modal({ title: "Follow another cluster", body, foot: [no, go], wide: true, onClose: () => { if (!done) resolve(null); } });
    const req = (planOnly) => {
      const b = { follower: st.follower, source: st.source, start: st.start, plan: planOnly };
      for (const k of ["name", "source_url", "key_secret", "ca_secret"]) if (st[k]) b[k] = st[k];
      return b;
    };
    async function replan() {
      const my = ++seq;
      go.disabled = true;
      if (!st.follower || !st.source || st.follower === st.source) { clear(what, h("div", { class: "notice" }, "Pick two clusters: one follows the other.")); return; }
      clear(what, h("div", { class: "small dim" }, "Asking both clusters…"));
      try { plan = await api("/v1/admin/hub/follow", { hub: true, body: req(true), timeout: 20000 }); }
      catch (e) { if (my === seq) clear(what, h("div", { class: "error" }, e.message)); return; }
      if (my !== seq) return;
      clear(what, plan.refused
        ? h("div", { class: "error" }, `${plan.follower} cannot follow ${plan.source} from here: `, plan.refused)
        : [h("div", { class: "small dim" }, `What starting does, ${plan.follower} following ${plan.source} as ${plan.name}${plan.source_url ? ` at ${plan.source_url} (${plan.source_url_from})` : ""}:`),
          h("ol", { class: "follow-steps", style: { margin: 0 } }, (plan.steps || []).map((s) => h("li", {}, s)))]);
      go.disabled = !!plan.refused;
    }
    const replanSoon = debounce(replan, 400);
    no.onclick = () => { done = true; m.close(); resolve(null); };
    go.onclick = async () => {
      go.disabled = true;
      try {
        const r = await api("/v1/admin/hub/follow", { hub: true, body: req(false), timeout: 60000, reason: `${st.follower} follows ${st.source}` });
        done = true; m.close();
        toast(`${r.follower} follows ${r.source} from now on: its operator starts the follow process`, "ok", 7000);
        resolve(r);
      } catch (e) { clear(what, h("div", { class: "error" }, e.message)); go.disabled = false; }
    };
    replan();
  });
}
