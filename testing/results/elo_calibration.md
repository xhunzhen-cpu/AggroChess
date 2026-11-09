# AggroChess Skill Level calibration

Generated 2026-11-09 04:41:28 UTC by `testing/calibrate.py`. Run directory: `/root/aggroclone/AggroChess/testing/runs/calib-rc2`. **Status: complete**.

* Engine under test: `AggroChess Ultimate Sacrificial Edition 5.0.0` (option `Skill Level`; Threads=1, Hash=16, default personality).
  Binary copied from `/root/aggroclone/tools/bin/aggro_use_rc2` at the start of the run, sha256 `90c49ff0e3403197b2a5f595cab4334411999edcc0c021271559f27ee80f789f`.
* Anchors (the only fixed ratings): `Stockfish 16` with UCI_LimitStrength=true, UCI_Elo in {1320, 1500, 1700, 1900, 2100, 2300, 2500}, Threads=1, Hash=16.
* Time control 10+0.1 (main), concurrency 12, openings `8moves_v3.txt` (each opening played twice with colours reversed; no opening reused within the run).
* Main-TC games: 2688 (scout 620, main 2068); target ~140 main-stage games per level.
* Ratings are on the **Stockfish 16 UCI_Elo scale at this time control on this machine** - not FIDE, not CCRL. The 95% CI is the game-pair bootstrap (headline); the Fisher interval is shown for comparison.

## Ratings at 10+0.1

| Skill Level | Estimated Elo | 95% CI (bootstrap) | Fisher 95% | games | W/D/L | games vs anchors | confidence |
|---:|---:|:---:|:---:|---:|:---:|---:|:---|
| 1 | 182 | [-10, 316] | [9, 355] | 170 | 54/7/109 | 20 | extrapolated |
| 2 | 225 | [59, 360] | [56, 394] | 218 | 78/9/131 | 20 | extrapolated |
| 3 | 364 | [212, 475] | [206, 523] | 212 | 96/6/110 | 20 | extrapolated |
| 4 | 558 | [418, 664] | [415, 701] | 184 | 100/7/77 | 20 | extrapolated |
| 5 | 575 | [412, 666] | [442, 707] | 180 | 76/7/97 | 20 | extrapolated |
| 6 | 613 | [471, 700] | [492, 734] | 180 | 66/5/109 | 20 | extrapolated |
| 7 | 745 | [617, 826] | [641, 849] | 250 | 82/6/162 | 90 | extrapolated |
| 8 | 824 | [728, 906] | [727, 921] | 180 | 73/4/103 | 20 | extrapolated |
| 9 | 988 | [901, 1058] | [904, 1071] | 180 | 93/5/82 | 20 | extrapolated |
| 10 | 1023 | [953, 1078] | [954, 1093] | 216 | 77/7/132 | 90 | extrapolated |
| 11 | 1076 | [1015, 1140] | [1013, 1140] | 194 | 65/6/123 | 90 | extrapolated |
| 12 | 1135 | [1070, 1186] | [1071, 1198] | 184 | 53/8/123 | 92 | extrapolated |
| 13 | 1294 | [1245, 1338] | [1237, 1352] | 184 | 80/11/93 | 92 | anchored |
| 14 | 1336 | [1290, 1390] | [1280, 1392] | 184 | 79/11/94 | 92 | anchored |
| 15 | 1366 | [1327, 1418] | [1310, 1422] | 184 | 71/13/100 | 92 | anchored |
| 16 | 1464 | [1410, 1511] | [1407, 1521] | 184 | 69/17/98 | 92 | anchored |
| 17 | 1539 | [1485, 1595] | [1483, 1596] | 184 | 76/14/94 | 92 | anchored |
| 18 | 1565 | [1508, 1620] | [1509, 1621] | 186 | 72/12/102 | 92 | anchored |
| 19 | 1681 | [1629, 1727] | [1625, 1738] | 200 | 73/15/112 | 92 | anchored |
| 20 | 1758 | [1710, 1813] | [1701, 1814] | 200 | 80/22/98 | 92 | anchored |
| 21 | 1845 | [1790, 1906] | [1785, 1904] | 172 | 88/21/63 | 92 | anchored |

Confidence labels:
* `anchored` - played Stockfish anchors directly, estimate inside the anchor range (+-100), at least 100 games.
* `chained` - inside the anchor range but rated only through games against other levels.
* `extrapolated` - estimate more than 100 below the lowest anchor (1320) or above the highest (2500): it rests on the chain of adjacent levels and the logistic model, not on a direct anchor comparison.
* `low-sample` - fewer than 100 games; the number is computed but should not be quoted.
* A CI marked `(Fisher)` replaces the bootstrap where it is not meaningful: fewer than 5 opening pairs, or fewer than 5 distinct resampled values.
* `bound only (0% / 100% score)` - no finite maximum-likelihood estimate exists; the column shows a conservative one-sided 95% bound instead (`< X`: losing every one of those games has probability 5% at rating X, opponents at their estimates). `no finite estimate` - same situation for a group of levels, no bound computed.

### Monotonicity

Every rated level is above the previous one.

## Hybrid TC check: 60+0.6 vs 10+0.1

Levels re-measured at 60+0.6 against their nearest anchors (games at this TC only, anchors fixed at their nominal UCI_Elo; a level with no anchor within the window is chained to the next stronger checked level). Drift = Elo at the check TC - Elo at the main TC; the +- is an approximate 95% half-width (root sum of squares of both CI half-widths; a Fisher interval stands in where the bootstrap is not meaningful, see above).

| Skill Level | Elo @ 10+0.1 | Elo @ 60+0.6 | 95% CI @ 60+0.6 | drift | games | W/D/L | opponents | confidence |
|---:|---:|---:|:---:|---:|---:|:---:|:---|:---|
| 6 | 613 | 101 | [-inf, 384] | -511 (CI unbounded) | 60 | 1/0/59 | L11 | low-sample, chained via L11 |
| 11 | 1076 | 809 | [-inf, 968] | -268 (CI unbounded) | 120 | 62/0/58 | sf1320 | anchored |
| 16 | 1464 | 1295 | [1182, 1385] | -169 +- 113 | 60 | 19/4/37 | sf1320, sf1500 | low-sample |
| 19 | 1681 | 1631 | [1530, 1723] | -50 +- 109 | 60 | 31/3/26 | sf1500, sf1700 | low-sample |
| 21 | 1845 | 1730 | [1619, 1813] | -115 +- 113 | 60 | 24/1/35 | sf1700, sf1900 | low-sample |

## Pairings at 10+0.1 (stages 1 + 2 pooled)

Per-pairing W/D/L from A's view, Elo difference with 95% CI (trinomial normal approximation), LOS, and the difference implied by the joint fit.

| A | B | games | +/=/- | score | Elo diff (95% CI) | LOS | joint-fit diff |
|:---|:---|---:|:---:|---:|:---:|---:|---:|
| aggro_L01 | aggro_L02 | 104 | +42 =6 -56 | 43.3% | -47 [-115, +17] | 7.9% | -43 |
| aggro_L01 | aggro_L03 | 46 | +12 =1 -33 | 27.2% | -171 [-308, -72] | 0.1% | -182 |
| aggro_L01 | sf1320 | 10 | +0 =0 -10 | 0.0% | n/a [-inf, -166] | 0.1% | -1138 |
| aggro_L01 | sf1500 | 10 | +0 =0 -10 | 0.0% | n/a [-inf, -166] | 0.1% | -1318 |
| aggro_L02 | aggro_L03 | 66 | +18 =3 -45 | 29.5% | -151 [-254, -69] | 0.0% | -139 |
| aggro_L02 | aggro_L04 | 28 | +4 =0 -24 | 14.3% | -311 [-749, -171] | 0.0% | -333 |
| aggro_L02 | sf1320 | 10 | +0 =0 -10 | 0.0% | n/a [-inf, -166] | 0.1% | -1095 |
| aggro_L02 | sf1500 | 10 | +0 =0 -10 | 0.0% | n/a [-inf, -166] | 0.1% | -1275 |
| aggro_L03 | aggro_L04 | 56 | +14 =2 -40 | 26.8% | -175 [-295, -84] | 0.0% | -194 |
| aggro_L03 | aggro_L05 | 24 | +4 =0 -20 | 16.7% | -280 [-699, -134] | 0.1% | -210 |
| aggro_L03 | sf1320 | 10 | +0 =0 -10 | 0.0% | n/a [-inf, -166] | 0.1% | -956 |
| aggro_L03 | sf1500 | 10 | +0 =0 -10 | 0.0% | n/a [-inf, -166] | 0.1% | -1136 |
| aggro_L04 | aggro_L05 | 56 | +30 =4 -22 | 57.1% | +50 [-37, +144] | 86.6% | -17 |
| aggro_L04 | aggro_L06 | 24 | +6 =1 -17 | 27.1% | -172 [-386, -39] | 1.1% | -55 |
| aggro_L04 | sf1320 | 10 | +0 =0 -10 | 0.0% | n/a [-inf, -166] | 0.1% | -762 |
| aggro_L04 | sf1500 | 10 | +0 =0 -10 | 0.0% | n/a [-inf, -166] | 0.1% | -942 |
| aggro_L05 | aggro_L06 | 56 | +27 =2 -27 | 50.0% | +0 [-91, +91] | 50.0% | -38 |
| aggro_L05 | aggro_L07 | 24 | +7 =1 -16 | 31.2% | -137 [-328, -5] | 3.0% | -170 |
| aggro_L05 | sf1320 | 10 | +0 =0 -10 | 0.0% | n/a [-inf, -166] | 0.1% | -745 |
| aggro_L05 | sf1500 | 10 | +0 =0 -10 | 0.0% | n/a [-inf, -166] | 0.1% | -925 |
| aggro_L06 | aggro_L07 | 56 | +19 =2 -35 | 35.7% | -102 [-206, -14] | 1.5% | -132 |
| aggro_L06 | aggro_L08 | 24 | +3 =0 -21 | 12.5% | -338 [-inf, -184] | 0.0% | -211 |
| aggro_L06 | sf1320 | 10 | +0 =0 -10 | 0.0% | n/a [-inf, -166] | 0.1% | -707 |
| aggro_L06 | sf1500 | 10 | +0 =0 -10 | 0.0% | n/a [-inf, -166] | 0.1% | -887 |
| aggro_L07 | aggro_L08 | 56 | +25 =3 -28 | 47.3% | -19 [-111, +71] | 34.0% | -79 |
| aggro_L07 | aggro_L09 | 24 | +2 =0 -22 | 8.3% | -417 [-inf, -248] | 0.0% | -243 |
| aggro_L07 | sf1320 | 80 | +4 =0 -76 | 5.0% | -512 [-1059, -386] | 0.0% | -575 |
| aggro_L07 | sf1500 | 10 | +0 =0 -10 | 0.0% | n/a [-inf, -166] | 0.1% | -755 |
| aggro_L08 | aggro_L09 | 56 | +18 =1 -37 | 33.0% | -123 [-232, -33] | 0.5% | -164 |
| aggro_L08 | aggro_L10 | 24 | +6 =0 -18 | 25.0% | -191 [-432, -54] | 0.7% | -199 |
| aggro_L08 | sf1320 | 10 | +0 =0 -10 | 0.0% | n/a [-inf, -166] | 0.1% | -496 |
| aggro_L08 | sf1500 | 10 | +0 =0 -10 | 0.0% | n/a [-inf, -166] | 0.1% | -676 |
| aggro_L09 | aggro_L10 | 56 | +21 =4 -31 | 41.1% | -63 [-158, +24] | 8.3% | -35 |
| aggro_L09 | aggro_L11 | 24 | +11 =0 -13 | 45.8% | -29 [-183, +113] | 34.2% | -89 |
| aggro_L09 | sf1320 | 10 | +1 =0 -9 | 10.0% | -382 [-inf, -159] | 0.6% | -332 |
| aggro_L09 | sf1500 | 10 | +1 =0 -9 | 10.0% | -382 [-inf, -159] | 0.6% | -512 |
| aggro_L10 | aggro_L11 | 34 | +11 =2 -21 | 35.3% | -105 [-243, +6] | 3.9% | -53 |
| aggro_L10 | aggro_L12 | 12 | +3 =0 -9 | 25.0% | -191 [-919, -3] | 4.2% | -111 |
| aggro_L10 | sf1320 | 80 | +12 =1 -67 | 15.6% | -293 [-430, -205] | 0.0% | -297 |
| aggro_L10 | sf1500 | 10 | +2 =0 -8 | 20.0% | -241 [-inf, -36] | 2.9% | -477 |
| aggro_L11 | aggro_L12 | 34 | +11 =2 -21 | 35.3% | -105 [-243, +6] | 3.9% | -58 |
| aggro_L11 | aggro_L13 | 12 | +2 =1 -9 | 20.8% | -232 [-inf, -54] | 1.7% | -218 |
| aggro_L11 | sf1320 | 80 | +18 =1 -61 | 23.1% | -209 [-316, -129] | 0.0% | -244 |
| aggro_L11 | sf1500 | 10 | +0 =0 -10 | 0.0% | n/a [-inf, -166] | 0.1% | -424 |
| aggro_L12 | aggro_L13 | 34 | +7 =2 -25 | 23.5% | -205 [-384, -91] | 0.1% | -160 |
| aggro_L12 | aggro_L14 | 12 | +1 =2 -9 | 16.7% | -280 [-inf, -113] | 0.6% | -201 |
| aggro_L12 | sf1320 | 46 | +8 =1 -37 | 18.5% | -258 [-437, -151] | 0.0% | -185 |
| aggro_L12 | sf1500 | 46 | +7 =1 -38 | 16.3% | -284 [-483, -175] | 0.0% | -365 |
| aggro_L13 | aggro_L14 | 34 | +16 =3 -15 | 51.5% | +10 [-104, +127] | 57.1% | -42 |
| aggro_L13 | aggro_L15 | 12 | +5 =1 -6 | 45.8% | -29 [-254, +171] | 38.2% | -72 |
| aggro_L13 | sf1320 | 46 | +16 =3 -27 | 38.0% | -85 [-195, +11] | 4.7% | -26 |
| aggro_L13 | sf1500 | 46 | +9 =1 -36 | 20.7% | -234 [-399, -130] | 0.0% | -206 |
| aggro_L14 | aggro_L15 | 34 | +14 =2 -18 | 44.1% | -41 [-165, +73] | 24.0% | -30 |
| aggro_L14 | aggro_L16 | 12 | +7 =0 -5 | 58.3% | +58 [-144, +319] | 71.8% | -128 |
| aggro_L14 | sf1320 | 46 | +22 =1 -23 | 48.9% | -8 [-110, +94] | 44.1% | +16 |
| aggro_L14 | sf1500 | 46 | +12 =3 -31 | 29.3% | -153 [-279, -56] | 0.2% | -164 |
| aggro_L15 | aggro_L16 | 34 | +13 =1 -20 | 39.7% | -73 [-205, +41] | 11.2% | -98 |
| aggro_L15 | aggro_L17 | 12 | +1 =2 -9 | 16.7% | -280 [-inf, -113] | 0.6% | -173 |
| aggro_L15 | sf1320 | 36 | +18 =4 -14 | 55.6% | +39 [-69, +154] | 76.0% | +46 |
| aggro_L15 | sf1500 | 46 | +12 =3 -31 | 29.3% | -153 [-279, -56] | 0.2% | -134 |
| aggro_L15 | sf1700 | 10 | +3 =0 -7 | 30.0% | -147 [-716, +59] | 10.3% | -334 |
| aggro_L16 | aggro_L17 | 34 | +11 =6 -17 | 41.2% | -62 [-180, +43] | 12.8% | -75 |
| aggro_L16 | aggro_L18 | 12 | +1 =1 -10 | 12.5% | -338 [-inf, -153] | 0.3% | -101 |
| aggro_L16 | sf1500 | 46 | +19 =4 -23 | 45.7% | -30 [-132, +66] | 26.9% | -36 |
| aggro_L16 | sf1700 | 46 | +13 =5 -28 | 33.7% | -118 [-231, -24] | 1.0% | -236 |
| aggro_L17 | aggro_L18 | 34 | +18 =2 -14 | 55.9% | +41 [-73, +165] | 76.0% | -26 |
| aggro_L17 | aggro_L19 | 12 | +2 =1 -9 | 20.8% | -232 [-inf, -54] | 1.7% | -142 |
| aggro_L17 | sf1500 | 36 | +13 =2 -21 | 38.9% | -79 [-206, +30] | 8.5% | +39 |
| aggro_L17 | sf1700 | 46 | +14 =1 -31 | 31.5% | -135 [-260, -36] | 0.6% | -161 |
| aggro_L17 | sf1900 | 10 | +3 =0 -7 | 30.0% | -147 [-716, +59] | 10.3% | -361 |
| aggro_L18 | aggro_L19 | 34 | +8 =2 -24 | 26.5% | -177 [-343, -65] | 0.2% | -117 |
| aggro_L18 | aggro_L20 | 14 | +5 =1 -8 | 39.3% | -76 [-306, +99] | 20.3% | -193 |
| aggro_L18 | sf1500 | 36 | +19 =2 -15 | 55.6% | +39 [-72, +158] | 75.4% | +65 |
| aggro_L18 | sf1700 | 46 | +14 =3 -29 | 33.7% | -118 [-235, -22] | 1.1% | -135 |
| aggro_L18 | sf1900 | 10 | +2 =1 -7 | 25.0% | -191 [-1527, -0] | 4.8% | -335 |
| aggro_L19 | aggro_L20 | 38 | +10 =3 -25 | 30.3% | -145 [-283, -41] | 0.6% | -76 |
| aggro_L19 | aggro_L21 | 24 | +4 =2 -18 | 20.8% | -232 [-489, -100] | 0.1% | -163 |
| aggro_L19 | sf1700 | 36 | +14 =3 -19 | 43.1% | -49 [-168, +60] | 19.2% | -19 |
| aggro_L19 | sf1900 | 46 | +12 =3 -31 | 29.3% | -153 [-279, -56] | 0.2% | -219 |
| aggro_L19 | sf2100 | 10 | +0 =1 -9 | 5.0% | -512 [-inf, -311] | 0.1% | -419 |
| aggro_L20 | aggro_L21 | 56 | +14 =11 -31 | 34.8% | -109 [-202, -29] | 0.6% | -87 |
| aggro_L20 | sf1700 | 36 | +11 =5 -20 | 37.5% | -89 [-211, +15] | 5.3% | +58 |
| aggro_L20 | sf1900 | 46 | +16 =2 -28 | 37.0% | -93 [-207, +4] | 3.5% | -142 |
| aggro_L20 | sf2100 | 10 | +6 =0 -4 | 60.0% | +70 [-150, +389] | 73.6% | -342 |
| aggro_L21 | sf1700 | 36 | +22 =5 -9 | 68.1% | +131 [+29, +264] | 99.0% | +145 |
| aggro_L21 | sf1900 | 46 | +16 =3 -27 | 38.0% | -85 [-195, +11] | 4.7% | -55 |
| aggro_L21 | sf2100 | 10 | +1 =0 -9 | 10.0% | -382 [-inf, -159] | 0.6% | -255 |

Terminations (main TC): checkmate 2552, fifty 33, insufficient 29, stalemate 8, threefold 66

## Schedule

* Stage 1 (scout): 620/620 games played in 3 batch(es), tc 10+0.1.
* Stage 2 (main): 2068/2068 games played in 10 batch(es), tc 10+0.1.

| Skill Level | prior Elo | scout estimate | stage-2 anchors (games each) | stage-2 chain partners (games) |
|---:|---:|---:|:---|:---|
| 1 | 400 | 718 | none in window | L2 (94), L3 (46) |
| 2 | 485 | 747 | none in window | L1 (94), L3 (56), L4 (28) |
| 3 | 570 | 810 | none in window | L1 (46), L2 (56), L4 (46), L5 (24) |
| 4 | 655 | 924 | none in window | L2 (28), L3 (46), L5 (46), L6 (24) |
| 5 | 740 | 766 | none in window | L3 (24), L4 (46), L6 (46), L7 (24) |
| 6 | 825 | 813 | none in window | L4 (24), L5 (46), L7 (46), L8 (24) |
| 7 | 910 | 1076 | sf1320 (70) | L5 (24), L6 (46), L8 (46), L9 (24) |
| 8 | 995 | 910 | none in window | L6 (24), L7 (46), L9 (46), L10 (24) |
| 9 | 1080 | 966 | none in window | L7 (24), L8 (46), L10 (46), L11 (24) |
| 10 | 1165 | 1072 | sf1320 (70) | L8 (24), L9 (46), L11 (24), L12 (12) |
| 11 | 1250 | 1079 | sf1320 (70) | L9 (24), L10 (24), L12 (24), L13 (12) |
| 12 | 1335 | 1155 | sf1320, sf1500 (36) | L10 (12), L11 (24), L13 (24), L14 (12) |
| 13 | 1420 | 1377 | sf1320, sf1500 (36) | L11 (12), L12 (24), L14 (24), L15 (12) |
| 14 | 1505 | 1415 | sf1320, sf1500 (36) | L12 (12), L13 (24), L15 (24), L16 (12) |
| 15 | 1590 | 1433 | sf1320, sf1500 (36) | L13 (12), L14 (24), L16 (24), L17 (12) |
| 16 | 1675 | 1578 | sf1500, sf1700 (36) | L14 (12), L15 (24), L17 (24), L18 (12) |
| 17 | 1760 | 1643 | sf1500, sf1700 (36) | L15 (12), L16 (24), L18 (24), L19 (12) |
| 18 | 1845 | 1623 | sf1500, sf1700 (36) | L16 (12), L17 (24), L19 (24), L20 (14) |
| 19 | 1930 | 1708 | sf1700, sf1900 (36) | L17 (12), L18 (24), L20 (28), L21 (24) |
| 20 | 2015 | 1885 | sf1700, sf1900 (36) | L18 (14), L19 (28), L21 (46) |
| 21 | 2100 | 1844 | sf1700, sf1900 (36) | L19 (24), L20 (46) |

## Environment

* date: `2026-11-08 23:52:01 UTC`
* host: `kv1`
* cpu_model: `Intel(R) Core(TM) Ultra 5 125H`
* nproc: `18`
* memory: `{'MemTotal': '13143104 kB', 'MemAvailable': '9068884 kB'}`
* loadavg_at_start: `[3.30322265625, 4.6875, 2.4130859375]`
* os: `Linux-6.8.0-147-generic-x86_64-with-glibc2.39`
* python: `3.12.3`
* engine_id: `AggroChess Ultimate Sacrificial Edition 5.0.0`
* engine_source: `/root/aggroclone/tools/bin/aggro_use_rc2`
* engine_sha256: `90c49ff0e3403197b2a5f595cab4334411999edcc0c021271559f27ee80f789f`
* engine_files: `{'ph-gambitbook.bin': 'f2dddc37343b155624fddb08a7743965e806b717ac87985f887643e126119f14'}`
* stockfish_id: `Stockfish 16`
* stockfish: `/usr/games/stockfish`
* stockfish_sha256: `00628bd9c9855c1b7ff93d7f8d51b413586cdc6336fe637f9a14a61531a05aca`
* arena_sha256: `53adacb9b885b689528b8dbd73e96daced94654d16988269b7182f70889fef1a`
* openings_sha256: `46ac9df7ad5f72f18d50530553265e41e4fd46756ca38f33c4601a28f166405f`
* aggrochess_git_head: `bbdb4a50f5a81b65791c4a399f828c268bdcf6de`
* aggrochess_git_branch: `ultimate-sacrificial`
* aggrochess_src_dirty_files: `8`
* tc: `10+0.1`
* tc_check: `60+0.6`
* games_per_level: `140`
* scout_games: `10`
* concurrency: `12`

## Method

* Joint maximum-likelihood logistic Elo (draws = half points) over all main-TC games; Stockfish anchors fixed at their nominal UCI_Elo. Prior: 0.01 virtual draws per pair of players that met (numerical safety only; levels whose plain MLE is infinite get a bound instead of a number). The scout's scheduling-only fit used 1 virtual draws per pair (this inflates the scout estimates of levels far below the lowest anchor, so some of them got anchor games although their true rating is outside the window; the final ratings are not affected). See `testing/elo.py`.
* 95% CI: game-pair bootstrap (200 resamples of opening pairs within each pairing, refit each time, 2.5/97.5 percentiles). Fisher: inverse observed information. In simulated 21-level calibrations the bootstrap interval covered the true rating about 93% of the time (91-93% for chained levels far below the anchors, where the truth lies above the interval 2-3x more often than below); the Fisher interval is conservative (96-99.7%).
* Scheduling: prior map 1:400, 21:2100 (linear), scout 10 games vs the 2 nearest anchors + next level; main stage ~140 games per level, 50% vs up to 2 anchors within +-350 of the scout estimate, the rest vs levels up to 2 steps away in the level list.
