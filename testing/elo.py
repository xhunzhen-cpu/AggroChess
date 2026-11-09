#!/usr/bin/env python3
"""elo.py -- joint maximum-likelihood Elo ratings from engine games (stdlib only).

Library + CLI. Reads the JSONL game records written by `aggro_arena match` (or any list of
(white, black, result) games) and fits one rating per player.

Model
-----
Logistic (Bradley-Terry) Elo with draws counted as half points. For a game between i and j
with score s in {0, 1/2, 1} for i the log-likelihood contribution is

    s*log(p) + (1-s)*log(1-p),        p = 1 / (1 + 10^(-(R_i - R_j)/400)).

The likelihood only depends on the number of games N_ij and the total score S_ij of every
unordered pair, so games are aggregated per pair before fitting.

* Anchors: any number of players can be fixed at given ratings (e.g. Stockfish UCI_Elo
  levels). All other players are free parameters. Without anchors the ratings are centred
  (mean 0).
* Players not connected to any anchor through games cannot be rated; they are reported as
  `unconnected` instead of getting a number.
* Prior for 0% / 100% scores: `prior_draws` virtual draws (default 0.01) are added to every
  pair of players that actually met. This only exists to keep the optimisation finite when a
  player (or a group) scored 0% / 100%; it is deliberately tiny. Measured on a simulated
  21-level calibration (levels far below the anchors, rated through a chain of adjacent
  levels, still play a few hopeless games vs the anchors): ONE virtual draw per pair biased
  the bottom levels by +130 Elo (bootstrap CI coverage 4/20), because the virtual draws
  against far-away opponents keep pulling while the 0% results carry almost no information;
  0.01-0.1 virtual draws were unbiased within noise (coverage 18-19/20), as was 0.
* Players whose plain MLE is infinite (no prior: they, or a group they belong to, never
  scored against / never conceded to the rest - see `unbounded_players`) get NO point
  estimate: `elo` is None and, for an individual 0% / 100% score, a conservative one-sided
  95% bound is reported instead (the rating at which losing / winning every one of those
  games has probability 5%, opponents at their fitted ratings). The prior-driven value is
  kept as `elo_penalized` for debugging only.
* Fitting: Newton-Raphson on the (concave) penalised log-likelihood with step clamping and
  a backtracking line search; converges in a handful of iterations.

Uncertainty
-----------
1. Fisher: covariance = inverse of the observed information (negative Hessian of the
   penalised log-likelihood) over the free players; for a no-anchor fit the pseudo-inverse
   under the mean-zero constraint. 95% = +-1.96 SE. With many draws this is conservative,
   because the score-based likelihood treats a draw like a half win + half loss.
2. Game-pair bootstrap (headline): the unit is one opening played twice with colours
   reversed (`pairing`, `game_pair` in the arena JSONL, per input file). Within every
   unordered player pair the units are resampled with replacement (same count), the joint
   MLE is refitted, and the 2.5 / 97.5 percentiles of each rating are reported
   (default 200 resamples, fixed seed => reproducible). Measured coverage (300 simulated
   21-level calibrations, truth known): about 93% rather than 95% overall, 91-93% for levels
   rated through a long chain far below the anchors, where the truth lies above the interval
   2-3 times more often than below it (1000 resamples: 93.6%). The Fisher interval is
   conservative (96-99.7%); for extrapolated levels consider the wider of the two.

Also: per-pairing W/D/L -> Elo difference with a 95% CI (trinomial normal approximation,
identical to aggro_arena's summary) and LOS.

CLI
---
    python3 testing/elo.py runs/x/*.jsonl [more.jsonl | run_dir]... \
        [--anchor NAME=ELO]... [--anchor-regex 'sf(\\d+)'] [--prior-draws 0.01] \
        [--bootstrap 200] [--seed 1] [--pairs] [--json out.json]

A directory argument means every *.jsonl file directly inside it.
"""
from __future__ import annotations

import argparse
import collections
import json
import math
import os
import random
import re
import sys
from typing import Dict, Iterable, List, NamedTuple, Optional, Sequence, Tuple

C = math.log(10.0) / 400.0  # d/dR of the logistic in Elo units
Z95 = 1.959964
RESULT_SCORE = {"1-0": 1.0, "0-1": 0.0, "1/2-1/2": 0.5}
MAX_ABS_RATING = 20000.0  # numeric guard (only reachable with prior_draws=0 and perfect scores)


# ---------------------------------------------------------------- basic formulas

def expected_score(diff: float) -> float:
    """Expected score of a player rated `diff` Elo above the opponent."""
    diff = max(-8000.0, min(8000.0, diff))
    return 1.0 / (1.0 + 10.0 ** (-diff / 400.0))


def elo_from_score(s: float) -> Optional[float]:
    """Elo difference for a score fraction; None for 0, 1 or invalid input."""
    if s is None or not (0.0 < s < 1.0) or not math.isfinite(s):
        return None
    return -400.0 * math.log10(1.0 / s - 1.0)


def los(w: int, l: int) -> Optional[float]:
    """Likelihood of superiority from decisive games (normal approximation)."""
    if w + l == 0:
        return None
    return 0.5 * (1.0 + math.erf((w - l) / math.sqrt(2.0 * (w + l))))


def wdl_stats(w: int, d: int, l: int) -> dict:
    """W/D/L -> score, Elo difference and 95% CI (trinomial normal approximation), LOS.

    Same formulas as aggro_arena's summary: per-game variance
    var = w*(1-s)^2 + d*(1/2-s)^2 + l*s^2 (fractions), SE = sqrt(var/n), CI = s +- 1.96 SE
    mapped through the logistic. When every game had the same result the variance is 0 and
    that interval would collapse to a point (all draws) or claim an unbounded upper end
    (0-N); the Wilson score interval on score points is used instead (`ci_method`). A bound
    whose score falls outside (0, 1) is None with `elo_lo_unbounded` / `elo_hi_unbounded`
    True; None with the flag False means "not available"."""
    n = w + d + l
    if n == 0:
        return {"games": 0, "wins": 0, "draws": 0, "losses": 0, "score": None, "draw_ratio": None,
                "elo": None, "elo_lo": None, "elo_hi": None, "elo_lo_unbounded": False, "elo_hi_unbounded": False,
                "ci_method": None, "score_se": None, "los": None}
    wf, df, lf = w / n, d / n, l / n
    s = wf + 0.5 * df
    var = wf * (1 - s) ** 2 + df * (0.5 - s) ** 2 + lf * s ** 2
    se = math.sqrt(var / n)
    if var <= 1e-12:
        lo_s, hi_s = wilson(s, n)
        method = "wilson (all games had the same result)"
    else:
        lo_s, hi_s = s - Z95 * se, s + Z95 * se
        method = "trinomial normal"
    return {"games": n, "wins": w, "draws": d, "losses": l, "score": s, "draw_ratio": df,
            "elo": elo_from_score(s), "elo_lo": elo_from_score(lo_s), "elo_hi": elo_from_score(hi_s),
            "elo_lo_unbounded": lo_s <= 0.0, "elo_hi_unbounded": hi_s >= 1.0, "ci_method": method,
            "score_se": se, "los": los(w, l)}


def wilson(p: float, n: float) -> Tuple[float, float]:
    """Wilson 95% score interval for a fraction p observed over n trials."""
    z2 = Z95 * Z95
    den = 1.0 + z2 / n
    centre = (p + z2 / (2.0 * n)) / den
    half = Z95 * math.sqrt(p * (1.0 - p) / n + z2 / (4.0 * n * n)) / den
    lo, hi = centre - half, centre + half
    # p = 0 / p = 1 give exactly 0 / 1 mathematically; snap the rounding error
    return (0.0 if lo < 1e-9 else lo), (1.0 if hi > 1.0 - 1e-9 else hi)


def pair_ci_str(st: dict, spec: str = "+.0f") -> str:
    """'[lo, hi]' of a wdl_stats result; an end is +-inf only if it is really unbounded."""
    if st.get("elo_lo") is None and st.get("elo_hi") is None and not (st.get("elo_lo_unbounded") or st.get("elo_hi_unbounded")):
        return "n/a"
    lo = fmt(st.get("elo_lo"), spec, "-inf" if st.get("elo_lo_unbounded") else "n/a")
    hi = fmt(st.get("elo_hi"), spec, "+inf" if st.get("elo_hi_unbounded") else "n/a")
    return f"[{lo}, {hi}]"


def percentile(sorted_vals: Sequence[float], q: float) -> Optional[float]:
    """Linear-interpolation percentile (numpy's default) of an already sorted list."""
    if not sorted_vals:
        return None
    k = (len(sorted_vals) - 1) * q / 100.0
    f = int(math.floor(k))
    c = min(f + 1, len(sorted_vals) - 1)
    lo, hi = sorted_vals[f], sorted_vals[c]
    if lo == hi or k == f:
        return lo
    if not (math.isfinite(lo) and math.isfinite(hi)):
        return lo if q < 50 else hi   # interpolating towards +-inf: take the outer value
    return lo + (hi - lo) * (k - f)


# ---------------------------------------------------------------- games

class Game(NamedTuple):
    white: str
    black: str
    score: float          # white's score: 1, 0.5, 0
    unit: tuple           # bootstrap unit (games sharing an opening pair share the unit)
    tc: str = ""
    termination: str = ""


def load_jsonl(path: str, source: Optional[str] = None, warn=None) -> List[Game]:
    """Read arena JSONL records. Lines that are not valid JSON game records are skipped
    (a truncated last line after a crash) and reported through `warn`."""
    src = source if source is not None else os.path.abspath(path)
    games: List[Game] = []
    bad = 0
    with open(path, encoding="utf-8") as fh:
        for line in fh:
            line = line.strip()
            if not line:
                continue
            try:
                rec = json.loads(line)
                score = RESULT_SCORE[rec["result"]]
                white, black = rec["white"], rec["black"]
            except (ValueError, KeyError, TypeError):
                bad += 1
                continue
            gp = rec.get("game_pair")
            unit = (src, rec.get("pairing", 0), gp if gp is not None else ("id", rec.get("id")))
            games.append(Game(white, black, score, unit, rec.get("tc", ""), rec.get("termination", "")))
    if bad and warn:
        warn(f"{path}: skipped {bad} malformed line(s)")
    return games


def expand_paths(paths: Iterable[str]) -> List[str]:
    out: List[str] = []
    for p in paths:
        if os.path.isdir(p):
            out.extend(sorted(os.path.join(p, f) for f in os.listdir(p) if f.endswith(".jsonl")))
        else:
            out.append(p)
    return out


def pair_key(a: str, b: str) -> Tuple[str, str]:
    return (a, b) if a <= b else (b, a)


def aggregate(games: Iterable[Game]) -> Dict[Tuple[str, str], Tuple[float, float]]:
    """{(a, b) sorted: (games, score of a)}"""
    agg: Dict[Tuple[str, str], List[float]] = {}
    for g in games:
        if g.white == g.black:
            continue
        k = pair_key(g.white, g.black)
        s_a = g.score if g.white == k[0] else 1.0 - g.score
        slot = agg.setdefault(k, [0.0, 0.0])
        slot[0] += 1.0
        slot[1] += s_a
    return {k: (v[0], v[1]) for k, v in agg.items()}


def wdl_by_pair(games: Iterable[Game]) -> Dict[Tuple[str, str], List[int]]:
    """{(a, b) sorted: [wins, draws, losses] from a's view}"""
    out: Dict[Tuple[str, str], List[int]] = collections.defaultdict(lambda: [0, 0, 0])
    for g in games:
        if g.white == g.black:
            continue
        k = pair_key(g.white, g.black)
        s_a = g.score if g.white == k[0] else 1.0 - g.score
        out[k][0 if s_a == 1.0 else 1 if s_a == 0.5 else 2] += 1
    return dict(out)


def wdl_by_player(games: Iterable[Game]) -> Dict[str, List[int]]:
    out: Dict[str, List[int]] = collections.defaultdict(lambda: [0, 0, 0])
    for g in games:
        if g.white == g.black:
            continue
        for name, s in ((g.white, g.score), (g.black, 1.0 - g.score)):
            out[name][0 if s == 1.0 else 1 if s == 0.5 else 2] += 1
    return dict(out)


# ---------------------------------------------------------------- linear algebra (small dense)

def _solve(a: List[List[float]], b: List[float]) -> Optional[List[float]]:
    n = len(b)
    m = [row[:] + [b[i]] for i, row in enumerate(a)]
    for col in range(n):
        piv = max(range(col, n), key=lambda r: abs(m[r][col]))
        if abs(m[piv][col]) < 1e-300:
            return None
        m[col], m[piv] = m[piv], m[col]
        inv = 1.0 / m[col][col]
        for r in range(col + 1, n):
            f = m[r][col] * inv
            if f != 0.0:
                row_r, row_c = m[r], m[col]
                for k in range(col, n + 1):
                    row_r[k] -= f * row_c[k]
    x = [0.0] * n
    for i in range(n - 1, -1, -1):
        x[i] = (m[i][n] - sum(m[i][k] * x[k] for k in range(i + 1, n))) / m[i][i]
    return x


def _inverse(a: List[List[float]]) -> Optional[List[List[float]]]:
    n = len(a)
    m = [row[:] + [1.0 if i == j else 0.0 for j in range(n)] for i, row in enumerate(a)]
    for col in range(n):
        piv = max(range(col, n), key=lambda r: abs(m[r][col]))
        if abs(m[piv][col]) < 1e-300:
            return None
        m[col], m[piv] = m[piv], m[col]
        inv = 1.0 / m[col][col]
        m[col] = [v * inv for v in m[col]]
        for r in range(n):
            if r != col and m[r][col] != 0.0:
                f = m[r][col]
                row_r, row_c = m[r], m[col]
                for k in range(2 * n):
                    row_r[k] -= f * row_c[k]
    return [row[n:] for row in m]


def _components(players: Iterable[str], pairs: Iterable[Tuple[str, str]]) -> List[List[str]]:
    parent = {p: p for p in players}

    def find(x):
        while parent[x] != x:
            parent[x] = parent[parent[x]]
            x = parent[x]
        return x

    for a, b in pairs:
        ra, rb = find(a), find(b)
        if ra != rb:
            parent[ra] = rb
    comps: Dict[str, List[str]] = collections.defaultdict(list)
    for p in sorted(parent):
        comps[find(p)].append(p)
    return sorted(comps.values(), key=lambda c: (-len(c), c[0]))


# ---------------------------------------------------------------- fitting

class Fit(NamedTuple):
    ratings: Dict[str, float]            # rated players (anchors included)
    fisher_se: Dict[str, Optional[float]]  # 0.0 for anchors, None if not computable
    anchors: Dict[str, float]
    unconnected: List[str]               # players without a path to an anchor
    loglik: float
    iterations: int
    converged: bool
    prior_draws: float


DEFAULT_PRIOR_DRAWS = 0.01
# A percentile bootstrap over fewer resampling units than this is not reported (with 2-4 opening
# pairs the resampled ratings often coincide and the "interval" collapses to a single value).
MIN_BOOT_UNITS = 5
# ... nor one whose resamples take fewer distinct values than this (e.g. every opening pair of a
# pairing had the same result, so every resample reproduces the data exactly).
MIN_BOOT_DISTINCT = 5


def fit_ratings(data, anchors: Optional[Dict[str, float]] = None, prior_draws: float = DEFAULT_PRIOR_DRAWS,
                init: Optional[Dict[str, float]] = None, tol: float = 1e-6, max_iter: int = 200,
                want_cov: bool = True) -> Fit:
    """Joint MLE. `data` is a list of Game or the output of `aggregate`."""
    if not isinstance(data, dict):
        data = aggregate(data)
    anchors = dict(anchors or {})
    players = sorted({p for k in data for p in k})
    comps = _components(players, [k for k, v in data.items() if v[0] > 0])
    present_anchors = {p: float(r) for p, r in anchors.items() if p in set(players)}
    if present_anchors:
        kept = [c for c in comps if any(p in present_anchors for p in c)]
        centre = False
    else:
        kept = comps[:1]
        centre = True
    kept_set = {p for c in kept for p in c}
    unconnected = [p for p in players if p not in kept_set]
    free = [p for p in sorted(kept_set) if p not in present_anchors]
    idx = {p: i for i, p in enumerate(free)}
    m = len(free)

    # penalised data: prior virtual draws per pair that met
    aug = []
    for (a, b), (n, s) in sorted(data.items()):
        if a in kept_set and b in kept_set and n > 0:
            aug.append((a, b, n + prior_draws, s + 0.5 * prior_draws))

    base = (sum(present_anchors.values()) / len(present_anchors)) if present_anchors else 0.0
    r = dict(present_anchors)
    for p in free:
        v = (init or {}).get(p)
        r[p] = float(v) if v is not None and math.isfinite(v) else base

    def loglik(rr):
        ll = 0.0
        for a, b, n, s in aug:
            p = expected_score(rr[a] - rr[b])
            p = min(max(p, 1e-300), 1.0 - 1e-16)
            ll += s * math.log(p) + (n - s) * math.log(1.0 - p)
        return ll

    def grad_info(rr):
        g = [0.0] * m
        h = [[0.0] * m for _ in range(m)]
        for a, b, n, s in aug:
            p = expected_score(rr[a] - rr[b])
            gr = C * (s - n * p)
            w = C * C * n * p * (1.0 - p)
            ia, ib = idx.get(a), idx.get(b)
            if ia is not None:
                g[ia] += gr
                h[ia][ia] += w
            if ib is not None:
                g[ib] -= gr
                h[ib][ib] += w
            if ia is not None and ib is not None:
                h[ia][ib] -= w
                h[ib][ia] -= w
        if centre and m:
            # information is singular along the all-ones direction: H + J/m is invertible and
            # gives the minimum-norm (mean-preserving) Newton step / the constrained covariance.
            for i in range(m):
                for j in range(m):
                    h[i][j] += 1.0 / m
        return g, h

    it = 0
    converged = m == 0
    ll = loglik(r)
    while not converged and it < max_iter:
        it += 1
        g, h = grad_info(r)
        step = _solve(h, g)
        if step is None:
            break
        mx = max(abs(x) for x in step) if step else 0.0
        if mx > 400.0:
            step = [x * 400.0 / mx for x in step]
            mx = 400.0
        t = 1.0
        while True:
            trial = dict(r)
            for p, i in idx.items():
                trial[p] = max(-MAX_ABS_RATING, min(MAX_ABS_RATING, r[p] + t * step[i]))
            ll_new = loglik(trial)
            if ll_new >= ll - 1e-12 or t < 1e-6:
                break
            t *= 0.5
        r, ll = trial, ll_new
        if mx * t < tol:
            converged = True

    if centre and r:
        mean = sum(r[p] for p in kept_set) / len(kept_set)
        for p in kept_set:
            r[p] -= mean

    se: Dict[str, Optional[float]] = {p: 0.0 for p in present_anchors}
    if want_cov and m:
        _, h = grad_info(r)
        cov = _inverse(h)
        for p, i in idx.items():
            if cov is None:
                se[p] = None
            else:
                v = cov[i][i] - (1.0 / m if centre else 0.0)
                se[p] = math.sqrt(v) if v > 0 else None
    elif not want_cov:
        se = {}
    ratings = {p: r[p] for p in sorted(kept_set)}
    return Fit(ratings, se, present_anchors, unconnected, ll, it, converged, prior_draws)


# ---------------------------------------------------------------- existence of the MLE, bounds

def unbounded_players(data, anchors: Optional[Dict[str, float]] = None) -> Dict[str, str]:
    """Players whose plain MLE (no prior) is infinite: {name: 'low' | 'high' | 'both'}.

    'low': the set of players reachable from p through "scored at least a draw against"
    contains no anchor -> that set never scored against anyone outside it and can be pushed
    to -inf. 'high' likewise through "conceded at least a draw to". Without anchors the set
    must cover p's whole connected component (the scoring graph must be strongly connected).
    Players not connected to an anchor at all are not listed (see Fit.unconnected)."""
    if not isinstance(data, dict):
        data = aggregate(data)
    scored: Dict[str, set] = collections.defaultdict(set)
    conceded: Dict[str, set] = collections.defaultdict(set)
    players = set()
    for (a, b), (n, s) in data.items():
        if n <= 0:
            continue
        players.update((a, b))
        if s > 0:
            scored[a].add(b)
            conceded[b].add(a)
        if s < n:
            scored[b].add(a)
            conceded[a].add(b)
    anchor_set = set(anchors or {}) & players
    comp_of = {}
    for comp in _components(players, [k for k, v in data.items() if v[0] > 0]):
        cs = set(comp)
        for p in comp:
            comp_of[p] = cs

    def closure(x, edges):
        seen, stack = {x}, [x]
        while stack:
            for y in edges.get(stack.pop(), ()):
                if y not in seen:
                    seen.add(y)
                    stack.append(y)
        return seen

    out = {}
    for p in sorted(players - anchor_set):
        if anchor_set:
            if not (comp_of[p] & anchor_set):
                continue
            low = not (closure(p, scored) & anchor_set)
            high = not (closure(p, conceded) & anchor_set)
        else:
            low = closure(p, scored) != comp_of[p]
            high = closure(p, conceded) != comp_of[p]
        if low or high:
            out[p] = "both" if low and high else ("low" if low else "high")
    return out


def perfect_score_bound(data, player: str, ratings: Dict[str, float], alpha: float = 0.05):
    """For a player with a 0% (100%) score: ('upper' | 'lower', R) such that losing (winning)
    every one of its games has probability alpha at rating R, opponents fixed at `ratings`.
    Conservative when draws are possible (P(loss) <= 1 - expected score). None otherwise."""
    if not isinstance(data, dict):
        data = aggregate(data)
    rows = []
    for (a, b), (n, s) in data.items():
        if n <= 0 or player not in (a, b):
            continue
        opp, sp = (b, s) if a == player else (a, n - s)
        if opp not in ratings or ratings[opp] is None:
            return None
        rows.append((ratings[opp], n, sp))
    if not rows:
        return None
    n_tot = sum(r[1] for r in rows)
    s_tot = sum(r[2] for r in rows)
    if s_tot == 0:
        kind = "upper"

        def f(r):  # log P(all losses) - log alpha, decreasing in r
            return sum(n * math.log(max(1e-300, 1.0 - expected_score(r - ro))) for ro, n, _ in rows) - math.log(alpha)
    elif s_tot == n_tot:
        kind = "lower"

        def f(r):  # log P(all wins) - log alpha, increasing in r -> negate to make it decreasing
            return -(sum(n * math.log(max(1e-300, expected_score(r - ro))) for ro, n, _ in rows) - math.log(alpha))
    else:
        return None
    lo = min(r[0] for r in rows) - 4000.0
    hi = max(r[0] for r in rows) + 4000.0
    if not (f(lo) > 0 > f(hi)):
        return None
    for _ in range(100):
        mid = 0.5 * (lo + hi)
        if f(mid) > 0:
            lo = mid
        else:
            hi = mid
    return kind, 0.5 * (lo + hi)


# ---------------------------------------------------------------- bootstrap

def bootstrap_units(games: Iterable[Game]) -> Dict[Tuple[str, str], List[Tuple[float, float]]]:
    """{(a, b) sorted: [(games, score of a) per unit]}; one unit = one opening pair."""
    units: Dict[Tuple[Tuple[str, str], tuple], List[float]] = {}
    for g in games:
        if g.white == g.black:
            continue
        k = pair_key(g.white, g.black)
        s_a = g.score if g.white == k[0] else 1.0 - g.score
        slot = units.setdefault((k, g.unit), [0.0, 0.0])
        slot[0] += 1.0
        slot[1] += s_a
    out: Dict[Tuple[str, str], List[Tuple[float, float]]] = collections.defaultdict(list)
    for (k, _u), (n, s) in sorted(units.items(), key=lambda kv: (kv[0][0], repr(kv[0][1]))):
        out[k].append((n, s))
    return dict(out)


def bootstrap(games: Sequence[Game], anchors: Optional[Dict[str, float]] = None,
              prior_draws: float = DEFAULT_PRIOR_DRAWS,
              n: int = 200, seed: int = 1, init: Optional[Dict[str, float]] = None) -> Dict[str, List[float]]:
    """Game-pair bootstrap. Returns {player: [rating per resample]} (anchors constant).
    In a resample where a player has no finite MLE (e.g. it happened to lose every resampled
    game that connects it to the anchors) its value is -inf / +inf instead of the prior-driven
    number; resamples in which it could go either way are left out for that player."""
    groups = bootstrap_units(games)
    rng = random.Random(seed)
    samples: Dict[str, List[float]] = collections.defaultdict(list)
    keys = sorted(groups)
    for _ in range(n):
        data = {}
        for k in keys:
            ul = groups[k]
            cnt = len(ul)
            tn = ts = 0.0
            for _j in range(cnt):
                un, us = ul[rng.randrange(cnt)]
                tn += un
                ts += us
            data[k] = (tn, ts)
        f = fit_ratings(data, anchors, prior_draws, init=init, want_cov=False)
        unb = unbounded_players(data, f.anchors) if f.anchors else unbounded_players(
            {k: v for k, v in data.items() if k[0] in f.ratings and k[1] in f.ratings})
        for p, v in f.ratings.items():
            u = unb.get(p)
            if u is None:
                samples[p].append(v)
            elif u == "low":
                samples[p].append(-math.inf)
            elif u == "high":
                samples[p].append(math.inf)
    return dict(samples)


def prob_not_above(samples: Dict[str, List[float]], lower: str, upper: str) -> Optional[float]:
    """Fraction of bootstrap resamples in which `upper` is rated <= `lower`."""
    a, b = samples.get(lower), samples.get(upper)
    if not a or not b or len(a) != len(b):
        return None
    return sum(1 for x, y in zip(a, b) if y <= x) / len(a)


# ---------------------------------------------------------------- full analysis

def analyze(games: Sequence[Game], anchors: Optional[Dict[str, float]] = None,
            prior_draws: float = DEFAULT_PRIOR_DRAWS,
            n_boot: int = 200, seed: int = 1, keep_samples: bool = False) -> dict:
    """Point estimates + Fisher + bootstrap CIs + per-pairing statistics as a JSON-able dict."""
    games = list(games)
    agg = aggregate(games)
    fit = fit_ratings(agg, anchors, prior_draws)
    samples = bootstrap(games, fit.anchors, prior_draws, n_boot, seed, init=fit.ratings) if n_boot > 0 else {}
    unb = unbounded_players(agg, fit.anchors) if fit.anchors else unbounded_players(
        {k: v for k, v in agg.items() if k[0] in fit.ratings and k[1] in fit.ratings})
    bounded = {p: v for p, v in fit.ratings.items() if p not in unb}
    units_p: Dict[str, int] = collections.defaultdict(int)
    for (a, b), ul in bootstrap_units(games).items():
        units_p[a] += len(ul)
        units_p[b] += len(ul)
    wdl_p = wdl_by_player(games)
    players = {}
    for p in sorted(set(fit.ratings) | set(fit.unconnected)):
        w, d, l = wdl_p.get(p, [0, 0, 0])
        n = w + d + l
        rec = {"games": n, "wins": w, "draws": d, "losses": l,
               "score": (w + 0.5 * d) / n if n else None,
               "anchored": p in fit.anchors, "unconnected": p in fit.unconnected,
               "elo": fit.ratings.get(p), "fisher_se": fit.fisher_se.get(p),
               "fisher_lo": None, "fisher_hi": None, "boot_lo": None, "boot_hi": None,
               "boot_median": None, "boot_n": 0, "boot_units": units_p.get(p, 0), "boot_note": None,
               "unbounded": unb.get(p), "bound_type": None, "bound": None, "elo_penalized": None}
        if p in unb:
            # the plain MLE is infinite: no point estimate, no interval; a bound for 0% / 100%
            rec["elo_penalized"], rec["elo"], rec["fisher_se"] = rec["elo"], None, None
            b = perfect_score_bound(agg, p, bounded)
            if b is not None:
                rec["bound_type"], rec["bound"] = b
            players[p] = rec
            continue
        if rec["elo"] is not None and rec["fisher_se"] is not None and not rec["anchored"]:
            rec["fisher_lo"] = rec["elo"] - Z95 * rec["fisher_se"]
            rec["fisher_hi"] = rec["elo"] + Z95 * rec["fisher_se"]
        sv = sorted(samples.get(p, []))
        distinct = len({round(v, 3) if math.isfinite(v) else v for v in sv})
        if sv and not rec["anchored"] and rec["boot_units"] < MIN_BOOT_UNITS:
            rec["boot_note"] = f"too few opening pairs for a bootstrap ({rec['boot_units']} < {MIN_BOOT_UNITS})"
        elif sv and not rec["anchored"] and distinct < MIN_BOOT_DISTINCT:
            rec["boot_note"] = f"degenerate bootstrap ({distinct} distinct resampled values)"
        elif sv and not rec["anchored"]:
            rec["boot_lo"] = percentile(sv, 2.5)
            rec["boot_hi"] = percentile(sv, 97.5)
            rec["boot_median"] = percentile(sv, 50.0)
            rec["boot_n"] = len(sv)
            # JSON has no infinities: keep the information in flags (the value becomes null)
            rec["boot_lo_unbounded"] = rec["boot_lo"] == -math.inf
            rec["boot_hi_unbounded"] = rec["boot_hi"] == math.inf
            rec["boot_resamples_without_finite_mle"] = sum(1 for v in sv if not math.isfinite(v))
        players[p] = rec
    pairs = []
    for (a, b), (w, d, l) in sorted(wdl_by_pair(games).items()):
        st = wdl_stats(w, d, l)
        ra, rb = bounded.get(a), bounded.get(b)
        st.update({"a": a, "b": b, "model_diff": (ra - rb) if ra is not None and rb is not None else None})
        pairs.append(st)
    out = {
        "method": {
            "model": "logistic Elo, draws = 1/2 point, joint MLE (Newton)",
            "prior": f"{prior_draws:g} virtual draw(s) per pair of players that met",
            "fisher": "inverse observed information of the penalised likelihood, 95% = +-1.96 SE",
            "bootstrap": f"{n_boot} resamples of opening pairs within each player pair, 2.5/97.5 percentiles, seed {seed}; "
                         f"not reported for players with fewer than {MIN_BOOT_UNITS} opening pairs or "
                         f"fewer than {MIN_BOOT_DISTINCT} distinct resampled values",
        },
        "anchors": fit.anchors, "converged": fit.converged, "iterations": fit.iterations,
        "loglik": fit.loglik, "games": len(games), "players": players, "pairs": pairs,
        "unconnected": fit.unconnected, "unbounded": unb,
    }
    if keep_samples:
        out["_samples"] = samples
    return out


# ---------------------------------------------------------------- formatting

def fmt(v: Optional[float], spec: str = "+.0f", na: str = "n/a") -> str:
    if v is None or (isinstance(v, float) and not math.isfinite(v)):
        return na
    x = 0.0 if abs(v) < 0.05 else v
    return format(x, spec)


def elo_cell(r: dict) -> str:
    """Point estimate, or the one-sided bound for a player without a finite estimate."""
    if r.get("elo") is not None:
        return fmt(r["elo"], ".0f")
    if r.get("bound") is not None:
        return ("< " if r["bound_type"] == "upper" else "> ") + fmt(r["bound"], ".0f")
    return "n/a"


def format_ratings(res: dict) -> str:
    rows = [("player", "Elo", "95% CI (bootstrap)", "Fisher +-", "games", "W/D/L", "score", "note")]
    def key(kv):
        v = kv[1]["elo"] if kv[1]["elo"] is not None else kv[1].get("bound")
        return (v is None, -(v or 0.0), kv[0])
    order = sorted(res["players"].items(), key=key)
    for p, r in order:
        ci = (f"[{fmt(r['boot_lo'], '.0f', '-inf')}, {fmt(r['boot_hi'], '.0f', '+inf')}]" if r["boot_lo"] is not None else
              ("anchor" if r["anchored"] else "n/a"))
        fse = fmt(Z95 * r["fisher_se"], ".0f") if r["fisher_se"] and not r["anchored"] else ("-" if r["anchored"] else "n/a")
        note = "anchor" if r["anchored"] else ("unconnected" if r["unconnected"] else "")
        if r.get("unbounded"):
            note = ("0% score: one-sided 95% bound" if r["bound_type"] == "upper" else
                    "100% score: one-sided 95% bound" if r["bound_type"] == "lower" else
                    f"no finite estimate ({r['unbounded']})")
        rows.append((p, elo_cell(r), ci, fse, str(r["games"]), f"{r['wins']}/{r['draws']}/{r['losses']}",
                     fmt(None if r["score"] is None else 100 * r["score"], ".1f") + ("%" if r["score"] is not None else ""),
                     note))
    return _table(rows)


def format_pairs(res: dict) -> str:
    rows = [("A", "B", "games", "+/=/-", "score", "Elo diff (95% CI)", "LOS", "model diff")]
    for st in res["pairs"]:
        rows.append((st["a"], st["b"], str(st["games"]), f"+{st['wins']} ={st['draws']} -{st['losses']}",
                     f"{100 * st['score']:.1f}%",
                     f"{fmt(st['elo'], '+.0f')} {pair_ci_str(st)}",
                     fmt(None if st["los"] is None else 100 * st["los"], ".1f") + ("%" if st["los"] is not None else ""),
                     fmt(st["model_diff"], "+.0f")))
    return _table(rows)


def _table(rows: List[tuple]) -> str:
    widths = [max(len(str(r[i])) for r in rows) for i in range(len(rows[0]))]
    lines = []
    for j, r in enumerate(rows):
        lines.append("  ".join(str(c).ljust(widths[i]) if i == 0 else str(c).rjust(widths[i])
                               for i, c in enumerate(r)).rstrip())
        if j == 0:
            lines.append("  ".join("-" * w for w in widths))
    return "\n".join(lines)


# ---------------------------------------------------------------- CLI

def _json_clean(o):
    if isinstance(o, float):
        return o if math.isfinite(o) else None
    if isinstance(o, dict):
        return {str(k): _json_clean(v) for k, v in o.items() if not str(k).startswith("_")}
    if isinstance(o, (list, tuple)):
        return [_json_clean(v) for v in o]
    return o


def main(argv=None) -> int:
    ap = argparse.ArgumentParser(description="Joint MLE Elo ratings from aggro_arena JSONL games.")
    ap.add_argument("inputs", nargs="+", help="JSONL files or directories (all *.jsonl inside)")
    ap.add_argument("--anchor", action="append", default=[], metavar="NAME=ELO",
                    help="fix a player's rating (repeatable)")
    ap.add_argument("--anchor-regex", default=None, metavar="RE",
                    help=r"anchor every player whose full name matches RE at int(group 1), e.g. 'sf(\d+)'")
    ap.add_argument("--prior-draws", type=float, default=DEFAULT_PRIOR_DRAWS,
                    help=f"virtual draws added per pair of players that met (default {DEFAULT_PRIOR_DRAWS}; 0 = plain MLE)")
    ap.add_argument("--bootstrap", type=int, default=200, help="game-pair bootstrap resamples (0 = off)")
    ap.add_argument("--seed", type=int, default=1)
    ap.add_argument("--pairs", action="store_true", help="also print the per-pairing table")
    ap.add_argument("--json", default=None, help="write the full result as JSON")
    a = ap.parse_args(argv)

    anchors: Dict[str, float] = {}
    for s in a.anchor:
        if "=" not in s:
            ap.error(f"--anchor needs NAME=ELO, got {s!r}")
        k, v = s.rsplit("=", 1)
        anchors[k.strip()] = float(v)
    games: List[Game] = []
    for path in expand_paths(a.inputs):
        games.extend(load_jsonl(path, warn=lambda m: print("warning:", m, file=sys.stderr)))
    if not games:
        print("no games found", file=sys.stderr)
        return 1
    if a.anchor_regex:
        rx = re.compile(a.anchor_regex)
        for p in {x for g in games for x in (g.white, g.black)}:
            mm = rx.fullmatch(p)
            if mm and p not in anchors:
                anchors[p] = float(mm.group(1))
    res = analyze(games, anchors, a.prior_draws, a.bootstrap, a.seed)
    print(f"{res['games']} games, {len(res['players'])} players, anchors: "
          + (", ".join(f"{k}={v:g}" for k, v in sorted(res['anchors'].items())) or "none (ratings centred on 0)"))
    print(f"prior: {res['method']['prior']}; bootstrap: {res['method']['bootstrap'] if a.bootstrap else 'off'}")
    if not res["converged"]:
        print("WARNING: the fit did not converge (perfect scores with --prior-draws 0?)")
    if res["unbounded"]:
        print("no finite rating (0%/100% against everyone they connect to): " + ", ".join(sorted(res["unbounded"])))
    print()
    print(format_ratings(res))
    if res["unconnected"]:
        print("\nnot connected to an anchor (no rating): " + ", ".join(res["unconnected"]))
    if a.pairs:
        print()
        print(format_pairs(res))
    if a.json:
        with open(a.json, "w", encoding="utf-8") as fh:
            json.dump(_json_clean(res), fh, indent=1)
            fh.write("\n")
    return 0


if __name__ == "__main__":
    sys.exit(main())
