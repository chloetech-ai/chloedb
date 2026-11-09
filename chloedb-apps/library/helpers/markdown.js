// title: Markdown
// description: Reads Markdown into a tree of plain objects that a view or email can render without raw HTML: headings, paragraphs, lists and task lists, quotes, code, links and images; also its plain text, an excerpt, its headings, and ticking a task in the source.
// api: parse(text) -> blocks: {type: 'heading', level, children} | {type: 'paragraph', children} | {type: 'list', ordered, start, items: [{checked: null | true | false, task, children}]}
//   | {type: 'quote', children: blocks} | {type: 'code', lang, text} | {type: 'rule'};
//   inline children: {type: 'text', text} | {type: 'strong' | 'em' | 'del', children} | {type: 'code', text} | {type: 'link', href, children}
//   | {type: 'image', src, alt} | {type: 'break'}; links only http(s) and mailto; image src is https, data:image or file:<id> (an app's file);
//   plain(text) -> the words without markup; excerpt(text, length = 160) -> plain text cut at a word;
//   headings(text) -> [{level, text, slug}]; tasks(text) -> [{index, checked, text}];
//   toggleTask(text, index, checked?) -> the source with that task ticked or unticked
function helper() {
  function safe(url, image) {
    const u = String(url || '').trim();
    if (/^https?:\/\/[^\s]+$/i.test(u)) return u;
    if (!image && /^mailto:[^\s]+$/i.test(u)) return u;
    if (image && /^data:image\/(png|jpeg|gif|webp);base64,[a-z0-9+/=]+$/i.test(u)) return u;
    if (image && /^file:[a-z0-9]{6,20}$/i.test(u)) return u;
    return null;
  }
  function inline(s) {
    const out = [];
    let buf = '';
    const flush = () => { if (buf) { out.push({ type: 'text', text: buf }); buf = ''; } };
    let i = 0;
    while (i < s.length) {
      const rest = s.slice(i);
      const prev = i ? s[i - 1] : ' ';
      let m;
      if (rest[0] === '\\' && /[\\`*_~[\]()#!>+-]/.test(rest[1] || '')) { buf += rest[1]; i += 2; continue; }
      if ((m = rest.match(/^`([^`]+)`/))) { flush(); out.push({ type: 'code', text: m[1] }); i += m[0].length; continue; }
      if ((m = rest.match(/^!\[([^\]]*)\]\(([^)\s]+)(?:\s+"[^"]*")?\)/))) { flush(); const src = safe(m[2], true); if (src) out.push({ type: 'image', src, alt: m[1] }); else buf += m[1]; i += m[0].length; continue; }
      if ((m = rest.match(/^\[([^\]]+)\]\(([^)\s]+)(?:\s+"[^"]*")?\)/))) { flush(); const href = safe(m[2]); out.push(href ? { type: 'link', href, children: inline(m[1]) } : { type: 'text', text: m[1] }); i += m[0].length; continue; }
      if ((m = rest.match(/^(\*\*|__)(?=\S)([\s\S]*?\S)\1/)) && (m[1] === '**' || !/\w/.test(prev))) { flush(); out.push({ type: 'strong', children: inline(m[2]) }); i += m[0].length; continue; }
      if ((m = rest.match(/^~~(?=\S)([\s\S]*?\S)~~/))) { flush(); out.push({ type: 'del', children: inline(m[1]) }); i += m[0].length; continue; }
      if ((m = rest.match(/^(\*|_)(?=[^\s*_])([\s\S]*?[^\s\\])\1(?![*_\w])/)) && (m[1] === '*' || !/\w/.test(prev))) { flush(); out.push({ type: 'em', children: inline(m[2]) }); i += m[0].length; continue; }
      if (!/\w/.test(prev) && (m = rest.match(/^https?:\/\/[^\s<>()]*[^\s<>().,;:!?'"]/))) { flush(); out.push({ type: 'link', href: m[0], children: [{ type: 'text', text: m[0] }] }); i += m[0].length; continue; }
      if (rest[0] === '\n') { flush(); out.push({ type: 'break' }); i++; continue; }
      buf += rest[0];
      i++;
    }
    flush();
    return out;
  }
  const LIST = /^(\s*)([-*+]|(\d{1,9})[.)])\s+(.*)$/;
  function blocks(lines) {
    const out = [];
    let i = 0;
    while (i < lines.length) {
      const line = lines[i];
      let m;
      if (!line.trim()) { i++; continue; }
      if ((m = line.match(/^\s*(```|~~~)\s*([\w+-]*)\s*$/))) {
        const fence = m[1]; const body = [];
        i++;
        while (i < lines.length && !lines[i].trim().startsWith(fence)) body.push(lines[i++]);
        i++;
        out.push({ type: 'code', lang: m[2] || '', text: body.join('\n') });
        continue;
      }
      if ((m = line.match(/^\s{0,3}(#{1,6})\s+(.*?)\s*#*\s*$/))) { out.push({ type: 'heading', level: m[1].length, children: inline(m[2]) }); i++; continue; }
      if (/^\s{0,3}([-*_])(\s*\1){2,}\s*$/.test(line)) { out.push({ type: 'rule' }); i++; continue; }
      if (/^\s{0,3}>/.test(line)) {
        const body = [];
        while (i < lines.length && /^\s{0,3}>/.test(lines[i])) body.push(lines[i++].replace(/^\s{0,3}>\s?/, ''));
        out.push({ type: 'quote', children: blocks(body) });
        continue;
      }
      if ((m = line.match(LIST))) {
        const ordered = !!m[3];
        const list = { type: 'list', ordered, start: ordered ? Number(m[3]) : 1, items: [] };
        while (i < lines.length && (m = lines[i].match(LIST)) && !!m[3] === ordered) {
          let text = m[4];
          i++;
          while (i < lines.length && lines[i].trim() && !LIST.test(lines[i]) && /^\s{2,}/.test(lines[i])) text += `\n${lines[i++].trim()}`;
          const t = text.match(/^\[([ xX])\]\s+(.*)$/s);
          list.items.push({ checked: t ? t[1] !== ' ' : null, task: !!t, children: inline(t ? t[2] : text) });
        }
        out.push(list);
        continue;
      }
      const para = [];
      while (i < lines.length && lines[i].trim() && !LIST.test(lines[i]) && !/^\s{0,3}(#{1,6}\s|>|```|~~~)/.test(lines[i]) && !/^\s{0,3}([-*_])(\s*\1){2,}\s*$/.test(lines[i])) para.push(lines[i++].trim());
      out.push({ type: 'paragraph', children: inline(para.join('\n')) });
    }
    return out;
  }
  const parse = (text) => blocks(String(text || '').replace(/\r\n?/g, '\n').split('\n'));
  function textOf(nodes) {
    let s = '';
    for (const n of nodes || []) {
      if (n.type === 'text' || n.type === 'code') s += n.text;
      else if (n.type === 'break') s += ' ';
      else if (n.type === 'image') s += n.alt || '';
      else if (n.children) s += textOf(n.children);
      else if (n.items) s += n.items.map((it) => textOf(it.children)).join(' ');
      if (['heading', 'paragraph', 'list', 'quote', 'code'].includes(n.type)) s += ' ';
    }
    return s;
  }
  const plain = (text) => textOf(parse(text)).replace(/\s+/g, ' ').trim();
  function excerpt(text, length = 160) {
    const p = plain(text);
    if (p.length <= length) return p;
    const cut = p.slice(0, length);
    return `${cut.slice(0, Math.max(cut.lastIndexOf(' '), length * 0.6)).trim()}…`;
  }
  const slug = (s) => s.toLowerCase().normalize('NFKD').replace(/[̀-ͯ]/g, '').replace(/[^a-z0-9]+/g, '-').replace(/^-|-$/g, '');
  const headings = (text) => parse(text).filter((b) => b.type === 'heading').map((b) => { const t = textOf(b.children).trim(); return { level: b.level, text: t, slug: slug(t) }; });
  const TASK = /^(\s*(?:[-*+]|\d{1,9}[.)])\s+)\[([ xX])\](\s+)(.*)$/;
  function tasks(text) {
    const out = [];
    for (const line of String(text || '').split('\n')) { const m = line.match(TASK); if (m) out.push({ index: out.length, checked: m[2] !== ' ', text: m[4] }); }
    return out;
  }
  function toggleTask(text, index, checked) {
    let n = -1;
    return String(text || '').split('\n').map((line) => {
      const m = line.match(TASK);
      if (!m || ++n !== index) return line;
      const on = checked === undefined ? m[2] === ' ' : !!checked;
      return `${m[1]}[${on ? 'x' : ' '}]${m[3]}${m[4]}`;
    }).join('\n');
  }
  return { parse, plain, excerpt, headings, tasks, toggleTask };
}
