"""Is the look's own margin calibrated to the strict-brilliancy bar?

The strict predicate allows a sacrifice to be at most 50 cp worse than the best move. So the
question is not "does the look take sacrifices" but "does a promotion cost <= 50 cp". For
each sacrifice-branch site we know
    d_look = s1 - s2                     (how much worse the look rates the sacrifice)
    d_sf   = SF(quiet) - SF(sacrifice)   (how much worse Stockfish rates it)
and whether the current rule takes it. If d_look tracks d_sf, the margin is a real predictor
and simply has to be tightened to the same 50 cp bar.

Usage: python3 -I testing/calib.py <sites.jsonl>
"""
import json, sys

MATE = 9000


def val(site, mv):
    v = site["sf"].get(mv)
    return None if v is None or abs(v) >= MATE else v


def main():
    rows = [json.loads(l) for l in open(sys.argv[1])]
    pts = []
    for r in rows:
        if r["branch"] != "sacrifice":
            continue
        if abs(r["s1"]) >= MATE or abs(r["s2"]) >= MATE:
            continue
        sac = val(r, r["look_move"])
        quiet = val(r, r["alt"] or r["bm"])
        if sac is None or quiet is None:
            continue
        pts.append((r["s1"] - r["s2"], quiet - sac, r))

    print(f"{len(pts)} usable sacrifice sites")
    # vacuous sites: the engine plays the sacrifice with Brilliance 0 too, so the look did not
    # cause the move and there is no counterfactual to judge
    vac = [p for p in pts if p[2]["alt"] == p[2]["look_move"]]
    print(f"  of which {len(vac)} vacuous (Brilliance 0 plays the sacrifice anyway)")
    # correlation between the look's view and SF's
    n = len(pts)
    mx = sum(p[0] for p in pts) / n
    my = sum(p[1] for p in pts) / n
    sxy = sum((p[0] - mx) * (p[1] - my) for p in pts)
    sxx = sum((p[0] - mx) ** 2 for p in pts)
    syy = sum((p[1] - my) ** 2 for p in pts)
    r_corr = sxy / (sxx * syy) ** 0.5 if sxx and syy else 0
    print(f"correlation d_look vs d_sf: r = {r_corr:+.3f}")
    print()

    print("what the current rule (margin 80) takes, split by SF cost:")
    take = [(dl, ds, r) for dl, ds, r in pts
            if r["alt"] != r["look_move"]
            and r["s2"] >= r["s1"] - 80 and r["s2"] >= -50 and r["s1"] < 1000]
    for dl, ds, r in sorted(take, key=lambda x: -x[1]):
        verdict = "brilliant" if ds <= 50 else "BAD"
        print(f"  g{r['game']} p{r['ply']:<3} {r['look_move']} d_look={dl:>5} d_sf={ds:>5} {verdict}")
    print(f"  -> {len(take)} taken, {sum(1 for _, ds, _ in take if ds > 50)} cost more than the 50cp bar")
    print()

    print(f"{'margin':>7} | {'taken':>6} {'ok(<=50)':>9} {'bad(>50)':>9} {'cost':>7}")
    for m in (40, 50, 60, 70, 80, 100, 150):
        t = [(dl, ds) for dl, ds, r in pts
             if r["alt"] != r["look_move"]
             and r["s2"] >= r["s1"] - m and r["s2"] >= -50 and r["s1"] < 1000]
        ok = sum(1 for _, ds in t if ds <= 50)
        bad = sum(1 for _, ds in t if ds > 50)
        cost = sum(ds for _, ds in t)  # net cp given away vs the quiet move
        print(f"{m:>7} | {len(t):>6} {ok:>9} {bad:>9} {cost:>7}")


main()
