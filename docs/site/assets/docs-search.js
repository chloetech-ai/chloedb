// The docs' search box, above the sidebar of every page build_docs.py
// writes. The first time it is used it loads search-index.js (every page cut
// at its h2 and h3 headings), then ranks the sections that hold every word
// typed first and those that hold some of them after (more of them first),
// each by BM25 with their headings and page titles counting extra. "/" or Ctrl+K focuses it; the arrows and Enter pick a result; a
// page opened with ?q=words searches at once.
(function () {
  "use strict";
  var form = document.querySelector(".doc-search");
  if (!form) return;
  var input = form.querySelector("input");
  var box = form.querySelector(".doc-search-results");
  var rows = null, avgLen = 1, loading = false, waiting = null, active = -1, hits = [], timer = 0;
  var WORD = /[\p{L}\p{N}_]/u;

  function load(then) {
    if (rows) return then();
    waiting = then;
    if (loading) return;
    loading = true;
    var s = document.createElement("script");
    s.src = form.getAttribute("data-index");
    s.onload = function () {
      var data = window.CHLOEDB_DOCS_INDEX || { sections: [] };
      rows = data.sections.map(function (r) {
        return { u: r[0], p: r[1], h: r[2], t: r[3], lp: r[1].toLowerCase(), lh: r[2].toLowerCase(), lt: r[3].toLowerCase() };
      });
      avgLen = rows.reduce(function (a, r) { return a + r.lt.length; }, 0) / Math.max(1, rows.length) || 1;
      if (waiting) waiting();
    };
    s.onerror = function () { loading = false; note("The search index did not load. Reload the page to try again."); };
    document.head.appendChild(s);
  }

  function words(q) {
    var out = [];
    q.toLowerCase().split(/[^\p{L}\p{N}_.\-]+/u).forEach(function (w) {
      w = w.replace(/^[.\-]+|[.\-]+$/g, "");
      if (w && out.indexOf(w) < 0) out.push(w);
    });
    return out;
  }

  function count(hay, w, max) {
    var n = 0, i = hay.indexOf(w);
    while (i >= 0 && n < max) { n++; i = hay.indexOf(w, i + w.length); }
    return n;
  }

  function atWordStart(hay, w) {
    for (var i = hay.indexOf(w); i >= 0; i = hay.indexOf(w, i + 1)) {
      if (i === 0 || !WORD.test(hay[i - 1])) return true;
    }
    return false;
  }

  // BM25 over the sections, as ChloeDB ranks text: a rare word counts more
  // than a common one, and a word said often in a short section more than
  // in a long one. A word in the section's heading or the page's title adds
  // to that, more when a word of the heading starts with it. A section with
  // more of the words comes first, so one that holds them all leads, and a
  // question in a beginner's words ("similar recipes") still finds the
  // sections that hold one of them ("similar").
  function search(q) {
    var ws = words(q);
    if (!ws.length) return [];
    var phrase = ws.length > 1 ? ws.join(" ") : "";
    var n = rows.length, df = ws.map(function () { return 0; }), found = [];
    rows.forEach(function (r, i) {
      var per = [], got = 0;
      ws.forEach(function (w, k) {
        var x = { h: r.lh.indexOf(w) >= 0, p: r.lp.indexOf(w) >= 0, tf: count(r.lt, w, 50) };
        if (x.h || x.p || x.tf) { df[k]++; got++; }
        per.push(x);
      });
      if (got) found.push({ r: r, i: i, per: per, got: got });
    });
    var idf = df.map(function (d) { return Math.log(1 + (n - d + 0.5) / (d + 0.5)); });
    found.forEach(function (x) {
      var r = x.r, norm = 1.2 * (0.25 + 0.75 * r.lt.length / avgLen), s = 0;
      x.per.forEach(function (m, k) {
        var w = ws[k], part = m.tf ? m.tf * 2.2 / (m.tf + norm) : 0;
        if (m.h) part += atWordStart(r.lh, w) ? 3 : 1.5;
        if (m.p) part += atWordStart(r.lp, w) ? 1.5 : 0.75;
        s += idf[k] * part;
      });
      if (phrase && (r.lh.indexOf(phrase) >= 0 || r.lt.indexOf(phrase) >= 0)) s *= 1.5;
      x.s = s;
    });
    found.sort(function (a, b) { return b.got - a.got || b.s - a.s || a.i - b.i; });
    // At most three sections of one page among the first results, so one
    // long page does not hide the others.
    var seen = {}, top = [], rest = [];
    found.forEach(function (x) {
      var page = x.r.u.split("#")[0];
      seen[page] = (seen[page] || 0) + 1;
      (seen[page] <= 3 ? top : rest).push(x);
    });
    return top.concat(rest).slice(0, 12).map(function (x) { return { r: x.r, all: x.got === ws.length }; });
  }

  // The text around the first word found, with every word marked.
  function snippet(r, ws) {
    var t = r.t, lt = r.lt, at = -1;
    ws.forEach(function (w) { var i = lt.indexOf(w); if (i >= 0 && (at < 0 || i < at)) at = i; });
    var from = at < 0 ? 0 : Math.max(0, at - 60), to = Math.min(t.length, from + 180);
    if (from > 0) { var sp = t.indexOf(" ", from); if (sp >= 0 && sp < at) from = sp + 1; }
    if (to < t.length) { var sp2 = t.lastIndexOf(" ", to); if (sp2 > at) to = sp2; }
    return mark((from > 0 ? "… " : "") + t.slice(from, to) + (to < t.length ? " …" : ""), ws);
  }

  function mark(text, ws) {
    var frag = document.createDocumentFragment(), lower = text.toLowerCase(), i = 0;
    while (i < text.length) {
      var best = -1, len = 0;
      ws.forEach(function (w) { var j = lower.indexOf(w, i); if (j >= 0 && (best < 0 || j < best || (j === best && w.length > len))) { best = j; len = w.length; } });
      if (best < 0) break;
      if (best > i) frag.appendChild(document.createTextNode(text.slice(i, best)));
      var m = document.createElement("mark");
      m.textContent = text.slice(best, best + len);
      frag.appendChild(m);
      i = best + len;
    }
    if (i < text.length) frag.appendChild(document.createTextNode(text.slice(i)));
    return frag;
  }

  function el(tag, cls, child) {
    var e = document.createElement(tag);
    if (cls) e.className = cls;
    if (typeof child === "string") e.textContent = child; else if (child) e.appendChild(child);
    return e;
  }

  function open(on) {
    box.hidden = !on;
    input.setAttribute("aria-expanded", on ? "true" : "false");
  }

  function note(text) {
    box.replaceChildren(el("p", "doc-search-none", text));
    open(true);
  }

  function show() {
    var q = input.value.trim();
    active = -1;
    input.removeAttribute("aria-activedescendant");
    if (!q) { hits = []; open(false); return; }
    var ws = words(q);
    var found = search(q);
    hits = found.map(function (x) { return x.r; });
    if (!hits.length) { note("No section has a word of “" + q + "”. Try other or shorter words."); return; }
    box.replaceChildren();
    if (ws.length > 1 && !found[0].all) box.appendChild(el("p", "doc-search-none", "No section has every word of “" + q + "”. These have some of them:"));
    hits.forEach(function (r, i) {
      var a = el("a", "doc-search-hit");
      a.href = r.u;
      a.id = "doc-search-hit-" + i;
      a.setAttribute("role", "option");
      a.appendChild(el("b", "", mark(r.h || r.p, ws)));
      if (r.h) a.appendChild(el("span", "where", r.p));
      if (r.t) a.appendChild(el("span", "snip", snippet(r, ws)));
      box.appendChild(a);
    });
    open(true);
  }

  function select(i) {
    var links = box.querySelectorAll(".doc-search-hit");
    if (!links.length) return;
    active = (i + links.length) % links.length;
    links.forEach(function (a, k) { a.setAttribute("aria-selected", k === active ? "true" : "false"); });
    links[active].scrollIntoView({ block: "nearest" });
    input.setAttribute("aria-activedescendant", links[active].id);
  }

  function go(i) {
    var r = hits[i < 0 ? 0 : i];
    if (!r) return;
    open(false);
    window.location.href = r.u;
  }

  input.addEventListener("focus", function () { load(function () { if (input.value.trim()) show(); }); });
  input.addEventListener("input", function () {
    clearTimeout(timer);
    timer = setTimeout(function () { load(show); }, 60);
  });
  input.addEventListener("keydown", function (e) {
    if (e.key === "ArrowDown") { e.preventDefault(); select(active + 1); }
    else if (e.key === "ArrowUp") { e.preventDefault(); select(active - 1); }
    else if (e.key === "Escape") { if (!box.hidden) open(false); else input.blur(); }
  });
  form.addEventListener("submit", function (e) {
    e.preventDefault();
    load(function () { if (!hits.length) show(); go(active); });
  });
  box.addEventListener("click", function () { open(false); });
  document.addEventListener("click", function (e) { if (!form.contains(e.target)) open(false); });
  document.addEventListener("keydown", function (e) {
    var t = e.target, typing = t && (t.isContentEditable || /^(INPUT|TEXTAREA|SELECT)$/.test(t.tagName));
    if ((e.key === "/" && !typing) || (e.key.toLowerCase() === "k" && (e.ctrlKey || e.metaKey))) {
      e.preventDefault();
      input.focus();
      input.select();
    }
  });

  var q = new URLSearchParams(window.location.search).get("q");
  if (q) {
    input.value = q;
    load(function () { show(); input.focus(); });
  }
})();
