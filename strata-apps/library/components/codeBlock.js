// title: Code block
// description: Code shown with line numbers and light syntax colouring (keywords, strings, comments, numbers), with a header naming the file or language and a Copy button; for snippets in notes, docs, recipes for scripts, answers and logs.
// props: code: the text to show (string; a trailing newline is dropped); language: 'javascript' | 'typescript' | 'python' | 'json' | 'html' | 'css' | 'sql' | 'bash' | 'text'
//   (aliases js, jsx, ts, tsx, py, sh, shell, zsh, xml, svg, scss; default 'text'; an unknown language shows uncoloured);
//   lineNumbers: false to hide the numbers (default true); start: the number of the first line (default 1);
//   highlight: [line numbers] to mark, counted as shown (default none); title: a file name or caption for the header (default the language's name);
//   copy: false to hide the Copy button (default true); wrap: true to wrap long lines (default false: they scroll sideways);
//   maxHeight: pixels after which the code scrolls (default none)
function component(props) {
  css('sa-code', `
.sa-code{margin:0;border:1px solid var(--line);border-radius:var(--radius);background:var(--card);overflow:hidden;font-size:13px;min-width:0;max-width:100%}
.sa-code .hd{display:flex;align-items:center;gap:8px;padding:4px 6px 4px 12px;border-bottom:1px solid var(--line);font-size:12px;color:var(--muted)}
.sa-code .hd .nm{flex:1;min-width:0;overflow:hidden;text-overflow:ellipsis;white-space:nowrap}
.sa-code .hd button{font-size:12px;padding:2px 10px;margin:0}
.sa-code pre{margin:0;padding:8px 0;overflow:auto;background:none;border:0;border-radius:0;font-family:ui-monospace,SFMono-Regular,Menlo,Consolas,monospace;font-size:inherit;line-height:1.5;tab-size:4;-moz-tab-size:4;color:var(--fg)}
.sa-code pre:focus-visible{outline:2px solid var(--accent);outline-offset:-2px}
.sa-code code{display:block;min-width:max-content;background:none;border:0;padding:0;font:inherit;color:inherit}
.sa-code.wrap code{min-width:0}
.sa-code .ln{display:flex;min-height:1.5em}
.sa-code .no{position:sticky;left:0;flex:none;box-sizing:border-box;width:calc(var(--nw) + 24px);padding:0 12px;text-align:right;color:var(--muted);background:var(--card);user-select:none;-webkit-user-select:none}
.sa-code .tx{flex:1;white-space:pre;padding:0 12px}
.sa-code.wrap .tx{white-space:pre-wrap;overflow-wrap:anywhere;min-width:0}
.sa-code .ln.hl,.sa-code .ln.hl .no{background:color-mix(in srgb,var(--accent) 14%,var(--card))}
.sa-code .ln.hl .no{box-shadow:inset 3px 0 var(--accent);color:var(--fg)}
.sa-code .t-kw{color:color-mix(in srgb,#8b5cf6 78%,var(--fg))}
.sa-code .t-str{color:color-mix(in srgb,#16a34a 78%,var(--fg))}
.sa-code .t-com{color:var(--muted);font-style:italic}
.sa-code .t-num{color:color-mix(in srgb,#ea580c 80%,var(--fg))}
.sa-code .t-lit{color:color-mix(in srgb,#0891b2 80%,var(--fg))}
.sa-code .t-fn{color:color-mix(in srgb,#2563eb 78%,var(--fg))}
.sa-code .t-prop,.sa-code .t-attr{color:color-mix(in srgb,#0e7490 75%,var(--fg))}
.sa-code .t-tag{color:color-mix(in srgb,#db2777 78%,var(--fg))}
.sa-code .t-var{color:color-mix(in srgb,#ca8a04 80%,var(--fg))}
.sa-code .t-meta{color:color-mix(in srgb,#64748b 80%,var(--fg))}
`);

  const words = (s) => new Set(s.split(' '));
  const r = (t, re, next, ok) => ({ t, re, next, ok });
  const WS = r(null, /\s+/y);
  const NUM = r('num', /(?:0[xX][\da-fA-F_]+|0[bB][01_]+|0[oO][0-7_]+|(?:\d[\d_]*\.?[\d_]*|\.\d[\d_]*)(?:[eE][+-]?\d+)?)n?/y);
  const DQ = r('str', /"(?:\\[\s\S]|[^"\\\n])*"?/y);
  const SQ = r('str', /'(?:\\[\s\S]|[^'\\\n])*'?/y);
  const BLOCK = r('com', /\/\*[\s\S]*?(?:\*\/|$)/y);
  const SLASH = r('com', /\/\/[^\n]*/y);

  const LANGS = {
    javascript: {
      name: 'JavaScript',
      kw: words('break case catch class const continue debugger default delete do else export extends finally for from function get if import in instanceof let new of return set static super switch throw try typeof var void while with yield async await as interface type enum implements private public protected readonly declare namespace abstract keyof satisfies'),
      lit: words('true false null undefined NaN Infinity this'),
      states: { main: [WS, SLASH, BLOCK, DQ, SQ, r('str', /`(?:\\[\s\S]|[^`\\])*`?/y), NUM, r('ident', /[A-Za-z_$][\w$]*/y)] },
    },
    python: {
      name: 'Python',
      kw: words('and as assert async await break class continue def del elif else except finally for from global if import in is lambda nonlocal not or pass raise return try while with yield match case'),
      lit: words('True False None self cls'),
      states: { main: [WS, r('com', /#[^\n]*/y),
        r('str', /[rRbBuUfF]{0,2}(?:"""[\s\S]*?(?:"""|$)|'''[\s\S]*?(?:'''|$))/y),
        r('str', /[rRbBuUfF]{0,2}(?:"(?:\\[\s\S]|[^"\\\n])*"?|'(?:\\[\s\S]|[^'\\\n])*'?)/y),
        r('meta', /@[A-Za-z_][\w.]*/y), NUM, r('ident', /[A-Za-z_]\w*/y)] },
    },
    json: {
      name: 'JSON', kw: new Set(), lit: words('true false null'),
      states: { main: [WS, r('prop', /"(?:\\[\s\S]|[^"\\\n])*"(?=\s*:)/y), DQ, r('num', /-?\d+(?:\.\d+)?(?:[eE][+-]?\d+)?/y), r('ident', /[A-Za-z_]\w*/y)] },
    },
    css: {
      name: 'CSS', kw: new Set(), lit: words('inherit initial unset none auto transparent'),
      states: { main: [WS, BLOCK, SLASH, DQ, SQ, r('kw', /@[\w-]+/y), r('kw', /!important\b/y),
        r('num', /#[\da-fA-F]{3,8}(?![\w-])/y), r('num', /-?(?:\d+\.?\d*|\.\d+)(?:%|[a-zA-Z]+)?/y),
        r('prop', /--?[A-Za-z_][\w-]*(?=\s*:(?!:))/y), r('ident', /[A-Za-z_-][\w-]*/y)] },
    },
    sql: {
      name: 'SQL', ci: true,
      kw: words('select from where and or not in is like between join left right inner outer full cross on as group by order having limit offset union all distinct insert into values update set delete create table alter drop index primary key foreign references default case when then else end with exists asc desc match'),
      lit: words('null true false'),
      states: { main: [WS, r('com', /--[^\n]*/y), BLOCK, r('str', /'(?:''|[^'])*'?/y), r('str', /"(?:""|[^"])*"?/y),
        r('var', /:[A-Za-z_]\w*/y), NUM, r('ident', /[A-Za-z_]\w*/y)] },
    },
    bash: {
      name: 'Shell',
      kw: words('if then else elif fi for while until do done case esac in function return local export readonly echo cd exit source set unset shift sudo'),
      lit: words('true false'),
      states: { main: [WS, r('com', /#[^\n]*/y, null, (s, i) => i === 0 || /\s/.test(s[i - 1])),
        r('str', /"(?:\\[\s\S]|[^"\\])*"?/y), r('str', /'[^']*'?/y),
        r('var', /\$(?:\{[^}\n]*\}?|\w+|[@#?*!$-])/y), r('num', /\d+(?:\.\d+)?(?![\w-])/y), r('ident', /[A-Za-z_][\w-]*/y)] },
    },
    html: {
      name: 'HTML', kw: new Set(), lit: new Set(),
      states: {
        main: [WS, r('com', /<!--[\s\S]*?(?:-->|$)/y), r('meta', /<![^>]*>?/y), r('tag', /<\/?[A-Za-z][\w:.-]*/y, 'tag'), r('num', /&#?\w+;/y)],
        tag: [WS, r('tag', /\/?>/y, 'main'), r('str', /"[^"]*"?/y), r('str', /'[^']*'?/y), r('attr', /[^\s"'=<>\/]+/y)],
      },
    },
  };
  const ALIAS = { js: 'javascript', jsx: 'javascript', mjs: 'javascript', ts: 'javascript', tsx: 'javascript', typescript: 'javascript',
    py: 'python', sh: 'bash', shell: 'bash', zsh: 'bash', console: 'bash', xml: 'html', svg: 'html', htm: 'html', vue: 'html', scss: 'css' };

  const raw = String(props.language || 'text').trim().toLowerCase();
  const key = ALIAS[raw] || raw;
  const lang = LANGS[key] || null;
  const langName = /^(typescript|ts|tsx)$/.test(raw) ? 'TypeScript' : raw === 'xml' ? 'XML' : lang ? lang.name : (raw && raw !== 'text' && raw !== 'plain' ? raw : '');

  let code = props.code == null ? '' : String(props.code);
  code = code.replace(/\r\n?/g, '\n');
  const shown = code.endsWith('\n') ? code.slice(0, -1) : code;

  function tokenize(text) {
    if (!lang) return [[null, text]];
    const out = [];
    let state = 'main', i = 0, plain = '';
    const flush = () => { if (plain) { out.push([null, plain]); plain = ''; } };
    while (i < text.length) {
      let hit = false;
      for (const rule of lang.states[state]) {
        if (rule.ok && !rule.ok(text, i)) continue;
        rule.re.lastIndex = i;
        const m = rule.re.exec(text);
        if (!m || !m[0]) continue;
        let t = rule.t;
        if (t === 'ident') {
          const w = lang.ci ? m[0].toLowerCase() : m[0];
          t = lang.kw.has(w) ? 'kw' : lang.lit.has(w) ? 'lit' : text[i + m[0].length] === '(' ? 'fn' : null;
        }
        if (t) { flush(); out.push([t, m[0]]); } else plain += m[0];
        i += m[0].length;
        if (rule.next) state = rule.next;
        hit = true;
        break;
      }
      if (!hit) plain += text[i++];
    }
    flush();
    return out;
  }

  const lines = [[]];
  for (const [t, v] of tokenize(shown)) {
    v.split('\n').forEach((part, k) => {
      if (k) lines.push([]);
      if (part) lines[lines.length - 1].push(t ? h('span', { class: 't-' + t }, part) : part);
    });
  }

  const start = Number.isFinite(Number(props.start)) && props.start !== null && props.start !== '' ? Math.floor(Number(props.start)) : 1;
  const numbers = props.lineNumbers !== false;
  const marks = new Set([].concat(props.highlight == null ? [] : props.highlight).map(Number));
  const nw = Math.max(String(start).length, String(start + lines.length - 1).length);

  const body = h('code', null, lines.map((parts, k) => {
    const n = start + k;
    return h('div', { class: marks.has(n) ? 'ln hl' : 'ln' },
      numbers ? h('span', { class: 'no', 'aria-hidden': 'true' }, String(n)) : null,
      h('span', { class: 'tx' }, parts));
  }));
  const maxH = Number(props.maxHeight);
  const pre = h('pre', { tabindex: '0', style: maxH > 0 ? `max-height:${maxH}px` : null }, body);

  let header = null;
  const title = props.title ? String(props.title) : langName;
  if (title || props.copy !== false) {
    let btn = null;
    if (props.copy !== false) {
      let timer = null;
      btn = h('button', { type: 'button', class: 'ghost small', 'aria-label': 'Copy code', onclick: async () => {
        let ok = false;
        try { await navigator.clipboard.writeText(code); ok = true; } catch (e) {
          try {
            const ta = h('textarea', { style: 'position:fixed;top:0;left:0;opacity:0' });
            ta.value = code;
            document.body.append(ta);
            ta.select();
            ok = document.execCommand('copy');
            ta.remove();
          } catch (e2) { ok = false; }
        }
        btn.textContent = ok ? 'Copied' : 'Copy failed';
        clearTimeout(timer);
        timer = setTimeout(() => { btn.textContent = 'Copy'; }, 1500);
      } }, 'Copy');
    }
    header = h('div', { class: 'hd' }, h('span', { class: 'nm', title: title || null }, title || ''), btn);
  }

  const cls = `sa-code${props.wrap ? ' wrap' : ''}${numbers ? '' : ' nonum'}`;
  return h('figure', { class: cls, style: `--nw:${nw}ch`, role: 'group', 'aria-label': title ? `${title} code` : 'Code' }, header, pre);
}
