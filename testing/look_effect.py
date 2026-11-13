#!/usr/bin/env python3
"""look_effect.py - does the brilliancy look improve the position, by Stockfish's own measure?

For each FEN, ask the engine for its move at Brilliance 0 (personality only, no look) and at
Brilliance 100 (look active). When the two differ, evaluate both resulting positions with
Stockfish from the *original mover's* perspective and compare.

Scores are handled carefully:
  * Stockfish reports from the side to move; after a move that is the opponent, so the score is
    negated to get the mover's view.
  * Mate scores are clamped and reported separately: they must never be averaged with cp scores.

The look is only judged where it CHANGED the move. A positive mean delta means the look is
improving the position; a negative one means it costs material evaluation.

Usage:
  python3 testing/look_effect.py --fen-file testing/positions/sacs.epd --limit 80 --jobs 8
"""
import argparse
import os
import subprocess
import sys
from concurrent.futures import ThreadPoolExecutor

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
BIN = os.path.join(ROOT, "target", "release", "aggro_chess")
SF = "/usr/games/stockfish"
RIG = [("Threads", 1), ("Hash", 64), ("Skill Level", 21),
       ("Sacrifice Appetite", 95), ("Aggression", 90), ("King Attack Priority", 90),
       ("Material Preservation", 15), ("Speculative Sacrifices", 90),
       ("Human Style", 70), ("Human Think Time", 0), ("Move Overhead", 50)]
MATE_CAP = 20000


def engine_move(fen, brill, movetime, extra=()):
    p = subprocess.Popen([BIN], stdin=subprocess.PIPE, stdout=subprocess.PIPE,
                         stderr=subprocess.DEVNULL, text=True, bufsize=0, cwd=ROOT,
                         env={**os.environ, "AGGRO_DEV": "1"})

    def s(x):
        p.stdin.write(x + "\n")
        p.stdin.flush()

    s("uci")
    while p.stdout.readline().strip() != "uciok":
        pass
    for k, v in RIG:
        s(f"setoption name {k} value {v}")
    s(f"setoption name Brilliance value {brill}")
    for k, v in extra:
        s(f"setoption name {k} value {v}")
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
    return best


class Sf:
    def __init__(self):
        self.p = subprocess.Popen([SF], stdin=subprocess.PIPE, stdout=subprocess.PIPE,
                                  stderr=subprocess.DEVNULL, text=True, bufsize=0)
        self.send("uci")
        while self.read() != "uciok":
            pass
        self.send("setoption name Threads value 2")
        self.send("setoption name Hash value 64")
        self.send("isready")
        while self.read() != "readyok":
            pass

    def send(self, x):
        self.p.stdin.write(x + "\n")
        self.p.stdin.flush()

    def read(self):
        return self.p.stdout.readline().strip()

    def score(self, fen, moves, nodes):
        self.send("ucinewgame")
        self.send(f"position fen {fen}" + (f" moves {moves}" if moves else ""))
        self.send(f"go nodes {nodes}")
        sc = None
        while True:
            l = self.read()
            if l.startswith("bestmove"):
                break
            if "score cp" in l:
                p = l.split()
                sc = int(p[p.index("cp") + 1])
            elif "score mate" in l:
                p = l.split()
                m = int(p[p.index("mate") + 1])
                sc = MATE_CAP if m > 0 else -MATE_CAP
        if sc is None:
            return None
        # side to move after `moves` is the opponent when a move was given
        return -sc if moves else sc


def one(job, local, extra_a=(), extra_b=None):
    i, fen, movetime, nodes = job
    if not hasattr(local, "sf"):
        local.sf = Sf()
    sf = local.sf
    # default comparison: look off vs look on. --vs-opts swaps the "before" side for a second
    # look configuration, so a threshold change can be judged the same way (old rule vs new).
    if extra_b is None:
        b0 = engine_move(fen, 0, movetime)
        b100 = engine_move(fen, 100, movetime)
    else:
        b0 = engine_move(fen, 100, movetime, extra_a)
        b100 = engine_move(fen, 100, movetime, extra_b)
    if not b0 or not b100:
        return None
    if b0 == b100:
        return dict(i=i, fen=fen, changed=False)
    before = sf.score(fen, "", nodes)
    e0 = sf.score(fen, b0, nodes)
    e100 = sf.score(fen, b100, nodes)
    return dict(i=i, fen=fen, changed=True, b0=b0, b100=b100,
                before=before, e0=e0, e100=e100)


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--fen-file", required=True)
    ap.add_argument("--movetime", type=int, default=300)
    ap.add_argument("--jobs", type=int, default=8)
    ap.add_argument("--nodes", type=int, default=150000)
    ap.add_argument("--limit", type=int, default=0)
    ap.add_argument("--vs-opts", default=None,
                    help="compare two look configs instead of off-vs-on: 'k=v;k=v|k=v;k=v' "
                         "(before|after)")
    a = ap.parse_args()
    fens = [l.strip() for l in open(a.fen_file) if l.strip() and not l.startswith("#")]
    if a.limit:
        fens = fens[:a.limit]

    import threading
    local = threading.local()
    jobs = [(i, f, a.movetime, a.nodes) for i, f in enumerate(fens)]
    extra_a, extra_b = (), None
    if a.vs_opts:
        parts = a.vs_opts.split("|")
        def parse(t):
            out = []
            for p in t.split(";"):
                p = p.strip()
                if p:
                    k, _, v = p.partition("=")
                    out.append((k.strip(), v.strip()))
            return tuple(out)
        extra_a, extra_b = parse(parts[0]), parse(parts[1] if len(parts) > 1 else parts[0])
    with ThreadPoolExecutor(max_workers=a.jobs) as ex:
        res = [r for r in ex.map(lambda j: one(j, local, extra_a, extra_b), jobs) if r]

    changed = [r for r in res if r.get("changed")]
    cp = [r for r in changed if r["e0"] is not None and r["e100"] is not None
          and abs(r["e0"]) < MATE_CAP and abs(r["e100"]) < MATE_CAP]
    print(f"positions={len(res)} look_changed={len(changed)} "
          f"(cp-comparable {len(cp)}, mate-involved {len(changed)-len(cp)})")
    if cp:
        deltas = [r["e100"] - r["e0"] for r in cp]
        mean = sum(deltas) / len(deltas)
        better = sum(1 for d in deltas if d > 30)
        worse = sum(1 for d in deltas if d < -30)
        label_b, label_a = ("after", "before") if a.vs_opts else ("look", "no-look")
        print(f"mean delta ({label_b} - {label_a}, mover's cp) = {mean:+.0f}")
        print(f"look better (>+30) = {better}, worse (<-30) = {worse}, "
              f"within noise = {len(deltas)-better-worse}")
        for r in sorted(cp, key=lambda x: x["e100"] - x["e0"]):
            print(f"  [{r['i']:>3}] {r['b0']:6}->{r['b100']:6}  before={r['before']:>7} "
                  f"nolook={r['e0']:>7} look={r['e100']:>7} d={r['e100']-r['e0']:>+7}")


if __name__ == "__main__":
    main()
