#!/usr/bin/env python3
"""gems.py - evaluate look take-rules on site datasets, with the two data traps handled.

Traps this handles that earlier ad-hoc scripts did not:

  1. TERMINAL POSITIONS. The look reports s2 == 0 only at a terminal node (mate/stalemate).
     Those rows score sf_sac == sf_alt == 0 and sail through a naive strict predicate,
     inflating the gem count. They are dropped.

  2. VACUOUS SITES. alt == look_move means there is no counterfactual quiet move. Dropped.

  3. MATE scores are not centipawns. Dropped.

The strict-brilliant predicate is the one from testing/score_brilliant.py: the sacrifice is
within 50 cp of the quiet move, the quiet move is not already winning (< 300), and the
sacrifice does not leave a lost position (> -150).

Take-rule (src/search.rs): sb2 >= sa - margin && sb2 >= floor && sa < quiet_cap,
where sb2 = sacrifice's neutral score, sa = quiet group's neutral score.

Usage:
  python3 -I testing/gems.py <sites.jsonl> [...]
  python3 -I testing/gems.py --rules "60,-100,1000;110,-150,250" <sites...>
"""
import json
import sys

MATE = 9000
STRICT_NEAR = 50      # sac within 50 cp of the quiet move
STRICT_QUIET = 300    # quiet move must not already be winning
STRICT_FLOOR = -150   # sacrifice must not leave a lost position


def load(paths):
    seen = {}
    dropped = {"vacuous": 0, "mate": 0, "terminal": 0, "nofield": 0, "nodata": 0}
    for p in paths:
        for line in open(p):
            r = json.loads(line)
            if r["branch"] != "sacrifice":
                continue
            if r["alt"] == r["look_move"]:
                dropped["vacuous"] += 1
                continue
            if abs(r["s1"]) >= MATE or abs(r["s2"]) >= MATE:
                dropped["mate"] += 1
                continue
            if r["s2"] == 0:
                dropped["terminal"] += 1
                continue
            sac = r["sf"].get(r["look_move"])
            q = r["sf"].get(r["alt"])
            if sac is None or q is None:
                dropped["nofield"] += 1
                continue
            if abs(sac) >= MATE or abs(q) >= MATE:
                dropped["mate"] += 1
                continue
            r["_sac"], r["_q"] = sac, q
            seen[" ".join(r["prefix"])] = r
    return list(seen.values()), dropped


def strict(r):
    return (r["_sac"] > STRICT_FLOOR
            and (r["_sac"] - r["_q"]) >= -STRICT_NEAR
            and r["_q"] < STRICT_QUIET)


def takes(r, margin, floor, cap):
    return r["s2"] >= r["s1"] - margin and r["s2"] >= floor and r["s1"] < cap


def run(rows, margin, floor, cap):
    t = [r for r in rows if takes(r, margin, floor, cap)]
    gems = [r for r in t if strict(r)]
    cost = sum(r["_q"] - r["_sac"] for r in t)
    return len(t), len(gems), cost, gems


def main():
    args = sys.argv[1:]
    rules = [(60, -100, 1000), (110, -150, 250)]
    if "--rules" in args:
        i = args.index("--rules")
        rules = [tuple(int(x) for x in spec.split(",")) for spec in args[i + 1].split(";")]
        args = args[:i] + args[i + 2:]
    rows, dropped = load([a for a in args if not a.startswith("--")])
    print(f"{len(rows)} usable sites "
          f"(dropped: {dropped['vacuous']} vacuous, {dropped['terminal']} terminal, "
          f"{dropped['mate']} mate, {dropped['nofield']} missing)")
    print(f"{sum(1 for r in rows if strict(r))} hold a strict-brilliant sacrifice\n")
    print(f"{'rule (margin/floor/cap)':<26}{'takes':>6}{'gems':>6}{'cp':>8}{'cp/gem':>8}")
    for margin, floor, cap in rules:
        t, g, c, _ = run(rows, margin, floor, cap)
        per = c / max(1, g)
        print(f"{f'{margin}/{floor}/{cap}':<26}{t:>6}{g:>6}{c:>8}{per:>8.0f}")


main()
