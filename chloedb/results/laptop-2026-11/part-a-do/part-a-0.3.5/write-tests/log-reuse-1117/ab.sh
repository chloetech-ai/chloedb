#!/bin/bash
# On the bench pod: single-row writes, 0.3.5 against main (sync=True and
# sync=False) and main with the log change, in turns, three rounds; SQLite
# 3.53.1 FULL and NORMAL beside them; the fdatasync micro test; and the
# system calls of one run of each sync=True build.
set -x
cd /work/ab
lscpu | grep -E "Model name|^CPU\(s\)"; df -h /work | tail -1
echo "## fsync micro on /work"; python3 fsync_micro.py /work/ab 400
S="env LD_LIBRARY_PATH=/work/sqlite-3.53.1/lib"
for r in 1 2 3; do
  for c in v035:1 main:1 wal:1 main:0 wal:0; do
    t=${c%:*}; s=${c#*:}
    echo "## round $r $t sync=$s"; SYNC=$s v-$t/bin/python3 writes.py /work/ab/db-$t-$s 2000
  done
  echo "## round $r sqlite FULL"; $S v-main/bin/python3 sqlite_writes.py /work/ab/db-sq 2000 FULL
  echo "## round $r sqlite NORMAL"; $S v-main/bin/python3 sqlite_writes.py /work/ab/db-sq 2000 NORMAL
done
TR="trace=fsync,fdatasync,rename,renameat,renameat2,unlink,unlinkat,openat,write,pwrite64,ftruncate,fallocate"
for t in v035 wal; do
  echo "## $t under strace -T"
  rm -f mark; MARK=/work/ab/mark strace -f -T -o /work/ab/trace-$t.txt -e $TR v-$t/bin/python3 writes.py /work/ab/db-$t-s 2000
  python3 count.py /work/ab/trace-$t.txt 2000
  echo "pwrite64 calls on app.strata-wal and their times:"; python3 - /work/ab/trace-$t.txt <<'PY'
import re, sys, statistics
fd = {}; t = []
for line in open(sys.argv[1]):
    m = re.search(r'openat\(.*?"([^"]+)".*\) = (\d+)', line)
    if m: fd[m.group(2)] = m.group(1); continue
    m = re.search(r'fdatasync\((\d+)\).*<([\d.]+)>', line)
    if m and fd.get(m.group(1), "").endswith("-wal"): t.append(float(m.group(2)) * 1000)
t.sort(); print(f"fdatasync on the log: {len(t)} calls, p50 {t[len(t)//2]:.3f} ms, p90 {t[int(len(t)*.9)]:.3f} ms, mean {statistics.mean(t):.3f} ms")
PY
done
echo "## AB DONE"
