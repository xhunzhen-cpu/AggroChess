#!/usr/bin/env python3
"""sacprobe.py - per-position brilliant instrument.

For each FEN: run the engine at the rig settings, take its bestmove, and ask a stronger
oracle (Stockfish) two questions the arena analyzer asks of whole games:

  1. does the move give material away (net material lost by the mover)?
  2. is the resulting position still good (sound)?

A move counts as a *brilliant opportunity taken* when the mover gives up a queen /
exchange / piece (>= 320 cp of material, not a pawn) in a position that was not already
won and does not become lost. That is the strict chess.com-ish predicate, per position,
without game noise.

This is the instrument to A/B engine options against: it answers "on how many of these
positions does the engine play a sound material sacrifice?" directly.

Usage:
  python3 testing/sacprobe.py --fen-file testing/positions/sacs.epd --jobs 6
  python3 testing/sacprobe.py --opts "x_sacbonus=400" --jobs 6
"""
import argparse
import os
import re
import subprocess
import sys
from concurrent.futures import ThreadPoolExecutor

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
BIN = os.path.join(ROOT, "target", "release", "aggro_chess")
SF = "/usr/games/stockfish"
RIG = [("Threads", 1), ("Hash", 64), ("Skill Level", 21), ("Brilliance", 100),
       ("Sacrifice Appetite", 95), ("Aggression", 90), ("King Attack Priority", 90),
       ("Material Preservation", 15), ("Speculative Sacrifices", 90),
       ("Human Style", 70), ("Human Think Time", 0), ("Move Overhead", 50)]

VAL = {"P": 100, "N": 320, "B": 330, "R": 500, "Q": 900, "K": 0}
WIN_BEFORE = 300     # already winning -> the sac is not brave
MIN_AFTER = -150     # do not leave a lost position
MAX_DROP = 50        # sound: the eval may fall at most this much
MIN_GIVE = 320       # a pawn is not a sacrifice
MATE_WIN = 20000     # score above this is a forced mate: material is irrelevant


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

    def sync(self):
        """isready barrier: stockfish answers every earlier command before readyok."""
        self.send("isready")
        while self.read() != "readyok":
            pass

    def read(self):
        return self.p.stdout.readline().strip()

    def eval_after(self, fen, moves, nodes, negate):
        """Score in cp from the original mover's perspective.

        With `moves` empty the side to move IS the mover, so the raw score is already theirs;
        after a move the side to move is the opponent, so the raw score is negated.
        """
        self.sync()
        self.send("ucinewgame")
        if moves:
            self.send(f"position fen {fen} moves {moves}")
        else:
            self.send(f"position fen {fen}")
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
                # mate in N for the side to move: +mate; against: -mate
                sc = 30000 - m if m > 0 else -30000 - m
        if sc is None:
            return None
        return -sc if negate else sc






def engine_move(fen, opts, movetime):
    p = subprocess.Popen([BIN], stdin=subprocess.PIPE, stdout=subprocess.PIPE,
                         stderr=subprocess.DEVNULL, text=True, bufsize=0, cwd=ROOT,
                         env={**os.environ, "AGGRO_DEV": "1"})

    def s(x):
        p.stdin.write(x + "\n")
        p.stdin.flush()

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


def after_fen(sf, fen, uci):
    sf.sync()
    sf.send(f"position fen {fen} moves {uci}")
    sf.send("d")
    for _ in range(400):
        l = sf.read()
        if l.startswith("Fen: "):
            return l[5:].strip()
        if l.startswith("Checkers"):
            return None
    return None


def side_material(fen, side):
    """Sum of piece values for `side` from a FEN."""
    rows = fen.split()[0].split("/")
    total = 0
    for rank in rows:
        for ch in rank:
            if ch.isalpha():
                v = VAL.get(ch.upper(), 0)
                if (ch.isupper() and side == "w") or (ch.islower() and side == "b"):
                    total += v
    return total


def fen_side(fen):
    return fen.split()[1]


def one(job, sf_factory):
    i, fen, opts, movetime, nodes = job
    mv = engine_move(fen, opts, movetime)
    if not mv or mv == "(none)":
        return dict(i=i, fen=fen, mv=mv, verdict="nomove")
    sf = sf_factory()
    af = after_fen(sf, fen, mv)
    if not af:
        return dict(i=i, fen=fen, mv=mv, verdict="noafter")
    mover = fen_side(fen)
    before = side_material(fen, mover)
    after = side_material(af, mover)
    given = before - after
    sb = sf.eval_after(fen, "", nodes, negate=False)
    sa = sf.eval_after(fen, mv, nodes, negate=True)
    d = dict(i=i, fen=fen, mv=mv, given=given, sf_before=sb, sf_after=sa)
    if given < MIN_GIVE:
        d["verdict"] = "quiet"
    elif sa is not None and sa >= MATE_WIN:
        d["verdict"] = "mating"
    elif sb is not None and sb >= WIN_BEFORE:
        d["verdict"] = "already-winning"
    elif sa is not None and sa <= MIN_AFTER:
        d["verdict"] = "left-losing"
    elif sb is not None and sa is not None and sa - sb < -MAX_DROP:
        d["verdict"] = "unsound"
    else:
        d["verdict"] = "BRILLIANT"
    return d


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--fen-file", required=True)
    ap.add_argument("--opts", default="")
    ap.add_argument("--movetime", type=int, default=300)
    ap.add_argument("--jobs", type=int, default=6)
    ap.add_argument("--nodes", type=int, default=120000, help="SF nodes for the judgement")
    ap.add_argument("--limit", type=int, default=0)
    ap.add_argument("--verbose", action="store_true")
    a = ap.parse_args()
    opts = []
    for part in (a.opts or "").split(";"):
        if "=" in part:
            k, v = part.split("=", 1)
            opts.append((k.strip(), v.strip()))

    fens = [l.strip() for l in open(a.fen_file) if l.strip() and not l.startswith("#")]
    if a.limit:
        fens = fens[:a.limit]
    jobs = [(i, f, opts, a.movetime, a.nodes) for i, f in enumerate(fens)]

    # one Stockfish per worker thread
    import threading
    local = threading.local()

    def factory():
        if not hasattr(local, "sf"):
            local.sf = Sf()
        return local.sf

    with ThreadPoolExecutor(max_workers=a.jobs) as ex:
        res = list(ex.map(lambda j: one(j, factory), jobs))

    import collections
    c = collections.Counter(r["verdict"] for r in res)
    n = len(res)
    big = [r for r in res if r.get("given", 0) >= MIN_GIVE]
    sfj = [r for r in big if r.get("sf_before") is not None]
    print(f"{os.path.basename(a.fen_file)}: n={n} look/sac moves={len(big)} "
          f"BRILLIANT={c['BRILLIANT']} ({100*c['BRILLIANT']/max(1,n):.0f}%) "
          f"already-winning={c['already-winning']} left-losing={c['left-losing']} "
          f"unsound={c['unsound']} quiet={c['quiet']} opts='{a.opts}'")
    if a.verbose:
        for r in res:
            if r["verdict"] == "BRILLIANT":
                print(f"   [{r['i']}] {r['mv']} given={r['given']} "
                      f"sf {r['sf_before']}->{r['sf_after']}")


if __name__ == "__main__":
    main()
