# Lever results — owner-full programme, 2026-11-14/15

Baseline = `bng_ctrl_s11` (1500 games, both sides byte-identical): HONEST **0.0420**/g,
owner-full **0.0800**/g (63 honest sac + 5 underpromo + 52 ignored).
Noise floor for a 1500-game arm: **±0.0147** HONEST/g (SD 0.0062, five provable null controls).

| arm | knob | games | HONEST/g | delta vs ctl | Elo (ctl−var) | verdict |
|---|---|---|---|---|---|---|
| bng_ctrl_s11 | (control) | 1500 | 0.0420 | — | −17.4 | baseline |
| ws1_s11 | `x_winsacoff=1` | 1500 | 0.0407 | −0.0013 | −13.9 [−30,+2] | **null** |
| ws1_s7 | `x_winsacoff=1` | 1500 | pending | | −26.0 [−43,−10] | Elo-sig worse |
| as470_s11 | `x_aftersac=470` | 1500 | pending | | −4.4 [−21,+12] | |
| as470_s7 | `x_aftersac=470` | 1500 | pending | | +0.7 [−16,+17] | |
| hl50_s11 | `x_hanglive=50` | 1500 | pending | | +6.6 [−10,+24] | |
| hl150_s7 | `x_hanglive=150` | 1500 | pending | | −2.9 [−20,+14] | |
| planm1_s11 | `x_plan=60` mode 1 | 1500 | pending | | pending | |
| planm1_s7 | `x_plan=60` mode 1 | 1500 | pending | | pending | |

## ws1_s11 detail (x_winsacoff=1)

The lever was aimed at the "already winning" gate (199 piece+ events/1500 games die there).
It did not move that gate:

| funnel step | control | ws1_s11 |
|---|---|---|
| all sac events | 2217 | 2210 |
| piece+ (not pawn) | 824 | 866 |
| not mating | 741 | 747 |
| not already winning | 542 | 551 |
| not lost | 234 | 211 |
| sound (LOOSE) | 116 | 103 |
| HONEST | 63 | 61 |

The "not already winning" count barely changed (542→551, +9 of ~551, i.e. ~2%), so switching
off the winning-sacrifice licence did **not** meaningfully reduce how often the engine
sacrifices from a won position. That is worth knowing: the gate is mostly fed by sacrifices
the search reaches for *before* the position is decided, not by the licence. So the funnel's
"199 die as already winning" is not the lever-sized target the handover assumed.

Verdict: **null on both metric and Elo** (−0.0013, well inside ±0.0147).

## x_winsacoff — pooled verdict: NULL

| seed | HONEST/g | delta | Elo |
|---|---|---|---|
| s11 | 0.0407 | −0.0013 | −13.9 [−30,+2] |
| s7 | 0.0560 | +0.0140 | −26.0 [−43,−10] |

Pooled delta **+0.0064**, i.e. **null** (floor ±0.0147 per arm, and the two seeds disagree in
sign). Elo is −14 and −26, so the lever is not free either — it weakly *hurts*.

The mechanism check is the more useful result, and it is negative for the hypothesis: the
"not already winning" funnel step barely moved (542 → 551), so switching off the winning-sacrifice
licence does not meaningfully reduce sacrificing from won positions. The 199 events the handover
flagged as killing the funnel are mostly sacrifices the search reaches for **before** the game is
decided, not ones the licence promotes. The gate is real but it is not the lever-sized target it
was believed to be.

## Plan layer fire rate (why the arm is not a guaranteed null, and not a big one either)

Replaying real games with `AGGRO_PLANDBG=1`: **4/117 searches fire (3.4%), 4/99 eligible (4.0%)**.
So the plan re-ranks roughly one move in 25 — a few thousand moves across a 1500-game arm. That is
enough for an effect to show if the idea is right, and small enough that a null would be
uninformative about fire rate alone. Mode 2/3 have not been fire-measured yet.

## x_aftersac=470 — pooled verdict: NULL

| seed | HONEST/g | delta | Elo |
|---|---|---|---|
| s11 | 0.0447 | +0.0027 | −4.4 [−21,+12] |
| s7 | 0.0387 | −0.0033 | +0.7 [−16,+17] |

Pooled delta **−0.0003**, null. This one is the most informative null of the three: the knob
raises the post-sacrifice appetite cap from 180 to 470 (2.6x) and is mechanically live
(bite-verified 5/16), Elo-neutral, yet the honest count does not move. Together with
[[x_winsacoff]] and the six prior lever families, that closes the "bonus volume / appetite"
hypothesis for good: **more appetite does not produce more honest brilliants**, because the
bottleneck is the moved-piece gate, not the willingness to give material away.

## Knob scoreboard (all pooled, 2 seeds x 1500 games each)

| lever | pooled delta HONEST/g | verdict |
|---|---|---|
| x_winsacoff=1 | +0.0064 | null |
| x_aftersac=470 | −0.0003 | null |
| x_hanglive | pending | |
| x_plan=60 (mode 1) | pending | |
| x_plan=200 (mode 1) | pending | |

## WHY x_winsacoff was null — the mechanism, not the measurement

Over the control arm's 741 piece+ non-mating sacrifice events:

* the arena calls **199** of them "already winning" (Stockfish `sf_before >= 300`);
* of those 199, the engine's OWN eval (`e0`) was >= 300 in **94%** and >= 500 in **83%** — so the
  arena and the engine agree the position is won; this is not a labelling mismatch;
* **but the winning-sacrifice licence (`WIN_SAC_AT=500`) fires on only ~11%** of the 199
  (`e0 >= 500` in 166 of the *whole* 741, 60 of the 542 non-winning ones).

`x_winsacoff` switches off the licence. Most of those 199 events never used it — they were reached
by the **pressure ramp** (`20 + 15t`), which the lever only touches through the `decided`
acceleration. So the lever was aimed at a sub-mechanism governing roughly a ninth of its target,
which is why the funnel step did not move (542 → 551).

The deeper reading: suppressing machinery does not stop the engine sacrificing in a won position,
because *it is still winning afterwards*. A 900cp position that drops to 700cp is still won, so the
search keeps liking the sacrifice. **"Already winning" is not a fixable gate from the bonus side** —
it needs the evaluation to care that a decided game should be finished cleanly, which no bonus knob
expresses. That closes the whole "stop sacrificing when winning" branch of the funnel analysis.

## x_hanglive — the only lever that moved its target, but still null on sound

`x_hanglive` targets the IGNORED-ATTACK shape (43% of owner-full), so the sacrifice leg is the
wrong metric for it. Measured properly:

| arm | structural flags | **SF-sound** | sound/g | delta |
|---|---|---|---|---|
| bng_ctrl_s11 | 386 (0.2573/g) | **52** | 0.0347 | — |
| hl50_s11 | 448 (0.2987/g, **+16%**) | **66** | 0.0440 | +0.0093 |
| hl150_s7 | 415 (0.2767/g, +7.5%) | **47** | 0.0313 | −0.0033 |

Pooled sound delta **+0.0030**, against a 95% bar of **0.0188** (Poisson, 52-count base) — **null**.
On the sacrifice leg it was also null (0.0447, 0.0347 vs 0.0420).

What is real and worth keeping: the lever **raises the raw ignored-attack shape on both seeds**
(+16%, +7.5%), and the *sound fraction* stays roughly flat (13.5% → 14.7% / 11.3%). So the lever
does what it claims mechanically — it makes the engine leave pieces hanging more often — but most
of the increase is not Stockfish-sound, and the sound part is inside noise. The honest reading is
that "hang more material" buys a bigger candidate pool without buying more genuine brilliancies,
which is the same wall the whole programme keeps hitting.

## Plan layer: it does what it claims (verified before reading the arm)

The plan arms are only informative if the layer actually picks sacrifices. Sampling 372 searches
across 25 real games with `AGGRO_PLANDBG=1`:

* **fires 20/372 = 5.4%** at margin 200 (9.5% of *eligible* searches from the earlier probe);
* **15 of 19 sampled picks (79%) are GENUINE sacrifices** by direct material counting — the mover
  ends up >=150cp down after the opponent's best capture. Examples: `d2e4` (gives up 320),
  `f3g5` (320), `f6g4` (320), `c1h6` (330), `f8f3` (580), `e1e7` (500).

So the layer is not a no-op and not firing on trades: it re-ranks to quiet and capturing moves that
hand over real material, which is precisely the badge shape the funnel says the engine fails to
play. That makes the arm a fair test of the idea rather than of a broken lever — whatever the honest
metric does next is attributable to the plan, not to the plumbing.

Elo cost is real and expected (margin 60: +20; margin 200: +75 at 2/3 distance) — sanctioned, since
the floor is the only limit.

## "Already winning" is closed on evidence, not inference

Of the control arm's 199 piece+ non-mating events the arena calls "already winning", **182 (91%)
are still clearly winning after the sacrifice** (`sf_after > 150`), and 156 (78%) are still at
>= +300. So for nine in ten of them the sacrifice genuinely did not hurt the engine — which is
precisely why suppressing the appetite cannot help: there is nothing to gain by declining a
sacrifice that leaves you winning anyway. Combined with the licence governing only ~11% of the set,
this closes the entire "stop sacrificing when winning" branch of the funnel analysis.

# THE DECISIVE RESULT: the plan layer is null too

| arm | margin / mode | HONEST/g | delta vs 0.0420 | Elo cost |
|---|---|---|---|---|
| planm1_s11 | 60 / mode 1 | 0.0353 | −0.0067 | +22.7 |
| planm1_s7 | 60 / mode 1 | 0.0460 | +0.0040 | +27.2 |
| planbig_s11 | 200 / mode 1 | 0.0433 | +0.0013 | +72.7 |

Pooled mode-1 (2 seeds): **−0.0014** — null, while paying ~25 Elo.

This is the most important result of the session, because unlike the knob nulls it cannot be
explained away by the lever not working:

* the layer **fires** (5.4% of searches; 9.5% of eligible at margin 200);
* **79% of its picks are genuine sacrifices** by direct material counting (verified independently);
* at margin 200 it **shifts the funnel exactly as intended** — "not already winning" rises
  542 → 610 (+68 events, +13%), i.e. sacrifices really do move into balanced positions.

And the honest count still does not move. planm1_s11 is the sharpest case: it shifts 68 more
sacrifices into badgeable positions and the moved-piece count goes **down** (63 → 53). More
sacrifice-shaped opportunities in the right contexts did not convert into more brilliants.

## What this means

The plan layer was the strongest version of the "steer the search" idea: proactive, verified to
fire, verified to pick real sacrifices, aimed at the exact gate the funnel identifies, and free to
spend Elo. It fails anyway. Together with the three knob nulls and the six prior lever families, the
honest conclusion is:

**The honest brilliancy rate is not a knobbable or re-rankable quantity in this engine's current
event mix.** The ceiling is structural: ~0.087 sound sacrifices/game exist, and turning them into
~0.042 moved-piece brilliants is where the process ends. Re-ranking near-equal moves cannot create
positions that were never there — which is exactly what the plan's own failure mode shows (it moved
sacrifices toward balance and the badge count fell).

The only direction with real headroom left is changing **which positions arise at all** — an opening
repertoire / playing-style change, i.e. the gambit book. Every other lever family is now measured
out.

## Counter-check: did ANY arm shift a badge count in a way the sacrifice leg hides?

Scored the other two legs too, so a movement there cannot hide behind a flat sacrifice count.

| arm | sacrifice HONEST | ignored-attack structural |
|---|---|---|
| bng_ctrl_s11 | 63 | 386 (0.2573/g) |
| planm1_s11 | 53 | 389 (0.2593/g) |
| planm1_s7 | 69 | 369 (0.2460/g) |
| planbig_s11 | 65 | 303 (0.2020/g) |

No leg moves the right way: the sacrifice count is flat-to-down, and the ignored-attack shape is
flat-to-**down** (planbig_s11 loses 21% of it, which is unsurprising -- the plan re-ranks toward
offering the MOVED piece, so it spends moves that would otherwise have left another piece hanging).
The STRICTEST tier is flat too (33 -> 30 / 41 / 42). There is no hidden effect in any of the three
shapes.

# FIRST POSITIVE SIGNAL: plan mode 2 (ignored attacks)

Mode 2 re-ranks toward leaving *another* piece winnable -- the ignored-attack shape, 43% of
owner-full, and the one the sacrifice-focused plan mode 1 actively suppressed.

| arm | sacrifice HONEST | ignored structural | **ignored SF-sound** | owner-full |
|---|---|---|---|---|
| bng_ctrl_s11 | 0.0420 (63) | 0.2573 (386) | **0.0347 (52)** | 0.0800 |
| planm2_s23 | 0.0427 (64) | **0.3640 (546, +41%)** | **0.0467 (70, +35%)** | **0.0927 (+16%)** |

* sound delta **+0.0120/g**, against a 95% bar of 0.0188 -> **not significant on one seed**;
* the sound FRACTION of the shape falls slightly (13.5% -> 12.8%), so the gain is from volume, not
  purity -- consistent with the `x_hanglive` lesson that hanging more is not automatically more
  brilliant, but here the volume gain is large enough that some of it survives Stockfish;
* Elo cost +46.6 -- large, but the floor is the only limit.

**Status: a lead, not a result.** Replication on seeds 11 and 7 is running (`planm2_s11`,
`planm2_s7`). One seed under the bar is exactly the pattern the project has been burned by before
(x_sacungate cleared the bar on s11 and reversed on s23), so nothing ships on this alone.

Note the asymmetry that makes this plausible rather than noise: mode 1 (offer the MOVED piece)
pushed the ignored-attack shape DOWN 21%, and mode 2 pushes it UP 41%. The lever demonstrably
controls the shape it targets, in both directions, which is what a real mechanism looks like.

## Mode 2's extra sound badges come from the right bucket

Splitting the sound (SF-graded) ignored attacks by the position's eval BEFORE the move:

| arm | sound flags | losing (<-100) | balanced | winning (>100) |
|---|---|---|---|---|
| bng_ctrl_s11 | 52 | 19 | 19 | 14 |
| planm2_s23 | 70 | 29 | **32** | 9 |

Mode 2's gain is concentrated in the **balanced** bucket (19 -> 32), which is where a badge genuinely
matters, not in already-decided positions. And "sound" is a genuinely different population from
"speculative": the NOISE flags cluster in winning positions (control noise: 45 winning / 55 balanced
/ only 10 losing), so the Stockfish gate is selecting on soundness, not merely on position class.

This is supporting evidence that the lead is a real effect rather than the lever padding its count
with cheap positions. It is still one seed until the replicates land.

# Gambit book (move-1 rig): the largest STRUCTURAL movement of the session

First real test of the curated gambit repertoire, with the `openings` line dropped so games start
at move 1 and the book can actually fire (every other arm forces the first 8 plies, so the book is
invisible to them).

| arm | games | structural ignored-attack | Elo (ctl−var) |
|---|---|---|---|
| bng_ctrl_s11 (ply-9 rig) | 1500 | 0.2564/g | — |
| book1500_s11 (move 1, book ON vs OFF) | 1356+ | **0.4387/g (+71%)** | −39.6 |

Two things are already clear and one is a caution:

1. The book **does** what an opening repertoire is for: the ignored-attack shape rises 71%, the
   biggest structural movement any arm has produced. Gambit lines really do reach positions where
   pieces hang.
2. Book ON beats book OFF by ~40 Elo, and the games are genuinely varied (400/400 distinct move
   sequences, so this is not a determinism artefact).
3. **The caution:** with the openings line dropped, the two sides no longer start from a shared
   balanced position. The book arm measures "book vs no book from move 1", not "book vs book from a
   common opening" -- so its Elo number is about the repertoire's quality, and the honest/game
   numbers cannot be compared directly against the ply-9 control above. Any brilliancy claim from
   this arm must be book-ON leg vs book-OFF leg within the same arm, which is what the graded
   events give.

SF-sound grading of the 0.4387/g structural rate is running; only that number decides whether the
book is a brilliance lever or just a different position mix.

# Mode 2 failed replication — the lead is dead

Three seeds on the ignored-attack leg:

| seed | structural | SF-sound | sound/g | delta |
|---|---|---|---|---|
| (control) | 386 | 52 | 0.0347 | — |
| s23 | 546 (+41%) | 70 | 0.0467 | +0.0120 |
| s11 | 485 (+26%) | 43 | 0.0287 | **−0.0060** |
| s7 | 536 (+39%) | 62 | 0.0413 | +0.0067 |

**Pooled +0.0042/g — null**, against a 0.0188 bar, with signs disagreeing across seeds. This is
exactly the failure mode the handover flagged (x_sacungate: +0.0167 on s11, −0.0053 on s23), and it
is why nothing ships on one seed.

The CONSISTENT part is the raw shape: structural rises on all three seeds (mean +35%, 386 -> 522).
The sound count does not follow it — the SF-sound fraction of the shape falls from 13.5% to
11.2% pooled. So mode 2 reliably makes the engine leave more pieces hanging, and reliably fails to
make that count as brilliancy, because most of the extra hangs are not Stockfish-sound.

**This is now the third independent instance of the same wall:**

| lever | target shape movement | sound movement |
|---|---|---|
| x_hanglive | structural +16% / +7.5% | flat |
| plan mode 1 | ignored shape **−21%** | flat |
| plan mode 2 | structural **+35% (3 seeds)** | **null, pooled +0.0042** |

A lever can move the raw shape in either direction, reliably and reproducibly, and the
Stockfish-sound count does not move with it. **The raw shape and the badge are decoupled.**

## The gambit book: the one thing that DID move the honest metric

Book arm, compared within itself (the only fair comparison, since dropping the openings line means
the two sides no longer share a starting position):

| leg | sacs/g | LOOSE | HONEST | strictest |
|---|---|---|---|---|
| book OFF (ctl) | 1.679 | 0.0707 | 0.0387 | 0.0240 |
| book ON (var) | 1.737 | 0.0853 | **0.0547** | **0.0340** |

**+0.0160 HONEST/g**, above the 0.0147 floor, and the strictest tier moves with it (+0.0100).
Repertoire quality also shows up as strength: book ON beats book OFF by ~40 Elo, with genuinely
varied games (400/400 distinct sequences).

But its own ignored-attack leg tells the same cautionary story: structural +69% (652 vs 386), sound
fraction collapsed 13.5% -> 9.7% (63 vs 52). So the book's honest gain is carried by the SACRIFICE
leg, not the ignored-attack leg.

**Status: the book is the live lead, one seed, needs replication.** Seed 11 is grading.

## Book, two seeds: pooled +28% honest badges, at the edge of significance

Within-arm (book ON vs OFF on the same 1500 openings, same seed, so genuinely paired):

| seed | book OFF | book ON | delta/g |
|---|---|---|---|
| s7 | 58 | 82 | +0.0160 |
| s11 | 52 | 59 | +0.0047 |
| **pooled (3000 games)** | **110** | **141** | **+0.0103** |

CI [+0.000, +0.021] -- not yet significant. Both seeds point the same way, effect size +28% on the
count. Seed 23 is running to settle it.

## The book is ON BY DEFAULT -- so this lead is already live in real play

Verified with zero setoptions: the shipped binary plays a book move from the initial position
(d2d4), and after 1.e4 e5 it plays f2f4 (the King's Gambit). own_book defaults true (uci.rs:138),
the book file ships alongside the binary, and nothing else gates it.

Every MATCH forces the first 8 plies from 8moves_v3.txt, so the arms are the only place the book is
invisible; the owner's real games have had it all along. The book arms therefore measure what the
EXISTING repertoire is worth, not whether to add one -- which had never been measured, because no
previous arm dropped the openings line.

# WHY "more shape" never becomes "more badge" — the dilution law

Grading every ignored-attack flag by the position's own eval band (Stockfish, raw UCI):

| arm | in-band | lost (<=-300) | win (>=+300) | sound (all from in-band) |
|---|---|---|---|---|
| bng_ctrl_s11 | 133 | 128 | 39 | 44 |
| planm2_s23 | 190 | **208** | 44 | 69 |

Mode 2's extra 142 flags split **+57 in-band, +80 LOST, +5 win**. So **56% of the extra shape lands
in positions that are already lost** — where the badge gate rejects it by construction (`sf_after <=
-150`, and Stockfish says <= -300). It adds 80 structurally ineligible flags for every 57 eligible
ones. Even within the in-band bucket the sound rate barely moves (33% -> 36%).

**This is the general law behind all three failed shape-levers:**

| lever | raw shape | eligible (in-band) share | sound |
|---|---|---|---|
| x_hanglive | +16% / +7.5% | mostly not eligible | flat |
| plan mode 1 | −21% | — | flat |
| plan mode 2 | +35% (3 seeds) | 56% of the gain is in LOST positions | null |

A lever that makes the engine leave more material hanging does so **mostly in positions it is
already losing**, where hanging more changes nothing that can be counted. That is why the raw shape
moves reliably in both directions while the badge count does not follow: the extra shape is not just
unsound, it is *in the wrong half of the board*.

**The actionable consequence** for any future lever: it is not enough to increase the badge SHAPE;
the increase must land in IN-BAND positions. The gambit book is the only intervention so far that
plausibly does this, because it changes the positions themselves (opening the game from balanced
gambit lines) rather than the engine's appetite within a position it already has.

## A confound in the book arm's design, caught before it is reported

The book-ON side gets something the book-OFF side does not: **move diversity**.

| side | first-move distribution over 300 games |
|---|---|
| ctl (book OFF) | `1.e4` in **151/151** — fully deterministic |
| var (book ON) | e4 90, d4 33, c4 18, Nf3 8 |

So the arm measures "gambit repertoire **plus** opening variety" vs "one deterministic move", not
"gambit quality" alone. Part of the +28% could be diversity rather than the repertoire's content.

This does NOT invalidate the measurement -- it is a real difference in how the two configurations
play, and the owner's real settings are the book-ON configuration, so the comparison is the one that
matters for the product. But the honest description is "book ON beats book OFF", and any claim that
the *specific gambit lines* are what does it needs a third arm (a variety-matched control: a book
that picks randomly among the same first moves) that has not been run. Recorded so the claim does
not outrun the evidence.
