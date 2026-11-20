// A loop that never ends stops an app, not the page (DEV-1). An app's frame
// runs in the page's own process, so a `while (true)` there freezes the chat
// too. Every `while`, `do … while` and three-part `for` of an app's own
// scripts gets a guard in its condition: the kit's `__iaLoop()`, which
// throws once one task has looped for more than two seconds. A `for … of`
// or `for … in` walks something that ends, and is left as it is.

const WORD = /[A-Za-z0-9_$]/
// After these, a `/` starts a regular expression, not a division.
const BEFORE_REGEX = new Set(['', '(', ',', '=', ':', '[', '!', '&', '|', '?', '{', '}', ';', '+', '-', '*', '%', '<', '>', '~', '^', 'return', 'typeof', 'case', 'do', 'else', 'in', 'of', 'new', 'delete', 'void', 'throw', 'yield', 'await', '=>'])

// The places of code (not strings, comments, templates' text or regular
// expressions) as a list of [start, end) spans, with each word token.
function scan(js) {
  const tokens = []
  let i = 0
  let last = ''
  const n = js.length
  const templ = [] // brace depth at each `${` of an open template
  let depth = 0
  while (i < n) {
    const c = js[i]
    if (c === '/' && js[i + 1] === '/') {
      while (i < n && js[i] !== '\n') i++
      continue
    }
    if (c === '/' && js[i + 1] === '*') {
      const end = js.indexOf('*/', i + 2)
      i = end < 0 ? n : end + 2
      continue
    }
    if (c === '"' || c === "'") {
      i++
      while (i < n && js[i] !== c && js[i] !== '\n') i += js[i] === '\\' ? 2 : 1
      i++
      last = 'str'
      continue
    }
    if (c === '`' || (c === '}' && templ.length && templ.at(-1) === depth)) {
      if (c === '}') templ.pop()
      i++
      while (i < n && js[i] !== '`') {
        if (js[i] === '\\') i += 2
        else if (js[i] === '$' && js[i + 1] === '{') {
          templ.push(depth)
          i += 2
          break
        } else i++
      }
      if (js[i] === '`') i++
      last = 'str'
      continue
    }
    if (c === '/' && BEFORE_REGEX.has(last)) {
      i++
      let cls = false
      while (i < n && js[i] !== '\n') {
        if (js[i] === '\\') i += 2
        else if (js[i] === '[') (cls = true), i++
        else if (js[i] === ']') (cls = false), i++
        else if (js[i] === '/' && !cls) break
        else i++
      }
      i++
      while (i < n && /[a-z]/i.test(js[i])) i++
      last = 'str'
      continue
    }
    if (WORD.test(c)) {
      const start = i
      while (i < n && WORD.test(js[i])) i++
      const word = js.slice(start, i)
      tokens.push({ word, start, end: i })
      last = /^[0-9]/.test(word) ? 'num' : ['return', 'typeof', 'case', 'do', 'else', 'in', 'of', 'new', 'delete', 'void', 'throw', 'yield', 'await'].includes(word) ? word : 'id'
      continue
    }
    if (/\s/.test(c)) {
      i++
      continue
    }
    if (c === '=' && js[i + 1] === '>') {
      tokens.push({ punct: '=>', start: i, end: i + 2 })
      last = '=>'
      i += 2
      continue
    }
    if (c === '{') depth++
    if (c === '}') depth--
    tokens.push({ punct: c, start: i, end: i + 1 })
    last = c === ')' || c === ']' ? 'id' : c
    i++
  }
  return tokens
}

// Guards each loop's condition; what can't be read is left as it is.
export function guardLoops(js) {
  let tokens
  try {
    tokens = scan(js)
  } catch {
    return js
  }
  const edits = [] // [at, text] inserts, or [from, to, text] replacements
  for (let k = 0; k < tokens.length; k++) {
    const t = tokens[k]
    if (t.word !== 'while' && t.word !== 'for') continue
    if (tokens[k - 1]?.punct === '.') continue
    let open = k + 1
    if (t.word === 'for' && tokens[open]?.word === 'await') continue
    if (tokens[open]?.punct !== '(') continue
    // The matching `)` and, for a `for`, its two `;` at the top.
    let d = 0
    let close = -1
    const semis = []
    for (let j = open; j < tokens.length; j++) {
      const p = tokens[j].punct
      if (p === '(' || p === '[' || p === '{') d++
      else if (p === ')' || p === ']' || p === '}') {
        d--
        if (d === 0) {
          close = j
          break
        }
      } else if (p === ';' && d === 1) semis.push(j)
    }
    if (close < 0) continue
    if (t.word === 'while') {
      edits.push([tokens[open].end, '__iaLoop() && ('], [tokens[close].start, ')'])
    } else if (semis.length === 2) {
      const a = tokens[semis[0]].end
      const b = tokens[semis[1]].start
      const cond = js.slice(a, b)
      edits.push(cond.trim() ? [a, ' __iaLoop() && ('] : [a, ' __iaLoop()'])
      if (cond.trim()) edits.push([b, ')'])
    }
  }
  if (!edits.length) return js
  edits.sort((x, y) => y[0] - x[0])
  let out = js
  for (const [at, text] of edits) out = out.slice(0, at) + text + out.slice(at)
  return out
}

// Each inline script of an app's own source, guarded.
export function guardScripts(html) {
  return html.replace(/(<script\b(?![^>]*\bsrc=)[^>]*>)([\s\S]*?)(<\/script>)/gi, (_, open, body, close) => open + guardLoops(body) + close)
}

// The guard itself, a plain script at the top of an app's page, so it is
// there before any script of the app runs: a loop that runs more than two
// seconds in one go is stopped, and the app's error says so.
export const LOOP_GUARD = `<script>(()=>{let start=0,turns=0;globalThis.__iaLoop=()=>{if((++turns&1023)!==0)return true;const now=performance.now();if(!start){start=now;setTimeout(()=>{start=0},0);return true}if(now-start>2000){start=0;throw new Error('This app ran a loop that never ended, so it was stopped after 2 seconds. Ask to fix it.')}return true}})()</script>`
