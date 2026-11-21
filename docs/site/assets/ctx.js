/* ChloeCtx's pages: the sequences, the live window, the gate and the bars.
 * Everything here only moves what the HTML already shows: without scripts, or
 * with reduced motion, each figure stands at its final state. */
(function () {
  "use strict";
  var root = document.documentElement;
  root.classList.add("js");
  var still = window.matchMedia && window.matchMedia("(prefers-reduced-motion: reduce)").matches;
  var $$ = function (sel, el) { return Array.prototype.slice.call((el || document).querySelectorAll(sel)); };

  /* Run fn(el, visible) as each element enters or leaves the screen. */
  function watch(els, fn) {
    if (!("IntersectionObserver" in window)) { els.forEach(function (el) { fn(el, true); }); return; }
    var io = new IntersectionObserver(function (entries) {
      entries.forEach(function (e) { fn(e.target, e.isIntersecting); });
    }, { threshold: 0.25 });
    els.forEach(function (el) { io.observe(el); });
  }

  /* ---------- sequences: [data-step="n"] turn on in order, then start over ---------- */
  $$("[data-seq]").forEach(function (seq) {
    var steps = $$("[data-step]", seq);
    var max = steps.reduce(function (m, s) { return Math.max(m, +s.getAttribute("data-step")); }, 0);
    var every = +(seq.getAttribute("data-interval") || 1300);
    var hold = +(seq.getAttribute("data-hold") || 4);
    var at = -1, timer = null, paused = false;
    var btn = seq.parentNode.querySelector("[data-seq-toggle]") || seq.querySelector("[data-seq-toggle]");
    function show(n) {
      steps.forEach(function (s) { s.classList.toggle("on", +s.getAttribute("data-step") <= n); });
    }
    if (still) { show(max); if (btn) btn.hidden = true; return; }
    function tick() {
      at += 1;
      if (at > max + hold) at = 1;
      show(at);
    }
    function start() { if (!timer && !paused) { at = 1; show(1); timer = setInterval(tick, every); } }
    function stop() { clearInterval(timer); timer = null; show(max); }
    show(max);
    watch([seq], function (_, vis) { if (vis) start(); else stop(); });
    if (btn) btn.addEventListener("click", function () {
      if (paused) { paused = false; btn.textContent = "Pause"; start(); }
      else { paused = true; stop(); show(max); btn.textContent = "Play"; }
    });
  });

  /* ---------- a session's hooks, lit one after another ---------- */
  $$("[data-cycle]").forEach(function (list) {
    var items = $$(":scope > li", list);
    var cap = document.getElementById(list.getAttribute("data-cycle"));
    var i = -1, timer = null;
    function light(n) {
      items.forEach(function (li, k) { li.classList.toggle("lit", k === n); });
      if (cap && items[n]) cap.innerHTML = items[n].getAttribute("data-say") || "";
    }
    items.forEach(function (li, k) {
      li.addEventListener("mouseenter", function () { clearInterval(timer); timer = null; light(k); });
      li.addEventListener("mouseleave", function () { if (!still && !timer) timer = setInterval(next, 2200); });
    });
    function next() { i = (i + 1) % items.length; light(i); }
    if (still) { light(0); return; }
    watch([list], function (_, vis) {
      if (vis && !timer) { next(); timer = setInterval(next, 2200); }
      else if (!vis) { clearInterval(timer); timer = null; }
    });
  });

  /* ---------- the live window ---------- */
  $$("[data-lw]").forEach(function (box) {
    var row = box.querySelector(".lw-row");
    var cells = $$("i", row);
    var win = row.querySelector(".win");
    var cut = row.querySelector(".cut");
    var cap = box.querySelector(".lw-cap");
    var n = cells.length;
    var pct = function (k) { return (k / n * 100) + "%"; };
    var states = [
      { w: 10, cut: false, picks: [], say: "<b>A session is running.</b> Its records from the window start on are still in the model's context (faded). Older sessions' records are outside it." },
      { w: 10, cut: false, picks: [2, 6], say: "<b>A prompt arrives.</b> Recall searches only outside the window, so it never repeats what the model can already see." },
      { w: 18, cut: true, picks: [], say: "<b>Claude Code compacts.</b> The window restarts at the compaction: what the summary dropped is now outside it." },
      { w: 18, cut: true, picks: [12, 15], say: "<b>The next prompt</b> can bring back exact details the summary threw away, such as an error message or a number." }
    ];
    var s = -1, timer = null;
    function draw(k) {
      var st = states[k];
      cells.forEach(function (c, i) {
        c.classList.toggle("in", i >= st.w);
        c.classList.toggle("pick", st.picks.indexOf(i) >= 0);
      });
      win.style.left = pct(st.w);
      win.style.right = "0";
      win.textContent = "live window · in context";
      cut.style.left = "calc(" + pct(18) + " - 3px)";
      cut.classList.toggle("on", st.cut);
      cap.innerHTML = st.say;
    }
    function next() { s = (s + 1) % states.length; draw(s); }
    if (still) { draw(3); return; }
    draw(0);
    watch([box], function (_, vis) {
      if (vis && !timer) { timer = setInterval(next, 3200); }
      else if (!vis) { clearInterval(timer); timer = null; }
    });
  });

  /* ---------- the gate: drag the evidence, see what gets injected ---------- */
  $$("[data-gate]").forEach(function (g) {
    var input = g.querySelector("input");
    var tier = g.querySelector(".tier");
    var budget = g.querySelector(".budget");
    var items = $$(".gate-out li", g);
    function set() {
      var v = +input.value;
      var lo = 38, hi = 65;
      var full = v >= hi, brief = v >= lo && v < hi;
      tier.textContent = full ? "Full recall" : brief ? "Brief recall" : "Nothing injected";
      budget.textContent = full ? "up to 4,000 characters, best records first" : brief ? "three records, up to 1,200 characters, marked as a weak match" : "the evidence is too weak: the prompt goes through untouched";
      items.forEach(function (li, k) {
        li.classList.toggle("off", !(full || (brief && k < 3)));
        li.classList.toggle("weak", brief && k < 3);
      });
      input.setAttribute("aria-valuetext", tier.textContent);
    }
    input.addEventListener("input", set);
    set();
  });

  /* ---------- bars grow when seen; the compaction strikes its lost lines ---------- */
  watch($$(".hb, .squeeze"), function (el, vis) {
    if (!vis) return;
    if (el.classList.contains("hb")) el.classList.add("done");
    else el.classList.add("go");
  });

  /* ---------- SVG packets stop when motion is reduced ---------- */
  if (still) $$("svg").forEach(function (svg) { if (svg.pauseAnimations) svg.pauseAnimations(); });
})();
