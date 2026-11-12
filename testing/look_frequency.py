#!/usr/bin/env python3
"""How often does the shipping 5.4.0 root look actually fire?

Drives the real binary over a spread of middlegame positions and counts:
  - positions where the look printed a verdict at all
  - positions where it printed "-> played" (it changed the move)
This is the honest frequency baseline any 'brilliant in N' feature has to beat:
if the immediate look already fires almost never, the horizon look will fire
even less often, and that is the design constraint.
"""
import subprocess
import sys

ROOT = "/root/aggroclone/AggroChess-brilliant"
BIN = ROOT + "/target/release/aggro_chess"

FENS = [
    "r1bq1rk1/pppn1ppp/4p3/3pP3/1b1P4/2NB1N2/PPP2PPP/R1BQK2R w KQ - 0 8",
    "r1bq1rk1/pp2bppp/2n1pn2/2pp4/2PP4/1PN1PN2/PB2BPPP/R2QK2R w KQ - 0 8",
    "r2q1rk1/pb1nbppp/1p2pn2/2pp4/2PP4/1PN1PN2/PB2BPPP/R2Q1RK1 w - - 0 1",
    "rnbqkb1r/pp3ppp/4pn2/2pp4/3P4/2PBPN2/PP3PPP/RNBQK2R w KQkq - 0 1",
    "r3k2r/p1ppqpb1/bn2pnp1/3PN3/1p2P3/2N2Q1p/PPPBBPPP/R3K2R w KQkq - 0 1",
    "r4rk1/1pp1qppp/p1np1n2/2b1p1B1/2B1P3/2NP1N2/PPP1QPPP/R4RK1 w - - 0 10",
    "2r2rk1/1bqnbppp/p2ppn2/1p6/3NPP2/1BN1B3/PPQ2PPP/2RR2K1 w - - 0 1",
    "rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1",
    "1r4k1/5ppp/8/8/8/8/5PPP/1R4K1 w - - 0 1",
    "2r3k1/5ppp/8/8/8/8/5PPP/1R4K1 w - - 0 1",
    "6k1/5ppp/8/8/8/8/5PPP/3R2K1 w - - 0 1",
    "r1b2rk1/2q1bppp/p2ppn2/1p6/3BPP2/2NB4/PPQ2PPP/2RR2K1 w - - 0 1",
    "2kr3r/pp1n1ppp/2pb1n2/q3p3/4P3/1BNP1N2/PPP1QPPP/R1B2RK1 w - - 0 1",
    "r2qkb1r/1b3ppp/p1nppn2/1p6/3NP3/1BN1B3/PPPQ1PPP/2KR3R w kq - 0 1",
    "5rk1/1pp2ppp/p1npbn2/2q1p3/2B1P3/2NP1N2/PPP2PPP/R2Q1RK1 w - - 0 1",
    "2rqr1k1/1p1nbppp/p2p1n2/3P4/1p2P3/2N1B3/PPQNBPPP/R4RK1 w - - 0 1",
]


def play(fen, movetime, opts):
    p = subprocess.Popen([BIN], stdin=subprocess.PIPE, stdout=subprocess.PIPE,
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
    s("go movetime %d" % movetime)
    look, best = [], None
    while True:
        line = p.stdout.readline()
        if not line:
            break
        line = line.strip()
        if line.startswith("info string brilliancy look"):
            look.append(line)
        if line.startswith("bestmove"):
            best = line.split()[1]
            break
    s("quit")
    try:
        p.wait(1)
    except Exception:
        p.kill()
    return look, best


def main():
    mt = int(sys.argv[1]) if len(sys.argv) > 1 else 300
    opts = [("Threads", 1)]
    verdicts = played = 0
    for fen in FENS:
        look, best = play(fen, mt, opts)
        v = len(look)
        pl = sum(1 for l in look if "-> played" in l)
        verdicts += v
        played += pl
        print("%-3s %-3s %s  %s" % (v, pl, best, fen))
        for l in look:
            print("      " + l)
    n = len(FENS)
    print("\n%d positions at %d ms: %d look verdicts, %d played (%.0f%% of positions)"
          % (n, mt, verdicts, played, 100.0 * played / n))


if __name__ == "__main__":
    main()
