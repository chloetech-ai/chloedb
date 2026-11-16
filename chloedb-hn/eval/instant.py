#!/usr/bin/env python3
"""Search as it is typed, a keystroke at a time, on Hacker News on ChloeDB
and on hn.algolia.com, from the same machine over kept-alive connections.

Each word list is typed one character at a time; every prefix is asked of
both: the app's instant search (`mode=instant`: the titles with the words so
far, the last one as the start of a word) and Algolia's search of stories,
20 a page, as its page asks while typing. For each it keeps the server's own
time (the app's took_ms, Algolia's processingTimeMS) and the round trip.

    python3 eval/instant.py --app https://hn.chloetech.ai --out eval/results/instant-2026-11-16.json
"""
import argparse, http.client, json, statistics, time, urllib.parse

WORDS = ["postgres vs my", "rust async", "kubernetes", "sqlite in prod", "show hn gam", "llama cpp"]


def conn(base):
    u = urllib.parse.urlparse(base)
    c = (http.client.HTTPSConnection if u.scheme == "https" else http.client.HTTPConnection)(u.hostname, u.port, timeout=30)
    return c, u.path.rstrip("/")


def get(c, path):
    t = time.time()
    c.request("GET", path, headers={"user-agent": "chloedb-hn eval/instant.py"})
    r = c.getresponse()
    b = r.read()
    return json.loads(b), (time.time() - t) * 1000


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--app", default="https://hn.chloetech.ai")
    ap.add_argument("--out")
    a = ap.parse_args()
    ours, pre = conn(a.app)
    alg, _ = conn("https://hn.algolia.com")
    # Both connections open before the first keystroke.
    get(ours, f"{pre}/v1/search?q=a&mode=instant")
    get(alg, "/api/v1/search?query=a&tags=story&hitsPerPage=20")
    rows = []
    for w in WORDS:
        for i in range(1, len(w) + 1):
            q = w[:i]
            j1, m1 = get(ours, f"{pre}/v1/search?" + urllib.parse.urlencode({"q": q, "mode": "instant"}))
            j2, m2 = get(alg, "/api/v1/search?" + urllib.parse.urlencode({"query": q, "tags": "story", "hitsPerPage": 20}))
            rows.append({"q": q, "ours_server": j1.get("took_ms"), "ours_rt": round(m1, 1), "ours_top": [s["title"] for s in j1.get("stories", [])[:3]],
                         "algolia_server": j2.get("processingTimeMS"), "algolia_rt": round(m2, 1), "algolia_top": [h.get("title") for h in j2.get("hits", [])[:3]]})
    def stat(k):
        xs = sorted(r[k] for r in rows if r[k] is not None)
        return {"p50": statistics.median(xs), "p90": xs[int(0.9 * (len(xs) - 1))], "max": xs[-1]}
    out = {"at": time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime()), "app": a.app, "keystrokes": len(rows),
           "ours_server": stat("ours_server"), "ours_rt": stat("ours_rt"), "algolia_server": stat("algolia_server"), "algolia_rt": stat("algolia_rt"), "rows": rows}
    print(json.dumps({k: v for k, v in out.items() if k != "rows"}, indent=1))
    if a.out:
        json.dump(out, open(a.out, "w"), indent=1)


if __name__ == "__main__":
    main()
