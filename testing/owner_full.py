#!/usr/bin/env python3
"""OWNER-FULL brilliant scorer (portable, in-repo copy).

The metric of record for this project counts THREE badge shapes as brilliant:

  1. SACRIFICE   -- a sound sacrifice where the MOVED piece is the sacrificed piece.
                    "HONEST": stricter than the shipped/loose metric, which counts any event
                    where a piece dies nearby even if a different piece moved (~1.8x inflation).
  2. UNDERPROMO  -- promotion to N/B/R.
  3. IGNORED     -- ignoring an attack on a piece so it falls next move.

Shapes 2 and 3 are INVISIBLE to testing/arena's event stream by construction, so they cannot be
read out of the arm alone:
  * the arena files EVERY engine underpromotion as category="pawn" (analyze.rs: an engine
    promotion counts the 100cp pawn loss and the new piece's gain, so for N/B/R the pawn loss
    dominates -> net={P:+1} -> Category::Pawn), and score_tiers.tier() requires category in
    queen/exchange/piece -> 100% of them are rejected;
  * an ignored attack loses a DIFFERENT piece than the one that moved, which the sacrifice
    classifier never models.

So this tool does two things:
  (a) grades the sacrifice leg from an arm's graded events, on all three tiers; and
  (b) assembles the full owner number from externally-supplied per-game rates for the two blind
      shapes, given as "events/games" so the denominator is explicit.

*** DEDUPLICATION IS MANDATORY. *** On the 1500-game control arm, ignored-attack flags and loose
sacrifice events overlap (14 of 386 shared a (game_id, ply)); ignored-attack and HONEST sacrifice
overlap is 0 by construction. If you are ASSEMBLING from minted event lists, take the union on
(game_id, ply) keys. If you are assembling from already-divided per-game RATES, the overlap is
unrecoverable and the total is an upper bound — say so.

Usage
  python3 -I owner_full.py --arm-dir <dir-with-graded-events> [--name NAME] [--explain]
  python3 -I owner_full.py --arm-dir <dir> --underpromo 5 --ignored 52 --games 1500
  python3 -I owner_full.py --baseline
  python3 -I owner_full.py --selftest

Typical real invocation for an arm produced by /tmp/bmax/matchrun.sh:
  python3 -I owner_full.py --arm-dir /tmp/bmax/runs/<arm> --name <arm> \
      --underpromo 5 --ignored 52 --games 1500

This file is the portable copy; the analysis scripts it grew from live outside the repo
(/tmp/bmax/honest_score.py, /tmp/bmax/score_tiers.py, /tmp/ign/ignored_attack.py,
/tmp/bmax/underpromo_classify.py). Keep the two in step.
"""
from __future__ import annotations

import argparse
import json
import os
import re
import sys

# --------------------------------------------------------------------------- scoring

SCORE_RE = re.compile(r"^\s*([+-])(M?\d+(?:\.\d+)?)/(\d+)")

MATE_BOUND = 30_000  # cp above which a score is a mate score, not an eval


def _moved_letter(e: dict):
    """Piece letter of the piece that MOVED, from the event's SAN.

    Parses "14...h5" / "12.Bf4" / "f7f8n" style SAN. Returns None for castling (which the tier
    predicate rejects anyway) and 'P' for pawn moves (including promotions, where the tier
    predicate uses `category` rather than this).
    """
    san = e.get("san") or ""
    mv = san.split("...")[-1] if "." in san else san
    mv = mv.split(".", 1)[-1] if mv[:1].isdigit() else mv
    mv = mv.strip()
    c = mv[0] if mv else ""
    if c in "KQRBN":
        return c
    if c in "abcdefgh":
        return "P"
    return None


def loose(e: dict, max_drop: int = 50, min_after: int = -150) -> bool:
    """The shipped metric (== testing/score_brilliant.py, score_tiers.tier(e, 50, -150)).

    Counts an event where ANY piece of ours dies within a few plies, even if a different piece
    moved. Overcounts ~1.8x against `honest`.
    """
    san = e.get("san") or ""
    mv = san.split("...")[-1] if "." in san else san
    mv = mv.split(".", 1)[-1] if mv[:1].isdigit() else mv
    mv = mv.strip()
    if mv.startswith("K") or mv.startswith("O-O") or mv.startswith("0-0"):
        return False
    if e.get("category") not in ("queen", "exchange", "piece"):
        return False                       # a pawn sac is never a chess.com brilliant
    if e.get("mating"):
        return False
    net = e.get("net_lost") or {}
    if sum(v for v in net.values() if v > 0) < 1:
        return False
    b, f = e.get("sf_before"), e.get("sf_after")
    if b is None or f is None:
        return False
    if b >= 300:                           # already winning: not brilliant
        return False
    if f <= min_after:
        return False
    return f - b >= -max_drop


def honest(e: dict) -> bool:
    """chess.com fidelity: the MOVED piece must be the sacrificed piece."""
    if not loose(e):
        return False
    m = _moved_letter(e)
    return bool(m and (e.get("net_lost") or {}).get(m, 0) >= 1)


def strictest(e: dict) -> bool:
    """honest AND not immediately recouped."""
    return honest(e) and not e.get("recouped")


# --------------------------------------------------------------------------- assembly

# Settled on the 1500-game control arm bng_ctrl_s11 (see the two AUTHORITATIVE.txt files in
# /tmp/ign and /tmp/bmax). Reproduced here so the baseline is quotable without that tooling.
REF = {
    "arm": "bng_ctrl_s11",
    "games": 1500,
    "sac_honest": 63,           # 0.0420/g
    "sac_loose": 116,           # 0.0773/g  (shipped/overcounting metric)
    "up_badge_engine": 5,       # 0.0033/g  badge-shaped; 5..8 over Stockfish depth 12..16
    "up_detector_engine": 12,   # 0.0080/g  the detector's own (looser) criteria
    "up_total_both": 145,       # 0.0967/g  BOTH colours -- the mislabelled "0.0140/game"
    "ign_sound": 52,            # 0.0347/g  Stockfish-graded; 46..55 over depth 12..16
    "ign_structural": 386,      # 0.2573/g  candidates BEFORE the soundness gate
}


def funnel(evs, g):
    sac = [e for e in evs if e.get("type") == "sacrifice"]
    piece = lambda e: e.get("category") in ("queen", "exchange", "piece")  # noqa: E731
    notlost = lambda e: (e.get("sf_after") is not None and e["sf_after"] > -150)  # noqa: E731
    return [
        ("all sac events", sac),
        ("piece+ (not pawn)", [e for e in sac if piece(e)]),
        ("not mating", [e for e in sac if piece(e) and not e.get("mating")]),
        ("not already winning", [e for e in sac if piece(e) and not e.get("mating")
                                 and (e.get("sf_before") or 0) < 300]),
        ("not lost", [e for e in sac if piece(e) and not e.get("mating")
                      and (e.get("sf_before") or 0) < 300 and notlost(e)]),
        ("sound (LOOSE metric)", [e for e in sac if loose(e)]),
        ("HONEST (moved piece)", [e for e in sac if honest(e)]),
        ("STRICTEST (+not recouped)", [e for e in sac if strictest(e)]),
    ]


def load(path):
    if not os.path.exists(path):
        return None
    out = []
    for line in open(path):
        line = line.strip()
        if line:
            out.append(json.loads(line))
    return out


def find_events(arm_dir, name=None):
    """Locate the graded event files in an arm dir. The arena names the var side '_sac'."""
    name = name or os.path.basename(os.path.normpath(arm_dir))
    found = {}
    for side, suffix in (("ctl", "ctl"), ("var", "sac")):
        p = os.path.join(arm_dir, f"{name}_{suffix}.events.jsonl")
        evs = load(p)
        if evs is not None:
            found[side] = (p, evs)
    return found


def main():
    ap = argparse.ArgumentParser(description=__doc__,
                                 formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--arm-dir")
    ap.add_argument("--name")
    ap.add_argument("--games", type=int, default=0)
    ap.add_argument("--underpromo", type=float, default=None,
                    help="badge-shaped underpromotions, in EVENTS (not a rate)")
    ap.add_argument("--ignored", type=float, default=None,
                    help="SF-sound ignored attacks, in EVENTS (not a rate)")
    ap.add_argument("--explain", action="store_true")
    ap.add_argument("--baseline", action="store_true")
    ap.add_argument("--selftest", action="store_true")
    a = ap.parse_args()

    if a.selftest:
        return selftest()

    if a.baseline:
        g = REF["games"]
        tot = REF["sac_honest"] + REF["up_badge_engine"] + REF["ign_sound"]
        print(f"SHIPPED BEHAVIOUR -- owner-full baseline ({REF['arm']}, {g} games)\n")
        print("  sacrifice (HONEST, moved piece)   %3d  %.4f/g" % (REF["sac_honest"], REF["sac_honest"] / g))
        print("  underpromotion (badge-shaped)     %3d  %.4f/g" % (REF["up_badge_engine"], REF["up_badge_engine"] / g))
        print("  ignored attack (Stockfish-sound)  %3d  %.4f/g" % (REF["ign_sound"], REF["ign_sound"] / g))
        print("  " + "-" * 46)
        print("  OWNER-FULL TOTAL                  %3d  %.4f/g\n" % (tot, tot / g))
        print("  [reference only -- NOT the headline]")
        print("    loose sacrifice (shipped metric)   %3d  %.4f/g" % (REF["sac_loose"], REF["sac_loose"] / g))
        print("    ignored attack, STRUCTURAL         %3d  %.4f/g" % (REF["ign_structural"], REF["ign_structural"] / g))
        print("    underpromotion, detector-brilliant %2d  %.4f/g (engine side)" % (REF["up_detector_engine"], REF["up_detector_engine"] / g))
        print("    underpromotion, BOTH colours       %3d  %.4f/g <- the mislabelled 0.0140" % (REF["up_total_both"], REF["up_total_both"] / g))
        return

    if not a.arm_dir:
        raise SystemExit("need --arm-dir <dir> or --baseline")

    events = find_events(a.arm_dir, a.name)
    if not events:
        raise SystemExit(f"no graded event files in {a.arm_dir} (looked for *_ctl/_sac.events.jsonl)")

    g = a.games
    if not g:
        name = a.name or os.path.basename(os.path.normpath(a.arm_dir))
        sp = os.path.join(a.arm_dir, f"{name}.jsonl.summary.json")
        if os.path.exists(sp):
            d = json.load(open(sp))
            if not d.get("complete"):
                raise SystemExit(
                    f"REFUSING {name}: summary says complete={d.get('complete')} "
                    f"({d.get('games_played')}/{d.get('games_total')}). The arena snapshots the "
                    "jsonl with no completeness marker, so grading a running match yields a "
                    "partial that looks finished. Pass --games to override deliberately.")
            g = d["pairs"][0]["games"]
        else:
            raise SystemExit("no summary.json found; pass --games <n> explicitly")
    print(f"{a.name or a.arm_dir}: {g} games\n")

    rates = {}
    for side, (path, evs) in sorted(events.items()):
        r = {
            "sacs": sum(1 for e in evs if e.get("type") == "sacrifice") / g,
            "loose": sum(1 for e in evs if e.get("type") == "sacrifice" and loose(e)) / g,
            "honest": sum(1 for e in evs if e.get("type") == "sacrifice" and honest(e)) / g,
            "strictest": sum(1 for e in evs if e.get("type") == "sacrifice" and strictest(e)) / g,
        }
        rates[side] = r
        print("  %-4s sacs %.3f/g   LOOSE %.4f/g   HONEST %.4f/g   strictest %.4f/g"
              % (side, r["sacs"], r["loose"], r["honest"], r["strictest"]))

    if a.explain and "var" in events:
        print("\n  --- sacrifice-gate funnel (var side) ---")
        for label, es in funnel(events["var"][1], g):
            print("    %-28s %5d  %.4f/g" % (label, len(es), len(es) / g))

    if a.underpromo is not None or a.ignored is not None:
        side = "var" if "var" in rates else next(iter(rates))
        hon = rates[side]["honest"]
        up = (a.underpromo or 0) / g
        ig = (a.ignored or 0) / g
        print("\n  --- owner-full assembly (dedup applied where recoverable) ---")
        print("    sacrifice HONEST   %.4f/g" % hon)
        print("    underpromotion     %.4f/g  (%g events)" % (up, a.underpromo or 0))
        print("    ignored attack     %.4f/g  (%g events)" % (ig, a.ignored or 0))
        print("    " + "-" * 34)
        print("    OWNER-FULL TOTAL   %.4f/g" % (hon + up + ig))
        print("\n  NOTE: assembled from per-game rates, so any overlap between the ignored-attack")
        print("  and sacrifice legs is NOT subtracted. Ignored-vs-HONEST overlap is 0 by")
        print("  construction; ignored-vs-LOOSE was 14/386 on the control arm. Treat as an")
        print("  upper bound unless you minted the event lists and unioned on (game_id, ply).")


def selftest():
    """Pin the three predicates, including the two failure modes found in the real tooling."""
    ok = True

    def chk(cond, msg):
        nonlocal ok
        print(("  ok   " if cond else "  FAIL ") + msg)
        ok = ok and cond

    base = dict(category="piece", mating=False, sf_before=0, sf_after=-10,
                net_lost={"N": 1}, san="12.Nf3", recouped=False)
    chk(honest(base), "a piece move that loses a knight is HONEST")
    chk(not honest(dict(base, net_lost={"P": -1, "N": 1}, san="12.h5")),
        "a PAWN move that loses a knight is NOT honest (the moved piece must be the sacrifice)")
    chk(not honest(dict(base, net_lost={"B": 1})),
        "moving a knight while a BISHOP dies is NOT honest")
    chk(loose(dict(base, net_lost={"B": 1})) and not honest(dict(base, net_lost={"B": 1})),
        "that same event IS loose -> proves loose overcounts")
    chk(not honest(dict(base, category="pawn")), "category 'pawn' is never a brilliant")
    chk(not honest(dict(base, mating=True)), "a mating sacrifice is not a brilliant")
    chk(not honest(dict(base, sf_before=300)), "already winning (>=300) is rejected")
    chk(not honest(dict(base, sf_after=-150)), "leaves lost (<=-150) is rejected")
    chk(not honest(dict(base, san="12.O-O")), "castling is rejected")
    chk(not honest(dict(base, san="12.Ke2")), "a king move is rejected")
    chk(_moved_letter(dict(base, san="f7f8n")) == "P",
        "an underpromotion's SAN parses to moved-letter P (the PAWN moved)")
    # ...and that is exactly why the arena's category gate, not the moved-piece test, is what
    # excludes underpromotions: they arrive as category="pawn" and `loose` rejects the category.
    chk(not loose(dict(base, san="f7f8n", category="pawn")),
        "an underpromotion is rejected by CATEGORY (pawn), which is the arena's blind spot")
    chk(strictest(base) and not strictest(dict(base, recouped=True)),
        "strictest additionally requires not recouped")
    print("\n" + ("SELFTEST PASS" if ok else "SELFTEST FAIL"))
    return 0 if ok else 1


if __name__ == "__main__":
    sys.exit(main() or 0)
