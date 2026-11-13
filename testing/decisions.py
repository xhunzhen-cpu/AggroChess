"""One dataset per look site, so the take-condition can be swept offline.

Every "brilliant per game" A/B in this repo has a 95% CI of +-0.05/g, wider than any
threshold change, so changing a number and re-running games tells you nothing. This script
records, for every position in a measured game where the look produced a verdict:

  branch   "sacrifice" = the sacrifice was the candidate (the look may promote it)
           "safe"      = the sacrifice was already the main search's move (may veto it)
  s2, s1   the look's own score for the sacrifice / for the quiet moves
  da, db   the depths behind those two numbers
  played   whether this verdict led to a swap
  alt      the move the same engine plays with Brilliance 0 - the counterfactual
  sf       Stockfish's score for the sacrifice, the engine's move, and `alt`

With that, any proposed take-condition can be replayed over every site in microseconds and
judged by "would it take a sacrifice SF supports / decline one SF rejects", which is exact
and noise-free. It is also the right metric: the strict-brilliant predicate is a per-move
judgement (SF after - SF before >= -50), not a per-game accumulation.

Usage: python3 -I testing/decisions.py <out.jsonl> <run> <games> <sf-nodes>
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
LOOK = re.compile(r"brilliancy look: (safe|sacrifice) (\S+) (-?\d+) vs .* (-?\d+) "
                  r"\(neutral, depth (\d+)/(\d+)\)( -> played)?")


def engine(prefix, brilliance):
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
    if brilliance != 100:
        s(f"setoption name Brilliance value {brilliance}")
    s("isready")
    while p.stdout.readline().strip() != "readyok":
        pass
    s("ucinewgame")
    s("position startpos" + (" moves " + " ".join(prefix) if prefix else ""))
    s("go movetime 300")
    look, bm, cand = None, None, None
    while True:
        l = p.stdout.readline()
        if not l:
            break
        l = l.strip()
        if "brilliancy look:" in l:
            look = l
        if "sacrifice-cand" in l:
            cand = l.split()[-1]
        if l.startswith("bestmove"):
            bm = l.split()[1]
            break
    s("quit")
    try:
        p.wait(2)
    except Exception:
        p.kill()
    return look, bm, cand


def sf_scores(prefix, moves, nodes):
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


def one(prefix, gid, ply, nodes):
    look, bm, cand = engine(prefix, 100)
    if not look or not bm:
        return None
    m = LOOK.search(look)
    if not m:
        return None
    branch, mv, s2, s1, da, db, played = m.groups()
    _, alt, _ = engine(prefix, 0)
    # On the veto branch the verdict names the quiet replacement, so the sacrifice's identity
    # comes from the `sacrifice-cand` line; on the promote branch it is the verdict's move.
    sac_mv = mv if branch == "sacrifice" else cand
    movs = list(dict.fromkeys([mv, bm, sac_mv] + ([alt] if alt else []) + ([cand] if cand else [])))
    movs = [x for x in movs if x]
    ev = sf_scores(prefix, movs, nodes)
    if ev.get(mv) is None or ev.get(bm) is None:
        return None
    return {"game": gid, "ply": ply, "prefix": prefix, "branch": branch,
            "look_move": mv, "s2": int(s2), "s1": int(s1), "da": int(da), "db": int(db),
            "played": bool(played), "bm": bm, "alt": alt, "sac": sac_mv, "sf": ev}


def main():
    out_path, run, n_games, nodes = sys.argv[1], sys.argv[2], int(sys.argv[3]), int(sys.argv[4])
    d = os.path.join(ROOT, "testing", "runs", "lever", run)
    games = [json.loads(l) for l in open(os.path.join(d, run + ".jsonl"))][:n_games]
    sites = [(g["moves"][:i], g["id"], i) for g in games for i in range(len(g["moves"]))]
    print(f"{len(sites)} sites", flush=True)

    with ThreadPoolExecutor(max_workers=6) as ex:
        rows = [r for r in ex.map(lambda a: one(a[0], a[1], a[2], nodes), sites) if r]
    with open(out_path, "w") as f:
        for r in rows:
            f.write(json.dumps(r) + "\n")
    print(f"wrote {len(rows)} look sites to {out_path}", flush=True)


main()
