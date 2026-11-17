#!/bin/bash
# On the bench pod: Rust, SQLite 3.53.1, and the Python package of three trees
# (0.3.5, main, main with the log change), each in its own venv.
set -ex
cd /work/ab
apt-get update -qq && apt-get install -y -qq build-essential curl strace >/dev/null
command -v cargo >/dev/null || curl -sSf https://sh.rustup.rs | sh -s -- -y --profile minimal >/dev/null
. $HOME/.cargo/env
rustc --version
python3 -m venv /work/build && /work/build/bin/pip install -q maturin
for t in v035 main wal; do
  rm -rf src-$t && tar -xzf src-$t.tar.gz
  (cd src-$t/chloedb-py && CARGO_TARGET_DIR=/work/ab/target /work/build/bin/maturin build --profile ci -o /work/ab/wheel-$t)
  rm -rf v-$t && python3 -m venv v-$t && v-$t/bin/pip install -q numpy && v-$t/bin/pip install -q --no-deps wheel-$t/*.whl
  v-$t/bin/python3 -c 'import chloedb; print("ok", chloedb.__version__)'
done
mkdir -p /work/src && cd /work/src
[ -f sqlite-autoconf-3530100.tar.gz ] || curl -sSfO https://www.sqlite.org/2026/sqlite-autoconf-3530100.tar.gz
rm -rf sqlite-autoconf-3530100 && tar xzf sqlite-autoconf-3530100.tar.gz && cd sqlite-autoconf-3530100
CFLAGS="-O2 -DSQLITE_ENABLE_FTS5" ./configure --prefix=/work/sqlite-3.53.1 >/dev/null
make -j8 >/dev/null 2>&1 && make install >/dev/null 2>&1
LD_LIBRARY_PATH=/work/sqlite-3.53.1/lib python3 -c 'import sqlite3; print("SQLITE OK", sqlite3.sqlite_version)'
echo BUILD DONE
