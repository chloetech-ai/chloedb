/* ChloeDB showcase site: small simulators that run in the browser.
 *
 *   data-sim="rank"      BM25 against vector similarity over 40 sample
 *                        products, fused with minmax(), as you type
 *   data-sim="cluster"   a coordinator, shards and copies: a search fans
 *                        out, a write is routed by its slot, a primary
 *                        fails over, a new shard takes slots
 *   data-sim="maxscore"  BM25 top-k with list and block bounds: what is
 *                        scored, what is skipped
 *   data-sim="hnsw"      a search walking down the layers of an HNSW graph
 *   data-sim="merge"     commits, segments and a tiered merge policy
 *
 * Each is a sketch of the mechanism, on made-up data, and says so. None
 * needs a library. With prefers-reduced-motion, nothing plays by itself
 * and every step lands without animation. */
(function () {
  "use strict";
  var mq = window.matchMedia ? window.matchMedia("(prefers-reduced-motion: reduce)") : null;
  var REDUCED = !!(mq && mq.matches);
  var $$ = function (sel, r) { return Array.prototype.slice.call((r || document).querySelectorAll(sel)); };
  var esc = function (s) { return String(s).replace(/[&<>"]/g, function (c) { return { "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;" }[c]; }); };
  var NS = "http://www.w3.org/2000/svg";
  function el(tag, attrs, html) {
    var e = document.createElement(tag);
    for (var k in attrs || {}) e.setAttribute(k, attrs[k]);
    if (html != null) e.innerHTML = html;
    return e;
  }
  function sv(tag, attrs, parent) {
    var e = document.createElementNS(NS, tag);
    for (var k in attrs || {}) e.setAttribute(k, attrs[k]);
    if (parent) parent.appendChild(e);
    return e;
  }
  function rng(seed) { // mulberry32
    return function () { seed |= 0; seed = (seed + 0x6d2b79f5) | 0; var t = Math.imul(seed ^ (seed >>> 15), 1 | seed); t = (t + Math.imul(t ^ (t >>> 7), 61 | t)) ^ t; return ((t ^ (t >>> 14)) >>> 0) / 4294967296; };
  }
  function cssVar(name, fallback) { var v = getComputedStyle(document.documentElement).getPropertyValue(name).trim(); return v || fallback; }
  // run on() while the element is on screen, off() when it leaves
  function onScreen(node, on, off, threshold) {
    if (!("IntersectionObserver" in window)) { on(); return; }
    new IntersectionObserver(function (es) { if (es[0].isIntersecting) on(); else if (off) off(); }, { threshold: threshold == null ? 0.25 : threshold }).observe(node);
  }
  var wait = function (ms) { return new Promise(function (r) { setTimeout(r, REDUCED ? 0 : ms); }); };
  var fmt = function (v, d) { return v.toLocaleString("en-US", { minimumFractionDigits: d || 0, maximumFractionDigits: d || 0 }); };

  /* ================================================================ rank */
  // [title, brand, price, description, meaning]
  var CATALOG = [
    ["Wireless over-ear headphones", "Northpeak", 129, "noise cancelling, 30 hour battery, folds flat for travel", { audio: 1, quiet: 0.7, travel: 0.45 }],
    ["Wireless earbuds with charging case", "Tideline", 79, "sweat resistant buds for running and the gym", { audio: 1, fitness: 0.55, travel: 0.3 }],
    ["Studio monitor headphones, wired", "Calmhaus", 99, "flat sound for mixing and long sessions at the desk", { audio: 1, desk: 0.4 }],
    ["Portable bluetooth speaker", "Tideline", 59, "waterproof speaker that floats, 12 hours of music outdoors", { audio: 0.9, outdoor: 0.5, rain: 0.45 }],
    ["Wireless gaming headset", "Pixelcraft", 109, "low latency headset with a boom mic for console and PC games", { gaming: 1, audio: 0.75 }],
    ["Wireless game controller", "Pixelcraft", 59, "rechargeable gamepad with rumble, pairs with PC and console", { gaming: 1 }],
    ["Mechanical keyboard, silent switches", "Copperleaf", 119, "quiet linear switches for the office and late night typing", { desk: 1, quiet: 0.8, gaming: 0.3 }],
    ["Clicky mechanical keyboard, RGB", "Pixelcraft", 89, "loud blue switches and lights for gaming setups", { gaming: 0.9, desk: 0.5 }],
    ["Wireless ergonomic mouse", "Copperleaf", 49, "vertical grip mouse for long work days", { desk: 1 }],
    ["Gaming mouse, 26K sensor", "Pixelcraft", 69, "light mouse with side buttons for shooters", { gaming: 1, desk: 0.2 }],
    ["Waterproof rain jacket", "Northpeak", 149, "breathable shell with taped seams for wet hikes", { rain: 1, outdoor: 0.85 }],
    ["Packable rain poncho", "Trailwise", 25, "light poncho that covers you and your backpack in a storm", { rain: 1, outdoor: 0.6, travel: 0.3 }],
    ["Waterproof hiking boots", "Summitry", 159, "leather boots with grip for muddy trails and creek crossings", { outdoor: 1, rain: 0.7 }],
    ["Trail running shoes", "Stridely", 129, "light shoes with lugs for running on dirt and rock", { fitness: 0.9, outdoor: 0.7 }],
    ["Road running shoes", "Stridely", 119, "cushioned shoes for daily miles and a marathon", { fitness: 1 }],
    ["Two person backpacking tent", "Trailwise", 229, "three season tent that pitches in minutes, stays dry in rain", { outdoor: 1, rain: 0.5, travel: 0.3 }],
    ["Hiking daypack, 24 litres", "Summitry", 89, "backpack with a rain cover and a hydration sleeve", { outdoor: 0.9, rain: 0.4, travel: 0.4 }],
    ["Insulated water bottle", "Trailwise", 32, "keeps drinks cold for a day on the trail or at the gym", { outdoor: 0.6, fitness: 0.5, travel: 0.3 }],
    ["Espresso machine with milk frother", "Saltgrain", 349, "fifteen bar pump for espresso and lattes at home", { coffee: 1 }],
    ["Burr coffee grinder", "Saltgrain", 129, "forty grind settings from espresso to cold brew", { coffee: 1 }],
    ["Pour-over coffee kettle", "Oakhaven", 59, "gooseneck kettle that holds the temperature for tea and coffee", { coffee: 0.95 }],
    ["Travel coffee mug", "Oakhaven", 28, "leak proof mug for the commute, keeps coffee hot", { coffee: 0.7, travel: 0.7 }],
    ["Standing desk, electric", "Copperleaf", 449, "two motor desk with memory presets for a home office", { desk: 1, fitness: 0.25 }],
    ["Desk lamp with wireless charger", "Oakhaven", 69, "warm light for late work, charges your phone", { desk: 0.9, sleep: 0.2 }],
    ["Noise cancelling earplugs", "Calmhaus", 35, "soft plugs for sleep, flights and open offices", { quiet: 1, sleep: 0.8, travel: 0.5 }],
    ["White noise machine", "Calmhaus", 45, "thirty soothing sounds to fall asleep and stay asleep", { sleep: 1, quiet: 0.8, audio: 0.3 }],
    ["Weighted blanket", "Oakhaven", 119, "seven kilo blanket for calmer nights", { sleep: 1, quiet: 0.4 }],
    ["Memory foam travel pillow", "Tideline", 29, "neck pillow for long flights and train rides", { travel: 1, sleep: 0.7 }],
    ["Carry-on suitcase, hard shell", "Northpeak", 199, "spinner wheels and a laptop pocket, fits in the overhead bin", { travel: 1, desk: 0.2 }],
    ["Yoga mat, non-slip", "Stridely", 39, "six millimetre mat for yoga and floor workouts", { fitness: 1, quiet: 0.3 }],
    ["Adjustable dumbbells", "Stridely", 299, "one pair from two to twenty four kilos for home workouts", { fitness: 1 }],
    ["Fitness tracker watch", "Tideline", 99, "heart rate, sleep and running pace, waterproof to fifty metres", { fitness: 0.9, sleep: 0.4, rain: 0.2 }],
    ["Kids' tablet with case", "Pixelcraft", 109, "a tablet for children with games and parental controls", { kids: 1, gaming: 0.5 }],
    ["Building blocks set, 900 pieces", "Ridgeline Co", 79, "a castle to build, a birthday gift for ages eight and up", { kids: 1 }],
    ["Family board game", "Ridgeline Co", 34, "a strategy game for two to six players, forty minutes", { kids: 0.8, gaming: 0.6 }],
    ["Kids' rain boots", "Summitry", 35, "rubber boots for puddles, easy to pull on", { kids: 0.9, rain: 0.9 }],
    ["Retro handheld game console", "Pixelcraft", 89, "four hundred classic games in your pocket for trips", { gaming: 1, travel: 0.5, kids: 0.3 }],
    ["Camping stove", "Trailwise", 49, "compact stove for boiling water and coffee at camp", { outdoor: 1, coffee: 0.5 }],
    ["Headlamp, 400 lumens", "Summitry", 29, "rechargeable light for night hikes and camping", { outdoor: 0.9, sleep: 0.1 }],
    ["Umbrella, wind resistant", "Northpeak", 32, "vented canopy that does not flip in a storm", { rain: 1, travel: 0.4 }]
  ];
  var DIMS = ["audio", "gaming", "outdoor", "rain", "coffee", "desk", "fitness", "kids", "travel", "sleep", "quiet"];
  // words people search with that the catalog never uses, mapped to meaning
  var MEAN = {
    music: { audio: 1 }, listen: { audio: 1 }, listening: { audio: 1 }, song: { audio: 1 }, podcast: { audio: 0.9 }, sound: { audio: 0.8 }, bass: { audio: 0.8 },
    gamer: { gaming: 1 }, play: { gaming: 0.6, kids: 0.4 }, esports: { gaming: 1 }, xbox: { gaming: 1 }, playstation: { gaming: 1 },
    hike: { outdoor: 1 }, mountain: { outdoor: 0.9 }, camp: { outdoor: 1 }, outdoors: { outdoor: 1 }, forest: { outdoor: 0.8 }, nature: { outdoor: 0.8 },
    rainy: { rain: 1 }, wet: { rain: 0.9 }, puddle: { rain: 0.7 }, drizzle: { rain: 0.9 }, weather: { rain: 0.6, outdoor: 0.3 },
    caffeine: { coffee: 1 }, latte: { coffee: 1 }, brew: { coffee: 1 }, morning: { coffee: 0.6 }, barista: { coffee: 1 },
    work: { desk: 0.9 }, typing: { desk: 0.9 }, productive: { desk: 0.7 }, focus: { desk: 0.6, quiet: 0.4 }, coding: { desk: 0.8 }, programmer: { desk: 0.8 },
    gym: { fitness: 1 }, exercise: { fitness: 1 }, run: { fitness: 1 }, jog: { fitness: 1 }, healthy: { fitness: 0.7 }, training: { fitness: 0.9 },
    child: { kids: 1 }, toddler: { kids: 1 }, gift: { kids: 0.6 }, present: { kids: 0.5 }, son: { kids: 1 }, daughter: { kids: 1 },
    trip: { travel: 1 }, vacation: { travel: 0.9 }, airport: { travel: 0.9 }, plane: { travel: 0.9 }, holiday: { travel: 0.8 },
    night: { sleep: 0.8 }, bed: { sleep: 0.9 }, nap: { sleep: 0.9 }, relax: { sleep: 0.6, quiet: 0.4 }, insomnia: { sleep: 1 }, tired: { sleep: 0.8 },
    silent: { quiet: 1 }, calm: { quiet: 0.8 }, peaceful: { quiet: 0.8 }, noisy: { quiet: 0.7 }
  };
  var STOP = new Set("a an and the for of to in on with at by from or my me i is it that this something some thing things good best".split(" "));
  function stem(w) { return w.length > 3 && /s$/.test(w) && !/ss$/.test(w) ? w.replace(/(ies)$/, "y").replace(/s$/, "") : w; }
  function rawWords(s) { return (s.toLowerCase().replace(/'s\b/g, "").match(/[a-z0-9]+(?:-[a-z0-9]+)*/g) || []).filter(function (w) { return !STOP.has(w) && !STOP.has(stem(w)); }); }
  function words(s) { return rawWords(s).map(stem); }
  function lev(a, b, max) {
    if (Math.abs(a.length - b.length) > max) return max + 1;
    var prev = [], cur = [], i, j;
    for (j = 0; j <= b.length; j++) prev[j] = j;
    for (i = 1; i <= a.length; i++) {
      cur = [i]; var best = i;
      for (j = 1; j <= b.length; j++) {
        cur[j] = Math.min(prev[j] + 1, cur[j - 1] + 1, prev[j - 1] + (a[i - 1] === b[j - 1] ? 0 : 1));
        if (i > 1 && j > 1 && a[i - 1] === b[j - 2] && a[i - 2] === b[j - 1]) cur[j] = Math.min(cur[j], prev2[j - 2] + 1);
        if (cur[j] < best) best = cur[j];
      }
      if (best > max) return max + 1;
      var prev2 = prev; prev = cur;
    }
    return prev[b.length];
  }
  function unit(v) { var n = Math.sqrt(v.reduce(function (s, x) { return s + x * x; }, 0)); return n ? v.map(function (x) { return x / n; }) : v; }
  function vecOf(o) { return DIMS.map(function (d) { return o[d] || 0; }); }

  function rank(root) {
    var docs = CATALOG.map(function (r, i) {
      var toks = words(r[0] + " " + r[3]);
      var tf = {}; toks.forEach(function (t) { tf[t] = (tf[t] || 0) + 1; });
      return { id: "p" + (i + 1), title: r[0], brand: r[1], price: r[2], len: toks.length, tf: tf, vec: unit(vecOf(r[4])) };
    });
    var N = docs.length, avgdl = docs.reduce(function (s, d) { return s + d.len; }, 0) / N;
    var df = {};
    docs.forEach(function (d) { Object.keys(d.tf).forEach(function (t) { df[t] = (df[t] || 0) + 1; }); });
    var vocab = Object.keys(df);
    // a catalog word means what the products using it mean (co-occurrence),
    // and the hand-made words above add what the catalog never says
    var wordVec = {};
    vocab.forEach(function (t) {
      var v = DIMS.map(function () { return 0; });
      docs.forEach(function (d) { if (d.tf[t]) d.vec.forEach(function (x, i) { v[i] += x; }); });
      wordVec[t] = unit(v);
    });
    Object.keys(MEAN).forEach(function (w) { var v = unit(vecOf(MEAN[w])); wordVec[stem(w)] = wordVec[stem(w)] ? unit(wordVec[stem(w)].map(function (x, i) { return x + v[i]; })) : v; });

    var K1 = 1.2, B = 0.75, LIMIT = 8;
    var state = { q: "wireles headphones", w: 0.5, typo: true, per: false };
    root.innerHTML =
      '<div class="rk-controls">' +
      '<label class="rk-search"><svg viewBox="0 0 24 24" aria-hidden="true"><circle cx="10.5" cy="10.5" r="6.5"/><path d="M15.5 15.5 21 21"/></svg><span class="sr">Search the sample products</span><input type="search" autocomplete="off" spellcheck="false" placeholder="Search 40 sample products"></label>' +
      '<div class="rk-try"><span>Try</span></div>' +
      '<div class="rk-knobs">' +
      '<label class="rk-slider"><span class="lo">Words <b class="wv"></b></span><input type="range" min="0" max="100" step="5" aria-label="Weight of words against meaning"><span class="hi"><b class="mv"></b> Meaning</span></label>' +
      '<label class="rk-check"><input type="checkbox" class="typo"> Typos <code>word~</code></label>' +
      '<label class="rk-check"><input type="checkbox" class="per"> <code>MAX 2 PER brand</code></label>' +
      "</div></div>" +
      '<div class="rk-body"><ol class="rk-list" aria-live="polite"></ol><div class="rk-side"><div class="code"><div class="code-head"><span>The statement</span></div><pre class="rk-sql"></pre></div><p class="rk-why"></p></div></div>';
    var input = root.querySelector("input[type=search]"), slider = root.querySelector("input[type=range]");
    var typo = root.querySelector(".typo"), per = root.querySelector(".per"), list = root.querySelector(".rk-list");
    var sqlEl = root.querySelector(".rk-sql"), why = root.querySelector(".rk-why");
    ["wireles headphones", "something for a rainy hike", "a gift for a gamer", "quiet keyboard for work", "coffee", "can't sleep on flights"].forEach(function (t) {
      var b = el("button", { type: "button", class: "chip" }, esc(t));
      b.addEventListener("click", function () { input.value = t; state.q = t; touched(); run(); });
      root.querySelector(".rk-try").appendChild(b);
    });
    input.value = state.q; slider.value = (1 - state.w) * 100; typo.checked = state.typo; per.checked = state.per;

    function expand(t, raw) {
      // word~: the terms within 1 edit up to 5 letters, 2 beyond (none up to 2);
      // a word the catalog doesn't know is measured as typed ("wireles", not "wirele")
      if (!state.typo) return df[t] ? [{ term: t, d: 0 }] : [];
      var base = df[t] ? t : raw, max = base.length <= 2 ? 0 : base.length <= 5 ? 1 : 2, out = [];
      vocab.forEach(function (v) { if (v[0] === base[0] || max > 1) { var d = lev(base, v, max); if (d <= max) out.push({ term: v, d: d }); } });
      return out;
    }
    function score() {
      var qw = rawWords(state.q), qv = DIMS.map(function () { return 0; }), seen = {}, groups = [];
      qw.forEach(function (raw) {
        var t = stem(raw);
        if (seen[t]) return; seen[t] = 1;
        var ex = expand(t, raw);
        if (ex.length) groups.push({ word: raw, terms: ex });
        ex.sort(function (a, b) { return a.d - b.d; });
        var wv = wordVec[t] || (ex.length ? wordVec[ex[0].term] : null);
        if (wv) wv.forEach(function (x, i) { qv[i] += x; });
      });
      qv = unit(qv);
      var hasV = qv.some(function (x) { return x; });
      var rows = docs.map(function (d) {
        var s = 0, hit = [];
        groups.forEach(function (g) {
          var tf = 0, dfg = 0;
          g.terms.forEach(function (e) { if (d.tf[e.term]) { tf += d.tf[e.term]; hit.push(e.d ? e.term + "~" : e.term); } });
          docs.forEach(function (x) { if (g.terms.some(function (e) { return x.tf[e.term]; })) dfg++; });
          if (!tf) return;
          var idf = Math.log(1 + (N - dfg + 0.5) / (dfg + 0.5));
          s += idf * tf * (K1 + 1) / (tf + K1 * (1 - B + B * d.len / avgdl));
        });
        var sim = hasV ? d.vec.reduce(function (a, x, i) { return a + x * qv[i]; }, 0) : null;
        return { d: d, bm: s > 0 ? s : null, sim: sim, hit: hit };
      });
      // minmax() over the candidates: a row without a value takes 0
      function mm(key) {
        var vals = rows.filter(function (r) { return r[key] != null; }).map(function (r) { return r[key]; });
        if (!vals.length) return function () { return 0; };
        var lo = Math.min.apply(null, vals), hi = Math.max.apply(null, vals);
        return function (v) { return v == null ? 0 : hi > lo ? (v - lo) / (hi - lo) : 1; };
      }
      var mb = mm("bm"), ms = mm("sim");
      rows.forEach(function (r) { r.wpart = state.w * mb(r.bm); r.mpart = hasV ? (1 - state.w) * ms(r.sim) : 0; r.total = r.wpart + r.mpart; });
      rows.sort(function (a, b) { return b.total - a.total || a.d.price - b.d.price; });
      var brands = {}, out = [];
      rows.forEach(function (r) {
        if (out.length >= LIMIT || r.total <= 0) return;
        if (state.per) { brands[r.d.brand] = (brands[r.d.brand] || 0) + 1; if (brands[r.d.brand] > 2) return; }
        out.push(r);
      });
      return { rows: out, groups: groups, hasV: hasV };
    }
    function sql(res) {
      var qtext = res.groups.map(function (g) { return g.word + (state.typo && g.word.length > 2 ? "~" : ""); }).join(" ") || state.q.trim();
      var w = state.w, lines = [];
      lines.push('<span class="tok-kw">SELECT</span> id, title, brand, price, <span class="tok-fn">score</span>()');
      lines.push('<span class="tok-kw">FROM</span> products');
      var parts = [];
      if (w > 0) parts.push((w < 1 ? '<span class="tok-num">' + w.toFixed(2) + "</span> * " : "") + '<span class="tok-fn">minmax</span>(<span class="tok-fn">bm25</span>(body, <span class="tok-par">:q</span>))');
      if (w < 1) parts.push((w > 0 ? '<span class="tok-num">' + (1 - w).toFixed(2) + "</span> * " : "") + '<span class="tok-fn">minmax</span>(<span class="tok-fn">similarity</span>(emb, <span class="tok-par">:v</span>))');
      lines.push('<span class="tok-kw">ORDER BY</span> ' + parts.join("\n       + ") + ' <span class="tok-kw">DESC</span>');
      lines.push((state.per ? '<span class="tok-kw">MAX</span> <span class="tok-num">2</span> <span class="tok-kw">PER</span> brand ' : "") + '<span class="tok-kw">LIMIT</span> <span class="tok-num">' + LIMIT + "</span>;");
      lines.push("");
      lines.push('<span class="tok-com">-- :q = \'' + esc(qtext) + "'</span>");
      lines.push('<span class="tok-com">-- :v = {"embed": \'' + esc(state.q.trim()) + "'}</span>");
      return lines.join("\n");
    }
    function run() {
      var res = score();
      root.querySelector(".wv").textContent = Math.round(state.w * 100) + "%";
      root.querySelector(".mv").textContent = Math.round((1 - state.w) * 100) + "%";
      sqlEl.innerHTML = "<code>" + sql(res) + "</code>";
      var typos = [];
      res.groups.forEach(function (g) { g.terms.forEach(function (e) { if (e.d && typos.indexOf(g.word + " → " + e.term) < 0) typos.push(g.word + " → " + e.term); }); });
      why.innerHTML = (typos.length ? "<b>Typos read as</b> " + esc(typos.slice(0, 4).join(", ")) + ". " : "") +
        (!res.hasV ? "No word here has a meaning in this small sample, so only the words rank. " : "") +
        (state.w >= 1 ? "Only the words rank: a row must contain one." : state.w <= 0 ? "Only meaning ranks: rows that never use your words can lead." : "Blue is the words' share of the score, green the meaning's.");
      // FLIP: remember where each row was, reorder, then slide from there
      var before = {};
      $$("li", list).forEach(function (li) { before[li.getAttribute("data-id")] = li.getBoundingClientRect().top; });
      var keep = {};
      res.rows.forEach(function (r, i) {
        var li = list.querySelector('li[data-id="' + r.d.id + '"]');
        if (!li) { li = el("li", { "data-id": r.d.id }); li.classList.add("enter"); }
        keep[r.d.id] = 1;
        li.innerHTML = '<span class="pos">' + (i + 1) + '</span><span class="t"><b>' + esc(r.d.title) + "</b><small>" + esc(r.d.brand) + " · $" + r.d.price +
          (r.hit.length ? ' · <span class="hit">' + esc(r.hit.slice(0, 3).join(" ")) + "</span>" : "") + '</small></span><span class="sc" title="words ' + r.wpart.toFixed(2) + " + meaning " + r.mpart.toFixed(2) + '"><span class="bars"><i class="w" style="width:' + (r.wpart * 100).toFixed(1) + '%"></i><i class="m" style="width:' + (r.mpart * 100).toFixed(1) + '%"></i></span><em>' + r.total.toFixed(2) + "</em></span>";
        list.appendChild(li);
      });
      $$("li", list).forEach(function (li) { if (!keep[li.getAttribute("data-id")]) li.remove(); });
      if (!res.rows.length) list.innerHTML = '<li class="none">No row matches. Turn typos on, or slide toward meaning.</li>';
      if (REDUCED) { $$("li.enter", list).forEach(function (li) { li.classList.remove("enter"); }); return; }
      $$("li", list).forEach(function (li) {
        var id = li.getAttribute("data-id"), now = li.getBoundingClientRect().top;
        if (before[id] != null && Math.abs(before[id] - now) > 1) {
          li.style.transition = "none"; li.style.transform = "translateY(" + (before[id] - now) + "px)";
          requestAnimationFrame(function () { requestAnimationFrame(function () { li.style.transition = ""; li.style.transform = ""; }); });
        }
      });
      requestAnimationFrame(function () { requestAnimationFrame(function () { $$("li.enter", list).forEach(function (li) { li.classList.remove("enter"); }); }); });
    }
    var demo = true;
    function touched() { demo = false; }
    input.addEventListener("input", function () { touched(); state.q = input.value; run(); });
    slider.addEventListener("input", function () { touched(); state.w = 1 - slider.value / 100; run(); });
    typo.addEventListener("change", function () { touched(); state.typo = typo.checked; run(); });
    per.addEventListener("change", function () { touched(); state.per = per.checked; run(); });
    run();
    // until someone touches it: slide from words to meaning and back
    if (REDUCED) return;
    var timer = 0, dir = -1;
    onScreen(root, function () {
      if (timer || !demo) return;
      timer = setInterval(function () {
        if (!demo) { clearInterval(timer); return; }
        var w = Math.round(state.w * 100) + dir * 25;
        if (w < 0 || w > 100) { dir = -dir; w = Math.round(state.w * 100) + dir * 25; }
        state.w = w / 100; slider.value = 100 - w; run();
      }, 1700);
    }, function () { clearInterval(timer); timer = 0; }, 0.35);
  }

  /* ============================================================= cluster */
  var SHARD_HUE = ["#3987e5", "#199e70", "#d95926", "#c98500"];
  function cluster(root) {
    var cells = 64, perCell = 4096 / cells, MAX_SHARDS = 4;
    var S, wide, L, svg, gPkt, logEl, btn = {}, busy = false, queued = null, demo = true, nkey = 40, nsearch = 0;
    function fresh() {
      S = { shards: [], slots: [], epoch: 1, term: {}, down: null };
      for (var s = 0; s < 2; s++) S.shards.push({ copies: [{ up: true, primary: true }, { up: true }, { up: true }], term: 1 });
      for (var c = 0; c < cells; c++) S.slots.push(c < cells / 2 ? 0 : 1);
    }
    root.innerHTML = '<div class="cl-bar"></div><div class="cl-stage"></div><p class="cl-log" aria-live="polite"></p>';
    logEl = root.querySelector(".cl-log");
    [["search", "Search"], ["write", "Write a row"], ["stop", "Stop a primary"], ["shard", "Add a shard"], ["reset", "Reset"]].forEach(function (b) {
      var e = el("button", { type: "button", class: "chip" + (b[0] === "reset" ? " ghost" : "") }, b[1]);
      e.addEventListener("click", function () { demo = false; act(b[0]); });
      root.querySelector(".cl-bar").appendChild(e); btn[b[0]] = e;
    });
    function log(h) { logEl.innerHTML = h; }
    function layout() {
      wide = root.clientWidth >= 640;
      var n = S.shards.length;
      if (wide) {
        var W = 960, x0 = 470, cw = (W - 20 - x0) / MAX_SHARDS;
        L = { W: W, H: 300, app: [74, 124], coord: [272, 124], shard: function (i) { return x0 + cw * i + cw / 2; }, cw: cw - 18, ch: 40, cy: function (j) { return 70 + j * 54; },
          strip: { x: 20, y: 246, w: W - 40, rows: 1 }, cap: "4,096 slots, 64 to a square, coloured by the shard that holds them: a key hashes to a slot" };
      } else {
        var W2 = 400, cw2 = (W2 - 20) / MAX_SHARDS;
        L = { W: W2, H: 600, app: [200, 40], coord: [200, 150], shard: function (i) { return 10 + cw2 * i + cw2 / 2; }, cw: cw2 - 10, ch: 40, cy: function (j) { return 280 + j * 52; },
          strip: { x: 10, y: 470, w: W2 - 20, rows: 4 }, cap: "4,096 slots, 64 to a square" };
      }
      return n;
    }
    function copyPos(s, j) { return [L.shard(s), L.cy(j)]; }
    function draw() {
      layout();
      var st = root.querySelector(".cl-stage");
      st.innerHTML = "";
      svg = sv("svg", { viewBox: "0 0 " + L.W + " " + L.H, role: "img", "aria-label": "An app, a coordinator, and shards of three copies each; the squares below are the 4,096 slots, 64 to a square, coloured by the shard that holds them" });
      st.appendChild(svg);
      var gLinks = sv("g", { class: "links" }, svg);
      // app and coordinator
      box(L.app, 116, 54, "app", "chloedb.connect(...)", "node app");
      box(L.coord, 150, 64, "coordinator", "slot map · epoch " + S.epoch, "node coord");
      sv("line", { x1: L.app[0] + 58, y1: L.app[1], x2: L.coord[0] - 75, y2: L.coord[1], class: "link" }, gLinks);
      S.shards.forEach(function (sh, s) {
        var x = L.shard(s);
        var lab = sv("text", { x: x, y: L.cy(0) - 34, class: "shlab", "text-anchor": "middle" }, svg);
        lab.textContent = "shard " + s;
        sv("rect", { x: x - L.cw / 2 - 4, y: L.cy(0) - 28, width: L.cw + 8, height: L.cy(2) - L.cy(0) + L.ch + 12, rx: 10, class: "shbox", style: "--h:" + SHARD_HUE[s] }, svg);
        sh.copies.forEach(function (c, j) {
          var p = copyPos(s, j);
          sv("line", { x1: L.coord[0] + (wide ? 75 : 0), y1: L.coord[1] + (wide ? 0 : 32), x2: p[0] - (wide ? L.cw / 2 : 0), y2: p[1] - (wide ? 0 : L.ch / 2), class: "link" }, gLinks);
          var g = sv("g", { class: "copy" + (c.primary ? " primary" : "") + (c.up ? "" : " down"), id: cid(s, j), style: "--h:" + SHARD_HUE[s] }, svg);
          sv("rect", { x: p[0] - L.cw / 2, y: p[1] - L.ch / 2, width: L.cw, height: L.ch, rx: 7 }, g);
          var t = sv("text", { x: p[0], y: p[1] + 4, "text-anchor": "middle" }, g);
          t.textContent = (c.primary ? "★ " : "") + "s" + s + "-" + j + (c.up ? "" : " ✕");
        });
      });
      // the slots: 64 squares of 64 slots each
      var sw = L.strip.w, per = cells / L.strip.rows, cs = sw / per;
      S.slots.forEach(function (sh, c) {
        var x = L.strip.x + (c % per) * cs, y = L.strip.y + Math.floor(c / per) * cs;
        sv("rect", { x: x + 1, y: y + 1, width: cs - 2, height: cs - 2, rx: 2, class: "slot", id: root.id + "-slot" + c, fill: SHARD_HUE[sh] }, svg);
      });
      var cap = sv("text", { x: L.strip.x, y: L.strip.y + L.strip.rows * cs + 18, class: "cap" }, svg);
      cap.textContent = L.cap;
      gPkt = sv("g", { class: "pkts" }, svg);
      sync();
    }
    function box(p, w, h, title, sub, cls) {
      var g = sv("g", { class: cls }, svg);
      sv("rect", { x: p[0] - w / 2, y: p[1] - h / 2, width: w, height: h, rx: 10 }, g);
      var t = sv("text", { x: p[0], y: p[1] - (sub ? 3 : -4), "text-anchor": "middle", class: "t" }, g); t.textContent = title;
      if (sub) { var u = sv("text", { x: p[0], y: p[1] + 15, "text-anchor": "middle", class: "u" }, g); u.textContent = sub; }
      return g;
    }
    function cid(s, j) { return root.id + "-c" + s + "-" + j; }
    function flash(id) { var e = document.getElementById(id); if (!e) return; e.classList.remove("hit"); void e.getBBox(); e.classList.add("hit"); }
    function fly(a, b, cls, ms) {
      return new Promise(function (done) {
        if (REDUCED) { done(); return; }
        var c = sv("circle", { r: 6, cx: a[0], cy: a[1], class: "pkt " + (cls || "") }, gPkt);
        var t0 = performance.now(); ms = ms || 520;
        (function step(now) {
          var k = Math.min(1, (now - t0) / ms), e = k < 0.5 ? 2 * k * k : 1 - Math.pow(-2 * k + 2, 2) / 2;
          c.setAttribute("cx", a[0] + (b[0] - a[0]) * e); c.setAttribute("cy", a[1] + (b[1] - a[1]) * e);
          if (k < 1) requestAnimationFrame(step); else { c.remove(); done(); }
        })(t0);
      });
    }
    function edge(p, side) { return wide ? [p[0] + side * 75, p[1]] : [p[0], p[1] + side * 32]; }
    function cEdge(s, j) { var p = copyPos(s, j); return wide ? [p[0] - L.cw / 2, p[1]] : [p[0], p[1] - L.ch / 2]; }
    function appEdge() { return wide ? [L.app[0] + 58, L.app[1]] : [L.app[0], L.app[1] + 27]; }
    function primaryOf(s) { var i = -1; S.shards[s].copies.forEach(function (c, j) { if (c.primary && c.up) i = j; }); return i; }
    function sync() {
      btn.stop.textContent = S.down ? "Restart s" + S.down[0] + "-" + S.down[1] + " as a replica" : "Stop a primary";
      // the buttons stay live while something plays: a press waits for it to finish
      btn.shard.disabled = S.shards.length >= MAX_SHARDS;
    }
    function hashKey(k) { var h = 0x811c9dc5; for (var i = 0; i < k.length; i++) { h ^= k.charCodeAt(i); h = Math.imul(h, 0x01000193); } return (h >>> 0) % 4096; }

    async function search() {
      nsearch++;
      var hedge = nsearch % 3 === 0 ? Math.floor(Math.random() * S.shards.length) : -1;
      log("<b>Search.</b> The coordinator asks one copy of every shard for its top rows" + (hedge >= 0 ? ", hedging a slow one," : "") + " and merges them, scores and all.");
      await fly(appEdge(), edge(L.coord, -1), "q");
      await Promise.all(S.shards.map(async function (sh, s) {
        var ups = sh.copies.map(function (c, j) { return c.up ? j : -1; }).filter(function (j) { return j >= 0; });
        var j = ups[(nsearch + s) % ups.length];
        await fly(edge(L.coord, 1), cEdge(s, j), "q");
        flash(cid(s, j));
        if (s === hedge && ups.length > 1) {
          var g = document.getElementById(cid(s, j)); if (g) g.classList.add("slow");
          await wait(500);
          var k = ups[(ups.indexOf(j) + 1) % ups.length];
          log("<b>Hedge.</b> Copy s" + s + "-" + j + " has not answered in 100 ms (<code>--hedge-ms</code>), so the coordinator also asks s" + s + "-" + k + " and takes the first answer.");
          await fly(edge(L.coord, 1), cEdge(s, k), "q hedge");
          flash(cid(s, k));
          await fly(cEdge(s, k), edge(L.coord, 1), "a");
          if (g) g.classList.remove("slow");
          return;
        }
        await wait(120);
        await fly(cEdge(s, j), edge(L.coord, 1), "a");
      }));
      var co = svg.querySelector(".coord"); if (co) { co.classList.remove("merge"); void co.getBBox(); co.classList.add("merge"); }
      await wait(250);
      await fly(edge(L.coord, -1), appEdge(), "a");
    }
    async function write() {
      var key = "r" + (++nkey), slot = hashKey(key), cell = Math.floor(slot / perCell), s = S.slots[cell], p = primaryOf(s);
      var sq = document.getElementById(root.id + "-slot" + cell);
      if (sq) sq.classList.add("lit");
      if (p < 0) {
        log("<b>Write " + key + ".</b> It hashes to slot " + fmt(slot) + ", held by shard " + s + ", which has no primary until its copies elect one: the write waits.");
        await wait(1200); if (sq) sq.classList.remove("lit"); return;
      }
      log("<b>Write " + key + ".</b> Its key hashes to slot " + fmt(slot) + " of 4,096, held by shard " + s + ". Its primary, s" + s + "-" + p + ", commits it and ships it to every copy.");
      await fly(appEdge(), edge(L.coord, -1), "w");
      await fly(edge(L.coord, 1), cEdge(s, p), "w");
      flash(cid(s, p));
      var others = S.shards[s].copies.map(function (c, j) { return c.up && j !== p ? j : -1; }).filter(function (j) { return j >= 0; });
      await Promise.all(others.map(function (j) { return fly(copyPos(s, p), copyPos(s, j), "w", 380).then(function () { flash(cid(s, j)); }); }));
      await Promise.all(others.map(function (j) { return fly(copyPos(s, j), copyPos(s, p), "a", 300); }));
      await fly(cEdge(s, p), edge(L.coord, 1), "a");
      await fly(edge(L.coord, -1), appEdge(), "a");
      if (sq) sq.classList.remove("lit");
    }
    async function stop() {
      if (S.down) { // bring it back as a replica
        var d = S.down; S.shards[d[0]].copies[d[1]].up = true; S.down = null; draw();
        var np = primaryOf(d[0]);
        log("<b>Restarted.</b> s" + d[0] + "-" + d[1] + " starts again with <code>--replica</code> and catches up from s" + d[0] + "-" + np + ", the primary now.");
        await fly(copyPos(d[0], np), copyPos(d[0], d[1]), "w", 600); flash(cid(d[0], d[1]));
        return;
      }
      var s = Math.floor(Math.random() * S.shards.length), p = primaryOf(s);
      S.shards[s].copies[p].up = false; S.shards[s].copies[p].primary = false; S.down = [s, p]; draw();
      log("<b>A primary stops.</b> s" + s + "-" + p + " goes silent. Its copies stop hearing from it; reads go to them meanwhile.");
      await wait(1100);
      var ups = S.shards[s].copies.map(function (c, j) { return c.up ? j : -1; }).filter(function (j) { return j >= 0; });
      log("<b>Election.</b> After <code>--failover-after-ms</code> without a primary, the copies vote for the one holding every write.");
      await Promise.all([fly(copyPos(s, ups[0]), copyPos(s, ups[1]), "v", 500), fly(copyPos(s, ups[1]), copyPos(s, ups[0]), "v", 500)]);
      S.shards[s].copies[ups[0]].primary = true; S.shards[s].term++; draw(); flash(cid(s, ups[0]));
      log("<b>New primary.</b> s" + s + "-" + ups[0] + " is shard " + s + "'s primary for term " + S.shards[s].term + ", and the coordinator sends its writes there. No write it acknowledged is lost.");
    }
    async function addShard() {
      var n = S.shards.length;
      S.shards.push({ copies: [{ up: true, primary: true }, { up: true }, { up: true }], term: 1 });
      draw();
      log("<b>A new shard.</b> Shard " + n + " starts empty. The coordinator moves slots to it, 256 at a time, while the cluster keeps serving.");
      await wait(500);
      var want = Math.floor(cells / (n + 1)), moved = 0;
      while (moved < want) {
        // take from the shard holding the most
        var count = S.shards.map(function (_, i) { return S.slots.filter(function (x) { return x === i; }).length; });
        var from = count.indexOf(Math.max.apply(null, count.slice(0, n)));
        var batch = [];
        for (var c = cells - 1; c >= 0 && batch.length < 4; c--) if (S.slots[c] === from) batch.push(c);
        await fly(copyPos(from, Math.max(0, primaryOf(from))), copyPos(n, 0), "m", 480);
        batch.forEach(function (c) { S.slots[c] = n; var r = document.getElementById(root.id + "-slot" + c); if (r) { r.setAttribute("fill", SHARD_HUE[n]); r.classList.add("lit"); setTimeout(function () { r.classList.remove("lit"); }, 500); } });
        moved += batch.length;
      }
      S.epoch++; draw();
      log("<b>Moved.</b> " + fmt(moved * perCell) + " slots now live on shard " + n + ". The coordinator publishes slot map epoch " + S.epoch + "; a shard refuses requests from an older one.");
    }
    async function act(what) {
      if (busy) { queued = what; return; }
      if (what === "reset") { fresh(); draw(); log("Two shards of three copies. Press a button, or watch."); return; }
      busy = true; sync();
      try {
        if (what === "search") await search();
        else if (what === "write") await write();
        else if (what === "stop") await stop();
        else if (what === "shard" && S.shards.length < MAX_SHARDS) await addShard();
      } finally { busy = false; sync(); }
      if (queued) { var q = queued; queued = null; await act(q); }
    }
    fresh(); draw();
    log("Two shards of three copies behind a coordinator. Press a button, or watch.");
    var rt = 0, lastWide = wide;
    window.addEventListener("resize", function () { clearTimeout(rt); rt = setTimeout(function () { if ((root.clientWidth >= 640) !== lastWide && !busy) { lastWide = root.clientWidth >= 640; draw(); } }, 150); });
    if (REDUCED) return;
    var SCRIPT = ["search", "write", "write", "search", "stop", "write", "search", "stop", "shard", "search", "write", "search", "shard", "write", "search", "reset"];
    var at = 0, playing = false;
    async function play() {
      if (playing) return; playing = true;
      while (demo && visible) {
        await act(SCRIPT[at % SCRIPT.length]); at++;
        await wait(SCRIPT[(at - 1) % SCRIPT.length] === "reset" ? 600 : 1500);
      }
      playing = false;
    }
    var visible = false;
    onScreen(root, function () { visible = true; play(); }, function () { visible = false; }, 0.35);
  }

  /* ============================================================ maxscore */
  function maxscore(root) {
    var N = 64, BLK = 8, K = 3, K1 = 1.2, B = 0.75;
    var TERMS = [["wireless", 0.5], ["gaming", 0.24], ["headset", 0.1]];
    var lists, len, avg, ev, i, timer = 0, demo = true, seed = 31; // a seed whose run shows every kind of skip
    root.innerHTML = '<div class="ms-head"><div class="ms-q">BM25 top ' + K + ' for <code>wireless gaming headset</code></div><div class="ms-btns"></div></div>' +
      '<div class="ms-grid"></div><div class="ms-foot"><div class="ms-top"></div><div class="ms-stats" aria-live="polite"></div></div>';
    var grid = root.querySelector(".ms-grid"), topEl = root.querySelector(".ms-top"), stats = root.querySelector(".ms-stats");
    [["play", "Play"], ["step", "Step"], ["new", "New lists"]].forEach(function (b) {
      var e = el("button", { type: "button", class: "chip" + (b[0] === "new" ? " ghost" : "") }, b[1]);
      e.addEventListener("click", function () {
        demo = false;
        if (b[0] === "play") { if (timer) pause(); else go(); }
        if (b[0] === "step") { pause(); tick(); }
        if (b[0] === "new") { pause(); seed++; build(); }
      });
      root.querySelector(".ms-btns").appendChild(e);
    });
    var playBtn = root.querySelector(".ms-btns .chip");
    function build() {
      var r = rng(seed);
      len = []; for (var d = 0; d < N; d++) len.push(6 + Math.floor(r() * 30));
      avg = len.reduce(function (a, b) { return a + b; }, 0) / N;
      lists = TERMS.map(function (t) {
        var post = {};
        for (var d = 0; d < N; d++) if (r() < t[1]) post[d] = 1 + Math.floor(r() * r() * 4);
        var df = Object.keys(post).length || 1, idf = Math.log(1 + (N - df + 0.5) / (df + 0.5));
        var sc = {}, bmax = [], lmax = 0;
        Object.keys(post).forEach(function (k) { var tf = post[k]; sc[k] = idf * tf * (K1 + 1) / (tf + K1 * (1 - B + B * len[k] / avg)); });
        for (var b = 0; b < N / BLK; b++) { var m = 0; for (var x = b * BLK; x < (b + 1) * BLK; x++) if (sc[x]) m = Math.max(m, sc[x]); bmax.push(m); lmax = Math.max(lmax, m); }
        return { term: t[0], idf: idf, sc: sc, bmax: bmax, lmax: lmax, df: df };
      });
      ev = run(); i = 0;
      render();
    }
    // MaxScore with block bounds, recorded as a list of states
    function run() {
      var order = lists.map(function (l, k) { return k; }).sort(function (a, b) { return lists[a].lmax - lists[b].lmax; });
      var top = [], theta = 0, out = [], state = {};
      function essentialFrom() { var s = 0; for (var k = 0; k < order.length; k++) { s += lists[order[k]].lmax; if (s > theta) return k; } return order.length; }
      var d = -1;
      while (true) {
        var ef = essentialFrom(), ess = order.slice(ef);
        var next = N;
        ess.forEach(function (k) { for (var x = d + 1; x < N; x++) if (lists[k].sc[x]) { next = Math.min(next, x); break; } });
        // the rows only the other lists hold, passed over
        for (var x = d + 1; x < next; x++) if (state[x] == null && lists.some(function (l) { return l.sc[x]; })) state[x] = "never";
        if (next >= N) { out.push({ d: N, theta: theta, top: top.slice(), ess: ess.slice(), state: Object.assign({}, state), msg: "Done. Every row that could reach the top " + K + " was scored." }); break; }
        d = next;
        var blk = Math.floor(d / BLK), ub = 0;
        lists.forEach(function (l) { ub += l.bmax[blk]; });
        if (top.length >= K && ub <= theta) {
          state[d] = "skip";
          out.push({ d: d, theta: theta, top: top.slice(), ess: ess.slice(), state: Object.assign({}, state), msg: "Row " + d + ": its block bounds add to " + ub.toFixed(2) + ", not above the threshold " + theta.toFixed(2) + ". Skipped without reading its score." });
          continue;
        }
        var s = 0; lists.forEach(function (l) { if (l.sc[d]) s += l.sc[d]; });
        state[d] = "scored";
        var msg = "Row " + d + ": bound " + ub.toFixed(2) + ", scored " + s.toFixed(2) + ".";
        if (top.length < K || s > theta) {
          top.push({ d: d, s: s }); top.sort(function (a, b) { return b.s - a.s; }); if (top.length > K) top.pop();
          if (top.length >= K) theta = top[K - 1].s;
          msg += " It enters the top " + K + (top.length >= K ? "; the threshold is now " + theta.toFixed(2) + "." : ".");
          var ef2 = essentialFrom();
          if (ef2 > ef) msg += " “" + order.slice(ef, ef2).map(function (k) { return lists[k].term; }).join("”, “") + "” can no longer lift a row on its own: it only confirms.";
        }
        out.push({ d: d, theta: theta, top: top.slice(), ess: essentialFrom(), state: Object.assign({}, state), msg: msg });
      }
      return out;
    }
    function render() {
      var e = ev[Math.min(i, ev.length - 1)], ess = Array.isArray(e.ess) ? e.ess : null;
      var order = lists.map(function (l, k) { return k; }).sort(function (a, b) { return lists[a].lmax - lists[b].lmax; });
      var essSet = ess || order.slice(e.ess);
      var maxB = Math.max.apply(null, lists.map(function (l) { return l.lmax; }));
      var h = "";
      lists.forEach(function (l, k) {
        var isEss = essSet.indexOf(k) >= 0;
        h += '<div class="ms-row' + (isEss ? "" : " conf") + '"><div class="ms-lab"><b>' + esc(l.term) + "</b><small>" + l.df + " rows · best " + l.lmax.toFixed(2) + "</small><em>" + (isEss ? "drives" : "only confirms") + '</em></div><div class="ms-cells" style="--n:' + N + '">';
        for (var b = 0; b < N / BLK; b++) h += '<span class="ms-bmax" style="grid-column:' + (b * BLK + 1) + " / span " + BLK + ";--v:" + (l.bmax[b] / maxB).toFixed(3) + '" title="block ' + b + ": best " + l.bmax[b].toFixed(2) + '"></span>';
        for (var d = 0; d < N; d++) {
          var st = e.state[d] || "", has = l.sc[d] != null;
          h += '<span class="ms-c' + (has ? " has" : "") + (st ? " " + st : "") + (d === e.d ? " cur" : "") + '" style="grid-column:' + (d + 1) + (has ? ";--v:" + (0.25 + 0.75 * l.sc[d] / l.lmax).toFixed(2) : "") + '"></span>';
        }
        h += "</div></div>";
      });
      grid.innerHTML = h;
      topEl.innerHTML = "<b>Top " + K + "</b>" + (e.top.length ? e.top.map(function (t, r) { return "<span>" + (r + 1) + ". row " + t.d + " <em>" + t.s.toFixed(2) + "</em></span>"; }).join("") : "<span>empty</span>") + '<span class="th">threshold <em>' + e.theta.toFixed(2) + "</em></span>";
      var cnt = { scored: 0, skip: 0, never: 0 }, total = 0;
      for (var d2 = 0; d2 < N; d2++) { if (lists.some(function (l) { return l.sc[d2] != null; })) total++; if (e.state[d2]) cnt[e.state[d2]]++; }
      stats.innerHTML = '<p class="msg">' + esc(e.msg) + '</p><p class="cnt"><span class="k s"></span>scored ' + cnt.scored + ' <span class="k b"></span>skipped by a block bound ' + cnt.skip + ' <span class="k n"></span>never visited ' + cnt.never + " · of " + total + " rows with a word</p>";
      playBtn.textContent = timer ? "Pause" : i >= ev.length - 1 ? "Replay" : "Play";
    }
    function tick() { if (i >= ev.length - 1) { pause(); return false; } i++; render(); return true; }
    function go() { if (i >= ev.length - 1) { i = 0; } clearInterval(timer); timer = setInterval(function () { if (!tick()) { pause(); if (demo) setTimeout(function () { if (demo && visible) { seed++; build(); go(); } }, 2600); } }, REDUCED ? 0 : 520); render(); }
    function pause() { clearInterval(timer); timer = 0; render(); }
    var visible = false;
    build();
    if (REDUCED) { i = ev.length - 1; render(); return; }
    onScreen(root, function () { visible = true; if (demo && !timer) go(); }, function () { visible = false; if (demo) pause(); }, 0.35);
  }

  /* ================================================================ hnsw */
  function hnsw(root) {
    var NPTS = 90, M = [5, 3, 3], EF = 8, KNN = 4;
    var pts, lev, adj, entry, cv, ctx, W, H, dpr, path, t0 = 0, raf = 0, q, demo = true, seed = 3, timer = 0;
    root.innerHTML = '<canvas aria-label="Three layers of an HNSW graph, the top one sparse; a search starts at the top and walks down toward the query"></canvas>' +
      '<div class="hn-foot"><div class="hn-stats" aria-live="polite"></div><div class="hn-btns"></div></div>';
    cv = root.querySelector("canvas"); ctx = cv.getContext("2d");
    var stats = root.querySelector(".hn-stats");
    [["new", "New query"], ["graph", "New graph"]].forEach(function (b) {
      var e = el("button", { type: "button", class: "chip" + (b[0] === "graph" ? " ghost" : "") }, b[1]);
      e.addEventListener("click", function () { demo = false; if (b[0] === "graph") { seed++; build(); } newQuery(); });
      root.querySelector(".hn-btns").appendChild(e);
    });
    var d2 = function (a, b) { var x = a[0] - b[0], y = a[1] - b[1]; return x * x + y * y; };
    function build() {
      var r = rng(seed);
      pts = []; lev = [];
      for (var i = 0; i < NPTS; i++) { pts.push([0.04 + r() * 0.92, 0.06 + r() * 0.88]); var l = 0; while (l < 2 && r() < 0.25) l++; lev.push(l); }
      // the entry point lives on the top layer
      if (lev.indexOf(2) < 0) lev[0] = 2;
      entry = lev.indexOf(2);
      adj = [0, 1, 2].map(function (L) {
        var ids = []; lev.forEach(function (l, i) { if (l >= L) ids.push(i); });
        var a = {}; ids.forEach(function (i) { a[i] = []; });
        ids.forEach(function (i) {
          var near = ids.filter(function (j) { return j !== i; }).sort(function (x, y) { return d2(pts[i], pts[x]) - d2(pts[i], pts[y]); }).slice(0, M[L]);
          near.forEach(function (j) { if (a[i].indexOf(j) < 0) a[i].push(j); if (a[j].indexOf(i) < 0) a[j].push(i); });
        });
        return { ids: ids, a: a };
      });
    }
    // greedy on the upper layers, a beam of EF on the bottom one
    function search(qp) {
      var steps = [], seen = {}, comps = 0, cur = entry;
      var dist = function (i) { if (!seen[i]) { seen[i] = 1; comps++; } return d2(pts[i], qp); };
      dist(cur);
      for (var L = 2; L >= 1; L--) {
        var moved = true;
        while (moved) {
          moved = false;
          var bn = -1, bd = dist(cur);
          adj[L].a[cur].forEach(function (n) { var dn = dist(n); if (dn < bd) { bd = dn; bn = n; } });
          if (bn >= 0) { steps.push({ L: L, a: cur, b: bn }); cur = bn; moved = true; }
        }
        steps.push({ L: L, down: cur });
      }
      var cand = [cur], best = [cur], vis = {}; vis[cur] = 1;
      while (cand.length) {
        cand.sort(function (a, b) { return dist(a) - dist(b); });
        var c = cand.shift();
        best.sort(function (a, b) { return dist(a) - dist(b); });
        if (best.length >= EF && dist(c) > dist(best[best.length - 1])) break;
        adj[0].a[c].forEach(function (n) {
          if (vis[n]) return; vis[n] = 1;
          steps.push({ L: 0, a: c, b: n, look: true });
          if (best.length < EF || dist(n) < dist(best[best.length - 1])) { cand.push(n); best.push(n); best.sort(function (a, b) { return dist(a) - dist(b); }); if (best.length > EF) best.pop(); }
        });
      }
      var found = best.slice(0, KNN);
      var truth = pts.map(function (_, i) { return i; }).sort(function (a, b) { return d2(pts[a], qp) - d2(pts[b], qp); }).slice(0, KNN);
      return { steps: steps, found: found, truth: truth, comps: comps };
    }
    function size() {
      dpr = Math.min(2, window.devicePixelRatio || 1);
      W = root.clientWidth; H = Math.round(Math.max(340, Math.min(560, W * 0.62)));
      cv.width = W * dpr; cv.height = H * dpr; cv.style.height = H + "px";
      ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
    }
    // three planes seen from above and in front, top layer on top
    function proj(p, L) {
      var pad = 16, skew = Math.min(90, W * 0.12), ph = (H - 40) / 3 * 0.78, gap = (H - 40) / 3;
      var top = 14 + (2 - L) * gap;
      return [pad + skew * (1 - p[1]) + p[0] * (W - 2 * pad - skew), top + p[1] * ph];
    }
    function unproj(x, y) {
      var pad = 16, skew = Math.min(90, W * 0.12), ph = (H - 40) / 3 * 0.78, gap = (H - 40) / 3, top = 14 + 2 * gap;
      var py = (y - top) / ph; if (py < 0 || py > 1) return null;
      var px = (x - pad - skew * (1 - py)) / (W - 2 * pad - skew); if (px < 0 || px > 1) return null;
      return [px, py];
    }
    function frame(now) {
      var ink = cssVar("--ink", "#0b1220"), dim = cssVar("--ink-3", "#5f6b7d"), rule = cssVar("--rule-2", "#cfd5df"), vec = cssVar("--fam-vec", "#1baf7a"), acc = cssVar("--accent", "#2f45e6");
      var el2 = REDUCED ? 1e9 : (now - t0) / 1000, per = 0.16;
      var shown = Math.min(path.steps.length, Math.floor(el2 / per));
      ctx.clearRect(0, 0, W, H);
      for (var L = 2; L >= 0; L--) {
        var c = [proj([0, 0], L), proj([1, 0], L), proj([1, 1], L), proj([0, 1], L)];
        ctx.beginPath(); c.forEach(function (p, i) { i ? ctx.lineTo(p[0], p[1]) : ctx.moveTo(p[0], p[1]); }); ctx.closePath();
        ctx.fillStyle = vec; ctx.globalAlpha = 0.05; ctx.fill(); ctx.globalAlpha = 0.5; ctx.strokeStyle = rule; ctx.lineWidth = 1; ctx.stroke(); ctx.globalAlpha = 1;
        ctx.fillStyle = dim; ctx.font = "500 11px " + cssVar("--mono", "monospace");
        ctx.fillText("layer " + L + " · " + adj[L].ids.length + " nodes", c[0][0], c[0][1] - 5);
        // edges
        ctx.strokeStyle = vec; ctx.globalAlpha = L === 0 ? 0.18 : 0.35; ctx.lineWidth = 1;
        ctx.beginPath();
        adj[L].ids.forEach(function (i) { adj[L].a[i].forEach(function (j) { if (j > i) { var a = proj(pts[i], L), b = proj(pts[j], L); ctx.moveTo(a[0], a[1]); ctx.lineTo(b[0], b[1]); } }); });
        ctx.stroke(); ctx.globalAlpha = 1;
        adj[L].ids.forEach(function (i) { var p = proj(pts[i], L); ctx.fillStyle = vec; ctx.globalAlpha = 0.75; ctx.beginPath(); ctx.arc(p[0], p[1], L ? 3.6 : 2.6, 0, 6.283); ctx.fill(); ctx.globalAlpha = 1; });
      }
      // the query, on every layer
      for (var L2 = 2; L2 >= 0; L2--) {
        var qp = proj(q, L2);
        ctx.strokeStyle = acc; ctx.lineWidth = 1.5; ctx.beginPath(); ctx.moveTo(qp[0] - 6, qp[1]); ctx.lineTo(qp[0] + 6, qp[1]); ctx.moveTo(qp[0], qp[1] - 6); ctx.lineTo(qp[0], qp[1] + 6); ctx.stroke();
        if (L2) { var nx = proj(q, L2 - 1); ctx.setLineDash([3, 4]); ctx.globalAlpha = 0.5; ctx.beginPath(); ctx.moveTo(qp[0], qp[1]); ctx.lineTo(nx[0], nx[1]); ctx.stroke(); ctx.setLineDash([]); ctx.globalAlpha = 1; }
      }
      // the walk so far
      var hops = [0, 0, 0], looked = 0, at = entry, atL = 2;
      for (var s = 0; s < shown; s++) {
        var st = path.steps[s];
        if (st.down != null) {
          var a = proj(pts[st.down], st.L), b = proj(pts[st.down], st.L - 1);
          ctx.strokeStyle = acc; ctx.setLineDash([4, 3]); ctx.lineWidth = 2; ctx.beginPath(); ctx.moveTo(a[0], a[1]); ctx.lineTo(b[0], b[1]); ctx.stroke(); ctx.setLineDash([]);
          at = st.down; atL = st.L - 1; continue;
        }
        var pa = proj(pts[st.a], st.L), pb = proj(pts[st.b], st.L);
        if (st.look) { looked++; ctx.strokeStyle = acc; ctx.globalAlpha = 0.35; ctx.lineWidth = 1.2; ctx.beginPath(); ctx.moveTo(pa[0], pa[1]); ctx.lineTo(pb[0], pb[1]); ctx.stroke(); ctx.globalAlpha = 1;
          ctx.strokeStyle = acc; ctx.beginPath(); ctx.arc(pb[0], pb[1], 5, 0, 6.283); ctx.stroke(); }
        else { hops[st.L]++; ctx.strokeStyle = acc; ctx.lineWidth = 2.6; ctx.beginPath(); ctx.moveTo(pa[0], pa[1]); ctx.lineTo(pb[0], pb[1]); ctx.stroke(); at = st.b; atL = st.L; }
      }
      var e0 = proj(pts[entry], 2);
      ctx.fillStyle = acc; ctx.beginPath(); ctx.arc(e0[0], e0[1], 5, 0, 6.283); ctx.fill();
      ctx.fillStyle = ink; ctx.font = "600 11px " + cssVar("--body", "sans-serif"); ctx.fillText("entry", e0[0] + 8, e0[1] - 6);
      var done = shown >= path.steps.length;
      if (done) {
        path.found.forEach(function (i) { var p = proj(pts[i], 0); ctx.fillStyle = acc; ctx.beginPath(); ctx.arc(p[0], p[1], 5.5, 0, 6.283); ctx.fill(); });
        var hit = path.found.filter(function (i) { return path.truth.indexOf(i) >= 0; }).length;
        stats.innerHTML = "<b>" + hops[2] + "</b> hop" + (hops[2] === 1 ? "" : "s") + " on layer 2, <b>" + hops[1] + "</b> on layer 1, then <b>" + looked + "</b> neighbours looked at on layer 0. Found <b>" + hit + "</b> of the <b>" + KNN + "</b> nearest, after <b>" + path.comps + "</b> distances out of " + NPTS + ".";
      } else {
        var cur = proj(pts[at], atL); ctx.strokeStyle = acc; ctx.lineWidth = 2; ctx.beginPath(); ctx.arc(cur[0], cur[1], 8, 0, 6.283); ctx.stroke();
        stats.innerHTML = "Walking layer " + atL + (atL ? ": move to the neighbour nearest the query until none is nearer, then go down." : ": keep the " + EF + " nearest seen, and look at their neighbours.");
      }
      if (!done && !REDUCED) raf = requestAnimationFrame(frame);
    }
    function newQuery(p) {
      var r = Math.random;
      q = p || [0.1 + r() * 0.8, 0.12 + r() * 0.76];
      path = search(q);
      t0 = performance.now(); cancelAnimationFrame(raf); raf = requestAnimationFrame(frame);
    }
    cv.addEventListener("click", function (e) {
      var b = cv.getBoundingClientRect(), p = unproj(e.clientX - b.left, e.clientY - b.top);
      if (p) { demo = false; newQuery(p); }
    });
    build(); size(); newQuery([0.72, 0.62]);
    var rt = 0;
    window.addEventListener("resize", function () { clearTimeout(rt); rt = setTimeout(function () { size(); t0 = -1e9; cancelAnimationFrame(raf); raf = requestAnimationFrame(frame); }, 120); });
    if (REDUCED) return;
    onScreen(root, function () { if (!timer) timer = setInterval(function () { if (demo) newQuery(); else { clearInterval(timer); } }, 7000); }, function () { clearInterval(timer); timer = 0; }, 0.3);
  }

  /* =============================================================== merge */
  function merge(root) {
    var F = 4, FLOOR = 1000, MAX_DEL = 0.3, segs, meta, nid, docs, timer = 0, demo = true, rewrites;
    root.innerHTML = '<div class="mg-head"><div class="mg-meta"><span class="m0">meta slot 0</span><span class="m1">meta slot 1</span></div><div class="mg-btns"></div></div>' +
      '<div class="mg-tiers"></div><p class="mg-log" aria-live="polite"></p>';
    var tiersEl = root.querySelector(".mg-tiers"), logEl = root.querySelector(".mg-log");
    [["commit", "Commit 1,000 rows"], ["del", "Delete rows"], ["play", "Play"]].forEach(function (b) {
      var e = el("button", { type: "button", class: "chip" + (b[0] === "play" ? " ghost" : "") }, b[1]);
      e.addEventListener("click", function () {
        demo = false;
        if (b[0] === "play") { if (timer) { clearInterval(timer); timer = 0; e.textContent = "Play"; } else { timer = setInterval(step, 900); e.textContent = "Pause"; } return; }
        if (b[0] === "commit") commit(); else del();
      });
      root.querySelector(".mg-btns").appendChild(e);
    });
    function reset() { segs = []; meta = 0; nid = 0; docs = 0; rewrites = 0; }
    var TIER_HUE = ["var(--fam-text)", "var(--fam-vec)", "var(--fam-cols)", "var(--fam-graph)"];
    var tier = function (n) { return n < FLOOR ? 0 : Math.floor(Math.log(n / FLOOR) / Math.log(F) + 1e-9); };
    function render(fresh, merged) {
      root.querySelector(".m0").classList.toggle("on", meta === 0);
      root.querySelector(".m1").classList.toggle("on", meta === 1);
      var by = {}; segs.forEach(function (s) { (by[tier(s.n)] = by[tier(s.n)] || []).push(s); });
      var top = Math.max(2, Math.max.apply(null, [0].concat(Object.keys(by).map(Number))));
      var h = "";
      for (var t = top; t >= 0; t--) {
        h += '<div class="mg-tier"><span class="mg-t">tier ' + t + "<small>" + fmt(FLOOR * Math.pow(F, t)) + "+ rows</small></span><div>";
        (by[t] || []).forEach(function (s) {
          var w = Math.max(18, Math.min(100, 18 + 26 * Math.log(s.n / FLOOR + 1) / Math.log(F)));
          h += '<span class="mg-seg' + (s.id === fresh ? " fresh" : "") + (merged && merged.indexOf(s.id) >= 0 ? " merged" : "") + '" style="--c:' + TIER_HUE[Math.min(t, 3)] + ";--w:" + w.toFixed(0) + "%;--del:" + (s.del / s.n * 100).toFixed(0) + '%" title="' + fmt(s.n) + " rows, " + fmt(s.del) + ' deleted"><b>' + fmt(s.n - s.del) + "</b></span>";
        });
        h += "</div></div>";
      }
      tiersEl.innerHTML = h;
    }
    function log(h) { logEl.innerHTML = h; }
    function commit() {
      var s = { id: ++nid, n: FLOOR, del: 0 }; segs.push(s); docs += FLOOR; meta = 1 - meta;
      log("<b>Commit.</b> 1,000 rows are written to new pages, then meta slot " + meta + " is rewritten to point at them: the switch is one checksummed write, so a crash leaves the old state or the new one.");
      render(s.id);
      setTimeout(mergeIfDue, REDUCED ? 0 : 650);
    }
    function mergeIfDue() {
      var by = {}; segs.forEach(function (s) { (by[tier(s.n)] = by[tier(s.n)] || []).push(s); });
      var t = Object.keys(by).map(Number).sort(function (a, b) { return a - b; }).filter(function (t) { return by[t].length >= F; })[0];
      var heavy = segs.filter(function (s) { return s.del / s.n > MAX_DEL; })[0];
      if (t == null && !heavy) return;
      var group = t != null ? by[t].sort(function (a, b) { return a.n - b.n; }).slice(0, F) : [heavy];
      render(null, group.map(function (s) { return s.id; }));
      setTimeout(function () {
        var n = group.reduce(function (a, s) { return a + s.n - s.del; }, 0), ns = { id: ++nid, n: n, del: 0 };
        segs = segs.filter(function (s) { return group.indexOf(s) < 0; }); segs.push(ns); rewrites += n; meta = 1 - meta;
        log(t != null ? "<b>Merge.</b> Tier " + t + " holds " + F + " segments, so they become one of " + fmt(n) + " rows in tier " + tier(n) + ", in the background. Queries keep reading the old ones until the new one is committed."
          : "<b>Rewrite.</b> Over " + MAX_DEL * 100 + "% of a segment's rows were deleted, so it is rewritten alone without them.");
        render(ns.id);
        setTimeout(mergeIfDue, REDUCED ? 0 : 700);
      }, REDUCED ? 0 : 700);
    }
    function del() {
      if (!segs.length) return;
      var s = segs[Math.floor(Math.random() * segs.length)], k = Math.round(s.n * (0.12 + Math.random() * 0.2));
      s.del = Math.min(s.n, s.del + k); meta = 1 - meta;
      log("<b>Delete.</b> " + fmt(k) + " rows are marked in the segment's delete set; the rows stay where they are until a merge drops them.");
      render(s.id); setTimeout(mergeIfDue, REDUCED ? 0 : 650);
    }
    var at = 0;
    function step() { at++; if (at % 7 === 0) del(); else commit(); if (segs.length > 14 || docs > 90000) { reset(); render(); } }
    reset(); render();
    log("Every commit makes a small segment; a tiered policy merges them. The merge factor is 4 here, to fit; ChloeDB's default is 8.");
    if (REDUCED) return;
    onScreen(root, function () { if (demo && !timer) timer = setInterval(function () { if (demo) step(); }, 1200); }, function () { if (demo) { clearInterval(timer); timer = 0; } }, 0.35);
  }

  var SIMS = { rank: rank, cluster: cluster, maxscore: maxscore, hnsw: hnsw, merge: merge };
  function boot() {
    $$("[data-sim]").forEach(function (root, i) {
      if (!root.id) root.id = "sim" + i;
      var f = SIMS[root.getAttribute("data-sim")];
      if (f) try { f(root); } catch (e) { root.innerHTML = '<p class="small muted">This sketch could not start in this browser.</p>'; if (window.console) console.error(e); }
    });
  }
  if (document.readyState === "loading") document.addEventListener("DOMContentLoaded", boot); else boot();
})();
