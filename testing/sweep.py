"""Replay candidate look rules over a site dataset (testing/decisions.py output).

For each site we know the look's own numbers (s2 = sacrifice score, s1 = quiet score, the
depths behind them) and Stockfish's truth for the sacrifice and for the quiet alternative.
So a proposed take-condition can be evaluated exactly: did it promote a sacrifice that is a
strict brilliancy, did it promote one that is not, did it decline one that was.

The strict-brilliant predicate (testing/score_brilliant.py), applied per move:
    sf(move) - best_before >= -50   and   sf(move) > -150   and   best_before < 300

Usage: python3 -I testing/sweep.py <sites.jsonl> [...]
"""
import json, sys

MATE = 9000


def best_before(site):
    """Best SF score available at the site, from our own samples."""
    cand = [v for v in site["sf"].values() if v is not None and abs(v) < MATE]
    return max(cand) if cand else None


def strict(site, mv):
    b = best_before(site)
    f = site["sf"].get(mv)
    if b is None or f is None or abs(f) >= MATE:
        return False
    return f - b >= -50 and f > -150 and b < 300


def rule(site, margin=80, floor=-50, max_quiet=1000, veto=None, min_quiet_gap=None):
    """The look's promote branch, parameterised. `veto`/`min_quiet_gap` describe the veto side."""
    if site["branch"] != "sacrifice":
        return None  # veto branch: nothing is promoted, handled separately
    s2, s1 = site["s2"], site["s1"]
    if abs(s1) >= MATE or abs(s2) >= MATE:
        return None
    ok = s2 >= s1 - margin and s2 >= floor and s1 < max_quiet
    return ok


def report(rows, **kw):
    prom_ok = prom_bad = 0
    both = []
    for r in rows:
        take = rule(r, **kw)
        if take is None:
            continue
        good = strict(r, r["look_move"])
        if take and good:
            prom_ok += 1
        elif take and not good:
            prom_bad += 1
        both.append((r, take, good))
    return prom_ok, prom_bad, both


def main():
    rows = [json.loads(l) for l in open(sys.argv[1])]
    sac = [r for r in rows if r["branch"] == "sacrifice"]
    print(f"{len(rows)} sites, {len(sac)} on the sacrifice branch "
          f"(the only ones the look can promote)")
    print()

    # the truth: how many sacrifice sites hold a strict-brilliant sacrifice at all?
    avail = [r for r in sac if strict(r, r["look_move"])]
    print(f"sites where the candidate sacrifice IS a strict brilliancy: {len(avail)}")
    for r in avail:
        print(f"  g{r['game']} p{r['ply']:<3} {r['look_move']} sf{r['sf'][r['look_move']]:>6} "
              f"best{best_before(r):>6} look sac{r['s2']:>5} quiet{r['s1']:>5} "
              f"played={r['played']}")
    print()

    print(f"{'margin':>7} {'floor':>6} {'maxq':>6} | {'took+good':>9} {'took+bad':>8}")
    for margin in (50, 65, 80):
        for floor in (-150, -100, -50):
            for maxq in (300, 600, 1000):
                ok, bad, _ = report(sac, margin=margin, floor=floor, max_quiet=maxq)
                print(f"{margin:>7} {floor:>6} {maxq:>6} | {ok:>9} {bad:>8}")


main()
