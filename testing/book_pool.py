#!/usr/bin/env python3
"""Pooled book analysis with CORRECT denominators.

The book arms are within-arm paired: coderoff and on play the SAME openings with the SAME seed, so
the ctl and sac legs share all 1500 games. Denominator = games PLAYED, never distinct game_id (the
bug that has silently corrupted several scripts in this project) and never games-with-events.

Usage: python3 -I book_pool.py <arm> [<arm> ...]
"""
import json, math, sys, os

sys.path.insert(0, "/tmp/bmax")
from honest_score import honest, loose, strictest  # noqa: E402

RUNS = "/tmp/bmax/runs"


def leg(arm, suffix):
    p = f"{RUNS}/{arm}/{arm}_{suffix}.events.jsonl"
    if not os.path.exists(p):
        return None
    return [json.loads(l) for l in open(p) if l.strip()]


def main(arms):
    print(f"{'arm':16s} {'games':>6s} {'ctl HON':>8s} {'var HON':>8s} {'delta':>8s} "
          f"{'ctl loose':>10s} {'var loose':>10s} {'strict d':>9s}")
    tc = tv = tl_c = tl_v = ts_c = ts_v = tg = 0
    for arm in arms:
        s = f"{RUNS}/{arm}/{arm}.jsonl.summary.json"
        if not os.path.exists(s):
            print(f"{arm:16s} (no summary)")
            continue
        d = json.load(open(s))
        if not d.get("complete"):
            print(f"{arm:16s} INCOMPLETE {d.get('games_played')}/{d.get('games_total')} -- skipped")
            continue
        g = d["pairs"][0]["games"]
        c, v = leg(arm, "ctl"), leg(arm, "sac")
        if c is None or v is None:
            print(f"{arm:16s} {g:6d} (missing a graded leg)")
            continue

        def cnt(evs, pred):
            return sum(1 for e in evs if e.get("type") == "sacrifice" and pred(e))

        ch, vh = cnt(c, honest), cnt(v, honest)
        cl, vl = cnt(c, loose), cnt(v, loose)
        cs, vs = cnt(c, strictest), cnt(v, strictest)
        print(f"{arm:16s} {g:6d} {ch/g:8.4f} {vh/g:8.4f} {(vh-ch)/g:+8.4f} "
              f"{cl/g:10.4f} {vl/g:10.4f} {(vs-cs)/g:+9.4f}")
        tc += ch; tv += vh; tl_c += cl; tl_v += vl; ts_c += cs; ts_v += vs; tg += g

    if tg:
        print("-" * 84)
        print(f"{'POOLED':16s} {tg:6d} {tc/tg:8.4f} {tv/tg:8.4f} {(tv-tc)/tg:+8.4f} "
              f"{tl_c/tg:10.4f} {tl_v/tg:10.4f} {(ts_v-ts_c)/tg:+9.4f}")
        # Poisson SE on the pooled honest difference (counts are small; this is the honest bar)
        se = math.sqrt(max(tc + tv, 1)) / tg
        d_ = (tv - tc) / tg
        lo, hi = d_ - 1.96 * se, d_ + 1.96 * se
        print(f"\npooled honest delta {d_:+.4f}/g   Poisson 95% CI [{lo:+.4f}, {hi:+.4f}]")
        print(f"counts: book OFF {tc} -> book ON {tv} over {tg} games "
              f"({tv/max(tc,1):.3f}x)")
        # per-arm consistency: this project has been burned by one strong seed + one flat seed
        print("\nper-arm deltas (consistency check -- one strong seed is not a result):")


if __name__ == "__main__":
    main(sys.argv[1:])
