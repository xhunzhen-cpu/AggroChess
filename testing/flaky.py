#!/usr/bin/env python3
"""How often does an am (avoid-move) position break, under two look configs?

`go movetime` is not reproducible (see the look-is-not-deterministic note), and `go nodes`
turns the look off entirely (the budget block is skipped when the GUI asked for nodes), so a
position that sits near a take threshold can flip between runs. This runs one position N times
per config and reports the break rate, which is the only honest way to compare configs on it.

Usage:
  python3 -I testing/flaky.py <fen> <banned-move> <N> [config|config...]
    config = "k=v;k=v" (x_* dev options), or "" for the build's defaults
"""
import os
import subprocess
import sys
from concurrent.futures import ThreadPoolExecutor

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
BIN = os.path.join(ROOT, "target", "release", "aggro_chess")


def once(fen, banned, cfg, movetime):
    p = subprocess.Popen([BIN], stdin=subprocess.PIPE, stdout=subprocess.PIPE,
                         stderr=subprocess.DEVNULL, text=True, bufsize=0, cwd=ROOT,
                         env={**os.environ, "AGGRO_DEV": "1"})

    def s(x):
        p.stdin.write(x + "\n")
        p.stdin.flush()

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
        best = None
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
    return best


def main():
    fen, banned = sys.argv[1], sys.argv[2]
    n = int(sys.argv[3])
    cfgs = sys.argv[4:] or [""]
    movetime = int(os.environ.get("MT", "2000"))
    for cfg in cfgs:
        with ThreadPoolExecutor(max_workers=4) as ex:
            moves = list(ex.map(lambda _: once(fen, banned, cfg, movetime), range(n)))
        bad = sum(1 for m in moves if m == banned)
        from collections import Counter
        top = Counter(moves).most_common(4)
        print(f"cfg={cfg or '(defaults)'}: broke {bad}/{n}   moves={top}")


main()
