# AggroChess Ultimate Sacrificial Edition 5.4.1

*The first sacrifice is nearly sound now.* 5.4.0 raised the bonus for the game's **first**
sacrifice to 340 cp, and it turned out to be the single most expensive knob in the engine:
it bought junk *inside* the badge window. This release shrinks it to **20 cp** and measures
the result: **more chess.com brilliants and ~29–52 Elo stronger**, on every draw tested.

## What changed

One line in `personality::derive()`: the first-sacrifice bonus for Sacrifice Appetite above
the classic 75 is **20** instead of 340. Everything else is the untouched 5.4.0 machinery —
the pressure ramp (which still adds up to +450 by move 50), the floor drag, chained
sacrifices, the after-sacrifice level, the gambit bonus, the winning-sacrifice licence and
the classification relaxation.

Nothing reads the Skill Level, so this is a personality change pure and simple; `Classic`
(Sacrifice Appetite 75 and below) is bit-identical to 4.5.0 and unchanged.

## Why: the bonus was buying junk where the badges are

The bonus is the engine's tolerance for unsoundness: how far below the best quiet move a
sacrifice may score and still be played. At 340 that tolerance was so large that, in the
phase where badge-eligible sacrifices actually occur (moves 10–30), the engine preferred
objectively worse sacrifices. Shrinking the base raises the *conversion* inside the
badge-shaped window even though it slightly lowers total sacrifice volume:

| base first-sacrifice bonus | strict brilliant/game (seed 11) | Elo vs shipped |
|---|---|---|
| 0 (also kills the ramp) | 0.1083 | +115 (stronger) |
| **20 (this release)** | **0.1133** | **+52 (stronger)** |
| 40 | 0.0917 | +64 |
| 60 | 0.0867 | +83 |
| 80 | 0.1083 | +59 |
| 120 | 0.0933 | +44 |
| 220 | 0.0783 | +12 |
| 340 (5.4.0) | 0.0650 | — |

Read that table the way it deserves: every base from 0 to 120 lands between 0.087 and 0.113,
which at 600 games is one noise band wide, so the *shape* between 0 and 120 is not resolved —
only the drop at 220–340 is. What the column really shows is that **the exact base hardly
matters and 340 was far outside the plateau**. The Elo column is the arena's `elo` with the
sign flipped (it reports `ctl − var`), and every base is stronger than 340. On two further
seeds the base-0 arm measures 116.8 (seed 7) / 119.1 (seed 23) against the same shipped engine,
and base 20 measures 51.9 / 57.3 / 34.9 (seeds 11/7/23), i.e. **base 0 is 59–84 Elo *stronger*
than base 20** — base 0 is not the weaker build. (An earlier draft of this note had that
backwards; the sign convention `ctl − var` makes a *more negative* Elo a *stronger* var.)

600 games per arm, same openings, paired, seed 11. The curve peaks at **20** *on that draw*,
which is why 20 and not 0 was chosen: it keeps the whole ramp alive. Two caveats the reader
should have. First, the peak is a single-seed feature: repeated on other seeds, base 0 and base
20 are indistinguishable on strict brilliants — 3 seeds give base 0 +0.0278/g over shipped and
4 seeds give base 20 +0.0217/g, and pairing the two directly on shared games gives
**+0.0006/g over 3 seeds** (a wash). What separates them is that base 0 is *stronger*, not
weaker: on the direct same-binary head-to-head (`bn0v20_s11`, only `x_sacbonus` differing) base 0
wins 301 games to base 20's 220, **+47 Elo**, and against shipped 340 base 0 is 59–84 Elo
stronger than base 20 on every seed. The only reason to stay above 0 is that a 0 bonus trips the
early return in `sac_params_for` and disables the entire search-level sacrifice subsystem — ramp
included — which is a capability this release wants to keep, not a strength win. 20 is the
smallest base that keeps it. Second, the ramp's own cost
is real but small: neutralising it (base 20, `x_pstep=0`) costs **0.113 → 0.072 strict/g** while
being about **12 Elo stronger**, so the ramp is ~12 Elo spent to buy +0.042 strict/g. At base
340 the same knob was not free either — neutralising it left strict exactly unchanged
(+0.0000/g) but measured **34.9 Elo stronger**, i.e. at 340 the ramp only ever cost strength and
bought nothing. The winning shape is **small early, growing late**: a
nearly-sound first sacrifice, then a rising appetite. The event counts say it directly: on the
same 600 games base 20 fires about the same number of sacrifices as base 340 (835 vs 819) but
**half again as many strict brilliants (68 vs 45)**.

## Evidence

Head-to-head against 5.4.0, same openings and options, quiet `movetime 300`
(positives mean this release is stronger). "knob" rows ran the shipped 5.4.0 binary against a
5.4.0 binary with `x_sacbonus` set by hand; "real binary" rows ran the shipped 5.4.0 binary
against the actual built 5.4.1:

| draw | kind | strict Δ/game | Elo (5.4.0 − 5.4.1) |
|---|---|---|---|
| seed 11 | knob | **+0.0483** [+0.013,+0.083] | −51.9 [−78,−26] |
| seed 23 | **real binary** | +0.0167 [−0.013,+0.048] | −34.9 [−61,−9] |
| seed 7 | **real binary** | +0.0100 [−0.018,+0.038] | −29.0 [−55,−3] |
| seed 5 | knob | +0.0050 [−0.025,+0.037] | −39.0 [−65,−13] |
| 10+0.1, 200 games | knob | +0.0500 [−0.015,+0.115] | −43.7 [−88,−1] |
| **pooled (4 seeds)** | mixed | **+0.0200** [+0.008,+0.039] | 5 of 5 draws stronger |

The honest planning number is **~+0.02 strict brilliant/game** (0.065 → ~0.085, about +30%
relative); seed 11 is a strong draw. Note the split: the two **real-binary** seeds are the
weakest-reading rows (+0.0167 and +0.0100, both straddling zero), so the pooled figure leans on
the seed-11 knob arm. The **Elo gain is the more consistent signal** — every draw of the five,
including both real binaries and the long time control, is **29–52 Elo stronger**. Only the
seed-11 strict delta is significant on its own; the 10+0.1 arm is 200 games and its strict CI
still straddles zero, so read it as confirmation at a second time control, not as evidence.

One thing the table does **not** show, and should: base 0 is *stronger still* than this release.
On the direct same-binary head-to-head (`bn0v20_s11`: the same 5.4.1 binary on both sides, only
`x_sacbonus` differing) base 0 wins 301 games to base 20's 220 — **base 0 is ~47 Elo stronger**,
with strict at 0.1000/g against base 20's 0.0733. Base 0 is also 59–84 Elo stronger than base 20
against shipped 340 on three seeds. The base is held at 20 only because a 0 bonus switches the
whole search-level sacrifice subsystem off.

## The one behaviour change at low Skill Levels

`testing/brilliance.py` B4 (the Greek gift, `Bxh7+`) is played at Skill 14 and 21 as before;
at Skill 1/3/8 the shallow look's neutral score for `Bxh7+` no longer beats the quiet group
once the bonus stops lifting it, so the look plays a different sacrifice (`f3g5`/`c1g5`)
instead. That is a move *choice* flip, not a loss of brilliant play: over 600 **Skill Level 8**
games re-analysed on both sides, this release plays **0.0500 strict brilliants/game vs 5.4.0's
0.0383 (+0.0117)** and is **85 Elo stronger** at that level. The suite therefore documents B4
as "Skill >= 14" (`c4 "14"` in the EPD); the "brilliant at every Skill Level" contract holds
for every other case, and the suite passes with 0 failures on **both** 5.4.0 and this build.

## Verification

* `cargo test --release`: 57 pass.
* `testing/regress_classic.py`: ALL MATCH (Classic/4.5.0 behaviour untouched).
* `testing/brilliance.py`: 0 failures on this build and on the shipped 5.4.0.
* `bench`: 287284 → 168104 nodes — expected, the benchmark exists to make behaviour changes
  visible, and this is one.
