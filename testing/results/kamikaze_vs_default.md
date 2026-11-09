# Sacrifice report: modified vs original AggroChess

Generated 2026-11-09T05:21:35Z by `testing/sactest.py` from run directory `testing/runs/sactest-kamikaze`.

* **new** = `aggro_new`: AggroChess Ultimate Sacrificial Edition 5.0.0, options `Threads=1; Hash=16; Skill Level=21; Aggression=100; Sacrifice Appetite=100; King Attack Priority=100; Material Preservation=0; Speculative Sacrifices=100` (personality: Aggression=100;Sacrifice Appetite=100;King Attack Priority=100;Material Preservation=0;Speculative Sacrifices=100)
* **old** = `aggro_old`: AggroChess Ultimate Sacrificial Edition 5.0.0, options `Threads=1; Hash=16`

## Setup

|  |  |
|---|---|
| Date (UTC) | 2026-11-09T05:11:00Z .. 2026-11-09T05:21:35Z |
| Host / CPU | kv1 / Intel(R) Core(TM) Ultra 5 125H (18 logical CPUs, 12.5 GB RAM) |
| OS / Python | Linux 6.8.0-147-generic / Python 3.12.3 |
| Time control | 10+0.1 |
| Games | 40 per engine per anchor; head-to-head 60 (colour-reversed opening pairs) |
| Anchors | Stockfish UCI_Elo 1800 (Threads=1, Hash=16, UCI_LimitStrength=true) |
| Concurrency | 12 |
| Openings | `testing/openings/8moves_v3.txt`, seed 1 (both gauntlets use the same openings and colours) |
| Analysis | Stockfish `/usr/games/stockfish`, 400000 nodes/position, 12 processes x 1 thread, Hash 64 |

|  | id | source | sha256 | copy used |
|---|---|---|---|---|
| aggro_new | AggroChess Ultimate Sacrificial Edition 5.0.0 | `/root/aggroclone/tools/bin/aggro_use_rc3` | `3a587a03ac6b856bb3e02e9e9540573da958b6cc305c73e1369994ab265b3a67` | `testing/runs/sactest-kamikaze/bin/aggro_new` |
| aggro_old | AggroChess Ultimate Sacrificial Edition 5.0.0 | `/root/aggroclone/tools/bin/aggro_use_rc3` | `3a587a03ac6b856bb3e02e9e9540573da958b6cc305c73e1369994ab265b3a67` | `testing/runs/sactest-kamikaze/bin/aggro_old` |
| book | ph-gambitbook.bin | `/root/aggroclone/AggroChess/ph-gambitbook.bin` | `f2dddc37343b155624fddb08a7743965e806b717ac87985f887643e126119f14` | (engine cwd) |
| arena | aggro_arena | `testing/arena/target/release/aggro_arena` | `877596a01ad52ddce35b9ccae7abd2d7e01a29f6473dc10198102e8952b056a5` |  |

## Match results

| pairing | games | W/D/L (first engine) | score | Elo diff [95% CI] | LOS | forfeits |
|---|---|---|---|---|---|---|
| aggro_new vs sf1800 | 40/40 | +14 =4 -22 | 40.0% | -70 [-185, +30] | 9.1% | none |
| aggro_old vs sf1800 | 40/40 | +17 =3 -20 | 46.2% | -26 [-136, +78] | 31.1% | none |
| aggro_new vs aggro_old | 60/60 | +27 =9 -24 | 52.5% | +17 [-64, +101] | 66.3% | none |

## Sacrifices: all games (anchors + head-to-head)

Rates are per game unless stated; proportions with Wilson 95% intervals (scores: Wilson on score points, draws = 1/2), means with normal-approximation 95% intervals; `new - old` with Newcombe (proportions, scores) or Welch (means) intervals. Head-to-head games appear in both columns (once from each side).

| metric | new | old | new - old |
|---|---|---|---|
| Games analysed | 100 | 100 | +0 |
| Score (all games) | 47.5% [38.0, 57.2] | 47.0% [37.5, 56.7] | +0.5 pp [-13.1, +14.1] |
| Games with >= 1 sacrifice | 77/100 = 77.0% [67.8, 84.2] | 90/100 = 90.0% [82.6, 94.5] | -13.0 pp [-23.2, -2.7] |
| Sacrifices | 153 | 189 | -36 |
| Sacrifices per game | 1.53 [1.29, 1.77] | 1.89 [1.66, 2.12] | -0.36 [-0.69, -0.03] |
| Sacrifices per 100 engine moves | 2.98 | 3.58 | -0.60 |
| Balanced-position sacrifices per game (own eval within +-3) | 0.71 [0.54, 0.88] | 0.88 [0.70, 1.06] | -0.17 [-0.42, +0.08] |
| Games with a balanced-position sacrifice | 50/100 = 50.0% [40.4, 59.6] | 60/100 = 60.0% [50.2, 69.1] | -10.0 pp [-23.2, +3.7] |
|   queen sacrifices | 2 | 6 | -4 |
|   exchange sacrifices | 6 | 11 | -5 |
|   piece sacrifices | 67 | 70 | -3 |
|   pawn sacrifices | 78 | 102 | -24 |
| Context balanced / winning / losing / unknown | 71 / 47 / 35 / 0 | 88 / 52 / 49 / 0 |  |
| Book-gambit sacrifices | 0 | 0 | +0 |
| Intent unknown (no score) | 0 | 0 | +0 |
| Chained sacrifices (chain_len > 1) | 10 | 15 | -5 |
| Blunders (filtered out) | 168 | 170 | -2 |
|   of which caused by the previous move (own eval had collapsed) | 107 | 112 | -5 |
| Blunders per game | 1.68 [1.44, 1.92] | 1.70 [1.44, 1.96] | -0.02 [-0.37, +0.33] |
| Forced material losses (single legal move; excluded) | 1 | 2 | -1 |
| Moves while being mated (desperation; excluded) | 52 | 33 | +19 |
| Mate within 30 plies (of sacrifices) | 19/153 = 12.4% [8.1, 18.6] | 34/189 = 18.0% [13.2, 24.1] | -5.6 pp [-13.0, +2.3] |
| Recouped within 24 plies | 69/153 = 45.1% [37.4, 53.0] | 81/189 = 42.9% [36.0, 50.0] | +2.2 pp [-8.2, +12.7] |
| Successful attacks | 69/153 = 45.1% [37.4, 53.0] | 88/189 = 46.6% [39.6, 53.7] | -1.5 pp [-11.9, +9.1] |
| Unsound (SF; of SF-evaluated sacrifices) | 26/153 = 17.0% [11.9, 23.7] | 27/189 = 14.3% [10.0, 20.0] | +2.7 pp [-5.0, +10.7] |
| Practical chances (of unsound) | 14/26 = 53.8% [35.5, 71.2] | 14/27 = 51.8% [34.0, 69.3] | +2.0 pp [-23.3, +26.9] |
| Blunders confirmed by SF | 46/168 = 27.4% [21.2, 34.6] | 41/170 = 24.1% [18.3, 31.1] | +3.3 pp [-6.0, +12.5] |
| Score in games with a sacrifice | 50.0% [39.1, 60.9] | 50.6% [40.4, 60.7] | -0.6 pp [-15.4, +14.3] |
| Score in games without a sacrifice | 39.1% [22.2, 59.2] | 15.0% [3.5, 45.9] | +24.1 pp [-11.1, +47.3] |
| Average move number of sacrifices | 30.73 [27.23, 34.22] | 33.31 [30.04, 36.58] | -2.59 [-7.37, +2.20] |
| Average SF eval before the sacrifice (cp, engine view) | 3 [-163, 170] | 65 [-89, 220] | -62 [-289, +165] |
| Average SF eval after the sacrifice (cp, engine view) | -65 [-239, 109] | -19 [-181, 144] | -46 [-285, +192] |

### Per position context (per 100 engine moves)

Per-game counts depend on how long each engine spends in balanced positions (a weaker engine spends more of its games losing). Here each engine's own moves are split by its own score when it moved (balanced = within +-3 pawns, winning/losing beyond), and sacrifices and blunders are counted per 100 engine moves in each context. 'Material given' adds sacrifices and blunders: every persistent material deficit the engine started, whatever the intent filter says.

| engine | context | engine moves | sacrifices (per 100) | blunders (per 100) | material given per 100 |
|---|---|---|---|---|---|
| new | balanced | 2074 | 71 (3.42) | 76 (3.66) | 7.09 |
| new | winning | 1467 | 47 (3.20) | 28 (1.91) | 5.11 |
| new | losing | 1577 | 35 (2.22) | 64 (4.06) | 6.28 |
| old | balanced | 2178 | 88 (4.04) | 67 (3.08) | 7.12 |
| old | winning | 1445 | 52 (3.60) | 28 (1.94) | 5.54 |
| old | losing | 1632 | 49 (3.00) | 75 (4.60) | 7.60 |

### Per category

| category | new n | new successful | new mate | new unsound | old n | old successful | old mate | old unsound |
|---|---|---|---|---|---|---|---|---|
| queen | 2 | 1/2 = 50.0% [9.4, 90.5] | 1/2 = 50.0% [9.4, 90.5] | 0/2 = 0.0% [0.0, 65.8] | 6 | 4/6 = 66.7% [30.0, 90.3] | 3/6 = 50.0% [18.8, 81.2] | 0/6 = 0.0% [0.0, 39.0] |
| exchange | 6 | 3/6 = 50.0% [18.8, 81.2] | 0/6 = 0.0% [0.0, 39.0] | 1/6 = 16.7% [3.0, 56.4] | 11 | 7/11 = 63.6% [35.4, 84.8] | 3/11 = 27.3% [9.8, 56.6] | 0/11 = 0.0% [0.0, 25.9] |
| piece | 67 | 29/67 = 43.3% [32.1, 55.2] | 13/67 = 19.4% [11.7, 30.4] | 21/67 = 31.3% [21.5, 43.2] | 70 | 36/70 = 51.4% [40.0, 62.7] | 18/70 = 25.7% [16.9, 37.0] | 19/70 = 27.1% [18.1, 38.5] |
| pawn | 78 | 36/78 = 46.2% [35.5, 57.1] | 5/78 = 6.4% [2.8, 14.1] | 4/78 = 5.1% [2.0, 12.5] | 102 | 41/102 = 40.2% [31.2, 49.9] | 10/102 = 9.8% [5.4, 17.1] | 8/102 = 7.8% [4.0, 14.7] |

## Per opponent

### vs sf1800

| metric | new | old | new - old |
|---|---|---|---|
| Games analysed | 40 | 40 | +0 |
| Score (all games) | 40.0% [26.4, 55.4] | 46.2% [31.8, 61.3] | -6.2 pp [-26.6, +14.9] |
| Games with >= 1 sacrifice | 33/40 = 82.5% [68.0, 91.2] | 38/40 = 95.0% [83.5, 98.6] | -12.5 pp [-27.4, +2.0] |
| Sacrifices per game | 1.70 [1.31, 2.09] | 2.10 [1.70, 2.50] | -0.40 [-0.96, +0.16] |
| Balanced-position sacrifices per game (own eval within +-3) | 0.90 [0.67, 1.13] | 0.93 [0.65, 1.20] | -0.03 [-0.38, +0.33] |
|   queen sacrifices | 1 | 0 | +1 |
|   exchange sacrifices | 1 | 4 | -3 |
|   piece sacrifices | 27 | 25 | +2 |
|   pawn sacrifices | 39 | 55 | -16 |
| Blunders per game | 1.85 [1.46, 2.24] | 1.90 [1.49, 2.31] | -0.05 [-0.62, +0.52] |
| Mate within 30 plies (of sacrifices) | 5/68 = 7.3% [3.2, 16.1] | 12/84 = 14.3% [8.4, 23.3] | -6.9 pp [-16.9, +3.6] |
| Successful attacks | 19/68 = 27.9% [18.7, 39.6] | 33/84 = 39.3% [29.5, 50.0] | -11.4 pp [-25.5, +3.8] |
| Unsound (SF; of SF-evaluated sacrifices) | 12/68 = 17.6% [10.4, 28.4] | 15/84 = 17.9% [11.1, 27.4] | -0.2 pp [-12.2, +12.4] |
| Practical chances (of unsound) | 5/12 = 41.7% [19.3, 68.0] | 5/15 = 33.3% [15.2, 58.3] | +8.3 pp [-25.2, +40.4] |
| Score in games with a sacrifice | 39.4% [24.7, 56.3] | 48.7% [33.7, 63.9] | -9.3 pp [-30.5, +13.3] |
| Score in games without a sacrifice | 42.9% [15.8, 75.0] | 0.0% [0.0, 65.8] | +42.9 pp [-28.2, +75.0] |

### head-to-head (new vs old; each column is that engine's side)

| metric | new | old | new - old |
|---|---|---|---|
| Games analysed | 60 | 60 | +0 |
| Score (all games) | 52.5% [40.1, 64.6] | 47.5% [35.4, 59.9] | +5.0 pp [-12.5, +22.1] |
| Games with >= 1 sacrifice | 44/60 = 73.3% [61.0, 82.9] | 52/60 = 86.7% [75.8, 93.1] | -13.3 pp [-27.2, +1.1] |
| Sacrifices per game | 1.42 [1.12, 1.71] | 1.75 [1.48, 2.02] | -0.33 [-0.74, +0.07] |
| Balanced-position sacrifices per game (own eval within +-3) | 0.58 [0.36, 0.81] | 0.85 [0.60, 1.10] | -0.27 [-0.60, +0.07] |
|   queen sacrifices | 1 | 6 | -5 |
|   exchange sacrifices | 5 | 7 | -2 |
|   piece sacrifices | 40 | 45 | -5 |
|   pawn sacrifices | 39 | 47 | -8 |
| Blunders per game | 1.57 [1.27, 1.86] | 1.57 [1.22, 1.91] | +0.00 [-0.45, +0.45] |
| Mate within 30 plies (of sacrifices) | 14/85 = 16.5% [10.1, 25.8] | 22/105 = 20.9% [14.3, 29.7] | -4.5 pp [-15.3, +7.0] |
| Successful attacks | 50/85 = 58.8% [48.2, 68.7] | 55/105 = 52.4% [42.9, 61.7] | +6.4 pp [-7.7, +20.1] |
| Unsound (SF; of SF-evaluated sacrifices) | 14/85 = 16.5% [10.1, 25.8] | 12/105 = 11.4% [6.7, 18.9] | +5.0 pp [-4.8, +15.5] |
| Practical chances (of unsound) | 9/14 = 64.3% [38.8, 83.7] | 9/12 = 75.0% [46.8, 91.1] | -10.7 pp [-40.9, +23.5] |
| Score in games with a sacrifice | 58.0% [43.3, 71.3] | 51.9% [38.7, 64.9] | +6.0 pp [-13.5, +24.8] |
| Score in games without a sacrifice | 37.5% [18.5, 61.4] | 18.8% [4.5, 53.3] | +18.8 pp [-20.7, +46.6] |

## Example sacrifices

Balanced-position sacrifices first, larger categories first. `e0 -> e1` = the engine's own score when it played the move and one move later; SF = Stockfish before -> after the move (engine view).

**aggro_new**

| game | opponent | move | category | context | d4 | e0 -> e1 | SF | result, success | line |
|---|---|---|---|---|---|---|---|---|---|
| 33 | aggro_old | 20.Rxf6 | exchange | balanced | 170 | 22 -> 127 | 0 -> 0 | win, recouped | `20.Rxf6 Rxf6 21.Qe7 Rf2 22.Qh7+ Kf8 23.Qh8+ Kf7` |
| 33 | sf1800 | 33.Rxe7+ | exchange | balanced | 180 | -220 -> -296 | -544 -> -606 | loss, - | `33.Rxe7+ Bxe7 34.Qc2 g6 35.Rf4 Kg7 36.h5 gxh5` |
| 52 | aggro_old | 12...gxh6 | exchange | balanced | 170 | -93 -> -83 | -109 -> -420 | loss, unsound | `12...gxh6 13.Nxa8 Rg8 14.Nac7 Rg4 15.Qxh6+ Kg8 16.Bd3` |
| 56 | aggro_old | 24...Kh7 | exchange | balanced | 180 | -64 -> -68 | -19 -> -71 | win, - | `24...Kh7 25.Nxf8+ Qxf8 26.Qf5+ Kh8 27.dxc4 Rd8 28.Rxa1` |
| 1 | aggro_old | 9.Nf4 | piece | balanced | 320 | -116 -> -131 | 27 -> -390 | win, recouped | `9.Nf4 exf4 10.O-O Ngf6 11.Nb3 Bh3 12.Bxh3 fxg3` |
| 2 | sf1800 | 13...Bg4 | piece | balanced | 330 | -150 -> -199 | 19 -> -401 | win, recouped | `13...Bg4 14.hxg4 Qb6 15.Nd1 O-O-O 16.g5 Nxe4 17.Bg2` |
| 20 | aggro_old | 12...h5 | piece | balanced | 220 | -27 -> 302 | 75 -> -154 | win, recouped | `12...h5 13.hxg4 hxg4 14.g3 Qh5 15.Nxd6 cxd6 16.f4` |
| 21 | sf1800 | 15.Ba3 | piece | balanced | 220 | -289 -> -253 | -406 -> -434 | win, recouped | `15.Ba3 exd5 16.exd5 Qc8 17.Re1 Kg8 18.Bxf8 Kxf8` |

**aggro_old**

| game | opponent | move | category | context | d4 | e0 -> e1 | SF | result, success | line |
|---|---|---|---|---|---|---|---|---|---|
| 19 | aggro_new | 65...Qg4+ | queen | balanced | 730 | 203 -> 283 | 10 -> 3 | draw, recouped | `65...Qg4+ 66.Rxg4 Bxg4+ 67.Kxg4 c1=Q 68.Qf7+ Kd4 69.Qf6+` |
| 19 | sf1800 | 84.Rxd5+ | exchange | balanced | 170 | -75 -> -75 | 0 -> 0 | draw, recouped | `84.Rxd5+ Kxd5 85.h6 Ke6 86.h7 Rg2+ 87.Kf8 Ra2` |
| 40 | aggro_new | 15.Ba3 | exchange | balanced | 180 | 49 -> -67 | -125 -> -241 | loss, recouped | `15.Ba3 Nxa1 16.Rxa1 h5 17.Qh3 Nxf4 18.Qe3 Nxg2` |
| 56 | aggro_new | 14.Ke2 | exchange | balanced | 170 | -254 -> -218 | -156 -> -157 | loss, recouped | `14.Ke2 Nxa1 15.Nf3 b6 16.Nxe5+ Kg8 17.Qa4 h6` |
| 60 | aggro_new | 23.Rxc5 | exchange | balanced | 170 | -113 -> -45 | -267 -> -228 | draw, recouped | `23.Rxc5 Qxc5 24.Nc2 Qc3+ 25.Re3 Qd2 26.Rxe6+ fxe6` |
| 2 | aggro_new | 18.Rf7+ | exchange | balanced | 180 | -75 -> -75 | -366 -> -343 | loss, - | `18.Rf7+ Kxf7 19.Qxd7+ Kg6 20.Qe6+ Kh5 21.Kh3 Raf8` |
| 23 | sf1800 | 21.Rxe6 | exchange | balanced | 170 | -59 -> -13 | -80 -> -218 | draw, - | `21.Rxe6 Qxe6 22.Qg5+ Kf8 23.Qd8+ Kg7 24.Qg5+ Kf8` |
| 6 | aggro_new | 44.Kc2 | piece | balanced | 500 | 95 -> 9 | 0 -> -36 | draw, sf | `44.Kc2 Qxa1 45.Qa8 Ne6 46.Qg2+ Kh4 47.Qxh2+ Kg5` |

## Definitions and caveats

* Detection (`aggro_arena analyze`, see testing/README.md): `bal(i)` = engine material minus opponent
  material (P=100 N=320 B=330 R=500 Q=900) before ply i; a sacrifice starts at an engine ply k outside the opening
  file with a persistent deficit d2 = bal(k)-bal(k+2), d4 = bal(k)-bal(k+4) (pawn: also d6), categorised by the
  piece-composition change k..k+4 (queen > exchange > piece > pawn). A move whose own engine score drops by more
  than 1.5 pawns one move later is a *blunder*, not a sacrifice. Plies k+2 and k+4 after a sacrifice only extend
  its chain.
* Additions to the spec rule: two trade guards (trades whose recapture is delayed by an in-between move are
  not sacrifices) and the `context` of each sacrifice from the engine's own eval (balanced = within +-3 pawns).
  AggroChess deliberately looks for a sacrifice in every game and accepts almost any material sacrifice once
  the game is decided, so the *balanced-position* rows are the cleanest measure of chosen, game-relevant
  sacrifices; sacrifices in lost positions are often just material it was going to lose anyway.
* Successful attack = mate within 30 plies, or material recouped within 24 plies with the engine's own score
  not worse than at the sacrifice (-50), or SF >= +150 for the engine 12 plies later. Unsound = SF after the
  move < SF before - 150 and < -100. Practical chance = unsound but the game was not lost, or SF >= +100 for the
  engine at a later engine move within 30 plies.
* Proportions over sacrifices treat sacrifices as independent; several sacrifices in one game are correlated,
  so those intervals are somewhat optimistic. Per-game rates (games with a sacrifice, sacrifices per game,
  score) use the game as the unit. New and old play the same anchor openings, but the intervals for `new - old`
  assume independent samples (conservative); head-to-head games are shared by both columns.
* The engines' own scores differ in scale (the original engine's evaluation is much more optimistic than
  Stockfish), which affects the intent filter and the `context` split.
