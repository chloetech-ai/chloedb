"""Time per syscall kind inside each marked section of an strace -f -T log, and the slowest calls."""
import sys, re, collections
seg = None; tot = collections.defaultdict(collections.Counter); cnt = collections.defaultdict(collections.Counter); slow = []
for line in open(sys.argv[1]):
    m = re.search(r'write\(\d+, "=== (.+?) (start|end)', line)
    if m:
        seg = m.group(1) if m.group(2) == "start" else None; continue
    m = re.search(r'^\d+\s+(\w+)\(.*<([\d.]+)>$', line.strip())
    if m and seg:
        ms = float(m.group(2)) * 1000
        tot[seg][m.group(1)] += ms; cnt[seg][m.group(1)] += 1
        slow.append((ms, seg, line.strip()[:160]))
for s in tot:
    print(s, "; ".join(f"{k}: {cnt[s][k]} calls, {v:.0f} ms total, {v / cnt[s][k]:.3f} ms each" for k, v in tot[s].most_common(5)))
slow.sort(reverse=True)
for ms, s, l in slow[:12]:
    print(f"{ms:8.2f} ms  {s:13} {l}")
