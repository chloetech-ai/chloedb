// One request in the Simple view: where it is (Plan, Build, Review, Done,
// and Needs you on top), what you can do there, and the conversation, as
// Claude Code desktop shows one session. The buttons follow the stage:
// Tell while it plans or builds, Merge or Send back in Review, Follow up,
// Undo merge or Resume when it is done. The box at the bottom says its
// verb: Tell, Reply, Send back, Follow up. Beside the board on a desktop,
// the whole screen on a phone.
import { h, useStyle, btn, avatar, since, ago, md, toMs } from "/levels/common.js";
import { STEPS, modeWords } from "/goals/simple/stage.js";

const enc = encodeURIComponent;

/** `q`: the request (index.js requestOf); `o.close()`, `o.changed()`,
 *  `o.merge(q)`, `o.undo(q)`. */
export function requestPanel(ctx, q0, o) {
  useStyle("g-simple-request", CSS);
  let q = q0;
  let alive = true;
  const S = { story: null, storyAt: 0, storyKey: "", sending: false, merging: false, err: null, mode: null };
  const id = q.a.id;
  const head = h("div", { class: "sr-head" });
  const steps = h("ol", { class: "sr-steps", "aria-label": "Where it is" });
  const stage = h("div", { class: "sr-stage" });
  const talk = h("div", { class: "sr-talk", "aria-label": "Conversation", tabindex: "-1" });
  const box = h("textarea", { class: "field sr-box", rows: 2, "aria-label": "Message" });
  const send = h("button", { class: "btn primary sm", type: "submit" }, "Send");
  const how = h("p", { class: "sr-how" });
  const foot = h("form", { class: "sr-foot", onsubmit: (e) => { e.preventDefault(); sendIt(); } }, how, h("div", { class: "sr-frow" }, box, send));
  const el = h("div", { class: "sr" }, head, h("div", { class: "sr-scroll" }, steps, stage, talk), foot);

  box.addEventListener("keydown", (e) => {
    if (e.key === "Enter" && (e.metaKey || e.ctrlKey)) { e.preventDefault(); sendIt(); }
    if (e.key === "Escape") { e.stopPropagation(); box.blur(); }
  });
  box.addEventListener("input", () => { send.disabled = !box.value.trim() || S.sending; });

  // ------------------------------------------------------------ data

  async function loadStory(force) {
    const key = `${q.a.state}${q.a.running ? 1 : 0}${toMs(q.a.state_since) || 0}`;
    const working = q.a.running && q.a.state !== "idle";
    if (!force && key === S.storyKey && (!working || Date.now() - S.storyAt < 8000)) return;
    S.storyKey = key;
    S.storyAt = Date.now();
    try {
      const st = await ctx.api(`/api/agents/${enc(id)}/story`);
      if (!alive) return;
      S.story = st;
      drawTalk();
    } catch (e) { if (alive && !S.story) { talk.replaceChildren(h("p", { class: "sr-dim" }, `The conversation did not load: ${e.message}`)); } }
  }

  // ------------------------------------------------------------ drawing

  /** The box's verb, from the stage: what sending does. */
  function verb() {
    const a = q.a;
    if (S.mode === "back" || q.stage === "review") return { label: "Send back", how: `Back to ${a.name} in its worktree: it gets this as its next instruction, and the card goes back to Building.`, ph: "What should it change before you merge?" };
    if (q.stage === "done") return a.running ? { label: "Tell", how: `Typed into ${a.name}'s prompt.`, ph: `What should ${a.name} do next?` }
      : { label: "Follow up", how: `${a.name} starts again in the same worktree and conversation, with this as its next instruction.`, ph: "What else should it do?" };
    if (q.replied) return { label: "Reply", how: `Typed into ${a.name}'s prompt as its next instruction.`, ph: `Reply to ${a.name}` };
    if (!a.running) return { label: "Resume", how: `${a.name} ended. It starts again in its worktree with this as its next instruction.`, ph: "What should it do now?" };
    if (q.need || a.state === "waiting") return { label: "Tell", how: "It is asking you something: nothing is typed into that prompt. Your words reach it at its next tool call, once the prompt is answered.", ph: `Tell ${a.name}…` };
    if (a.state === "idle") return { label: "Tell", how: `Typed into ${a.name}'s prompt as its next instruction.`, ph: `What should ${a.name} do next?` };
    return { label: "Tell", how: "It is working: it gets this at its next tool call, or as its next prompt when this turn ends.", ph: `Tell ${a.name}…` };
  }

  function draw() {
    const a = q.a;
    const p = q.p;
    const mode = a.read_only ? "Read-only" : modeWords(a.permission_mode);
    head.replaceChildren(
      btn("", { ghost: true, sm: true, cls: "icon sr-x", onclick: () => o.close(), title: "Back to the board (Esc)", icon: "back" }),
      avatar(a, 26),
      h("div", { class: "sr-ht" },
        h("h2", {}, q.title),
        h("span", { class: "sr-meta num" }, [a.name, p && p.project, a.branch, mode, a.model].filter(Boolean).join(" · "))),
      h("a", { class: "lv-link sr-adv", href: `#/a/${a.id}`, title: "Its page in the Advanced view: progress, changes, terminal, events" }, "Open in Advanced"));

    // The stepper: done, here, to come; Needs you rides on the current step.
    const at = STEPS.findIndex(([k]) => k === q.stage);
    steps.replaceChildren(...STEPS.map(([k, t], i) => h("li", { class: i < at ? "past" : i === at ? `now${q.need || a.state === "waiting" ? " need" : ""}` : "", "aria-current": i === at ? "step" : null },
      h("i", {}, i < at ? "✓" : String(i + 1)), h("span", {}, t), i === at && (q.need || a.state === "waiting") ? h("b", {}, "needs you") : null)));

    stage.replaceChildren(...stageBlock());
    const v = verb();
    box.placeholder = v.ph;
    send.textContent = S.sending ? "Sending…" : v.label;
    send.disabled = !box.value.trim() || S.sending;
    how.textContent = v.how;
    foot.hidden = q.stage === "done" && !!q.merged && S.mode !== "follow";
  }

  function stageBlock() {
    const a = q.a;
    const out = [];
    if (q.need && q.need.kind === "waiting" && q.need.asked) out.push(h("div", { class: "sr-need sr-wait" }, h("b", {}, `${a.name} asks you`), h("p", {}, q.need.text), h("span", { class: "sr-dim" }, "Answer in the box below.")));
    else if (q.need) out.push(h("div", { class: "sr-need" }, ctx.needCard(q.need, { big: true })));
    else if (a.running && a.state === "waiting") out.push(h("div", { class: "sr-need sr-wait" }, h("b", {}, `${a.name} waits for you`), h("p", {}, a.narration || "Claude Code is showing a prompt Workbench has not read yet."), btn("Open its terminal", { sm: true, href: `#/a/${a.id}/terminal` })));
    const now = a.narration && a.running && a.state !== "idle" ? h("p", { class: "sr-now" }, h("i", { class: a.state === "working" || a.state === "starting" ? "on" : "" }), h("span", {}, a.narration), h("span", { class: "num sr-dim" }, ` · ${since(a.narration_ts || a.state_since)}`)) : null;
    const stopB = a.running ? btn("Stop", { sm: true, ghost: true, onclick: () => ctx.stop(a.id) }) : null;
    const tellB = (label) => btn(label, { sm: true, primary: true, onclick: () => { S.mode = null; draw(); focusBox(); } });
    if (q.stage === "planning") {
      out.push(block("Planning", q.replied ? "Its turn ended with nothing changed yet: it answered, or it asks you in words. Reply below." : a.permission_mode === "plan" ? "It reads and plans, and changes nothing until you approve its plan." : "It reads the code and plans. Nothing has changed in its worktree yet.",
        now, acts(q.need && !q.need.asked ? null : tellB(q.replied || (q.need && q.need.asked) ? "Reply" : `Tell ${a.name}`), btn("Watch", { sm: true, href: `#/a/${a.id}/terminal` }), stopB)));
    } else if (q.stage === "building") {
      out.push(block("Building", "It changes code in its own worktree. You review it when its turn ends.",
        q.stat ? h("div", { class: "sr-chips" }, chip(q.stat), a.branch ? h("code", { class: "sr-br" }, a.branch) : null) : null,
        now, acts(btn("Watch", { sm: true, primary: !q.need, href: `#/a/${a.id}/terminal` }), q.need ? null : btn(`Tell ${a.name}`, { sm: true, onclick: () => { S.mode = null; draw(); focusBox(); } }), btn("Changes", { sm: true, href: `#/a/${a.id}/changes` }), stopB)));
    } else if (q.stage === "review") {
      out.push(reviewBlock());
    } else {
      out.push(doneBlock());
    }
    if (S.err) out.push(h("p", { class: "sr-err", role: "alert" }, S.err));
    return out;
  }

  function reviewBlock() {
    const a = q.a;
    const r = q.ready;
    const p = q.p;
    if (!r) return block("Review", "Its turn ended with a change. Reading its checks…", acts(btn("Review in Advanced", { sm: true, href: p ? `#/p/${enc(p.project)}/ship/${a.id}` : `#/a/${a.id}/changes` })));
    const g = r.gate || {};
    const canMerge = g.can_merge && !g.anyway && g.primary === "merge";
    const unconf = (r.decisions && r.decisions.matter || []).filter((d) => d.why === "replaces" && d.confirmed !== true).length;
    const checks = h("ul", { class: "sr-checks" },
      h("li", { class: r.tests && r.tests.ok === true ? "ok" : r.tests && r.tests.failed ? "bad" : "" }, r.tests && r.tests.line ? r.tests.line : "No test run on its branch yet"),
      h("li", { class: r.merge && r.merge.ok ? "ok" : r.merge && r.merge.main && r.merge.main.clean === false ? "bad" : "" }, (r.merge && r.merge.line) || `Merges into ${r.onto || "main"}`),
      (r.uncommitted || []).length ? h("li", { class: "bad" }, `${r.uncommitted.length} file${r.uncommitted.length === 1 ? "" : "s"} not committed`) : null,
      unconf ? h("li", { class: "dec" }, `${unconf} decision${unconf === 1 ? "" : "s"} to confirm: it replaces an earlier one`) : null,
      r.risk && r.risk.line ? h("li", { class: r.risk.level === "high" ? "bad" : "" }, r.risk.line) : null);
    const ship = p ? `#/p/${enc(p.project)}/ship/${a.id}` : `#/a/${a.id}/changes`;
    return block("Review", canMerge ? "Every check passes. Merge it, or send it back with what to change." : g.label ? `${g.label}.` : "Check it before it merges.",
      r.said ? h("div", { class: "sr-said" }, md(r.said)) : null,
      h("div", { class: "sr-chips" }, chip(r), r.branch ? h("code", { class: "sr-br" }, r.branch) : null),
      checks,
      acts(canMerge ? btn(S.merging ? "Merging…" : `Merge into ${r.onto || "main"}`, { sm: true, primary: true, disabled: S.merging, onclick: () => merge(), cls: "sr-merge" }) : btn("Review in Advanced", { sm: true, primary: true, href: ship }),
        btn("Send back…", { sm: true, onclick: () => { S.mode = "back"; draw(); focusBox(); } }),
        btn("Changes", { sm: true, href: `#/a/${a.id}/changes` }),
        canMerge ? h("a", { class: "lv-link sr-pr", href: ship }, "Open a pull request instead") : null));
  }

  function doneBlock() {
    const a = q.a;
    const m = q.merged;
    if (m) {
      return block("Done", `Merged into ${m.onto || "main"}${m.commit ? ` as ${String(m.commit).slice(0, 7)}` : ""}, ${ago(m.ts || a.state_since)}.`,
        acts(btn("Follow up", { sm: true, primary: true, onclick: () => { S.mode = "follow"; draw(); focusBox(); } }),
          btn("Undo merge", { sm: true, onclick: () => undo() }),
          btn("Its story", { sm: true, href: `#/a/${a.id}` })));
    }
    const why = a.running ? "Its turn ended with nothing to merge." : a.exit_code != null && a.exit_code !== 0 ? `It ended (exit ${a.exit_code}) with nothing to merge.` : "It ended with nothing to merge.";
    return block("Done", `${why} ${a.ended ? `Ended ${ago(a.ended)}.` : ""}`.trim(),
      acts(btn(a.running ? `Tell ${a.name}` : "Follow up", { sm: true, primary: true, onclick: () => { S.mode = "follow"; draw(); focusBox(); } }),
        btn("Its story", { sm: true, href: `#/a/${a.id}` })));
  }

  /** The conversation: each turn's prompt, what it did in a line a step,
   *  and what it said at the end, newest at the bottom. */
  function drawTalk() {
    const st = S.story;
    if (!st) { talk.replaceChildren(h("p", { class: "sr-dim" }, "Reading the conversation…")); return; }
    const chs = st.chapters || [];
    const keepBottom = talk.scrollHeight - talk.scrollTop - talk.clientHeight < 40;
    if (!chs.length) {
      talk.replaceChildren(h("div", { class: "sr-msg you" }, h("div", { class: "sr-who" }, "you"), h("div", { class: "sr-bub" }, md(q.a.task || "Started at its prompt."))),
        h("p", { class: "sr-dim" }, q.a.running ? "It has not started its first turn yet." : "No turn was recorded."));
      return;
    }
    const shown = chs.slice(-12);
    talk.replaceChildren(...[
      chs.length > shown.length ? h("p", { class: "sr-dim" }, `${chs.length - shown.length} earlier turns in `, h("a", { href: `#/a/${q.a.id}` }, "its story")) : null,
      ...shown.flatMap((c) => {
        const by = c.prompt && c.prompt.by && c.prompt.by !== "you" ? c.prompt.by : "you";
        const stepsL = (c.steps || []).slice(-6);
        return [
          c.prompt && c.prompt.text ? h("div", { class: `sr-msg ${by === "you" ? "you" : "peer"}` }, h("div", { class: "sr-who" }, by), h("div", { class: "sr-bub" }, md(clipText(c.prompt.text, 900)))) : null,
          stepsL.length ? h("ul", { class: "sr-tools" }, (c.steps || []).length > stepsL.length ? h("li", { class: "sr-dim" }, `${c.steps.length - stepsL.length} earlier steps`) : null,
            stepsL.map((s) => h("li", { class: s.failed ? "bad" : s.running ? "run" : "" }, h("b", {}, s.label), s.detail && !String(s.label).includes(s.detail) ? h("span", {}, ` ${s.detail}`) : null))) : null,
          c.summary ? h("div", { class: "sr-msg it" }, h("div", { class: "sr-who" }, q.a.name), h("div", { class: "sr-bub" }, md(c.summary))) : c.state === "now" ? h("p", { class: "sr-dim sr-wip" }, h("i"), q.a.narration || "Working…") : null,
        ];
      })].filter(Boolean));
    if (keepBottom || !S.drawnOnce) { talk.scrollTop = talk.scrollHeight; S.drawnOnce = true; }
  }

  // ------------------------------------------------------------ actions

  function focusBox() { requestAnimationFrame(() => { box.focus(); box.scrollIntoView({ block: "nearest" }); }); }

  async function sendIt() {
    const text = box.value.trim();
    if (!text || S.sending) { box.focus(); return; }
    const a = q.a;
    S.sending = true; S.err = null; draw();
    try {
      if (a.running) {
        try {
          const r = await ctx.api(`/api/agents/${enc(a.id)}/steer`, { method: "POST", body: { text } });
          ctx.toast(r && r.message ? r.message : r && r.delivered === "typed" ? `Typed into ${a.name}'s prompt.` : `Queued: ${a.name} gets it at its next tool call.`);
        } catch (e) {
          if (e.status !== 409) throw e;
          await restartWith(text);
        }
      } else {
        await restartWith(text);
      }
      box.value = "";
      S.mode = null;
      o.changed();
    } catch (e) { S.err = `Not sent: ${e.message}`; }
    S.sending = false;
    if (alive) { draw(); loadStory(true); }
  }

  async function restartWith(text) {
    const r = await ctx.api(`/api/agents/${enc(q.a.id)}/restart`, { method: "POST", body: { prompt: text } });
    const na = r && (r.agent || r);
    ctx.toast(`${q.a.name} starts again with your words.`);
    if (na && na.id && na.id !== q.a.id) ctx.go(`#/s/r/${na.id}`);
  }

  // Merge and Undo are the board's (index.js), so the card moves at once.
  async function merge() {
    if (S.merging) return;
    S.merging = true; S.err = null; draw();
    try { await o.merge(q); } finally { S.merging = false; if (alive) draw(); }
  }
  async function undo() { await o.undo(q); }

  const block = (title, sub, ...kids) => h("section", { class: `sr-blk sr-${q.stage}` }, h("div", { class: "lbl" }, title), sub ? h("p", { class: "sr-sub" }, sub) : null, ...kids.filter(Boolean));
  const acts = (...k) => { const x = k.filter(Boolean); return x.length ? h("div", { class: "sr-acts" }, x) : null; };

  draw();
  drawTalk();
  loadStory(true);
  const qy = new URLSearchParams((location.hash.split("?")[1] || ""));
  if (qy.get("tell")) focusBox();

  return {
    id,
    el,
    update(nq) {
      if (!alive || !nq) return;
      const was = q;
      q = nq;
      // Never under a field being typed in, or an answer being picked.
      const act = document.activeElement;
      const typing = act && el.contains(act) && act.matches("input, textarea") && act !== box;
      if (!typing && !(stage.querySelector(".qc-opt.sel"))) draw();
      else { const v = verb(); send.textContent = S.sending ? "Sending…" : v.label; }
      if (was.a.state !== q.a.state || was.a.running !== q.a.running) loadStory(true);
    },
    tick() { if (alive) loadStory(false); },
    onKey(e) {
      if (e.key === "Escape") { o.close(); return true; }
      if (e.key === "r" || e.key === "t") { S.mode = null; draw(); focusBox(); return true; }
      if (e.key === "Enter" && (e.metaKey || e.ctrlKey)) { const m = el.querySelector(".sr-merge"); if (m) { m.click(); return true; } }
      return false;
    },
    unmount() { alive = false; el.remove(); },
  };
}

function chip(s) {
  if (!s || !(s.added || s.removed || s.files)) return null;
  return h("span", { class: "sv-chip num" }, h("span", { class: "add" }, `+${s.added || 0}`), " ", h("span", { class: "del" }, `−${s.removed || 0}`), s.files ? ` · ${s.files} file${s.files === 1 ? "" : "s"}` : "");
}
function clipText(t, n) { const s = String(t || ""); return s.length > n ? `${s.slice(0, n - 1)}…` : s; }

const CSS = `
.sr{display:flex;flex-direction:column;height:100%;min-height:0}
.sr-head{display:flex;align-items:flex-start;gap:var(--s-2);padding:var(--s-3) var(--s-4);border-bottom:var(--hair) solid var(--line)}
.sr-head .sr-x{flex:none;margin:-2px 0 0 -6px}
.sr-head .lv-av{margin-top:2px;flex:none}
.sr-ht{display:flex;flex-direction:column;gap:2px;min-width:0;flex:1}
.sr-ht h2{margin:0;font:var(--w-title) var(--fs-md)/var(--lh-md) var(--sans);letter-spacing:var(--track-md);overflow-wrap:anywhere}
.sr-meta{font:var(--w-text) var(--fs-xs)/var(--lh-xs) var(--mono);color:var(--text-3);overflow-wrap:anywhere}
.sr-adv{flex:none;font-size:var(--fs-sm);margin-top:3px}
.sr-scroll{flex:1;min-height:0;overflow-y:auto;display:flex;flex-direction:column;gap:var(--s-4);padding:var(--s-4)}
.sr-steps{list-style:none;margin:0;padding:0;display:grid;grid-template-columns:repeat(4,minmax(0,1fr));gap:4px}
.sr-steps li{display:flex;align-items:center;gap:6px;padding:6px 8px;border-radius:var(--r-sm);background:var(--surface-2);color:var(--text-3);font-size:var(--fs-sm);min-width:0;flex-wrap:wrap}
.sr-steps li i{font:var(--w-mid) var(--fs-xs)/16px var(--mono);font-style:normal;width:16px;height:16px;border-radius:50%;display:inline-grid;place-items:center;border:var(--hair) solid var(--line-2);flex:none}
.sr-steps li.past{color:var(--text-2)}.sr-steps li.past i{color:var(--good-ink);border-color:transparent}
.sr-steps li.now{background:var(--text);color:var(--surface);font-weight:var(--w-mid)}.sr-steps li.now i{border-color:currentColor}
.sr-steps li.now.need{background:var(--accent);color:var(--accent-ink)}
.sr-steps li b{font:var(--w-mid) var(--fs-xs)/1 var(--mono);text-transform:uppercase;letter-spacing:.06em}
.sr-blk{display:flex;flex-direction:column;gap:var(--s-2)}
.sr-sub{margin:0;color:var(--text-2);font-size:var(--fs-sm);line-height:var(--lh-sm)}
.sr-now{margin:0;display:flex;align-items:baseline;gap:6px;font-size:var(--fs-sm);line-height:var(--lh-sm);color:var(--text)}
.sr-now i,.sr-wip i{width:7px;height:7px;border-radius:50%;background:var(--text-3);flex:none;display:inline-block}
.sr-now i.on,.sr-wip i{background:var(--good);animation:pulse 1.6s ease-in-out infinite}
.sr-dim{color:var(--text-3);font-size:var(--fs-sm);margin:0}
.sr-wip{display:flex;align-items:baseline;gap:6px}
.sr-chips{display:flex;gap:6px;flex-wrap:wrap;align-items:center}
.sr-br{font:var(--w-text) var(--fs-xs)/20px var(--mono);color:var(--text-2);overflow-wrap:anywhere}
.sr-checks{list-style:none;margin:0;padding:var(--s-2) var(--s-3);display:flex;flex-direction:column;gap:4px;font-size:var(--fs-sm);line-height:var(--lh-sm);background:var(--surface-2);border-radius:var(--r-sm)}
.sr-checks li{display:flex;gap:6px}
.sr-checks li::before{content:"·";width:12px;flex:none;text-align:center}
.sr-checks li.ok::before{content:"✓";color:var(--good-ink)}
.sr-checks li.bad::before{content:"!";color:var(--warn-ink);font-weight:var(--w-title)}
.sr-checks li.dec{color:var(--accent-text)}.sr-checks li.dec::before{content:"◆"}
.sr-said{font-size:var(--fs-sm);line-height:var(--lh-sm);color:var(--text)}
.sr-said p{margin:0 0 var(--s-1)}
.sr-acts{display:flex;gap:6px;flex-wrap:wrap;align-items:center}
.sr-pr{font-size:var(--fs-sm);margin-left:var(--s-1)}
.sr-err{margin:0;color:var(--crit-ink);font-size:var(--fs-sm)}
.sr-need .qc{box-shadow:0 0 0 3px var(--accent-soft);border-color:var(--accent-line)}
.sr-wait{display:flex;flex-direction:column;gap:var(--s-1);padding:var(--s-3);border-radius:var(--r);background:var(--accent-soft)}
.sr-wait p{margin:0;font-size:var(--fs-sm)}
.sr-talk{display:flex;flex-direction:column;gap:var(--s-2);padding-top:var(--s-3);border-top:var(--hair) solid var(--line);outline:0}
.sr-msg{display:flex;flex-direction:column;gap:2px;max-width:92%}
.sr-msg.you{align-self:flex-end;align-items:flex-end}
.sr-who{font:var(--w-mid) var(--fs-xs)/var(--lh-xs) var(--mono);color:var(--text-3)}
.sr-bub{padding:var(--s-2) var(--s-3);border-radius:var(--r);background:var(--surface-2);font-size:var(--fs-sm);line-height:var(--lh-sm);overflow-wrap:anywhere;min-width:0;max-width:100%}
.sr-bub p{margin:0 0 var(--s-1)}.sr-bub p:last-child{margin:0}
.sr-bub pre{white-space:pre-wrap;overflow-wrap:anywhere;margin:var(--s-1) 0}
.sr-msg.you .sr-bub{background:var(--accent-soft)}
.sr-msg.it .sr-bub{background:transparent;border:var(--hair) solid var(--line);padding:var(--s-2) var(--s-3)}
.sr-tools{list-style:none;margin:0;padding:0 0 0 var(--s-2);border-left:2px solid var(--line);display:flex;flex-direction:column;gap:2px;font-size:var(--fs-xs);line-height:var(--lh-xs);color:var(--text-2)}
.sr-tools li{overflow:hidden;text-overflow:ellipsis;white-space:nowrap}
.sr-tools li b{font-weight:var(--w-mid);color:var(--text)}
.sr-tools li.bad b{color:var(--warn-ink)}
.sr-tools li.run b::after{content:" …"}
.sr-foot{border-top:var(--hair) solid var(--line);padding:var(--s-3) var(--s-4) calc(var(--s-3) + env(safe-area-inset-bottom));display:flex;flex-direction:column;gap:6px;background:var(--surface)}
.sr-foot[hidden]{display:none}
.sr-how{margin:0;font-size:var(--fs-xs);line-height:var(--lh-xs);color:var(--text-3)}
.sr-frow{display:flex;gap:var(--s-2);align-items:flex-end}
.sr .sr-box{flex:1;min-height:44px;height:64px;max-height:180px;resize:vertical}
@media (max-width:640px){
  .sr{height:auto;min-height:calc(100vh - var(--top) - var(--tabh) - env(safe-area-inset-top) - env(safe-area-inset-bottom))}
  .sr-head{padding:var(--s-3) var(--s-4);position:sticky;top:0;background:var(--bg);z-index:2}
  .sr-adv,.sr-head .sr-x{display:none}
  .sr-scroll{overflow:visible;padding:var(--s-3) var(--s-4)}
  .sr-steps li{justify-content:center;padding:6px 4px}
  .sr-steps li span{display:none}
  .sr-steps li.now span{display:inline}
  .sr-steps li.now{grid-column:span 1}
  .sr-steps li b{display:none}
  .sr-acts .btn{height:var(--touch);flex:1 1 auto}
  .sr-foot{position:sticky;bottom:0;padding:var(--s-2) var(--s-4)}
  .sr-frow .btn{height:var(--touch)}
}
`;
