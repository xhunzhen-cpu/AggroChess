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

RESULTS (2026-11-13), so nobody re-litigates them:

  rule            pooled(1061 seats)      held-out(1206 sites)
                  gems   cp  cp/gem       gems   cp  cp/gem
  80/-50/1000        9  3607    401         13  1036    80     <- 5.3.0
  60/-100/1000      11  2557    232         15   959    64     <- ADOPTED
  110/-150/250      16  3221    201         14  2011   144     <- REFUTED

  The wide rule wins on the pooled set only because that set was used to fit it.
  On held-out data it is worse than the adopted rule on every metric, at >2x the
  cost. Never adopt a take-rule without evaluating it on runs outside the fit.

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


def takes(r, margin, floor, cap, s2cap=0):
    """The engine's promote condition. `cap` is the quiet cap (s1 < cap); `s2cap` is
    LOOK_S2_CAP (s2 < s2cap, 0 = off), which drops sacrifices already scoring near-winning."""
    return (r["s2"] >= r["s1"] - margin and r["s2"] >= floor
            and r["s1"] < cap and (s2cap == 0 or r["s2"] < s2cap))


def run(rows, margin, floor, cap, s2cap=0):
    t = [r for r in rows if takes(r, margin, floor, cap, s2cap)]
    gems = [r for r in t if strict(r)]
    cost = sum(r["_q"] - r["_sac"] for r in t)
    return len(t), len(gems), cost, gems


def main():
    args = sys.argv[1:]
    rules = [(60, -100, 1000)]
    if "--rules" in args:
        i = args.index("--rules")
        rules = [tuple(int(x) for x in spec.split(",")) for spec in args[i + 1].split(";")]
        args = args[:i] + args[i + 2:]
    rows, dropped = load([a for a in args if not a.startswith("--")])
    print(f"{len(rows)} usable sites "
          f"(dropped: {dropped['vacuous']} vacuous, {dropped['terminal']} terminal, "
          f"{dropped['mate']} mate, {dropped['nofield']} missing)")
    print(f"{sum(1 for r in rows if strict(r))} hold a strict-brilliant sacrifice\n")
    print(f"{'rule (margin/floor/cap[/s2cap])':<32}{'takes':>6}{'gems':>6}{'cp':>8}{'cp/gem':>8}")
    for rule in rules:
        margin, floor, cap = rule[:3]
        s2cap = rule[3] if len(rule) > 3 else 0
        t, g, c, _ = run(rows, margin, floor, cap, s2cap)
        per = c / max(1, g)
        label = f"{margin}/{floor}/{cap}" + (f"/{s2cap}" if s2cap else "")
        print(f"{label:<32}{t:>6}{g:>6}{c:>8}{per:>8.0f}")


main()
