#!/usr/bin/env python3
"""A discrete-event model of chloedb-route-bench --external: closed-loop
clients, one coordinator, shards of copies reached over a network, each copy
a processor-sharing CPU with a number of cores, optional query slots, and
the injected faults of the bench's scenarios. The coordinator's routing,
hedging and cool-off follow chloedb-server's coord.rs, cores/route.rs and
coord/replica_stats.rs (main 1458dff):

  round-robin: the turn's copy; copies in cool-off after the others.
  least-cost:  the turn's copy unless another, drawn at random, costs < 75%
               of it and >= 1 ms less; cost = max(rtt_fast, 1 ms) x (1 + in
               flight); rtt_fast an EWMA (2 s or 20 calls).
  hedge:       after 100 ms, or with adaptive hedging after the copy's
               plain_slow + 6 x plain_dev (30 s or 200 calls), clamped to
               [2, 100] ms, once 20 samples are known; a budget of 5% of
               calls, 10 saved. A copy outrun is put in cool-off for 5 s,
               unless an adaptive hedge outran it before 100 ms.

Time is in milliseconds.
"""
import heapq, math, random
from collections import deque

FLOOR_MS, MARGIN, MIN_GAIN_MS = 1.0, 0.25, 1.0
HEDGE_MS, HEDGE_FLOOR, TAIL_SAMPLES, TAIL_DEV = 100.0, 2.0, 20, 6.0
HEDGE_SHARE, HEDGE_BURST, COOL_OFF, RTT_FRESH = 0.05, 10.0, 5000.0, 10000.0
FAST_TAU, FAST_CAP, SLOW_TAU, SLOW_CAP = 2000.0, 20, 30000.0, 200


class Ewma:
    __slots__ = ("v", "at", "n")

    def __init__(self):
        self.v, self.at, self.n = None, None, 0

    def sample(self, x, now, tau, cap):
        self.n = min(self.n + 1, cap)
        if self.v is None:
            self.v = x
        else:
            w = max(1.0 - math.exp(-(now - self.at) / tau), 1.0 / self.n)
            self.v += w * (x - self.v)
        self.at = now

    def at_least(self, x, now, tau, cap):
        if self.v is None or self.v < x:
            self.sample(x, now, tau, cap)


class Stats:
    """What the coordinator keeps of one copy."""
    __slots__ = ("fast", "slow", "dev", "inflight", "avoid_until")

    def __init__(self):
        self.fast, self.slow, self.dev = Ewma(), Ewma(), Ewma()
        self.inflight, self.avoid_until = 0, -1.0

    def took(self, ms, now):
        if self.slow.v is not None:
            self.dev.sample(abs(ms - self.slow.v), now, SLOW_TAU, SLOW_CAP)
        self.fast.sample(ms, now, FAST_TAU, FAST_CAP)
        self.slow.sample(ms, now, SLOW_TAU, SLOW_CAP)

    def outran(self, ms, now):
        self.fast.at_least(ms, now, FAST_TAU, FAST_CAP)

    def rtt(self, now):
        f = self.fast
        return f.v if f.v is not None and now - f.at <= RTT_FRESH else None

    def hedge_ms(self):
        if self.slow.n < TAIL_SAMPLES:
            return None
        return self.slow.v + TAIL_DEV * (self.dev.v or 0.0)


class Copy:
    """A copy's CPU: egalitarian processor sharing on `cores` cores at
    `speed` (1 = a copy alike the calibration ones), a job using one core
    at most; `slots` query slots (calls past them wait in turn); paused
    (no progress) inside its pause windows."""

    def __init__(self, sim, idx, cores, speed, slots, pause=None, extra=None, n_half=0.0, stress=0):
        self.sim, self.idx = sim, idx
        self.cores, self.speed, self.slots = cores, speed, slots
        self.n_half, self.stress = n_half, stress
        self.pause = pause          # (period_ms, stop_ms, phase_ms) or None
        self.extra = extra          # fn(now) -> ms added to an answer, or None
        self.V, self.t, self.n, self.inuse = 0.0, 0.0, 0, 0
        self.heap, self.queue, self.ver, self.seq = [], deque(), 0, 0
        self.paused = False
        self.busy_area = 0.0        # core-ms used, for utilisation

    def rate(self):
        if self.paused or self.n == 0:
            return 0.0
        n = self.n
        # The server's CPU use grows with its concurrency, to `cores` at most.
        c = self.cores * n / (n + self.n_half)
        r = min(1.0, c / n)
        if self.stress:
            # CPU-bound threads beside it: the node's 2 vCPUs shared by thread.
            r = min(r, 2.0 / (n + self.stress))
        return self.speed * r

    def advance(self, now):
        dt = now - self.t
        if dt > 0:
            r = self.rate()
            self.V += r * dt
            if r > 0 and now > self.sim.p["warmup"]:
                self.busy_area += r * self.n / self.speed * dt
            self.t = now

    def arrive(self, now, call, demand):
        self.advance(now)
        if self.slots is not None and self.inuse >= self.slots:
            self.queue.append((call, demand))
            return
        self._start(call, demand)
        self._schedule(now)

    def _start(self, call, demand):
        self.inuse += 1
        self.n += 1
        self.seq += 1
        heapq.heappush(self.heap, (self.V + demand, self.seq, call))

    def _schedule(self, now):
        self.ver += 1
        r = self.rate()
        if self.heap and r > 0:
            t = now + max(0.0, (self.heap[0][0] - self.V) / r)
            self.sim.push(t, 2, (self, self.ver))

    def depart(self, now, ver):
        if ver != self.ver:
            return
        self.advance(now)
        eps = 1e-9
        while self.heap and self.heap[0][0] <= self.V + eps:
            _, _, call = heapq.heappop(self.heap)
            self.n -= 1
            self.inuse -= 1
            self.sim.answered_by_copy(now, call, self)
            if self.queue:
                c, d = self.queue.popleft()
                self._start(c, d)
        self._schedule(now)

    def set_paused(self, now, paused):
        self.advance(now)
        self.paused = paused
        self._schedule(now)


class Sim:
    def __init__(self, p, rng):
        self.p, self.rng = p, rng
        self.ev, self.seq = [], 0
        self.now = 0.0
        S, R = p["shards"], p["copies"]
        self.copies = [[Copy(self, (i, r), p["cores"][i][r], p["speed"][i][r], p["slots"][i][r],
                             p.get("pause", {}).get((i, r)), p.get("extra", {}).get((i, r)),
                             p.get("n_half", 0.0), p.get("stress", {}).get((i, r), 0))
                        for r in range(R)] for i in range(S)]
        self.stats = [[Stats() for _ in range(R)] for _ in range(S)]
        self.turn = 0
        self.budget = HEDGE_BURST
        self.lat, self.hedges, self.done = [], 0, 0
        self.answers = [[0] * R for _ in range(S)]
        self.inflight_area = [[0.0] * R for _ in range(S)]
        self.last_t = 0.0

    def push(self, t, kind, data):
        self.seq += 1
        heapq.heappush(self.ev, (t, self.seq, kind, data))

    # Network: a call's two legs; the bench's delay line adds to the answer.
    def leg(self):
        p = self.p
        return 0.5 * p["net_med"] * math.exp(self.rng.gauss(0.0, p["net_sigma"]))

    def demand(self):
        p = self.p
        return p["d_med"] * math.exp(self.rng.gauss(0.0, p["d_sigma"]))

    def order(self, i, start):
        R = self.p["copies"]
        now = self.now
        st = self.stats[i]
        rot = [(start + k) % R for k in range(R)]
        if self.p["route"] == "least-cost":
            rtts = [s.rtt(now) for s in st]
            known = sorted(v for v in rtts if v is not None)
            if known:
                m = len(known) // 2
                neutral = known[m] if len(known) % 2 else (known[m - 1] + known[m]) / 2
            else:
                neutral = FLOOR_MS
            cost = [max(rtts[r] if rtts[r] is not None else neutral, FLOOR_MS) * (1 + st[r].inflight) for r in range(R)]
            openc = [r for r in rot if st[r].avoid_until <= now]
            avoided = [r for r in rot if st[r].avoid_until > now]
            if not openc and self.p.get("fix_all_avoided"):
                # Every copy in cool-off: still choose by cost, not by turn.
                openc, avoided = avoided, []
            out = []
            if openc:
                lead = openc[0]
                if len(openc) > 1:
                    other = openc[1 + self.rng.randrange(len(openc) - 1)]
                    if cost[other] < cost[lead] * (1 - MARGIN) and cost[lead] - cost[other] >= MIN_GAIN_MS:
                        lead = other
                out.append(lead)
                rest = sorted((r for r in openc if r != lead), key=lambda r: cost[r])
                out.extend(rest)
            out.extend(avoided)
            return out
        return sorted(rot, key=lambda r: st[r].avoid_until > now)

    def hedge_after(self, i, r):
        if self.p["adaptive"]:
            h = self.stats[i][r].hedge_ms()
            if h is not None:
                return min(max(h, HEDGE_FLOOR), HEDGE_MS)
        return HEDGE_MS

    def track(self, now):
        dt = now - self.last_t
        if dt > 0 and now > self.p["warmup"]:
            for i, row in enumerate(self.stats):
                for r, s in enumerate(row):
                    self.inflight_area[i][r] += s.inflight * dt
        self.last_t = now

    def launch(self, q, i, r, again):
        st = self.stats[i][r]
        st.inflight += 1
        call = {"q": q, "i": i, "r": r, "t0": self.now, "open": True}
        q["calls"][i].append(call)
        q["asked"][i].append((r, self.now))
        self.push(self.now + self.leg(), 1, (call, q["demand"][i]))

    def start_query(self, client):
        S = self.p["shards"]
        self.turn += 1
        q = {"client": client, "t0": self.now, "left": S, "calls": [[] for _ in range(S)],
             "asked": [[] for _ in range(S)], "done": [False] * S, "order": [None] * S,
             "hedging": [True] * S, "hver": [0] * S,
             "demand": [self.demand() for _ in range(S)]}
        for i in range(S):
            q["order"][i] = self.order(i, self.turn)
            self.budget = min(self.budget + HEDGE_SHARE, HEDGE_BURST)
            r = q["order"][i][0]
            self.launch(q, i, r, False)
            self.push(self.now + self.hedge_after(i, r), 3, (q, i, 0))

    def answered_by_copy(self, now, call, copy):
        extra = copy.extra(now) if copy.extra else 0.0
        self.push(now + self.leg() + extra, 4, call)

    def on_answer(self, call):
        if not call["open"]:
            return  # outrun: the coordinator stopped waiting for it
        now = self.now
        q, i, r = call["q"], call["i"], call["r"]
        st = self.stats[i][r]
        st.inflight -= 1
        call["open"] = False
        st.took(now - call["t0"], now)
        q["done"][i] = True
        if now > self.p["warmup"]:
            self.answers[i][r] += 1
        # The calls still open were outrun: dropped by the coordinator (the
        # copy still does the work), their time a lower bound.
        for c in q["calls"][i]:
            if c["open"]:
                c["open"] = False
                o = self.stats[i][c["r"]]
                o.inflight -= 1
                o.outran(now - c["t0"], now)
                # coord.rs: only an adaptive hedge's loser asked < --hedge-ms ago
                # is spared; fix_early spares every copy asked that recently.
                early = (self.p["adaptive"] or self.p.get("fix_early")) and now - c["t0"] < HEDGE_MS
                if not early:
                    o.avoid_until = now + COOL_OFF
        q["left"] -= 1
        if q["left"] == 0:
            if now > self.p["warmup"]:
                self.lat.append(now - q["t0"])
                self.done += 1
            self.start_query(q["client"])

    def on_hedge(self, q, i, ver):
        if q["done"][i] or ver != q["hver"][i] or not q["hedging"][i]:
            return
        asked = [a for a, _ in q["asked"][i]]
        more = [r for r in q["order"][i] if r not in asked]
        if not more:
            return
        if self.budget < 1.0:
            q["hedging"][i] = False
            return
        self.budget -= 1.0
        if self.now > self.p["warmup"]:
            self.hedges += 1
        nxt = more[0]
        self.launch(q, i, nxt, True)
        q["hver"][i] += 1
        self.push(self.now + self.hedge_after(i, nxt), 3, (q, i, q["hver"][i]))

    def run(self):
        p = self.p
        end = p["warmup"] + p["seconds"]
        for i, row in enumerate(self.copies):
            for c in row:
                if c.pause:
                    period, stop, phase = c.pause
                    t = phase
                    while t < end:
                        self.push(t, 0, (c, True))
                        self.push(t + stop, 0, (c, False))
                        t += period
        for k in range(p["clients"]):
            self.start_query(k)
        while self.ev:
            t, _, kind, data = heapq.heappop(self.ev)
            if t > end:
                break
            self.track(t)
            self.now = t
            if kind == 1:
                call, d = data
                self.copies[call["i"]][call["r"]].arrive(t, call, d)
            elif kind == 2:
                c, ver = data
                c.depart(t, ver)
            elif kind == 4:
                self.on_answer(data)
            elif kind == 3:
                self.on_hedge(*data)
            elif kind == 0:
                c, on = data
                c.set_paused(t, on)
        secs = p["seconds"] / 1000.0
        lat = sorted(self.lat)
        def pct(x):
            if not lat:
                return float("nan")
            return lat[min(len(lat) - 1, int(x * len(lat)))]
        util = [[c.busy_area / (c.cores * p["seconds"]) for c in row] for row in self.copies]
        return {"qps": self.done / secs, "p50": pct(0.5), "p90": pct(0.9), "p99": pct(0.99),
                "p999": pct(0.999), "mean": sum(lat) / len(lat) if lat else float("nan"),
                "hedges": self.hedges, "answers": self.answers,
                "inflight": [[a / p["seconds"] for a in row] for row in self.inflight_area],
                "util": util}


def stall_extra(phase):
    """The bench's stall: once a second, what the copy answers in 50 ms is
    held 60 ms (the delay line's delay is 60 for 50 ms after 950 ms)."""
    def f(now):
        x = (now - phase) % 1000.0
        return 60.0 if x >= 950.0 else 0.0
    return f


def run(p, seed=1):
    return Sim(p, random.Random(seed)).run()
