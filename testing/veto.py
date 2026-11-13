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


def main():
    paths = [a for a in sys.argv[1:] if not a.startswith("--")]
    rows, drop = load(paths)
    if not rows:
        print(f"no usable veto sites (dropped {drop})")
        return
    fired = [r for r in rows if r["played"]]
    kept = [r for r in rows if not r["played"]]
    snd = [r for r in rows if sound(r)]
    print(f"{len(rows)} usable veto sites "
          f"(dropped: {drop['nosac']} no-sac, {drop['nofield']} missing SF, {drop['same']} same)\n")

    def rate(name, rs):
        if not rs:
            print(f"  {name:<28}   0")
            return
        s = sum(1 for r in rs if sound(r))
        print(f"  {name:<28} {len(rs):>4} sites, {s:>3} sound sacrifices ({100*s/len(rs):5.1f}%)")

    print("veto behaviour:")
    rate("all sites", rows)
    rate("veto FIRED (quiet played)", fired)
    rate("veto declined (sac kept)", kept)
    print()
    if fired:
        wrong = [r for r in fired if sound(r)]
        print(f"vetoes of a SOUND sacrifice (lost brilliancies): {len(wrong)}/{len(fired)}")
        for r in wrong[:20]:
            print(f"  g{r['game']} p{r['ply']:<3} vetoed {r['sac']} "
                  f"(SF {r['_sac']:+5.0f}, quiet {r['look_move']} SF {r['_q']:+5.0f}, "
                  f"gap {r['_q']-r['_sac']:+5.0f})  look: s1={r['s1']} s2={r['s2']}")
    if kept:
        bad = [r for r in kept if not sound(r)]
        print(f"\nkept unsound sacrifices (blunders let through): {len(bad)}/{len(kept)}")


main()
