// Away (SPEC.md, "Levels", Away): the digest of what happened while you were
// gone, from `GET /api/digest`. A headline, four tiles, the story in order
// with ◆ chips, the open questions answered inline, a rule proposal to
// adopt, and "Replay the evening": the decision moments played one after
// another through the Rewind player. Mounting it marks the fleet seen.
import {
  h, useStyle, avatar, agentChip, ctxChip, pill, empty, fmtDur, fmtClock, fmtAgo, toMs, md, card, setFleet,
} from "./common.js";

const nameOf = (a) => (a && (a.name || a.id)) || "?";
const CTX_KINDS = new Set(["deja_vu", "rule", "decision", "preference", "fact", "fix", "record", "error-fix", "ctx"]);
const num = (x) => (x == null ? 0 : typeof x === "number" ? x : Number(x.count ?? x.n ?? 0));
const parse = (x) => { if (typeof x !== "string") return x; try { return JSON.parse(x); } catch { return null; } };
const plural = (n, one, many = one + "s") => `${n} ${n === 1 ? one : many}`;

export function mount(root, ctx) {
  const S = { d: null, err: null, answered: new Map(), busy: new Set(), errs: new Map(), ruleOn: new Map(), ruleText: new Map(), freeText: new Map(),
    dismissed: new Set(), adopted: new Set(), playing: -1, stop: false };
  const el = h("div", { class: "lv-screen aw" });
  root.append(el);
  let alive = true, waitT = null, waitRes = null;
  setFleet(ctx.state?.agents || []);

  const agents = () => new Map((ctx.state?.agents || []).map((a) => [a.id, a]));
  const agentOf = (x) => (x && typeof x === "object" ? x : agents().get(x) || (x ? { id: x, name: x } : null));

  async function load() {
    try {
      // The digest first, then mark seen: the digest's default `since` is the last time seen.
      S.d = await ctx.api("/api/digest");
      S.err = null;
      setFleet([...(ctx.state?.agents || []), ...(S.d.story || []).map((s) => s.agent).filter(Boolean)].filter((a, i, all) => all.findIndex((b) => b.id === a.id) === i));
    } catch (e) { S.err = e.message || String(e); }
    ctx.api("/api/seen", { method: "POST", body: {} }).catch(() => {});
    if (alive) render();
  }

  // ---------------------------------------------------------------- render
  function render() {
    if (!S.d) { el.replaceChildren(S.err ? empty(`The digest could not load: ${S.err}`) : empty("Reading what happened…")); return; }
    const d = S.d;
    const since = toMs(d.since_ms), until = toMs(d.until_ms) || Date.now();
    const t = d.tiles || {};
    const open = openQuestions();
    const waiting = open.length;
    const done = num(t.done), decided = num(t.decided) + [...S.answered.values()].length, learned = num(t.learned);
    el.replaceChildren(
      h("div", { class: "aw-eyebrow" }, `You were away ${fmtDur(until - since)} · ${fmtClock(since)} → ${fmtClock(until)}`),
      h("h1", { class: "aw-h1" }, headline(done, waiting, decided)),
      h("div", { class: "aw-tiles" },
        tile(done, "good", "outcomes done", done ? "finished and marked done by their agents" : "none finished yet", () => ctx.go("#/mission")),
        tile(decided, "ctx", "decided without you", decidedSub(), () => ctx.go("#/decide")),
        tile(waiting, "warn", "waiting on you", waitingSub(open), waiting ? () => el.querySelector(".aw-q button, .aw-q textarea")?.focus() : null),
        tile(learned, "acc", "new things learned", learnedSub(learned), null)),
      h("div", { class: "aw-grid" },
        h("div", { class: "aw-col" }, storyCard(d), nowCard()),
        h("div", { class: "aw-col" }, replayCard(d), questionsCard(open))));
  }

  function headline(done, waiting, decided) {
    if (!done && !waiting && !decided && !(S.d.story || []).length) return "Welcome back. Nothing happened while you were away.";
    const parts = [];
    if (done) parts.push(`${plural(done, "outcome")} done`);
    if (decided) parts.push(`${decided} decided without you`);
    const head = parts.length ? parts.join(", ") : "The fleet kept working";
    const tail = waiting ? (waiting === 1 ? "one question needs you." : `${waiting} questions need you.`) : "nothing needs you.";
    return `Welcome back. ${head}; ${tail}`;
  }

  function tile(n, tone, label, sub, go) {
    const body = [h("div", { class: `aw-tn aw-${tone}` }, String(n)), h("div", { class: "aw-tl" }, label), h("div", { class: "aw-ts" }, sub)];
    return go ? h("button", { class: "lv-card aw-tile", onclick: go }, body) : h("div", { class: "lv-card aw-tile" }, body);
  }

  function decidedSub() {
    const qs = (S.d.questions || []).filter((q) => q.state === "answered");
    const byRule = qs.filter((q) => String(q.answered_by || "").startsWith("rule:")).length;
    const byTerm = qs.filter((q) => q.answered_by === "terminal").length;
    const bits = [];
    const byYou = qs.length - byRule - byTerm;
    if (byRule) bits.push(`${byRule} by your rules`);
    if (byTerm) bits.push(`${byTerm} in a terminal`);
    if (byYou) bits.push(`${byYou} by you`);
    if (S.answered.size) bits.push(`${S.answered.size} just now`);
    return bits.length ? bits.join(", ") + "; each can be undone" : "by your rules; each can be undone";
  }
  function waitingSub(open) {
    if (!open.length) return "nothing; the fleet is unblocked";
    const oldest = open.reduce((a, b) => (toMs(a.created) < toMs(b.created) ? a : b));
    const names = [...new Set(open.map((q) => nameOf(agentOf(q.agent))))];
    return `${names.slice(0, 2).join(", ")}${names.length > 2 ? ` and ${names.length - 2} more` : ""}, ${fmtAgo(oldest.created).replace(" ago", "")}`;
  }
  function learnedSub(n) {
    const props = proposals().length;
    return [n ? "records in ChloeCtx" : "nothing new", props ? `${plural(props, "rule proposal")}` : null].filter(Boolean).join(", ");
  }

  const openQuestions = () => (S.d.questions || []).filter((q) => q.state === "open" || S.answered.has(q.id));
  const proposals = () => (S.d.rules || []).filter((r) => (r.proposed || r.active === false || S.adopted.has(r.id)) && !S.dismissed.has(r.id));

  // ----------------------------------------------------------------- story
  function storyCard(d) {
    const lines = [...(d.story || [])].sort((a, b) => toMs(a.ts) - toMs(b.ts));
    return h("section", { class: "lv-card aw-story" },
      h("div", { class: "lv-card-head" }, h("h2", { class: "lv-h2" }, "What happened, in order"), h("span", { class: "lv-sub" }, plural(lines.length, "moment"))),
      lines.length ? h("ol", { class: "aw-lines" }, lines.map((s) => {
        const a = agentOf(s.agent);
        return h("li", { class: "aw-line" },
          h("time", { class: "aw-time", datetime: new Date(toMs(s.ts)).toISOString() }, fmtClock(s.ts)),
          h("p", null,
            a ? h("span", { class: "aw-who" }, agentChip(a, ctx)) : h("span", { class: "aw-who aw-ctxwho" }, ctxChip("ChloeCtx")),
            " ", md(s.text), " ",
            (s.chips || []).map((c) => chip(c))));
      })) : h("div", { class: "lv-sub" }, "No agent did anything worth telling."),
      proposals().map((r) => proposal(r)));
  }

  function chip(c) {
    const k = String(c.kind || "").toLowerCase();
    if (CTX_KINDS.has(k)) return h("span", { class: "aw-chip" }, ctxChip(c.text, `${k.replace("_", " ")}, from ChloeCtx`));
    if (k === "waiting" || k === "question") return h("span", { class: "aw-chip" }, pill(c.text, "warn"));
    if (k === "test" || k === "pr" || k === "done") return h("span", { class: "aw-chip" }, pill(c.text, "good"));
    if (k === "failed" || k === "error") return h("span", { class: "aw-chip" }, pill(c.text, "crit"));
    return h("span", { class: "aw-chip" }, pill(c.text, "mut"));
  }

  function proposal(r) {
    const adopted = S.adopted.has(r.id) || r.active === true;
    const busy = S.busy.has(r.id);
    return h("div", { class: "aw-prop" + (adopted ? " done" : "") },
      h("div", { class: "aw-prop-t" }, h("b", null, adopted ? "Rule adopted" : "Proposed rule"), r.evidence ? `, ${r.evidence}` : "", ": ",
        h("i", null, `“${r.text}”`)),
      adopted ? h("div", { class: "aw-prop-a" }, pill("Every agent of the repo gets it from its next prompt", "good"),
        h("button", { class: "btn sm ghost", onclick: () => setRule(r, false) }, "Undo"))
        : h("div", { class: "aw-prop-a" },
          h("button", { class: "btn sm primary", disabled: busy ? true : null, onclick: () => setRule(r, true) }, busy ? "Adopting…" : "Adopt"),
          h("button", { class: "btn sm", onclick: () => { S.dismissed.add(r.id); render(); } }, "Not yet")),
      S.errs.has(r.id) ? h("div", { class: "err" }, S.errs.get(r.id)) : null);
  }

  async function setRule(r, active) {
    S.busy.add(r.id); S.errs.delete(r.id); render();
    try {
      const res = await ctx.api(`/api/rules/${encodeURIComponent(r.id)}`, { method: "PATCH", body: { active } });
      Object.assign(r, res.rule || { active });
      if (active) S.adopted.add(r.id); else { S.adopted.delete(r.id); r.active = false; }
    } catch (e) { S.errs.set(r.id, e.message || String(e)); }
    S.busy.delete(r.id);
    render();
  }

  // ---------------------------------------------------------------- replay
  // A moment's times are since its agent's session started (as Rewind
  // counts them); the clock time is that plus the session's start.
  function clockOf(m) {
    const from = toMs(m.from_ms);
    if (from > 1e12) return from;
    const a = agentOf(m.agent);
    const st = toMs(a?.started_ms ?? a?.created);
    return st ? st + from : null;
  }

  function replayCard(d) {
    const moments = d.replay || [];
    const total = moments.reduce((s, m) => s + Math.max(0, toMs(m.to_ms) - toMs(m.from_ms)), 0);
    const since = toMs(d.since_ms), until = toMs(d.until_ms) || Date.now();
    const playing = S.playing >= 0;
    return h("section", { class: "lv-card aw-replay" },
      h("div", { class: "lv-card-head" }, h("h2", { class: "lv-h2" }, "Replay the evening"),
        h("span", { class: "lv-sub" }, moments.length ? `${fmtDur(until - since)} in ${fmtDur(total)}` : "")),
      moments.length ? h("div", { class: "aw-reel" }, moments.map((m, i) => {
        const a = agentOf(m.agent);
        const t = clockOf(m);
        return h("button", {
          class: "aw-cell" + (S.playing === i ? " on" : "") + (playing && i < S.playing ? " past" : ""),
          "aria-label": `Rewind ${nameOf(a)}${t ? " at " + fmtClock(t) : ""}: ${String(m.label || "").replace(/\n/g, " ")}`,
          onclick: () => playOne(i),
        },
        h("span", { class: "aw-cl" }, m.label || ""),
        h("span", { class: "aw-cf" }, avatar(a, 14), h("span", null, nameOf(a)), h("span", { class: "aw-ct" }, t ? fmtClock(t) : "")),
        S.playing === i ? h("span", { class: "aw-prog", style: `animation-duration:${clipMs(m)}ms` }) : null);
      })) : h("div", { class: "lv-sub" }, "No decision was made while you were away."),
      moments.length ? h("div", { class: "aw-acts" },
        playing
          ? h("button", { class: "btn primary sm", onclick: stopPlay }, "■ Stop")
          : h("button", { class: "btn primary sm", onclick: () => playAll(0) }, "▶ Play the decisions"),
        h("span", { class: "lv-sub" }, playing ? `${S.playing + 1} of ${moments.length}: ${nameOf(agentOf(moments[S.playing].agent))}` : `${plural(moments.length, "moment")}, one after another`)) : null);
  }

  const clipMs = (m) => Math.min(60000, Math.max(3000, toMs(m.to_ms) - toMs(m.from_ms)));
  const argsOf = (m) => ({ agent: agentOf(m.agent)?.id || m.agent, from_ms: toMs(m.from_ms), to_ms: toMs(m.to_ms), label: m.label });

  // Each moment goes to the Rewind player; when it answers with a promise,
  // the next one waits for it, otherwise for the clip's own length.
  function waitFor(res, m) {
    if (res && typeof res.then === "function") return Promise.race([res, new Promise((r) => { waitRes = r; })]);
    return new Promise((r) => { waitRes = r; waitT = setTimeout(r, clipMs(m)); });
  }
  async function playAll(from) {
    const moments = S.d.replay || [];
    S.stop = false;
    for (let i = from; i < moments.length && !S.stop && alive; i++) {
      S.playing = i; render();
      let res;
      try { res = ctx.openRewind(argsOf(moments[i])); } catch {}
      await waitFor(res, moments[i]);
    }
    S.playing = -1;
    if (alive) render();
  }
  function playOne(i) {
    stopPlay();
    const m = S.d.replay[i];
    S.playing = i; render();
    let res;
    try { res = ctx.openRewind(argsOf(m)); } catch {}
    waitFor(res, m).then(() => { if (S.playing === i) { S.playing = -1; if (alive) render(); } });
  }
  function stopPlay() {
    S.stop = true;
    clearTimeout(waitT);
    waitRes?.();
    S.playing = -1;
    render();
  }

  // ------------------------------------------------------------- questions
  function questionsCard(open) {
    if (!open.length) return null;
    const still = open.filter((q) => !S.answered.has(q.id)).length;
    return h("section", { class: "lv-card aw-qs" },
      h("div", { class: "lv-card-head" }, h("h2", { class: "lv-h2" }, still === 1 ? "Answer the one question" : still ? `Answer ${still} questions` : "Answered")),
      open.map((q) => question(q)));
  }

  function question(q) {
    const a = agentOf(q.agent);
    const opts = parse(q.options) || [];
    const prec = parse(q.precedent);
    const recs = Array.isArray(prec) ? prec : prec?.records || prec?.hits || prec?.results || [];
    const done = S.answered.get(q.id);
    if (done) {
      return h("div", { class: "aw-q done" },
        h("div", { class: "aw-qa" }, pill("✓ answered", "good"), h("span", null, `“${done.answer}”`), done.rule ? ctxChip("and a rule") : null),
        h("div", { class: "lv-sub" }, `${nameOf(a)} goes on${done.rule ? "; the rule reaches every agent from its next prompt" : ""}.`));
    }
    const busy = S.busy.has(q.id);
    const ruleOn = S.ruleOn.get(q.id) || false;
    const free = h("input", { class: "aw-in", placeholder: "Or answer in your own words…", "aria-label": "Your answer", value: S.freeText.get(q.id) || "" });
    free.addEventListener("input", () => S.freeText.set(q.id, free.value));
    free.addEventListener("keydown", (e) => { if (e.key === "Enter" && free.value.trim()) answer(q, { text: free.value.trim() }, free.value.trim()); });
    const ruleIn = ruleOn ? h("input", { class: "aw-in", "aria-label": "The rule", value: S.ruleText.get(q.id) ?? "" , placeholder: "The rule, as agents will read it" }) : null;
    ruleIn?.addEventListener("input", () => S.ruleText.set(q.id, ruleIn.value));
    return h("div", { class: "aw-q" },
      h("div", { class: "aw-qh" }, agentChip(a, ctx), h("span", { class: "lv-sub" }, `${q.kind === "permission" ? "asks permission" : "asks"} · ${fmtAgo(q.created)}`)),
      h("div", { class: "aw-qt" }, q.text),
      q.context ? h("div", { class: "aw-qc" }, q.context) : null,
      recs.length ? h("div", { class: "aw-prec" }, h("span", { class: "aw-prec-h" }, "Precedent"), recs.slice(0, 3).map((r) => h("div", { class: "aw-prec-r" }, ctxChip(r.kind || "record"), h("span", null, r.text)))) : null,
      h("div", { class: "aw-opts" }, opts.map((o, i) => h("button", {
        class: "btn sm" + (i === q.recommended ? " primary" : ""), disabled: busy ? true : null, title: o.detail || "",
        onclick: () => answer(q, { option: i }, o.label),
      }, o.label, o.matches_precedent ? h("span", { class: "aw-match", title: "matches what was decided before" }, "◆") : null))),
      opts.some((o) => o.detail) ? h("ul", { class: "aw-od" }, opts.filter((o) => o.detail).map((o) => h("li", null, h("b", null, o.label), ": ", o.detail))) : null,
      free,
      h("label", { class: "aw-rule" }, h("input", { type: "checkbox", checked: ruleOn ? true : null, onchange: (e) => {
        S.ruleOn.set(q.id, e.target.checked);
        if (e.target.checked && !S.ruleText.has(q.id)) S.ruleText.set(q.id, "");
        render();
        requestAnimationFrame(() => el.querySelector(".aw-q .aw-rule + .aw-in")?.focus());
      } }), "and make it a rule for this repository"),
      ruleIn,
      S.errs.has(q.id) ? h("div", { class: "err" }, S.errs.get(q.id)) : null);
  }

  async function answer(q, body, label) {
    const rule = S.ruleOn.get(q.id) ? (S.ruleText.get(q.id) || "").trim() || `${q.text.replace(/\?$/, "")}: ${label}.` : null;
    S.busy.add(q.id); S.errs.delete(q.id); render();
    try {
      const res = await ctx.api(`/api/questions/${encodeURIComponent(q.id)}/answer`, { method: "POST", body: rule ? { ...body, rule } : body });
      S.answered.set(q.id, { answer: res.question?.answer || label, rule });
    } catch (e) { S.errs.set(q.id, e.message || String(e)); }
    S.busy.delete(q.id);
    render();
  }

  // ------------------------------------------------------------------- now
  function nowCard() {
    const list = (ctx.state?.agents || []).filter((a) => a.running !== false && a.state !== "exited");
    if (!list.length) return null;
    const tone = { working: "acc", waiting: "warn", idle: "mut", starting: "mut" };
    return card("Now", h("span", { class: "lv-sub" }, plural(list.length, "agent") + " running"),
      h("div", { class: "aw-now" }, list.map((a) => h("div", { class: "aw-now-r" },
        agentChip(a, ctx), pill(a.state === "waiting" ? "waiting for you" : a.state, tone[a.state] || "mut"),
        h("div", { class: "aw-now-n" }, a.narration || a.last_event || "")))));
  }

  load();
  render();
  return {
    update(state) { if (state) ctx.state = { ...ctx.state, ...state }; if (S.d && S.playing < 0) render(); },
    unmount() { alive = false; S.stop = true; clearTimeout(waitT); waitRes?.(); el.remove(); },
  };
}

useStyle("away", `
.aw{display:flex;flex-direction:column}
.aw-eyebrow{color:var(--muted);font-weight:650;font-size:12px;text-transform:uppercase;letter-spacing:.05em}
.aw-h1{margin:4px 0 0;font-size:26px;font-weight:700;letter-spacing:-.015em;line-height:1.25}
.aw-tiles{display:grid;grid-template-columns:repeat(4,minmax(0,1fr));gap:12px;margin-top:18px}
.aw-tile{text-align:left;font:inherit;color:inherit;padding:16px 18px;display:flex;flex-direction:column;gap:2px}
button.aw-tile{cursor:pointer}button.aw-tile:hover{border-color:var(--line-2);background:var(--surface-2)}
.aw-tn{font-size:36px;font-weight:750;line-height:1.1;letter-spacing:-.02em;font-variant-numeric:tabular-nums}
.aw-good{color:var(--good)}.aw-ctx{color:var(--ctx)}.aw-warn{color:var(--warn)}.aw-acc{color:var(--accent)}
.aw-tl{font-weight:650;margin-top:4px}.aw-ts{color:var(--muted);font-size:12.5px}
.aw-grid{display:grid;grid-template-columns:minmax(0,1fr) 470px;gap:18px;margin-top:16px;align-items:start}
.aw-col{display:flex;flex-direction:column;gap:12px;min-width:0}
.aw-story{padding:20px 24px}
.aw-lines{list-style:none;margin:0;padding:0;display:flex;flex-direction:column}
.aw-line{display:grid;grid-template-columns:62px minmax(0,1fr);gap:10px;padding:9px 0;border-top:1px dashed var(--line)}
.aw-line:first-child{border-top:0;padding-top:2px}
.aw-time{color:var(--muted);font:11.5px/26px var(--mono);white-space:nowrap}
.aw-line p{margin:0;font-size:15px;line-height:1.7;color:var(--text-2)}
.aw-line p b{color:var(--text)}
.aw-who{display:inline-flex;vertical-align:1px}
.aw-who .lv-agent{background:var(--surface-3);border-radius:7px;padding:1px 8px 1px 3px;color:var(--text);font-size:13px}
.aw-ctxwho{background:color-mix(in srgb,var(--ctx) 12%,transparent);border-radius:7px;padding:0 8px}
.aw-chip{display:inline-flex;vertical-align:1px;margin-left:2px}
.aw-chip .lv-ctx{font-size:12px}
.aw-prop{margin-top:14px;padding:14px 16px;border-radius:10px;background:color-mix(in srgb,var(--ctx) 8%,var(--surface));border:1px solid color-mix(in srgb,var(--ctx) 30%,var(--line))}
.aw-prop-t{font-size:15px;line-height:1.55;color:var(--text-2)}.aw-prop-t b{color:var(--text)}.aw-prop-t i{color:var(--text)}
.aw-prop-a{display:flex;align-items:center;gap:8px;margin-top:10px;flex-wrap:wrap}
.aw-reel{display:grid;grid-template-columns:repeat(3,minmax(0,1fr));gap:6px}
.aw-cell{position:relative;display:flex;flex-direction:column;justify-content:space-between;gap:6px;height:88px;padding:7px 8px;border-radius:8px;background:var(--term-bg);border:1px solid var(--line);color:var(--term-fg);text-align:left;cursor:pointer;overflow:hidden;font:inherit}
.aw-cell:hover{border-color:var(--line-2)}
.aw-cell.on{border-color:var(--ctx);box-shadow:0 0 0 2px color-mix(in srgb,var(--ctx) 35%,transparent)}
.aw-cell.past{opacity:.55}
.aw-cl{font:10.5px/1.45 var(--mono);white-space:pre-line;overflow:hidden;display:-webkit-box;-webkit-line-clamp:3;-webkit-box-orient:vertical;opacity:.85}
.aw-cf{display:flex;align-items:center;gap:5px;font-size:11px;color:var(--muted);min-width:0}
.aw-cf span:nth-child(2){overflow:hidden;text-overflow:ellipsis;white-space:nowrap}
.aw-ct{margin-left:auto;font-variant-numeric:tabular-nums;white-space:nowrap}
.aw-prog{position:absolute;left:0;bottom:0;height:3px;width:100%;background:var(--ctx);transform-origin:left;animation:aw-prog linear forwards}
@keyframes aw-prog{from{transform:scaleX(0)}to{transform:scaleX(1)}}
.aw-acts{display:flex;align-items:center;gap:10px;margin-top:12px;flex-wrap:wrap}
.aw-qs .lv-card-head{margin-bottom:6px}
.aw-q{display:flex;flex-direction:column;gap:8px;padding:10px 0;border-top:1px solid var(--line)}
.lv-card-head+.aw-q{border-top:0;padding-top:0}
.aw-qh{display:flex;align-items:center;gap:8px}
.aw-qt{font-weight:650;font-size:14.5px}
.aw-qc{color:var(--text-2)}
.aw-prec{display:flex;flex-direction:column;gap:4px;padding:8px 10px;border-radius:8px;background:color-mix(in srgb,var(--ctx) 7%,transparent)}
.aw-prec-h{font-size:10.5px;font-weight:650;text-transform:uppercase;letter-spacing:.05em;color:var(--muted)}
.aw-prec-r{display:flex;gap:7px;align-items:baseline;font-size:12.5px;color:var(--text-2)}.aw-prec-r .lv-ctx{flex:none}
.aw-opts{display:flex;flex-wrap:wrap;gap:6px}
.aw-match{margin-left:4px;color:inherit;opacity:.8;font-size:10px}
.aw-od{margin:0;padding-left:16px;color:var(--muted);font-size:12px}.aw-od b{color:var(--text-2);font-weight:600}
.aw-in{height:32px;border:1px solid var(--line-2);border-radius:8px;background:var(--sunken);padding:0 10px;outline:none;width:100%}
.aw-in:focus{border-color:var(--accent);box-shadow:0 0 0 3px var(--focus)}
.aw-rule{display:flex;gap:7px;align-items:center;font-size:12.5px;color:var(--text-2);cursor:pointer}
.aw-q.done{gap:4px}.aw-qa{display:flex;align-items:center;gap:8px;flex-wrap:wrap;font-weight:600}
.aw-now{display:flex;flex-direction:column;gap:8px}
.aw-now-r{display:grid;grid-template-columns:auto auto minmax(0,1fr);gap:4px 10px;align-items:center}
.aw-now-n{grid-column:1/-1;color:var(--muted);font-size:12.5px;padding-left:24px;margin-top:-2px}
@media (max-width:1240px){.aw-grid{grid-template-columns:minmax(0,1fr) 400px}}
@media (max-width:1000px){.aw-grid{grid-template-columns:1fr}.aw-tiles{grid-template-columns:repeat(2,minmax(0,1fr))}}
@media (max-width:640px){.aw-h1{font-size:21px}.aw-tn{font-size:28px}.aw-story{padding:16px}.aw-line{grid-template-columns:1fr;gap:0}.aw-time{line-height:1.4}.aw-line p{font-size:14px}.aw-reel{grid-template-columns:repeat(2,minmax(0,1fr))}}
`);
