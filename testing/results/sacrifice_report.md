# Sacrifice report: modified vs original AggroChess

Generated 2026-11-09T05:03:10Z by `testing/sactest.py` from run directory `testing/runs/sactest-final`.

* **new** = `aggro_new`: AggroChess Ultimate Sacrificial Edition 5.0.0, options `Threads=1; Hash=16; Skill Level=21` (personality: default)
* **old** = `aggro_old`: AggroChess 4.5.0, options `Threads=1`

## Setup

|  |  |
|---|---|
| Date (UTC) | 2026-11-09T04:43:08Z .. 2026-11-09T05:03:10Z |
| Host / CPU | kv1 / Intel(R) Core(TM) Ultra 5 125H (18 logical CPUs, 12.5 GB RAM) |
| OS / Python | Linux 6.8.0-147-generic / Python 3.12.3 |
| Time control | 10+0.1 |
| Games | 60 per engine per anchor; head-to-head 100 (colour-reversed opening pairs) |
| Anchors | Stockfish UCI_Elo 1800, Stockfish UCI_Elo 2000 (Threads=1, Hash=16, UCI_LimitStrength=true) |
| Concurrency | 14 |
| Openings | `testing/openings/8moves_v3.txt`, seed 1 (both gauntlets use the same openings and colours) |
| Analysis | Stockfish `/usr/games/stockfish`, 400000 nodes/position, 12 processes x 1 thread, Hash 64 |

|  | id | source | sha256 | copy used |
|---|---|---|---|---|
| aggro_new | AggroChess Ultimate Sacrificial Edition 5.0.0 | `/root/aggroclone/tools/bin/aggro_use_rc2` | `90c49ff0e3403197b2a5f595cab4334411999edcc0c021271559f27ee80f789f` | `testing/runs/sactest-final/bin/aggro_new` |
| aggro_old | AggroChess 4.5.0 | `/root/aggroclone/engines/aggro_original` | `a010a1024c43705366551996d383929949a3bf32a02b18c3acd906cda625b252` | `testing/runs/sactest-final/bin/aggro_old` |
| book | ph-gambitbook.bin | `/root/aggroclone/AggroChess/ph-gambitbook.bin` | `f2dddc37343b155624fddb08a7743965e806b717ac87985f887643e126119f14` | (engine cwd) |
| arena | aggro_arena | `testing/arena/target/release/aggro_arena` | `877596a01ad52ddce35b9ccae7abd2d7e01a29f6473dc10198102e8952b056a5` |  |

## Match results

| pairing | games | W/D/L (first engine) | score | Elo diff [95% CI] | LOS | forfeits |
|---|---|---|---|---|---|---|
| aggro_new vs sf1800 | 60/60 | +22 =13 -25 | 47.5% | -17 [-98, +61] | 33.1% | none |
| aggro_new vs sf2000 | 60/60 | +20 =3 -37 | 35.8% | -101 [-200, -16] | 1.2% | none |
| aggro_old vs sf1800 | 60/60 | +39 =8 -13 | 71.7% | +161 [+79, +264] | 100.0% | none |
| aggro_old vs sf2000 | 60/60 | +38 =2 -20 | 65.0% | +108 [+22, +208] | 99.1% | none |
| aggro_new vs aggro_old | 100/100 | +25 =14 -61 | 32.0% | -131 [-204, -68] | 0.0% | none |

## Sacrifices: all games (anchors + head-to-head)

Rates are per game unless stated; proportions with Wilson 95% intervals (scores: Wilson on score points, draws = 1/2), means with normal-approximation 95% intervals; `new - old` with Newcombe (proportions, scores) or Welch (means) intervals. Head-to-head games appear in both columns (once from each side).

| metric | new | old | new - old |
|---|---|---|---|
| Games analysed | 220 | 220 | +0 |
| Score (all games) | 37.3% [31.1, 43.8] | 68.2% [61.8, 74.0] | -30.9 pp [-39.3, -21.7] |
| Games with >= 1 sacrifice | 187/220 = 85.0% [79.7, 89.1] | 172/220 = 78.2% [72.3, 83.1] | +6.8 pp [-0.4, +14.0] |
| Sacrifices | 398 | 339 | +59 |
| Sacrifices per game | 1.81 [1.64, 1.98] | 1.54 [1.38, 1.70] | +0.27 [+0.03, +0.50] |
| Sacrifices per 100 engine moves | 3.65 | 3.10 | +0.56 |
| Balanced-position sacrifices per game (own eval within +-3) | 1.02 [0.88, 1.16] | 0.65 [0.53, 0.76] | +0.38 [+0.20, +0.56] |
| Games with a balanced-position sacrifice | 138/220 = 62.7% [56.2, 68.8] | 102/220 = 46.4% [39.9, 53.0] | +16.4 pp [+7.1, +25.3] |
|   queen sacrifices | 11 | 10 | +1 |
|   exchange sacrifices | 19 | 22 | -3 |
|   piece sacrifices | 151 | 104 | +47 |
|   pawn sacrifices | 217 | 203 | +14 |
| Context balanced / winning / losing / unknown | 225 / 65 / 107 / 1 | 142 / 116 / 80 / 1 |  |
| Book-gambit sacrifices | 1 | 1 | +0 |
| Intent unknown (no score) | 5 | 2 | +3 |
| Chained sacrifices (chain_len > 1) | 39 | 27 | +12 |
| Blunders (filtered out) | 429 | 302 | +127 |
|   of which caused by the previous move (own eval had collapsed) | 283 | 107 | +176 |
| Blunders per game | 1.95 [1.77, 2.13] | 1.37 [1.24, 1.51] | +0.58 [+0.35, +0.80] |
| Forced material losses (single legal move; excluded) | 6 | 11 | -5 |
| Moves while being mated (desperation; excluded) | 109 | 43 | +66 |
| Mate within 30 plies (of sacrifices) | 39/398 = 9.8% [7.2, 13.1] | 77/339 = 22.7% [18.6, 27.5] | -12.9 pp [-18.3, -7.6] |
| Recouped within 24 plies | 146/398 = 36.7% [32.1, 41.5] | 158/339 = 46.6% [41.4, 51.9] | -9.9 pp [-16.9, -2.8] |
| Successful attacks | 143/398 = 35.9% [31.4, 40.8] | 176/339 = 51.9% [46.6, 57.2] | -16.0 pp [-23.0, -8.8] |
| Unsound (SF; of SF-evaluated sacrifices) | 72/398 = 18.1% [14.6, 22.2] | 16/339 = 4.7% [2.9, 7.5] | +13.4 pp [+8.9, +17.8] |
| Practical chances (of unsound) | 34/72 = 47.2% [36.1, 58.6] | 10/16 = 62.5% [38.6, 81.5] | -15.3 pp [-37.3, +11.2] |
| Blunders confirmed by SF | 110/429 = 25.6% [21.7, 30.0] | 157/302 = 52.0% [46.4, 57.6] | -26.4 pp [-33.2, -19.2] |
| Score in games with a sacrifice | 38.5% [31.8, 45.6] | 66.6% [59.2, 73.2] | -28.1 pp [-37.5, -17.8] |
| Score in games without a sacrifice | 30.3% [17.4, 47.3] | 74.0% [60.1, 84.3] | -43.7 pp [-60.2, -21.7] |
| Average move number of sacrifices | 29.45 [27.61, 31.30] | 34.89 [32.62, 37.17] | -5.44 [-8.37, -2.51] |
| Average SF eval before the sacrifice (cp, engine view) | -140 [-225, -54] | 198 [72, 324] | -338 [-490, -186] |
| Average SF eval after the sacrifice (cp, engine view) | -225 [-317, -134] | 154 [30, 278] | -379 [-534, -225] |

### Per position context (per 100 engine moves)

Per-game counts depend on how long each engine spends in balanced positions (a weaker engine spends more of its games losing). Here each engine's own moves are split by its own score when it moved (balanced = within +-3 pawns, winning/losing beyond), and sacrifices and blunders are counted per 100 engine moves in each context. 'Material given' adds sacrifices and blunders: every persistent material deficit the engine started, whatever the intent filter says.

| engine | context | engine moves | sacrifices (per 100) | blunders (per 100) | material given per 100 |
|---|---|---|---|---|---|
| new | balanced | 4634 | 225 (4.86) | 185 (3.99) | 8.85 |
| new | winning | 1842 | 65 (3.53) | 48 (2.61) | 6.13 |
| new | losing | 4371 | 107 (2.45) | 196 (4.48) | 6.93 |
| old | balanced | 4397 | 142 (3.23) | 100 (2.27) | 5.50 |
| old | winning | 3588 | 116 (3.23) | 120 (3.34) | 6.58 |
| old | losing | 2914 | 80 (2.75) | 82 (2.81) | 5.56 |

### Per category

| category | new n | new successful | new mate | new unsound | old n | old successful | old mate | old unsound |
|---|---|---|---|---|---|---|---|---|
| queen | 11 | 4/11 = 36.4% [15.2, 64.6] | 4/11 = 36.4% [15.2, 64.6] | 0/11 = 0.0% [0.0, 25.9] | 10 | 7/10 = 70.0% [39.7, 89.2] | 6/10 = 60.0% [31.3, 83.2] | 0/10 = 0.0% [0.0, 27.8] |
| exchange | 19 | 7/19 = 36.8% [19.1, 59.0] | 1/19 = 5.3% [0.9, 24.6] | 3/19 = 15.8% [5.5, 37.6] | 22 | 13/22 = 59.1% [38.7, 76.7] | 7/22 = 31.8% [16.4, 52.7] | 1/22 = 4.5% [0.8, 21.8] |
| piece | 151 | 64/151 = 42.4% [34.8, 50.4] | 18/151 = 11.9% [7.7, 18.1] | 61/151 = 40.4% [32.9, 48.4] | 104 | 62/104 = 59.6% [50.0, 68.5] | 36/104 = 34.6% [26.2, 44.2] | 8/104 = 7.7% [4.0, 14.4] |
| pawn | 217 | 68/217 = 31.3% [25.5, 37.8] | 16/217 = 7.4% [4.6, 11.6] | 8/217 = 3.7% [1.9, 7.1] | 203 | 94/203 = 46.3% [39.6, 53.2] | 28/203 = 13.8% [9.7, 19.2] | 7/203 = 3.5% [1.7, 7.0] |

## Per opponent

### vs sf1800

| metric | new | old | new - old |
|---|---|---|---|
| Games analysed | 60 | 60 | +0 |
| Score (all games) | 47.5% [35.4, 59.9] | 71.7% [59.2, 81.5] | -24.2 pp [-39.8, -6.6] |
| Games with >= 1 sacrifice | 51/60 = 85.0% [73.9, 91.9] | 46/60 = 76.7% [64.6, 85.6] | +8.3 pp [-5.9, +22.3] |
| Sacrifices per game | 1.72 [1.40, 2.03] | 1.60 [1.25, 1.95] | +0.12 [-0.35, +0.59] |
| Balanced-position sacrifices per game (own eval within +-3) | 0.97 [0.69, 1.25] | 0.67 [0.47, 0.87] | +0.30 [-0.04, +0.64] |
|   queen sacrifices | 1 | 2 | -1 |
|   exchange sacrifices | 3 | 5 | -2 |
|   piece sacrifices | 40 | 22 | +18 |
|   pawn sacrifices | 59 | 67 | -8 |
| Blunders per game | 1.98 [1.60, 2.37] | 1.45 [1.18, 1.72] | +0.53 [+0.06, +1.00] |
| Mate within 30 plies (of sacrifices) | 10/103 = 9.7% [5.4, 17.0] | 19/96 = 19.8% [13.1, 28.9] | -10.1 pp [-20.1, -0.2] |
| Successful attacks | 37/103 = 35.9% [27.3, 45.5] | 45/96 = 46.9% [37.2, 56.8] | -11.0 pp [-24.1, +2.7] |
| Unsound (SF; of SF-evaluated sacrifices) | 21/103 = 20.4% [13.7, 29.2] | 6/96 = 6.2% [2.9, 13.0] | +14.1 pp [+4.7, +23.5] |
| Practical chances (of unsound) | 13/21 = 61.9% [40.9, 79.2] | 3/6 = 50.0% [18.8, 81.2] | +11.9 pp [-25.8, +47.6] |
| Score in games with a sacrifice | 48.0% [34.9, 61.4] | 67.4% [53.0, 79.1] | -19.4 pp [-36.9, +0.3] |
| Score in games without a sacrifice | 44.4% [18.9, 73.3] | 85.7% [60.1, 96.0] | -41.3 pp [-68.8, -2.6] |

### vs sf2000

| metric | new | old | new - old |
|---|---|---|---|
| Games analysed | 60 | 60 | +0 |
| Score (all games) | 35.8% [24.9, 48.5] | 65.0% [52.4, 75.8] | -29.2 pp [-44.6, -11.3] |
| Games with >= 1 sacrifice | 47/60 = 78.3% [66.4, 86.9] | 48/60 = 80.0% [68.2, 88.2] | -1.7 pp [-16.1, +12.9] |
| Sacrifices per game | 1.82 [1.47, 2.17] | 1.53 [1.22, 1.84] | +0.28 [-0.18, +0.75] |
| Balanced-position sacrifices per game (own eval within +-3) | 0.92 [0.67, 1.16] | 0.67 [0.47, 0.86] | +0.25 [-0.06, +0.56] |
|   queen sacrifices | 3 | 1 | +2 |
|   exchange sacrifices | 9 | 8 | +1 |
|   piece sacrifices | 39 | 21 | +18 |
|   pawn sacrifices | 58 | 62 | -4 |
| Blunders per game | 1.98 [1.65, 2.31] | 1.28 [1.03, 1.54] | +0.70 [+0.28, +1.12] |
| Mate within 30 plies (of sacrifices) | 15/109 = 13.8% [8.5, 21.5] | 22/92 = 23.9% [16.4, 33.6] | -10.2 pp [-21.1, +0.6] |
| Successful attacks | 32/109 = 29.4% [21.6, 38.5] | 41/92 = 44.6% [34.8, 54.7] | -15.2 pp [-28.0, -1.9] |
| Unsound (SF; of SF-evaluated sacrifices) | 21/109 = 19.3% [13.0, 27.7] | 3/92 = 3.3% [1.1, 9.2] | +16.0 pp [+7.4, +24.7] |
| Practical chances (of unsound) | 6/21 = 28.6% [13.8, 50.0] | 2/3 = 66.7% [20.8, 93.8] | -38.1 pp [-69.0, +12.5] |
| Score in games with a sacrifice | 37.2% [24.9, 51.5] | 66.7% [52.5, 78.3] | -29.4 pp [-46.4, -9.3] |
| Score in games without a sacrifice | 30.8% [12.7, 57.6] | 58.3% [31.9, 80.7] | -27.6 pp [-56.3, +10.1] |

### head-to-head (new vs old; each column is that engine's side)

| metric | new | old | new - old |
|---|---|---|---|
| Games analysed | 100 | 100 | +0 |
| Score (all games) | 32.0% [23.7, 41.7] | 68.0% [58.3, 76.3] | -36.0 pp [-47.8, -22.3] |
| Games with >= 1 sacrifice | 89/100 = 89.0% [81.4, 93.8] | 78/100 = 78.0% [68.9, 85.0] | +11.0 pp [+0.6, +21.2] |
| Sacrifices per game | 1.86 [1.62, 2.10] | 1.51 [1.29, 1.73] | +0.35 [+0.02, +0.68] |
| Balanced-position sacrifices per game (own eval within +-3) | 1.12 [0.91, 1.33] | 0.62 [0.44, 0.80] | +0.50 [+0.22, +0.78] |
|   queen sacrifices | 7 | 7 | +0 |
|   exchange sacrifices | 7 | 9 | -2 |
|   piece sacrifices | 72 | 61 | +11 |
|   pawn sacrifices | 100 | 74 | +26 |
| Blunders per game | 1.91 [1.66, 2.16] | 1.38 [1.19, 1.57] | +0.53 [+0.21, +0.85] |
| Mate within 30 plies (of sacrifices) | 14/186 = 7.5% [4.5, 12.2] | 36/151 = 23.8% [17.8, 31.2] | -16.3 pp [-24.3, -8.6] |
| Successful attacks | 74/186 = 39.8% [33.0, 47.0] | 90/151 = 59.6% [51.6, 67.1] | -19.8 pp [-29.9, -9.1] |
| Unsound (SF; of SF-evaluated sacrifices) | 30/186 = 16.1% [11.5, 22.1] | 7/151 = 4.6% [2.3, 9.3] | +11.5 pp [+5.0, +17.9] |
| Practical chances (of unsound) | 15/30 = 50.0% [33.1, 66.8] | 5/7 = 71.4% [35.9, 91.8] | -21.4 pp [-47.8, +17.9] |
| Score in games with a sacrifice | 33.7% [24.7, 44.0] | 66.0% [55.0, 75.6] | -32.3 pp [-45.4, -17.2] |
| Score in games without a sacrifice | 18.2% [5.1, 47.7] | 75.0% [54.2, 88.4] | -56.8 pp [-75.5, -20.7] |

## Example sacrifices

Balanced-position sacrifices first, larger categories first. `e0 -> e1` = the engine's own score when it played the move and one move later; SF = Stockfish before -> after the move (engine view).

**aggro_new**

| game | opponent | move | category | context | d4 | e0 -> e1 | SF | result, success | line |
|---|---|---|---|---|---|---|---|---|---|
| 96 | aggro_old | 29...Kc7 | queen | balanced | 720 | -283 -> -270 | -207 -> -193 | draw, - | `29...Kc7 30.Rxd8 Rbxd8 31.Bxc6 Kxc6 32.Qxf7 Rd1+ 33.Kg2` |
| 4 | aggro_old | 12...cxd4 | exchange | balanced | 180 | 71 -> -4 | 339 -> 69 | win, sf | `12...cxd4 13.Nxh8 dxc3 14.bxc3 Qc6 15.e4 Qxe4 16.f3` |
| 7 | aggro_old | 13.Bxc5 | exchange | balanced | 180 | 23 -> 124 | -6 -> -221 | draw, recouped | `13.Bxc5 Nxh1 14.f4 Bf8 15.Bxf8 Qb6 16.Qxd5 Qf2+` |
| 14 | aggro_old | 18...Qg5 | exchange | balanced | 180 | -217 -> -221 | -310 -> -345 | win, recouped | `18...Qg5 19.Qxa8 Qxf5 20.Bc4+ Kh8 21.e4 Qg4 22.Bf7` |
| 3 | sf2000 | 16.Rxd6 | exchange | balanced | 170 | 173 -> 183 | -28 -> -68 | win, - | `16.Rxd6 Qxd6 17.Rg1 Qe5 18.Bd4 Qd6 19.Rxg7+ Ke8` |
| 26 | aggro_old | 22...Re3 | exchange | balanced | 170 | 255 -> 218 | 165 -> 74 | draw, - | `22...Re3 23.Bxe3 Qxe3 24.Rg3 Qd2 25.Re1 Qxb2 26.g6` |
| 67 | aggro_old | 24.g3 | exchange | balanced | 270 | -78 -> -145 | -390 -> -396 | win, - | `24.g3 Bxc4 25.Qxc4 Bxd6 26.Rd1 Ke7 27.f5 Bxe5` |
| 95 | sf2000 | 34.Rxd4 | exchange | balanced | 280 | 125 -> 67 | 242 -> 35 | win, - | `34.Rxd4 exd4 35.f4 Nxf4 36.e5 Be7 37.Qg3 Ng6` |

**aggro_old**

| game | opponent | move | category | context | d4 | e0 -> e1 | SF | result, success | line |
|---|---|---|---|---|---|---|---|---|---|
| 45 | sf1800 | 60.Rd6 | queen | balanced | 720 | -40 -> -16 | 0 -> 0 | win, recouped | `60.Rd6 Rxh4 61.Qxh4 Qxh4 62.gxh4 Rf7 63.Kg3 Re7` |
| 31 | aggro_new | 15...Qxf1+ | queen | balanced | 400 | -83 -> -157 | -147 -> -146 | loss, - | `15...Qxf1+ 16.Kxf1 Na6 17.Nd5 e5 18.g4 Kg7 19.Qc3` |
| 9 | aggro_new | 35...Qc6 | exchange | balanced | 180 | -48 -> -38 | -269 -> -478 | win, recouped | `35...Qc6 36.gxf5 Qxf3 37.Re4 Kg5 38.Rh4 Kxf5 39.Qa5+` |
| 43 | sf2000 | 20.Rd6 | exchange | balanced | 170 | 163 -> 47 | 138 -> 49 | win, mate | `20.Rd6 Bxd6 21.exd6 h3 22.Bg4 Rh7 23.Nc4 Bb7` |
| 86 | sf1800 | 26...Qxc4 | exchange | balanced | 170 | 36 -> -4 | 0 -> 0 | win, recouped | `26...Qxc4 27.Rxc4 Rxe5 28.fxe5 Ne6 29.Ra4 a6 30.Kc2` |
| 95 | sf2000 | 33.Rxf3 | exchange | balanced | 170 | 160 -> 1546 | 0 -> 0 | win, mate | `33.Rxf3 exf3 34.Bd3 f2+ 35.Kxf2 d5 36.Bxg6+ Qxg6` |
| 112 | sf2000 | 20...Rxe4 | exchange | balanced | 180 | 61 -> 16 | -71 -> -83 | win, mate | `20...Rxe4 21.Bxe4 Be5 22.Rac1 Bxf1 23.Rxf1 Bd4 24.a4` |
| 82 | sf1800 | 25...Rxd2 | exchange | balanced | 170 | 13 -> -50 | -367 -> -481 | draw, - | `25...Rxd2 26.Rdxd2 Qh4 27.e4 h5 28.Rd3 c5 29.Bxe6+` |

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
