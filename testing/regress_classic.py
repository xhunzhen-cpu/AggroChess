#!/usr/bin/env python3
"""Regression check: with the *Classic* personality preset (Aggression 60, Sacrifice Appetite 75,
King Attack Priority 60, Material Preservation 50, Speculative Sacrifices 50) and Skill Level 21,
the Ultimate Sacrificial Edition must search exactly like AggroChess 4.5.0: same best move, same
score and the same node count at every depth of a fixed-depth, single-threaded search.

Usage: python3 testing/regress_classic.py [--new target/release/aggro_chess]
                                          [--old /path/to/aggro_original] [--depth 12]
Exit status 0 if every position matches.
"""
import argparse
import subprocess
import sys

FENS = [
    "rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1",
    "r1bqkb1r/pppp1ppp/2n2n2/4p2Q/2B1P3/8/PPPP1PPP/RNB1K1NR w KQkq - 4 4",
    "r1bq1rk1/pp2bppp/2n1pn2/3p4/2PP4/2N1PN2/PP1B1PPP/R2QKB1R w KQ - 0 8",
    "r2q1rk1/pb1nbppp/1p2pn2/2pp4/2PP4/1PN1PN2/PB2BPPP/R2Q1RK1 w - - 0 10",
    "r1b2rk1/2q1bppp/p2ppn2/1p6/3BPP2/2NB4/PPPQ2PP/2KR3R w - - 0 13",
    "r1bq1rk1/pppn1ppp/4p3/3pP3/1b1P4/2NB1N2/PPP2PPP/R1BQK2R w KQ - 0 8",
    "r2qkbnr/ppp2ppp/2n1b3/4P3/2Pp4/PQ3N2/1P2PPPP/RNB1KB1R b KQkq - 2 6",
    "5r1R/ppp2kp1/3p4/4npr1/4P3/2P4Q/PPq3PP/4R1K1 w - - 3 23",
    "8/8/3k4/3p4/3P4/3K4/8/8 w - - 0 1",
    "6k1/5ppp/5b2/8/8/3Q4/5PPP/4R1K1 w - - 0 1",
]
CLASSIC = {
    "Aggression": 60,
    "Sacrifice Appetite": 75,
    "King Attack Priority": 60,
    "Material Preservation": 50,
    "Speculative Sacrifices": 50,
    "Skill Level": 21,
}


def run(engine, fen, depth, options):
    cmds = ["uci", "setoption name Threads value 1"]
    cmds += [f"setoption name {k} value {v}" for k, v in options.items()]
    cmds += ["isready", "ucinewgame", f"position fen {fen}", f"go depth {depth}"]
    p = subprocess.Popen([engine], stdin=subprocess.PIPE, stdout=subprocess.PIPE, text=True)
    p.stdin.write("\n".join(cmds) + "\n")
    p.stdin.flush()
    infos, best = [], None
    for line in p.stdout:
        line = line.strip()
        if line.startswith("info depth") and " nodes " in line:
            t = line.split()
            infos.append((int(t[t.index("depth") + 1]), int(t[t.index("nodes") + 1]),
                          " ".join(t[t.index("pv") + 1:]) if "pv" in t else ""))
        if line.startswith("bestmove"):
            best = line.split()[1]
            break
    p.stdin.write("quit\n")
    p.stdin.flush()
    p.wait(timeout=10)
    return infos, best


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--new", default="target/release/aggro_chess")
    ap.add_argument("--old", default="/root/aggroclone/engines/aggro_original")
    ap.add_argument("--depth", type=int, default=12)
    a = ap.parse_args()
    ok = True
    for fen in FENS:
        old = run(a.old, fen, a.depth, {})
        new = run(a.new, fen, a.depth, CLASSIC)
        same = old == new
        ok &= same
        last_o = old[0][-1] if old[0] else None
        last_n = new[0][-1] if new[0] else None
        print(f"{'OK  ' if same else 'DIFF'} {fen}\n     4.5.0: {old[1]} {last_o}\n     new  : {new[1]} {last_n}")
        if not same:
            for d, (o, n) in enumerate(zip(old[0], new[0]), 1):
                if o != n:
                    print(f"     first difference at depth {d}: {o} vs {n}")
                    break
    print("ALL MATCH" if ok else "MISMATCH")
    return 0 if ok else 1


if __name__ == "__main__":
    sys.exit(main())
