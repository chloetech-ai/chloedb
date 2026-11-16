#!/usr/bin/env python3
"""The HN canon: the books and papers Hacker News recommends most.

Builds the rows of `hn_canon` from the comments in a chloedb-hn cluster, in
steps that each write a file to --dir, so a later step can run again alone:

  pull    the comments that link Amazon, Goodreads or arXiv (ILIKE through
          the substring index), as JSON lines
  links   the works those links name (ASINs and ISBNs, Goodreads ids, arXiv
          ids), with who linked each and when
  titles  titles, authors and covers from Open Library (books, by ISBN) and
          the arXiv API (papers), a request a second or slower
  count   each work counted by everyone who linked it or named it
  load    the rows upserted into hn_canon

Books are merged across editions by their title and first author, and a
Goodreads id joins the book of its title. A comment names a work when it

  - links it,
  - uses its short name (SICP, DDIA, CLRS, TAOCP, GEB),
  - writes its title as a title, each word capitalised ("Getting Things
    Done", not "getting things done"), beside a word about books or papers
    (book, read, author, paper, arXiv...); a title of five words or more
    besides small ones needs no such word,
  - or writes the title, in any case, beside one of its authors' names,
    which is the only way to name a title of one or two such words
    ("Steve Jobs", "Deep Learning"),

and a paper counts only from the year of its arXiv id. The statement that
found a work's candidates, the rule and how many each part of it kept are
kept in the row, and the work's page shows them.

  CHLOEDB_TOKEN=... python3 canon/build.py all --strata http://127.0.0.1:7700 --dir /tmp/canon
"""
import argparse, collections, html, json, os, re, sys, time, unicodedata, urllib.error, urllib.request

UA = {'User-Agent': 'chloedb-hn canon (https://hn.chloetech.ai)'}
STOP = {'the', 'a', 'an', 'of', 'and', 'to', 'in', 'on', 'for', 'with', 'at', 'by', 'is', 'all', 'you', 'how', 'it', 'from', 'are', 'what', 'we', 'your', 'about', 'into'}
ALIAS = {'structure and interpretation of computer programs': ['SICP'], 'designing data intensive applications': ['DDIA'], 'introduction to algorithms': ['CLRS'],
         'the art of computer programming': ['TAOCP'], 'godel escher bach': ['GEB']}
CUE = re.compile(r'\b(book|books|novel|read|reading|author|paper|papers|arxiv)\b', re.I)
PULLS = {'amazon': '%amazon.%', 'goodreads': '%goodreads.com/%', 'arxiv': '%arxiv.org/%'}
AMZ = re.compile(r'amazon\.(?:com|co\.uk|ca|de|fr|in|co\.jp|com\.au|es|it)/(?:([A-Za-z0-9%\-]+)/)?(?:dp|gp/product|exec/obidos/ASIN|exec/obidos/tg/detail/-|o/ASIN|gp/aw/d)/([A-Z0-9]{10})', re.I)
GR = re.compile(r'goodreads\.com/book/show/(\d+)(?:[.\-]([A-Za-z0-9_\-%]+))?')
AX = re.compile(r'arxiv\.org/(?:abs|pdf)/(\d{4}\.\d{4,5}|[a-z\-]+/\d{7})')


class Strata:
    def __init__(self, url, token):
        self.url, self.token = url.rstrip('/'), token

    def post(self, path, body, timeout=600, tries=6):
        """A request, tried again on a dropped connection, a 429 or a 5xx."""
        h = {'content-type': 'application/json', **({'Authorization': 'Bearer ' + self.token} if self.token else {})}
        for i in range(tries):
            try:
                req = urllib.request.Request(self.url + path, data=json.dumps(body).encode(), headers=h)
                return json.load(urllib.request.urlopen(req, timeout=timeout))
            except urllib.error.HTTPError as e:
                if e.code != 429 and e.code < 500 or i == tries - 1:
                    raise
            except (OSError, ValueError):
                if i == tries - 1:
                    raise
            time.sleep(2 ** i)

    def scroll(self, sql, params=None, most=None):
        """Every row of a statement, a page of 5,000 at a time (a lost
        cursor starts the statement again)."""
        for attempt in range(3):
            try:
                return list(self._scroll(sql, params, most))
            except (OSError, ValueError, urllib.error.HTTPError):
                if attempt == 2:
                    raise
                time.sleep(5)

    def _scroll(self, sql, params, most):
        j = self.post('/v1/query', {'sql': sql, 'params': params or {}, 'page_size': 5000, 'keep_alive_ms': 300000, 'timeout_ms': 600000})
        cols = None
        n = 0
        while True:
            cols = j.get('columns') or cols
            for r in j.get('rows', []):
                yield dict(zip(cols, r)) if isinstance(r, list) else r
                n += 1
            c = j.get('cursor')
            if not c or (most and n >= most):
                return
            j = self.post('/v1/query', {'cursor': c, 'keep_alive_ms': 300000, 'timeout_ms': 600000}, tries=1)


def ascii_(t):
    return unicodedata.normalize('NFKD', t).encode('ascii', 'ignore').decode()


def norm(t):
    return ' '.join(re.sub(r'[^a-z0-9]+', ' ', ascii_(t).lower()).split())


def short(t):
    return re.split(r'\s*[:(\[]|\s+-\s+|\s+—\s+', t)[0].strip()


def plain(body):
    return ascii_(html.unescape(re.sub(r'<[^>]+>', ' ', body or '')))


def title_re(title, capital):
    """The title as a comment writes it: words apart by spaces or
    punctuation, and with `capital` each word (but small ones) capitalised."""
    parts = []
    for t in re.findall(r'[A-Za-z0-9]+', ascii_(title)):
        if not capital or t.lower() in STOP or t[0].isdigit():
            parts.append('(?i:' + re.escape(t) + ')')
        else:
            parts.append(re.escape(t[0].upper()) + '(?i:' + re.escape(t[1:]) + ')')
    return re.compile(r'(?<![A-Za-z0-9])' + r'[\W_]{1,3}'.join(parts) + r'(?![A-Za-z0-9])')


def name_re(names):
    """Authors' last names: in any case when long, capitalised when short
    (so "Ha" is not "ha")."""
    alts = [('(?i:' + re.escape(n) + ')') if len(n) >= 5 else re.escape(n[0].upper() + n[1:]) for n in map(ascii_, names) if len(n) >= 2]
    return re.compile(r'(?<![A-Za-z])(' + '|'.join(alts) + r')(?![A-Za-z])') if alts else None


def last_names(authors):
    out = []
    for a in authors[:3]:
        p = a.split()
        if p and p[-1].strip('.,').lower() not in [x.lower() for x in out]:
            out.append(p[-1].strip('.,'))
    return out


def arxiv_year(a):
    m = re.match(r'(\d{2})(\d{2})\.', a) or re.search(r'/(\d{2})(\d{2})', a)
    if not m:
        return None
    y = int(m.group(1))
    return 2000 + y if y < 90 else 1900 + y


def names_work(body, w):
    """Why a comment names a work, or None: 'alias', 'name' or 'title'."""
    b = plain(body)
    if w['alias'] and w['alias'].search(b):
        return 'alias'
    if w['names'] and w['any'].search(b) and w['names'].search(b):
        return 'name'
    if w['sig'] >= 5 and w['cap'].search(b):
        return 'title'
    if w['sig'] >= 3 and w['cap'].search(b) and CUE.search(b):
        return 'title'
    return None


def step_pull(s, d):
    for name, pat in PULLS.items():
        t0, n = time.time(), 0
        with open(os.path.join(d, name + '.jsonl'), 'w') as f:
            for r in s.scroll('SELECT id, author, year, story, body FROM hn_comments WHERE body ILIKE :p', {'p': pat}):
                f.write(json.dumps(r) + '\n')
                n += 1
        print(f'pull {name}: {n} comments in {time.time() - t0:.0f} s', flush=True)


def step_links(d):
    works = collections.defaultdict(lambda: {'people': set(), 'comments': [], 'years': collections.Counter(), 'slugs': collections.Counter()})
    def add(k, r, slug=None):
        w = works[k]
        if r['id'] in w['comments']:
            return
        w['people'].add(r['author']); w['comments'].append(r['id']); w['years'][r['year']] += 1
        if slug:
            w['slugs'][slug] += 1
    for name in PULLS:
        for line in open(os.path.join(d, name + '.jsonl')):
            r = json.loads(line); b = r['body'] or ''
            if name == 'amazon':
                for slug, asin in AMZ.findall(b): add('book:' + asin.upper(), r, slug)
            elif name == 'goodreads':
                for gid, slug in GR.findall(b): add('gr:' + gid, r, slug)
            else:
                for aid in AX.findall(b): add('paper:' + aid, r)
    out = {k: {'who': sorted(w['people']), 'people': len(w['people']), 'comments': w['comments'][:400], 'years': dict(w['years']),
               'slug': w['slugs'].most_common(1)[0][0] if w['slugs'] else ''} for k, w in works.items()}
    json.dump(out, open(os.path.join(d, 'works.json'), 'w'))
    print(f'links: {len(out)} works', flush=True)


def step_titles(d):
    works = json.load(open(os.path.join(d, 'works.json')))
    get = lambda u: urllib.request.urlopen(urllib.request.Request(u, headers=UA), timeout=60).read()
    out = {}
    isbns = [k[5:] for k, w in works.items() if k.startswith('book:') and w['people'] >= 3 and re.fullmatch(r'\d{9}[\dX]', k[5:])]
    for i in range(0, len(isbns), 40):
        try:
            j = json.loads(get('https://openlibrary.org/api/books?format=json&jscmd=data&bibkeys=' + ','.join('ISBN:' + x for x in isbns[i:i + 40])))
        except Exception as e:
            print('Open Library:', e); time.sleep(5); continue
        for k, v in j.items():
            out['book:' + k[5:]] = {'title': v.get('title', ''), 'authors': [a['name'] for a in v.get('authors', [])][:3], 'year': v.get('publish_date', ''), 'cover': (v.get('cover') or {}).get('medium', '')}
        time.sleep(1)
    papers = [k[6:] for k, w in sorted(works.items(), key=lambda kw: -kw[1]['people']) if k.startswith('paper:') and w['people'] >= 3]
    for i in range(0, len(papers), 100):
        try:
            x = get('https://export.arxiv.org/api/query?max_results=100&id_list=' + ','.join(papers[i:i + 100])).decode()
        except Exception as e:
            print('arXiv:', e); time.sleep(5); continue
        for ent in x.split('<entry>')[1:]:
            m = re.search(r'<id>https?://arxiv\.org/abs/([^<]+?)(v\d+)?</id>', ent)
            t = re.search(r'<title>([^<]*)</title>', ent, re.S)
            pub = re.search(r'<published>(\d{4})', ent)
            if m and t:
                out['paper:' + m.group(1)] = {'title': ' '.join(html.unescape(t.group(1)).split()), 'authors': re.findall(r'<name>([^<]*)</name>', ent)[:3], 'year': pub.group(1) if pub else ''}
        time.sleep(3.5)  # arXiv asks for a request every three seconds
    json.dump(out, open(os.path.join(d, 'titles.json'), 'w'))
    print(f'titles: {len(out)} works titled', flush=True)


def groups_of(works, titles):
    groups = {}
    for k, w in works.items():
        t = titles.get(k)
        if not t or not t['title']:
            continue
        if k.startswith('book:'):
            st = short(t['title'])
            if len([x for x in norm(st).split() if x not in STOP]) < 2:  # "JavaScript: The Good Parts"
                st = re.split(r'\s*[(\[]', t['title'])[0].strip()
            last = t['authors'][0].split()[-1].lower() if t['authors'] else ''
            g = groups.setdefault('book:' + norm(st) + '|' + last, {'kind': 'book', 'title': st, 'authors': t['authors'], 'year': t.get('year', ''), 'ids': [], 'who': set(), 'links': set(), 'cover': ''})
            g['cover'] = g['cover'] or t.get('cover', '')
        elif k.startswith('paper:'):
            g = groups.setdefault(k, {'kind': 'paper', 'title': t['title'], 'authors': t['authors'], 'year': t.get('year', ''), 'ids': [], 'who': set(), 'links': set(), 'arxiv': k[6:]})
        else:
            continue
        g['ids'].append(k); g['who'].update(w['who']); g['links'].update(w['comments'])
    bytitle = collections.defaultdict(list)
    for key, g in groups.items():
        if g['kind'] == 'book':
            bytitle[norm(g['title'])].append(key)
    for k, w in works.items():
        if k.startswith('gr:') and w['slug']:
            t = norm(re.sub(r'^\d+[._-]?', '', w['slug']).replace('_', ' ').replace('-', ' '))
            if len(bytitle.get(t, [])) == 1:
                g = groups[bytitle[t][0]]; g['ids'].append(k); g['who'].update(w['who']); g['links'].update(w['comments'])
    return groups


def step_count(s, d, books, papers):
    groups = groups_of(json.load(open(os.path.join(d, 'works.json'))), json.load(open(os.path.join(d, 'titles.json'))))
    todo = sorted((g for g in groups.values() if g['kind'] == 'book'), key=lambda g: -len(g['who']))[:books] \
        + sorted((g for g in groups.values() if g['kind'] == 'paper'), key=lambda g: -len(g['who']))[:papers]
    # Works counted by an earlier run that stopped are kept, a line each.
    part = os.path.join(d, 'canon.partial.jsonl')
    done = {}
    if os.path.exists(part):
        for line in open(part):
            r = json.loads(line); done[r['_key']] = r
    rows, t0 = [], time.time()
    pf = open(part, 'a')
    for i, g in enumerate(todo):
        gk = g['ids'][0]
        if gk in done:
            rows.append({k: v for k, v in done[gk].items() if k != '_key'})
            continue
        st = norm(g['title'])
        sig = len([x for x in st.split() if x not in STOP])
        names = last_names(g['authors'])
        aliases = ALIAS.get(st, [])
        links = sorted(g['links'])[:500]
        conds, params = [], {'p': f'"{st}"'}
        if sig >= 2:
            conds.append('MATCH(body, :p)')
        elif names:
            conds.append('(MATCH(body, :p) AND MATCH(body, :a))'); params['a'] = ' OR '.join(n.lower() for n in names)
        for j, al in enumerate(aliases):
            conds.append(f'MATCH(body, :al{j})'); params[f'al{j}'] = al.lower()
        if links:
            conds.append(f"id IN ({', '.join(map(str, links))})")
        if not conds:
            continue
        since = arxiv_year(g['arxiv']) if g['kind'] == 'paper' else None
        where = '(' + ' OR '.join(conds) + ')' + (f' AND year >= {since}' if since else '')
        w = {'sig': sig, 'cap': title_re(g['title'], True), 'any': title_re(g['title'], False), 'names': name_re(names),
             'alias': re.compile(r'\b(' + '|'.join(aliases) + r')\b') if aliases else None}
        lset, kept, cand = set(links), [], 0
        for r in s.scroll(f'SELECT id, author, year, pop, body FROM hn_comments WHERE {where}', params, most=60000):
            cand += 1
            why = 'link' if r['id'] in lset else names_work(r.get('body'), w)
            if why:
                kept.append((r, why))
        years = collections.defaultdict(lambda: [0, set()])
        for r, _ in kept:
            years[r['year']][0] += 1; years[r['year']][1].add(r['author'])
        ys = {str(y): {'n': v[0], 'people': len(v[1])} for y, v in sorted(years.items()) if y}
        top = [r['id'] for r, _ in sorted(kept, key=lambda x: -(x[0].get('pop') or 0))[:8]]
        rule = ('a comment counts when it links the work' + (', uses its short name (' + ', '.join(aliases) + ')' if aliases else '')
                + ((', writes the title with each word capitalised' + (' beside a word about books or papers' if sig < 5 else '')) if sig >= 3 else '')
                + (', or writes the title beside ' + ' or '.join(names) if names else '') + (f'; and is from {since} or later, when the paper appeared' if since else ''))
        kid = g['ids'][0] if g['kind'] == 'paper' else 'book:' + re.sub(r'[^a-z0-9]+', '-', st)[:60] + ('-' + names[0].lower() if names else '')
        url = f"https://arxiv.org/abs/{g['arxiv']}" if g['kind'] == 'paper' else (f"https://openlibrary.org/isbn/{g['ids'][0][5:]}" if g['ids'][0].startswith('book:') else '')
        rows.append({'id': kid, 'kind': g['kind'], 'title': g['title'], 'creator': ', '.join(g['authors'][:3]), 'published': str(g['year'])[-4:], 'url': url, 'image': g.get('cover', ''),
                     'people': len({r['author'] for r, _ in kept}), 'mentions': len(kept), 'linkers': len(g['who']),
                     'first': min(map(int, ys)) if ys else None, 'last': max(map(int, ys)) if ys else None, 'years': json.dumps(ys),
                     'comments': json.dumps(top), 'phrase': where, 'params': json.dumps(params), 'ids': json.dumps(g['ids'][:20]),
                     'rule': rule, 'candidates': cand, 'why': json.dumps(dict(collections.Counter(w for _, w in kept)))})
        pf.write(json.dumps({**rows[-1], '_key': gk}) + '\n'); pf.flush()
        if i % 50 == 0:
            print(f'count {i}/{len(todo)} {time.time() - t0:.0f} s: {g["title"][:50]}, {rows[-1]["people"]} people', flush=True)
    pf.close()
    json.dump(rows, open(os.path.join(d, 'canon.json'), 'w'))
    os.remove(part)
    print(f'count: {len(rows)} works in {time.time() - t0:.0f} s', flush=True)


def step_load(s, d):
    rows = json.load(open(os.path.join(d, 'canon.json')))
    for i in range(0, len(rows), 200):
        s.post('/v1/write', {'ops': [{'op': 'upsert', 'table': 'hn_canon', 'doc': {k: v for k, v in r.items() if v is not None}} for r in rows[i:i + 200]]})
    print(f'load: {len(rows)} rows into hn_canon', flush=True)


def main():
    ap = argparse.ArgumentParser(description=__doc__.split('\n\n')[0])
    ap.add_argument('step', choices=['pull', 'links', 'titles', 'count', 'load', 'all'])
    ap.add_argument('--strata', default='http://127.0.0.1:7700', help="the cluster's coordinator")
    ap.add_argument('--dir', default='canon-work', help='where the steps keep their files')
    ap.add_argument('--books', type=int, default=450)
    ap.add_argument('--papers', type=int, default=300)
    a = ap.parse_args()
    os.makedirs(a.dir, exist_ok=True)
    s = Strata(a.strata, os.environ.get('CHLOEDB_TOKEN', ''))
    steps = ['pull', 'links', 'titles', 'count', 'load'] if a.step == 'all' else [a.step]
    for st in steps:
        if st == 'pull': step_pull(s, a.dir)
        elif st == 'links': step_links(a.dir)
        elif st == 'titles': step_titles(a.dir)
        elif st == 'count': step_count(s, a.dir, a.books, a.papers)
        elif st == 'load': step_load(s, a.dir)


if __name__ == '__main__':
    main()
