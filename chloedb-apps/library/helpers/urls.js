// title: URLs
// description: Links as plain text: read a URL into its parts, take its domain, add or remove query parameters, tidy it for comparing or storing, check it, join a base and a path, and shorten it for display (bookmarks, link lists, sources, API addresses).
// api: parse(url) -> {protocol ('https'), host (lower case, no port), port ('' when none), domain (host without www.), path ('/' when none), query: {name: value, or [values] when repeated}, hash (without '#')} or null;
//   a url without a scheme ('example.com/a') is read as https; text with spaces or no host gives null;
//   domain(url) -> 'example.com' (the host without a leading www.) or null;
//   withParams(url, params) -> the url with each param added, or replaced where it was (in place, other parameters kept as written);
//   a null or undefined value removes it, an array gives the name once per value; works on relative urls too; null when url is not text;
//   params(url) -> the query as an object, decoded ('+' is a space; repeated names give an array; {} when none);
//   normalize(url) -> the url with https:// added when it has no scheme, scheme and host lower-cased, the default port (:443, :80) dropped,
//   trailing slashes of the path dropped ('https://example.com/' -> 'https://example.com') and utm_ parameters removed; null when it is not a url;
//   isUrl(text) -> true for an http(s) address with a real host (a domain with a top-level part, localhost, an IPv4 or [IPv6] address);
//   join(base, path) -> base and path with one slash between, '.' and '..' resolved; a path starting with '/' starts at the base's host,
//   '?' or '#' replaces the base's query or hash, a full url or '//host' path stands alone;
//   shorten(url, n = 40) -> the url for display without scheme, www. and trailing slash, at most n characters, cut in the middle with '…' keeping the last part of the path
function helper() {
  const SCHEME = /^[a-z][a-z0-9+.\-]*:\/\//i;
  const RE = /^([a-z][a-z0-9+.\-]*):\/\/([^\/?#]*)([^?#]*)(?:\?([^#]*))?(?:#(.*))?$/i;
  const str = (v) => (typeof v === 'string' ? v.trim() : '');
  const decode = (s) => { try { return decodeURIComponent(String(s).replace(/\+/g, ' ')); } catch (e) { return String(s); } };
  const encode = (v) => encodeURIComponent(String(v));

  // text to an absolute url, or null
  function absolute(s) {
    if (!s || /\s/.test(s)) return null;
    if (SCHEME.test(s)) return s;
    if (s.startsWith('//')) return 'https:' + s;
    const lead = /^[^\/?#]+/.exec(s);
    if (lead && (/^[a-z0-9\-\u00a1-\uffff]+(\.[a-z0-9\-\u00a1-\uffff]+)+(:\d+)?$/i.test(lead[0]) || /^localhost(:\d+)?$/i.test(lead[0]))) return 'https://' + s;
    return null;
  }
  function authority(auth) {
    const at = auth.lastIndexOf('@');
    const user = at >= 0 ? auth.slice(0, at) : '';
    const hostPort = auth.slice(at + 1);
    let host;
    let port;
    const v6 = /^(\[[0-9a-f:.]+\])(?::(\d*))?$/i.exec(hostPort);
    if (v6) { host = v6[1]; port = v6[2]; } else {
      const m = /^([^:\[\]\s]*)(?::(\d*))?$/.exec(hostPort);
      if (!m) return null;
      host = m[1]; port = m[2];
    }
    if (!host) return null;
    return { user, host: host.toLowerCase(), port: port || '' };
  }
  function queryObject(qs) {
    const out = {};
    for (const part of String(qs || '').split('&')) {
      if (!part) continue;
      const i = part.indexOf('=');
      const k = decode(i >= 0 ? part.slice(0, i) : part);
      if (!k || k === '__proto__') continue;
      const v = i >= 0 ? decode(part.slice(i + 1)) : '';
      if (Object.prototype.hasOwnProperty.call(out, k)) out[k] = [].concat(out[k], v);
      else out[k] = v;
    }
    return out;
  }

  function parse(url) {
    const s = absolute(str(url));
    const m = s ? RE.exec(s) : null;
    if (!m) return null;
    const auth = authority(m[2]);
    if (!auth) return null;
    return {
      protocol: m[1].toLowerCase(),
      host: auth.host,
      port: auth.port,
      domain: auth.host.replace(/^www\./, ''),
      path: m[3] || '/',
      query: queryObject(m[4]),
      hash: m[5] || '',
    };
  }
  function domain(url) { const p = parse(url); return p ? p.domain : null; }
  function params(url) {
    if (typeof url !== 'string') return {};
    const hashAt = url.indexOf('#');
    const rest = hashAt >= 0 ? url.slice(0, hashAt) : url;
    const q = rest.indexOf('?');
    return q < 0 ? {} : queryObject(rest.slice(q + 1));
  }
  function withParams(url, changes) {
    if (typeof url !== 'string') return null;
    const hashAt = url.indexOf('#');
    const hash = hashAt >= 0 ? url.slice(hashAt) : '';
    const rest = hashAt >= 0 ? url.slice(0, hashAt) : url;
    const q = rest.indexOf('?');
    const base = q >= 0 ? rest.slice(0, q) : rest;
    let pairs = q < 0 ? [] : rest.slice(q + 1).split('&').filter(Boolean).map((raw) => {
      const i = raw.indexOf('=');
      return { key: decode(i >= 0 ? raw.slice(0, i) : raw), raw };
    });
    const given = changes && typeof changes === 'object' ? changes : {};
    for (const key of Object.keys(given)) {
      const v = given[key];
      const values = v === null || v === undefined ? [] : (Array.isArray(v) ? v : [v]).filter((x) => x !== null && x !== undefined);
      const fresh = values.map((x) => ({ key, raw: encode(key) + '=' + encode(x) }));
      const at = pairs.findIndex((p) => p.key === key);
      pairs = pairs.filter((p) => p.key !== key);
      if (fresh.length) { if (at >= 0) pairs.splice(at, 0, ...fresh); else pairs.push(...fresh); }
    }
    return base + (pairs.length ? '?' + pairs.map((p) => p.raw).join('&') : '') + hash;
  }
  function normalize(url) {
    const s = absolute(str(url));
    const m = s ? RE.exec(s) : null;
    if (!m) return null;
    const protocol = m[1].toLowerCase();
    const auth = authority(m[2]);
    if (!auth) return null;
    let port = auth.port;
    if ((protocol === 'https' && port === '443') || (protocol === 'http' && port === '80')) port = '';
    const path = (m[3] || '').replace(/\/+$/, '');
    const kept = String(m[4] || '').split('&').filter((p) => p && !/^utm_/i.test(decode(p.split('=')[0])));
    return protocol + '://' + (auth.user ? auth.user + '@' : '') + auth.host + (port ? ':' + port : '') + path +
      (kept.length ? '?' + kept.join('&') : '') + (m[5] ? '#' + m[5] : '');
  }
  function isUrl(text) {
    const m = /^(https?):\/\/([^\/?#\s]+)([\/?#]\S*)?$/i.exec(str(text));
    if (!m) return false;
    const auth = authority(m[2]);
    if (!auth) return false;
    if (auth.port && Number(auth.port) > 65535) return false;
    const hostName = auth.host;
    if (hostName === 'localhost' || hostName.startsWith('[')) return true;
    if (/^\d+(\.\d+){3}$/.test(hostName)) return hostName.split('.').every((n) => Number(n) <= 255);
    return /^([a-z0-9\u00a1-\uffff](?:[a-z0-9\-\u00a1-\uffff]{0,61}[a-z0-9\u00a1-\uffff])?\.)+(?:[a-z\u00a1-\uffff]{2,63}|xn--[a-z0-9\-]{1,59})$/.test(hostName);
  }
  function resolve(path) {
    const abs = path.startsWith('/');
    const parts = path.split('/');
    const out = [];
    for (const seg of parts) {
      if (seg === '..') { if (out.length) out.pop(); } else if (seg !== '.' && seg !== '') out.push(seg);
    }
    const last = parts[parts.length - 1];
    const trail = (last === '' || last === '.' || last === '..') && out.length > 0;
    return (abs ? '/' : '') + out.join('/') + (trail ? '/' : '');
  }
  function join(base, path) {
    const b = typeof base === 'string' ? base.trim() : '';
    const p = path === null || path === undefined ? '' : String(path).trim();
    if (!b) return p;
    if (SCHEME.test(p)) return p;
    const m = /^([a-z][a-z0-9+.\-]*:)?(\/\/[^\/?#]*)?([^?#]*)(\?[^#]*)?(#.*)?$/i.exec(b);
    if (!m) return null;
    const scheme = m[1] || '';
    const auth = m[2] || '';
    const bpath = m[3] || '';
    const bquery = m[4] || '';
    if (!p) return b;
    if (p.startsWith('//')) return (scheme || 'https:') + p;
    if (p[0] === '#') return scheme + auth + bpath + bquery + p;
    if (p[0] === '?') return scheme + auth + bpath + p;
    const i = p.search(/[?#]/);
    const pp = i >= 0 ? p.slice(0, i) : p;
    const suffix = i >= 0 ? p.slice(i) : '';
    const full = pp[0] === '/' ? pp : bpath.replace(/\/+$/, '') + '/' + pp;
    return scheme + auth + resolve(full) + suffix;
  }
  function shorten(url, n = 40) {
    let t = str(url);
    if (!t) return '';
    n = Math.max(5, Math.floor(Number(n)) || 40);
    t = t.replace(SCHEME, '').replace(/^www\./i, '').replace(/\/+$/, '');
    if (t.length <= n) return t;
    const hostEnd = t.search(/[\/?#]/);
    const hostLen = hostEnd < 0 ? t.length : hostEnd;
    const slash = t.lastIndexOf('/');
    let tail = slash > hostLen ? t.slice(slash) : '';
    if (tail.length > Math.floor(n / 2)) tail = '';
    return t.slice(0, n - 1 - tail.length) + '…' + tail;
  }
  return { parse, domain, withParams, params, normalize, isUrl, join, shorten };
}
