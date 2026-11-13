"""What does the look's promote rule actually COST, in centipawns?

For every site on the sacrifice branch, the look either promotes the sacrifice or keeps the
quiet move. Stockfish tells us the objective price of each choice, so the cost of a rule is

    sum over sites where rule takes, of (SF(quiet) - SF(sacrifice))     [positive = loss]

and the benefit is the negative part (sacrifices SF likes better than the quiet). That is a
per-move Elo proxy, exact and noise-free, unlike a 120-game brilliant-per-game A/B.

Usage: python3 -I testing/cost.py <sites.jsonl>
"""
import json, sys

MATE = 9000


def val(site, mv):
    v = site["sf"].get(mv)
    if v is None or abs(v) >= MATE:
        return None
    return v


def evaluate(rows, margin, floor, maxq, depth_min=0):
    cost = benefit = 0
    n_take = 0
    worst = []
    for r in rows:
        if r["branch"] != "sacrifice" or r["da"] < depth_min:
            continue
        s2, s1 = r["s2"], r["s1"]
        if abs(s1) >= MATE or abs(s2) >= MATE:
            continue
        take = s2 >= s1 - margin and s2 >= floor and s1 < maxq
        if not take:
            continue
        n_take += 1
        sac = val(r, r["look_move"])
        # the counterfactual quiet: when the look promoted, bm IS the sacrifice, so the
        # "quiet" it displaced is the Brilliance-0 move (alt); otherwise bm is the quiet.
        quiet = val(r, r["alt"] or r["bm"])
        if sac is None or quiet is None:
            continue
        d = quiet - sac  # positive = the sacrifice is worse
        if d > 0:
            cost += d
        else:
            benefit -= d
        worst.append((d, r["game"], r["ply"], r["look_move"], sac, quiet))
    return n_take, cost, benefit, sorted(worst, reverse=True)


def main():
    rows = [json.loads(l) for l in open(sys.argv[1])]
    sac = [r for r in rows if r["branch"] == "sacrifice"]
    print(f"{len(sac)} sacrifice-branch sites\n")
    print(f"{'margin':>6} {'floor':>6} {'maxq':>6} {'mind':>5} | {'n':>3} {'cost':>7} {'gain':>6} {'net':>7}")
    for mind in (0, 6):
        for margin in (50, 65, 80):
            for floor in (-150, -100, -50):
                for maxq in (200, 300, 1000):
                    n, c, g, _ = evaluate(sac, margin, floor, maxq, mind)
                    print(f"{margin:>6} {floor:>6} {maxq:>6} {mind:>5} | {n:>3} {c:>7} {g:>6} {c-g:>7}")
        print()

    print("current rule (80, -50, 1000), worst promotions:")
    _, _, _, worst = evaluate(sac, 80, -50, 1000)
    for d, g_, p, mv, s, q in worst[:15]:
        print(f"  g{g_} p{p:<3} {mv} sf{s:>6} vs quiet sf{q:>6}  costs {d:>4}cp")


main()
