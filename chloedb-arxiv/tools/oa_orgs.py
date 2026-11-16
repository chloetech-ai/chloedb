"""Organisations of arXiv papers from the public OpenAlex snapshot (S3,
anonymous): per row group, `indexed_in` picks the arXiv works, then only the
columns of their landing pages and their authors' institutions are read.
Writes one JSON line per paper: {"id", "orgs": [[id, name, country, type], ...], "n": authors with one}.
Resumable: a file already written is skipped."""
import json, os, re, sys, time, urllib.request, concurrent.futures as cf
import pyarrow.fs as pfs, pyarrow.parquet as pq

OUT = sys.argv[1]
WORKERS = int(sys.argv[2]) if len(sys.argv) > 2 else 12
fs = pfs.S3FileSystem(anonymous=True, region="us-east-1", proxy_options=os.environ.get("HTTPS_PROXY"))
RX = re.compile(r"arxiv\.org/(?:abs|pdf)/([0-9]{4}\.[0-9]{4,5}|[a-zA-Z\-]+(?:\.[A-Z]{2})?/[0-9]{7})")
COLS = ["doi", "locations.list.element.landing_page_url",
        "authorships.list.element.institutions.list.element.id", "authorships.list.element.institutions.list.element.display_name",
        "authorships.list.element.institutions.list.element.country_code", "authorships.list.element.institutions.list.element.type"]

def keys():
    out, tok = [], None
    while True:
        u = "https://openalex.s3.amazonaws.com/?list-type=2&prefix=data/parquet/works/" + ("&continuation-token=" + urllib.parse.quote(tok) if tok else "")
        t = urllib.request.urlopen(u, timeout=60).read().decode()
        out += [(k, int(s)) for k, s in re.findall(r"<Key>([^<]*\.parquet)</Key>.*?<Size>(\d+)</Size>", t)]
        m = re.search(r"<NextContinuationToken>([^<]*)</NextContinuationToken>", t)
        if not m:
            return out
        tok = m.group(1)

def one(key):
    name = key.replace("data/parquet/works/", "").replace("/", "_").replace("updated_date=", "")
    dst = os.path.join(OUT, name + ".jsonl")
    if os.path.exists(dst):
        return 0, 0
    for attempt in range(5):
        try:
            pf = pq.ParquetFile(fs.open_input_file("openalex/" + key))
            lines, seen = [], 0
            for g in range(pf.metadata.num_row_groups):
                ii = pf.read_row_group(g, columns=["indexed_in"]).column(0).to_pylist()
                idx = [i for i, x in enumerate(ii) if x and "arxiv" in x]
                seen += len(ii)
                if not idx:
                    continue
                t = pf.read_row_group(g, columns=COLS).take(idx)
                for r in t.to_pylist():
                    aid = None
                    for l in r["locations"] or []:
                        m = RX.search(l.get("landing_page_url") or "")
                        if m:
                            aid = m.group(1)
                            break
                    if not aid and r["doi"] and "10.48550/arxiv." in r["doi"].lower():
                        aid = r["doi"].lower().split("arxiv.", 1)[1]
                    if not aid:
                        continue
                    orgs, n = {}, 0
                    for a in r["authorships"] or []:
                        got = False
                        for i in a["institutions"] or []:
                            if i["id"]:
                                orgs[i["id"].rsplit("/", 1)[-1]] = [i["display_name"], i["country_code"], i["type"]]
                                got = True
                        n += got
                    lines.append(json.dumps({"id": aid, "orgs": [[k] + v for k, v in orgs.items()], "n": n}, ensure_ascii=False))
            tmp = dst + ".tmp"
            with open(tmp, "w") as f:
                f.write("\n".join(lines) + ("\n" if lines else ""))
            os.rename(tmp, dst)
            return len(lines), seen
        except Exception as e:
            err = e
            time.sleep(5 * (attempt + 1))
    print("FAILED", key, err, flush=True)
    return 0, 0

ks = keys()
print(len(ks), "files", round(sum(s for _, s in ks) / 1e9, 1), "GB", flush=True)
t0, done, papers, rows = time.time(), 0, 0, 0
with cf.ThreadPoolExecutor(WORKERS) as ex:
    for p, s in ex.map(one, [k for k, _ in sorted(ks, key=lambda x: -x[1])]):
        done += 1; papers += p; rows += s
        if done % 25 == 0:
            print(f"{done}/{len(ks)} files, {papers} arXiv papers, {rows} works, {time.time()-t0:.0f} s", flush=True)
print("done", done, papers, rows, round(time.time() - t0), flush=True)
