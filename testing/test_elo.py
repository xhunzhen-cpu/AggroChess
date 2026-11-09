#!/usr/bin/env python3
"""Self-tests for elo.py:  python3 -m unittest discover -s testing -p 'test_*.py'   (from AggroChess/)
or  python3 testing/test_elo.py"""
import contextlib
import io
import json
import math
import os
import random
import sys
import tempfile
import unittest

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import elo  # noqa: E402


def games_from_counts(a, b, wins, draws, losses, source="t", pairing=0):
    """Games between a and b (a's view), colours alternating, two games per opening pair."""
    seq = [1.0] * wins + [0.5] * draws + [0.0] * losses
    out = []
    for i, s in enumerate(seq):
        unit = (source, pairing, i // 2)
        if i % 2 == 0:
            out.append(elo.Game(a, b, s, unit))
        else:
            out.append(elo.Game(b, a, 1.0 - s, unit))
    return out


def simulate(true, schedule, seed, draw_rate=0.3, source="sim"):
    """Random games from known ratings: P(win)/P(draw) consistent with the expected score."""
    rng = random.Random(seed)
    games = []
    for pi, (a, b, n) in enumerate(schedule):
        e = elo.expected_score(true[a] - true[b])
        d = min(draw_rate, 2 * min(e, 1 - e))
        pw = e - d / 2
        for i in range(n):
            u = rng.random()
            s = 1.0 if u < pw else (0.5 if u < pw + d else 0.0)
            unit = (source, pi, i // 2)
            games.append(elo.Game(a, b, s, unit) if i % 2 == 0 else elo.Game(b, a, 1.0 - s, unit))
    return games


class TestFormulas(unittest.TestCase):
    def test_75_percent(self):
        self.assertAlmostEqual(elo.elo_from_score(0.75), 190.85, places=2)
        self.assertAlmostEqual(elo.elo_from_score(0.25), -190.85, places=2)
        self.assertIsNone(elo.elo_from_score(1.0))
        self.assertIsNone(elo.elo_from_score(0.0))
        self.assertAlmostEqual(elo.expected_score(190.848501887865), 0.75, places=9)

    def test_wdl_stats(self):
        st = elo.wdl_stats(10, 10, 10)
        self.assertAlmostEqual(st["score"], 0.5)
        self.assertAlmostEqual(st["elo"], 0.0, places=9)
        self.assertAlmostEqual(st["los"], 0.5)
        self.assertAlmostEqual(st["elo_lo"], -st["elo_hi"], places=9)
        st = elo.wdl_stats(5, 0, 0)
        self.assertIsNone(st["elo"])
        self.assertIsNone(st["elo_hi"])
        # 60 W 20 D 20 L: score 0.7, per-game var = .6*.09 + .2*.04 + .2*.49 = 0.16
        st = elo.wdl_stats(60, 20, 20)
        self.assertAlmostEqual(st["score_se"], math.sqrt(0.16 / 100))
        self.assertAlmostEqual(st["elo"], elo.elo_from_score(0.7))
        self.assertAlmostEqual(st["elo_lo"], elo.elo_from_score(0.7 - 1.959964 * 0.04))
        self.assertAlmostEqual(elo.los(10, 0), 0.5 * (1 + math.erf(10 / math.sqrt(20))))

    def test_wdl_stats_zero_variance(self):
        # all draws: no zero-width interval
        st = elo.wdl_stats(0, 10, 0)
        self.assertEqual(st["ci_method"], "wilson (all games had the same result)")
        self.assertAlmostEqual(st["elo_lo"], -st["elo_hi"], places=6)
        self.assertGreater(st["elo_hi"], 150)
        # 0-10: unbounded below, but the upper end is finite (it was printed as +inf)
        st = elo.wdl_stats(0, 0, 10)
        self.assertIsNone(st["elo"])
        self.assertTrue(st["elo_lo_unbounded"])
        self.assertFalse(st["elo_hi_unbounded"])
        self.assertAlmostEqual(st["elo_hi"], elo.elo_from_score(elo.wilson(0.0, 10)[1]))
        self.assertEqual(elo.pair_ci_str(st), f"[-inf, {st['elo_hi']:+.0f}]")
        st = elo.wdl_stats(10, 0, 0)
        self.assertEqual(elo.pair_ci_str(st), f"[{st['elo_lo']:+.0f}, +inf]")
        self.assertEqual(elo.pair_ci_str(elo.wdl_stats(0, 0, 0)), "n/a")

    def test_percentile(self):
        v = [1.0, 2.0, 3.0, 4.0, 5.0]
        self.assertEqual(elo.percentile(v, 50), 3.0)
        self.assertEqual(elo.percentile(v, 0), 1.0)
        self.assertEqual(elo.percentile(v, 100), 5.0)
        self.assertAlmostEqual(elo.percentile(v, 2.5), 1.1)


class TestFit(unittest.TestCase):
    def test_two_players_75_percent(self):
        g = games_from_counts("A", "B", 60, 30, 10)   # 75 %
        f = elo.fit_ratings(g, {"B": 1000.0}, prior_draws=0)
        self.assertTrue(f.converged)
        self.assertAlmostEqual(f.ratings["A"], 1190.85, places=2)
        self.assertEqual(f.ratings["B"], 1000.0)
        self.assertEqual(f.fisher_se["B"], 0.0)
        # Fisher SE for one free player: 1/sqrt(C^2 N p (1-p))
        p = 0.75
        self.assertAlmostEqual(f.fisher_se["A"], 1 / math.sqrt(elo.C ** 2 * 100 * p * (1 - p)), places=6)

    def test_symmetry(self):
        g = games_from_counts("A", "B", 60, 30, 10)
        swapped = [elo.Game(x.black, x.white, 1.0 - x.score, x.unit) for x in g]
        f1 = elo.fit_ratings(g, {"B": 0.0}, prior_draws=0)
        f2 = elo.fit_ratings(swapped, {"B": 0.0}, prior_draws=0)
        self.assertAlmostEqual(f1.ratings["A"], f2.ratings["A"], places=6)
        # names swapped: B beats A 75% -> A is -190.85 below B
        g3 = games_from_counts("B", "A", 60, 30, 10)
        f3 = elo.fit_ratings(g3, {"B": 0.0}, prior_draws=0)
        self.assertAlmostEqual(f3.ratings["A"], -190.85, places=2)
        # no anchors: centred, symmetric
        f4 = elo.fit_ratings(g, None, prior_draws=0)
        self.assertAlmostEqual(f4.ratings["A"], 95.42, places=2)
        self.assertAlmostEqual(f4.ratings["B"], -95.42, places=2)
        self.assertAlmostEqual(f4.ratings["A"] + f4.ratings["B"], 0.0, places=9)
        # constrained covariance: both players share the SE of half the difference
        self.assertAlmostEqual(f4.fisher_se["A"], f4.fisher_se["B"], places=9)
        self.assertAlmostEqual(f4.fisher_se["A"], f1.fisher_se["A"] / 2, places=6)

    def test_perfect_score_prior(self):
        g = games_from_counts("A", "B", 10, 0, 0)
        f = elo.fit_ratings(g, {"B": 1500.0}, prior_draws=1.0)
        self.assertTrue(f.converged)
        # 10 wins + 1 virtual draw = 10.5 / 11
        self.assertAlmostEqual(f.ratings["A"], 1500 + elo.elo_from_score(10.5 / 11), places=3)
        self.assertAlmostEqual(f.ratings["A"], 2028.9, places=1)
        g0 = games_from_counts("A", "B", 0, 0, 10)
        f0 = elo.fit_ratings(g0, {"B": 1500.0}, prior_draws=1.0)
        self.assertAlmostEqual(f0.ratings["A"], 1500 - 528.9, places=1)
        # plain MLE diverges: must not converge or produce a finite "estimate" silently
        f_inf = elo.fit_ratings(g, {"B": 1500.0}, prior_draws=0.0, max_iter=30)
        self.assertFalse(f_inf.converged)

    def test_anchor_handling(self):
        true = {"S1": 1300.0, "S2": 1700.0, "X": 1500.0}
        g = []
        # exact expected fractions on large counts (no draws) -> exact recovery
        for opp in ("S1", "S2"):
            e = elo.expected_score(true["X"] - true[opp])
            w = round(e * 10000)
            g += games_from_counts("X", opp, w, 0, 10000 - w)
        f = elo.fit_ratings(g, {"S1": 1300, "S2": 1700, "UNUSED": 9999}, prior_draws=0)
        self.assertEqual(set(f.anchors), {"S1", "S2"})
        self.assertNotIn("UNUSED", f.ratings)
        self.assertAlmostEqual(f.ratings["X"], 1500.0, delta=0.5)
        self.assertEqual(f.ratings["S1"], 1300.0)
        self.assertEqual(f.ratings["S2"], 1700.0)

    def test_two_anchors_inconsistent(self):
        # X scores 50% vs both anchors 1000 and 2000 -> symmetric compromise at 1500
        g = games_from_counts("X", "S1", 0, 20, 0) + games_from_counts("X", "S2", 0, 20, 0)
        f = elo.fit_ratings(g, {"S1": 1000, "S2": 2000}, prior_draws=0)
        self.assertAlmostEqual(f.ratings["X"], 1500.0, places=4)

    def test_chain_exact(self):
        # A anchored; B and C only reachable through the chain A-B-C (+ A-C)
        true = {"A": 1500.0, "B": 1700.0, "C": 1900.0}
        g = []
        for a, b in (("B", "A"), ("C", "B"), ("C", "A")):
            e = elo.expected_score(true[a] - true[b])
            w = round(e * 20000)
            g += games_from_counts(a, b, w, 0, 20000 - w)
        f = elo.fit_ratings(g, {"A": 1500.0}, prior_draws=0)
        self.assertAlmostEqual(f.ratings["B"], 1700.0, delta=0.5)
        self.assertAlmostEqual(f.ratings["C"], 1900.0, delta=0.5)

    def test_chain_simulated(self):
        true = {"A": 1500.0, "B": 1650.0, "C": 1850.0, "D": 1400.0}
        sched = [("A", "B", 600), ("B", "C", 600), ("A", "C", 300), ("D", "A", 600)]
        g = simulate(true, sched, seed=7)
        res = elo.analyze(g, {"A": 1500.0}, n_boot=100, seed=3)   # default (tiny) prior
        for p in ("B", "C", "D"):
            r = res["players"][p]
            self.assertAlmostEqual(r["elo"], true[p], delta=45, msg=p)
            self.assertLessEqual(r["boot_lo"], r["elo"])
            self.assertGreaterEqual(r["boot_hi"], r["elo"])
            # the truth should be inside a generous interval
            self.assertLess(r["boot_lo"] - 30, true[p])
            self.assertGreater(r["boot_hi"] + 30, true[p])
            # bootstrap and Fisher widths are of the same order (draws make Fisher wider)
            bw = r["boot_hi"] - r["boot_lo"]
            fw = r["fisher_hi"] - r["fisher_lo"]
            self.assertTrue(0.4 * fw < bw < 1.6 * fw, (p, bw, fw))
        self.assertTrue(res["players"]["A"]["anchored"])
        self.assertIsNone(res["players"]["A"]["boot_lo"])

    def test_unconnected(self):
        g = games_from_counts("A", "S", 5, 5, 5) + games_from_counts("X", "Y", 5, 0, 5)
        f = elo.fit_ratings(g, {"S": 1000.0})
        self.assertEqual(sorted(f.unconnected), ["X", "Y"])
        self.assertNotIn("X", f.ratings)
        res = elo.analyze(g, {"S": 1000.0}, n_boot=10)
        self.assertIsNone(res["players"]["X"]["elo"])
        self.assertTrue(res["players"]["X"]["unconnected"])

    def test_more_draw_prior_is_shrinkage(self):
        g = games_from_counts("A", "B", 8, 0, 2)
        f0 = elo.fit_ratings(g, {"B": 0}, prior_draws=0)
        f1 = elo.fit_ratings(g, {"B": 0}, prior_draws=1)
        f5 = elo.fit_ratings(g, {"B": 0}, prior_draws=5)
        self.assertGreater(f0.ratings["A"], f1.ratings["A"])
        self.assertGreater(f1.ratings["A"], f5.ratings["A"])
        self.assertGreater(f5.ratings["A"], 0)


class TestUnbounded(unittest.TestCase):
    def test_detection(self):
        agg = elo.aggregate(games_from_counts("A", "S", 10, 0, 0))
        self.assertEqual(elo.unbounded_players(agg, {"S": 0}), {"A": "high"})
        agg = elo.aggregate(games_from_counts("A", "S", 0, 0, 10))
        self.assertEqual(elo.unbounded_players(agg, {"S": 0}), {"A": "low"})
        # chain: L1 loses everything to L2, L2 is fine vs the anchor -> only L1 unbounded
        g = games_from_counts("L2", "S", 5, 5, 5) + games_from_counts("L1", "L2", 0, 0, 6)
        self.assertEqual(elo.unbounded_players(g, {"S": 0}), {"L1": "low"})
        # a group: X and Y draw each other but lose every game to the rest -> both unbounded
        g = (games_from_counts("X", "Y", 0, 4, 0) + games_from_counts("X", "S", 0, 0, 4)
             + games_from_counts("Y", "S", 0, 0, 4))
        self.assertEqual(elo.unbounded_players(g, {"S": 0}), {"X": "low", "Y": "low"})
        # no anchors: needs a strongly connected scoring graph
        g = games_from_counts("A", "B", 3, 0, 0)
        self.assertEqual(elo.unbounded_players(g), {"A": "high", "B": "low"})
        g = games_from_counts("A", "B", 3, 1, 0)
        self.assertEqual(elo.unbounded_players(g), {})

    def test_bound(self):
        agg = elo.aggregate(games_from_counts("A", "S", 0, 0, 10))
        kind, r = elo.perfect_score_bound(agg, "A", {"S": 1000.0})
        self.assertEqual(kind, "upper")
        p = 1 - 0.05 ** (1 / 10)   # P(10 losses) = 5 %
        self.assertAlmostEqual(r, 1000 + elo.elo_from_score(p), places=3)
        self.assertAlmostEqual(r, 817.27, delta=0.01)
        agg = elo.aggregate(games_from_counts("A", "S", 10, 0, 0))
        kind, r = elo.perfect_score_bound(agg, "A", {"S": 1000.0})
        self.assertEqual(kind, "lower")
        self.assertAlmostEqual(r, 1182.73, delta=0.01)
        self.assertIsNone(elo.perfect_score_bound(elo.aggregate(games_from_counts("A", "S", 1, 0, 9)), "A", {"S": 0}))

    def test_analyze_reports_bound_not_number(self):
        g = games_from_counts("L2", "S", 10, 10, 10) + games_from_counts("L1", "L2", 0, 0, 20)
        res = elo.analyze(g, {"S": 1000.0}, n_boot=20)
        r1, r2 = res["players"]["L1"], res["players"]["L2"]
        self.assertIsNone(r1["elo"])
        self.assertIsNone(r1["boot_lo"])
        self.assertIsNone(r1["fisher_se"])
        self.assertEqual(r1["unbounded"], "low")
        self.assertEqual(r1["bound_type"], "upper")
        self.assertLess(r1["bound"], r2["elo"])
        self.assertIsNotNone(r1["elo_penalized"])
        self.assertAlmostEqual(r2["elo"], 1000.0, delta=1.0)   # unaffected by L1
        self.assertEqual(elo.elo_cell(r1), "< " + format(r1["bound"], ".0f"))
        pair = [p for p in res["pairs"] if p["a"] == "L1"][0]
        self.assertIsNone(pair["model_diff"])


class TestBootstrap(unittest.TestCase):
    def test_units_and_determinism(self):
        g = games_from_counts("A", "B", 6, 2, 2)
        units = elo.bootstrap_units(g)
        self.assertEqual(len(units[("A", "B")]), 5)  # 10 games = 5 opening pairs
        self.assertTrue(all(n == 2 for n, _ in units[("A", "B")]))
        s1 = elo.bootstrap(g, {"B": 0}, n=20, seed=5)
        s2 = elo.bootstrap(g, {"B": 0}, n=20, seed=5)
        self.assertEqual(s1["A"], s2["A"])
        self.assertEqual(len(s1["A"]), 20)
        self.assertTrue(all(v == 0 for v in s1["B"]))

    def test_too_few_units(self):
        g = games_from_counts("A", "S", 2, 1, 1)                 # 2 opening pairs
        r = elo.analyze(g, {"S": 0.0}, n_boot=50)["players"]["A"]
        self.assertEqual(r["boot_units"], 2)
        self.assertIsNone(r["boot_lo"])
        self.assertIn("too few opening pairs", r["boot_note"])
        self.assertIsNotNone(r["fisher_lo"])                      # Fisher is still available
        g = games_from_counts("A", "S", 6, 2, 4)                 # 6 opening pairs
        r = elo.analyze(g, {"S": 0.0}, n_boot=50)["players"]["A"]
        self.assertIsNotNone(r["boot_lo"])
        # 6 opening pairs, every one 1-1: all resamples identical -> no bootstrap interval
        g = []
        for i in range(6):
            g += [elo.Game("A", "S", 1.0, ("t", 0, i)), elo.Game("S", "A", 1.0, ("t", 0, i))]
        r = elo.analyze(g, {"S": 0.0}, n_boot=50)["players"]["A"]
        self.assertIsNone(r["boot_lo"])
        self.assertIn("degenerate", r["boot_note"])

    def test_unbounded_resamples(self):
        # 6 opening pairs vs the anchor, only one of them not lost: a third of the resamples
        # miss it -> no finite MLE there -> the lower CI end is -inf, not a prior artefact
        g = []
        for i in range(6):
            s = 1.0 if i == 0 else 0.0
            g += [elo.Game("A", "S", s, ("t", 0, i)), elo.Game("S", "A", 0.0 if i == 0 else 1.0, ("t", 0, i))]
        r = elo.analyze(g, {"S": 0.0}, n_boot=200)["players"]["A"]
        self.assertIsNotNone(r["elo"])
        self.assertEqual(r["boot_lo"], -math.inf)
        self.assertTrue(r["boot_lo_unbounded"])
        self.assertTrue(math.isfinite(r["boot_hi"]))
        self.assertGreater(r["boot_resamples_without_finite_mle"], 20)
        self.assertEqual(elo.percentile([-math.inf, -math.inf, 1.0, 2.0], 2.5), -math.inf)
        self.assertEqual(elo.percentile([1.0, 2.0, math.inf], 97.5), math.inf)

    def test_prob_not_above(self):
        s = {"L1": [1.0, 2.0, 3.0, 4.0], "L2": [2.0, 1.0, 5.0, 4.0]}
        self.assertAlmostEqual(elo.prob_not_above(s, "L1", "L2"), 0.5)
        self.assertIsNone(elo.prob_not_above(s, "L1", "L3"))


class TestJsonl(unittest.TestCase):
    def test_load_arena_records(self):
        recs = [
            {"id": 1, "white": "a", "black": "b", "pairing": 0, "game_pair": 0, "result": "1-0", "tc": "10+0.1",
             "termination": "checkmate"},
            {"id": 2, "white": "b", "black": "a", "pairing": 0, "game_pair": 0, "result": "1/2-1/2", "tc": "10+0.1",
             "termination": "threefold"},
            {"id": 3, "white": "a", "black": "c", "pairing": 1, "game_pair": 0, "result": "0-1", "tc": "10+0.1",
             "termination": "time"},
        ]
        with tempfile.TemporaryDirectory() as d:
            p = os.path.join(d, "g.jsonl")
            with open(p, "w") as fh:
                for r in recs:
                    fh.write(json.dumps(r) + "\n")
                fh.write('{"id": 4, "white": "a", "bla')  # truncated line
            warnings = []
            games = elo.load_jsonl(p, warn=warnings.append)
            self.assertEqual(len(games), 3)
            self.assertEqual(len(warnings), 1)
            self.assertEqual(games[0].unit, games[1].unit)
            self.assertNotEqual(games[0].unit, games[2].unit)
            self.assertEqual(elo.wdl_by_pair(games)[("a", "b")], [1, 1, 0])
            self.assertEqual(elo.wdl_by_player(games)["a"], [1, 1, 1])
            self.assertEqual(elo.expand_paths([d]), [p])
            out = os.path.join(d, "r.json")
            buf = io.StringIO()
            with contextlib.redirect_stdout(buf):
                rc = elo.main([d, "--anchor", "b=1000", "--bootstrap", "5", "--json", out, "--pairs"])
            self.assertEqual(rc, 0)
            self.assertIn("anchor", buf.getvalue())
            with open(out) as fh:
                res = json.load(fh)
            self.assertEqual(res["players"]["b"]["elo"], 1000.0)
            self.assertIn("c", res["players"])


if __name__ == "__main__":
    unittest.main()
