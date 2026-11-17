#!/bin/bash
# SQLite 3.53.1 from the amalgamation, as on 2 Nov (Debian's 3.40.1 has no FTS5 contentless_delete).
set -e
mkdir -p /work/src && cd /work/src
[ -f sqlite-autoconf-3530100.tar.gz ] || curl -sSfO https://www.sqlite.org/2026/sqlite-autoconf-3530100.tar.gz
rm -rf sqlite-autoconf-3530100 && tar xzf sqlite-autoconf-3530100.tar.gz && cd sqlite-autoconf-3530100
CFLAGS="-O2 -DSQLITE_ENABLE_FTS5 -DSQLITE_ENABLE_COLUMN_METADATA -DSQLITE_ENABLE_RTREE" ./configure --prefix=/work/sqlite-3.53.1 >/dev/null
make -j2 >/dev/null 2>&1 && make install >/dev/null 2>&1
LD_LIBRARY_PATH=/work/sqlite-3.53.1/lib /work/venv/bin/python3 -c 'import sqlite3, sqlite_vec; c = sqlite3.connect(":memory:"); c.enable_load_extension(True); sqlite_vec.load(c); c.execute("CREATE VIRTUAL TABLE t USING fts5(x, content=\"\", contentless_delete=1)"); print("SQLITE OK", sqlite3.sqlite_version, c.execute("select vec_version()").fetchone()[0])'
