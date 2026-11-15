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
