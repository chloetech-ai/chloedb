"""fdatasync after a small write: appending to a new file (as ChloeDB's WAL does),
overwriting blocks already written (as SQLite's reused WAL does), and appending
into a file whose space was written with zeros first."""
import os, sys, time, statistics
d = sys.argv[1]; n = int(sys.argv[2]) if len(sys.argv) > 2 else 400
def run(mode):
    p = os.path.join(d, f"f-{mode}")
    if os.path.exists(p): os.remove(p)
    fd = os.open(p, os.O_RDWR | os.O_CREAT)
    if mode in ("overwrite", "prealloc"):
        os.write(fd, b"\0" * (300 * (n + 10))); os.fsync(fd)
    ts = []
    for i in range(n):
        t = time.perf_counter()
        os.pwrite(fd, (b"%08d" % i) * 37 + b"xxxx", 300 * i if mode != "overwrite" else 300 * (i % 4))
        os.fdatasync(fd)
        ts.append((time.perf_counter() - t) * 1000)
    os.close(fd); os.remove(p); s = sorted(ts)
    print(f"{mode:10} p50 {s[n//2]:.3f} p90 {s[int(n*.9)]:.3f} p99 {s[int(n*.99)]:.3f} mean {statistics.mean(ts):.3f} ms")
for m in ["append", "prealloc", "overwrite", "append", "prealloc", "overwrite"]:
    run(m)
