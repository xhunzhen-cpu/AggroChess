#!/usr/bin/env python3
"""sactest.py - compare the sacrificial play of the modified AggroChess with the original.

Stdlib only. Drives `aggro_arena match` and `aggro_arena analyze`:

  (a) each engine plays the same Stockfish anchors (UCI_LimitStrength + UCI_Elo; default
      1800 and 2200, as in the spec) with the same openings and colours (same seed),
  (b) modified vs original head-to-head,
then analyses each engine's games (gauntlet + head-to-head) for sacrifices and writes a
side-by-side report: testing/results/sacrifice_report.md + .json (copies in the run dir).

Both engine binaries are copied into the run directory before anything starts, so a
rebuild of target/release/aggro_chess does not affect a running test.

Examples
  python3 testing/sactest.py                                   # full run (defaults)
  python3 testing/sactest.py --games 6 --h2h-games 6 --tc 3+0.03 --concurrency 5 \\
          --sf-nodes 100000 --sf-threads 4 --report testing/runs/smoke_report   # smoke test
  python3 testing/sactest.py --skip-matches --out testing/runs/sactest-20261108-120000
  python3 testing/sactest.py --new-options 'Sacrifice Appetite=50;Aggression=60'
"""

import argparse
import datetime
import glob
import hashlib
import json
import math
import os
import platform
import shutil
import signal
import socket
import subprocess
import sys
import time

TESTING = os.path.dirname(os.path.abspath(__file__))
ROOT = os.path.dirname(TESTING)  # the AggroChess checkout
ARENA_DIR = os.path.join(TESTING, "arena")
ARENA_DEFAULT = os.path.join(ARENA_DIR, "target", "release", "aggro_arena")
NEW_DEFAULT = os.path.join(ROOT, "target", "release", "aggro_chess")
OLD_DEFAULT = "/root/aggroclone/engines/aggro_original"
SF_DEFAULT = "/usr/games/stockfish"
OPENINGS_DEFAULT = os.path.join(TESTING, "openings", "8moves_v3.txt")
REPORT_DEFAULT = os.path.join(TESTING, "results", "sacrifice_report")
RUNS_DIR = os.path.join(TESTING, "runs")

NEW, OLD = "aggro_new", "aggro_old"
Z95 = 1.959964


class Interrupted(Exception):
    pass


# ----------------------------------------------------------------------------- helpers

def log(msg):
    print(msg, flush=True)


def utc_now():
    return datetime.datetime.now(datetime.timezone.utc).strftime("%Y-%m-%dT%H:%M:%SZ")


def sha256(path):
    h = hashlib.sha256()
    with open(path, "rb") as f:
        for chunk in iter(lambda: f.read(1 << 20), b""):
            h.update(chunk)
    return h.hexdigest()


def cpu_model():
    try:
        with open("/proc/cpuinfo") as f:
            for line in f:
                if line.lower().startswith("model name"):
                    return line.split(":", 1)[1].strip()
    except OSError:
        pass
    return platform.processor() or "unknown"


def mem_total_gb():
    try:
        with open("/proc/meminfo") as f:
            for line in f:
                if line.startswith("MemTotal:"):
                    return round(int(line.split()[1]) / 1024 / 1024, 1)
    except OSError:
        pass
    return None


def parse_options(text):
    """'Name=Value;Name 2=Value' -> [(name, value)]"""
    out = []
    if not text:
        return out
    for part in text.split(";"):
        part = part.strip()
        if not part:
            continue
        if "=" not in part:
            raise SystemExit(f"bad option '{part}' (expected Name=Value)")
        k, v = part.split("=", 1)
        out.append((k.strip(), v.strip()))
    return out


def uci_probe(path, cwd, timeout=15):
    """Run `uci` and return (id_name, {option_name: option_line})."""
    try:
        p = subprocess.Popen([path], cwd=cwd, stdin=subprocess.PIPE, stdout=subprocess.PIPE,
                             stderr=subprocess.DEVNULL, text=True)
    except OSError as e:
        raise SystemExit(f"cannot start {path}: {e}")
    try:
        out, _ = p.communicate("uci\nisready\nquit\n", timeout=timeout)
    except subprocess.TimeoutExpired:
        p.kill()
        out, _ = p.communicate()
    id_name, opts = None, {}
    for line in out.splitlines():
        line = line.strip()
        if line.startswith("id name "):
            id_name = line[8:].strip()
        elif line.startswith("option name ") and " type " in line:
            name = line[len("option name "):line.index(" type ")].strip()
            opts[name] = line
    if "uciok" not in out:
        raise SystemExit(f"{path}: no 'uciok' (output: {out[:300]!r})")
    return id_name, opts


def merge_options(base, extra):
    """Options in order; later entries override earlier ones (case-insensitive name)."""
    out = list(base)
    for k, v in extra:
        for i, (k0, _) in enumerate(out):
            if k0.lower() == k.lower():
                out[i] = (k0, v)
                break
        else:
            out.append((k, v))
    return out


# ----------------------------------------------------------------------------- statistics

def wilson(k, n):
    if n == 0:
        return None
    p = k / n
    z2 = Z95 * Z95
    d = 1 + z2 / n
    c = (p + z2 / (2 * n)) / d
    h = Z95 * math.sqrt(p * (1 - p) / n + z2 / (4 * n * n)) / d
    return max(0.0, c - h), min(1.0, c + h)


def newcombe_diff(k1, n1, k2, n2):
    """95% CI of p1 - p2 (Newcombe hybrid score interval, method 10)."""
    if n1 == 0 or n2 == 0:
        return None
    p1, p2 = k1 / n1, k2 / n2
    l1, u1 = wilson(k1, n1)
    l2, u2 = wilson(k2, n2)
    d = p1 - p2
    return d - math.sqrt((p1 - l1) ** 2 + (u2 - p2) ** 2), d + math.sqrt((u1 - p1) ** 2 + (p2 - l2) ** 2)


def mean_diff(m1, m2):
    """Welch-style normal CI of mean1 - mean2 from {'n','mean','sd'} blocks."""
    if not m1 or not m2 or not m1.get("n") or not m2.get("n") or m1.get("mean") is None or m2.get("mean") is None:
        return None, None
    d = m1["mean"] - m2["mean"]
    if m1["n"] < 2 or m2["n"] < 2:
        return d, None
    se = math.sqrt(m1["sd"] ** 2 / m1["n"] + m2["sd"] ** 2 / m2["n"])
    return d, (d - Z95 * se, d + Z95 * se)


def get(d, path, default=None):
    cur = d
    for part in path.split("."):
        if not isinstance(cur, dict) or part not in cur:
            return default
        cur = cur[part]
    return cur


# ----------------------------------------------------------------------------- formatting

def f_prop(p, pct=True):
    if not p or p.get("of") in (None, 0):
        return f"{(p or {}).get('count', 0)}/0" if p else "-"
    r = p["rate"]
    lo, hi = p["ci95"]
    if pct:
        return f"{p['count']}/{p['of']} = {100 * r:.1f}% [{100 * lo:.1f}, {100 * hi:.1f}]"
    return f"{p['count']}/{p['of']} = {r:.3f} [{lo:.3f}, {hi:.3f}]"


def f_mean(m, scale=1.0, digits=2, unit=""):
    if not m or m.get("mean") is None:
        return "-"
    s = f"{m['mean'] * scale:.{digits}f}{unit}"
    if m.get("ci95"):
        lo, hi = m["ci95"]
        s += f" [{lo * scale:.{digits}f}, {hi * scale:.{digits}f}]"
    return s


def f_diff(d, ci, scale=1.0, digits=2, unit=""):
    if d is None:
        return "-"
    s = f"{d * scale:+.{digits}f}{unit}"
    if ci:
        s += f" [{ci[0] * scale:+.{digits}f}, {ci[1] * scale:+.{digits}f}]"
    return s


def f_elo(v):
    return "n/a" if v is None else f"{v:+.0f}"


# metric rows: (label, path, kind) - kind: prop | mean | score | int | float | ctx
ROWS = [
    ("Games analysed", "games", "int"),
    ("Score (all games)", "result.score", "score"),
    ("Games with >= 1 sacrifice", "games_with_sacrifice", "prop"),
    ("Sacrifices", "sacrifices", "int"),
    ("Sacrifices per game", "sacrifices_per_game", "mean"),
    ("Sacrifices per 100 engine moves", "sacrifices_per_100_moves", "float"),
    ("Balanced-position sacrifices per game (own eval within +-3)", "balanced_sacrifices_per_game", "mean"),
    ("Games with a balanced-position sacrifice", "games_with_balanced_sacrifice", "prop"),
    ("  queen sacrifices", "categories.queen", "int"),
    ("  exchange sacrifices", "categories.exchange", "int"),
    ("  piece sacrifices", "categories.piece", "int"),
    ("  pawn sacrifices", "categories.pawn", "int"),
    ("Context balanced / winning / losing / unknown", "sacrifices_by_context", "ctx"),
    ("Book-gambit sacrifices", "book_sacrifices", "int"),
    ("Intent unknown (no score)", "intent_unknown", "int"),
    ("Chained sacrifices (chain_len > 1)", "chained_sacrifices", "int"),
    ("Blunders (filtered out)", "blunders", "int"),
    ("  of which caused by the previous move (own eval had collapsed)", "blunders_collapsed", "int"),
    ("Blunders per game", "blunders_per_game", "mean"),
    ("Forced material losses (single legal move; excluded)", "forced_losses", "int"),
    ("Moves while being mated (desperation; excluded)", "desperation_moves", "int"),
    ("Mate within 30 plies (of sacrifices)", "mates_after_sacrifice", "prop"),
    ("Recouped within 24 plies", "recouped", "prop"),
    ("Successful attacks", "successful_attacks", "prop"),
    ("Unsound (SF; of SF-evaluated sacrifices)", "unsound", "prop"),
    ("Practical chances (of unsound)", "practical_chances", "prop"),
    ("Blunders confirmed by SF", "blunders_confirmed_by_sf", "prop"),
    ("Score in games with a sacrifice", "score_games_with_sacrifice.score", "score"),
    ("Score in games without a sacrifice", "score_games_without_sacrifice.score", "score"),
    ("Average move number of sacrifices", "avg_move_number", "mean"),
    ("Average SF eval before the sacrifice (cp, engine view)", "avg_sf_before", "mean0"),
    ("Average SF eval after the sacrifice (cp, engine view)", "avg_sf_after", "mean0"),
]

CATS = ["queen", "exchange", "piece", "pawn"]


def compare_row(label, path, kind, a, b):
    """Returns (cells for markdown, json record)."""
    va, vb = get(a, path), get(b, path)
    rec = {"metric": label, "key": path, "new": va, "old": vb, "diff": None, "diff_ci95": None}
    if kind == "prop":
        cells = [f_prop(va), f_prop(vb)]
        if va and vb and va.get("of") and vb.get("of"):
            d = va["rate"] - vb["rate"]
            ci = newcombe_diff(va["count"], va["of"], vb["count"], vb["of"])
            rec["diff"], rec["diff_ci95"] = d, ci
            cells.append(f_diff(d, ci, 100.0, 1, " pp"))
        else:
            cells.append("-")
    elif kind in ("mean", "mean0", "score"):
        scale, digits, unit = (100.0, 1, "%") if kind == "score" else (1.0, 0 if kind == "mean0" else 2, "")
        cells = [f_mean(va, scale, digits, unit), f_mean(vb, scale, digits, unit)]
        if kind == "score" and va and vb and va.get("n") and vb.get("n") and "points" in va and "points" in vb:
            # score points (draw = 1/2) as fractional successes: Newcombe interval, as for proportions
            d = va["mean"] - vb["mean"]
            ci = newcombe_diff(va["points"], va["n"], vb["points"], vb["n"])
        else:
            d, ci = mean_diff(va, vb)
        rec["diff"], rec["diff_ci95"] = d, ci
        cells.append(f_diff(d, ci, scale, digits, " pp" if kind == "score" else ""))
    elif kind == "ctx":
        fmt = lambda c: "-" if not c else " / ".join(str(c.get(k, 0)) for k in ("balanced", "winning", "losing", "unknown"))
        cells = [fmt(va), fmt(vb), ""]
    elif kind == "float":
        cells = ["-" if va is None else f"{va:.2f}", "-" if vb is None else f"{vb:.2f}"]
        if va is not None and vb is not None:
            rec["diff"] = va - vb
            cells.append(f"{va - vb:+.2f}")
        else:
            cells.append("-")
    else:  # int
        cells = ["-" if va is None else str(va), "-" if vb is None else str(vb)]
        if va is not None and vb is not None:
            rec["diff"] = va - vb
            cells.append(f"{va - vb:+d}")
        else:
            cells.append("-")
    return cells, rec


def table(headers, rows):
    out = ["| " + " | ".join(headers) + " |", "|" + "|".join("---" for _ in headers) + "|"]
    for r in rows:
        out.append("| " + " | ".join(str(c).replace("|", "\\|") for c in r) + " |")
    return "\n".join(out)


# ----------------------------------------------------------------------------- configs / matches

def write_config(path, event, engines, pairs, args, seed, stem):
    lines = [f"# generated by sactest.py {utc_now()}", f"event {event}"]
    for name, eng in engines.items():
        lines.append(f"engine {name} {eng['path']}")
        for k, v in eng["options"]:
            lines.append(f"option {name} {k} = {v}")
        if eng.get("cwd"):
            lines.append(f"cwd {name} {eng['cwd']}")
    lines += [f"tc {args.tc}", f"concurrency {args.concurrency}", f"openings {args.openings}", f"seed {seed}",
              f"pgn {stem}.pgn", f"jsonl {stem}.jsonl"]
    for a, b, n in pairs:
        lines.append(f"pair {a} {b} {n}")
    with open(path, "w") as f:
        f.write("\n".join(lines) + "\n")


_child = None
_stop = False


def _on_signal(signum, frame):
    """SIGINT/SIGTERM: forward one SIGTERM to the running aggro_arena (it then abandons the
    running games, writes its summary and exits 130) and keep reading its output; a second
    signal is forwarded too (the arena then exits at once). Without a child: stop now."""
    global _stop
    _stop = True
    if _child is not None and _child.poll() is None:
        try:
            _child.send_signal(signal.SIGTERM)
        except OSError:
            pass
        return
    raise Interrupted()


def run_logged(cmd, cwd, logfile, echo=True):
    """Run a command, tee its stdout+stderr to logfile (and stdout). Returns its exit code;
    raises Interrupted after the child has finished if a stop signal was received."""
    global _child
    with open(logfile, "a") as lf:
        lf.write(f"$ {' '.join(cmd)}\n")
        lf.flush()
        # own session: a terminal Ctrl-C reaches only this script, which forwards exactly one
        # signal (two would make the arena exit without writing its summary)
        _child = subprocess.Popen(cmd, cwd=cwd, stdout=subprocess.PIPE, stderr=subprocess.STDOUT, text=True,
                                  bufsize=1, start_new_session=True)
        try:
            # keep draining until EOF even after a stop signal, so the child never hits a
            # closed pipe while writing its final report
            for line in _child.stdout:
                lf.write(line)
                lf.flush()
                if echo:
                    sys.stdout.write(line)
                    sys.stdout.flush()
            rc = _child.wait()
        finally:
            _child = None
    if _stop:
        raise Interrupted()
    return rc


def ensure_arena(arena):
    if os.path.exists(arena):
        return
    if arena != ARENA_DEFAULT:
        raise SystemExit(f"arena binary not found: {arena}")
    log("building aggro_arena (cargo build --release --offline)")
    subprocess.check_call(["cargo", "build", "--release", "--offline"], cwd=ARENA_DIR)


def freeze_engines(run_dir, args):
    """Copy both engines into <run>/bin, verify hashes, probe options."""
    engines = {}
    for role, src, extra in ((NEW, args.new, args.new_options), (OLD, args.old, args.old_options)):
        src = os.path.abspath(src)
        if not os.path.isfile(src):
            raise SystemExit(f"engine not found: {src}")
        dst = os.path.join(run_dir, "bin", role)
        shutil.copy2(src, dst)
        os.chmod(dst, 0o755)
        h_src, h_dst = sha256(src), sha256(dst)
        if h_src != h_dst:
            raise SystemExit(f"{src} changed while copying (rebuild in progress?) - retry")
        id_name, adv = uci_probe(dst, args.engine_cwd)
        base = [("Threads", "1")]
        if "Hash" in adv:
            base.append(("Hash", "16"))
        if role == NEW and "Skill Level" in adv:
            base.append(("Skill Level", "21"))
        extra_opts = []
        for k, v in parse_options(extra):
            match = [a for a in adv if a.lower() == k.lower()]
            if not match:
                raise SystemExit(f"{role} ({id_name}) has no UCI option '{k}'; available: {', '.join(sorted(adv))}")
            extra_opts.append((match[0], v))
        engines[role] = {"source": src, "path": dst, "sha256": h_dst, "id_name": id_name,
                         "options": merge_options(base, extra_opts), "cwd": os.path.abspath(args.engine_cwd),
                         "advertised_options": sorted(adv)}
        log(f"{role}: {id_name}  sha256 {h_dst[:16]}..  options {engines[role]['options']}")
    return engines


# ----------------------------------------------------------------------------- report

def opponent_label(name, meta):
    if name in (NEW, OLD):
        return "head-to-head"
    return name


def notable(events_path, limit=8):
    """A few illustrative sacrifices: bigger categories first, balanced context, successful."""
    evs = []
    if os.path.exists(events_path):
        with open(events_path) as f:
            for line in f:
                e = json.loads(line)
                if e.get("type") == "sacrifice":
                    evs.append(e)
    rank = {c: i for i, c in enumerate(CATS)}
    ctx = {"balanced": 0, "unknown": 1, "winning": 2, "losing": 3}
    evs.sort(key=lambda e: (ctx.get(e.get("context"), 4), rank.get(e["category"], 9),
                            not e.get("successful_attack"), e["game_id"], e["ply"]))
    return evs[:limit]


def context_rates(run_dir, role, events_path):
    """Engine moves, sacrifices and blunders by the engine's own-score context (balanced = within +-300)."""
    def ctx(v):
        if v is None:
            return "unknown"
        return "winning" if v >= 300 else "losing" if v <= -300 else "balanced"
    moves = {}
    for stem in ("gauntlet_new", "gauntlet_old", "h2h"):
        gp = os.path.join(run_dir, f"{stem}.jsonl")
        if not os.path.exists(gp):
            continue
        with open(gp) as f:
            for line in f:
                g = json.loads(line)
                for side, par in (("white", 0), ("black", 1)):
                    if g.get(side) != role:
                        continue
                    sc = g.get("scores") or []
                    for i in range(g.get("opening_plies", 0), len(g.get("moves", []))):
                        if i % 2 == par:
                            c = ctx(sc[i] if i < len(sc) else None)
                            moves[c] = moves.get(c, 0) + 1
    sacs, blun = {}, {}
    if os.path.exists(events_path):
        with open(events_path) as f:
            for line in f:
                e = json.loads(line)
                c = e.get("context", "unknown")
                if e.get("type") == "sacrifice":
                    sacs[c] = sacs.get(c, 0) + 1
                elif e.get("type") == "blunder":
                    blun[c] = blun.get(c, 0) + 1
    return {"moves": moves, "sacrifices": sacs, "blunders": blun}


def build_report(run_dir, meta, an_new, an_old, args):
    md = []
    rep = {"generated": utc_now(), "run_dir": run_dir, "meta": meta}
    new_id = meta["engines"][NEW]["id_name"]
    old_id = meta["engines"][OLD]["id_name"]
    md.append("# Sacrifice report: modified vs original AggroChess\n")
    md.append(f"Generated {rep['generated']} by `testing/sactest.py` from run directory `{os.path.relpath(run_dir, ROOT)}`.\n")
    md.append(f"* **new** = `{NEW}`: {new_id}, options `{'; '.join(f'{k}={v}' for k, v in meta['engines'][NEW]['options'])}`"
              f" (personality: {meta.get('personality', 'default')})")
    md.append(f"* **old** = `{OLD}`: {old_id}, options `{'; '.join(f'{k}={v}' for k, v in meta['engines'][OLD]['options'])}`\n")

    # ---- setup
    env = meta["environment"]
    md.append("## Setup\n")
    rows = [
        ["Date (UTC)", f"{meta['started']} .. {meta.get('finished') or '(running)'}"],
        ["Host / CPU", f"{env['host']} / {env['cpu_model']} ({env['logical_cpus']} logical CPUs, {env.get('mem_total_gb')} GB RAM)"],
        ["OS / Python", f"{env['os']} / Python {env['python']}"],
        ["Time control", meta["tc"]],
        ["Games", f"{meta['games_per_anchor']} per engine per anchor; head-to-head {meta['h2h_games']} "
                  f"(colour-reversed opening pairs)"],
        ["Anchors", ", ".join(f"Stockfish UCI_Elo {a}" for a in meta["anchors"]) + " (Threads=1, Hash=16, UCI_LimitStrength=true)"],
        ["Concurrency", str(meta["concurrency"])],
        ["Openings", f"`{os.path.relpath(meta['openings'], ROOT)}`, seed {meta['seed']} (both gauntlets use the same openings and colours)"],
        ["Analysis", "no Stockfish" if not meta.get("sf") else
         f"Stockfish `{meta['sf']}`, {meta['sf_nodes']} nodes/position, {meta['sf_threads']} processes x 1 thread, Hash 64"],
    ]
    md.append(table(["", ""], rows) + "\n")
    rows = []
    for role in (NEW, OLD):
        e = meta["engines"][role]
        rows.append([role, e["id_name"], f"`{e['source']}`", f"`{e['sha256']}`", f"`{os.path.relpath(e['path'], ROOT)}`"])
    if meta.get("book"):
        rows.append(["book", os.path.basename(meta["book"]["path"]), f"`{meta['book']['path']}`", f"`{meta['book']['sha256']}`", "(engine cwd)"])
    if meta.get("arena"):
        rows.append(["arena", "aggro_arena", f"`{os.path.relpath(meta['arena']['path'], ROOT)}`", f"`{meta['arena']['sha256']}`", ""])
    md.append(table(["", "id", "source", "sha256", "copy used"], rows) + "\n")

    # ---- match results
    md.append("## Match results\n")
    mrows, mjson = [], []
    for stem in ("gauntlet_new", "gauntlet_old", "h2h"):
        sp = os.path.join(run_dir, f"{stem}.jsonl.summary.json")
        if not os.path.exists(sp):
            continue
        with open(sp) as f:
            s = json.load(f)
        for p in s.get("pairs", []):
            ff = p.get("forfeits", {})
            forf = ", ".join(f"{n}: {v.get('time', 0)}t/{v.get('illegal', 0)}i/{v.get('crash', 0)}c"
                             for n, v in ff.items() if any(v.get(x, 0) for x in ("time", "illegal", "crash"))) or "none"
            mrows.append([f"{p['engine_a']} vs {p['engine_b']}", f"{p['games']}/{p['games_planned']}",
                          f"+{p['wins']} ={p['draws']} -{p['losses']}", f"{100 * p['score']:.1f}%",
                          f"{f_elo(p.get('elo'))} [{f_elo(p.get('elo_lo'))}, {f_elo(p.get('elo_hi'))}]",
                          "n/a" if p.get("los") is None else f"{100 * p['los']:.1f}%", forf])
            mjson.append({"file": sp, **{k: p.get(k) for k in ("engine_a", "engine_b", "games", "games_planned", "wins", "draws",
                                                                 "losses", "score", "elo", "elo_lo", "elo_hi", "los", "forfeits")}})
    rep["matches"] = mjson
    md.append(table(["pairing", "games", "W/D/L (first engine)", "score", "Elo diff [95% CI]", "LOS", "forfeits"], mrows) + "\n")

    # ---- overall comparison
    def comparison(block_new, block_old):
        rows, recs = [], []
        for label, path, kind in ROWS:
            if kind == "prop" and path in ("unsound", "practical_chances", "blunders_confirmed_by_sf") and not meta.get("sf"):
                continue
            if kind == "mean0" and not meta.get("sf"):
                continue
            cells, rec = compare_row(label, path, kind, block_new or {}, block_old or {})
            rows.append([label] + cells)
            recs.append(rec)
        return rows, recs

    md.append("## Sacrifices: all games (anchors + head-to-head)\n")
    md.append("Rates are per game unless stated; proportions with Wilson 95% intervals (scores: Wilson on score "
              "points, draws = 1/2), means with normal-approximation 95% intervals; `new - old` with Newcombe "
              "(proportions, scores) or Welch (means) intervals. Head-to-head games appear in both columns (once from "
              "each side).\n")
    rows, recs = comparison(an_new["overall"], an_old["overall"])
    md.append(table(["metric", "new", "old", "new - old"], rows) + "\n")
    rep["overall"] = {"comparison": recs, "new": an_new["overall"], "old": an_old["overall"]}

    # ---- per position context, per 100 engine moves
    md.append("### Per position context (per 100 engine moves)\n")
    md.append("Per-game counts depend on how long each engine spends in balanced positions (a weaker engine spends "
              "more of its games losing). Here each engine's own moves are split by its own score when it moved "
              "(balanced = within +-3 pawns, winning/losing beyond), and sacrifices and blunders are counted per 100 "
              "engine moves in each context. 'Material given' adds sacrifices and blunders: every persistent material "
              "deficit the engine started, whatever the intent filter says.\n")
    crows, cjson = [], {}
    for role in (NEW, OLD):
        short = role.split("_")[1]
        r = context_rates(run_dir, role, os.path.join(run_dir, f"analysis_{short}.events.jsonl"))
        cjson[role] = r
        for c in ("balanced", "winning", "losing"):
            m, sc, bl = r["moves"].get(c, 0), r["sacrifices"].get(c, 0), r["blunders"].get(c, 0)
            per = (lambda x: f"{100 * x / m:.2f}") if m else (lambda x: "n/a")
            crows.append([short, c, str(m), f"{sc} ({per(sc)})", f"{bl} ({per(bl)})", per(sc + bl)])
    rep["per_context"] = cjson
    md.append(table(["engine", "context", "engine moves", "sacrifices (per 100)", "blunders (per 100)",
                     "material given per 100"], crows) + "\n")

    # ---- per category
    md.append("### Per category\n")
    crow = []
    for c in CATS:
        r = [c]
        for an in (an_new, an_old):
            b = get(an, f"overall.per_category.{c}", {})
            r.append(str(b.get("count", 0)))
            r.append(f_prop(b.get("successful_attack")))
            r.append(f_prop(b.get("mate")))
            if meta.get("sf"):
                r.append(f_prop(b.get("unsound")))
        crow.append(r)
    hdr = ["category"]
    for who in ("new", "old"):
        hdr += [f"{who} n", f"{who} successful", f"{who} mate"] + ([f"{who} unsound"] if meta.get("sf") else [])
    md.append(table(hdr, crow) + "\n")

    # ---- per opponent
    md.append("## Per opponent\n")
    opp_new = an_new.get("per_opponent", {})
    opp_old = an_old.get("per_opponent", {})
    labels = []
    for name in list(opp_new) + list(opp_old):
        lab = opponent_label(name, meta)
        if lab not in labels:
            labels.append(lab)
    labels.sort(key=lambda l: (l == "head-to-head", l))
    rep["per_opponent"] = {}
    short = {"Games analysed", "Score (all games)", "Games with >= 1 sacrifice", "Sacrifices per game",
             "Balanced-position sacrifices per game (own eval within +-3)", "  queen sacrifices", "  exchange sacrifices",
             "  piece sacrifices", "  pawn sacrifices", "Blunders per game", "Mate within 30 plies (of sacrifices)",
             "Successful attacks", "Unsound (SF; of SF-evaluated sacrifices)", "Practical chances (of unsound)",
             "Score in games with a sacrifice", "Score in games without a sacrifice"}
    for lab in labels:
        bn = next((v for k, v in opp_new.items() if opponent_label(k, meta) == lab), None)
        bo = next((v for k, v in opp_old.items() if opponent_label(k, meta) == lab), None)
        title = "head-to-head (new vs old; each column is that engine's side)" if lab == "head-to-head" else f"vs {lab}"
        md.append(f"### {title}\n")
        rows, recs = comparison(bn, bo)
        rows = [r for r in rows if r[0] in short]
        md.append(table(["metric", "new", "old", "new - old"], rows) + "\n")
        rep["per_opponent"][lab] = {"comparison": recs, "new": bn, "old": bo}

    # ---- examples
    md.append("## Example sacrifices\n")
    md.append("Balanced-position sacrifices first, larger categories first. `e0 -> e1` = the engine's own score "
              "when it played the move and one move later; SF = Stockfish before -> after the move (engine view).\n")
    rep["examples"] = {}
    for role in (NEW, OLD):
        ex = notable(os.path.join(run_dir, f"analysis_{role.split('_')[1]}.events.jsonl"))
        rep["examples"][role] = ex
        if not ex:
            md.append(f"**{role}**: no sacrifices.\n")
            continue
        rows = []
        for e in ex:
            sf = "-" if e.get("sf_before") is None else f"{e['sf_before']} -> {e['sf_after']}"
            out = e.get("success_reason") or ("unsound" if e.get("unsound") else "-")
            rows.append([f"{e['game_id']}", e["opponent"], e["san"], e["category"], e["context"], str(e["d4"]),
                         f"{e['e0']} -> {e['e1']}", sf, f"{e['result']}, {out}", f"`{e['line']}`"])
        md.append(f"**{role}**\n")
        md.append(table(["game", "opponent", "move", "category", "context", "d4", "e0 -> e1", "SF", "result, success", "line"], rows) + "\n")

    # ---- notes
    md.append("## Definitions and caveats\n")
    md.append("""* Detection (`aggro_arena analyze`, see testing/README.md): `bal(i)` = engine material minus opponent
  material (P=100 N=320 B=330 R=500 Q=900) before ply i; a sacrifice starts at an engine ply k outside the opening
  file with a persistent deficit d2 = bal(k)-bal(k+2), d4 = bal(k)-bal(k+4) (pawn: also d6), categorised by the
  piece-composition change k..k+4 (queen > exchange > piece > pawn). A move whose own engine score drops by more
  than 1.5 pawns one move later is a *blunder*, not a sacrifice. Plies k+2 and k+4 after a sacrifice only extend
  its chain.
* Additions to the spec rule: two trade guards (trades whose recapture is delayed by an in-between move are
  not sacrifices) and the `context` of each sacrifice from the engine's own eval (balanced = within +-3 pawns).
  AggroChess deliberately looks for a sacrifice in every game and accepts almost any material sacrifice once
  the game is decided, so the *balanced-position* rows are the cleanest measure of chosen, game-relevant
  sacrifices; sacrifices in lost positions are often just material it was going to lose anyway.
* Successful attack = mate within 30 plies, or material recouped within 24 plies with the engine's own score
  not worse than at the sacrifice (-50), or SF >= +150 for the engine 12 plies later. Unsound = SF after the
  move < SF before - 150 and < -100. Practical chance = unsound but the game was not lost, or SF >= +100 for the
  engine at a later engine move within 30 plies.
* Proportions over sacrifices treat sacrifices as independent; several sacrifices in one game are correlated,
  so those intervals are somewhat optimistic. Per-game rates (games with a sacrifice, sacrifices per game,
  score) use the game as the unit. New and old play the same anchor openings, but the intervals for `new - old`
  assume independent samples (conservative); head-to-head games are shared by both columns.
* The engines' own scores differ in scale (the original engine's evaluation is much more optimistic than
  Stockfish), which affects the intent filter and the `context` split.
""")
    return "\n".join(md), rep


# ----------------------------------------------------------------------------- main

def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--new", default=NEW_DEFAULT, help="modified engine binary (copied into the run dir)")
    ap.add_argument("--old", default=OLD_DEFAULT, help="original engine binary (copied into the run dir)")
    ap.add_argument("--new-options", default="", help="extra UCI options for the new engine: 'Name=Value;Name=Value'")
    ap.add_argument("--old-options", default="", help="extra UCI options for the old engine")
    ap.add_argument("--engine-cwd", default=ROOT, help="working directory of both engines (book lookup)")
    ap.add_argument("--games", type=int, default=100, help="games per engine per anchor (default 100)")
    ap.add_argument("--h2h-games", type=int, default=100, help="head-to-head games (default 100)")
    ap.add_argument("--tc", default="10+0.1", help="time control (default 10+0.1)")
    ap.add_argument("--concurrency", type=int, default=14, help="parallel games (default 14)")
    ap.add_argument("--anchors", default="1800,2200", help="Stockfish UCI_Elo anchors, comma separated (default 1800,2200)")
    ap.add_argument("--sf", default=SF_DEFAULT, help="Stockfish binary (anchors and analysis)")
    ap.add_argument("--sf-nodes", type=int, default=400000, help="Stockfish nodes per analysed position")
    ap.add_argument("--sf-threads", type=int, default=12, help="parallel Stockfish processes for the analysis")
    ap.add_argument("--no-sf", action="store_true", help="analyse without Stockfish (no unsound/practical metrics)")
    ap.add_argument("--seed", type=int, default=1, help="opening seed")
    ap.add_argument("--openings", default=OPENINGS_DEFAULT)
    ap.add_argument("--arena", default=ARENA_DEFAULT)
    ap.add_argument("--out", default=None, help="run directory (default testing/runs/sactest-<timestamp>; "
                                                 "with --skip-matches: the latest one)")
    ap.add_argument("--report", default=REPORT_DEFAULT, help="report prefix (writes .md and .json)")
    ap.add_argument("--skip-matches", action="store_true", help="re-analyse an existing run directory")
    ap.add_argument("--dry-run", action="store_true", help="write configs and print the plan, play nothing")
    args = ap.parse_args()

    signal.signal(signal.SIGTERM, _on_signal)
    signal.signal(signal.SIGINT, _on_signal)
    args.openings = os.path.abspath(args.openings)
    args.arena = os.path.abspath(args.arena)
    ensure_arena(args.arena)
    use_sf = not args.no_sf

    if args.skip_matches:
        run_dir = args.out
        if not run_dir:
            cands = sorted(d for d in glob.glob(os.path.join(RUNS_DIR, "sactest-*")) if os.path.exists(os.path.join(d, "run.json")))
            if not cands:
                raise SystemExit("--skip-matches: no previous run found in testing/runs/")
            run_dir = cands[-1]
        run_dir = os.path.abspath(run_dir)
        with open(os.path.join(run_dir, "run.json")) as f:
            meta = json.load(f)
        log(f"re-analysing {run_dir}")
    else:
        anchors = [int(a) for a in args.anchors.split(",") if a.strip()]
        for v in (args.games, args.h2h_games):
            if v < 0:
                raise SystemExit("game counts must be >= 0")
        if args.games == 0 and args.h2h_games == 0:
            raise SystemExit("nothing to play")
        run_dir = os.path.abspath(args.out or os.path.join(RUNS_DIR, "sactest-" + time.strftime("%Y%m%d-%H%M%S")))
        if " " in run_dir or " " in args.openings:
            raise SystemExit("paths with spaces are not supported by the match config format")
        if os.path.exists(os.path.join(run_dir, "run.json")):
            raise SystemExit(f"{run_dir} already contains a run (use --skip-matches to re-analyse, or another --out)")
        created = not os.path.exists(run_dir)
        os.makedirs(os.path.join(run_dir, "bin"), exist_ok=True)
        try:
            engines = freeze_engines(run_dir, args)
        except BaseException:
            if created:  # do not leave a half-made run directory behind
                shutil.rmtree(run_dir, ignore_errors=True)
            raise
        sf_id, _ = uci_probe(args.sf, run_dir)
        book = os.path.join(args.engine_cwd, "ph-gambitbook.bin")
        meta = {
            "tool": "sactest.py",
            "started": utc_now(),
            "finished": None,
            "run_dir": run_dir,
            "tc": args.tc,
            "games_per_anchor": args.games,
            "h2h_games": args.h2h_games,
            "anchors": anchors,
            "concurrency": args.concurrency,
            "seed": args.seed,
            "openings": args.openings,
            "personality": args.new_options or "default",
            "engines": engines,
            "stockfish": {"path": args.sf, "id_name": sf_id, "sha256": sha256(args.sf)},
            "book": {"path": book, "sha256": sha256(book)} if os.path.exists(book) else None,
            "arena": {"path": args.arena, "sha256": sha256(args.arena)},
            "environment": {
                "host": socket.gethostname(),
                "cpu_model": cpu_model(),
                "logical_cpus": os.cpu_count(),
                "mem_total_gb": mem_total_gb(),
                "os": f"{platform.system()} {platform.release()}",
                "python": platform.python_version(),
            },
            "matches": {},
        }

        # ---- configs
        sf_engines = {}
        for a in anchors:
            sf_engines[f"sf{a}"] = {"path": args.sf, "cwd": None,
                                    "options": [("Threads", "1"), ("Hash", "16"), ("UCI_LimitStrength", "true"), ("UCI_Elo", str(a))]}
        plan = []
        if args.games > 0 and anchors:
            for role in (NEW, OLD):
                stem = f"gauntlet_{role.split('_')[1]}"
                engs = {role: engines[role], **sf_engines}
                pairs = [(role, f"sf{a}", args.games) for a in anchors]
                write_config(os.path.join(run_dir, stem + ".cfg"), f"sactest gauntlet {role}", engs, pairs, args, args.seed, stem)
                plan.append((stem, sum(p[2] + p[2] % 2 for p in pairs)))
        if args.h2h_games > 0:
            write_config(os.path.join(run_dir, "h2h.cfg"), "sactest head-to-head", {NEW: engines[NEW], OLD: engines[OLD]},
                         [(NEW, OLD, args.h2h_games)], args, args.seed + 1000, "h2h")
            plan.append(("h2h", args.h2h_games + args.h2h_games % 2))
        meta["plan"] = [{"match": s, "games": g} for s, g in plan]
        with open(os.path.join(run_dir, "run.json"), "w") as f:
            json.dump(meta, f, indent=2)

        total = sum(g for _, g in plan)
        try:
            base_s, inc_s = (float(x) for x in (args.tc.split("+") + ["0"])[:2])
            est = total * 2 * (base_s + 60 * inc_s) * 0.8 / max(1, args.concurrency)
            log(f"plan: {', '.join(f'{s} {g} games' for s, g in plan)}; {total} games at {args.tc}, "
                f"concurrency {args.concurrency}: roughly {est / 60:.0f} min")
        except ValueError:
            log(f"plan: {total} games")
        if args.dry_run:
            log(f"dry run: configs written to {run_dir}")
            return 0

        # ---- matches
        for stem, games in plan:
            log(f"\n=== {stem}: {games} games ===")
            t0 = time.time()
            rc = run_logged([args.arena, "match", os.path.join(run_dir, stem + ".cfg")], run_dir, os.path.join(run_dir, stem + ".log"))
            meta["matches"][stem] = {"exit_code": rc, "duration_s": round(time.time() - t0, 1)}
            with open(os.path.join(run_dir, "run.json"), "w") as f:
                json.dump(meta, f, indent=2)
            if rc == 130:
                raise Interrupted()
            if rc != 0:
                raise SystemExit(f"{stem}: aggro_arena match exited with {rc} (see {stem}.log)")

    # ---- analysis
    meta["sf"] = args.sf if use_sf else None
    meta["sf_nodes"] = args.sf_nodes if use_sf else None
    meta["sf_threads"] = args.sf_threads if use_sf else None
    analyses = {}
    for role in (NEW, OLD):
        short = role.split("_")[1]
        inputs = [os.path.join(run_dir, f) for f in (f"gauntlet_{short}.jsonl", "h2h.jsonl") if os.path.exists(os.path.join(run_dir, f))]
        if not inputs:
            raise SystemExit(f"no game files for {role} in {run_dir}")
        prefix = os.path.join(run_dir, f"analysis_{short}")
        cmd = [args.arena, "analyze", *inputs, "--engine", role, "--out", prefix]
        if use_sf:
            cmd += ["--sf", args.sf, "--sf-nodes", str(args.sf_nodes), "--threads", str(args.sf_threads)]
        log(f"\n=== analyze {role} ===")
        logf = os.path.join(run_dir, f"analysis_{short}.log")
        if os.path.exists(logf):
            os.remove(logf)
        rc = run_logged(cmd, run_dir, logf)
        if rc == 130:
            raise Interrupted()
        if rc != 0:
            raise SystemExit(f"analyze {role} failed with exit code {rc} (see {logf})")
        with open(prefix + ".json") as f:
            analyses[role] = json.load(f)
    meta["finished"] = utc_now()
    meta["analysis"] = {r: os.path.join(run_dir, f"analysis_{r.split('_')[1]}.json") for r in (NEW, OLD)}
    with open(os.path.join(run_dir, "run.json"), "w") as f:
        json.dump(meta, f, indent=2)

    # ---- report
    md, rep = build_report(run_dir, meta, analyses[NEW], analyses[OLD], args)
    report = os.path.abspath(args.report)
    os.makedirs(os.path.dirname(report), exist_ok=True)
    for path in (report, os.path.join(run_dir, "sacrifice_report")):
        with open(path + ".md", "w") as f:
            f.write(md)
        with open(path + ".json", "w") as f:
            json.dump(rep, f, indent=2)
    log(f"\nreport: {report}.md / .json (copies in {run_dir})")
    return 0


if __name__ == "__main__":
    try:
        sys.exit(main())
    except Interrupted:
        print("\ninterrupted", file=sys.stderr)
        sys.exit(130)
