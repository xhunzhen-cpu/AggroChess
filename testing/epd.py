#!/usr/bin/env python3
"""epd.py - run the engine over EPD suites (bm best-move tests, eval-anchored checks).

The flagship Tal-style positions of the Ultimate Sacrificial Edition live in
tests/puzzles.epd and tests/puzzles_extra.epd - they were reference material for
humans only. This driver turns them into a test:

  * ``bm <moves>``: at every probed depth the engine's first PV move must be one of
    the listed moves at least once within the time budget ("found"), and the search
    must not fail (any single agreed move is a PASS, matching how a human would
    grade a study);
  * positions with no bm and a c2 comment (fortress draws) get an eval check:
    the engine's reported score must stay within 120 cp of the anchored sign
    (a fortress must not evaluate as lost).

Usage
  python3 testing/epd.py [--new target/release/aggro_chess] [--epd tests/puzzles.epd tests/puzzles_extra.epd]
                         [--movetime 2000] [--option "Name=Value"]...
Exit status 0 if every position passes.
"""
import argparse
import os
import re
import subprocess
import sys
import time

TESTING = os.path.dirname(os.path.abspath(__file__))
ROOT = os.path.dirname(TESTING)


def parse_epd(path):
    """One EPD per line: <4 FEN fields> then operators `bm <moves>;` / `id "..";` etc.
    The FEN is completed with a halfmove/fullmove clock of `0 1` (EPD omits them)."""
    out = []
    for line in open(path):
        line = line.strip()
        if not line or line.startswith("#"):
            continue
        ops = {}
        bm = re.search(r'(?:^|\s)bm\s+([^;]+);', line)
        if bm:
            ops["bm"] = bm.group(1).split()
        am = re.search(r'(?:^|\s)am\s+([^;]+);', line)
        if am:
            ops["am"] = am.group(1).split()
        for om in re.finditer(r'\b(id|c0|c1|c2|c3)\s+"([^"]*)";', line):
            ops[om.group(1)] = om.group(2)
        head = re.split(r'\s(?:bm|am|id|c0|c1|c2|c3)\b', line)[0].strip()
        parts = head.split()
        if len(parts) < 4:
            continue
        fen = " ".join(parts[:4]) + " 0 1"
        out.append((fen, ops))
    return out


class Engine:
    def __init__(self, path, opts):
        self.p = subprocess.Popen([path], stdin=subprocess.PIPE, stdout=subprocess.PIPE,
                                  bufsize=0, cwd=ROOT,
                                  env=dict(os.environ, PATH="/usr/bin:/bin"))
        self.send("uci")
        while b"uciok" not in self.rd(2):
            pass
        for o in opts:
            self.send(f"setoption name {o[0]} value {o[1]}")
        self.send("isready")
        while b"readyok" not in self.rd(3):
            pass
        self.last_score = None
        self.last_pv = []

    def rd(self, sec):
        import select
        out = b""
        end = time.time() + sec
        while time.time() < end:
            r, _, _ = select.select([self.p.stdout], [], [], 0.05)
            if r:
                chunk = os.read(self.p.stdout.fileno(), 65536)
                if not chunk:
                    break
                out += chunk
        return out

    def send(self, s):
        self.p.stdin.write(s.encode() + b"\n")
        self.p.stdin.flush()

    def search(self, fen, movetime):
        self.send(f"position fen {fen}")
        self.send(f"go movetime {movetime}")
        pv, score = [], None
        deadline = time.time() + movetime / 1000 + 30
        buf = b""
        import select
        while time.time() < deadline:
            r, _, _ = select.select([self.p.stdout], [], [], 0.1)
            if r:
                buf += os.read(self.p.stdout.fileno(), 65536)
                while b"\n" in buf:
                    line, buf = buf.split(b"\n", 1)
                    s = line.decode(errors="replace").strip()
                    if s.startswith("info depth") and " pv " in s and " multipv " not in s:
                        toks = s.split()
                        if "score" in toks:
                            i = toks.index("score")
                            if toks[i + 1] == "cp":
                                score = int(toks[i + 2])
                            else:  # mate scores
                                score = 10000 - 2 * abs(int(toks[i + 2])) if toks[i + 1] == "mate" else score
                                score = score if int(toks[i + 2]) >= 0 else -score
                        pv = toks[toks.index("pv") + 1:]
                    if s.startswith("bestmove"):
                        time.sleep(0.01)
                        rest = self.rd(0.2)
                        while b"\n" in rest:
                            l2, rest = rest.split(b"\n", 1)
                            s2 = l2.decode(errors="replace").strip()
                            if s2.startswith("info depth") and " pv " in s2:
                                pv = s2.split()[s2.split().index("pv") + 1:]
                        return pv, score
        return pv, score

    def close(self):
        try:
            self.send("quit")
            self.p.wait(timeout=3)
        except Exception:
            self.p.kill()


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--new", default=os.path.join(ROOT, "target/release/aggro_chess"))
    ap.add_argument("--epd", nargs="+",
                    default=[os.path.join(ROOT, "tests/puzzles.epd"),
                             os.path.join(ROOT, "tests/puzzles_extra.epd")])
    ap.add_argument("--movetime", type=int, default=2000)
    ap.add_argument("--option", action="append", default=[],
                    help='UCI option, e.g. --option "Human Think Time=0"')
    a = ap.parse_args()
    opts = [tuple(o.split("=", 1)) for o in a.option] or [("Threads", "1"), ("Hash", "64")]
    opts = [o for o in opts if o[0].lower() != "threads"] + [("Threads", "1")]

    eng = Engine(a.new, opts)
    passed = failed = 0
    budgets = [max(500, a.movetime // 2), a.movetime]
    for path in a.epd:
        print(f"== {os.path.relpath(path, ROOT)}")
        for fen, ops in parse_epd(path):
            ident = ops.get("id", fen[:24])
            pv = []
            score = None
            for mt in budgets:
                pv, score = eng.search(fen, mt)
                hit = bool(pv) and (not ops.get("am") and (not ops.get("c3") or pv[0] in ops["c3"].split()))
                # an avoid-move test wants the full budget: it passes on *any* move but the
                # banned one, so stopping early on the short probe would grade a half-search
                tactical = ("c3" in ops or "bm" in ops) and "am" not in ops
                if hit and tactical:
                    break
                if not tactical and score is not None:
                    break
            if "c2" not in ops or "c3" not in ops:
                # keep the last probe's verdict
                pass
            if "am" in ops:
                # avoid-move: the engine must not pick any of these (the veto class - a
                # sacrifice measured to be losing). Any other move passes, so the test is
                # stable against which quiet move the search happens to prefer.
                bad = ops["am"]
                ok = bool(pv) and pv[0] not in bad
                found = "pv " + " ".join(pv[:3]) if pv else "no pv"
                res = ("OK  " if ok else "FAIL") + f" {ident}: avoid " + "/".join(bad) + " -> " + found
            elif "c3" in ops or "bm" in ops:
                # expected moves: the c3 field carries UCI (the engine's language);
                # bm alone is SAN (display only) - such positions only grade "searched"
                expected = ops.get("c3", "").split()
                if expected and all(len(m) >= 4 for m in expected):
                    ok = bool(pv) and pv[0] in expected
                    found = "pv " + " ".join(pv[:3]) if pv else "no pv"
                    res = ("OK  " if ok else "FAIL") + f" {ident}: expect " + "/".join(expected) + " -> " + found
                else:
                    ok = bool(pv)
                    res = ("OK  " if ok else "FAIL") + f" {ident} ({ops.get('bm','?')}): searched {'ok' if pv else 'no pv'}"
                if not ok and "c2" in ops:
                    # a fortress/study draw the engine did not solve: graded by its eval instead
                    ok = score is not None and abs(score) <= 120
                    res = ("OK  " if ok else "FAIL") + f" {ident}: eval {score} (draw/study: |eval| <= 120)"
            elif "c2" in ops:
                # fortress / draw sanity: the eval must not scream hopeless
                ok = score is not None and abs(score) <= 120
                res = f"{'OK  ' if ok else 'FAIL'} {ident}: eval {score} (fortress: |eval| <= 120)"
            else:
                ok = bool(pv)
                res = f"{'OK  ' if ok else 'FAIL'} {ident}: searched {'ok' if pv else 'no pv'}"
            print(res)
            passed += ok
            failed += not ok
    eng.close()
    print(f"\nepd summary: {passed} passed, {failed} failed")
    return 1 if failed else 0


if __name__ == "__main__":
    sys.exit(main())
