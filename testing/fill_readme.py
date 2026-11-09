#!/usr/bin/env python3
"""Fill the measured tables of README.md from the testing results (stdlib only).

    python3 testing/fill_readme.py [--elo testing/results/elo_calibration.json]
                                   [--sac testing/results/sacrifice_report.json] [--readme README.md]

Replaces the text between `<!-- ELO_TABLE -->` / `<!-- /ELO_TABLE -->` and `<!-- SAC_TABLE -->` /
`<!-- /SAC_TABLE -->`. Every number written comes from the result files; a missing file leaves
the section saying so.
"""
import argparse
import json
import math
import os
import re

INTENDED = {1: "Extremely weak", 2: "Beginner", 3: "Beginner", 4: "Beginner", 5: "Lower intermediate",
            6: "Lower intermediate", 7: "Lower intermediate", 8: "Intermediate", 9: "Intermediate",
            10: "Intermediate", 11: "Upper intermediate", 12: "Upper intermediate", 13: "Upper intermediate",
            14: "Advanced", 15: "Advanced", 16: "Advanced", 17: "Strong", 18: "Strong", 19: "Strong",
            20: "Near full strength", 21: "Full engine strength"}


def fnum(x):
    return "n/a" if x is None or (isinstance(x, float) and math.isnan(x)) else f"{x:.0f}"


def ci(lo, hi, lo_unb=False, hi_unb=False):
    a = "−∞" if lo_unb or lo is None else f"{lo:.0f}"
    b = "+∞" if hi_unb or hi is None else f"{hi:.0f}"
    return f"[{a}, {b}]"


def elo_section(path):
    if not os.path.exists(path):
        return "_No calibration results yet: run `testing/calibrate.py` (see [Testing tools](#testing-tools))._"
    d = json.load(open(path))
    s3, env, st = d["stage3"], d["environment"], d["settings"]
    L = []
    L.append(f"Measured with `testing/calibrate.py` (status: **{d['status']}**, {d['generated']}). "
             "Every level played Stockfish 16 with `UCI_LimitStrength` (the only fixed ratings) and its neighbouring levels; "
             "all games were rated together by maximum likelihood with the Stockfish levels fixed at their nominal `UCI_Elo`.")
    L.append("")
    L.append("**These are estimates on the Stockfish 16 `UCI_Elo` scale at 10 s + 0.1 s on the test machine, with the default "
             "personality. They are not FIDE or human ratings, and not CCRL ratings.** See the caveats below.")
    L.append("")
    L.append("| Skill Level | Intended strength | Estimated Elo | 95% CI | Games (W/D/L) | Confidence |")
    L.append("|---:|:---|---:|:---:|:---:|:---|")
    for lv in s3["levels"]:
        n = lv["level"]
        if lv.get("elo") is None and lv.get("bound") is not None:
            est = f"{'<' if lv.get('bound_type') == 'upper' else '>'} {lv['bound']:.0f}"
        else:
            est = fnum(lv.get("elo"))
        c = ci(lv.get("ci_lo"), lv.get("ci_hi"), lv.get("ci_lo_unbounded"), lv.get("ci_hi_unbounded"))
        if lv.get("boot_note"):
            c += " (Fisher)"
        labels = ", ".join(lv.get("labels") or []) or "n/a"
        L.append(f"| {n} | {INTENDED.get(n, '')} | {est} | {c} | {lv['games']} ({lv['wins']}/{lv['draws']}/{lv['losses']}) | {labels} |")
    L.append("")
    inv = [m for m in s3.get("monotonicity", []) if m.get("diff", 0) < 0]
    if inv:
        txt = "; ".join(f"level {m['upper']} is {abs(m['diff']):.0f} below level {m['lower']} "
                        f"(probability of a real inversion by bootstrap {m['p_inversion'] * 100:.0f}%)" for m in inv)
        L.append(f"**Monotonicity:** strength rises with the level except: {txt}. Such small inversions lie within the confidence intervals.")
    else:
        L.append("**Monotonicity:** every level is rated above the one below it.")
    L.append("")
    L.append("Confidence labels: *anchored* = played the Stockfish anchors directly and lies inside their range (1320–2500); "
             "*extrapolated* = more than 100 below the lowest anchor, so the number rests on the chain of games between neighbouring "
             "levels and the logistic Elo model; *low-sample* = fewer than 100 games.")
    s4 = d.get("stage4")
    if s4 and s4.get("levels"):
        L.append("")
        L.append(f"**Time-control check.** Some levels were measured again at {s4['tc']} (the slower time control associated with "
                 "Stockfish's `UCI_Elo` calibration) against their nearest anchors:")
        L.append("")
        L.append(f"| Skill Level | Elo at {st['tc']} | Elo at {s4['tc']} | 95% CI at {s4['tc']} | Drift | Games (W/D/L) | Confidence |")
        L.append("|---:|---:|---:|:---:|---:|:---:|:---|")
        for lv in s4["levels"]:
            drift = "n/a" if lv.get("drift") is None else f"{lv['drift']:+.0f}" + (
                f" ± {lv['drift_halfwidth']:.0f}" if lv.get("drift_halfwidth") else " (CI unbounded)")
            L.append(f"| {lv['level']} | {fnum(lv.get('main_elo'))} | {fnum(lv.get('check_elo'))} | "
                     f"{ci(lv.get('check_ci_lo'), lv.get('check_ci_hi'))} | {drift} | {lv['games']} ({lv['wins']}/{lv['draws']}/{lv['losses']}) | "
                     f"{', '.join(lv.get('labels') or []) or 'n/a'} |")
        L.append("")
        sig = [lv for lv in s4["levels"] if lv.get("drift") is not None and lv.get("drift_halfwidth")
               and abs(lv["drift"]) > lv["drift_halfwidth"]]
        L.append("Drift = rating at the slower time control minus rating at the main one, both against the same fixed Stockfish "
                 "anchors; the ± is an approximate 95% interval. " +
                 (f"Drifts clearly different from zero: {', '.join('level %d (%+.0f)' % (lv['level'], lv['drift']) for lv in sig)}; "
                  "for the others the data cannot tell a change from none." if sig else
                  "None of the drifts with a finite interval is clearly different from zero at this sample size."))
    mem = env.get("memory", {}).get("MemTotal", "")
    m = re.match(r"(\d+)\s*kB", mem)
    if m:
        mem = f"{int(m.group(1)) / 1024 / 1024:.1f} GB"
    L.append("")
    L.append("**Test environment.** "
             f"{env.get('cpu_model')} ({env.get('nproc')} logical CPUs, {mem} RAM), {env.get('os')}; "
             f"{st['concurrency'] if 'concurrency' in st else env.get('concurrency')} games at a time, one thread per engine, 16 MB hash; "
             f"opponents {env.get('stockfish_id')} with `UCI_LimitStrength` at UCI_Elo {', '.join(str(a) for a in st['anchors'])}; "
             f"time control {st['tc']}; openings from `8moves_v3.pgn` (each played twice with colours reversed); "
             f"{s3['games']} games at {st['tc']}" + (f" plus {s4['games']} at {s4['tc']}" if s4 else "") +
             f"; engine sha256 `{env.get('engine_sha256', '')[:16]}…`. Full report: `testing/results/elo_calibration.md`.")
    L.append("")
    L.append("**Caveats.** (1) The scale is Stockfish's: its `UCI_Elo` was fitted by the Stockfish team against the Stash engine at "
             "120 s + 1 s and is described as roughly CCRL Blitz (older Stockfish READMEs say 60 s + 0.6 s, CCRL 40/4); at 10 s + 0.1 s "
             "on this machine the absolute numbers may be shifted. (2) Levels below 1320 are extrapolated through the chain. "
             "(3) The ratings belong to the default personality and this hardware; a different personality, time control or "
             "machine changes them, especially for levels 19–21, which use the clock. Re-run `testing/calibrate.py` to measure your setup.")
    return "\n".join(L)


def pct(x):
    return "n/a" if x is None else f"{100 * x:.1f}%"


def _prop(b):
    if not isinstance(b, dict) or not b.get("of"):
        return "n/a"
    lo, hi = (b.get("ci95") or [None, None])
    ci_txt = f" [{100 * lo:.0f}–{100 * hi:.0f}%]" if lo is not None else ""
    return f"{b['count']}/{b['of']} = {100 * b['rate']:.1f}%{ci_txt}"


def _mean(b, nd=2):
    if not isinstance(b, dict) or b.get("mean") is None:
        return "n/a"
    lo, hi = (b.get("ci95") or [None, None])
    ci_txt = f" [{lo:.{nd}f}–{hi:.{nd}f}]" if lo is not None else ""
    return f"{b['mean']:.{nd}f}{ci_txt}"


def sac_section(path):
    if not os.path.exists(path):
        return "_No sacrifice report yet: run `testing/sactest.py`._"
    d = json.load(open(path))
    m = d["meta"]
    new, old = d["overall"]["new"], d["overall"]["old"]
    eng = m["engines"]
    L = []
    L.append(f"Measured with `testing/sactest.py` ({m['started'][:10]}): the Ultimate Sacrificial Edition with the default "
             f"personality and Skill Level 21 (**new**, sha256 `{eng['aggro_new']['sha256'][:12]}…`) against AggroChess 4.5.0 "
             f"(**old**, `{eng['aggro_old']['sha256'][:12]}…`). Each played {m['games_per_anchor']} games against each of "
             f"Stockfish 16 at UCI_Elo {', '.join(str(a) for a in m['anchors'])} (same openings and colours for both), plus "
             f"{m['h2h_games']} games head-to-head; time control {m['tc']}, one thread each, {m['concurrency']} games at a time. "
             f"Every game was then analysed by `aggro_arena analyze`, with Stockfish checking each sacrifice.")
    L.append("")
    L.append("| Match | Games | W/D/L | Score | Elo difference [95% CI] |")
    L.append("|---|---:|:---:|---:|:---:|")
    for x in d["matches"]:
        a = x["engine_a"].replace("aggro_", "")
        b = x["engine_b"].replace("aggro_", "")
        el = "n/a" if x.get("elo") is None else f"{x['elo']:+.0f} [{fnum(x.get('elo_lo'))}, {fnum(x.get('elo_hi'))}]"
        L.append(f"| {a} vs {b} | {x['games']} | {x['wins']}/{x['draws']}/{x['losses']} | {100 * x['score']:.1f}% | {el} |")
    L.append("")
    pc = d.get("per_context", {})

    def per100(role, kind, ctx="balanced"):
        r = pc.get(role, {})
        mv = r.get("moves", {}).get(ctx, 0)
        if not mv:
            return "n/a"
        if kind == "given":
            n = r.get("sacrifices", {}).get(ctx, 0) + r.get("blunders", {}).get(ctx, 0)
        else:
            n = r.get(kind, {}).get(ctx, 0)
        return f"{100 * n / mv:.2f}"

    cat = lambda b, c: str((b.get("categories") or {}).get(c, 0))
    rows = [
        ("**Sacrifices per 100 engine moves in balanced positions**", per100("aggro_new", "sacrifices"), per100("aggro_old", "sacrifices")),
        ("Material given per 100 balanced moves (sacrifices + blunders)", per100("aggro_new", "given"), per100("aggro_old", "given")),
        ("Sacrifices in balanced positions, per game", _mean(new.get("balanced_sacrifices_per_game")), _mean(old.get("balanced_sacrifices_per_game"))),
        ("Games with a sacrifice in a balanced position", _prop(new.get("games_with_balanced_sacrifice")), _prop(old.get("games_with_balanced_sacrifice"))),
        ("Sacrifices per game (all positions)", _mean(new.get("sacrifices_per_game")), _mean(old.get("sacrifices_per_game"))),
        ("Queen / exchange / piece / pawn sacrifices",
         " / ".join(cat(new, c) for c in ("queen", "exchange", "piece", "pawn")),
         " / ".join(cat(old, c) for c in ("queen", "exchange", "piece", "pawn"))),
        ("Chained sacrifices (Tal chains)", str(new.get("chained_sacrifices", "n/a")), str(old.get("chained_sacrifices", "n/a"))),
        ("Average move number of a sacrifice", _mean(new.get("avg_move_number"), 1), _mean(old.get("avg_move_number"), 1)),
        ("Stockfish eval before the sacrifice (cp, engine's view)", _mean(new.get("avg_sf_before"), 0), _mean(old.get("avg_sf_before"), 0)),
        ("Sacrifices followed by mate within 30 plies", _prop(new.get("mates_after_sacrifice")), _prop(old.get("mates_after_sacrifice"))),
        ("Successful attacks (mate, material back with the attack still on, or Stockfish +1.5 twelve plies later)",
         _prop(new.get("successful_attacks")), _prop(old.get("successful_attacks"))),
        ("Objectively unsound (Stockfish: worse by 1.5+ pawns and below −1)", _prop(new.get("unsound")), _prop(old.get("unsound"))),
        ("…of those, practical chances (still scored, or Stockfish later swung to +1)", _prop(new.get("practical_chances")), _prop(old.get("practical_chances"))),
        ("Blunders per game (own eval collapsed after the reply)", _mean(new.get("blunders_per_game")), _mean(old.get("blunders_per_game"))),
        ("Score in all analysed games", _mean((new.get("result") or {}).get("score"), 3), _mean((old.get("result") or {}).get("score"), 3)),
    ]
    L.append("| Metric | new | old |")
    L.append("|---|---|---|")
    for lab, a, b in rows:
        L.append(f"| {lab} | {a} | {b} |")
    L.append("")
    L.append("Balanced = the engine's own score within ±3 pawns when it moved. Intervals are 95% (Wilson for proportions, "
             "normal for means); sacrifices within one game are treated as independent, so event-level intervals are "
             "somewhat optimistic. A *sacrifice* is a material deficit of about two pawns or more (one pawn for a gambit) "
             "that the engine started and that persists for two moves, which the engine's own evaluation saw coming; when its "
             "evaluation collapses instead, it is counted as a *blunder*. Moves made while being mated, forced moves and "
             "trades are excluded. The full report, with per-opponent and per-category tables and example games, is "
             "[`testing/results/sacrifice_report.md`](testing/results/sacrifice_report.md); the detection rules are in "
             "[`testing/README.md`](testing/README.md).")
    return "\n".join(L)


def replace(readme, tag, body):
    a, b = f"<!-- {tag} -->", f"<!-- /{tag} -->"
    i, j = readme.index(a) + len(a), readme.index(b)
    return readme[:i] + "\n" + body.strip() + "\n" + readme[j:]


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--elo", default="testing/results/elo_calibration.json")
    ap.add_argument("--sac", default="testing/results/sacrifice_report.json")
    ap.add_argument("--readme", default="README.md")
    ap.add_argument("--only", choices=["elo", "sac"])
    a = ap.parse_args()
    r = open(a.readme).read()
    if a.only in (None, "elo"):
        r = replace(r, "ELO_TABLE", elo_section(a.elo))
    if a.only in (None, "sac"):
        r = replace(r, "SAC_TABLE", sac_section(a.sac))
    open(a.readme, "w").write(r)
    print("README updated")


if __name__ == "__main__":
    main()
