"""Does the look need more time than it gets? For every *sound* sacrifice the engine found
during play (from the events), replay the position at increasing `go movetime` and record
whether the look promotes a sacrifice. If the promotion rate climbs with time, the look is
time-starved at 300 ms; if it is flat, the budget is not the constraint.

Usage: python3 -I testing/look_time.py <run> [games] [positions]
"""
import json, os, sys, subprocess, re
from concurrent.futures import ThreadPoolExecutor

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
BIN = os.path.join(ROOT, "target", "release", "aggro_chess")
RIG = [("Threads", "1"), ("Hash", "64"), ("Skill Level", "21"), ("Brilliance", "100"),
       ("Sacrifice Appetite", "95"), ("Aggression", "90"), ("King Attack Priority", "90"),
       ("Material Preservation", "15"), ("Speculative Sacrifices", "90"),
       ("Human Style", "70"), ("Human Think Time", "0"), ("Move Overhead", "50")]
LOOK = re.compile(r"brilliancy look: (safe|sacrifice) (\S+) (-?\d+) vs .* (-?\d+) \(neutral, depth (\d+)/(\d+)\)( -> played)?")
TIMES = (100, 300, 1000, 3000)


def ask(prefix, ms):
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
    s(f"go movetime {ms}")
    look, bm, depth = None, None, None
    while True:
        l = p.stdout.readline()
        if not l:
            break
        l = l.strip()
        if l.startswith("info depth") and " pv " in l:
            m = re.search(r"info depth (\d+)", l)
            if m:
                depth = int(m.group(1))
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
    return look, bm, depth


def is_promote(look):
    if not look:
        return False, None
    m = LOOK.search(look)
    if not m:
        return False, None
    kind, mv, s2, s1, da, db, played = m.groups()
    return (kind == "sacrifice" and bool(played)), int(da)


def main():
    run = sys.argv[1]
    n_games = int(sys.argv[2]) if len(sys.argv) > 2 else 20
    npos = int(sys.argv[3]) if len(sys.argv) > 3 else 40
    d = os.path.join(ROOT, "testing", "runs", "lever", run)
    games = {g["id"]: g for g in (json.loads(l) for l in open(os.path.join(d, run + ".jsonl")))}
    evs = [json.loads(l) for l in open(os.path.join(d, run + "_sac.events.jsonl"))]
    sacs = [e for e in evs if e.get("type") == "sacrifice" and e.get("category") in ("queen", "exchange", "piece")]
    sacs = [e for e in sacs if e.get("sf_before") is not None and 0 <= e["sf_before"] < 300
            and (e.get("sf_after") or -999) > -100][:npos]
    print(f"{len(sacs)} contested sound sacrifices to probe", flush=True)

    def one(e):
        prefix = games[e["game_id"]]["moves"][:e["ply"]]
        out = []
        for ms in TIMES:
            look, bm, depth = ask(prefix, ms)
            prom, da = is_promote(look)
            out.append((ms, prom, da, depth, bm))
        return e, out

    with ThreadPoolExecutor(max_workers=5) as ex:
        res = list(ex.map(one, sacs))
    print(f"{'game':>5} {'ply':>4} " + "  ".join(f"{ms:>5}ms" for ms in TIMES))
    counts = [0] * len(TIMES)
    for e, out in sorted(res, key=lambda x: (x[0]["game_id"], x[0]["ply"])):
        cells = []
        for i, (ms, prom, da, depth, bm) in enumerate(out):
            counts[i] += prom
            cells.append(f"{'PROMOTE' if prom else 'quiet':>7}(d{da or 0})")
        print(f"g{e['game_id']:>3} {e['ply']:>4} " + " ".join(cells))
    n = len(res)
    for i, ms in enumerate(TIMES):
        print(f"  {ms:>5} ms: promote {counts[i]}/{n}")


main()
