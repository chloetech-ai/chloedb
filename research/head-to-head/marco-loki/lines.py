"""An hour of made-up log lines shaped like Marco's (nothing a persona brought
is copied): edge nginx access lines, JSON lines from a dozen services with
trace ids, and network syslog, 1,000 lines a second for an hour, the unit
Marco sizes disk in. Seeded, so every run writes the same lines, and stamped
to end at the minute the run starts.

The last fifteen minutes hold what an on-call engineer greps for: one client
address (203.0.113.77) hammering the login page and trying ssh, a burst of
503s on two edge hosts, and timeouts in checkout. `truth()` answers each of
the page's questions from the lines themselves.
"""

import os
import random

RATE = 1000          # lines a second
SECONDS = int(os.environ.get("H2H_SECONDS", 3600))  # an hour
NEEDLE_IP = "203.0.113.77"
NEAR_IP = "203.0.113.7"   # contains the needle's first digits, so a substring match must not take it
EDGE = [f"edge-{i:02d}" for i in range(1, 13)]
APPS = ["checkout", "cart", "payments", "inventory", "search", "accounts", "orders", "shipping",
        "pricing", "reviews", "notify", "gateway"]
APP_HOSTS = {s: [f"app-{(i * 2 + k) % 16 + 1:02d}" for k in range(2)] for i, s in enumerate(APPS)}
NET = [f"net-sw-{i:02d}" for i in range(1, 13)]
PATHS = ["/", "/api/v1/items/{n}", "/api/v1/cart", "/api/v1/search?q={w}", "/static/app.{n}.js", "/login",
         "/api/v1/orders/{n}", "/healthz", "/img/{n}.webp", "/api/v1/prices/{n}"]
WORDS = "shoes lamp chair desk phone case cable kettle mug jacket boots tent stove".split()
UAS = ["Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/131.0 Safari/537.36",
       "Mozilla/5.0 (iPhone; CPU iPhone OS 18_1 like Mac OS X) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/18.1 Mobile/15E148 Safari/604.1",
       "Mozilla/5.0 (Macintosh; Intel Mac OS X 14_6) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/18.0 Safari/605.1.15",
       "curl/8.5.0", "Go-http-client/2.0", "kube-probe/1.31"]
APP_MSGS = {
    "INFO": ["request served", "cache hit", "order placed", "payment authorised", "item reserved", "user signed in",
             "price computed", "email queued", "search answered", "stock updated"],
    "WARN": ["slow query against the replica", "retrying the call to inventory", "cache miss storm", "rate limit near"],
    "ERROR": ["upstream Timeout after 3000 ms calling inventory", "payment declined by the processor",
              "database connection refused", "context deadline exceeded calling pricing"],
}
# how each service spells its levels: SDKs and loggers do not agree
SPELL = {"INFO": ["INFO", "info", "Information"], "WARN": ["WARN", "warning", "Warning"], "ERROR": ["ERROR", "error", "Err"]}
SYSLOG = [
    ("info", 9, "kernel", "%LINK-5-CHANGED: Interface GigabitEthernet0/{p}, changed state to up"),
    ("notice", 10, "sshd", "Accepted publickey for deploy from 10.20.{a}.{b} port {port} ssh2"),
    ("warning", 13, "kernel", "%STORM_CONTROL-3-FILTERED: A packet storm was detected on Gi1/0/{p}"),
    ("info", 9, "cron", "(root) CMD (/usr/local/bin/backup.sh)"),
    ("err", 17, "kernel", "%LINK-3-UPDOWN: Interface GigabitEthernet0/{p}, changed state to down"),
    ("notice", 10, "sshd", "Failed password for root from {ip} port {port} ssh2"),
]
MONTHS = "Jan Feb Mar Apr May Jun Jul Aug Sep Oct Nov Dec".split()


def _ip(rng):
    # a few busy clients and a long tail, as an edge sees them
    if rng.random() < 0.3:
        return f"10.1.{rng.randrange(4)}.{rng.randrange(1, 40)}"
    return f"{rng.choice((10, 172, 192))}.{rng.randrange(256)}.{rng.randrange(256)}.{rng.randrange(1, 255)}"


def lines(end_ns, seed=5):
    """Every line, oldest first: dicts with ts (ns), job, service, host, level (one spelling, for the truth and
    Loki's label), sev_text and sev_num (as the source sent them; sev_num None when the source sent none), body,
    trace_id (app lines) and attributes (syslog's hostname and appname)."""
    rng = random.Random(seed)
    start_ns = end_ns - SECONDS * 10**9
    last15 = end_ns - 15 * 60 * 10**9
    traces = []
    n = RATE * SECONDS
    step = SECONDS * 10**9 / n
    out = []
    for i in range(n):
        ts = start_ns + int(i * step + rng.random() * step)
        hot = ts >= last15
        r = rng.random()
        if r < 0.5:
            host = rng.choice(EDGE)
            if hot and rng.random() < 0.0012:
                ip, path, status = NEEDLE_IP, "/login", rng.choice((401, 401, 401, 429))
            elif not hot and rng.random() < 0.00005:
                ip, path, status = NEEDLE_IP, "/", 200
            else:
                ip = NEAR_IP if rng.random() < 0.002 else _ip(rng)
                path = rng.choice(PATHS).format(n=rng.randrange(1, 99999), w=rng.choice(WORDS))
                burst = hot and host in ("edge-03", "edge-07") and rng.random() < 0.08
                status = 503 if burst else rng.choices((200, 304, 404, 500, 503), (0.9, 0.05, 0.04, 0.006, 0.004))[0]
            t = ts // 10**9
            stamp = _clf(t)
            body = (f'{ip} - - [{stamp}] "GET {path} HTTP/1.1" {status} {rng.randrange(200, 90000)} "-" '
                    f'"{rng.choice(UAS)}" rt={rng.random() * 0.4:.3f} uct=0.00{rng.randrange(1, 9)}')
            level = "ERROR" if status >= 500 else "WARN" if status >= 400 else "INFO"
            out.append({"ts": ts, "job": "nginx", "service": "edge-nginx", "host": host, "level": level,
                        "sev_text": level, "sev_num": {"INFO": 9, "WARN": 13, "ERROR": 17}[level], "body": body,
                        "trace_id": None, "attributes": None})
        elif r < 0.9:
            svc = rng.choice(APPS)
            host = rng.choice(APP_HOSTS[svc])
            p_err = 0.02 if not (hot and svc == "checkout") else 0.15
            level = rng.choices(("INFO", "WARN", "ERROR"), (0.92 - p_err, 0.08, p_err))[0]
            msg = rng.choice(APP_MSGS[level])
            if hot and svc == "checkout" and level == "ERROR":
                msg = "upstream timeout after 3000 ms calling payments"
            if traces and rng.random() < 0.6:
                tid = rng.choice(traces)
            else:
                tid = "%032x" % rng.getrandbits(128)
                traces.append(tid)
                if len(traces) > 50:
                    traces.pop(0)
            body = ('{"msg": "%s", "user_id": "u-%d", "order_id": "o-%d", "duration_ms": %d, "trace_id": "%s", '
                    '"span_id": "%016x"}' % (msg, rng.randrange(1, 400000), rng.randrange(1, 9000000),
                                             rng.randrange(1, 4000), tid, rng.getrandbits(64)))
            out.append({"ts": ts, "job": "app", "service": svc, "host": host, "level": level,
                        "sev_text": rng.choice(SPELL[level]), "sev_num": None, "body": body, "trace_id": tid,
                        "attributes": None})
        else:
            host = rng.choice(NET)
            sev, num, app, text = rng.choice(SYSLOG)
            ip = _ip(rng)
            if "{ip}" in text and hot and rng.random() < 0.05:
                ip = NEEDLE_IP
            msg = text.format(p=rng.randrange(1, 48), a=rng.randrange(256), b=rng.randrange(256),
                              port=rng.randrange(1024, 65535), ip=ip)
            t = ts // 10**9
            pid = rng.randrange(100, 30000)
            body = f"<{_pri(num)}>{_syslog_stamp(t)} {host} {app}[{pid}]: {msg}"
            level = "ERROR" if num >= 17 else "WARN" if num >= 13 else "INFO"
            out.append({"ts": ts, "job": "syslog", "service": app, "host": host, "level": level, "sev_text": sev,
                        "sev_num": num, "body": body, "trace_id": None,
                        "attributes": {"hostname": host, "appname": app}})
    # one trace an engineer follows: seven lines across four services in the last minutes
    tid = "5b8efff798038103d269b633813fc60c"
    for k, svc in enumerate(["gateway", "checkout", "payments", "payments", "checkout", "notify", "checkout"]):
        ts = end_ns - (5 * 60 - k * 3) * 10**9 + 1234567
        body = ('{"msg": "%s", "user_id": "u-4242", "order_id": "o-777777", "duration_ms": %d, "trace_id": "%s", '
                '"span_id": "%016x"}' % ("upstream timeout after 3000 ms calling payments" if k == 3 else "request served",
                                         100 + k, tid, k + 1))
        lvl = "ERROR" if k == 3 else "INFO"
        out.append({"ts": ts, "job": "app", "service": svc, "host": APP_HOSTS[svc][0], "level": lvl, "sev_text": lvl,
                    "sev_num": None, "body": body, "trace_id": tid, "attributes": None})
    out.sort(key=lambda x: x["ts"])
    return out


def _clf(t):
    import time
    g = time.gmtime(t)
    return f"{g.tm_mday:02d}/{MONTHS[g.tm_mon - 1]}/{g.tm_year}:{g.tm_hour:02d}:{g.tm_min:02d}:{g.tm_sec:02d} +0000"


def _syslog_stamp(t):
    import time
    g = time.gmtime(t)
    return f"{MONTHS[g.tm_mon - 1]} {g.tm_mday:2d} {g.tm_hour:02d}:{g.tm_min:02d}:{g.tm_sec:02d}"


def _pri(num):
    # facility local7 (23), severity from the number
    return 23 * 8 + {9: 6, 10: 5, 13: 4, 17: 3}[num]


TRACE = "5b8efff798038103d269b633813fc60c"


def questions(end_ns):
    """The on-call questions, each with its window: (name, start_ns, end_ns)."""
    m15 = end_ns - 15 * 60 * 10**9
    h1 = end_ns - SECONDS * 10**9
    return {
        "ip": ("Every line with one client address, last 15 minutes", m15),
        "trace": ("Every line of one trace, last hour", h1),
        "errors_per_minute": ("Errors per service per minute, last 15 minutes", m15),
        "status_503": ("503s per edge host, last hour", h1),
        "timeout": ("Lines that say timeout in any case, last 15 minutes", m15),
        "tail": ("The newest 100 lines of checkout, last hour", h1),
    }


def truth(rows, end_ns):
    """Each question's answer, from the lines: counts, groups, or the lines themselves (as bodies)."""
    q = questions(end_ns)
    win = lambda name, x: q[name][1] < x["ts"] <= end_ns  # noqa: E731
    t = {}
    t["ip"] = sorted(x["body"] for x in rows if win("ip", x) and NEEDLE_IP in x["body"])
    t["trace"] = sorted(x["body"] for x in rows if win("trace", x) and TRACE in x["body"])
    epm = {}
    for x in rows:
        if win("errors_per_minute", x) and x["level"] == "ERROR":
            minute = (x["ts"] // (60 * 10**9)) * 60
            k = (minute, x["service"])
            epm[k] = epm.get(k, 0) + 1
    t["errors_per_minute"] = {f"{m}|{s}": n for (m, s), n in sorted(epm.items())}
    s503 = {}
    for x in rows:
        if win("status_503", x) and x["job"] == "nginx" and '" 503 ' in x["body"]:
            s503[x["host"]] = s503.get(x["host"], 0) + 1
    t["status_503"] = dict(sorted(s503.items()))
    t["timeout"] = sum(1 for x in rows if win("timeout", x) and "timeout" in x["body"].lower())
    co = [x for x in rows if win("tail", x) and x["service"] == "checkout"]
    t["tail"] = sorted(x["body"] for x in co[-100:])
    return t
