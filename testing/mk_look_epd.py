"""Build a regression EPD from measured look decisions.

Sources: the lever runs. For every position where the look *declined* a sacrifice that
Stockfish confirmed is bad, the engine's chosen quiet move is the expected answer: the look
must not be relaxed into taking those. For every position where the look *promoted* a sound
sacrifice, the sacrifice is the expected answer: the look must not be tightened into losing
those.

Each line is a normal EPD with `bm <uci>` and comments naming the source game and the SF
verdict, so a future change that flips either class fails the suite.

Usage: python3 -I testing/mk_look_epd.py <run> <games> <sf-nodes> <out.epd>
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


def board_fen(prefix):
    """FEN after `prefix` moves from startpos (via the engine's own `d`)."""
    env = {**os.environ, "AGGRO_DEV": "1"}
    p = subprocess.Popen([BIN], stdin=subprocess.PIPE, stdout=subprocess.PIPE,
                         stderr=subprocess.DEVNULL, text=True, bufsize=0, cwd=ROOT, env=env)

    def s(x):
        p.stdin.write(x + "\n"); p.stdin.flush()

    s("uci")
    while p.stdout.readline().strip() != "uciok":
        pass
    s("position startpos" + (" moves " + " ".join(prefix) if prefix else ""))
    s("d")
    fen = None
    while True:
        l = p.stdout.readline()
        if not l:
            break
        l = l.strip()
        if l.count("/") == 7 and " " in l and not l.startswith("Fen:"):
            fen = l
        if l.startswith("eval (side"):
            break
    s("quit")
    try:
        p.wait(2)
    except Exception:
        p.kill()
    return fen


def ask(prefix):
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


def sf_pair(prefix, mv1, mv2, nodes):
    p = subprocess.Popen([SF], stdin=subprocess.PIPE, stdout=subprocess.PIPE,
                         stderr=subprocess.DEVNULL, text=True, bufsize=0)

    def s(x):
        p.stdin.write(x + "\n"); p.stdin.flush()

    s("uci")
    while p.stdout.readline().strip() != "uciok":
        pass
    s("setoption name Hash value 128")
    out = {}
    for mv in (mv1, mv2):
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
    return out.get(mv1), out.get(mv2)


def main():
    run, n_games, nodes, out_path = sys.argv[1], int(sys.argv[2]), int(sys.argv[3]), sys.argv[4]
    d = os.path.join(ROOT, "testing", "runs", "lever", run)
    games = [json.loads(l) for l in open(os.path.join(d, run + ".jsonl"))][:n_games]

    def scan(g):
        out = []
        for i in range(len(g["moves"])):
            look, bm = ask(g["moves"][:i])
            if not look or not bm:
                continue
            m = LOOK.search(look)
            if not m:
                continue
            kind, smv, s2, s1, da, db, played = m.groups()
            out.append((g["id"], i, g["moves"][:i], smv, bm, kind, bool(played), int(s1), int(s2)))
        return out

    with ThreadPoolExecutor(max_workers=6) as ex:
        cand = [x for r in ex.map(scan, games) for x in r]

    def judge(c):
        gid, i, prefix, smv, bm, kind, played, s1, s2 = c
        a, b = sf_pair(prefix, smv, bm, nodes)
        return c + (a, b)

    with ThreadPoolExecutor(max_workers=6) as ex:
        res = [r for r in ex.map(judge, cand) if r[9] is not None and r[10] is not None]

    lines = []
    for gid, i, prefix, smv, bm, kind, played, s1, s2, a, b in sorted(res, key=lambda r: (r[0], r[1])):
        # Only positions where the *game* was still in doubt count: an already-won position is
        # not a brilliancy site (the strict predicate requires SF(before) < +300), and there a
        # second winning move is not a regression.
        if b >= 300:
            continue
        # veto class: the look declined a sacrifice Stockfish rates far worse than the move
        # played. Assert the *avoid* form: the banned move must never be chosen, while any
        # other answer is fine - that is exactly the policy under test and it does not
        # depend on which quiet move the search happens to prefer on a given day.
        if kind == "sacrifice" and not played and a < b - 60:
            fen = board_fen(prefix)
            if fen:
                lines.append(f'{fen} am {smv}; id "V{gid}-{i}"; '
                             f'c0 "look vetoed {smv} (SF {a} vs played {bm} at {b})"; '
                             f'c1 "veto class - relaxing the look would play a losing sacrifice";')
        # promote class: the look played a sacrifice SF confirms is good. Here the exact move
        # is the point, so a plain bm test is right.
        if kind == "sacrifice" and played and a > b + 40:
            fen = board_fen(prefix)
            if fen:
                lines.append(f'{fen} bm {smv}; id "P{gid}-{i}"; '
                             f'c0 "look promoted {smv} (SF {a} vs {b}): a sound sacrifice"; '
                             f'c1 "promote class - tightening the look would lose this"; '
                             f'c3 "{smv}";')
    with open(out_path, "w") as f:
        f.write("\n".join(lines) + "\n")
    print(f"wrote {len(lines)} positions to {out_path}")


main()
