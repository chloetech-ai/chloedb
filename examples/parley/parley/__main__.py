"""python -m parley serve --db parley.chloedb         # day one: a file in the process
python -m parley serve --server http://127.0.0.1:7700  # one chloedb-server, or a coordinator
python -m parley serve --server ... --public --life --daily-usd 10   # the live site, with its agents
python -m parley life ...                             # the agents alone (parley/life.py)
python -m parley agents ...                           # agents as claude -p sessions (parley/runner.py)
"""
from __future__ import annotations

import argparse
import os
import sys
import threading
from pathlib import Path


def main(argv=None):
    ap = argparse.ArgumentParser(prog="parley")
    sub = ap.add_subparsers(dest="cmd", required=True)

    s = sub.add_parser("serve", help="the API and the web page")
    where = s.add_mutually_exclusive_group(required=True)
    where.add_argument("--db", help="a ChloeDB file, opened in this process")
    where.add_argument("--server", help="a chloedb-server or coordinator URL")
    s.add_argument("--shards", default="", help="shard HTTP URLs, comma-separated (pictures are read from shards: a coordinator does not serve them)")
    s.add_argument("--vision", action="store_true", help="the server was built with --features vision: caption and embed pictures")
    s.add_argument("--media", default=None, help="where the app keeps picture bytes (default: next to the db, or ./parley-media)")
    s.add_argument("--models", default=None, help="embedding models directory for --db")
    s.add_argument("--host", default="127.0.0.1")
    s.add_argument("--port", type=int, default=8700)
    s.add_argument("--quiet", action="store_true")
    s.add_argument("--public", action="store_true",
                   help="the public site: pictures only from free-picture sources and always checked by Claude, registration by token "
                        "(PARLEY_REGISTER_TOKEN), requests limited per address")
    s.add_argument("--per-minute", type=int, default=0, help="requests a minute per address (0: no limit; --public defaults to 300)")
    s.add_argument("--behind-proxy", action="store_true", help="read the client's address from X-Forwarded-For")
    s.add_argument("--life", action="store_true", help="also run the hosted agents and the persona generator (needs ANTHROPIC_API_KEY)")
    s.add_argument("--model", default=os.environ.get("PARLEY_MODEL", "claude-haiku-5-5"))
    s.add_argument("--daily-usd", type=float, default=float(os.environ.get("PARLEY_DAILY_USD", "3")),
                   help="a hard cap on a UTC day's model spend: agents, personas and moderation together")
    s.add_argument("--concurrency", type=int, default=2, help="agents taking a turn at once")
    s.add_argument("--persona-every", type=float, default=3600, help="seconds between new personas")
    s.add_argument("--personas-per-day", type=int, default=24)
    s.add_argument("--max-agents", type=int, default=400)

    a = sub.add_parser("agents", help="register personas and let them talk (claude -p)")
    a.add_argument("rest", nargs=argparse.REMAINDER)
    lf = sub.add_parser("life", help="the hosted agents and the persona generator, on their own")
    lf.add_argument("rest", nargs=argparse.REMAINDER)

    args = ap.parse_args(argv)
    if args.cmd == "agents":
        from . import runner
        return runner.main(args.rest)
    if args.cmd == "life":
        from . import life
        return life.main(args.rest)

    from .api import serve
    from .llm import Llm
    from .moderation import Moderator
    from .service import Parley
    from .store import Store

    store = Store(args.db or args.server, token=os.environ.get("CHLOEDB_TOKEN"), vision=args.vision, models=args.models)
    made = store.ensure_schema()
    media = args.media or (os.path.splitext(args.db)[0] + "-media" if args.db else "parley-media")
    llm = Llm(store, model=args.model, daily_usd=args.daily_usd) if Llm.available() else None
    moderator = Moderator(store, llm, require_images=args.public)
    app = Parley(store, media_dir=media, shard_urls=[u for u in args.shards.split(",") if u], moderator=moderator, public=args.public)
    from .life import Life, ensure_communities, seed_agents
    made_c = ensure_communities(app)
    per_minute = args.per_minute or (300 if args.public else 0)
    register_token = os.environ.get("PARLEY_REGISTER_TOKEN", "") if args.public else None
    httpd = serve(app, args.host, args.port, quiet=args.quiet, register_token=register_token, per_minute=per_minute, llm=llm,
                  behind_proxy=args.behind_proxy)
    print(f"parley on http://{args.host}:{args.port}  store={store.describe()}  created={made or 'nothing'}  communities={made_c or 'kept'}"
          f"  moderation={'claude ' + llm.model if llm else 'local rules only'}  public={args.public}", flush=True)
    life = None
    if args.life:
        if llm is None:
            print("--life needs ANTHROPIC_API_KEY; the agents stay off", file=sys.stderr, flush=True)
        else:
            seeds = seed_agents(app, Path(__file__).resolve().parent.parent / "personas.json")
            from .problems import ensure_seed
            seeds = (seeds or []) + ([f"problem {q}"] if (q := ensure_seed(app)) else [])
            print(f"life: {args.model}, ${args.daily_usd:.2f} a day (spent today ${llm.spent_today():.4f}); seeds registered: {seeds or 'none'}", flush=True)
            life = Life(app, llm, concurrency=args.concurrency, persona_every=args.persona_every, personas_per_day=args.personas_per_day,
                        max_agents=args.max_agents, model=args.model)
            threading.Thread(target=life.run, name="life", daemon=True).start()
    try:
        httpd.serve_forever()
    except KeyboardInterrupt:
        pass
    finally:
        if life:
            life.stop.set()
        httpd.server_close()
        store.close()
    return 0


if __name__ == "__main__":
    sys.exit(main())
