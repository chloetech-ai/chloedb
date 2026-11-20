"""Made-up tickets shaped like a Zendesk export (nothing a persona brought is
copied): the columns Grace's Monday report reads, 20,000 rows over twelve
weeks. Seeded, so every run writes the same file."""

import csv
import datetime as dt
import math
import random

START = dt.datetime(2026, 8, 31, tzinfo=dt.timezone.utc)  # a Monday
WEEKS = 12
PRIORITIES = (("normal", 0.49), ("high", 0.21), ("low", 0.21), ("urgent", 0.09))
AGENTS = ("Avery", "Bo", "Cyril", "Dana", "Eun", "Farid", "Gia", "Hollis")
TAGS = ("billing", "refund", "login", "password", "invoice", "shipping", "delay", "web_form", "email", "chat",
        "how_to", "bug", "feature_request", "vip", "us", "eu")
WORDS = "charge refund login password invoice package late account upgrade export error app page card".split()


def rows(n=20000, seed=11):
    rng = random.Random(seed)
    span = WEEKS * 7 * 24 * 3600
    out = []
    for i in range(n):
        created = START + dt.timedelta(seconds=rng.randrange(span))
        pr = rng.choices([p for p, _ in PRIORITIES], [w for _, w in PRIORITIES])[0]
        # urgent tickets are solved sooner; about one in eight is not solved yet
        solved = None
        status = rng.choices(["solved", "closed", "open", "pending", "new"], [0.55, 0.32, 0.07, 0.04, 0.02])[0]
        if status in ("solved", "closed"):
            hours = math.exp(rng.gauss(math.log({"urgent": 9, "high": 22, "normal": 38, "low": 55}[pr]), 0.9))
            solved = created + dt.timedelta(seconds=round(hours * 3600))
        agent = rng.choices(AGENTS, [1.4, 1.2, 1.1, 1.0, 1.0, 0.9, 0.8, 0.6])[0]
        sat = "unoffered" if solved is None else rng.choices(["good", "bad", "offered"], [0.50 + 0.04 * AGENTS.index(agent), 0.14, 0.26])[0]
        tags = sorted(set(rng.choices(TAGS, k=1 + rng.randrange(4))))
        out.append({
            "id": f"{100000 + i}",
            "subject": " ".join(rng.choice(WORDS) for _ in range(3 + rng.randrange(4))),
            "status": status, "priority": pr, "assignee": agent, "satisfaction": sat,
            "tags": " ".join(tags),  # as Zendesk exports them: one value, spaces between
            "created_at": created.strftime("%Y-%m-%dT%H:%M:%SZ"),
            "solved_at": solved.strftime("%Y-%m-%dT%H:%M:%SZ") if solved else "",
        })
    return out


def write_csv(path, rs):
    with open(path, "w", newline="") as f:
        w = csv.DictWriter(f, fieldnames=list(rs[0]))
        w.writeheader()
        w.writerows(rs)
