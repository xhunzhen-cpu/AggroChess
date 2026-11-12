# "Brilliant in N" — outcome: not shipped, and why

Date: 2026-11-12. Branch `brilliancy` (5.4.0 shipped from `a2690d0`).

The user asked for the mate-in-N analogue for brilliancies: spot a brilliancy a few moves ahead
and play toward it like a mate. They chose the rule "forced, then no-regret bait", clock-scaled
depth, as deep as the clock allows. This is what happened.

## Verdict: no. It cannot be built to that design, and the reasons are measurements, not opinions.

Nothing was shipped. `src/search.rs` is byte-identical to 5.4.0. Bench 287284 unchanged,
56 tests pass, `testing/brilliance.py` clean. The working tree carries only this document and
the two measurement scripts; `src/horizon.rs` (the attempt) was deleted after it failed.

## The three measurements that decided it

**1. The predicate is nearly always true — measured.** `brilliant::candidates` nominates any move
that gives material away. Over all 34 root moves and all 1020 replies in the Greek-gift position,
**1018 replies "allow a brilliancy"**, 12.7 nominees per position; in Kiwipete 26.6 and 2036 of
2039 replies. In seven middlegames, **33 of 34 root moves** were nominees. A predicate that is
true almost everywhere cannot define "brilliant in N", cannot make a *forced* tier meaningful,
and would fire on every move. Making it rare requires a search, not a scan — which is the next
problem.

**2. There is no clock to do that search in — measured.** At `go movetime 300` the main search
uses 156 ms to depth 13 (96k nodes); the look's share is ~66 ms total. Split over 3 candidates
and 3 searches each, that is ~5–7 ms per search, ~4000 nodes. Enough to *confirm* a brilliancy,
not to *refute* one — and refutation is the horizon look's main job.

**3. When implemented as specified, it played `a2a3` and called it brilliant — measured.** The
implementation was written, compiled, and run. In `r4rk1/1pp1qppp/p1np1n2/2b1p1B1/2B1P3/2NP1N2/PPP1QPPP/R4RK1 w`
it reported:

```
info string brilliant in 2: a2a3 no-regret bait (39 allowing/1 declining, verified 2, worst 947 vs best 563)
bestmove a2a3
```

The 947 is nonsense. It came from searching a **single declining reply** on ~5 ms in the
**opponent's** frame and reading the result as ours. A pawn move that does nothing was played as
a brilliancy. That is the exact failure the review predicted, reproduced. It was removed rather
than patched, because the flaws are structural, not local.

## The adversarial review (10 lenses → 2 skeptics per finding): 41 findings, 8 blockers

Run as workflow `wf_adcb400c-9b2` over the design and the real code. The blockers, all
independently consistent with the measurements above:

1. **`group_look` cannot express a minimising "worst reply" search.** It returns a maximising
   root score. The design's central primitive does not exist and is not a small addition.
2. **The score frame is the opponent's.** `group_look` re-roots at its own `board.side_to_move()`,
   so every child value must be negated to compare with the root's. Getting this wrong plays
   nothing-moves as brilliancies (observed).
3. **Tier 1 ("declines is empty") is the *typical* case, not the forced one**, precisely because
   the predicate is nearly always true. It would break `testing/brilliance.py`'s controls.
4. **The 20 cp no-regret tolerance compares incomparable scales**: a neutral child score against
   the root's personality-inclusive score, which carries up to 340 cp of sacrifice bonus at ply 2.
5. **"Verified sound" is undefined at the horizon.** The root look's thresholds (margin 45–80 cp,
   depth ≥ 5) do not transfer to a 4000-node search two plies deeper; the code says so itself.
6. **The FORCED tier announces a proof it does not have.** There is no proof; it is a bounded look.
7. **No budget exists at node-budgeted levels 1–18.** "Clock-scaled N" is meaningless there.
8. **The horizon's sub-searches miss the two setup plies of repetition history**, so a repetition
   draw can be scored as a winning brilliancy.

Plus 33 major/minor findings on TT frame keys, root LMR hiding the opponent's best decliner, the
info-string grammar, `go infinite`/ponder gating, warm-TT contamination of a neutral search, and
more. Full list in the workflow journal
(`…/subagents/workflows/wf_adcb400c-9b2/journal.jsonl`).

## What the review says *is* worth having

Its most constructive finding: **the engine already has a bounded horizon-sacrifice look** —
`LOOK_PRIME` inside Tal's look (`search.rs:2957-2971`, `3674-3695`) — but it runs `full`-only and
only at long thinks. That is the existing precedent for "look one move ahead for a sacrifice",
and any future attempt should extend *that* rather than build a parallel horizon path.

## If this is ever revisited

The only shape that can work, given the measurements:

* A **rare** predicate: the brilliancy must beat the best quiet alternative by a large margin,
  decided by a search — accept that this is expensive and therefore rare in *both* senses.
* **One** group search per candidate, on the opponent's frame, negated once, never per reply.
* Budget at full strength only (or with the same separate-budget discipline as 5.4.0's weak
  look), and **never** claim "forced" — report the depth reached and nothing more.
* Measure the hit rate first. The immediate look already fires 2/16 (12%); a horizon look must
  be rarer, and someone should confirm there is anything to find before building the finder.

## Files

* `DESIGN_brilliant_in_n.md` — the proposal, with every measured finding folded in as it was found.
* `testing/look_frequency.py` — reproducible: how often the shipped look fires (12%).
* `testing/horizon.py` — the behaviour suite written for the attempt; it encodes the contracts a
  future implementation must meet (silent at Brilliance 0, announced when played, bounded overhead).
