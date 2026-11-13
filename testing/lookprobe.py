#!/usr/bin/env python3
"""lookprobe.py - measure the brilliancy look's decisions directly, not through game noise.

For each position in a suite (FEN + side to move), run the engine at the rig settings and
parse the `brilliancy look:` verdict line. The look prints exactly one line per search where
it fired, with its two neutral scores and whether it took the sacrifice.

This is deterministic (same FEN + same options + same movetime -> same verdict almost
always), so it measures the look's *decision rate* with a fraction of the games' variance.
It complements the game-level brilliant count: the look is the only thing that can turn a
sacrifice into a chosen move on purpose.

Usage:
  python3 testing/lookprobe.py --fen-file testing/positions/sacs.epd [--opts "x_deepms=0"]
"""
import argparse
import os
import re
import subprocess
import sys

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
BIN = os.path.join(ROOT, "target", "release", "aggro_chess")
RIG = [("Threads", 1), ("Hash", 64), ("Skill Level", 21), ("Brilliance", 100),
       ("Sacrifice Appetite", 95), ("Aggression", 90), ("King Attack Priority", 90),
       ("Material Preservation", 15), ("Speculative Sacrifices", 90),
       ("Human Style", 70), ("Human Think Time", 0), ("Move Overhead", 50)]

VERDICT = re.compile(r"brilliancy look: (.*)")


def probe(fen, movetime, opts):
    p = subprocess.Popen([BIN], stdin=subprocess.PIPE, stdout=subprocess.PIPE,
                         stderr=subprocess.DEVNULL, text=True, bufsize=0, cwd=ROOT,
                         env={**os.environ, "AGGRO_DEV": "1"})
    def s(x):
        p.stdin.write(x + "\n"); p.stdin.flush()
    s("uci")
    while p.stdout.readline().strip() != "uciok":
        pass
    for k, v in RIG + opts:
        s(f"setoption name {k} value {v}")
    s("isready")
    while p.stdout.readline().strip() != "readyok":
        pass
    s("ucinewgame")
    s("position fen " + fen)
    s(f"go movetime {movetime}")
    lines, best = [], None
    while True:
        line = p.stdout.readline()
        if not line:
            break
        line = line.strip()
        if "brilliancy look:" in line:
            lines.append(line)
        if line.startswith("bestmove"):
            best = line.split()[1]
            break
    s("quit")
    try:
        p.wait(2)
    except Exception:
        p.kill()
    return lines, best


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--fen-file", required=True)
    ap.add_argument("--opts", default="")
    ap.add_argument("--movetime", type=int, default=300)
    ap.add_argument("--limit", type=int, default=0)
    ap.add_argument("--verbose", action="store_true")
    ap.add_argument("--jobs", type=int, default=1, help="parallel engine processes")
    a = ap.parse_args()
    opts = []
    for part in (a.opts or "").split(";"):
        if "=" in part:
            k, v = part.split("=", 1)
            opts.append((k.strip(), v.strip()))

    fens = []
    for line in open(a.fen_file):
        line = line.strip()
        if not line or line.startswith("#"):
            continue
        fens.append(line)
    if a.limit:
        fens = fens[:a.limit]

    fired = took = vetoed = 0
    takes = []

    def one(pair):
        i, fen = pair
        return i, fen, probe(fen, a.movetime, opts)

    if a.jobs > 1:
        from concurrent.futures import ThreadPoolExecutor
        with ThreadPoolExecutor(max_workers=a.jobs) as ex:
            results = list(ex.map(one, enumerate(fens)))
    else:
        results = [one((i, f)) for i, f in enumerate(fens)]

    played_sac = promo = demote = 0
    for i, fen, (lines, best) in results:
        if lines:
            fired += 1
            v = lines[0].split("brilliancy look: ", 1)[1]
            # label1 == "safe"  -> the main search had ALREADY chosen a sacrifice, and the
            #                      look is checking a safe alternative against it
            # label1 == "sacrifice" -> the main search chose a quiet move; the look checks
            #                      whether a sacrifice is worth playing instead
            # "-> played" means the look CHANGED the move.
            best_is_sac = v.startswith("safe")
            changed = "-> played" in v
            if best_is_sac:
                # sacrifice is played unless the look swapped in the safe move
                if changed:
                    demote += 1
                else:
                    played_sac += 1
            else:
                # a quiet move is played unless the look swapped in the sacrifice
                if changed:
                    promo += 1
                    played_sac += 1
                    takes.append((i, best, v))
        if a.verbose:
            print(f"[{i}] {fen[:40]}  best={best}  {' | '.join(l.split('info string ')[-1] for l in lines) or '(no look)'}")
    print(f"{os.path.basename(a.fen_file)}: positions={len(fens)} look_fired={fired} ({100*fired/max(1,len(fens)):.0f}%) "
          f"sacrifice_played={played_sac} ({100*played_sac/max(1,len(fens)):.0f}%) "
          f"look_promoted={promo} look_demoted={demote} opts='{a.opts}'")
    for i, b, v in takes[:10]:
        print(f"   [{i}] played {b}: {v}")


if __name__ == "__main__":
    main()
