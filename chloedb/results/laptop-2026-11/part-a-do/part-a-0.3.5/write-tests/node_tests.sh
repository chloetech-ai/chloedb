#!/bin/bash
# Run on the bench pod after Part A: why single-row writes are slow on this node.
set -x
cd /work/wtest
command -v strace >/dev/null || (apt-get update -qq >/dev/null && apt-get install -y -qq strace >/dev/null)
S353="env LD_LIBRARY_PATH=/work/sqlite-3.53.1/lib"
echo "## fsync micro on /work"; python3 fsync_micro.py /work/wtest 400
echo "## 0.3.5 writes"; /work/venv/bin/python3 writes.py /work/wtest/db-035 1000
echo "## 6 Nov build writes"; /work/wtest/b022/bin/python3 writes.py /work/wtest/db-b022 1000
echo "## 0.3.5 writes again"; /work/venv/bin/python3 writes.py /work/wtest/db-035 1000
echo "## 6 Nov build writes again"; /work/wtest/b022/bin/python3 writes.py /work/wtest/db-b022 1000
echo "## sqlite FULL"; $S353 /work/venv/bin/python3 sqlite_writes.py /work/wtest/db-sq 1000 FULL
echo "## sqlite NORMAL"; $S353 /work/venv/bin/python3 sqlite_writes.py /work/wtest/db-sq 1000 NORMAL
TR="trace=fsync,fdatasync,rename,renameat,renameat2,unlink,unlinkat,openat,write,pwrite64,ftruncate,fallocate"
echo "## 0.3.5 under strace -T"
rm -f mark; MARK=/work/wtest/mark strace -f -T -o /work/wtest/trace-035.txt -e $TR /work/venv/bin/python3 writes.py /work/wtest/db-035s 2000
python3 count.py /work/wtest/trace-035.txt 2000; python3 slow.py /work/wtest/trace-035.txt
echo "## 6 Nov build under strace -T"
rm -f mark; MARK=/work/wtest/mark strace -f -T -o /work/wtest/trace-b022.txt -e $TR /work/wtest/b022/bin/python3 writes.py /work/wtest/db-b022s 2000
python3 count.py /work/wtest/trace-b022.txt 2000; python3 slow.py /work/wtest/trace-b022.txt
echo "## sqlite FULL under strace -T"
rm -f mark; MARK=/work/wtest/mark $S353 strace -f -T -o /work/wtest/trace-sq.txt -e $TR /work/venv/bin/python3 sqlite_writes.py /work/wtest/db-sqs 2000 FULL
python3 count.py /work/wtest/trace-sq.txt 2000; python3 slow.py /work/wtest/trace-sq.txt
echo "## done"
