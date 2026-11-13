#!/usr/bin/env python3
"""amflaky.py - break rate of an avoid-move set, by repetition.

`go movetime` is not reproducible, so a position near a take threshold can flip between runs.
That makes a single pass of an am gate a coin flip on its worst entries. This runs every am
position REP times and reports, per position, how often the engine played a banned move —
that is the number a gate should be judged on, and the number to compare two configs with.

Usage:
  python3 -I testing/amflaky.py <epd> <reps> [--jobs 8] [--movetime 2000] [--cmp "a|b"]
    --cmp compares two configs (x_* dev options) instead of one default run.
"""
import argparse
import os
import re
import subprocess
import sys
from collections import Counter
from concurrent.futures import ThreadPoolExecutor

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
BIN = os.path.join(ROOT, "target", "release", "aggro_chess")


def parse(path):
    out = []
    for line in open(path):
        line = line.strip()
        if not line or line.startswith("#"):
            continue
        m = re.search(r"(?:^|\s)am\s+([^;]+);", line)
        if not m:
            continue
        ident = re.search(r'\bid\s+"([^"]*)"', line)
        head = re.split(r"\s(?:bm|am|id|c0|c1|c2|c3)\b", line)[0].strip().split()
        if len(head) < 4:
            continue
        out.append((" ".join(head[:4]) + " 0 1", m.group(1).split(),
                    ident.group(1) if ident else head[0]))
    return out


def once(job):
    fen, banned, ident, cfg, movetime = job
    p = subprocess.Popen([BIN], stdin=subprocess.PIPE, stdout=subprocess.PIPE,
                         stderr=subprocess.DEVNULL, text=True, bufsize=0, cwd=ROOT,
                         env={**os.environ, "AGGRO_DEV": "1"})

    def s(x):
        p.stdin.write(x + "\n")
        p.stdin.flush()

    best = None
    try:
        s("uci")
        while p.stdout.readline().strip() != "uciok":
            pass
        for kv in filter(None, cfg.split(";")):
            k, _, v = kv.partition("=")
            s(f"setoption name {k.strip()} value {v.strip()}")
        s("isready")
        while p.stdout.readline().strip() != "readyok":
            pass
        s("ucinewgame")
        s("position fen " + fen)
        s(f"go movetime {movetime}")
        while True:
            l = p.stdout.readline()
            if not l:
                break
            if l.startswith("bestmove"):
                best = l.split()[1]
                break
        s("quit")
        try:
            p.wait(2)
        except Exception:
            p.kill()
    finally:
        if p.poll() is None:
            p.kill()
    return ident, best in banned


def run(positions, reps, cfg, jobs, movetime):
    jobslist = [(fen, banned, ident, cfg, movetime)
                for fen, banned, ident in positions for _ in range(reps)]
    with ThreadPoolExecutor(max_workers=jobs) as ex:
        res = list(ex.map(once, jobslist))
    tally = {}
    for ident, broke in res:
        t = tally.setdefault(ident, [0, 0])
        t[0] += broke
        t[1] += 1
    return tally


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("epd")
    ap.add_argument("reps", type=int)
    ap.add_argument("--jobs", type=int, default=8)
    ap.add_argument("--movetime", type=int, default=2000)
    ap.add_argument("--cmp", default=None, help="'a|b' two x_* configs")
    a = ap.parse_args()
    positions = parse(a.epd)
    cfgs = a.cmp.split("|") if a.cmp else [""]
    results = []
    for cfg in cfgs:
        t = run(positions, a.reps, cfg, a.jobs, a.movetime)
        total_b = sum(v[0] for v in t.values())
        total_n = sum(v[1] for v in t.values())
        results.append((cfg, t))
        print(f"cfg={cfg or '(defaults)'}: {total_b}/{total_n} broken "
              f"({100.0 * total_b / max(1, total_n):.1f}%)")
    if len(results) == 2:
        (ca, ta), (cb, tb) = results
        print(f"\n{'id':<8} {ca or 'defaults':>22} {cb or 'defaults':>22}")
        for ident in sorted(ta, key=lambda k: -(ta[k][0] + tb[k][0])):
            if ta[ident][0] or tb[ident][0]:
                x, y = ta[ident], tb[ident]
                print(f"{ident:<8} {x[0]:>10}/{x[1]:<10} {y[0]:>10}/{y[1]:<10}")


main()
