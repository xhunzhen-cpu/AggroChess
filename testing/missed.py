"""Find BRILLIANCIES THE ENGINE MISSED: positions where a sacrifice was sound and the engine
played something else.

This is the deterministic, per-position question ("is there headroom?") rather than the noisy
per-game count. For each search position of one side in a batch of games:

  * ask Stockfish whether one of the engine's own sacrifice candidates (from its look line)
    is objectively at least as good as the move it played, by a real margin
  * if yes, we have a missed brilliancy; report it with the position

Usage: python3 -I testing/missed.py <run-name> [games] [sf-nodes]
"""
import json, os, re, sys, subprocess
from concurrent.futures import ThreadPoolExecutor

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
BIN = os.path.join(ROOT, "target", "release", "aggro_chess")
SF = "/usr/games/stockfish"
RIG = [("Threads", "1"), ("Hash", "64"), ("Skill Level", "21"), ("Brilliance", "100"),
       ("Sacrifice Appetite", "95"), ("Aggression", "90"), ("King Attack Priority", "90"),
       ("Material Preservation", "15"), ("Speculative Sacrifices", "90"),
       ("Human Style", "70"), ("Human Think Time", "0"), ("Move Overhead", "50")]
LOOK = re.compile(r"brilliancy look: (safe|sacrifice) (\S+) (-?\d+) vs .* (-?\d+) \(neutral, depth (\d+)/(\d+)\)( -> played)?")


def engine_scan(prefix):
    env = {**os.environ, "AGGRO_DEV": "1"}
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
    look, bm = None, None
    while True:
        l = p.stdout.readline()
        if not l:
            break
        l = l.strip()
        if "brilliancy look:" in l:
            look = l
        if l.startswith("bestmove"):
            bm = l.split()[1]
            break
    s("quit")
    try:
        p.wait(2)
    except Exception:
        p.kill()
    return look, bm


def sf_scores(prefix, moves, nodes):
    """White-POV cp after each move of `moves` played from `prefix`."""
    p = subprocess.Popen([SF], stdin=subprocess.PIPE, stdout=subprocess.PIPE,
                         stderr=subprocess.DEVNULL, text=True, bufsize=0)

    def s(x):
        p.stdin.write(x + "\n"); p.stdin.flush()

    s("uci")
    while p.stdout.readline().strip() != "uciok":
        pass
    s("setoption name Threads value 1")
    s("setoption name Hash value 128")
    res = {}
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
            res[mv] = None
        elif sc[0] == "mate":
            res[mv] = 10000 if sc[1] > 0 else -10000
        else:
            res[mv] = -sc[1]  # SF scores the side to move; we want the mover's view
    s("quit")
    try:
        p.wait(2)
    except Exception:
        p.kill()
    return res


def main():
    run = sys.argv[1]
    n_games = int(sys.argv[2]) if len(sys.argv) > 2 else 8
    nodes = int(sys.argv[3]) if len(sys.argv) > 3 else 120000
    d = os.path.join(ROOT, "testing", "runs", "lever", run)
    games = [json.loads(l) for l in open(os.path.join(d, run + ".jsonl"))][:n_games]

    def scan_game(g):
        moves = g["moves"]
        out = []
        for i in range(len(moves)):
            look, bm = engine_scan(moves[:i])
            if not look or not bm:
                continue
            m = LOOK.search(look)
            if not m:
                continue
            kind, smv, s2, s1, da, db, played = m.groups()
            # Only a line whose first label is "sacrifice" names a sacrifice move at all:
            # then the printed order is "<sac move> <sac score> vs best quiet <quiet score>".
            # A "safe" line's move is a quiet move and belongs to the other comparison.
            if kind != "sacrifice" or played:
                continue  # not a declined sacrifice
            sac_score, quiet_score = int(s2), int(s1)
            out.append((g["id"], i, moves[:i], smv, bm, sac_score, quiet_score, da, db, kind))
        return out

    with ThreadPoolExecutor(max_workers=6) as ex:
        allc = [x for r in ex.map(scan_game, games) for x in r]
    print(f"{len(allc)} positions where a sacrifice was on offer and declined", flush=True)

    def check(c):
        gid, i, prefix, smv, bm, s1, s2, da, db, kind = c
        ev = sf_scores(prefix, [smv, bm], nodes)
        return c + (ev.get(smv), ev.get(bm))

    with ThreadPoolExecutor(max_workers=6) as ex:
        res = list(ex.map(check, allc))
    missed, close = [], []
    for r in res:
        gid, i, prefix, smv, bm, s1, s2, da, db, kind, a, b = r
        if a is None or b is None:
            continue
        if a > b + 60:
            missed.append(r)
        elif a > b - 40:
            close.append(r)
    print(f"SF says the sacrifice is clearly better (> +60cp): {len(missed)}")
    for r in missed[:25]:
        gid, i, prefix, smv, bm, s1, s2, da, db, kind = r[:10]
        a, b = r[10], r[11]
        print(f"  g{gid} ply{i} sac {smv} sf{a:>6} vs played {bm} sf{b:>6} "
              f"| look said sac {s1} vs best {s2} at depth {da}/{db}")
    print(f"within 40cp (a coin flip): {len(close)}")


main()
