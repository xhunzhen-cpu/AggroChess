#!/usr/bin/env python3
"""score_brilliant.py - strict, chess.com-like brilliant count from an analyze events file.

The arena's `analyze` classifier is deliberately loose (it flags castling and king moves
when material vanishes nearby, and counts sacrifices in won or lost positions). For the
goal "more moves a site would badge brilliant", score instead:

  * the moved piece is not the king, and the move is not castling
  * category is queen / exchange / piece (a pawn sac is not a brilliant)
  * the engine gave material by the analyzer's own accounting (net_lost >= 100)
  * Stockfish: the position was not already won (sf_before < 300) and the sacrifice is
    sound (not unsound; sf_after > -150; drop <= 50 cp)
  * the engine is not mating at that moment (material is irrelevant in a mate)

Prints per-file counts plus per game. Usage:
  python3 testing/score_brilliant.py <events.jsonl> [...]
"""
import json
import os
import sys

WIN_BEFORE = 300      # already winning: the sac is not brave
MIN_AFTER = -150      # do not leave a lost position
MAX_DROP = 50         # sound: the eval may fall at most this much


def strict(e):
    san = e.get("san") or ""
    # SAN may be prefixed with "12." or "12..." - strip it, then the piece letter leads
    mv = san.split("...")[-1] if "." in san else san
    mv = mv.split(".", 1)[-1] if mv[:1].isdigit() else mv
    mv = mv.strip()
    if mv.startswith("K") or mv.startswith("O-O") or mv.startswith("0-0"):
        return False
    if e.get("category") not in ("queen", "exchange", "piece"):
        return False
    if e.get("mating"):
        return False
    net = e.get("net_lost") or {}
    if sum(v for v in net.values() if v > 0) < 1:
        return False
    b, f = e.get("sf_before"), e.get("sf_after")
    if b is None or f is None:
        return False
    if e.get("unsound"):
        return False
    return f - b >= -MAX_DROP and b < WIN_BEFORE and f > MIN_AFTER


def main():
    for path in sys.argv[1:]:
        evs = [json.loads(l) for l in open(path)]
        sacs = [e for e in evs if e.get("type") == "sacrifice"]
        games = {e["game_id"] for e in sacs}
        big = [e for e in sacs if e.get("category") in ("queen", "exchange", "piece")]
        br = [e for e in sacs if strict(e)]
        brg = {e["game_id"] for e in br}
        n = os.path.basename(path).split("_sac")[0]
        print(f"{n:24} games={len(games):>3} sacs={len(sacs):>3} piece+={len(big):>3} "
              f"strict_brilliant={len(br):>3} games_with={len(brg):>3}")
        if br:
            for e in sorted(br, key=lambda x: (x["game_id"], x["ply"]))[:12]:
                print(f"    g{e['game_id']:>2} {e['san']:12} {e['category']:8} "
                      f"sf {e['sf_before']}->{e['sf_after']}")


if __name__ == "__main__":
    main()
