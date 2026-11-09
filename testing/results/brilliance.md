# Brilliance measurement (2026-11-09)

10+0.1, Threads 1, Hash 16, openings testing/openings/8moves_v3.txt seed 1, opponent Stockfish 16 UCI_Elo 1800; analysis: aggro_arena analyze + Stockfish 400k nodes. 'old' = 5.0 default build (rc3, 40 games from testing/runs/sactest-kamikaze).

* bril-rc5 = 5.1 with Brilliance 75 (sha256 f5b6f261f9eb…), 60 games: +34 =7 -19 (62.5%)
* bril-100 = same binary with Brilliance 100, 60 games: +34 =7 -19 (62.5%)
* head-to-head 5.1 (Brilliance 75) vs 5.0: aggro_new vs aggro_old: +42 =14 -24 (61.3%, Elo +79.5)

## Strict (SF before < +300)

| run | engine | games | piece+ sacrifices | sound (SF) | brilliant-style | per game | games with >= 1 | games with >= 2 |
|---|---|---|---|---|---|---|---|---|
| bril-100 | new | 60 | 40 | 32 | 9 | 0.15 | 9 | 0 |
| bril-100 | old | 40 | 29 | 21 | 5 | 0.12 | 5 | 0 |
| bril-rc5 | new | 60 | 34 | 31 | 8 | 0.13 | 8 | 0 |
| bril-rc5 | old | 40 | 29 | 21 | 5 | 0.12 | 5 | 0 |

## Including already-won positions

| run | engine | games | piece+ sacrifices | sound (SF) | brilliant-style | per game | games with >= 1 | games with >= 2 |
|---|---|---|---|---|---|---|---|---|
| bril-100 | new | 60 | 40 | 32 | 22 | 0.37 | 20 | 2 |
| bril-100 | old | 40 | 29 | 21 | 7 | 0.17 | 7 | 0 |
| bril-rc5 | new | 60 | 34 | 31 | 18 | 0.30 | 16 | 2 |
| bril-rc5 | old | 40 | 29 | 21 | 7 | 0.17 | 7 | 0 |
