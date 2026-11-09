#!/usr/bin/env python3
"""calibrate.py -- Elo calibration of AggroChess 'Skill Level' against Stockfish UCI_Elo anchors.

stdlib only. All games are played by `aggro_arena match` (testing/arena) from configs generated
into a run directory (default testing/runs/calib-<timestamp>/); ratings come from elo.py.

Stages
  1 scout   every level plays --scout-games games vs each of the --scout-anchors Stockfish anchors
            nearest to its prior Elo (--prior, a level->Elo map) and vs the next level in the list.
            A joint MLE over these games gives the scout estimate used for scheduling.
  2 main    every level gets ~--games games: --anchor-share of them vs the (up to) --main-anchors
            anchors whose nominal Elo is within +-(--anchor-window) of its scout estimate, the rest
            vs adjacent levels in the list (L+-1 weight 2, L+-2 weight 1). Levels with no anchor
            in the window (e.g. far below Stockfish's 1320 floor) play only the chain.
  3 fit     joint MLE over ALL main-TC games (stages 1+2), anchors fixed; game-pair bootstrap CIs
            (headline) + Fisher CIs; confidence labels; monotonicity check.
  4 tc      optional (--tc-check 60+0.6): --tc-levels re-measured at that TC vs their nearest
            anchors (levels with no anchor in the window are chained to the next stronger checked
            level); drift = rating at the check TC - rating at the main TC.

The engine binary (and --engine-files, e.g. the gambit book) is COPIED into <run>/engine/ at the
start, so rebuilding the engine during a long calibration does not affect it; sha256 sums are
recorded. Engines under test are named <prefix>_L<level>, anchors sf<elo> (never the same name).

Typical use
  python3 testing/calibrate.py                                  # full run, ~3,400 games at 10+0.1
  python3 testing/calibrate.py --tc-check 60+0.6 --tc-games 100 # + hybrid TC check
  python3 testing/calibrate.py --resume --run-dir testing/runs/calib-XXXX   # continue
  python3 testing/calibrate.py --report-only --run-dir ...      # re-fit + rewrite the report
  python3 testing/calibrate.py --quick                          # smoke test (levels 1,11,21; tiny)
  python3 testing/calibrate.py --dry-run                        # print the stage-1 configs
  # Stockfish's own 'Skill Level' 0..20 as a stand-in engine under test:
  python3 testing/calibrate.py --engine /usr/games/stockfish --engine-files '' \
      --levels 0-20 --prior 0:1300,20:2900 --name-prefix sfskill
"""
from __future__ import annotations

import argparse
import datetime
import hashlib
import json
import math
import os
import platform
import random
import re
import shutil
import signal
import socket
import subprocess
import sys
import time

HERE = os.path.dirname(os.path.abspath(__file__))
REPO = os.path.dirname(HERE)
sys.path.insert(0, HERE)
import elo  # noqa: E402

# ---------------------------------------------------------------- settings

DEFAULTS = {
    "engine": os.path.join(REPO, "target", "release", "aggro_chess"),
    "engine_cwd": REPO,
    "engine_files": "ph-gambitbook.bin",
    "engine_option": [],
    "level_option": "Skill Level",
    "name_prefix": None,
    "sf": "/usr/games/stockfish",
    "arena": os.path.join(HERE, "arena", "target", "release", "aggro_arena"),
    "openings": os.path.join(HERE, "openings", "8moves_v3.txt"),
    "tc": "10+0.1",
    "games": 200,
    "scout_games": 10,
    "scout_anchors": 2,
    "main_anchors": 2,
    "anchor_window": 350.0,
    "anchor_share": 0.5,
    "chain": 2,
    "concurrency": 14,
    "levels": "1-21",
    "anchors": "1320,1500,1700,1900,2100,2300,2500",
    "prior": "1:400,21:2100",
    "seed": 1,
    "batch_games": None,          # default 20 * concurrency (fixed when the run is created)
    "bootstrap": 200,
    "prior_draws": elo.DEFAULT_PRIOR_DRAWS,
    "min_games": 100,
    "extrap_margin": 100.0,
    "tc_check": None,
    "tc_levels": "1,6,11,16,21",
    "tc_games": 100,
}
QUICK = {
    "levels": "1,11,21",
    "scout_games": 4,
    "games": 12,
    "tc": "2+0.05",
    "concurrency": 5,
    "tc_levels": "1,11,21",
    "tc_games": 4,
}
# settings that define the schedule: may not change when resuming a run
LOCKED = {"engine", "engine_files", "engine_option", "level_option", "name_prefix", "sf", "openings", "tc",
          "games", "scout_games", "scout_anchors", "main_anchors", "anchor_window", "anchor_share", "chain",
          "levels", "anchors", "prior", "seed", "batch_games", "quick"}
ANCHOR_PREFIX = "sf"
# The scout fit is only used for scheduling. It uses the same tiny prior as the final fit (a firmer
# one - 1 virtual draw per pair - pulled the scout estimates of levels far below sf1320 up by
# +100..+500 Elo, so the +-anchor_window test was applied to wrong numbers and hopeless levels got
# anchor games). A level whose scout MLE is infinite (0% / 100%) is placed at its one-sided 95%
# bound, or at the prior map value if that is more extreme (see scout_estimates).
SCOUT_PRIOR_DRAWS = elo.DEFAULT_PRIOR_DRAWS


class CalibrationError(Exception):
    pass


def parse_int_list(s: str) -> list:
    out = set()
    for part in str(s).replace(" ", "").split(","):
        if not part:
            continue
        m = re.fullmatch(r"(-?\d+)-(-?\d+)", part)
        if m:
            a, b = int(m.group(1)), int(m.group(2))
            if a > b:
                a, b = b, a
            out.update(range(a, b + 1))
        else:
            out.add(int(part))
    return sorted(out)


def parse_prior(s: str) -> list:
    pts = []
    for part in str(s).replace(" ", "").split(","):
        if part:
            lv, e = part.split(":")
            pts.append([float(lv), float(e)])
    if not pts:
        raise ValueError("empty prior map")
    return sorted(pts)


def prior_elo(points: list, level: float) -> float:
    """Piecewise-linear level->Elo map, extrapolated linearly beyond the end points."""
    if len(points) == 1:
        return points[0][1]
    if level <= points[0][0]:
        (x0, y0), (x1, y1) = points[0], points[1]
    elif level >= points[-1][0]:
        (x0, y0), (x1, y1) = points[-2], points[-1]
    else:
        for (x0, y0), (x1, y1) in zip(points, points[1:]):
            if x0 <= level <= x1:
                break
    if x1 == x0:
        return y0
    return y0 + (y1 - y0) * (level - x0) / (x1 - x0)


def parse_tc(s: str) -> str:
    s = str(s).strip()
    if not re.fullmatch(r"\d+(\.\d+)?\+\d+(\.\d+)?", s):
        raise ValueError(f"time control must look like 10+0.1, got {s!r}")
    return s


def normalise(st: dict) -> dict:
    st = dict(st)
    for k in ("engine", "engine_cwd", "sf", "arena", "openings"):
        if st.get(k) and ("/" in st[k] or os.path.exists(st[k])):
            st[k] = os.path.abspath(st[k])
    if st["sf"] and "/" not in st["sf"]:
        st["sf"] = shutil.which(st["sf"]) or st["sf"]
    st["levels"] = parse_int_list(st["levels"]) if not isinstance(st["levels"], list) else st["levels"]
    st["anchors"] = parse_int_list(st["anchors"]) if not isinstance(st["anchors"], list) else st["anchors"]
    st["prior"] = parse_prior(st["prior"]) if not isinstance(st["prior"], list) else st["prior"]
    st["tc_levels"] = parse_int_list(st["tc_levels"]) if not isinstance(st["tc_levels"], list) else st["tc_levels"]
    st["tc"] = parse_tc(st["tc"])
    if st.get("tc_check"):
        st["tc_check"] = parse_tc(st["tc_check"])
    if isinstance(st["engine_files"], str):
        st["engine_files"] = [f for f in st["engine_files"].replace(",", " ").split() if f]
    st["engine_option"] = [o.strip() for o in st["engine_option"]]
    for o in st["engine_option"]:
        if "=" not in o:
            raise ValueError(f"--engine-option needs Name=Value, got {o!r}")
    if not st.get("name_prefix"):
        base = os.path.basename(st["engine"]).split(".")[0].split("_")[0]
        st["name_prefix"] = re.sub(r"[^A-Za-z0-9]", "", base) or "eng"
    if not re.fullmatch(r"[A-Za-z0-9_-]+", st["name_prefix"]):
        raise ValueError("--name-prefix may only contain letters, digits, '_' and '-'")
    if st["batch_games"] is None:
        st["batch_games"] = max(40, 20 * int(st["concurrency"]))
    for k in ("games", "scout_games", "scout_anchors", "main_anchors", "chain", "concurrency", "seed",
              "batch_games", "bootstrap", "min_games", "tc_games"):
        st[k] = int(st[k])
    for k in ("anchor_window", "anchor_share", "prior_draws", "extrap_margin"):
        st[k] = float(st[k])
    if not st["levels"]:
        raise ValueError("no levels")
    if not st["anchors"]:
        raise ValueError("no anchors")
    if not (0.0 <= st["anchor_share"] <= 1.0):
        raise ValueError("--anchor-share must be in [0, 1]")
    if st["concurrency"] < 1:
        raise ValueError("--concurrency must be >= 1")
    return st


# ---------------------------------------------------------------- names and scheduling

def level_name(st: dict, level: int) -> str:
    return f"{st['name_prefix']}_L{level:02d}"


def anchor_name(elo_: int) -> str:
    return f"{ANCHOR_PREFIX}{elo_}"


def round_even(x: float, minimum: int = 2) -> int:
    n = int(math.floor(x / 2.0 + 0.5)) * 2
    return max(minimum, n)


def nearest_anchors(est: float, anchors: list, k: int, window=None) -> list:
    """Up to k anchors closest to est (ties -> the lower anchor), optionally within +-window."""
    cand = sorted(anchors, key=lambda a: (abs(a - est), a))
    if window is not None:
        cand = [a for a in cand if abs(a - est) <= window]
    return sorted(cand[:k])


def plan_scout(st: dict) -> tuple:
    levels = st["levels"]
    pairings, info = [], {}
    g = round_even(st["scout_games"])
    for lv in levels:
        pe = prior_elo(st["prior"], lv)
        near = nearest_anchors(pe, st["anchors"], st["scout_anchors"])
        info[str(lv)] = {"prior": pe, "anchors": near}
        for a in near:
            pairings.append([level_name(st, lv), anchor_name(a), g])
    for lo, hi in zip(levels, levels[1:]):
        pairings.append([level_name(st, lo), level_name(st, hi), g])
    return pairings, info


def plan_main(st: dict, est: dict) -> tuple:
    """est: {level: scout estimate}. Returns (pairings, per-level info)."""
    levels = st["levels"]
    depth = max(0, st["chain"])
    G = st["games"]
    shares, info, pairings = {}, {}, []
    near_all = {lv: nearest_anchors(est[lv], st["anchors"], st["main_anchors"], st["anchor_window"]) for lv in levels}
    # The chain links all levels into one group (depth >= 1). If no level of the group has an anchor
    # in its window, the stage-2 games would not touch any anchor: the level closest to an anchor then
    # plays its nearest anchor although it is outside the window (like a lone level below).
    forced_lv = None
    if depth >= 1 and len(levels) > 1 and not any(near_all.values()):
        forced_lv = min(levels, key=lambda lv: (abs(nearest_anchors(est[lv], st["anchors"], 1)[0] - est[lv]), lv))
    for i, lv in enumerate(levels):
        e = est[lv]
        near = near_all[lv]
        nbrs = [(levels[i + d], depth + 1 - abs(d)) for d in range(-depth, depth + 1)
                if d != 0 and 0 <= i + d < len(levels)]
        forced = False
        if (not near and not nbrs) or lv == forced_lv:  # far from every anchor: use the nearest one anyway
            near = nearest_anchors(e, st["anchors"], 1)
            forced = True
        if near and nbrs:
            anchor_total = G * st["anchor_share"]
        elif near:
            anchor_total = G
        else:
            anchor_total = 0.0
        per_anchor = round_even(anchor_total / len(near)) if near and anchor_total > 0 else 0
        chain_total = G - anchor_total
        wsum = sum(w for _, w in nbrs)
        shares[lv] = {m: chain_total * w / wsum for m, w in nbrs} if nbrs and chain_total > 0 else {}
        info[str(lv)] = {"estimate_used": e, "anchors": near if per_anchor else [], "games_per_anchor": per_anchor,
                         "forced_anchor": forced, "chain": {}}
        for a in (near if per_anchor else []):
            pairings.append([level_name(st, lv), anchor_name(a), per_anchor])
    for i, lv in enumerate(levels):
        for d in range(1, depth + 1):
            if i + d >= len(levels):
                break
            m = levels[i + d]
            want = max(shares[lv].get(m, 0.0), shares[m].get(lv, 0.0))
            if want <= 0:
                continue
            n = round_even(want)
            pairings.append([level_name(st, lv), level_name(st, m), n])
            info[str(lv)]["chain"][str(m)] = n
            info[str(m)]["chain"][str(lv)] = n
    for lv in levels:
        d = info[str(lv)]
        d["planned_games"] = d["games_per_anchor"] * len(d["anchors"]) + sum(d["chain"].values())
    return pairings, info


def _groups(nodes: list, edges: list) -> list:
    parent = {n: n for n in nodes}

    def find(x):
        while parent[x] != x:
            parent[x] = parent[parent[x]]
            x = parent[x]
        return x
    for a, b in edges:
        parent[find(a)] = find(b)
    out = {}
    for n in nodes:
        out.setdefault(find(n), []).append(n)
    return sorted(out.values())


def plan_tc_check(st: dict, est: dict) -> tuple:
    """est: {level: stage-3 estimate} (rated levels only).

    A checked level plays the (up to --main-anchors) anchors within +-anchor_window of its
    estimate; one without such an anchor is bridged to the next stronger checked level (the
    strongest one to its nearest checked level). Every resulting group of bridged levels is then
    tied to an anchor: if no member has an anchor in its window, either the member closest to
    any anchor plays that anchor although it is outside the window ('forced_anchor'), or a member
    is bridged to the nearest checked level that has anchors - whichever Elo gap is smaller.
    Without this a group could be bridged only to itself (e.g. --tc-levels 1,6 with both levels
    below the window) and its ratings would float, centred on 0."""
    lv_list = [lv for lv in st["tc_levels"] if lv in est]
    g = st["tc_games"]
    info, edges = {}, []
    near = {lv: nearest_anchors(est[lv], st["anchors"], st["main_anchors"], st["anchor_window"]) for lv in lv_list}
    for lv in lv_list:
        e = est[lv]
        if near[lv]:
            info[str(lv)] = {"estimate_main": e, "anchors": near[lv], "games_per_anchor": round_even(g / len(near[lv])),
                             "bridge": None, "bridges": [], "forced_anchor": False}
            continue
        others = [m for m in lv_list if m != lv]
        stronger = sorted((m for m in others if est[m] > e), key=lambda m: est[m] - e)
        bridge = stronger[0] if stronger else (min(others, key=lambda m: abs(est[m] - e)) if others else None)
        info[str(lv)] = {"estimate_main": e, "anchors": [], "games_per_anchor": 0, "bridge": bridge,
                         "bridges": [bridge] if bridge is not None else [], "forced_anchor": False}
        if bridge is not None:
            edges.append(tuple(sorted((lv, bridge))))
    anchored = [lv for lv in lv_list if near[lv]]
    for comp in _groups(lv_list, edges):
        if any(near[m] for m in comp):
            continue
        best = None  # (gap, kind, member, target)
        for m in comp:
            a = nearest_anchors(est[m], st["anchors"], 1)[0]
            cand = (abs(a - est[m]), 0, m, a)
            best = cand if best is None or cand < best else best
            for x in anchored:
                cand = (abs(est[x] - est[m]), 1, m, x)
                best = cand if cand < best else best
        _, kind, m, target = best
        if kind == 0:
            info[str(m)].update({"anchors": [target], "games_per_anchor": round_even(g), "forced_anchor": True})
        else:
            edges.append(tuple(sorted((m, target))))
            info[str(m)]["bridges"].append(target)
            if info[str(m)]["bridge"] is None:
                info[str(m)]["bridge"] = target
    pairings = []
    for lv in lv_list:
        d = info[str(lv)]
        for a in d["anchors"]:
            pairings.append([level_name(st, lv), anchor_name(a), d["games_per_anchor"]])
    for a, b in sorted(set(edges)):
        pairings.append([level_name(st, a), level_name(st, b), round_even(g)])
    skipped = [lv for lv in st["tc_levels"] if lv not in est]
    return pairings, info, skipped


def make_batches(pairings: list, batch_games: int) -> list:
    """Pack pair lines (split if larger than a batch) into batches of <= batch_games games."""
    lines = []
    for a, b, n in pairings:
        while n > 0:
            take = min(n, max(2, batch_games - batch_games % 2))
            lines.append([a, b, take])
            n -= take
    batches, cur, cur_n = [], [], 0
    for ln in lines:
        if cur and cur_n + ln[2] > batch_games:
            batches.append(cur)
            cur, cur_n = [], 0
        cur.append(ln)
        cur_n += ln[2]
    if cur:
        batches.append(cur)
    return batches


# ---------------------------------------------------------------- utilities

def sha256(path: str):
    try:
        h = hashlib.sha256()
        with open(path, "rb") as fh:
            for chunk in iter(lambda: fh.read(1 << 20), b""):
                h.update(chunk)
        return h.hexdigest()
    except OSError:
        return None


def now_utc() -> str:
    return datetime.datetime.now(datetime.timezone.utc).strftime("%Y-%m-%d %H:%M:%S UTC")


def run_cmd(cmd, timeout=10, cwd=None, inp=None):
    try:
        r = subprocess.run(cmd, input=inp, capture_output=True, text=True, timeout=timeout, cwd=cwd)
        return r.stdout
    except (OSError, subprocess.SubprocessError):
        return ""


def uci_id(path: str, cwd=None):
    out = run_cmd([path], timeout=15, cwd=cwd, inp="uci\nisready\nquit\n")
    for line in out.splitlines():
        if line.startswith("id name "):
            return line[len("id name "):].strip()
    return None


def uci_options(path: str, cwd=None) -> list:
    out = run_cmd([path], timeout=15, cwd=cwd, inp="uci\nisready\nquit\n")
    names = []
    for line in out.splitlines():
        m = re.match(r"option name (.+?) type ", line)
        if m:
            names.append(m.group(1))
    return names


def environment(st: dict, run_dir: str) -> dict:
    lscpu = run_cmd(["lscpu"])
    model = next((ln.split(":", 1)[1].strip() for ln in lscpu.splitlines() if ln.startswith("Model name:")), None)
    nproc = run_cmd(["nproc"]).strip() or str(os.cpu_count())
    mem = {}
    try:
        with open("/proc/meminfo") as fh:
            for ln in fh:
                k, v = ln.split(":", 1)
                if k in ("MemTotal", "MemAvailable"):
                    mem[k] = v.strip()
    except OSError:
        pass
    eng_copy = os.path.join(run_dir, "engine", os.path.basename(st["engine"]))
    env = {
        "date": now_utc(),
        "host": socket.gethostname(),
        "cpu_model": model,
        "nproc": int(nproc) if nproc.isdigit() else nproc,
        "loadavg_at_start": list(os.getloadavg()),
        "memory": mem,
        "os": platform.platform(),
        "python": sys.version.split()[0],
        "engine_source": st["engine"],
        "engine_copy": eng_copy,
        "engine_sha256": sha256(eng_copy) if os.path.exists(eng_copy) else sha256(st["engine"]),
        "engine_id": uci_id(eng_copy, os.path.dirname(eng_copy)) if os.path.exists(eng_copy) else uci_id(st["engine"], st["engine_cwd"]),
        "engine_files": {f: sha256(os.path.join(run_dir, "engine", f)) for f in st["engine_files"]
                         if os.path.exists(os.path.join(run_dir, "engine", f))},
        "stockfish": st["sf"],
        "stockfish_sha256": sha256(st["sf"]),
        "stockfish_id": uci_id(st["sf"]),
        "arena": st["arena"],
        "arena_sha256": sha256(os.path.join(run_dir, "bin", "aggro_arena")) or sha256(st["arena"]),
        "openings": st["openings"],
        "openings_sha256": sha256(st["openings"]),
        "tc": st["tc"],
        "tc_check": st.get("tc_check"),
        "games_per_level": st["games"],
        "scout_games": st["scout_games"],
        "concurrency": st["concurrency"],
    }
    git = run_cmd(["git", "-C", REPO, "rev-parse", "HEAD"]).strip()
    if git:
        env["aggrochess_git_head"] = git
        env["aggrochess_git_branch"] = run_cmd(["git", "-C", REPO, "rev-parse", "--abbrev-ref", "HEAD"]).strip()
        env["aggrochess_src_dirty_files"] = len(run_cmd(["git", "-C", REPO, "status", "--porcelain", "--", "src"]).splitlines())
    return env


def write_json(path: str, obj) -> None:
    tmp = path + ".tmp"
    with open(tmp, "w", encoding="utf-8") as fh:
        json.dump(elo._json_clean(obj), fh, indent=1)
        fh.write("\n")
    os.replace(tmp, path)


def read_json(path: str):
    with open(path, encoding="utf-8") as fh:
        return json.load(fh)


# ---------------------------------------------------------------- configs and batches

def engine_block(st: dict, name: str, level=None, anchor=None) -> list:
    lines = []
    if level is not None:
        exe = os.path.basename(st["engine"])
        lines += [f"engine {name} ../engine/{exe}", f"cwd {name} ../engine"]
        opts = [("Threads", "1"), ("Hash", "16")]
        for o in st["engine_option"]:
            k, v = o.split("=", 1)
            k, v = k.strip(), v.strip()
            opts = [(kk, vv) for kk, vv in opts if kk.lower() != k.lower()] + [(k, v)]
        opts = [(kk, vv) for kk, vv in opts if kk.lower() != st["level_option"].lower()]
        opts.append((st["level_option"], str(level)))
    else:
        lines.append(f"engine {name} {st['sf']}")
        opts = [("Threads", "1"), ("Hash", "16"), ("UCI_LimitStrength", "true"), ("UCI_Elo", str(anchor))]
    lines += [f"option {name} {k} = {v}" for k, v in opts]
    return lines


def engine_of(st: dict, name: str) -> tuple:
    if name.startswith(st["name_prefix"] + "_L"):
        return ("level", int(name[len(st["name_prefix"]) + 2:]))
    m = re.fullmatch(ANCHOR_PREFIX + r"(\d+)", name)
    if m:
        return ("anchor", int(m.group(1)))
    raise CalibrationError(f"unknown engine name {name}")


def config_text(st: dict, stage: str, tc: str, batch: dict, concurrency: int) -> str:
    names = []
    for a, b, _ in batch["pairs"]:
        for n in (a, b):
            if n not in names:
                names.append(n)
    out = [f"# calibrate.py stage {stage} batch {batch['name']} -- generated; --resume compares this file",
           f"event AggroChess calibration {stage} {batch['name']}",
           f"tc {tc}",
           f"concurrency {concurrency}",
           f"openings {batch['name']}.openings",
           f"seed {batch['seed']}",
           f"pgn {batch['name']}.pgn",
           f"jsonl {batch['name']}.jsonl", ""]
    for n in names:
        kind, v = engine_of(st, n)
        out += engine_block(st, n, level=v if kind == "level" else None, anchor=v if kind == "anchor" else None)
        out.append("")
    out += [f"pair {a} {b} {g}" for a, b, g in batch["pairs"]]
    return "\n".join(out) + "\n"


def _norm_cfg(text: str) -> str:
    return "\n".join(ln for ln in text.splitlines() if not ln.startswith("concurrency ") and not ln.startswith("#"))


def batch_status(stage_dir: str, batch: dict) -> tuple:
    """(complete, records found). Complete = every pair line has exactly its planned games
    between the right engines."""
    path = os.path.join(stage_dir, batch["name"] + ".jsonl")
    if not os.path.exists(path):
        return False, 0
    counts = {}
    total = 0
    with open(path, encoding="utf-8") as fh:
        for line in fh:
            try:
                rec = json.loads(line)
                pi = rec["pairing"]
                names = {rec["white"], rec["black"]}
                rec["result"]
            except (ValueError, KeyError, TypeError):
                continue
            if not (0 <= pi < len(batch["pairs"])) or names != set(batch["pairs"][pi][:2]):
                return False, total
            counts[pi] = counts.get(pi, 0) + 1
            total += 1
    ok = all(counts.get(i, 0) == p[2] for i, p in enumerate(batch["pairs"])) and total == batch["games"]
    return ok, total


class Runner:
    def __init__(self, st: dict, run_dir: str, resume: bool):
        self.st = st
        self.run_dir = run_dir
        self.resume = resume
        self._perm = None
        self.proc = None  # running aggro_arena (for signal forwarding)
        local = os.path.join(run_dir, "bin", "aggro_arena")
        self.arena = local if os.path.exists(local) else st["arena"]

    def opening_slice(self, start: int, count: int) -> list:
        if self._perm is None:
            with open(self.st["openings"], encoding="utf-8") as fh:
                self._lines = [ln.rstrip("\n") for ln in fh if ln.strip()]
            self._perm = list(range(len(self._lines)))
            random.Random(self.st["seed"]).shuffle(self._perm)
        n = len(self._perm)
        if start + count > n:
            print(f"warning: opening suite exhausted ({n} openings); reusing openings", flush=True)
        return [self._lines[self._perm[(start + i) % n]] for i in range(count)]

    def make_plan(self, stage: str, tc: str, pairings: list, opening_start: int, extra: dict) -> dict:
        batches = []
        off = opening_start
        for i, pairs in enumerate(make_batches(pairings, self.st["batch_games"])):
            games = sum(p[2] for p in pairs)
            batches.append({"name": f"b{i + 1:02d}", "pairs": pairs, "games": games,
                            "openings_start": off, "openings_count": games // 2, "seed": self.st["seed"]})
            off += games // 2
        plan = {"stage": stage, "tc": tc, "created": now_utc(), "pairings": pairings, "batches": batches,
                "games": sum(b["games"] for b in batches), "openings_end": off}
        plan.update(extra)
        return plan

    def stage_dir(self, stage: str) -> str:
        return os.path.join(self.run_dir, stage)

    def load_or_create_plan(self, stage: str, factory) -> dict:
        d = self.stage_dir(stage)
        path = os.path.join(d, "plan.json")
        if os.path.exists(path):
            return read_json(path)
        plan = factory()
        os.makedirs(d, exist_ok=True)
        write_json(path, plan)
        return plan

    def run_stage(self, plan: dict) -> None:
        stage, d = plan["stage"], self.stage_dir(plan["stage"])
        total = plan["games"]
        done_before = 0
        for b in plan["batches"]:
            cfg_path = os.path.join(d, b["name"] + ".cfg")
            text = config_text(self.st, stage, plan["tc"], b, self.st["concurrency"])
            ok, n = batch_status(d, b)
            if ok:
                same = False
                if os.path.exists(cfg_path):
                    with open(cfg_path, encoding="utf-8") as fh:
                        same = _norm_cfg(fh.read()) == _norm_cfg(text)
                if same:
                    print(f"[{stage} {b['name']}] complete ({n} games) - reusing", flush=True)
                    done_before += b["games"]
                    continue
                print(f"[{stage} {b['name']}] config missing or changed - replaying", flush=True)
            elif n:
                print(f"[{stage} {b['name']}] incomplete ({n}/{b['games']} games) - replaying the batch", flush=True)
            with open(os.path.join(d, b["name"] + ".openings"), "w", encoding="utf-8") as fh:
                fh.write("\n".join(self.opening_slice(b["openings_start"], b["openings_count"])) + "\n")
            with open(cfg_path, "w", encoding="utf-8") as fh:
                fh.write(text)
            self.run_arena(cfg_path, os.path.join(d, b["name"] + ".log"), f"{stage} {b['name']}",
                           done_before, total)
            ok, n = batch_status(d, b)
            if not ok:
                raise CalibrationError(f"{stage} {b['name']}: only {n}/{b['games']} valid games recorded "
                                       f"(see {os.path.join(d, b['name'] + '.log')})")
            done_before += b["games"]

    def run_arena(self, cfg: str, log_path: str, label: str, done_before: int, total: int) -> None:
        print(f"[{label}] aggro_arena match {cfg}", flush=True)
        with open(log_path, "w", encoding="utf-8") as log:
            proc = subprocess.Popen([self.arena, "match", cfg], stdout=subprocess.PIPE,
                                    stderr=subprocess.STDOUT, text=True, bufsize=1, cwd=os.path.dirname(cfg))
            self.proc = proc
            done = done_before
            try:
                for line in proc.stdout:
                    log.write(line)
                    log.flush()
                    if line.startswith("game "):
                        done += 1
                        print(f"[{label} {done}/{total}] {line.rstrip()}", flush=True)
                    else:
                        print(f"[{label}] {line.rstrip()}", flush=True)
                rc = proc.wait()
            except KeyboardInterrupt:
                # A terminal Ctrl-C reaches the arena too (same process group) and SIGTERM/SIGHUP are
                # forwarded by the handler in main(); the arena then abandons the running games and
                # exits. Only if it is still alive after 5 s is it signalled (again) / killed.
                try:
                    proc.wait(5)
                except subprocess.TimeoutExpired:
                    proc.send_signal(signal.SIGINT)
                    try:
                        proc.wait(30)
                    except subprocess.TimeoutExpired:
                        proc.kill()
                        proc.wait()
                raise
            finally:
                self.proc = None
        if rc != 0:
            raise CalibrationError(f"aggro_arena exited with status {rc} for {cfg} (log: {log_path})")


# ---------------------------------------------------------------- analysis

def stage_games(run_dir: str, plan: dict) -> list:
    games = []
    d = os.path.join(run_dir, plan["stage"])
    for b in plan["batches"]:
        p = os.path.join(d, b["name"] + ".jsonl")
        if os.path.exists(p):
            games.extend(elo.load_jsonl(p, source=f"{plan['stage']}/{b['name']}",
                                        warn=lambda m: print("warning:", m, flush=True)))
    return games


def anchors_map(st: dict) -> dict:
    return {anchor_name(a): float(a) for a in st["anchors"]}


def level_table(st: dict, res: dict, games: list, scout_est: dict) -> list:
    lo_a, hi_a = min(st["anchors"]), max(st["anchors"])
    anchor_names = set(anchors_map(st))
    anchor_games = {}
    for g in games:
        for me, opp in ((g.white, g.black), (g.black, g.white)):
            if opp in anchor_names:
                anchor_games[me] = anchor_games.get(me, 0) + 1
    rows = []
    for lv in st["levels"]:
        name = level_name(st, lv)
        r = res["players"].get(name)
        row = {"level": lv, "name": name, "prior": prior_elo(st["prior"], lv),
               "scout_estimate": scout_est.get(str(lv)) if scout_est else None,
               "elo": None, "ci_lo": None, "ci_hi": None, "fisher_se": None, "fisher_lo": None, "fisher_hi": None,
               "games": 0, "wins": 0, "draws": 0, "losses": 0, "score": None,
               "anchor_games": anchor_games.get(name, 0), "bound_type": None, "bound": None, "labels": []}
        if r is None:
            row["labels"] = ["not played"]
            rows.append(row)
            continue
        row.update({"elo": r["elo"], "ci_lo": r["boot_lo"], "ci_hi": r["boot_hi"], "fisher_se": r["fisher_se"],
                    "fisher_lo": r["fisher_lo"], "fisher_hi": r["fisher_hi"], "games": r["games"],
                    "wins": r["wins"], "draws": r["draws"], "losses": r["losses"], "score": r["score"],
                    "bound_type": r.get("bound_type"), "bound": r.get("bound"),
                    "ci_lo_unbounded": r.get("boot_lo_unbounded", False), "ci_hi_unbounded": r.get("boot_hi_unbounded", False),
                    "boot_note": r.get("boot_note")})
        labels = []
        if r["unconnected"] or not res.get("anchors"):
            # without any anchor game elo.analyze centres the ratings on 0: not on the Stockfish scale
            for k in ("elo", "ci_lo", "ci_hi", "fisher_se", "fisher_lo", "fisher_hi", "bound", "bound_type"):
                row[k] = None
            labels.append("not connected to an anchor")
        elif r.get("unbounded"):
            labels.append("bound only (0% score)" if r.get("bound_type") == "upper" else
                          "bound only (100% score)" if r.get("bound_type") == "lower" else "no finite estimate")
            if r["games"] < st["min_games"]:
                labels.append("low-sample")
        else:
            if r["games"] < st["min_games"]:
                labels.append("low-sample")
            if r["elo"] < lo_a - st["extrap_margin"] or r["elo"] > hi_a + st["extrap_margin"]:
                labels.append("extrapolated")
            if not labels:
                labels.append("anchored" if row["anchor_games"] > 0 else "chained")
        row["labels"] = labels
        rows.append(row)
    return rows


def monotonicity(st: dict, rows: list, samples: dict) -> list:
    out = []
    rated = [r for r in rows if r["elo"] is not None]
    for prev, nxt in zip(rated, rated[1:]):
        diff = nxt["elo"] - prev["elo"]
        p_inv = elo.prob_not_above(samples, prev["name"], nxt["name"]) if samples else None
        status = "ok"
        if diff < 0:
            if nxt["ci_hi"] is not None and prev["ci_lo"] is not None and nxt["ci_hi"] < prev["ci_lo"]:
                status = "VIOLATION (95% CIs do not overlap)"
            else:
                status = "inversion within CI overlap"
        out.append({"lower": prev["level"], "upper": nxt["level"], "diff": diff, "p_inversion": p_inv,
                    "status": status})
    return out


def fit_stage3(st: dict, run_dir: str, plans: dict) -> dict:
    games = []
    for key in ("stage1_scout", "stage2_main"):
        if key in plans:
            games += stage_games(run_dir, plans[key])
    if not games:
        return {"games": 0}
    res = elo.analyze(games, anchors_map(st), st["prior_draws"], st["bootstrap"], st["seed"], keep_samples=True)
    samples = res.pop("_samples", {})
    scout_est = plans.get("stage2_main", {}).get("scout_estimates", {})
    rows = level_table(st, res, games, scout_est)
    return {"games": len(games), "fit": res, "levels": rows, "monotonicity": monotonicity(st, rows, samples),
            "terminations": _terminations(games)}


def _terminations(games: list) -> dict:
    t = {}
    for g in games:
        t[g.termination] = t.get(g.termination, 0) + 1
    return dict(sorted(t.items()))


def fit_stage4(st: dict, run_dir: str, plan: dict, stage3_rows: list) -> dict:
    games = stage_games(run_dir, plan)
    if not games:
        return {"games": 0, "tc": plan["tc"], "levels": []}
    res = elo.analyze(games, anchors_map(st), st["prior_draws"], st["bootstrap"], st["seed"])
    # No anchor game at the check TC: elo.analyze falls back to ratings centred on 0, which are not
    # on the Stockfish scale - report no Elo and no drift at all then.
    no_anchor = not res.get("anchors")
    main = {r["level"]: r for r in stage3_rows}
    rows = []
    for lv_s, info in sorted(plan["per_level"].items(), key=lambda kv: int(kv[0])):
        lv = int(lv_s)
        name = level_name(st, lv)
        r = res["players"].get(name)
        m = main.get(lv, {})
        row = {"level": lv, "name": name, "main_elo": m.get("elo"), "main_ci_lo": m.get("ci_lo"),
               "main_ci_hi": m.get("ci_hi"), "main_fisher_lo": m.get("fisher_lo"), "main_fisher_hi": m.get("fisher_hi"),
               "check_elo": None, "check_ci_lo": None, "check_ci_hi": None, "check_fisher_lo": None, "check_fisher_hi": None,
               "main_bound_type": m.get("bound_type"), "main_bound": m.get("bound"),
               "games": 0, "wins": 0, "draws": 0, "losses": 0, "drift": None, "drift_halfwidth": None,
               "check_bound_type": None, "check_bound": None,
               "anchors": info["anchors"], "bridge": info.get("bridge"), "bridges": info.get("bridges", []),
               "forced_anchor": info.get("forced_anchor", False), "labels": []}
        connected = r is not None and not r["unconnected"] and not no_anchor
        if r is not None and not connected:
            row.update({"games": r["games"], "wins": r["wins"], "draws": r["draws"], "losses": r["losses"]})
        if connected:
            row.update({"check_elo": r["elo"], "check_ci_lo": r["boot_lo"], "check_ci_hi": r["boot_hi"],
                        "check_fisher_lo": r["fisher_lo"], "check_fisher_hi": r["fisher_hi"],
                        "games": r["games"], "wins": r["wins"], "draws": r["draws"], "losses": r["losses"],
                        "check_bound_type": r.get("bound_type"), "check_bound": r.get("bound")})
        labels = []
        if r is None:
            labels.append("not played")
        elif not connected:
            labels.append("not connected to an anchor at the check TC")
        elif r.get("unbounded"):
            labels.append("bound only (0% score)" if r.get("bound_type") == "upper" else
                          "bound only (100% score)" if r.get("bound_type") == "lower" else "no finite estimate")
            if row["games"] < st["min_games"]:
                labels.append("low-sample")
        else:
            if row["main_elo"] is not None:
                row["drift"] = row["check_elo"] - row["main_elo"]
                lo3, hi3, src3 = ci_pref(row, "main_")
                lo4, hi4, src4 = ci_pref(row, "check_")
                if src3 and src4:
                    h3, h4 = (hi3 - lo3) / 2, (hi4 - lo4) / 2
                    row["drift_halfwidth"] = math.sqrt(h3 * h3 + h4 * h4)
                    row["drift_ci_source"] = "bootstrap" if src3 == src4 == "bootstrap" else "includes Fisher"
            if row["games"] < st["min_games"]:
                labels.append("low-sample")
            if not info["anchors"]:
                br = info.get("bridges") or ([info["bridge"]] if info.get("bridge") is not None else [])
                labels.append("chained via " + " and ".join(f"L{b}" for b in br) if br else "unanchored")
            elif info.get("forced_anchor"):
                labels.append(f"anchor outside the +-{st['anchor_window']:g} window")
            if row["main_elo"] is None:
                labels.append("no main-TC estimate: no drift")
            if not labels:
                labels.append("anchored")
        row["labels"] = labels
        rows.append(row)
    return {"games": len(games), "tc": plan["tc"], "fit": res, "levels": rows, "terminations": _terminations(games)}


# ---------------------------------------------------------------- report

def f0(v, signed=False, na="n/a"):
    if v is None:
        return na
    return elo.fmt(v, "+.0f" if signed else ".0f", na)


def elo_str(elo_, bound_type=None, bound=None):
    if elo_ is not None:
        return f0(elo_)
    if bound is not None:
        return ("< " if bound_type == "upper" else "> ") + f0(bound)
    return "n/a"


def ci_str(lo, hi):
    if lo is None or hi is None:
        return "n/a"
    return f"[{f0(lo, na='-inf')}, {f0(hi, na='+inf')}]"


def ci_pref(row: dict, prefix: str = ""):
    """(lo, hi, source): the bootstrap CI, else the Fisher CI (marked), else nothing."""
    lo, hi = row.get(prefix + "ci_lo"), row.get(prefix + "ci_hi")
    if lo is not None and hi is not None:
        return lo, hi, "bootstrap"
    lo, hi = row.get(prefix + "fisher_lo"), row.get(prefix + "fisher_hi")
    if lo is not None and hi is not None:
        return lo, hi, "Fisher"
    return None, None, None


def ci_cell(row: dict, prefix: str = "") -> str:
    lo, hi, src = ci_pref(row, prefix)
    if src is None:
        return "n/a"
    return ci_str(lo, hi) + (" (Fisher)" if src == "Fisher" else "")


def render_markdown(report: dict) -> str:
    st, env = report["settings"], report["environment"]
    s3 = report.get("stage3", {})
    L = []
    L.append("# AggroChess Skill Level calibration")
    L.append("")
    L.append(f"Generated {report['generated']} by `testing/calibrate.py`{' (--quick smoke test)' if st.get('quick') else ''}. "
             f"Run directory: `{report['run_dir']}`. **Status: {report['status']}**.")
    L.append("")
    L.append(f"* Engine under test: `{env.get('engine_id')}` (option `{st['level_option']}`; Threads=1, Hash=16"
             + (", " + ", ".join(st["engine_option"]) if st["engine_option"] else ", default personality") + ").")
    L.append(f"  Binary copied from `{env.get('engine_source')}` at the start of the run, sha256 `{env.get('engine_sha256')}`.")
    L.append(f"* Anchors (the only fixed ratings): `{env.get('stockfish_id')}` with UCI_LimitStrength=true, "
             f"UCI_Elo in {{{', '.join(str(a) for a in st['anchors'])}}}, Threads=1, Hash=16.")
    L.append(f"* Time control {st['tc']} (main), concurrency {st['concurrency']}, openings `{os.path.basename(st['openings'])}`"
             " (each opening played twice with colours reversed; no opening reused within the run).")
    n_s1 = report.get("stage1", {}).get("games_played", 0)
    n_s2 = report.get("stage2", {}).get("games_played", 0)
    L.append(f"* Main-TC games: {s3.get('games', 0)} (scout {n_s1}, main {n_s2}); target ~{st['games']} main-stage games per level.")
    L.append("* Ratings are on the **Stockfish 16 UCI_Elo scale at this time control on this machine** - not FIDE, "
             "not CCRL. The 95% CI is the game-pair bootstrap (headline); the Fisher interval is shown for comparison.")
    L.append("")
    L.append(f"## Ratings at {st['tc']}")
    L.append("")
    if not s3.get("levels"):
        L.append("No games yet.")
    else:
        L.append("| Skill Level | Estimated Elo | 95% CI (bootstrap) | Fisher 95% | games | W/D/L | games vs anchors | confidence |")
        L.append("|---:|---:|:---:|:---:|---:|:---:|---:|:---|")
        for r in s3["levels"]:
            L.append(f"| {r['level']} | {elo_str(r['elo'], r['bound_type'], r['bound'])} | {ci_cell(r)} "
                     f"| {ci_str(r['fisher_lo'], r['fisher_hi'])} "
                     f"| {r['games']} | {r['wins']}/{r['draws']}/{r['losses']} | {r['anchor_games']} | {', '.join(r['labels'])} |")
        L.append("")
        lo_a = min(st["anchors"])
        L.append("Confidence labels:")
        L.append(f"* `anchored` - played Stockfish anchors directly, estimate inside the anchor range (+-{st['extrap_margin']:g}), at least {st['min_games']} games.")
        L.append(f"* `chained` - inside the anchor range but rated only through games against other levels.")
        L.append(f"* `extrapolated` - estimate more than {st['extrap_margin']:g} below the lowest anchor ({lo_a}) or above the highest "
                 f"({max(st['anchors'])}): it rests on the chain of adjacent levels and the logistic model, not on a direct anchor comparison.")
        L.append(f"* `low-sample` - fewer than {st['min_games']} games; the number is computed but should not be quoted.")
        L.append(f"* A CI marked `(Fisher)` replaces the bootstrap where it is not meaningful: fewer than {elo.MIN_BOOT_UNITS} "
                 f"opening pairs, or fewer than {elo.MIN_BOOT_DISTINCT} distinct resampled values.")
        L.append("* `bound only (0% / 100% score)` - no finite maximum-likelihood estimate exists; the column shows a conservative "
                 "one-sided 95% bound instead (`< X`: losing every one of those games has probability 5% at rating X, "
                 "opponents at their estimates). `no finite estimate` - same situation for a group of levels, no bound computed.")
        L.append("")
        L.append("### Monotonicity")
        L.append("")
        bad = [m for m in s3["monotonicity"] if m["status"] != "ok"]
        if not s3["monotonicity"]:
            L.append("Fewer than two rated levels.")
        elif not bad:
            L.append("Every rated level is above the previous one.")
        else:
            L.append("| lower level | higher level | Elo difference | P(inversion) (bootstrap) | status |")
            L.append("|---:|---:|---:|---:|:---|")
            for m in bad:
                p = "n/a" if m["p_inversion"] is None else f"{100 * m['p_inversion']:.0f}%"
                L.append(f"| {m['lower']} | {m['upper']} | {f0(m['diff'], True)} | {p} | {m['status']} |")
        L.append("")
    s4 = report.get("stage4")
    if s4 is not None:
        L.append(f"## Hybrid TC check: {s4['tc']} vs {st['tc']}")
        L.append("")
        L.append(f"Levels re-measured at {s4['tc']} against their nearest anchors (games at this TC only, anchors fixed at their nominal "
                 "UCI_Elo; a level with no anchor within the window is chained to the next stronger checked level). "
                 "Drift = Elo at the check TC - Elo at the main TC; the +- is an approximate 95% half-width "
                 "(root sum of squares of both CI half-widths; a Fisher interval stands in where the bootstrap is not "
                 "meaningful, see above).")
        L.append("")
        if s4.get("skipped_levels"):
            L.append(f"Skipped (not rated at the main TC): {', '.join(str(x) for x in s4['skipped_levels'])}.")
            L.append("")
        if not s4.get("levels"):
            L.append("No games yet.")
        else:
            L.append(f"| Skill Level | Elo @ {st['tc']} | Elo @ {s4['tc']} | 95% CI @ {s4['tc']} | drift | games | W/D/L | opponents | confidence |")
            L.append("|---:|---:|---:|:---:|---:|---:|:---:|:---|:---|")
            for r in s4["levels"]:
                hw = r["drift_halfwidth"]
                drift = "n/a" if r["drift"] is None else (
                    f0(r["drift"], True) + ("" if hw is None else
                                            f" +- {f0(hw)}" if math.isfinite(hw) else " (CI unbounded)"))
                opp = ", ".join(anchor_name(a) for a in r["anchors"]) or (f"L{r['bridge']}" if r["bridge"] is not None else "-")
                L.append(f"| {r['level']} | {elo_str(r['main_elo'], r['main_bound_type'], r['main_bound'])} "
                         f"| {elo_str(r['check_elo'], r['check_bound_type'], r['check_bound'])} | {ci_cell(r, 'check_')} "
                         f"| {drift} | {r['games']} | {r['wins']}/{r['draws']}/{r['losses']} | {opp} | {', '.join(r['labels'])} |")
            L.append("")
    if s3.get("fit"):
        L.append(f"## Pairings at {st['tc']} (stages 1 + 2 pooled)")
        L.append("")
        L.append("Per-pairing W/D/L from A's view, Elo difference with 95% CI (trinomial normal approximation), "
                 "LOS, and the difference implied by the joint fit.")
        L.append("")
        L.append("| A | B | games | +/=/- | score | Elo diff (95% CI) | LOS | joint-fit diff |")
        L.append("|:---|:---|---:|:---:|---:|:---:|---:|---:|")
        for p in s3["fit"]["pairs"]:
            los = "n/a" if p["los"] is None else f"{100 * p['los']:.1f}%"
            L.append(f"| {p['a']} | {p['b']} | {p['games']} | +{p['wins']} ={p['draws']} -{p['losses']} | {100 * p['score']:.1f}% "
                     f"| {f0(p['elo'], True)} {elo.pair_ci_str(p)} | {los} "
                     f"| {f0(p['model_diff'], True)} |")
        L.append("")
        L.append("Terminations (main TC): " + ", ".join(f"{k} {v}" for k, v in s3.get("terminations", {}).items()))
        L.append("")
    L.append("## Schedule")
    L.append("")
    for key, title in (("stage1", "Stage 1 (scout)"), ("stage2", "Stage 2 (main)"), ("stage4", "Stage 4 (TC check)")):
        s = report.get(key)
        if s and s.get("planned") is not None:
            L.append(f"* {title}: {s['games_played']}/{s['planned']} games played in {s['batches']} batch(es), tc {s['tc']}.")
    s2 = report.get("stage2", {})
    if s2.get("per_level"):
        L.append("")
        L.append("| Skill Level | prior Elo | scout estimate | stage-2 anchors (games each) | stage-2 chain partners (games) |")
        L.append("|---:|---:|---:|:---|:---|")
        for lv in st["levels"]:
            info = s2["per_level"].get(str(lv), {})
            anc = ", ".join(anchor_name(a) for a in info.get("anchors", []))
            anc = f"{anc} ({info.get('games_per_anchor')})" if anc else "none in window"
            ch = ", ".join(f"L{m} ({n})" for m, n in sorted(info.get("chain", {}).items(), key=lambda kv: int(kv[0])))
            L.append(f"| {lv} | {f0(prior_elo(st['prior'], lv))} | {f0(s2.get('scout_estimates', {}).get(str(lv)))} | {anc} | {ch or '-'} |")
    L.append("")
    L.append("## Environment")
    L.append("")
    for k in ("date", "host", "cpu_model", "nproc", "memory", "loadavg_at_start", "os", "python", "engine_id", "engine_source",
              "engine_sha256", "engine_files", "stockfish_id", "stockfish", "stockfish_sha256", "arena_sha256",
              "openings_sha256", "aggrochess_git_head", "aggrochess_git_branch", "aggrochess_src_dirty_files",
              "tc", "tc_check", "games_per_level", "scout_games", "concurrency"):
        if k in env:
            L.append(f"* {k}: `{env[k]}`")
    if report.get("resumes"):
        L.append(f"* resumed {len(report['resumes'])} time(s): " + ", ".join(r.get("date", "?") for r in report["resumes"]))
    L.append("")
    L.append("## Method")
    L.append("")
    L.append("* Joint maximum-likelihood logistic Elo (draws = half points) over all main-TC games; Stockfish anchors fixed "
             f"at their nominal UCI_Elo. Prior: {st['prior_draws']:g} virtual draws per pair of players that met (numerical "
             "safety only; levels whose plain MLE is infinite get a bound instead of a number). The scout's scheduling-only fit "
             f"used {report.get('scout_prior_draws', SCOUT_PRIOR_DRAWS):g} virtual draws per pair"
             + (" (levels with a 0% / 100% scout score are scheduled at their one-sided bound or the prior map value, whichever "
                "is more extreme)" if report.get("scout_prior_draws", SCOUT_PRIOR_DRAWS) < 0.5 else
                " (this inflates the scout estimates of levels far below the lowest anchor, so some of them got anchor games "
                "although their true rating is outside the window; the final ratings are not affected)")
             + ". See `testing/elo.py`.")
    L.append(f"* 95% CI: game-pair bootstrap ({st['bootstrap']} resamples of opening pairs within each pairing, refit each time, "
             "2.5/97.5 percentiles). Fisher: inverse observed information. In simulated 21-level calibrations the bootstrap "
             "interval covered the true rating about 93% of the time (91-93% for chained levels far below the anchors, where "
             "the truth lies above the interval 2-3x more often than below); the Fisher interval is conservative (96-99.7%).")
    L.append(f"* Scheduling: prior map {', '.join(f'{int(a)}:{int(b)}' for a, b in st['prior'])} (linear), scout "
             f"{st['scout_games']} games vs the {st['scout_anchors']} nearest anchors + next level; main stage ~{st['games']} games per "
             f"level, {100 * st['anchor_share']:.0f}% vs up to {st['main_anchors']} anchors within +-{st['anchor_window']:g} of the scout "
             f"estimate, the rest vs levels up to {st['chain']} steps away in the level list.")
    L.append("")
    return "\n".join(L)


def build_report(st: dict, run_dir: str, plans: dict, meta: dict) -> dict:
    report = {"generated": now_utc(), "run_dir": run_dir, "settings": st, "environment": meta["environment"],
              "resumes": meta.get("resumes", []),
              # stage-2 plans made before the scout used the tiny prior do not record it: they used 1
              "scout_prior_draws": (plans["stage2_main"].get("scout_prior_draws", 1.0) if "stage2_main" in plans
                                    else SCOUT_PRIOR_DRAWS)}
    complete = True
    for key, stage in (("stage1", "stage1_scout"), ("stage2", "stage2_main"), ("stage4", "stage4_tc")):
        plan = plans.get(stage)
        if plan is None:
            if stage != "stage4_tc" or st.get("tc_check"):
                complete = False
            continue
        played = 0
        for b in plan["batches"]:
            ok, n = batch_status(os.path.join(run_dir, stage), b)
            played += n
            complete = complete and ok
        report[key] = {"planned": plan["games"], "games_played": played, "batches": len(plan["batches"]), "tc": plan["tc"],
                       "pairings": plan["pairings"]}
        for k in ("per_level", "scout_estimates", "skipped_levels"):
            if k in plan:
                report[key][k] = plan[k]
    report["status"] = "complete" if complete else "PARTIAL (not all planned games were played)"
    s3 = fit_stage3(st, run_dir, plans)
    report["stage3"] = s3
    if "stage4_tc" in plans:
        s4 = fit_stage4(st, run_dir, plans["stage4_tc"], s3.get("levels", []))
        s4["skipped_levels"] = plans["stage4_tc"].get("skipped_levels", [])
        report["stage4"] = s4
    return report


def write_report(st: dict, run_dir: str, plans: dict, meta: dict, out_prefix: str) -> dict:
    report = build_report(st, run_dir, plans, meta)
    os.makedirs(os.path.dirname(out_prefix) or ".", exist_ok=True)
    write_json(out_prefix + ".json", report)
    with open(out_prefix + ".md", "w", encoding="utf-8") as fh:
        fh.write(render_markdown(report))
    return report


def print_levels(report: dict) -> None:
    s3 = report.get("stage3", {})
    if not s3.get("levels"):
        return
    print("\nlevel    Elo   95% CI (bootstrap)  games  W/D/L        confidence")
    for r in s3["levels"]:
        print(f"{r['level']:5d} {elo_str(r['elo'], r['bound_type'], r['bound']):>6s}  {ci_cell(r):>18s}  {r['games']:5d}  "
              f"{r['wins']}/{r['draws']}/{r['losses']:<8}  {', '.join(r['labels'])}")
    s4 = report.get("stage4")
    if s4 and s4.get("levels"):
        print(f"\nTC check {s4['tc']}: level  main  check  drift")
        for r in s4["levels"]:
            hw = r["drift_halfwidth"]
            print(f"  {r['level']:5d} {elo_str(r['main_elo'], r['main_bound_type'], r['main_bound']):>6s} "
                  f"{elo_str(r['check_elo'], r['check_bound_type'], r['check_bound']):>6s} {f0(r['drift'], True):>6s}"
                  + ("" if hw is None else f" +-{f0(hw)}" if math.isfinite(hw) else " (CI unbounded)")
                  + f"  {', '.join(r['labels'])}")


# ---------------------------------------------------------------- main

def parse_args(argv):
    ap = argparse.ArgumentParser(description="Calibrate AggroChess Skill Levels against Stockfish UCI_Elo anchors.",
                                 formatter_class=argparse.RawDescriptionHelpFormatter, epilog=__doc__.split("Typical use")[1])
    a = ap.add_argument
    a("--engine", help=f"engine under test (default {DEFAULTS['engine']}); copied into the run dir")
    a("--engine-cwd", help="directory the --engine-files are copied from (default: the AggroChess repo)")
    a("--engine-files", help="files copied next to the engine copy, which runs with that dir as cwd "
                             "(default 'ph-gambitbook.bin'; '' for none)")
    a("--engine-option", action="append", help="extra UCI option for the engine under test, Name=Value (repeatable)")
    a("--level-option", help="UCI option that sets the level (default 'Skill Level')")
    a("--name-prefix", help="engine-name prefix for levels (default from the engine file name, e.g. aggro_L05)")
    a("--sf", help="Stockfish binary for the anchors (default /usr/games/stockfish)")
    a("--arena", help="aggro_arena binary")
    a("--openings", help="opening file (aggro_arena openings format)")
    a("--tc", help="main time control (default 10+0.1)")
    a("--games", type=int, help="target stage-2 games per level (default 200)")
    a("--scout-games", type=int, help="scout games per pairing (default 10)")
    a("--scout-anchors", type=int, help="anchors per level in the scout (default 2)")
    a("--main-anchors", type=int, help="max anchors per level in stage 2 and stage 4 (default 2)")
    a("--anchor-window", type=float, help="stage 2/4 anchors must be within this many Elo of the estimate (default 350)")
    a("--anchor-share", type=float, help="fraction of a level's stage-2 games played vs anchors (default 0.5)")
    a("--chain", type=int, help="chain depth: levels up to this many steps away in the list (default 2)")
    a("--concurrency", type=int, help="concurrent games (default 14)")
    a("--levels", help="levels to calibrate, e.g. 1-21 or 1,6,11 (default 1-21)")
    a("--anchors", help="Stockfish UCI_Elo anchors (default 1320,1500,1700,1900,2100,2300,2500)")
    a("--prior", help="level:Elo points of the scheduling prior, linear in between (default 1:400,21:2100)")
    a("--seed", type=int, help="opening shuffle / bootstrap seed (default 1)")
    a("--batch-games", type=int, help="games per arena config (default 20 x concurrency)")
    a("--bootstrap", type=int, help="bootstrap resamples (default 200)")
    a("--prior-draws", type=float, help=f"virtual draws per pair that met (default {elo.DEFAULT_PRIOR_DRAWS:g})")
    a("--min-games", type=int, help="fewer games than this => 'low-sample' (default 100)")
    a("--extrap-margin", type=float, help="estimate this far outside the anchor range => 'extrapolated' (default 100)")
    a("--tc-check", help="stage 4: hybrid TC check at this TC, e.g. 60+0.6")
    a("--tc-levels", help="levels for the TC check (default 1,6,11,16,21)")
    a("--tc-games", type=int, help="TC-check games per level (default 100)")
    a("--out", help="report prefix (writes .md and .json; default testing/results/elo_calibration, "
                    "with --quick <run-dir>/elo_calibration_quick)")
    a("--run-dir", help="run directory (default testing/runs/calib-<timestamp>)")
    a("--resume", action="store_true", help="continue the run in --run-dir (default: newest testing/runs/calib-*); "
                                            "batches whose jsonl already holds all planned games are skipped")
    a("--report-only", action="store_true", help="no games: refit and rewrite the report from an existing run dir")
    a("--dry-run", action="store_true", help="print the generated configs and the schedule, play nothing")
    a("--quick", action="store_true", default=None,
      help="smoke test: levels 1,11,21, 4 scout games per pairing, ~12 main games per level, tc 2+0.05, concurrency 5")
    return ap.parse_args(argv)


def latest_run() -> str:
    root = os.path.join(HERE, "runs")
    cands = [os.path.join(root, d) for d in os.listdir(root)] if os.path.isdir(root) else []
    cands = [d for d in cands if os.path.exists(os.path.join(d, "settings.json"))]
    if not cands:
        raise CalibrationError("no previous calibration run found under testing/runs; pass --run-dir")
    return max(cands, key=os.path.getmtime)


def resolve(args) -> tuple:
    explicit = {k: v for k, v in vars(args).items()
                if v is not None and k in DEFAULTS or (k == "quick" and v)}
    if args.engine_option is None:
        explicit.pop("engine_option", None)
    if args.resume or args.report_only:
        run_dir = os.path.abspath(args.run_dir) if args.run_dir else latest_run()
        sp = os.path.join(run_dir, "settings.json")
        if not os.path.exists(sp):
            raise CalibrationError(f"{run_dir} is not a calibration run directory (no settings.json)")
        meta = read_json(sp)
        st = meta["settings"]
        new = normalise({**DEFAULTS, **(QUICK if st.get("quick") else {}), **explicit})
        for k in explicit:
            if k in LOCKED and k in st and new.get(k) != st.get(k):
                raise CalibrationError(f"--resume: '{k}' cannot change for an existing run (run: {st.get(k)!r}, given: {new.get(k)!r})")
            if k not in LOCKED:
                st[k] = new[k]
        return st, run_dir, meta
    st = {**DEFAULTS, **(QUICK if args.quick else {}), **explicit}
    st["quick"] = bool(args.quick)
    st = normalise(st)
    stamp = datetime.datetime.now().strftime("%Y%m%d-%H%M%S")
    run_dir = os.path.abspath(args.run_dir) if args.run_dir else os.path.join(
        HERE, "runs", f"calib-{'quick-' if st['quick'] else ''}{stamp}")
    return st, run_dir, None


def check_tools(st: dict) -> None:
    for k in ("arena", "sf", "openings"):
        if not os.path.exists(st[k]):
            hint = " (build it: cd testing/arena && cargo build --release --offline)" if k == "arena" else ""
            raise CalibrationError(f"{k} not found: {st[k]}{hint}")


def setup_run(st: dict, run_dir: str) -> dict:
    if os.path.exists(run_dir) and os.listdir(run_dir):
        raise CalibrationError(f"run directory {run_dir} exists and is not empty (use --resume or another --run-dir)")
    if not os.path.exists(st["engine"]):
        raise CalibrationError(f"engine not found: {st['engine']}")
    created = not os.path.exists(run_dir)
    try:
        return _setup_run(st, run_dir)
    except BaseException:
        # leave nothing half-made behind (a retry with the same --run-dir must work)
        if created:
            shutil.rmtree(run_dir, ignore_errors=True)
        elif os.path.isdir(run_dir):
            for f in os.listdir(run_dir):
                p = os.path.join(run_dir, f)
                shutil.rmtree(p, ignore_errors=True) if os.path.isdir(p) else os.remove(p)
        raise


def _setup_run(st: dict, run_dir: str) -> dict:
    eng_dir = os.path.join(run_dir, "engine")
    os.makedirs(eng_dir, exist_ok=True)
    src_sha = sha256(st["engine"])
    dst = os.path.join(eng_dir, os.path.basename(st["engine"]))
    shutil.copy2(st["engine"], dst)
    os.chmod(dst, 0o755)
    copied_sha = sha256(dst)
    if copied_sha != src_sha:
        raise CalibrationError("the engine binary changed while it was being copied; try again")
    bin_dir = os.path.join(run_dir, "bin")
    os.makedirs(bin_dir, exist_ok=True)
    shutil.copy2(st["arena"], os.path.join(bin_dir, "aggro_arena"))   # testing/arena may be rebuilt meanwhile
    os.chmod(os.path.join(bin_dir, "aggro_arena"), 0o755)
    missing = []
    for f in st["engine_files"]:
        src = os.path.join(st["engine_cwd"], f)
        if os.path.exists(src):
            shutil.copy2(src, os.path.join(eng_dir, f))
        else:
            missing.append(f)
    if missing:
        print(f"warning: engine files not found in {st['engine_cwd']}: {', '.join(missing)}", flush=True)
    opts = uci_options(dst, eng_dir)
    if not opts:
        raise CalibrationError(f"{dst} did not answer 'uci'")
    lower = {o.lower() for o in opts}
    if st["level_option"].lower() not in lower:
        raise CalibrationError(f"the engine does not advertise the option '{st['level_option']}' (has: {', '.join(opts)})")
    for o in st["engine_option"]:
        name = o.split("=", 1)[0].strip()
        if name.lower() not in lower:
            raise CalibrationError(f"--engine-option '{o}': the engine does not advertise an option '{name}' "
                                   f"(has: {', '.join(opts)})")
    env = environment(st, run_dir)
    meta = {"settings": st, "created": now_utc(), "environment": env, "resumes": [], "missing_engine_files": missing}
    write_json(os.path.join(run_dir, "settings.json"), meta)
    return meta


def scout_estimate_map(st: dict, games: list) -> tuple:
    """({level: estimate}, {level: source}, fit) from scout games (scheduling only)."""
    anchors = anchors_map(st)
    agg = elo.aggregate(games)
    fit = elo.fit_ratings(agg, anchors, SCOUT_PRIOR_DRAWS, want_cov=False)
    unb = elo.unbounded_players(agg, fit.anchors) if fit.anchors else {}
    bounded = {p: v for p, v in fit.ratings.items() if p not in unb}
    est, src = {}, {}
    for lv in st["levels"]:
        name = level_name(st, lv)
        pr = prior_elo(st["prior"], lv)
        v = fit.ratings.get(name)
        if v is None:
            est[str(lv)], src[str(lv)] = pr, "prior (not connected in the scout)"
        elif name not in unb:
            est[str(lv)], src[str(lv)] = v, "scout"
        else:
            b = elo.perfect_score_bound(agg, name, bounded)
            opp = [bounded[o] for k in agg for o in k if name in k and o != name and o in bounded]
            if unb[name] == "low":
                cap = b[1] if b is not None else (min(opp) if opp else pr)
                est[str(lv)], src[str(lv)] = min(pr, cap), "scout 0%: min(prior, upper bound)"
            else:
                cap = b[1] if b is not None else (max(opp) if opp else pr)
                est[str(lv)], src[str(lv)] = max(pr, cap), "scout 100%: max(prior, lower bound)"
    return est, src, fit


def scout_estimates(st: dict, run_dir: str, plan: dict) -> dict:
    games = stage_games(run_dir, plan)
    est, src, fit = scout_estimate_map(st, games)
    write_json(os.path.join(run_dir, "stage1_scout", "fit.json"),
               {"ratings": fit.ratings, "unconnected": fit.unconnected, "prior_draws": SCOUT_PRIOR_DRAWS,
                "converged": fit.converged, "estimates": est, "source": src})
    return est


def dry_run(st: dict, run_dir: str) -> None:
    r = Runner(st, run_dir, False)
    pairings, info = plan_scout(st)
    plan = r.make_plan("stage1_scout", st["tc"], pairings, 0, {"per_level": info})
    print(f"# run dir would be {run_dir}; engine copied to {run_dir}/engine/")
    print(f"# stage 1 (scout): {plan['games']} games in {len(plan['batches'])} batch(es)\n")
    for b in plan["batches"]:
        print(f"# ---- {run_dir}/stage1_scout/{b['name']}.cfg")
        print(config_text(st, "stage1_scout", st["tc"], b, st["concurrency"]))
    est = {lv: prior_elo(st["prior"], lv) for lv in st["levels"]}
    p2, info2 = plan_main(st, est)
    print(f"# stage 2 (main) PREVIEW from the prior map (the real schedule uses the scout estimates): "
          f"{sum(p[2] for p in p2)} games")
    for lv in st["levels"]:
        d = info2[str(lv)]
        print(f"#   L{lv:<3d} est {est[lv]:6.0f}  anchors {d['anchors']} x{d['games_per_anchor']}  chain {d['chain']}  "
              f"total {d['planned_games']}")
    if st.get("tc_check"):
        p4, info4, skipped = plan_tc_check(st, est)
        print(f"# stage 4 (TC check {st['tc_check']}) PREVIEW: {sum(p[2] for p in p4)} games: "
              + "; ".join(f"{a} vs {b} x{n}" for a, b, n in p4) + (f"; skipped {skipped}" if skipped else ""))


def main(argv=None) -> int:
    args = parse_args(argv)
    try:
        st, run_dir, meta = resolve(args)
    except (ValueError, CalibrationError) as e:
        print(f"error: {e}", file=sys.stderr)
        return 2
    out = args.out or (os.path.join(run_dir, "elo_calibration_quick") if st.get("quick")
                       else os.path.join(HERE, "results", "elo_calibration"))
    for ext in (".md", ".json"):
        if out.endswith(ext):
            out = out[:-len(ext)]
    out = os.path.abspath(out)
    if args.dry_run:
        dry_run(st, run_dir)
        return 0

    runner = None

    def on_signal(signum, frame):
        if runner is not None and runner.proc is not None and runner.proc.poll() is None:
            runner.proc.send_signal(signal.SIGINT)   # the arena did not get this signal itself
        raise KeyboardInterrupt
    signal.signal(signal.SIGTERM, on_signal)
    signal.signal(signal.SIGHUP, on_signal)

    try:
        if not args.report_only:
            check_tools(st)
        if meta is None:
            meta = setup_run(st, run_dir)
            print(f"run directory: {run_dir}", flush=True)
        elif args.resume:
            eng_copy = os.path.join(run_dir, "engine", os.path.basename(st["engine"]))
            if sha256(eng_copy) != meta["environment"].get("engine_sha256"):
                raise CalibrationError(f"engine copy {eng_copy} is missing or differs from the recorded sha256")
            meta["resumes"].append({"date": now_utc(), "loadavg": list(os.getloadavg()), "concurrency": st["concurrency"]})
            meta["settings"] = st
            write_json(os.path.join(run_dir, "settings.json"), meta)
            print(f"resuming {run_dir}", flush=True)
        if st["concurrency"] > (os.cpu_count() or 1):
            print(f"warning: concurrency {st['concurrency']} exceeds the CPU count {os.cpu_count()}", flush=True)
        runner = Runner(st, run_dir, bool(args.resume))
        plans = {}
        for stage in ("stage1_scout", "stage2_main", "stage4_tc"):
            p = os.path.join(run_dir, stage, "plan.json")
            if os.path.exists(p):
                plans[stage] = read_json(p)

        if not args.report_only:
            # stage 1
            def mk1():
                pairings, info = plan_scout(st)
                return runner.make_plan("stage1_scout", st["tc"], pairings, 0, {"per_level": info})
            plans["stage1_scout"] = runner.load_or_create_plan("stage1_scout", mk1)
            print(f"== stage 1 (scout): {plans['stage1_scout']['games']} games at {st['tc']}", flush=True)
            runner.run_stage(plans["stage1_scout"])
            est = scout_estimates(st, run_dir, plans["stage1_scout"])
            print("scout estimates: " + ", ".join(f"L{lv}={est[str(lv)]:.0f}" for lv in st["levels"]), flush=True)

            # stage 2
            def mk2():
                pairings, info = plan_main(st, {lv: est[str(lv)] for lv in st["levels"]})
                return runner.make_plan("stage2_main", st["tc"], pairings, plans["stage1_scout"]["openings_end"],
                                        {"per_level": info, "scout_estimates": est,
                                         "scout_prior_draws": SCOUT_PRIOR_DRAWS})
            plans["stage2_main"] = runner.load_or_create_plan("stage2_main", mk2)
            print(f"== stage 2 (main): {plans['stage2_main']['games']} games at {st['tc']}", flush=True)
            runner.run_stage(plans["stage2_main"])

        # stage 3
        print("== stage 3: joint fit + bootstrap", flush=True)
        report = write_report(st, run_dir, plans, meta, out)
        print_levels(report)

        if st.get("tc_check") and not args.report_only:
            s3rows = report["stage3"].get("levels", [])

            def mk4():
                # scheduling only: a level without a finite estimate is placed at its bound
                est3 = {r["level"]: (r["elo"] if r["elo"] is not None else r["bound"])
                        for r in s3rows if r["elo"] is not None or r["bound"] is not None}
                pairings, info, skipped = plan_tc_check(st, est3)
                if not pairings:
                    raise CalibrationError("TC check: nothing to play (no --tc-levels rated at the main TC)")
                return runner.make_plan("stage4_tc", st["tc_check"], pairings, plans["stage2_main"]["openings_end"],
                                        {"per_level": info, "skipped_levels": skipped,
                                         "tc_levels": st["tc_levels"], "tc_games": st["tc_games"]})
            plans["stage4_tc"] = runner.load_or_create_plan("stage4_tc", mk4)
            p4 = plans["stage4_tc"]
            if p4["tc"] != st["tc_check"] or p4.get("tc_levels") != st["tc_levels"] or p4.get("tc_games") != st["tc_games"]:
                raise CalibrationError(f"stage 4 was already planned with tc {p4['tc']}, levels {p4.get('tc_levels')}, "
                                       f"games {p4.get('tc_games')}; delete {os.path.join(run_dir, 'stage4_tc')} to re-plan")
            print(f"== stage 4 (TC check): {p4['games']} games at {p4['tc']}", flush=True)
            runner.run_stage(p4)
            report = write_report(st, run_dir, plans, meta, out)
            print_levels(report)
        print(f"\nreport: {out}.md\n        {out}.json\nstatus: {report['status']}", flush=True)
        return 0
    except CalibrationError as e:
        print(f"error: {e}", file=sys.stderr)
        return 1
    except KeyboardInterrupt:
        print(f"\ninterrupted; continue with: python3 {os.path.relpath(__file__)} --resume --run-dir {run_dir}",
              file=sys.stderr)
        return 130


if __name__ == "__main__":
    sys.exit(main())
