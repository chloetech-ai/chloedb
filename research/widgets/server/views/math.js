// Math for apps that work things out (a calculator, a function and its
// derivative, a simulation): Kit.math reads a formula as a person types it,
// evaluates it, takes its exact derivative and integrates; Kit.plot draws
// functions on a canvas in the chat's theme. A formula is a tree evaluated
// by closures, never by eval, which the page's policy may forbid.
//
//   const e = Kit.math.parse('x^2 sin x')   // throws an Error saying what is wrong
//   e.text, e.html                          // 'x^2·sin(x)', with <sup> for powers
//   e.f(2), e.eval({ x: 2, a: 1 }), e.vars  // values; ['x']
//   e.derive('x').text                      // '2x·sin(x) + x^2·cos(x)'
;(() => {
  const FUNCS = {
    sin: Math.sin, cos: Math.cos, tan: Math.tan,
    sec: x => 1 / Math.cos(x), csc: x => 1 / Math.sin(x), cot: x => 1 / Math.tan(x),
    asin: Math.asin, acos: Math.acos, atan: Math.atan,
    sinh: Math.sinh, cosh: Math.cosh, tanh: Math.tanh,
    exp: Math.exp, ln: Math.log, log: Math.log10,
    sqrt: Math.sqrt, cbrt: Math.cbrt, abs: Math.abs, sign: Math.sign,
  }
  const ALIAS = { arcsin: 'asin', arccos: 'acos', arctan: 'atan', sgn: 'sign' }
  const INVERSE = { sin: 'asin', cos: 'acos', tan: 'atan' }
  const CONSTS = { pi: Math.PI, e: Math.E, tau: 2 * Math.PI }
  const NAMES = [...Object.keys(FUNCS), ...Object.keys(ALIAS), ...Object.keys(CONSTS)].sort((a, b) => b.length - a.length)

  // ---- Reading ----
  function tokens(text) {
    const src = String(text ?? '')
      .replace(/\*\*/g, '^').replace(/[×·∙⋅]/g, '*').replace(/÷/g, '/').replace(/[−–]/g, '-')
      .replace(/²/g, '^2').replace(/³/g, '^3').replace(/π/g, 'pi').replace(/√/g, 'sqrt')
    const out = []
    let i = 0
    while (i < src.length) {
      const rest = src.slice(i)
      if (/^\s/.test(rest)) { i++; continue }
      const num = /^(\d+\.?\d*|\.\d+)(e[+-]?\d+)?/i.exec(rest)
      if (num) { out.push({ k: 'num', v: Number(num[0]), at: i }); i += num[0].length; continue }
      const word = /^[a-zA-Z]+/.exec(rest)
      if (word) {
        // "sinx" is sin x and "xy" is x times y: known names first, else one letter.
        let w = word[0], at = i
        while (w) {
          const name = NAMES.find(n => w.toLowerCase().startsWith(n)) ?? w[0]
          const key = name.toLowerCase()
          if (FUNCS[key] || ALIAS[key]) out.push({ k: 'fn', v: ALIAS[key] ?? key, at })
          else if (key in CONSTS && name.length > 1 || (key === 'e' && name === 'e')) out.push({ k: 'const', v: key, at })
          else out.push({ k: 'var', v: name, at })
          w = w.slice(name.length)
          at += name.length
        }
        i += word[0].length
        continue
      }
      if ('+-*/^(),|!'.includes(rest[0])) { out.push({ k: rest[0], at: i }); i++; continue }
      throw new Error(`I can't read "${rest[0]}" at position ${i + 1}`)
    }
    return out
  }

  function parseTree(text) {
    const ts = tokens(text)
    if (!ts.length) throw new Error('Type a formula, such as x^2 sin(x)')
    let p = 0
    let bars = 0
    const peek = () => ts[p]
    const take = k => (ts[p]?.k === k ? ts[p++] : null)
    const where = () => (ts[p] ? `at position ${ts[p].at + 1}` : 'at the end')
    const starts = t => t && (t.k === 'num' || t.k === 'var' || t.k === 'const' || t.k === 'fn' || t.k === '(' || (t.k === '|' && bars === 0))

    function expr() {
      let a = term()
      for (;;) {
        if (take('+')) a = node('+', a, term())
        else if (take('-')) a = node('-', a, term())
        else return a
      }
    }
    function term() {
      let a = unary()
      for (;;) {
        if (take('*')) a = node('*', a, unary())
        else if (take('/')) a = node('/', a, unary())
        else if (starts(peek())) a = node('*', a, power()) // 2x, x(x+1), x sin x
        else return a
      }
    }
    function unary() {
      if (take('-')) return { t: 'neg', a: unary() }
      if (take('+')) return unary()
      return power()
    }
    function power() {
      const base = postfix()
      if (take('^')) return node('^', base, unary())
      return base
    }
    function postfix() {
      let a = primary()
      while (take('!')) a = { t: 'fn', n: 'fact', a }
      return a
    }
    function primary() {
      const t = peek()
      if (!t) throw new Error('The formula ends too soon')
      if (take('num')) return { t: 'num', v: t.v }
      if (take('var')) return { t: 'var', n: t.v }
      if (take('const')) return { t: 'const', n: t.v }
      if (take('(')) {
        const a = expr()
        if (!take(')')) throw new Error(`Missing ) ${where()}`)
        return a
      }
      if (t.k === '|') {
        p++
        bars++
        const a = expr()
        bars--
        if (!take('|')) throw new Error(`Missing closing | ${where()}`)
        return { t: 'fn', n: 'abs', a }
      }
      if (take('fn')) {
        let name = t.v
        let exponent = null
        if (take('^')) exponent = unary() // sin^2 x
        if (exponent && exponent.t === 'neg' && exponent.a.t === 'num' && exponent.a.v === 1 && INVERSE[name]) {
          name = INVERSE[name]
          exponent = null
        }
        let a
        if (peek()?.k === '(') {
          p++
          a = expr()
          if (!take(')')) throw new Error(`Missing ) after ${name}(… ${where()}`)
        } else {
          if (!starts(peek()) && peek()?.k !== '-') throw new Error(`${name} needs something to work on, as in ${name}(x)`)
          a = peek()?.k === '-' ? unary() : power()
          if (a.t === 'num' && starts(peek())) a = node('*', a, power()) // sin 2x
        }
        const call = { t: 'fn', n: name, a }
        return exponent ? node('^', call, exponent) : call
      }
      throw new Error(`Unexpected "${t.k === 'fn' || t.k === 'var' ? t.v : t.k}" ${where()}`)
    }

    const tree = expr()
    if (p < ts.length) throw new Error(`Unexpected "${ts[p].v ?? ts[p].k}" ${where()}`)
    return tree
  }

  // ---- Values ----
  function realPow(a, b) {
    if (a >= 0 || Number.isInteger(b)) return Math.pow(a, b)
    // A negative base with an odd root (x^(1/3), x^(2/3)) has a real value.
    for (let q = 3; q <= 15; q += 2) {
      const n = b * q
      if (Math.abs(n - Math.round(n)) < 1e-9) return (Math.round(n) % 2 ? -1 : 1) * Math.pow(-a, b)
    }
    return NaN
  }
  function fact(n) {
    if (n < 0 || !Number.isInteger(n)) return gamma(n + 1)
    let r = 1
    for (let i = 2; i <= n && i < 171; i++) r *= i
    return n >= 171 ? Infinity : r
  }
  function gamma(z) {
    if (z < 0.5) return Math.PI / (Math.sin(Math.PI * z) * gamma(1 - z))
    const g = 7, c = [0.99999999999980993, 676.5203681218851, -1259.1392167224028, 771.32342877765313, -176.61502916214059, 12.507343278686905, -0.13857109526572012, 9.9843695780195716e-6, 1.5056327351493116e-7]
    z -= 1
    let x = c[0]
    for (let i = 1; i < g + 2; i++) x += c[i] / (z + i)
    const t = z + g + 0.5
    return Math.sqrt(2 * Math.PI) * Math.pow(t, z + 0.5) * Math.exp(-t) * x
  }
  function compile(n) {
    switch (n.t) {
      case 'num': { const v = n.v; return () => v }
      case 'const': { const v = CONSTS[n.n]; return () => v }
      case 'var': { const k = n.n; return env => (k in env ? env[k] : NaN) }
      case 'neg': { const a = compile(n.a); return env => -a(env) }
      case 'fn': {
        const f = n.n === 'fact' ? fact : FUNCS[n.n]
        const a = compile(n.a)
        return env => f(a(env))
      }
      default: {
        const a = compile(n.a), b = compile(n.b)
        if (n.t === '+') return env => a(env) + b(env)
        if (n.t === '-') return env => a(env) - b(env)
        if (n.t === '*') return env => a(env) * b(env)
        if (n.t === '/') return env => a(env) / b(env)
        return env => realPow(a(env), b(env))
      }
    }
  }
  function varsOf(n, out = new Set()) {
    if (n.t === 'var') out.add(n.n)
    if (n.a) varsOf(n.a, out)
    if (n.b) varsOf(n.b, out)
    return out
  }
  const has = (n, x) => varsOf(n).has(x)

  // ---- Simplifying ----
  const num = v => ({ t: 'num', v })
  const isNum = (n, v) => n.t === 'num' && (v === undefined || n.v === v)
  function node(t, a, b) {
    return { t, a, b }
  }
  // A sum as [coefficient, term] pairs, and a product as coefficient and factors.
  function terms(n, sign = 1, out = []) {
    if (n.t === '+') { terms(n.a, sign, out); terms(n.b, sign, out) }
    else if (n.t === '-') { terms(n.a, sign, out); terms(n.b, -sign, out) }
    else if (n.t === 'neg') terms(n.a, -sign, out)
    else {
      const [c, rest] = split(n)
      out.push([sign * c, rest])
    }
    return out
  }
  function split(n) {
    if (n.t === 'num') return [n.v, null]
    if (n.t === '/' && n.a.t === 'num' && n.b.t === 'num') return [n.a.v / n.b.v, null]
    if (n.t === 'neg') { const [c, r] = split(n.a); return [-c, r] }
    if (n.t === '*') {
      const [c1, r1] = split(n.a)
      const [c2, r2] = split(n.b)
      return [c1 * c2, r1 && r2 ? node('*', r1, r2) : r1 ?? r2]
    }
    return [1, n]
  }
  function factors(n, out = []) {
    if (n.t === '*') { factors(n.a, out); factors(n.b, out) }
    else out.push(n)
    return out
  }
  // Factors in the order people write them: 2πx²·e^x·sin(x).
  function rank(f) {
    if (f.t === 'const') return 1
    const b = f.t === '^' ? f.a : f
    if (b.t === 'var') return 2
    if (b.t === 'const') return 3
    if (b.t === 'fn') return 4
    return 5
  }
  function baseOf(f) {
    return f.t === '^' ? [f.a, f.b] : [f, num(1)]
  }
  function product(c, list) {
    // Same bases add their powers: x·x^2 is x^3.
    const merged = []
    for (const f of list) {
      const [b, e] = baseOf(f)
      const k = text(b)
      const at = merged.find(m => m.k === k)
      if (at) at.e = simplify(node('+', at.e, e))
      else merged.push({ k, b, e })
    }
    let fs = merged.map(m => simplify(node('^', m.b, m.e))).filter(f => !isNum(f, 1))
    for (const f of fs) if (f.t === 'num') { c *= f.v }
    fs = fs.filter(f => f.t !== 'num')
    fs.sort((p, q) => rank(p) - rank(q))
    if (c === 0) return num(0)
    if (!fs.length) return num(c)
    const body = fs.reduce((a, b) => node('*', a, b))
    if (c === 1) return body
    if (c === -1) return { t: 'neg', a: body }
    return c < 0 ? { t: 'neg', a: node('*', num(-c), body) } : node('*', num(c), body)
  }
  function tidy(v) {
    return Math.abs(v - Math.round(v)) < 1e-12 ? Math.round(v) : v
  }
  function simplify(n) {
    if (n.t === 'num' || n.t === 'var' || n.t === 'const') return n
    if (n.t === 'fn') {
      const a = simplify(n.a)
      if (n.n === 'ln' && a.t === 'const' && a.n === 'e') return num(1)
      if (n.n === 'exp' && isNum(a, 0)) return num(1)
      if (a.t === 'num' && ['sqrt', 'abs'].includes(n.n) && Number.isInteger(FUNCS[n.n](a.v))) return num(FUNCS[n.n](a.v))
      return { t: 'fn', n: n.n, a }
    }
    if (n.t === 'neg') {
      const a = simplify(n.a)
      if (a.t === 'num') return num(-a.v)
      if (a.t === 'neg') return a.a
      return { t: 'neg', a }
    }
    const a = simplify(n.a)
    const b = simplify(n.b)
    if (n.t === '+' || n.t === '-') {
      // Like terms together: x + x is 2x.
      const list = []
      for (const [c, r] of terms(node(n.t, a, b))) {
        const k = r ? text(r) : ''
        const at = list.find(l => l.k === k)
        if (at) at.c = tidy(at.c + c)
        else list.push({ k, c, r })
      }
      const kept = list.filter(l => l.c !== 0)
      kept.sort((p, q) => (p.r ? 0 : 1) - (q.r ? 0 : 1))
      if (!kept.length) return num(0)
      const plus = kept.findIndex(l => l.c > 0)
      if (kept[0].c < 0 && plus > 0) kept.unshift(...kept.splice(plus, 1)) // 1 - x², not -x² + 1
      let out = null
      for (const { c, r } of kept) {
        const mag = r ? product(Math.abs(c), factors(r)) : num(Math.abs(c))
        if (!out) out = c < 0 ? (mag.t === 'num' ? num(-mag.v) : { t: 'neg', a: mag }) : mag
        else out = node(c < 0 ? '-' : '+', out, mag)
      }
      return out
    }
    if (n.t === '*') {
      if (isNum(a, 0) || isNum(b, 0)) return num(0)
      if (b.t === '/') return simplify(node('/', node('*', a, b.a), b.b))
      if (a.t === '/') return simplify(node('/', node('*', a.a, b), a.b))
      const [c1, r1] = split(a)
      const [c2, r2] = split(b)
      const list = [...(r1 ? factors(r1) : []), ...(r2 ? factors(r2) : [])]
      return product(tidy(c1 * c2), list)
    }
    if (n.t === '/') {
      if (isNum(b, 1)) return a
      if (isNum(a, 0)) return num(0)
      if (a.t === 'num' && b.t === 'num' && b.v !== 0) return num(a.v / b.v)
      const fa = a.t === 'num' ? ratio(a.v) : null
      if (fa && fa[1] !== 1) return simplify(node('/', num(fa[0]), node('*', num(fa[1]), b)))
      if (text(a) === text(b)) return num(1)
      if (a.t === 'neg') return simplify({ t: 'neg', a: node('/', a.a, b) })
      if (b.t === 'neg') return simplify({ t: 'neg', a: node('/', a, b.a) })
      if (a.t === '/') return simplify(node('/', a.a, node('*', a.b, b)))
      if (b.t === '/') return simplify(node('/', node('*', a, b.b), b.a))
      return cancel(a, b) ?? node('/', a, b)
    }
    if (n.t === '^') {
      if (isNum(b, 0)) return num(1)
      if (isNum(b, 1)) return a
      if (isNum(a, 1)) return num(1)
      if (a.t === 'num' && b.t === 'num' && Number.isInteger(Math.pow(a.v, b.v))) return num(Math.pow(a.v, b.v))
      if (a.t === '^' && a.b.t === 'num' && b.t === 'num') return simplify(node('^', a.a, num(a.b.v * b.v)))
      if (a.t === 'fn' && a.n === 'sqrt' && isNum(b, 2)) return a.a
      if (b.t === 'num' && b.v < 0) return simplify(node('/', num(1), node('^', a, num(-b.v))))
      return node('^', a, b)
    }
    return n
  }

  // A number as a small fraction [p, q], when it is one.
  function ratio(v) {
    if (!Number.isFinite(v)) return null
    for (let q = 1; q <= 64; q++) {
      const p = Math.round(v * q)
      if (Math.abs(v * q - p) < 1e-9 * Math.max(1, Math.abs(v * q))) return [p, q]
    }
    return null
  }

  // What a fraction's top and bottom share goes: 2x/x^2 is 2/x.
  function gcd(a, b) {
    return b ? gcd(b, a % b) : a
  }
  function cancel(a, b) {
    const [ca, ra] = split(a)
    const [cb, rb] = split(b)
    const fa = ra ? factors(ra).map(baseOf) : []
    const fb = rb ? factors(rb).map(baseOf) : []
    let changed = false
    for (const A of fa) for (const B of fb) {
      if (A[1].t === 'num' && B[1].t === 'num' && A[1].v > 0 && B[1].v > 0 && text(A[0]) === text(B[0])) {
        const m = Math.min(A[1].v, B[1].v)
        A[1] = num(A[1].v - m)
        B[1] = num(B[1].v - m)
        changed = true
      }
    }
    let [ka, kb] = [ca, cb]
    if (Number.isInteger(ca) && Number.isInteger(cb) && cb !== 0) {
      const g = gcd(Math.abs(ca), Math.abs(cb)) * (cb < 0 ? -1 : 1)
      if (g !== 1) { ka = ca / g; kb = cb / g; changed = true }
    }
    if (!changed) return null
    const top = product(ka, fa.map(([base, e]) => simplify(node('^', base, e))))
    const bottom = product(kb, fb.map(([base, e]) => simplify(node('^', base, e))))
    return isNum(bottom, 1) ? top : simplify(node('/', top, bottom))
  }

  // ---- Derivatives ----
  const fn = (n, a) => ({ t: 'fn', n, a })
  const mul = (a, b) => node('*', a, b)
  const div = (a, b) => node('/', a, b)
  const pow = (a, b) => node('^', a, b)
  const neg = a => ({ t: 'neg', a })
  function outer(name, u) {
    switch (name) {
      case 'sin': return fn('cos', u)
      case 'cos': return neg(fn('sin', u))
      case 'tan': return pow(fn('sec', u), num(2))
      case 'sec': return mul(fn('sec', u), fn('tan', u))
      case 'csc': return neg(mul(fn('csc', u), fn('cot', u)))
      case 'cot': return neg(pow(fn('csc', u), num(2)))
      case 'asin': return div(num(1), fn('sqrt', node('-', num(1), pow(u, num(2)))))
      case 'acos': return neg(div(num(1), fn('sqrt', node('-', num(1), pow(u, num(2))))))
      case 'atan': return div(num(1), node('+', num(1), pow(u, num(2))))
      case 'sinh': return fn('cosh', u)
      case 'cosh': return fn('sinh', u)
      case 'tanh': return div(num(1), pow(fn('cosh', u), num(2)))
      case 'exp': return fn('exp', u)
      case 'ln': return div(num(1), u)
      case 'log': return div(num(1), mul(u, fn('ln', num(10))))
      case 'sqrt': return div(num(1), mul(num(2), fn('sqrt', u)))
      case 'cbrt': return div(num(1), mul(num(3), pow(fn('cbrt', u), num(2))))
      // u/|u|, not sign(u): it has no value at a corner (0/0), where |x| has no slope.
      case 'abs': return div(u, fn('abs', u))
      case 'sign': return num(0)
      default: throw new Error(`I don't know the derivative of ${name}`)
    }
  }
  function d(n, x) {
    switch (n.t) {
      case 'num': case 'const': return num(0)
      case 'var': return num(n.n === x ? 1 : 0)
      case 'neg': return neg(d(n.a, x))
      case '+': return node('+', d(n.a, x), d(n.b, x))
      case '-': return node('-', d(n.a, x), d(n.b, x))
      case '*': return node('+', mul(d(n.a, x), n.b), mul(n.a, d(n.b, x)))
      case '/': return div(node('-', mul(d(n.a, x), n.b), mul(n.a, d(n.b, x))), pow(n.b, num(2)))
      case '^': {
        const [u, w] = [n.a, n.b]
        if (!has(w, x)) return mul(mul(w, pow(u, node('-', w, num(1)))), d(u, x))
        const lnu = u.t === 'const' && u.n === 'e' ? null : fn('ln', u)
        if (!has(u, x)) return mul(lnu ? mul(n, lnu) : n, d(w, x))
        return mul(n, node('+', mul(d(w, x), lnu), div(mul(w, d(u, x)), u)))
      }
      case 'fn':
        if (n.n === 'fact') throw new Error('No derivative for factorial')
        return mul(outer(n.n, n.a), d(n.a, x))
    }
    throw new Error('Cannot take that derivative')
  }

  // ---- Writing back ----
  const PREC = { '+': 1, '-': 1, '*': 2, '/': 2, neg: 3, '^': 4 }
  const prec = n => PREC[n.t] ?? (n.t === 'num' ? (n.v < 0 ? 3 : Number.isInteger(n.v) || !ratio(n.v) ? 9 : 2) : 9)
  function numText(v) {
    if (!Number.isFinite(v)) return String(v)
    if (Number.isInteger(v)) return String(v)
    const r = ratio(v)
    if (r && r[1] <= 64) return `${r[0]}/${r[1]}`
    return String(Number(v.toPrecision(6)))
  }
  function show(n, html) {
    const wrap = (c, min) => (prec(c) < min ? `(${show(c, html)})` : show(c, html))
    const v = name => (html ? `<i>${name}</i>` : name)
    switch (n.t) {
      case 'num': return numText(n.v)
      case 'const': return n.n === 'pi' ? 'π' : n.n === 'tau' ? 'τ' : v('e')
      case 'var': return v(n.n)
      case 'neg': return `${html ? '−' : '-'}${wrap(n.a, 2)}`
      case 'fn': {
        if (n.n === 'fact') return `${wrap(n.a, 9)}!`
        if (n.n === 'abs') return `|${show(n.a, html)}|`
        if (n.n === 'sqrt') return prec(n.a) === 9 ? `√${show(n.a, html)}` : `√(${show(n.a, html)})`
        return `${n.n}(${show(n.a, html)})`
      }
      case '+': {
        const b = n.b
        if (b.t === 'neg') return `${show(n.a, html)} ${html ? '−' : '-'} ${wrap(b.a, 2)}`
        if (b.t === 'num' && b.v < 0) return `${show(n.a, html)} ${html ? '−' : '-'} ${numText(-b.v)}`
        return `${show(n.a, html)} + ${show(b, html)}`
      }
      case '-': return `${show(n.a, html)} ${html ? '−' : '-'} ${wrap(n.b, 2)}`
      case '*': {
        const left = wrap(n.a, 2)
        const right = wrap(n.b, 2)
        // 2x, 3sin(x), 2(x + 1); a dot between anything else.
        const tight = n.a.t === 'num' && n.a.v >= 0 && Number.isInteger(n.a.v) && !/^[\d.(]/.test(right.replace(/^<i>/, ''))
        return tight ? `${left}${right}` : `${left}·${right}`
      }
      case '/': return `${wrap(n.a, 2)}/${wrap(n.b, 3)}`
      case '^': {
        const base = n.a.t === 'fn' && n.a.n !== 'sqrt' ? show(n.a, html) : wrap(n.a, 9)
        if (html) return `${base}<sup>${show(n.b, html)}</sup>`
        return `${base}^${wrap(n.b, 9)}`
      }
    }
    return '?'
  }
  const text = n => show(n, false)

  // ---- The expression people use ----
  // A derivative whose formula has no value at a point (0/0 in
  // x^2/|x| at 0) takes the slope from both sides of the function it came
  // from when they agree, and stays undefined where they don't: a corner
  // (|x| at 0) or a jump.
  function bothSides(F, x) {
    const h = 1e-7
    const at = F(x)
    if (!Number.isFinite(at)) return NaN
    const left = (at - F(x - h)) / h
    const right = (F(x + h) - at) / h
    if (!Number.isFinite(left) || !Number.isFinite(right)) return NaN
    if (Math.abs(left - right) > 1e-3 * Math.max(1, Math.abs(left), Math.abs(right))) return NaN
    return Math.round(((left + right) / 2) * 1e6) / 1e6
  }
  function expression(tree, from = null) {
    const run = compile(tree)
    const vars = [...varsOf(tree)].sort((a, b) => (a === 'x' ? -1 : b === 'x' ? 1 : a.localeCompare(b)))
    const first = vars[0] ?? 'x'
    const f = value => {
      const v = run({ [first]: value })
      return Number.isNaN(v) && from ? bothSides(from, value) : v
    }
    return {
      tree,
      text: text(tree),
      html: show(tree, true),
      vars,
      f,
      eval: env => run(env ?? {}),
      derive(x = first) {
        return expression(simplify(d(tree, x)), x === first && vars.length <= 1 ? f : null)
      },
      simplify() {
        return expression(simplify(tree))
      },
    }
  }
  // "y = x^2" and "f(x) = x^2" mean x^2.
  function read(formula) {
    const s = String(formula ?? '').trim().replace(/^\s*(y|f\s*\(\s*[a-z]\s*\))\s*=\s*/i, '')
    return expression(parseTree(s))
  }

  // ---- Numbers ----
  function slope(f, x, h = 1e-5) {
    return (f(x + h) - f(x - h)) / (2 * h)
  }
  function integrate(f, a, b, tol = 1e-9) {
    if (a === b) return 0
    const simpson = (lo, hi, flo, fmid, fhi) => ((hi - lo) / 6) * (flo + 4 * fmid + fhi)
    function rec(lo, hi, flo, fmid, fhi, whole, eps, depth) {
      const mid = (lo + hi) / 2
      const lm = (lo + mid) / 2, rm = (mid + hi) / 2
      const flm = f(lm), frm = f(rm)
      const left = simpson(lo, mid, flo, flm, fmid), right = simpson(mid, hi, fmid, frm, fhi)
      if (depth <= 0 || Math.abs(left + right - whole) <= 15 * eps) return left + right + (left + right - whole) / 15
      return rec(lo, mid, flo, flm, fmid, left, eps / 2, depth - 1) + rec(mid, hi, fmid, frm, fhi, right, eps / 2, depth - 1)
    }
    const fa = f(a), fb = f(b), fm = f((a + b) / 2)
    return rec(a, b, fa, fm, fb, simpson(a, b, fa, fm, fb), tol, 40)
  }
  // Where f crosses zero on [a, b], found by sampling and bisection.
  function zeros(f, a, b, n = 400) {
    const out = []
    let x0 = a, y0 = f(a)
    for (let i = 1; i <= n; i++) {
      const x1 = a + ((b - a) * i) / n, y1 = f(x1)
      if (y0 === 0) out.push(x0)
      else if (Number.isFinite(y0) && Number.isFinite(y1) && y0 * y1 < 0) {
        let lo = x0, hi = x1, flo = y0
        for (let k = 0; k < 60; k++) {
          const mid = (lo + hi) / 2, fm = f(mid)
          if (flo * fm <= 0) hi = mid
          else { lo = mid; flo = fm }
        }
        const r = (lo + hi) / 2
        if (Math.abs(f(r)) < 1e-6 * (1 + Math.abs(y0) + Math.abs(y1))) out.push(r)
      }
      x0 = x1
      y0 = y1
    }
    return out
  }

  // ---- Plotting ----
  function nice(step) {
    const p = Math.pow(10, Math.floor(Math.log10(step)))
    const m = step / p
    return (m < 1.5 ? 1 : m < 3 ? 2 : m < 7 ? 5 : 10) * p
  }
  function fit(curves, x0, x1) {
    const ys = []
    for (const c of curves) for (let i = 0; i <= 200; i++) {
      const y = c.f(x0 + ((x1 - x0) * i) / 200)
      if (Number.isFinite(y)) ys.push(y)
    }
    if (!ys.length) return [-5, 5]
    ys.sort((a, b) => a - b)
    let lo = ys[Math.floor(ys.length * 0.02)], hi = ys[Math.ceil(ys.length * 0.98) - 1]
    if (hi - lo < 1e-9) { lo -= 1; hi += 1 }
    const pad = (hi - lo) * 0.12
    return [lo - pad, hi + pad]
  }
  const PALETTE = ['--accent', '--down', '--up', '#b0569c', '#c2741a']
  function plot(canvas, view = {}) {
    const css = getComputedStyle(canvas)
    const color = c => (c && c.startsWith('--') ? css.getPropertyValue(c).trim() || '#888' : c || '#888')
    const dpr = globalThis.devicePixelRatio || 1
    const w = canvas.clientWidth || canvas.width
    const h = canvas.clientHeight || canvas.height
    if (canvas.width !== Math.round(w * dpr) || canvas.height !== Math.round(h * dpr)) {
      canvas.width = Math.round(w * dpr)
      canvas.height = Math.round(h * dpr)
    }
    const ctx = canvas.getContext('2d')
    ctx.setTransform(dpr, 0, 0, dpr, 0, 0)
    ctx.clearRect(0, 0, w, h)
    const curves = (view.curves ?? []).filter(c => typeof c.f === 'function')
    const [x0, x1] = view.x ?? [-5, 5]
    const [y0, y1] = view.y ?? fit(curves, x0, x1)
    const sx = x => ((x - x0) / (x1 - x0)) * w
    const sy = y => h - ((y - y0) / (y1 - y0)) * h
    const px = (x, y) => [sx(x), sy(y)]
    const font = css.getPropertyValue('--font').trim() || 'system-ui, sans-serif'

    // Grid and axes, with numbers along them.
    if (view.grid !== false) {
      ctx.lineWidth = 1
      ctx.font = `11px ${font}`
      const gx = nice((x1 - x0) / Math.max(4, w / 90)), gy = nice((y1 - y0) / Math.max(3, h / 60))
      const ax = Math.min(Math.max(sy(0), 12), h - 4), ay = Math.min(Math.max(sx(0), 4), w - 30)
      ctx.strokeStyle = color('--line')
      ctx.globalAlpha = 0.8
      ctx.beginPath()
      for (let x = Math.ceil(x0 / gx) * gx; x <= x1; x += gx) { ctx.moveTo(Math.round(sx(x)) + 0.5, 0); ctx.lineTo(Math.round(sx(x)) + 0.5, h) }
      for (let y = Math.ceil(y0 / gy) * gy; y <= y1; y += gy) { ctx.moveTo(0, Math.round(sy(y)) + 0.5); ctx.lineTo(w, Math.round(sy(y)) + 0.5) }
      ctx.stroke()
      ctx.globalAlpha = 1
      ctx.strokeStyle = color('--ink-3')
      ctx.beginPath()
      if (y0 <= 0 && y1 >= 0) { ctx.moveTo(0, Math.round(sy(0)) + 0.5); ctx.lineTo(w, Math.round(sy(0)) + 0.5) }
      if (x0 <= 0 && x1 >= 0) { ctx.moveTo(Math.round(sx(0)) + 0.5, 0); ctx.lineTo(Math.round(sx(0)) + 0.5, h) }
      ctx.stroke()
      ctx.fillStyle = color('--ink-3')
      const label = v => numText(Math.abs(v) < 1e-10 ? 0 : Number(v.toPrecision(4)))
      ctx.textAlign = 'center'
      ctx.textBaseline = 'top'
      for (let x = Math.ceil(x0 / gx) * gx; x <= x1; x += gx) if (Math.abs(x) > gx / 2) ctx.fillText(label(x), sx(x), Math.min(ax + 3, h - 14))
      ctx.textAlign = 'left'
      ctx.textBaseline = 'middle'
      for (let y = Math.ceil(y0 / gy) * gy; y <= y1; y += gy) if (Math.abs(y) > gy / 2) ctx.fillText(label(y), ay + 4, sy(y))
    }

    // Shaded areas: under a curve between two x values.
    curves.forEach((c, i) => {
      if (!c.area) return
      const [a, b] = c.area
      ctx.fillStyle = color(c.color ?? PALETTE[i % PALETTE.length])
      ctx.globalAlpha = c.areaAlpha ?? 0.16
      ctx.beginPath()
      ctx.moveTo(sx(a), sy(0))
      const n = Math.max(2, Math.ceil(Math.abs(sx(b) - sx(a))))
      for (let k = 0; k <= n; k++) {
        const x = a + ((b - a) * k) / n, y = c.f(x)
        ctx.lineTo(sx(x), sy(Number.isFinite(y) ? Math.max(Math.min(y, y1 * 4), y0 * 4) : 0))
      }
      ctx.lineTo(sx(b), sy(0))
      ctx.closePath()
      ctx.fill()
      ctx.globalAlpha = 1
    })

    // Curves, broken where the function is undefined or jumps off the view.
    curves.forEach((c, i) => {
      ctx.strokeStyle = color(c.color ?? PALETTE[i % PALETTE.length])
      ctx.lineWidth = c.width ?? 2
      ctx.setLineDash(c.dash ?? [])
      ctx.globalAlpha = c.alpha ?? 1
      ctx.beginPath()
      const n = Math.ceil(w * 1.5)
      let drawing = false, prev = null
      for (let k = 0; k <= n; k++) {
        const x = x0 + ((x1 - x0) * k) / n, y = c.f(x)
        if (!Number.isFinite(y)) { drawing = false; prev = null; continue }
        const Y = sy(y)
        if (prev !== null && Math.abs(Y - prev) > h * 2) drawing = false
        if (drawing) ctx.lineTo(sx(x), Math.max(Math.min(Y, h * 3), -h * 2))
        else ctx.moveTo(sx(x), Math.max(Math.min(Y, h * 3), -h * 2))
        drawing = true
        prev = Y
      }
      ctx.stroke()
      ctx.setLineDash([])
      ctx.globalAlpha = 1
    })

    // Lines: a segment from [x, y] to [x, y], or a line through a point with a slope.
    for (const l of view.lines ?? []) {
      ctx.strokeStyle = color(l.color ?? '--ink-2')
      ctx.lineWidth = l.width ?? 1.5
      ctx.setLineDash(l.dash ?? [])
      ctx.beginPath()
      if (l.through) {
        const [px0, py0] = l.through, m = l.slope ?? 0
        if (Number.isFinite(m)) { ctx.moveTo(sx(x0), sy(py0 + m * (x0 - px0))); ctx.lineTo(sx(x1), sy(py0 + m * (x1 - px0))) }
        else { ctx.moveTo(sx(px0), 0); ctx.lineTo(sx(px0), h) }
      } else if (l.from && l.to) {
        ctx.moveTo(sx(l.from[0]), sy(l.from[1]))
        ctx.lineTo(sx(l.to[0]), sy(l.to[1]))
      }
      ctx.stroke()
      ctx.setLineDash([])
    }

    // Points, with a label beside each.
    for (const p of view.points ?? []) {
      if (!Number.isFinite(p.x) || !Number.isFinite(p.y)) continue
      const [X, Y] = px(p.x, p.y)
      ctx.fillStyle = color(p.color ?? '--ink')
      ctx.beginPath()
      ctx.arc(X, Y, p.r ?? 4.5, 0, Math.PI * 2)
      ctx.fill()
      if (p.label) {
        ctx.font = `12px ${font}`
        ctx.textAlign = X > w - 120 ? 'right' : 'left'
        ctx.textBaseline = 'bottom'
        ctx.fillText(p.label, X + (X > w - 120 ? -8 : 8), Y - 6)
      }
    }
    return { px, x: left => x0 + (left / w) * (x1 - x0), y: top => y0 + ((h - top) / h) * (y1 - y0), ctx, width: w, height: h, view: { x: [x0, x1], y: [y0, y1] } }
  }

  globalThis.KitMath = { parse: read, slope, integrate, zeros, format: numText }
  globalThis.KitPlot = plot
})()
