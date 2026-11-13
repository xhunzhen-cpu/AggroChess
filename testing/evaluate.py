"""Evaluate a toute take-rule over one or more site datasets (testing/decisions.py output).

Reports, for a candidate promote rule, the three numbers that matter and nothing else:
  * gems      strict-brilliant sacrifices it takes (SF: sac > -150, within 50 of best, pos < 300)
  * cost      net centipawns against the move the sacrifice displaced (negative favours the sac)
  * cp/gem    the price per brilliancy - the number that decides whether a rule is worth it

Positions are deduplicated by move prefix across datasets, because lever runs share opening
lines and the same position can otherwise be counted three times.

Usage:
  python3 -I testing/evaluate.py <sites.jsonl> [...]            # sweep the built-in grid
  python3 -I testing/evaluate.py --rule 60,-75,1000 <sites...>   # one rule, with the gem list
"""
import json, sys

MATE = 9000


def load(paths):
    seen = {}
    for p in paths:
        for line in open(p):
            r = json.loads(line)
            if r["branch"] != "sacrifice" or r["alt"] == r["look_move"]:
                continue
            if abs(r["s1"]) >= MATE or abs(r["s2"]) >= MATE:
                continue
            sac = r["sf"].get(r["look_move"])
            q = r["sf"].get(r["alt"])
            if sac is None or q is None or abs(sac) >= MATE or abs(q) >= MATE:
                continue
            r["_sac"], r["_quiet"] = sac, q
            seen[" ".join(r["prefix"])] = r
    return list(seen.values())


def strict(r):
    """The strict-brilliant predicate applied to Stockfish's truth at this site."""
    return (r["_sac"] > -150) and (r["_sac"] - r["_quiet"] >= -50) and (r["_quiet"] < 300)


def takes(r, margin, floor, maxq):
    return r["s2"] >= r["s1"] - margin and r["s2"] >= floor and r["s1"] < maxq


def stats(rows, margin, floor, maxq):
    t = [r for r in rows if takes(r, margin, floor, maxq)]
    gems = sum(1 for r in t if strict(r))
    cost = sum(r["_quiet"] - r["_sac"] for r in t)
    return t, gems, cost


def main():
    args = sys.argv[1:]
    rule = None
    if "--rule" in args:
        i = args.index("--rule")
        rule = [int(x) for x in args[i + 1].split(",")]
        args = args[:i] + args[i + 2:]
    paths = [a for a in args if not a.startswith("--")]
    rows = load(paths)
    print(f"{len(paths)} dataset(s), {len(rows)} unique non-vacuous sacrifice sites")
    print(f"{sum(1 for r in rows if strict(r))} of them hold a strict-brilliant sacrifice\n")

    if rule:
        m, fl, mq = rule
        t, gems, cost = stats(rows, m, fl, mq)
        print(f"rule margin={m} floor={fl} quiet<{mq}: takes {len(t)}, {gems} gems, "
              f"{cost:+d} cp, {cost / max(1, gems):.0f} cp/gem")
        print("\ngems taken:")
        for r in t:
            if strict(r):
                print(f"  g{r['game']} p{r['ply']:<3} {r['look_move']} "
                      f"dl={r['s1'] - r['s2']:>5} cost={r['_quiet'] - r['_sac']:>5}")
        print("\nworst non-gems taken:")
        for r in sorted((r for r in t if not strict(r)),
                        key=lambda r: r["_quiet"] - r["_sac"], reverse=True)[:10]:
            print(f"  g{r['game']} p{r['ply']:<3} {r['look_move']} "
                  f"sf_sac={r['_sac']:>6} sf_quiet={r['_quiet']:>6} "
                  f"cost={r['_quiet'] - r['_sac']:>5}")
        return

    print(f"{'margin':>6} {'floor':>6} {'maxq':>6} | {'taken':>5} {'gems':>4} {'cp':>6} {'cp/gem':>7}")
    for m in (50, 60, 70, 80):
        for fl in (-150, -100, -75, -50):
            for mq in (300, 1000):
                t, gems, cost = stats(rows, m, fl, mq)
                print(f"{m:>6} {fl:>6} {mq:>6} | {len(t):>5} {gems:>4} {cost:>6} "
                      f"{cost / max(1, gems):>7.0f}")


main()
