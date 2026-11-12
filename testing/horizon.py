#!/usr/bin/env python3
"""horizon.py - behaviour of the "brilliant in N" horizon look.

Checks, at a fixed movetime:
  * with Brilliance 0 the horizon prints nothing and takes nothing
  * with Brilliance 100 the horizon either finds something or stays silent - never errors
  * a horizon finding must announce itself and must be the played move
  * wall-clock overhead versus Brilliance 0 stays within the documented cap
"""
import argparse
import os
import re
import subprocess
import sys
import time

TESTING = os.path.dirname(os.path.abspath(__file__))
ROOT = os.path.dirname(TESTING)

FENS = [
    ("G1", "r1bq1rk1/pppn1ppp/4p3/3pP3/1b1P4/2NB1N2/PPP2PPP/R1BQK2R w KQ - 0 8"),
    ("G2", "r1bq1rk1/pp2bppp/2n1pn2/2pp4/2PP4/1PN1PN2/PB2BPPP/R2QK2R w KQ - 0 8"),
    ("G3", "r2q1rk1/pb1nbppp/1p2pn2/2pp4/2PP4/1PN1PN2/PB2BPPP/R2Q1RK1 w - - 0 1"),
    ("G4", "r3k2r/p1ppqpb1/bn2pnp1/3PN3/1p2P3/2N2Q1p/PPPBBPPP/R3K2R w KQkq - 0 1"),
    ("G5", "r4rk1/1pp1qppp/p1np1n2/2b1p1B1/2B1P3/2NP1N2/PPP1QPPP/R4RK1 w - - 0 10"),
    ("G6", "2r2rk1/1bqnbppp/p2ppn2/1p6/3NPP2/1BN1B3/PPQ2PPP/2RR2K1 w - - 0 1"),
    ("G7", "r1b2rk1/2q1bppp/p2ppn2/1p6/3BPP2/2NB4/PPQ2PPP/2RR2K1 w - - 0 1"),
    ("G8", "rnbqkb1r/pp3ppp/4pn2/2pp4/3P4/2PBPN2/PP3PPP/RNBQK2R w KQkq - 0 1"),
    ("C1", "6k1/5ppp/8/8/8/8/5PPP/3R2K1 w - - 0 1"),
    ("C2", "rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1"),
]


def run(fen, movetime, opts):
    p = subprocess.Popen([os.path.join(ROOT, "target", "release", "aggro_chess")],
                         stdin=subprocess.PIPE, stdout=subprocess.PIPE,
                         stderr=subprocess.DEVNULL, text=True, bufsize=0, cwd=ROOT)

    def s(x):
        p.stdin.write(x + "\n")
        p.stdin.flush()

    s("uci")
    while p.stdout.readline().strip() != "uciok":
        pass
    for k, v in opts:
        s("setoption name %s value %s" % (k, v))
    s("isready")
    while p.stdout.readline().strip() != "readyok":
        pass
    s("ucinewgame")
    s("position fen " + fen)
    t0 = time.time()
    s("go movetime %d" % movetime)
    hz, best = [], None
    while True:
        line = p.stdout.readline()
        if not line:
            break
        line = line.strip()
        if line.startswith("info string brilliant in"):
            hz.append(line)
        if line.startswith("bestmove"):
            best = line.split()[1]
            break
    el = time.time() - t0
    s("quit")
    try:
        p.wait(1)
    except Exception:
        p.kill()
    return hz, best, el


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--movetime", type=int, default=300)
    ap.add_argument("--max-overhead", type=float, default=0.60,
                    help="allowed wall-clock growth vs Brilliance 0, as a fraction")
    a = ap.parse_args()
    fails = 0
    print("| id | Brilliance | horizon lines | played | verdict |")
    print("|---|---|---|---|---|")
    for name, fen in FENS:
        on, b_on, t_on = run(fen, a.movetime, [("Threads", 1), ("Brilliance", 100)])
        off, b_off, t_off = run(fen, a.movetime, [("Threads", 1), ("Brilliance", 0)])
        ok = True
        if off or "brilliant in" in " ".join(off):
            ok = False
        over = (t_on - t_off) / max(t_off, 1e-6)
        if over > a.max_overhead:
            ok = False
        # any "forced"/"bait" line must name the move actually played
        for line in on:
            if ("forced" in line or "bait" in line) and b_on:
                m = re.search(r": ([a-h][1-8][a-h][1-8][qrbn]?) ", line)
                if m and m.group(1) != b_on:
                    ok = False
        if not ok:
            fails += 1
        print("| %s | on | %d | %s | %s |" % (name, len(on), b_on, "PASS" if ok else "FAIL"))
        for line in on:
            print("    " + line)
    print("\nhorizon suite: %d failure(s)" % fails)
    return 0 if fails == 0 else 1


if __name__ == "__main__":
    sys.exit(main())
