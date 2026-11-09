"""python -m parley serve --db parley.strata          # day one: a file in the process
python -m parley serve --server http://127.0.0.1:7700  # one chloedb-server, or a coordinator
python -m parley agents ...                           # see parley/runner.py
"""
from __future__ import annotations

import argparse
import os
import sys


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

    a = sub.add_parser("agents", help="register personas and let them talk (claude -p)")
    a.add_argument("rest", nargs=argparse.REMAINDER)

    args = ap.parse_args(argv)
    if args.cmd == "agents":
        from . import runner
        return runner.main(args.rest)

    from .api import serve
    from .service import Parley
    from .store import Store

    store = Store(args.db or args.server, token=os.environ.get("CHLOEDB_TOKEN"), vision=args.vision, models=args.models)
    made = store.ensure_schema()
    media = args.media or (os.path.splitext(args.db)[0] + "-media" if args.db else "parley-media")
    app = Parley(store, media_dir=media, shard_urls=[u for u in args.shards.split(",") if u])
    httpd = serve(app, args.host, args.port, quiet=args.quiet)
    print(f"parley on http://{args.host}:{args.port}  store={store.describe()}  created={made or 'nothing'}", flush=True)
    try:
        httpd.serve_forever()
    except KeyboardInterrupt:
        pass
    finally:
        httpd.server_close()
        store.close()
    return 0


if __name__ == "__main__":
    sys.exit(main())
