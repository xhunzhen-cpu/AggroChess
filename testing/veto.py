#!/usr/bin/env python3
"""veto.py - judge the look's VETO branch.

On the veto branch the main search chose a sacrifice and the look may overrule it with a
quiet move. Until `info string brilliancy sacrifice-cand` existed, the sacrificed move's
identity was lost once the swap overwrote best_move, so this side could not be measured.

The question this answers: when the look vetoes (or declines to veto), is it right? A veto
of a SOUND sacrifice is a lost brilliant; a missed veto of an UNSOUND one is a blunder let
through.

Uses the same strict-brilliant predicate as gems.py:
  sf(sac) > -150  and  sf(sac) - sf(quiet) >= -50  and  sf(quiet) < 300

Usage:
  python3 -I testing/veto.py <sites.jsonl> [...]
"""
import json
import sys

MATE = 9000
STRICT_NEAR = 50
STRICT_QUIET = 300
STRICT_FLOOR = -150


def val(r, mv):
    if not mv:
        return None
    v = r["sf"].get(mv)
    return None if v is None or abs(v) >= MATE else v


def load(paths):
    """Veto-branch rows with a known sacrifice and quiet move."""
    seen = {}
    drop = {"nosac": 0, "nofield": 0, "mate": 0, "same": 0}
    for p in paths:
        for line in open(p):
            r = json.loads(line)
            if r["branch"] != "safe":
                continue
            sac, q = val(r, r.get("sac")), val(r, r["look_move"])
            if r.get("sac") is None:
                drop["nosac"] += 1
                continue
            if sac is None or q is None:
                drop["nofield"] += 1
                continue
            if r["sac"] == r["look_move"]:
                drop["same"] += 1
                continue
            r["_sac"], r["_q"] = sac, q
            seen[" ".join(r["prefix"])] = r
    return list(seen.values()), drop


def sound(r):
    """Would Stockfish call the vetoed sacrifice brilliant?"""
    return (r["_sac"] > STRICT_FLOOR
            and (r["_sac"] - r["_q"]) >= -STRICT_NEAR
            and r["_q"] < STRICT_QUIET)


def decided_right(r):
    """Did the veto make the better choice? On this branch s1 = the sacrifice's score, s2 =
    the quiet move's. A fired veto played quiet, so it was right when quiet > sacrifice; a
    declined veto played the sacrifice, so it was right when sacrifice > quiet. Getting this
    backwards is easy - see the `sac`/`look_move` map above."""
    return (r["_q"] > r["_sac"]) if r["played"] else (r["_sac"] >= r["_q"])


def main():
    paths = [a for a in sys.argv[1:] if not a.startswith("--")]
    rows, drop = load(paths)
    if not rows:
        print(f"no usable veto sites (dropped {drop})")
        return
    fired = [r for r in rows if r["played"]]
    kept = [r for r in rows if not r["played"]]
    print(f"{len(rows)} usable veto sites "
          f"(dropped: {drop['nosac']} no-sac, {drop['nofield']} missing SF, {drop['same']} same)\n")

    print(f"veto fired {len(fired)}, declined {len(kept)}")
    right = sum(1 for r in rows if decided_right(r))
    print(f"decision accuracy {100*right/len(rows):.1f}% ({right}/{len(rows)})\n")

    def net(r):
        # SF(played) - SF(counterfactual); positive means the decision helped
        return (r["_q"] - r["_sac"]) if r["played"] else (r["_sac"] - r["_q"])

    for name, rs in (("fired", fired), ("declined", kept), ("all", rows)):
        if not rs:
            continue
        nets = [net(r) for r in rs]
        mean = sum(nets) / len(nets)
        helped = sum(1 for x in nets if x > 0)
        print(f"  {name:<9} n={len(rs):<3} mean net {mean:+7.1f} cp  helped {helped}/{len(rs)}")

    wrong = [r for r in rows if not decided_right(r)]
    if wrong:
        print(f"\nwrong decisions ({len(wrong)}); loss = |SF(played) - SF(best)|:")
        for r in sorted(wrong, key=lambda x: abs(net(x))):
            print(f"  g{r['game']} p{r['ply']:<3} {'VETO->quiet' if r['played'] else 'kept sac':<12} "
                  f"sac {r['sac']} SF{r['_sac']:+5.0f} vs quiet {r['look_move']} SF{r['_q']:+5.0f} "
                  f"loss {abs(net(r)):+5.0f}  look d={r['s2']-r['s1']:+5d}")


main()
