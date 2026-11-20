"""``python -m chloedb fetch-model NAME [--models DIR]``: download an
embedding model ChloeDB knows ahead of its first use (opening a database
whose schema names it downloads it too)."""

from __future__ import annotations

import argparse
import sys

from . import fetch_model


def main(argv: list[str] | None = None) -> int:
    ap = argparse.ArgumentParser(prog="python -m chloedb")
    sub = ap.add_subparsers(dest="command", required=True)
    f = sub.add_parser("fetch-model", help="download an embedding model ChloeDB knows (potion-retrieval-32M, potion-base-8M, ...)")
    f.add_argument("name", nargs="?", default="potion-retrieval-32M")
    f.add_argument("--models", help="the models directory (default: $CHLOEDB_MODELS, else ~/.local/share/chloedb/models)")
    a = ap.parse_args(argv)
    try:
        print(fetch_model(a.name, a.models))
    except Exception as e:  # noqa: BLE001 - a command line says why and stops
        print(f"chloedb: {e}", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())
