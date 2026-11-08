#!/usr/bin/env python3
"""Turns a Loghub log into one JSON record a line, shaped like an
OpenTelemetry log record: ts (microseconds since 1970, UTC), host,
service, severity, pid (or null) and body. Lines the format does not
match are kept whole as the body, with the previous line's time.

    parse.py FORMAT IN.log OUT.jsonl     FORMAT: hdfs, bgl, android, ssh
"""
import calendar
import json
import re
import sys

MONTHS = {m: i for i, m in enumerate(calendar.month_abbr) if m}


def micros(y, mo, d, h, mi, s, us=0):
    return (calendar.timegm((y, mo, d, h, mi, s)) * 1_000_000) + us


# 081109 203518 143 INFO dfs.DataNode$DataXceiver: Receiving block ...
HDFS = re.compile(r"^(\d{2})(\d{2})(\d{2}) (\d{2})(\d{2})(\d{2}) (\d+) (\w+) ([^:]+): (.*)$")
# - 1117838570 2005.06.03 R02-M1-N0-C:J12-U11 2005-06-03-15.42.50.363779 R02-M1-N0-C:J12-U11 RAS KERNEL INFO body
BGL = re.compile(r"^(\S+) (\d+) \S+ (\S+) (\d{4})-(\d{2})-(\d{2})-(\d{2})\.(\d{2})\.(\d{2})\.(\d{6}) \S+ (\S+) (\S+) (\S+) ?(.*)$")
# 12-17 19:31:36.263  1795  1825 I PowerManager_screenOn: body
ANDROID = re.compile(r"^(\d{2})-(\d{2}) (\d{2}):(\d{2}):(\d{2})\.(\d{3})\s+(\d+)\s+(\d+) (\w) ([^:]*?)\s*: (.*)$")
# Dec 10 06:55:46 LabSZ sshd[24200]: body
SSH = re.compile(r"^(\w{3})\s+(\d+) (\d{2}):(\d{2}):(\d{2}) (\S+) ([^\[:]+)(?:\[(\d+)\])?: (.*)$")
ANDROID_LEVELS = {"V": "TRACE", "D": "DEBUG", "I": "INFO", "W": "WARN", "E": "ERROR", "F": "FATAL", "A": "FATAL"}


def parse(fmt, line, last):
    if fmt == "hdfs":
        m = HDFS.match(line)
        if m:
            g = m.groups()
            ts = micros(2000 + int(g[0]), int(g[1]), int(g[2]), int(g[3]), int(g[4]), int(g[5]))
            return dict(ts=ts, host="hdfs", service=g[8], severity=g[7], pid=int(g[6]), body=g[9])
    elif fmt == "bgl":
        m = BGL.match(line)
        if m:
            g = m.groups()
            ts = micros(int(g[3]), int(g[4]), int(g[5]), int(g[6]), int(g[7]), int(g[8]), int(g[9]))
            return dict(ts=ts, host=g[2], service=f"{g[10]} {g[11]}", severity=g[12], pid=None, body=g[13])
    elif fmt == "android":
        m = ANDROID.match(line)
        if m:
            g = m.groups()
            ts = micros(2017, int(g[0]), int(g[1]), int(g[2]), int(g[3]), int(g[4]), int(g[5]) * 1000)
            return dict(ts=ts, host="phone", service=g[9], severity=ANDROID_LEVELS.get(g[8], g[8]), pid=int(g[6]), body=g[10])
    elif fmt == "ssh":
        m = SSH.match(line)
        if m:
            g = m.groups()
            ts = micros(2017, MONTHS[g[0]], int(g[1]), int(g[2]), int(g[3]), int(g[4]))
            return dict(ts=ts, host=g[5], service=g[6], severity="INFO", pid=int(g[7]) if g[7] else None, body=g[8])
    return dict(ts=last, host="", service="", severity="", pid=None, body=line)


def main():
    fmt, src, dst = sys.argv[1:4]
    n = bad = 0
    last = 0
    with open(src, encoding="utf-8", errors="replace") as f, open(dst, "w") as out:
        for line in f:
            line = line.rstrip("\n").rstrip("\r")
            r = parse(fmt, line, last)
            if r["service"] == "" and r["host"] == "":
                bad += 1
            last = r["ts"]
            out.write(json.dumps(r, separators=(",", ":"), ensure_ascii=False))
            out.write("\n")
            n += 1
    print(f"{fmt}: {n} lines, {bad} kept whole", file=sys.stderr)


if __name__ == "__main__":
    main()
