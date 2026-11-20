"""Maya's head-to-head: a recipe search box, ChloeDB against Postgres.

Maya (persona study, round 4) adopts when her lead sees ChloeDB "side by side
with Postgres full-text search on our own data, with the same queries, and the
typo search never returning fewer rows than ILIKE". This builds a made-up
cookbook shaped like hers (about 4,000 recipes, names with spelling variants
such as Lasagna and Lasagne), asks the same 40 search-box texts of each
system, and measures:

- which rows each returns, against the rows the text means (recall, precision
  and the share of the first ten that are right), and any error;
- for a correctly spelled word, whether a system returned fewer rows than
  `name ILIKE '%word%'`, the thing Maya's search must never do;
- p50 and p95 of a first page (10 rows, ranked), memory and disk, and the
  setup each one takes.

Postgres is asked four ways a Flask developer would write it: ILIKE, full-text
search (websearch_to_tsquery, english), pg_trgm's word similarity, and both
(full text OR trigram). ChloeDB is asked the way its docs teach:
`MATCH(name, :q, syntax => 'plain', fuzzy => 'auto') ORDER BY bm25(name, :q)`,
and without fuzzy.

    python3 run.py            # writes raw/results.json; results.md is written from it by report.py
"""

import os
import random
import sys
import time

sys.path.insert(0, os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
import common  # noqa: E402

HERE = os.path.dirname(os.path.abspath(__file__))
WORK = os.environ.get("H2H_WORK", "/tmp/h2h-maya")
N = int(os.environ.get("H2H_RECIPES", "4000"))
REPS = int(os.environ.get("H2H_REPS", "30"))

# Each concept: the spellings a recipe's name may use, all real words.
DISHES = {
    "lasagna": ["Lasagna", "Lasagne"], "soup": ["Soup"], "curry": ["Curry"], "salad": ["Salad"], "tacos": ["Tacos"],
    "risotto": ["Risotto"], "stew": ["Stew"], "pie": ["Pie"], "bread": ["Bread"], "cake": ["Cake"], "pasta": ["Pasta"],
    "chili": ["Chili", "Chilli"], "yogurt": ["Yogurt Bowl", "Yoghurt Bowl"], "kebab": ["Kebab", "Kabob"],
    "creme_brulee": ["Crème Brûlée", "Creme Brulee"], "mac_cheese": ["Mac and Cheese", "Mac & Cheese"],
    "guacamole": ["Guacamole"], "spaghetti": ["Spaghetti"], "stir_fry": ["Stir-Fry", "Stir Fry"], "pancakes": ["Pancakes"],
}
INGREDIENTS = {
    "chicken": ["Chicken"], "beef": ["Beef"], "tomato": ["Tomato"], "broccoli": ["Broccoli"], "mushroom": ["Mushroom"],
    "lentil": ["Lentil"], "salmon": ["Salmon"], "tofu": ["Tofu"], "pumpkin": ["Pumpkin"], "spinach": ["Spinach"],
    "jalapeno": ["Jalapeño", "Jalapeno"], "potato": ["Potato"], "chickpea": ["Chickpea"], "shrimp": ["Shrimp"],
    "sweet_potato": ["Sweet Potato"], "coconut": ["Coconut"], "cauliflower": ["Cauliflower"], "eggplant": ["Eggplant"],
}
ADJECTIVES = {
    "easy": ["Easy"], "quick": ["Quick"], "classic": ["Classic"], "spicy": ["Spicy"], "creamy": ["Creamy"],
    "weeknight": ["Weeknight"], "vegan": ["Vegan"], "grandmas": ["Grandma's"], "smoky": ["Smoky"], "healthy": ["Healthy"],
    "crispy": ["Crispy"], "one_pot": ["One-Pot"],
}

# The text typed, the concepts a right row has, and what kind of text it is.
QUERIES = [
    # spelled right (a variant spelling is a real word too)
    ("lasagna", {"lasagna"}, "spelled"), ("lasagne", {"lasagna"}, "spelled"), ("chilli", {"chili"}, "spelled"),
    ("chili", {"chili"}, "spelled"), ("yogurt", {"yogurt"}, "spelled"), ("yoghurt", {"yogurt"}, "spelled"),
    ("tomato soup", {"tomato", "soup"}, "spelled"), ("chicken curry", {"chicken", "curry"}, "spelled"),
    ("jalapeño", {"jalapeno"}, "spelled"), ("jalapeno", {"jalapeno"}, "spelled"), ("creme brulee", {"creme_brulee"}, "spelled"),
    ("kebab", {"kebab"}, "spelled"), ("mushroom risotto", {"mushroom", "risotto"}, "spelled"),
    ("sweet potato pie", {"sweet_potato", "pie"}, "spelled"), ("stir fry", {"stir_fry"}, "spelled"),
    ("pancakes", {"pancakes"}, "spelled"),
    # typed with a typo
    ("lasagnia", {"lasagna"}, "typo"), ("tomatoe soup", {"tomato", "soup"}, "typo"), ("chiken curry", {"chicken", "curry"}, "typo"),
    ("brocoli", {"broccoli"}, "typo"), ("spagetti", {"spaghetti"}, "typo"), ("yoghrt", {"yogurt"}, "typo"),
    ("risoto", {"risotto"}, "typo"), ("guacamol", {"guacamole"}, "typo"), ("mushrom risotto", {"mushroom", "risotto"}, "typo"),
    ("pumkin pie", {"pumpkin", "pie"}, "typo"), ("chickpae curry", {"chickpea", "curry"}, "typo"), ("salmn", {"salmon"}, "typo"),
    ("cauliflour", {"cauliflower"}, "typo"), ("eggplnt stew", {"eggplant", "stew"}, "typo"),
    # with the punctuation a visitor types
    ("soup (easy", {"soup", "easy"}, "punct"), ('"tomato soup', {"tomato", "soup"}, "punct"), ("mac & cheese", {"mac_cheese"}, "punct"),
    ("chicken-curry", {"chicken", "curry"}, "punct"), ("soup)", {"soup"}, "punct"), ("grandma's pie", {"grandmas", "pie"}, "punct"),
    ("stir-fry", {"stir_fry"}, "punct"), ("crème brûlée", {"creme_brulee"}, "punct"), ("one-pot chili", {"one_pot", "chili"}, "punct"),
    ("salad!", {"salad"}, "punct"),
]


def cookbook(n, seed=7):
    """n made-up recipes: [adjective] [ingredient] dish, each spelling of a
    concept as likely as another; and the concepts each name holds."""
    rnd = random.Random(seed)
    rows, truth = [], {}
    dishes, ingredients, adjectives = list(DISHES), list(INGREDIENTS), list(ADJECTIVES)
    for i in range(n):
        concepts, words = set(), []
        if rnd.random() < 0.6:
            a = rnd.choice(adjectives)
            concepts.add(a)
            words.append(rnd.choice(ADJECTIVES[a]))
        if rnd.random() < 0.8:
            g = rnd.choice(ingredients)
            concepts.add(g)
            words.append(rnd.choice(INGREDIENTS[g]))
        d = rnd.choice(dishes)
        concepts.add(d)
        words.append(rnd.choice(DISHES[d]))
        rid = f"r{i:05d}"
        rows.append({"id": rid, "name": " ".join(words), "minutes": rnd.choice([10, 15, 20, 30, 45, 60, 90]),
                     "diet": rnd.choice(["any", "vegetarian", "vegan", "gluten-free"])})
        truth[rid] = concepts
    return rows, truth


def score(got, right):
    """recall, precision and precision of the first ten, of a ranked list."""
    got_set = set(got)
    hit = len(got_set & right)
    top = got[:10]
    return {
        "returned": len(got), "right": len(right), "recall": round(hit / len(right), 4) if right else None,
        "precision": round(hit / len(got_set), 4) if got_set else None,
        "p_at_10": round(sum(1 for g in top if g in right) / min(10, len(right)), 4) if right else None,
    }


PG_SQL = {
    "pg_ilike": ("SELECT id FROM recipes WHERE name ILIKE '%%' || %(q)s || '%%' ORDER BY id {limit}"),
    "pg_fts": ("SELECT id FROM recipes WHERE to_tsvector('english', name) @@ websearch_to_tsquery('english', %(q)s) "
               "ORDER BY ts_rank(to_tsvector('english', name), websearch_to_tsquery('english', %(q)s)) DESC, id {limit}"),
    "pg_trgm": ("SELECT id FROM recipes WHERE %(q)s <%% name ORDER BY word_similarity(%(q)s, name) DESC, id {limit}"),
    "pg_fts_or_trgm": ("SELECT id FROM recipes WHERE to_tsvector('english', name) @@ websearch_to_tsquery('english', %(q)s) "
                       "OR %(q)s <%% name ORDER BY ts_rank(to_tsvector('english', name), websearch_to_tsquery('english', %(q)s)) "
                       "+ word_similarity(%(q)s, name) DESC, id {limit}"),
}
CH_SQL = {
    "chloedb_fuzzy": "SELECT id FROM recipes WHERE MATCH(name, :q, syntax => 'plain', fuzzy => 'auto') ORDER BY bm25(name, :q) DESC {limit}",
    "chloedb_exact": "SELECT id FROM recipes WHERE MATCH(name, :q, syntax => 'plain') ORDER BY bm25(name, :q) DESC {limit}",
}

PG_SETUP = [
    "install Postgres and its contrib package (pg_trgm)",
    "initdb, start the server, create a database",
    "CREATE EXTENSION pg_trgm",
    "CREATE TABLE recipes (id text PRIMARY KEY, name text, minutes int, diet text)",
    "CREATE INDEX ... USING gin (to_tsvector('english', name))",
    "CREATE INDEX ... USING gin (name gin_trgm_ops)",
    "load the rows (COPY)",
    "write the query: websearch_to_tsquery for words, <% and word_similarity for typos, both ORed and ranked by hand",
]
CH_SETUP = [
    "install chloedb-server (or pip install chloedb for the embedded file)",
    "start it on a folder (or chloedb.open('recipes.chloedb'))",
    "CREATE TABLE recipes (id KEYWORD PRIMARY KEY, name TEXT, minutes INT, diet KEYWORD)",
    "write the rows (POST /v1/write, or db.write)",
    "write the query: MATCH(name, :q, syntax => 'plain', fuzzy => 'auto') ORDER BY bm25(name, :q) DESC",
]


def run_postgres(rows, truth):
    pg = common.Postgres(os.path.join(WORK, "pg"))
    c = pg.conn
    c.execute("CREATE EXTENSION pg_trgm")
    c.execute("CREATE TABLE recipes (id text PRIMARY KEY, name text, minutes int, diet text)")
    t = time.perf_counter()
    with c.cursor() as cur:
        with cur.copy("COPY recipes (id, name, minutes, diet) FROM STDIN") as cp:
            for r in rows:
                cp.write_row((r["id"], r["name"], r["minutes"], r["diet"]))
    c.execute("CREATE INDEX recipes_fts ON recipes USING gin (to_tsvector('english', name))")
    c.execute("CREATE INDEX recipes_trgm ON recipes USING gin (name gin_trgm_ops)")
    c.execute("VACUUM ANALYZE recipes")
    load_s = time.perf_counter() - t
    disk = c.execute("SELECT pg_total_relation_size('recipes')").fetchone()[0]
    out = {"version": pg.version, "load_s": round(load_s, 3), "table_and_indexes_bytes": disk, "systems": {}}
    for name, sql in PG_SQL.items():
        per = []
        all_ms = []
        for q, concepts, kind in QUERIES:
            right = {rid for rid, cs in truth.items() if concepts <= cs}
            err = None
            try:
                got = [r[0] for r in c.execute(sql.format(limit=""), {"q": q}).fetchall()]
            except Exception as e:  # noqa: BLE001 - an error is a result here
                got, err = [], str(e).splitlines()[0]
            s = score(got, right)
            page = sql.format(limit="LIMIT 10")
            for _ in range(3):
                c.execute(page, {"q": q}).fetchall()
            ms = common.timed(lambda: c.execute(page, {"q": q}).fetchall(), REPS)
            all_ms += ms
            per.append({"q": q, "kind": kind, **s, "error": err, **common.percentiles(ms)})
        out["systems"][name] = {"queries": per, "page": common.percentiles(all_ms)}
    out["memory_kb_pss"] = pg.memory_kb()
    pg.stop()
    out["data_dir_bytes"] = common.du_bytes(os.path.join(WORK, "pg", "data"))
    return out


def run_chloedb(rows, truth):
    ch = common.Chloedb(os.path.join(WORK, "chloedb"))
    ch.sql("CREATE TABLE recipes (id KEYWORD PRIMARY KEY, name TEXT, minutes INT, diet KEYWORD)")
    t = time.perf_counter()
    ch.write("recipes", rows)
    load_s = time.perf_counter() - t
    out = {"version": ch.version, "load_s": round(load_s, 3), "systems": {}}
    for name, sql in CH_SQL.items():
        per = []
        all_ms, all_took = [], []
        for q, concepts, kind in QUERIES:
            right = {rid for rid, cs in truth.items() if concepts <= cs}
            r = ch.sql(sql.format(limit=f"LIMIT {N}"), {"q": q})
            err = None
            if "error" in r:
                got, err = [], r["error"][:200]
            else:
                got = [row[0] for row in r["rows"]]
            s = score(got, right)
            s["widened_to"] = [w for line in (r.get("stats") or {}).get("plan", []) if line.startswith("bm25 expanded")
                               for w in line.split(": ", 1)[1].split(", ")]
            page = sql.format(limit="LIMIT 10")
            for _ in range(3):
                ch.sql(page, {"q": q})
            took = []
            ms = common.timed(lambda: took.append(ch.sql(page, {"q": q}).get("took_ms", 0)), REPS)
            all_ms += ms
            all_took += took
            per.append({"q": q, "kind": kind, **s, "error": err, **common.percentiles(ms),
                        "server": common.percentiles(took)})
        # page: as the client saw it (HTTP and JSON included); server: the server's own took_ms
        out["systems"][name] = {"queries": per, "page": common.percentiles(all_ms), "server": common.percentiles(all_took)}
    out["memory_kb_pss"] = ch.memory_kb()
    ch.stop()
    out["disk_bytes"] = ch.disk_bytes()
    return out


def main():
    rows, truth = cookbook(N)
    result = {
        "machine": common.machine(), "recipes": N, "reps": REPS, "queries": len(QUERIES),
        "at": time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime()),
        "setup": {"postgres": PG_SETUP, "chloedb": CH_SETUP},
        "postgres": run_postgres(rows, truth),
        "chloedb": run_chloedb(rows, truth),
    }
    # Maya's line: for a word spelled right, never fewer rows than ILIKE '%word%'.
    ilike = {p["q"]: p["returned"] for p in result["postgres"]["systems"]["pg_ilike"]["queries"]}
    for side in ("postgres", "chloedb"):
        for name, s in result[side]["systems"].items():
            s["fewer_than_ilike"] = [p["q"] for p in s["queries"] if p["kind"] == "spelled" and p["returned"] < ilike[p["q"]]]
    common.write_json(os.path.join(HERE, "raw", "results.json"), result)
    print("wrote", os.path.join(HERE, "raw", "results.json"))


if __name__ == "__main__":
    main()
