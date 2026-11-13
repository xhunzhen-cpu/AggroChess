"""The ceiling. For a sample of real search positions, enumerate every sacrifice the engine
classifies (its own candidate rule) and ask Stockfish which of them are actually sound. If the
engine already plays nearly all of them, the remaining brilliant count is a property of the
positions reached, not of the look - and no amount of tuning the look can raise it.

"Sound" here is the strict-brilliant test applied at the position level:
  SF(sacrifice) >= SF(best quiet) - 50cp, and SF(sacrifice) > -150 (not losing), and the
  position was not already won (SF best < +300).

Usage: python3 -I testing/ceiling.py <run> [games] [sf-nodes]
"""
import json, os, sys, subprocess, re
from concurrent.futures import ThreadPoolExecutor

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
BIN = os.path.join(ROOT, "target", "release", "aggro_chess")
SF = "/usr/games/stockfish"
RIG = [("Threads", "1"), ("Hash", "64"), ("Skill Level", "21"), ("Brilliance", "100"),
       ("Sacrifice Appetite", "95"), ("Aggression", "90"), ("King Attack Priority", "90"),
       ("Material Preservation", "15"), ("Speculative Sacrifices", "90"),
       ("Human Style", "70"), ("Human Think Time", "0"), ("Move Overhead", "50")]
# every sacrifice candidate the engine nominates, in one go, via a dev-only dump
DUMP = "AGGRO_SACDUMP"


def engine(prefix):
    """Returns (candidates, chosen). Candidates come from the engine's own classifier."""
    env = {**os.environ, "AGGRO_DEV": "1", DUMP: "1"}
    p = subprocess.Popen([BIN], stdin=subprocess.PIPE, stdout=subprocess.PIPE,
                         stderr=subprocess.DEVNULL, text=True, bufsize=0, cwd=ROOT, env=env)

    def s(x):
        p.stdin.write(x + "\n"); p.stdin.flush()

    s("uci")
    while p.stdout.readline().strip() != "uciok":
        pass
    for k, v in RIG:
        s(f"setoption name {k} value {v}")
    s("isready")
    while p.stdout.readline().strip() != "readyok":
        pass
    s("ucinewgame")
    s("position startpos" + (" moves " + " ".join(prefix) if prefix else ""))
    s("go movetime 300")
    cands, bm = [], None
    while True:
        l = p.stdout.readline()
        if not l:
            break
        l = l.strip()
        if l.startswith("info string sacdump"):
            cands.extend(l.split()[3:])
        if l.startswith("bestmove"):
            bm = l.split()[1]
            break
    s("quit")
    try:
        p.wait(2)
    except Exception:
        p.kill()
    return cands, bm


def sf_many(prefix, moves, nodes):
    p = subprocess.Popen([SF], stdin=subprocess.PIPE, stdout=subprocess.PIPE,
                         stderr=subprocess.DEVNULL, text=True, bufsize=0)

    def s(x):
        p.stdin.write(x + "\n"); p.stdin.flush()

    s("uci")
    while p.stdout.readline().strip() != "uciok":
        pass
    s("setoption name Hash value 128")
    out = {}
    for mv in moves:
        s("position startpos" + (" moves " + " ".join(prefix + [mv])))
        s(f"go nodes {nodes}")
        sc = None
        while True:
            l = p.stdout.readline()
            if not l:
                break
            l = l.strip()
            if l.startswith("info") and " score " in l and " pv " in l:
                m = re.search(r"score (cp|mate) (-?\d+)", l)
                if m:
                    sc = (m.group(1), int(m.group(2)))
            if l.startswith("bestmove"):
                break
        if sc is None:
            out[mv] = None
        elif sc[0] == "mate":
            out[mv] = 10000 if sc[1] > 0 else -10000
        else:
            out[mv] = -sc[1]
    s("quit")
    try:
        p.wait(2)
    except Exception:
        p.kill()
    return out


def main():
    run = sys.argv[1]
    n_games = int(sys.argv[2]) if len(sys.argv) > 2 else 10
    nodes = int(sys.argv[3]) if len(sys.argv) > 3 else 100000
    d = os.path.join(ROOT, "testing", "runs", "lever", run)
    games = [json.loads(l) for l in open(os.path.join(d, run + ".jsonl"))][:n_games]

    def per_game(g):
        rows = []
        for i in range(len(g["moves"])):
            cands, bm = engine(g["moves"][:i])
            if not cands or not bm:
                continue
            rows.append((g["id"], i, g["moves"][:i], cands, bm))
        return rows

    with ThreadPoolExecutor(max_workers=6) as ex:
        allrows = [r for rs in ex.map(per_game, games) for r in rs]
    print(f"{len(allrows)} positions with >=1 sacrifice candidate", flush=True)

    def judge(r):
        gid, i, prefix, cands, bm = r
        mv_set = list(dict.fromkeys(cands + [bm]))
        ev = sf_many(prefix, mv_set, nodes)
        return r, ev

    with ThreadPoolExecutor(max_workers=6) as ex:
        judged = list(ex.map(judge, allrows))

    have_sound = take = missed = 0
    missed_rows = []
    for (gid, i, prefix, cands, bm), ev in judged:
        b = ev.get(bm)
        if b is None or b >= 300:
            continue  # already won: not a brilliancy site
        if abs(b) >= 9000:
            continue  # a forced mate either way: material is irrelevant
        if b <= -250:
            continue  # already lost: a sacrifice that keeps it lost is not a brilliancy
        sound = [m for m in cands
                 if ev.get(m) is not None and abs(ev[m]) < 9000
                 and ev[m] > -150 and ev[m] >= b - 50]
        if not sound:
            continue
        have_sound += 1
        if bm in cands and ev.get(bm) is not None and ev[bm] > -150:
            take += 1
        else:
            missed += 1
            missed_rows.append((gid, i, prefix, bm, b, [(m, ev[m]) for m in sound][:3]))

    print(f"positions where a SOUND sacrifice existed (not already won): {have_sound}")
    print(f"  engine took a sound sacrifice : {take}")
    print(f"  engine missed one             : {missed}")
    if have_sound:
        print(f"  conversion rate: {take/have_sound:.1%}")
    for r in missed_rows[:15]:
        gid, i, prefix, bm, b, snd = r
        print(f"  MISSED g{gid} ply{i}: played {bm} ({b}); sound: {snd}")


main()
