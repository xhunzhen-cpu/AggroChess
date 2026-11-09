#!/usr/bin/env python3
"""brilliant.py - count "brilliant-style" sacrifices in a sactest run directory.

A sacrifice (as classified by `aggro_arena analyze`, i.e. purposeful, not a blunder) counts as
brilliant-style when, roughly like chess.com's "Brilliant" label:
  * it gives up a piece or more (piece, exchange or queen sacrifice; pawn sacrifices don't count),
  * Stockfish finds it sound: its eval (engine's view) drops by at most --max-drop cp
    (default 50) from before the sacrifice to after it, and the sacrifice is not flagged unsound,
  * the position was not already won (Stockfish eval before < --max-before cp, default 300),
  * and the engine is not left losing (Stockfish eval after > --min-after, default -100).

Usage: python3 testing/brilliant.py testing/runs/<sactest run dir> [...]
"""
import argparse
import json
import os
import sys


def load(run, side):
    path = os.path.join(run, f"analysis_{side}.events.jsonl")
    games = set()
    sacs = []
    with open(path) as f:
        for line in f:
            e = json.loads(line)
            if e.get("type") == "sacrifice":
                sacs.append(e)
    # games analysed: from the analysis summary when present
    summ = os.path.join(run, f"analysis_{side}.json")
    n_games = None
    if os.path.exists(summ):
        try:
            d = json.load(open(summ))
            n_games = d.get("games_analysed")
        except Exception:
            pass
    return sacs, n_games


def brilliant(e, a):
    if e.get("category") not in ("piece", "exchange", "queen"):
        return False
    if e.get("book"):
        return False
    b, f = e.get("sf_before"), e.get("sf_after")
    if b is None or f is None or e.get("unsound"):
        return False
    return f - b >= -a.max_drop and b < a.max_before and f > a.min_after


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("runs", nargs="+")
    ap.add_argument("--max-drop", type=int, default=50)
    ap.add_argument("--max-before", type=int, default=300)
    ap.add_argument("--min-after", type=int, default=-100)
    ap.add_argument("--games", type=int, default=None, help="games per engine (if not in the analysis summary)")
    a = ap.parse_args()
    print("| run | engine | games | piece+ sacrifices | sound (SF) | brilliant-style | per game | games with >= 1 | games with >= 2 |")
    print("|---|---|---|---|---|---|---|---|---|")
    for run in a.runs:
        for side in ("new", "old"):
            sacs, n = load(run, side)
            n = a.games or n
            big = [e for e in sacs if e.get("category") in ("piece", "exchange", "queen")]
            sound = [e for e in big if e.get("sf_before") is not None and not e.get("unsound")]
            bril = [e for e in big if brilliant(e, a)]
            per = {}
            for e in bril:
                k = (e["file"], e["game_id"])
                per[k] = per.get(k, 0) + 1
            ge1 = sum(1 for v in per.values() if v >= 1)
            ge2 = sum(1 for v in per.values() if v >= 2)
            pg = f"{len(bril) / n:.2f}" if n else "?"
            print(f"| {os.path.basename(run.rstrip('/'))} | {side} | {n or '?'} | {len(big)} | {len(sound)} | {len(bril)} | {pg} | {ge1} | {ge2} |")


if __name__ == "__main__":
    sys.exit(main())
