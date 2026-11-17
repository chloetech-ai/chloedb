import sys, re, collections
seg = None; counts = collections.defaultdict(collections.Counter); files = collections.defaultdict(collections.Counter)
fdmap = {}
for line in open(sys.argv[1]):
    m = re.search(r'openat\(.*?"([^"]+)".*\) = (\d+)', line)
    if m:
        fdmap[(line.split()[0], m.group(2))] = m.group(1); fdmap[m.group(2)] = m.group(1)
        continue
    m = re.search(r'write\(\d+, "=== (.+?) (start|end)', line)
    if m:
        seg = m.group(1) if m.group(2) == "start" else None; continue
    m = re.search(r'\b(fsync|fdatasync|msync|sync_file_range|syncfs)\((\w+)', line)
    if m and seg:
        counts[seg][m.group(1)] += 1
        files[seg][fdmap.get(m.group(2), m.group(2)).rsplit('/', 1)[-1]] += 1
n = int(sys.argv[2])
for s in counts:
    print(f"{s:14} per op: " + ", ".join(f"{k} {v/n:.2f}" for k, v in counts[s].items()) + "   files: " + ", ".join(f"{k} {v/n:.2f}" for k, v in files[s].most_common(6)))
