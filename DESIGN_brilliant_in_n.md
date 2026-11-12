# Design: "brilliant in N" (the horizon look)

Status: **proposal under review, not implemented.** Repo `/root/aggroclone/AggroChess-brilliant`,
branch `brilliancy`. Every claim below marked MEASURED came from running the 5.4.0 binary or a
throwaway probe against the real crate on 2026-11-12; nothing is claimed from inspection alone.

## The gap it fills

The 5.4.0 brilliancy look is **root-only and immediate**: it judges the moves available *in this
position*. It cannot see a brilliancy one of our moves away. The only thing that points that way
today is a static nudge — `search.rs:1692`, `sac_bonus` full at the root, 60% at ply 2, 50% at
ply 4, plus `SacParams::chain` — whose own comment says it "steers towards positions where a
sacrifice will be available". It rewards being *near* a sacrifice; it never checks one will exist.

The engine already does the analogous thing properly for mate: `matesearch.rs` (DFPN, plus
`GoalSearch` with `set_goal`/`solve`, `prove_mate`, `find_mate`), fed into the search by
`MATE_PROBE`. This design is the brilliancy analogue, but **bounded rather than proved** — see
"Not a prover, and why".

## MEASURED: the static predicate is nearly always true (read this first)

`brilliant::candidates` asks "does this move give material away?" — it is a **nomination**, not a
judgement. Measured on five middlegame positions with a probe over every root move and every
reply (`static_scan_cost`, throwaway test in `src/brilliant.rs`):

| position | replies | replies "allowing a brilliancy" | candidates/reply |
|---|---|---|---|
| Greek gift | 1020 | **1018 (99.8%)** | 12.7 |
| QGD-type | 1158 | 1117 (96.5%) | 5.4 |
| Nimzo-type | 2039 | **2036 (99.9%)** | 26.6 |
| K+pawn ending | 152 | 29 (19%) | 0.36 |

So **"a brilliancy is available" is true almost everywhere**, and is false only in quiet endings.
Tier 1 ("every reply allows") and Tier 2's `allows` test as written would fire on essentially
every move of every middlegame — the rule as literally specified is not discriminating.

The real predicate is "a **sound** brilliancy is available", and soundness is what the neutral
look decides. That makes the per-reply test a *search*, not a scan, and that changes the cost
profile from the one assumed below. The scan cost itself is genuinely cheap and is not the
problem: 0.35–15.6 ms for **all** root moves in the sample (5–27 µs per reply).

A second probe tested whether soundness narrows it down. It does not, nearly enough:

| position | root moves | nominated at root | survive opponent's best capture |
|---|---|---|---|
| Greek gift | 34 | **33** | 25 |
| QGD-type | 36 | 36 | 34 |
| Nimzo-type | 33 | 33 | 31 |
| Alapin | 40 | 13 | 10 |
| Kiwipete | 48 | 48 | 3 |
| Queen's Indian | 45 | 44 | 34 |
| Hedgehog-ish | 37 | 37 | 26 |

A cheap SEE "robustness" filter does not rescue it either. **The nomination criterion classifies
almost every move in a normal middlegame as a brilliancy.** Any rule of the form "play `S` if a
brilliancy is available after it" therefore contains no information in those positions.

This is the central finding and it reshapes the feature: what makes a move brilliant is not that
it is *classified* as a sacrifice, but that it is **unusually good for how much it gives up** —
it must beat the engine's own judgement of what is available, by a margin. The predicate to
build is therefore relative and rare:

> `S` is a **glance**: after `S`, a brilliancy `B` exists whose objective value is at least
> `margin` better than every quiet alternative, where `margin` is large (the existing look uses
> ~45 cp) — i.e. exactly the existing look's "sound sacrifice" condition, but evaluated one move
> ahead. "Brilliant in N" then means: a move that *creates* a glance.

Frequency is then the thing to measure first: if a glance appears once in a hundred moves, the
feature is a pleasure; if it appears once in five, it is the engine's default behaviour and
`Brilliance` must gate it hard.

MEASURED (`testing/look_frequency.py`, 16 middlegame positions at 300 ms, shipped 5.4.0 binary):
the **existing immediate look** prints a verdict in 13/16 positions but changes the move in only
**2/16 (12%)**. Its verdicts are mostly refusals ("sacrifice -276 vs best quiet 10"), and with
Brilliance at its default of 100 the look spends its share of the clock on them anyway. A horizon
look compounds this: it must find a *sound* brilliancy one move ahead, which is strictly rarer
than finding one now. Expect a hit rate at or below 12%, and plan for the common case being
"looked, found nothing" — which must be cheap and silent.

This number did not exist before and is the baseline any implementation is measured against.

Treat the sections below as the rule's shape, with this rare, relative predicate in place of the
static nomination test — this is the question the fleet review is asked to settle.

## The rule (user's, verbatim intent)

A **setup move** `S` is a quiet root move after which a brilliancy is available to us at our next
move. A **brilliancy** `B` is what `brilliant::candidates` nominates (material sacrifice incl.
desperado; underpromotion) *and* the neutral verification accepts as sound.

For candidate `S`, enumerate the opponent's replies `R` and split them statically:

* `allows` — `brilliant::candidates(position after S,R)` is non-empty;
* `declines` — the rest.

Then:

```
Tier 1  FORCED
    declines is empty                                  (every reply allows)
    AND B is verified sound against the opponent's best reply
    -> play S

Tier 2  NO-REGRET BAIT
    allows is non-empty
    AND min over declining R' of value(after S,R') >= value(normal best move) - 20cp
    AND B is verified sound on the opponent's best *allowing* reply
    -> play S

otherwise -> the normal best move
```

Tier 1 is the same predicate as Tier 2 with an empty `declines` (min over an empty set is +inf),
so one code path can express both; "forced" is then just the report.

**REVIEW FINDING (blocker, accepted):** Tier 1 as written has *no score bound at all*, yet it is
called "forced". The opponent picks whichever reply minimises our value, and an *allowing* reply
is constrained by nothing except a single-reply verification of `B`. A position with root score
+120 can contain a setup `S` after which their best reply leaves us +20 — a brilliancy is
"available" there because they walked into it being *good for them*, not for us — and Tier 1 would
play it. The bound must apply to **every** reply, not just the declining ones:

```
Tier 1  FORCED
    for every reply R:  a sound brilliancy exists after R
    AND for every reply R:  value(after S,R) >= value(normal best) - 20cp
    -> play S
```

The second clause is what the user's "if they can make the position worse, don't do it" actually
requires; with it, Tier 1 is no longer unbounded and the FORCED label is earned.

**REVIEW FINDING (blocker, accepted):** "sound" must not be the root look's near-equality
thresholds reused at a child two plies deeper on a fraction of the budget. The root look's
definition is objective-within-80cp-and-above-50cp-at-depth-5, and the code says outright that a
shallow look cannot tell sound from unsound. At the horizon the honest definition is weaker and
must be stated as such: `B` is a *verified improvement* — it is searched to the same depth as the
best quiet alternative at its child, and beats it by the look's margin. If the node budget does
not allow both searches to reach that depth, the horizon does **not** claim a brilliancy.


The 20 cp is a noise tolerance, not a concession budget: the bait is free or it is not played.
That is deliberately different from the rejected "blanket 30 cp allowance".

## Horizon: clock-scaled, iteratively deepened

`N` is the number of plies of setup before `B`. Derived from the look's own budget, not a
constant, and **iterative**: run `N = 2`; if budget remains, run `N = 4`; etc. Report the depth
actually reached, the way the main search reports `done_depth`.

* `go movetime 300` (the user's rig): `N = 2` almost always.
* long think (`> 4 s`, `DEEP_MIN_MS`): `N = 4`, maybe 6 if the budget allows.

At `N = 2` the chain is `S` (ours), `R` (theirs), and `B` is then available at ply 3.

## Cost, which is the whole difficulty

MEASURED budget (`probe.py`, Greek-gift position, `go movetime 300`, 1 thread): the main search
completes depth 13 at 156 ms using 96 000 nodes. The 5.3.0 look formula (`BRILLIANCE_PCT` 30)
then gives the look `156 * 30/70 ≈ 66 ms`. Divide that by the design's three searches per
candidate:

| candidates `K` | ms per verification search | ≈ nodes (measured ~600k n/s) |
|---|---|---|
| 1 | 22 | 13 000 |
| 2 | 11 | 6 600 |
| 3 | 7 | 4 200 |
| 4 | 5 | 3 000 |

That is enough budget to *confirm* a brilliancy (the existing look does exactly this at a similar
size) but not to *refute* one, and refutation is what a horizon look needs most: its whole job is
to reject the many nominees that look brilliant and are not. So the design must be built so that
the common case is cheap and silent, and it must never claim a refutation it did not reach.

Per candidate `S`:

1. static enumeration of all replies + a static brilliancy scan of each child (no search);
2. one `group_look` over `declines` (restricted root = those replies) -> the worst declining
   value. **Skipped entirely when `declines` is empty (Tier 1).**
3. one `group_look` over `allows` -> their best allowing reply, and its PV;
4. one verification comparison at that child: `B` vs the quiet alternatives (existing
   `group_look`, `exact = true`).

That is up to three searches per candidate. Boundaries to respect:

* only the top few quiet root moves are candidates (`K`), in the search's own ranking;
* the whole horizon look gets a slice of the move's budget, and reports incompleteness rather
  than silently claiming it looked;
* the static scan early-exits on the first brilliancy found (typical case), so step 1 is cheap.

## Gates that must not move

`bench` = 287284 nodes (`main.rs` calls `Searcher::search` directly, so post-search looks are
invisible); Classic preset parity (`testing/regress_classic.py`, uses `go depth`); `ucitest`
43 checks; `cargo test --release`; `testing/brilliance.py` 36 checks.

The horizon look must therefore be gated off for: `user_nodes`, `user_depth`, MultiPV > 1,
non-empty `ROOT_FILTER`, `Brilliance == 0`, and (unlike the immediate look) it must not run when
the move is forced/already decided.

## Not a prover, and why

A true "brilliant in N against every defence" is DFPN-able only if the goal predicate is cheap
and static. The honest goal would be "a *sound* brilliancy exists", and soundness needs a
search — so every DFPN goal test becomes a search and the tree explodes. A static herald
(e.g. "a sacrifice that gives check exists") proves the herald, not the brilliancy, and can be
outright wrong. This design therefore *verifies* on the opponent's best allowing reply and
*scans statically* over all replies, and says so rather than claiming a proof.

Open question for review: is a bounded DFPN goal-search feasible at any think time, or is the
above genuinely the right ceiling?
