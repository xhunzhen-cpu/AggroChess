#!/usr/bin/env python3
"""Self-tests for calibrate.py's scheduling / bookkeeping (no games are played):
python3 -m unittest discover -s testing -p 'test_*.py'   (from AggroChess/)"""
import json
import os
import sys
import tempfile
import unittest

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import calibrate as cal  # noqa: E402
import elo  # noqa: E402


def settings(**kw):
    st = {**cal.DEFAULTS, "engine": "/x/target/release/aggro_chess", "sf": "/usr/games/stockfish",
          "openings": "/nonexistent/openings.txt", "arena": "/nonexistent/aggro_arena", "quick": False}
    st.update(kw)
    return cal.normalise(st)


class TestParsing(unittest.TestCase):
    def test_lists(self):
        self.assertEqual(cal.parse_int_list("1-3,7,5-4"), [1, 2, 3, 4, 5, 7])
        self.assertEqual(cal.parse_int_list("21"), [21])
        self.assertEqual(cal.parse_int_list(" 1, 11 ,21"), [1, 11, 21])

    def test_prior(self):
        pts = cal.parse_prior("1:400,21:2100")
        self.assertAlmostEqual(cal.prior_elo(pts, 1), 400)
        self.assertAlmostEqual(cal.prior_elo(pts, 21), 2100)
        self.assertAlmostEqual(cal.prior_elo(pts, 11), 1250)
        self.assertAlmostEqual(cal.prior_elo(pts, 0), 315)          # linear extrapolation
        pts = cal.parse_prior("1:400,11:1000,21:2200")
        self.assertAlmostEqual(cal.prior_elo(pts, 16), 1600)

    def test_tc(self):
        self.assertEqual(cal.parse_tc("10+0.1"), "10+0.1")
        with self.assertRaises(ValueError):
            cal.parse_tc("10/0.1")

    def test_names_differ(self):
        st = settings()
        self.assertEqual(cal.level_name(st, 5), "aggro_L05")
        self.assertEqual(cal.anchor_name(1320), "sf1320")
        self.assertEqual(cal.engine_of(st, "aggro_L05"), ("level", 5))
        self.assertEqual(cal.engine_of(st, "sf1320"), ("anchor", 1320))
        st2 = settings(engine="/usr/games/stockfish", levels="0-20")
        self.assertEqual(cal.level_name(st2, 0), "stockfish_L00")
        self.assertNotEqual(cal.level_name(st2, 13)[:2], "sf")

    def test_quick_defaults(self):
        st = cal.normalise({**cal.DEFAULTS, **cal.QUICK, "quick": True})
        self.assertEqual(st["levels"], [1, 11, 21])
        self.assertEqual(st["tc"], "2+0.05")
        self.assertEqual(st["concurrency"], 5)
        self.assertEqual(cal.normalise({**cal.DEFAULTS})["concurrency"], 14)
        self.assertEqual(cal.normalise({**cal.DEFAULTS})["anchors"], [1320, 1500, 1700, 1900, 2100, 2300, 2500])


class TestScheduling(unittest.TestCase):
    def test_nearest_anchors(self):
        a = [1320, 1500, 1700, 1900, 2100, 2300, 2500]
        self.assertEqual(cal.nearest_anchors(400, a, 2), [1320, 1500])
        self.assertEqual(cal.nearest_anchors(2100, a, 2), [1900, 2100])   # tie -> lower
        self.assertEqual(cal.nearest_anchors(400, a, 2, window=350), [])
        self.assertEqual(cal.nearest_anchors(1000, a, 2, window=350), [1320])
        self.assertEqual(cal.nearest_anchors(3000, a, 1), [2500])

    def test_scout(self):
        st = settings(levels="1,11,21", scout_games=4)
        pairings, info = cal.plan_scout(st)
        self.assertIn(["aggro_L01", "sf1320", 4], pairings)
        self.assertIn(["aggro_L01", "aggro_L11", 4], pairings)
        self.assertIn(["aggro_L11", "aggro_L21", 4], pairings)
        self.assertEqual(info["21"]["anchors"], [1900, 2100])
        self.assertEqual(len(pairings), 3 * 2 + 2)

    def test_main_plan(self):
        st = settings(games=200)
        est = {lv: cal.prior_elo(st["prior"], lv) for lv in st["levels"]}
        pairings, info = cal.plan_main(st, est)
        # below the floor: no anchors, the chain carries the whole budget
        self.assertEqual(info["1"]["anchors"], [])
        self.assertGreaterEqual(info["1"]["planned_games"], 190)
        self.assertEqual(set(info["1"]["chain"]), {"2", "3"})
        # anchored levels: ~half vs anchors within the window
        self.assertEqual(info["15"]["anchors"], [1500, 1700])
        self.assertEqual(info["15"]["games_per_anchor"], 50)
        for lv in st["levels"]:
            self.assertGreaterEqual(info[str(lv)]["planned_games"], 190, lv)
            for a in info[str(lv)]["anchors"]:
                self.assertLessEqual(abs(a - est[lv]), 350)
        # every pair line has an even game count and distinct engines
        for a, b, n in pairings:
            self.assertEqual(n % 2, 0)
            self.assertNotEqual(a, b)
        # chain is symmetric in the bookkeeping
        self.assertEqual(info["5"]["chain"]["6"], info["6"]["chain"]["5"])

    def test_main_plan_group_without_anchor(self):
        st = settings(levels="1,6", games=200)
        pairings, info = cal.plan_main(st, {1: 180.0, 6: 820.0})
        self.assertTrue(info["6"]["forced_anchor"])
        self.assertEqual(info["6"]["anchors"], [1320])
        self.assertIn(["aggro_L06", "sf1320", 100], pairings)
        self.assertEqual(info["1"]["anchors"], [])

    def test_tc_check_plan(self):
        st = settings(tc_levels="1,6,11,16,21", tc_games=100)
        est = {1: 500.0, 6: 900.0, 11: 1400.0, 16: 1700.0, 21: 2150.0}
        pairings, info, skipped = cal.plan_tc_check(st, est)
        self.assertEqual(skipped, [])
        self.assertEqual(info["1"]["bridge"], 6)
        self.assertEqual(info["6"]["bridge"], 11)
        self.assertEqual(info["21"]["anchors"], [2100, 2300])
        self.assertIn(["aggro_L01", "aggro_L06", 100], pairings)
        _, _, skipped = cal.plan_tc_check(settings(tc_levels="1,7"), {1: 500.0})
        self.assertEqual(skipped, [7])

    def _anchor_connected(self, pairings):
        # every level must reach an anchor through the planned games
        adj = {}
        for a, b, _ in pairings:
            adj.setdefault(a, set()).add(b)
            adj.setdefault(b, set()).add(a)
        for start in adj:
            seen, todo = {start}, [start]
            while todo:
                x = todo.pop()
                for y in adj[x] - seen:
                    seen.add(y)
                    todo.append(y)
            self.assertTrue(any(n.startswith("sf") for n in seen), f"{start} not connected: {pairings}")

    def test_tc_check_plan_always_anchored(self):
        # all checked levels below the window: they used to be bridged only to each other
        st = settings(tc_levels="1,6", tc_games=100)
        pairings, info, _ = cal.plan_tc_check(st, {1: 500.0, 6: 900.0})
        self._anchor_connected(pairings)
        self.assertEqual(info["6"]["anchors"], [1320])
        self.assertTrue(info["6"]["forced_anchor"])
        self.assertIn(["aggro_L06", "sf1320", 100], pairings)
        # two strongest levels above the window (2500 + 350)
        st = settings(tc_levels="11,16,21", tc_games=100)
        pairings, info, _ = cal.plan_tc_check(st, {11: 2000.0, 16: 2900.0, 21: 3100.0})
        self._anchor_connected(pairings)
        self.assertEqual(info["16"]["anchors"], [2500])
        # a lone level far from every anchor
        pairings, info, _ = cal.plan_tc_check(settings(tc_levels="1", tc_games=100), {1: 300.0})
        self.assertEqual(pairings, [["aggro_L01", "sf1320", 100]])
        # bridging to an anchored level is preferred when that gap is smaller
        st = settings(tc_levels="1,6,11", tc_games=100, anchors="1320,1900")
        pairings, info, _ = cal.plan_tc_check(st, {1: 400.0, 6: 950.0, 11: 1350.0})
        self._anchor_connected(pairings)
        self.assertFalse(info["6"]["forced_anchor"])

    def test_stage4_without_anchor_reports_no_elo(self):
        # games only between two levels: elo.analyze centres them on 0 - that is no rating
        import test_elo
        st = settings(levels="1,6", tc_levels="1,6", min_games=10)
        g = test_elo.games_from_counts("aggro_L01", "aggro_L06", 5, 2, 13)
        with tempfile.TemporaryDirectory() as d:
            os.makedirs(os.path.join(d, "stage4_tc"))
            with open(os.path.join(d, "stage4_tc", "b01.jsonl"), "w") as fh:
                for i, x in enumerate(g):
                    fh.write(json.dumps({"id": i + 1, "white": x.white, "black": x.black, "pairing": 0, "game_pair": i // 2,
                                         "result": {1.0: "1-0", 0.5: "1/2-1/2", 0.0: "0-1"}[x.score]}) + "\n")
            plan = {"stage": "stage4_tc", "tc": "60+0.6", "batches": [{"name": "b01", "games": len(g)}],
                    "per_level": {"1": {"anchors": [], "bridge": 6}, "6": {"anchors": [], "bridge": 1}}}
            rows = [{"level": 1, "elo": 500.0}, {"level": 6, "elo": 900.0}]
            out = cal.fit_stage4(st, d, plan, rows)
        for r in out["levels"]:
            self.assertIsNone(r["check_elo"])
            self.assertIsNone(r["drift"])
            self.assertIn("not connected to an anchor at the check TC", r["labels"])
            self.assertEqual(r["games"], 20)

    def test_scout_estimates_use_tiny_prior(self):
        import test_elo
        st = settings(levels="1,2,3", prior="1:300,3:1100")
        g = []
        g += test_elo.games_from_counts("aggro_L03", "sf1320", 1, 0, 9)   # ~ 1320 - 382
        g += test_elo.games_from_counts("aggro_L02", "sf1320", 0, 0, 10)  # 0%: bound
        g += test_elo.games_from_counts("aggro_L02", "aggro_L03", 2, 0, 8)
        g += test_elo.games_from_counts("aggro_L01", "aggro_L02", 0, 0, 10)
        est, src, _ = cal.scout_estimate_map(st, g)
        self.assertAlmostEqual(est["3"], 1320 - 382, delta=40)        # was ~1320 - 290 with 1 virtual draw
        self.assertLess(est["2"], est["3"])
        self.assertEqual(src["1"], "scout 0%: min(prior, upper bound)")
        self.assertLessEqual(est["1"], 300.0)

    def test_setup_validates_options_and_cleans_up(self):
        with tempfile.TemporaryDirectory() as d:
            eng = os.path.join(d, "fake_engine")
            with open(eng, "w") as fh:
                fh.write("#!/usr/bin/env python3\nimport sys\nfor l in sys.stdin:\n"
                         "    l = l.strip()\n"
                         "    if l == 'uci':\n"
                         "        print('id name fake'); print('option name Skill Level type spin default 21 min 1 max 21');"
                         " print('option name Aggression type spin default 50 min 0 max 100'); print('uciok', flush=True)\n"
                         "    elif l == 'isready': print('readyok', flush=True)\n"
                         "    elif l == 'quit': break\n")
            os.chmod(eng, 0o755)
            arena = os.path.join(d, "arena")
            with open(arena, "w") as fh:
                fh.write("#!/bin/sh\n")
            run = os.path.join(d, "run")
            st = settings(engine=eng, engine_files="", arena=arena, engine_option=["Agression=50"])
            with self.assertRaises(cal.CalibrationError) as cm:
                cal.setup_run(st, run)
            self.assertIn("Agression", str(cm.exception))
            self.assertFalse(os.path.exists(run))                       # nothing half-made left behind
            os.makedirs(run)
            with self.assertRaises(cal.CalibrationError):
                cal.setup_run(settings(engine=eng, engine_files="", arena=arena, level_option="Skil Level"), run)
            self.assertEqual(os.listdir(run), [])                       # emptied, retry possible
            meta = cal.setup_run(settings(engine=eng, engine_files="", arena=arena, engine_option=["aggression=60"]), run)
            self.assertTrue(os.path.exists(os.path.join(run, "settings.json")))
            self.assertEqual(meta["settings"]["engine_option"], ["aggression=60"])
            # --resume on a directory without settings.json is a clean error
            bad = os.path.join(d, "bad")
            os.makedirs(bad)
            args = cal.parse_args(["--resume", "--run-dir", bad])
            with self.assertRaises(cal.CalibrationError):
                cal.resolve(args)

    def test_batches(self):
        b = cal.make_batches([["a", "b", 100], ["a", "c", 30], ["b", "c", 500]], 120)
        self.assertEqual([sum(p[2] for p in x) for x in b], [100, 30, 120, 120, 120, 120, 20])
        flat = [p for x in b for p in x]
        self.assertEqual(sum(p[2] for p in flat), 630)
        self.assertTrue(all(sum(p[2] for p in x) <= 120 for x in b))
        self.assertTrue(all(p[2] % 2 == 0 for p in flat))


class TestBookkeeping(unittest.TestCase):
    def test_config_and_status(self):
        st = settings(levels="1,2")
        batch = {"name": "b01", "pairs": [["aggro_L01", "sf1320", 4], ["aggro_L01", "aggro_L02", 2]], "games": 6,
                 "openings_start": 0, "openings_count": 3, "seed": 1}
        text = cal.config_text(st, "stage1_scout", "2+0.05", batch, 5)
        self.assertIn("engine aggro_L01 ../engine/aggro_chess", text)
        self.assertIn("option aggro_L01 Skill Level = 1", text)
        self.assertIn("option sf1320 UCI_Elo = 1320", text)
        self.assertIn("option sf1320 UCI_LimitStrength = true", text)
        self.assertIn("pair aggro_L01 aggro_L02 2", text)
        self.assertEqual(text.count("engine aggro_L01 "), 1)
        # concurrency does not matter for --resume
        self.assertEqual(cal._norm_cfg(text), cal._norm_cfg(cal.config_text(st, "stage1_scout", "2+0.05", batch, 14)))
        with tempfile.TemporaryDirectory() as d:
            self.assertEqual(cal.batch_status(d, batch), (False, 0))
            recs = [("aggro_L01", "sf1320", 0), ("sf1320", "aggro_L01", 0), ("aggro_L01", "sf1320", 0),
                    ("sf1320", "aggro_L01", 0), ("aggro_L01", "aggro_L02", 1)]
            with open(os.path.join(d, "b01.jsonl"), "w") as fh:
                for i, (w, b, p) in enumerate(recs):
                    fh.write(json.dumps({"id": i + 1, "white": w, "black": b, "pairing": p, "game_pair": i // 2,
                                         "result": "1-0"}) + "\n")
            self.assertEqual(cal.batch_status(d, batch), (False, 5))
            with open(os.path.join(d, "b01.jsonl"), "a") as fh:
                fh.write(json.dumps({"id": 6, "white": "aggro_L02", "black": "aggro_L01", "pairing": 1,
                                     "game_pair": 0, "result": "0-1"}) + "\n")
            self.assertEqual(cal.batch_status(d, batch), (True, 6))

    def test_engine_option_override(self):
        st = settings(engine_option=["Hash=64", "Aggression=50", "Skill Level=3"])
        lines = cal.engine_block(st, "aggro_L07", level=7)
        self.assertIn("option aggro_L07 Hash = 64", lines)
        self.assertNotIn("option aggro_L07 Hash = 16", lines)
        self.assertIn("option aggro_L07 Aggression = 50", lines)
        self.assertIn("option aggro_L07 Skill Level = 7", lines)
        self.assertNotIn("option aggro_L07 Skill Level = 3", lines)

    def test_labels(self):
        st = settings(levels="1,2,3", min_games=10)
        g = []
        # L3 ~ at sf1320, L2 well below via the chain, L1 swept by L2
        import test_elo
        g += test_elo.games_from_counts("aggro_L03", "sf1320", 10, 10, 10)
        g += test_elo.games_from_counts("aggro_L02", "aggro_L03", 2, 2, 26)
        g += test_elo.games_from_counts("aggro_L01", "aggro_L02", 0, 0, 4)
        res = elo.analyze(g, cal.anchors_map(st), 1.0, 50, 1, keep_samples=True)
        samples = res.pop("_samples")
        rows = cal.level_table(st, res, g, {})
        by = {r["level"]: r for r in rows}
        self.assertEqual(by[3]["labels"], ["anchored"])
        self.assertIn("extrapolated", by[2]["labels"])
        self.assertIn("low-sample", by[1]["labels"])
        self.assertIn("bound only (0% score)", by[1]["labels"])
        self.assertIsNone(by[1]["elo"])                       # no number that was not estimable
        self.assertEqual(by[1]["bound_type"], "upper")
        # 0/4 vs L2: P(4 losses) = 5 % at expected score 1 - 0.05**0.25 = 0.527 -> bound = L2 + 19
        self.assertAlmostEqual(by[1]["bound"], by[2]["elo"] + elo.elo_from_score(1 - 0.05 ** 0.25), places=3)
        self.assertEqual(cal.elo_str(None, "upper", 791.4), "< 791")
        self.assertEqual(cal.elo_str(None, None, None), "n/a")
        self.assertEqual(by[3]["anchor_games"], 30)
        mono = cal.monotonicity(st, rows, samples)
        self.assertEqual([m["status"] for m in mono], ["ok"])  # only rated levels (2, 3) are compared

    def test_monotonicity_violation(self):
        st = settings(levels="1,2,3")
        rows = [{"level": 1, "name": "aggro_L01", "elo": 900.0, "ci_lo": 850.0, "ci_hi": 950.0},
                {"level": 2, "name": "aggro_L02", "elo": 880.0, "ci_lo": 820.0, "ci_hi": 940.0},
                {"level": 3, "name": "aggro_L03", "elo": 700.0, "ci_lo": 650.0, "ci_hi": 750.0}]
        samples = {"aggro_L01": [900, 910, 890, 905], "aggro_L02": [880, 920, 870, 860],
                   "aggro_L03": [700, 690, 710, 705]}
        m = cal.monotonicity(st, rows, samples)
        self.assertEqual(m[0]["status"], "inversion within CI overlap")
        self.assertAlmostEqual(m[0]["p_inversion"], 0.75)
        self.assertEqual(m[1]["status"], "VIOLATION (95% CIs do not overlap)")
        self.assertEqual(m[1]["p_inversion"], 1.0)


if __name__ == "__main__":
    unittest.main()
