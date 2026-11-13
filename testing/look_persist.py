"""The inverse of look_time.py: do the look's BAD promotions survive more time?

For every sacrifice the look promoted that Stockfish rates clearly worse than the quiet move
it displaced, replay the position at increasing `go movetime` and see whether the look still
promotes. If the bad promotions persist at 3 s, the look's error is its own evaluation - no
margin, floor or time budget will fix it - and the right response is to stop the look from
overriding the search there, not to give it more of anything.

Usage: python3 -I testing/look_persist.py <sites.jsonl>
"""
import json, os, re, sys, subprocess

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
BIN = os.path.join(ROOT, "target", "release", "aggro_chess")
RIG = [("Threads", "1"), ("Hash", "64"), ("Skill Level", "21"), ("Brilliance", "100"),
       ("Sacrifice Appetite", "95"), ("Aggression", "90"), ("King Attack Priority", "90"),
       ("Material Preservation", "15"), ("Speculative Sacrifices", "90"),
       ("Human Style", "70"), ("Human Think Time", "0"), ("Move Overhead", "50")]
LOOK = re.compile(r"brilliancy look: (safe|sacrifice) (\S+) (-?\d+) vs .* (-?\d+) "
                  r"\(neutral, depth (\d+)/(\d+)\)( -> played)?")
TIMES = (300, 1000, 3000)
MATE = 9000


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


def main():
    rows = [json.loads(l) for l in open(sys.argv[1])]
    bad = []
    for r in rows:
        if r["branch"] != "sacrifice" or r["alt"] == r["look_move"]:
            continue
        sac = r["sf"].get(r["look_move"])
        q = r["sf"].get(r["alt"] or r["bm"])
        if sac is None or q is None or abs(sac) >= MATE or abs(q) >= MATE:
            continue
        if q - sac > 50:
            bad.append(r)
    print(f"{len(bad)} promotions Stockfish rates >50cp worse than the quiet alternative\n")
    for r in bad:
        print(f"g{r['game']} p{r['ply']} {r['look_move']} sf{sac} vs quiet sf{q}")
        cells = []
        for ms in TIMES:
            look, bm = ask(r["prefix"], ms)
            m = LOOK.search(look) if look else None
            if not m:
                cells.append(f"{ms}ms: —")
                continue
            br, mv, s2, s1, da, db, played = m.groups()
            if br == "sacrifice" and played:
                cells.append(f"{ms}ms: STILL PROMOTES {mv} (d{da})")
            elif br == "sacrifice":
                cells.append(f"{ms}ms: declined (sac{s2} vs quiet{s1}, d{da})")
            else:
                cells.append(f"{ms}ms: quiet {mv}")
        for c in cells:
            print("     " + c)
        print()


main()
