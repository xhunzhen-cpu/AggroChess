"""Aggregate brilliant counts across every 120-game lever run with a comparable rig, and
compute the pooled per-game rate with a proper binomial CI. Event counts this small are
dominated by noise; this tells us which A/B differences were ever real.

Compares only runs whose cfg has the *identical* control rig, and reports:
  * brilliant/g for each run, with a 95% binomial interval on the event count
  * winner score of var vs ctl, from each game's result string
"""
import json, os, glob, math

VAL = {"P": 100, "N": 320, "B": 330, "R": 500, "Q": 900, "K": 0}


def real(e):
    san = e.get("san") or ""
    mv = san.split("...")[-1] if "." in san else san
    mv = mv.split(".", 1)[-1] if mv[:1].isdigit() else mv
    mv = mv.strip()
    if mv.startswith("K") or mv.startswith("O-O") or mv.startswith("0-0"):
        return False
    if e.get("category") not in ("queen", "exchange", "piece"):
        return False
    return sum(v for v in (e.get("net_lost") or {}).values() if v > 0) >= 1


def strict(e):
    if not real(e) or e.get("mating"):
        return False
    b, f = e.get("sf_before"), e.get("sf_after")
    if b is None or f is None or e.get("unsound"):
        return False
    return f - b >= -50 and b < 300 and f > -150


def wilson(k, n):
    if n == 0:
        return (0, 0)
    z = 1.96
    p = k / n
    d = 1 + z * z / n
    c = (p + z * z / (2 * n)) / d
    h = z * math.sqrt(p * (1 - p) / n + z * z / (4 * n * n)) / d
    return (max(0, c - h), c + h)


def main():
    base = os.path.join(os.path.dirname(os.path.abspath(__file__)), "runs", "lever")
    rows = []
    for d in sorted(glob.glob(os.path.join(base, "*"))):
        name = os.path.basename(d)
        ev = os.path.join(d, f"{name}_sac.events.jsonl")
        gj = os.path.join(d, f"{name}.jsonl")
        if not (os.path.exists(ev) and os.path.exists(gj)):
            continue
        evs = [json.loads(l) for l in open(ev)]
        sacs = [e for e in evs if e.get("type") == "sacrifice"]
        n = len({g["id"] for g in (json.loads(l) for l in open(gj))})
        if n == 0:
            continue
        br = [e for e in sacs if strict(e)]
        r = [e for e in sacs if real(e)]
        w = lo = dr = 0
        for g in (json.loads(l) for l in open(gj)):
            res = g.get("result") or ""
            if res == "1-0":
                winner = "white"
            elif res == "0-1":
                winner = "black"
            else:
                dr += 1
                continue
            if g.get(winner) == "var":
                w += 1
            else:
                lo += 1
        sc = (w + 0.5 * dr) / n
        ci = wilson(len(br), n)
        rows.append((name, n, len(r), len(br), ci, sc, w, dr, lo))

    print(f"{'run':16} {'games':>5} {'real/g':>7} {'bril/g':>7} {'95% CI on bril/g':>20} {'var score':>10}")
    for name, n, nr, nb, ci, sc, w, dr, lo in rows:
        print(f"{name:16} {n:>5} {nr/n:>7.3f} {nb/n:>7.3f}   [{ci[0]:.3f}, {ci[1]:.3f}]        "
              f"{sc:>5.1%} ({w}/{dr}/{lo})")
    tot_n = sum(r[1] for r in rows)
    tot_b = sum(r[3] for r in rows)
    ci = wilson(tot_b, tot_n)
    print(f"\nPOOLED: {tot_b} brilliant events over {tot_n} games = {tot_b/tot_n:.3f}/g "
          f"[{ci[0]:.3f}, {ci[1]:.3f}]")


main()
