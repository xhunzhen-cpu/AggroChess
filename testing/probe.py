#!/usr/bin/env python3
"""probe.py - run one position N times, print the brilliancy verdict and the move each time.

`go movetime` is not reproducible, so a position near the take threshold can flip. This
reports what the look actually decided on each run, which is what explains a flaky gate.

Usage:
  AGGRO_DEV=1 python3 -I testing/probe.py <fen> <N> [movetime] [k=v;k=v]
"""
import os
import subprocess
import sys
from concurrent.futures import ThreadPoolExecutor

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
BIN = os.path.join(ROOT, "target", "release", "aggro_chess")


def once(fen, cfg, movetime):
    p = subprocess.Popen([BIN], stdin=subprocess.PIPE, stdout=subprocess.PIPE,
                         stderr=subprocess.DEVNULL, text=True, bufsize=0, cwd=ROOT,
                         env={**os.environ, "AGGRO_DEV": "1"})

    def s(x):
        p.stdin.write(x + "\n")
        p.stdin.flush()

    look, best = [], None
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
            if "brilliancy look" in l:
                look.append(l.strip())
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
    return best, " | ".join(look)


def main():
    fen, n = sys.argv[1], int(sys.argv[2])
    movetime = int(sys.argv[3]) if len(sys.argv) > 3 else 2000
    cfg = sys.argv[4] if len(sys.argv) > 4 else ""
    with ThreadPoolExecutor(max_workers=4) as ex:
        res = list(ex.map(lambda _: once(fen, cfg, movetime), range(n)))
    for best, look in res:
        print(f"{best:8} {look}")


main()
