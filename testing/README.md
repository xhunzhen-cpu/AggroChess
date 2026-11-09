# AggroChess testing harness

Everything here is self-contained: the only Rust dependency is `cozy-chess 0.3`
(the same crate the engine uses) and any Python tooling is stdlib-only. No
fastchess / cutechess / python-chess.

```
testing/
  arena/                 Rust crate, binary `aggro_arena` (match runner, opening converter, UCI tester)
  openings/8moves_v3.txt opening suite: 34,700 openings x 16 plies, UCI moves from the start position
  runs/                  scratch output of generated matches (git-ignored)
```

## Building

```sh
cd testing/arena
cargo build --release --offline      # --offline: cozy-chess is in the local cargo cache
cargo test --release --offline       # unit tests (SAN vs cozy-chess cross-check, JSON, rules, config)
```

The binary is `testing/arena/target/release/aggro_arena` (`testing/arena/target` is git-ignored).

```
aggro_arena openings <in.pgn> <out.txt> [--plies N]
aggro_arena match <config>
aggro_arena ucitest <engine-path> [--option "Name=Value"]... [--cwd DIR] [--arg ARG]...
aggro_arena summary <games.jsonl>...
```

---

## `openings` — PGN opening suite to UCI lines

```sh
aggro_arena openings /root/aggroclone/tools/books_dl/8moves_v3.pgn testing/openings/8moves_v3.txt [--plies N]
```

Reads PGN movetext (strips tags, `{}` and `;` comments, `( )` variations, NAGs, move
numbers, results), replays every SAN move from the start position and writes one opening
per line as space-separated UCI moves (castling as `e1g1`), truncated to `N` plies
(default: all). SAN matching formats every legal move as SAN and compares (piece letter,
file/rank/both disambiguation, `x`, `=Q`, `O-O`; `+ # ! ?` ignored; `0-0` accepted); a
structural fallback accepts non-canonical SAN such as `Ngf3` when not needed. Games that
fail to parse are skipped and counted.

`testing/openings/8moves_v3.txt` was produced this way: 34,700 games read, 34,700 openings
written, 0 skipped, all 16 plies.

## `match` — engine tournaments

```sh
aggro_arena match path/to/match.cfg
```

### Config format

Line based; `#` starts a comment (at line start or after whitespace); blank lines ignored.
Relative paths (openings, pgn, jsonl, cwd, and engine paths containing a `/`) are
resolved against the **directory of the config file**. A bare engine command without `/`
is looked up in `PATH`.

| directive | meaning | default |
|---|---|---|
| `engine <name> <path> [args...]` | declare an engine (args are whitespace-separated) | — |
| `option <name> <UCI option name> = <value>` | sent as `setoption name … value …` after `uciok`, in file order; the option name may contain spaces | — |
| `cwd <name> <dir>` | working directory of that engine process (e.g. so AggroChess finds `ph-gambitbook.bin`) | arena's cwd |
| `tc <base>+<inc>` | Fischer clock in seconds, e.g. `10+0.1`, `60+0.6`, `3+0.05` | required |
| `tc nodes=N` / `tc movetime=MS` / `tc depth=D` | fixed per-move limits (`go nodes N` / `go movetime MS` / `go depth D`) | |
| `concurrency <N>` | parallel games (worker threads) | 1 |
| `openings <file>` | file from `openings`; if absent every game starts from the start position | startpos |
| `seed <u64>` | opening selection seed | 1 |
| `maxplies <N>` | adjudicate a draw after N plies (opening plies included) | 600 |
| `timemargin <ms>` | grace before a clock overrun is a time forfeit | 100 |
| `resign <cp> <moves>` | resign adjudication (see below) | off |
| `pgn <file>` | PGN output (truncated at start) | none |
| `jsonl <file>` | JSONL output (truncated at start); summary goes to `<file>.summary.json` | none |
| `event <text>` | PGN `Event` tag | `aggro_arena match` |
| `poolsize <N>` | idle engine processes a worker may keep *beyond* the two its next game uses (0..64) | 0 |
| `pair <engA> <engB> <games>` | schedule games (rounded up to even) between A and B | at least one |

Example (a gauntlet of the original engine vs two Stockfish anchors):

```
engine aggro /root/aggroclone/engines/aggro_original
option aggro Threads = 1
cwd    aggro /root/aggroclone/AggroChess
engine sf1800 /usr/games/stockfish
option sf1800 Threads = 1
option sf1800 Hash = 16
option sf1800 UCI_LimitStrength = true
option sf1800 UCI_Elo = 1800
engine sf2200 /usr/games/stockfish
option sf2200 Threads = 1
option sf2200 Hash = 16
option sf2200 UCI_LimitStrength = true
option sf2200 UCI_Elo = 2200
tc 10+0.1
concurrency 12
openings /root/aggroclone/AggroChess/testing/openings/8moves_v3.txt
seed 1
pgn games.pgn
jsonl games.jsonl
pair aggro sf1800 100
pair aggro sf2200 100
```

Engines in matches should run with `Threads=1` (AggroChess defaults to 8!) and Stockfish
with `Hash=16`. At start-up every engine is launched once to check the handshake; a
warning is printed for any configured option the engine does not advertise.

### Scheduling

* Each `pair` with G games becomes G/2 **game pairs**: one opening, played twice in a row
  with colours reversed (game 1: A white, game 2: B white).
* Openings: the whole file is shuffled with the seed (SplitMix64 + Fisher–Yates) and taken
  sequentially — pair line 1 gets the first G1/2 openings, pair line 2 the next G2/2, … so
  different pairings never share openings (the list wraps around with a warning only if
  the file is exhausted). Same config + seed ⇒ same openings and colours.
* All games of all pairs go into one queue, interleaved pair line by pair line, processed
  by `concurrency` worker threads. Game ids (`Round` tag) are queue positions; output
  files are written in completion order.

### Engine handling

* Spawned with piped stdin/stdout (stderr → /dev/null), in their own process group, with
  `PR_SET_PDEATHSIG=SIGKILL` (children die even if the arena is `kill -9`ed). A reader
  thread per engine timestamps every output line and feeds a **bounded** channel (4096
  lines; when it is full the reader, and then the engine, block on the pipe); every read
  uses a timeout, and every wait checks its deadline and the stop flag on each line. An
  engine that floods `info` lines without ever sending `bestmove` therefore still loses on
  time at the normal deadline, memory stays flat, and Ctrl-C works while it floods. A
  handshake/`isready` wait gives up after 100,000 lines without the awaited reply.
* Killing an engine (time-out, restart, shutdown) sends SIGKILL to its **whole process
  group**, and the group is also cleared right after an engine exits by itself, so helpers
  of an engine started through a wrapper script (`sh` without `exec`) cannot survive as
  orphans holding the pipe.
* Handshake: `uci` → `uciok` (10 s), options, `isready` → `readyok` (30 s).
* Workers keep engine processes between games and send `ucinewgame` + `isready` before each
  game. Before a game, a worker shuts down every idle engine that game does not use (keeping
  at most `poolsize` others, default 0), so it never holds more than 2 + `poolsize` engine
  processes — with many engine names in one config (calibration batches: many levels plus
  several Stockfish anchors at ~150 MB each) idle processes no longer pile up. An engine that crashed, timed out
  or did not answer `isready` is killed and a fresh process is started for the next game
  (3 attempts; if it still cannot start, the game is recorded as a `crash` loss).
* First Ctrl-C / SIGTERM / SIGHUP: running games are abandoned (not recorded), engines
  are shut down, the summary is written, exit code 130. A second signal exits at once.
* Output never panics on a closed stdout/stderr (e.g. `aggro_arena match x.cfg | tee log`
  and Ctrl-C, which also kills `tee`, or a driver script that died): the arena then stops as
  if interrupted — finalises the summary JSON (`interrupted: true`) and exits 130.

### Game rules

* Each move: `position startpos moves <all moves>` then
  `go wtime W btime B winc I binc I` (ms) or the fixed-limit `go`. Info lines are parsed
  for the last `score cp|mate` and `depth` (only `multipv 1`), and `info string book move`
  marks a book move.
* **Clock**: elapsed = time from writing `go` to reading `bestmove`. `remaining -= elapsed`;
  if `remaining < -timemargin` the side loses on time (the move is not played); then
  `remaining += inc`. No `bestmove` within `remaining + timemargin + 2000 ms` ⇒ the engine
  is killed and loses on time. Fixed nodes/depth/movetime TCs have a hard 60 s per-move
  timeout (movetime: max(60 s, movetime + 10 s)). A time forfeit is a **draw** if the
  opponent has only a bare king or king + one minor.
* `bestmove` is validated with cozy-chess; an illegal move, or `0000`/`(none)` while legal
  moves exist, loses (`illegal`, offending string in `detail`). Only standard UCI notation
  is accepted: Chess960-style castling (`e1h1`, king takes own rook) is illegal in a
  standard game, as in standard GUIs (the `detail` says so). A dead process loses
  (`crash`).
* Termination checks after every move, in order: checkmate; stalemate; insufficient
  material (KvK, K+minor vs K, bishops-only all on one square colour); 50-move rule
  (halfmove clock ≥ 100); threefold repetition (positions since the last irreversible
  move; en-passant square only counts if an e.p. capture is legal); `maxplies` (draw);
  resign adjudication.
* **Resign adjudication** (`resign <cp> <moves>`): after `moves` consecutive moves *each*
  in which both engines' own scores agree on the same winner by at least `cp`
  (winner reports ≥ cp, loser ≤ −cp; mate scores count), the game is scored for that
  winner. Any move without a score (book) resets the count.

### Outputs

**stdout**: one line per finished game, e.g.

```
game 7/12 (#10): sf1800 vs sf1320 1-0 checkmate | sf1320 vs sf1800: +0 =0 -7 (0.0%, Elo n/a)
    -> <detail, printed for time/illegal/crash>
```

and a final report per pair line: W/D/L from engA's view, score, draw rate, Elo
difference with a 95% CI (trinomial normal approximation; when every game had the same
result its variance is 0, so the Wilson score interval is used instead; a bound is ±inf
only when the score interval really leaves (0,1)), LOS, white/black wins, average plies, termination counts, and per
engine: time forfeits / illegal moves / crashes plus average and maximum move time.

**JSONL** (one object per game; `game::GameRecord` / `game::read_jsonl` in the crate can read
it back):

| field | |
|---|---|
| `id` | game id = queue position (1-based) |
| `white`, `black` | engine names from the config |
| `pairing` | index of the `pair` line (0-based) |
| `game_pair` | index of the opening pair within the pairing; both colour-reversed games share (`pairing`, `game_pair`) — use this for pair bootstrap |
| `opening_index` | 0-based line in the openings file, `-1` = start position |
| `opening_plies` | number of leading plies taken from the opening file |
| `moves` | UCI moves from the start position (castling `e1g1`) |
| `san` | the same moves in SAN (with `+`/`#`) |
| `scores` | per ply: the mover's last reported score in cp from the **mover's** view; mate in n = ±(100000 − n); `null` for opening plies, book moves without score, or no info |
| `depths` | per ply: depth of that score (or `null`) |
| `times_ms` | per ply: wall time from `go` to `bestmove` (0 for opening plies) |
| `book` | per ply: engine printed `info string book move` |
| `result` | `1-0`, `0-1`, `1/2-1/2` |
| `termination` | `checkmate`, `stalemate`, `insufficient`, `fifty`, `threefold`, `maxplies`, `resign`, `time`, `illegal`, `crash` |
| `detail` | human-readable reason (e.g. `white (slow) lost on time: used 3092 ms, clock -1625 ms (bestmove e1g1 not played)`) |
| `tc` | `10+0.1`, `nodes=20000`, `movetime=500`, `depth=8` |
| `forfeit_side` | `white`/`black` for time/illegal/crash terminations, else `null` |
| `clock_end_ms` | `[white, black]` remaining clock at the end (clock TCs), else `null` |
| `date` | UTC time the game finished |

**PGN**: tags `Event`, `Site` (host name), `Date`, `Round` (= id), `White`, `Black`,
`Result`, `Termination` (custom: same values as JSONL), `TimeControl` (`10+0.1`, or `-` for
fixed limits plus `ArenaTC "nodes=5000"`), `PlyCount`, `OpeningIndex`, `OpeningPlies`.
Movetext is SAN with move numbers; each engine move carries `{+0.35/12 0.512s}`
(score from the mover's view / depth, time), mates `{+M3/18 0.020s}`, book moves `{book}`;
opening-file moves carry no comment. The `detail` text is a final comment before the
result.

**Summary JSON** (`<jsonl>.summary.json`, rewritten atomically after every game): config,
start/update time, duration, tc, concurrency, `poolsize`, seed, openings, `games_total`,
`games_played`, `complete`, `interrupted`, engine ids/paths/options, and per pair line:
`engine_a`, `engine_b`, `games_planned`, `games`, `wins`/`draws`/`losses` (engine A's view),
`score`, `draw_ratio`, `elo`, `elo_lo`, `elo_hi` (95%, `null` when unbounded), `score_se`,
`los`, `white_wins`, `black_wins`, `avg_plies`, `terminations`, `forfeits`
(`{engine: {time, illegal, crash}}`), `timing` (`{engine: {moves, avg_ms, max_ms}}`).

Exit codes: 0 done, 1 runtime/output error, 2 config/usage error, 130 interrupted.

## `summary` — re-summarise JSONL files

```sh
aggro_arena summary runs/a/games.jsonl runs/b/games.jsonl
```

Merges all games per unordered engine pair (A–B and B–A together) and prints the same
report as the end of `match`.

## `ucitest` — UCI conformance check

```sh
aggro_arena ucitest /usr/games/stockfish --option "Threads=1" --option "Hash=16"
aggro_arena ucitest /root/aggroclone/engines/aggro_original --option "Threads=1" --cwd /root/aggroclone/AggroChess
```

Prints `PASS` / `FAIL` / `WARN` per check and exits 1 if anything FAILed. Every `bestmove`
is checked for legality with cozy-chess (a `ponder` move is checked too, WARN only) and must
be in standard UCI notation (castling `e1g1`; `e1h1` is a FAIL). A relative engine path is
resolved against the directory `ucitest` is run from, not against `--cwd`.
`--option` values are applied after the handshake and re-applied after every restart.

Checks: `uci` → `id name`, `uciok` (10 s), advertised options listed; `isready` →
`readyok`; every advertised option: spin min / max / default / below-min / above-max,
check true/false, combo every `var` plus an invalid one, button press, string default
plus a missing path (`/nonexistent/...`) — after each value the engine must still answer
`isready`; the option is then restored (to the `--option` value if given). Resource
options are capped so the test is safe on a shared machine: names containing
`thread` → max 4, `hash`/`memory`/`cache`/`table size` → max 64, and for those no above-max
value is sent. A string option that makes the engine exit when pointed at a missing file
is only a WARN (Stockfish exits by design for `Debug Log File`). The engine is restarted
after the option phase so the search checks run on a clean process.
Then: `ucinewgame`; `position startpos` + `go depth 6`; `position fen … [moves …]` +
`go movetime 200` for castling (castling moves in the move list), en passant (available
and played), promotion (incl. under-promotion in the move list), in-check and a
black-to-move middlegame (WARN if movetime is overrun by > 300 ms); clock handling
(`wtime 3000 winc 30`; black to move with `btime 3000` but `wtime 100000` — catches engines
reading the wrong clock; `wtime 300` without increment; `movestogo 1`) — FAIL if the
engine uses more than its remaining time, WARN above half of it; `go infinite`
(FAIL if `bestmove` arrives before `stop`) + `stop` after 500 ms → `bestmove` within 1 s;
`stop` while idle (WARN on a spurious `bestmove`); checkmated and stalemated side to move
→ `bestmove 0000`/`(none)` and still ready; a position with exactly one legal move;
50 × `ucinewgame`/`position`/`go nodes 2000` from pseudo-random positions; garbage input
lines followed by a normal search; `go` immediately followed by `quit` → exit within 2 s.

## For developers

The crate is organised so further subcommands (e.g. `analyze`) can be added in
`src/main.rs`: `engine.rs` (process wrapper, `wait_bestmove`, info parsing), `game.rs`
(rules, clocks, `GameRecord`, `read_jsonl`), `san.rs`, `pgn.rs`, `json.rs` (writer + reader),
`stats.rs` (W/D/L → Elo), `tournament.rs`, `ucitest.rs`, `openings.rs`, `sys.rs`
(signals, child hardening, dates).

## Elo calibration — `elo.py`, `calibrate.py`

Both are stdlib-only Python 3. Self-tests (no games are played):

```sh
python3 -m unittest discover -s testing -p 'test_*.py'     # from AggroChess/: test_elo.py + test_calibrate.py
```

### `elo.py` — joint maximum-likelihood ratings

```sh
python3 testing/elo.py runs/x/stage1_scout runs/x/stage2_main/b01.jsonl ... \
    [--anchor sf1320=1320]... [--anchor-regex 'sf(\d+)'] [--prior-draws 0.01] \
    [--bootstrap 200] [--seed 1] [--pairs] [--json out.json]
```

Inputs are arena JSONL files (a directory means every `*.jsonl` directly inside it).

* **Model**: logistic Elo, draws = ½ point; per game `s·log p + (1−s)·log(1−p)`,
  `p = 1/(1+10^(−(Ri−Rj)/400))`. Games are aggregated per player pair and the joint MLE is
  found with Newton–Raphson (step clamp + line search). Any number of players can be
  **anchored** at fixed ratings; without anchors the ratings are centred on 0. Players with no
  path of games to an anchor are reported as `unconnected` (no rating).
* **Prior / 0 % and 100 % scores**: `--prior-draws` virtual draws (default **0.01**) are added
  to every pair of players that met, only to keep the optimisation finite. One full virtual
  draw per pair (the textbook choice) was measured to be harmful for the calibration design:
  on a simulated 21-level run, levels far below the anchors (rated through the chain, but also
  playing a few hopeless scout games against the anchors) came out **+130 Elo too high** with
  4/20 CI coverage, because the virtual draws against far-away opponents keep pulling while
  0 % results carry almost no information. With 0–0.1 virtual draws the same simulation was
  unbiased within noise and had 18–20/20 coverage.
* **No fake numbers**: a player whose plain MLE is infinite — it (or a group it belongs to)
  never scored against, or never conceded to, anyone connecting it to an anchor — gets
  **no point estimate**. For an individual 0 % / 100 % score a conservative one-sided 95 %
  bound is printed instead (`< 796`: at that rating, losing every one of those games has
  probability 5 %, opponents at their estimates). A group without a finite MLE is reported
  as `no finite estimate`.
* **Confidence intervals**: (1) Fisher — inverse observed information over the free players
  (pseudo-inverse under the mean-zero constraint without anchors), ±1.96 SE; conservative
  when there are many draws. (2) **Game-pair bootstrap (headline)** — opening pairs
  (`pairing`, `game_pair` per file) are resampled with replacement within each player pair,
  the MLE is refitted, 2.5/97.5 percentiles over 200 resamples (seeded, reproducible). It is
  not reported for a player with fewer than 5 opening pairs, or when the resamples take fewer
  than 5 distinct values (e.g. every opening pair of a pairing ended 1–1).
  **Measured coverage**: in 300 simulated 21-level calibrations (truth known) the bootstrap
  interval covered the true rating about **93 %** of the time, not 95 % (1000 resamples:
  93.6 %); for levels rated through a long chain far below the anchors 91–93 %, and there
  the truth lies *above* the interval 2–3× more often than below. The Fisher interval is
  conservative (96–99.7 %). Read the headline CI of `extrapolated` levels as roughly a 90 %
  interval, or use the wider of the two.
* `--pairs` adds the per-pairing table: W/D/L, score, Elo difference with a 95 % CI
  (trinomial normal approximation, same formulas as the arena summary; Wilson score
  interval when every game had the same result, so an all-draw pairing does not get a
  zero-width interval and a 0–N pairing gets a finite upper end), LOS, and the difference
  implied by the joint fit.
* Library: `load_jsonl`, `aggregate`, `fit_ratings`, `bootstrap`, `analyze` (everything as
  one JSON-able dict), `unbounded_players`, `perfect_score_bound`, `wdl_stats`,
  `elo_from_score`, `los`.

### `calibrate.py` — Skill Level → Elo calibration

Calibrates the engine's `Skill Level` (default 1–21, default personality, `Threads=1`,
`Hash=16`) against **Stockfish 16 with `UCI_LimitStrength=true`**, `UCI_Elo` ∈
{1320, 1500, 1700, 1900, 2100, 2300, 2500} (`--anchors`), `Threads=1`, `Hash=16`. The
anchors are the only fixed ratings. Levels are named `<prefix>_L<level>` (e.g. `aggro_L05`),
anchors `sf<elo>`, so the two can never be confused in the PGN/JSONL. All games are played
by `aggro_arena match` from generated configs.

```sh
python3 testing/calibrate.py --dry-run                     # print the stage-1 configs + a stage-2 preview
python3 testing/calibrate.py                               # full run at 10+0.1, concurrency 14
python3 testing/calibrate.py --tc-check 60+0.6             # ... plus the hybrid TC check (stage 4)
python3 testing/calibrate.py --resume [--run-dir testing/runs/calib-...]   # continue (default: newest run)
python3 testing/calibrate.py --report-only --run-dir testing/runs/calib-...  # re-fit, rewrite the report
python3 testing/calibrate.py --quick [--tc-check 4+0.04]   # smoke test: levels 1,11,21, tiny game counts
# generic engine under test, e.g. Stockfish's own Skill Level 0..20 as a stand-in:
python3 testing/calibrate.py --engine /usr/games/stockfish --engine-files '' \
    --levels 0-20 --prior 0:1300,20:2900
```

**Stages**

1. **Scout** — every level plays `--scout-games` (10) games against each of the
   `--scout-anchors` (2) anchors nearest to its prior Elo, and against the next level in the
   list. The prior (`--prior`, `level:Elo` points, linear in between; default `1:400,21:2100`)
   is only used here. A joint fit of the scout games with the same tiny prior as the final fit
   gives the scout estimate (scheduling only); a level that scored 0 % / 100 % in the scout is
   placed at its one-sided 95 % bound, or at its prior-map value if that is more extreme.
   (Runs whose stage-2 plan was made before 2026-11-09 used 1 virtual draw per pair here,
   which put the scout estimates of levels far below sf1320 +100…+500 Elo too high, so some
   of them got anchor games outside the intended window; the report's Method section says
   which prior a run used. Final ratings were not biased by it.)
2. **Main** — every level gets ~`--games` (200) games: `--anchor-share` (½) of them split
   between the (up to) `--main-anchors` (2) anchors within ±`--anchor-window` (350) of its
   scout estimate, the rest against levels up to `--chain` (2) steps away in the level list
   (L±1 weight 2, L±2 weight 1; a pairing gets the larger of the two levels' wishes). Levels
   with no anchor in the window — the weak levels far below Stockfish's 1320 floor — play only
   the chain, which ties them to the anchored levels. If *no* level has an anchor in its
   window, the level closest to an anchor plays that anchor anyway (`forced_anchor`), so the
   stage-2 games are always tied to the Stockfish scale. `--dry-run` shows the preview: 620 scout
   games and ~2,900 main games for 21 levels (the real numbers depend on the scout).
3. **Fit** — joint MLE over all games of stages 1+2 with the anchors fixed (`elo.py`),
   bootstrap and Fisher CIs, confidence labels, and a monotonicity check (every inversion
   between consecutive levels is listed with its bootstrap probability; it is a
   `VIOLATION` when the 95 % CIs do not overlap).
4. **Hybrid TC check** (optional, `--tc-check 60+0.6 --tc-levels 1,6,11,16,21 --tc-games 100`)
   — the listed levels are re-measured at that TC against their nearest anchors (within the
   window of their stage-3 estimate); a level without one is chained to the next stronger
   checked level (the strongest such level to its nearest checked level). Every group of
   chained levels is then tied to an anchor: if no member has one in its window, either the
   member closest to an anchor plays it although it is outside the window (label `anchor
   outside the ±350 window`) or a member is chained to the nearest checked level that has
   anchors, whichever Elo gap is smaller. A separate fit of these games gives the rating at
   the check TC, and the report shows the **drift** vs the main TC with an approximate 95 %
   half-width (root sum of squares of both CI half-widths). If the check-TC games contain no
   anchor at all (only possible with a stage-4 plan made by an older version), no rating and
   no drift are reported (`not connected to an anchor at the check TC`) instead of ratings
   centred on 0.

**Confidence labels** (report column): `anchored` (played anchors directly, inside the anchor
range ± `--extrap-margin` 100, ≥ `--min-games` 100 games); `chained` (inside the range but
rated only through other levels); `extrapolated` (more than 100 below the lowest or above the
highest anchor); `low-sample` (< 100 games); `bound only (0% / 100% score)` /
`no finite estimate` (see `elo.py` above). A CI marked `(Fisher)` stands in where the
bootstrap is not meaningful. Nothing is printed that was not computed (`n/a` otherwise).

Every `--engine-option` name (and `--level-option`) must be advertised by the engine
(case-insensitive) — a misspelt option is an error at start-up, not a silent default. If
setting up a new run fails, its directory is removed again so the same `--run-dir` can be
retried; `--resume` on a directory without `settings.json` is a clean error.

**Run directory** (default `testing/runs/calib-<timestamp>/`, `--run-dir` to choose):

```
settings.json            resolved settings, environment, list of resumes
engine/aggro_chess       COPY of --engine taken at start (+ --engine-files, default ph-gambitbook.bin);
                         the engine runs from here with this dir as cwd, so rebuilding target/ is harmless
bin/aggro_arena          copy of the arena binary (testing/arena may be rebuilt meanwhile)
stage1_scout/  stage2_main/  stage4_tc/
    plan.json            pair lines, batches, opening ranges (written once, reused on --resume)
    bNN.cfg  bNN.openings  bNN.jsonl  bNN.pgn  bNN.jsonl.summary.json  bNN.log
stage1_scout/fit.json    scout estimates used for scheduling
```

Each stage is split into batches of `--batch-games` (default 20 × concurrency) games, one
arena config each, run one after another with `--concurrency` (default 14) parallel games.
Openings: the suite is shuffled once (`--seed`) and every batch gets its own consecutive
slice, so no opening is reused anywhere in a run; each opening is played twice with colours
reversed.

**Resume**: `--resume` reuses `settings.json` and the stage plans; a batch whose JSONL already
holds exactly the planned games of every pair line (and whose config is unchanged apart from
`concurrency`) is skipped, an incomplete batch is replayed from scratch. Settings that define
the schedule (engine, levels, anchors, prior, tc, game counts, seed, batch size, …) cannot be
changed on resume; `--concurrency`, `--bootstrap`, `--min-games`, `--out` and the stage-4
options can. The engine copy's sha256 is checked. Ctrl-C / SIGTERM stops the arena cleanly
(exit 130) and prints the resume command.

**Outputs**: `testing/results/elo_calibration.md` and `.json` (`--out PREFIX`; with `--quick`
the default is `<run-dir>/elo_calibration_quick.*` so a smoke test never overwrites real
results). The report holds the table *Skill Level | Estimated Elo | 95% CI | Fisher 95% |
games | W/D/L | games vs anchors | confidence*, monotonicity, the TC-check table, all pairings,
the schedule (prior, scout estimate, chosen anchors/chain partners per level), the
environment (CPU model, `nproc`, memory, load, date, host, Python, engine `id name` and
sha256 of the copied binary and book, Stockfish `id name` and sha256, arena and opening-file
sha256, AggroChess git head/branch, TC, games, concurrency) and the method. The JSON has
everything, including per-pairing statistics and the stage plans.

Ratings are on the **Stockfish 16 `UCI_Elo` scale at the chosen TC on this machine**, not
FIDE or CCRL; the hybrid TC check shows how far the 10+0.1 numbers move at the TC the
`UCI_Elo` scale was tuned for.

---

## `analyze` — sacrifice detection and classification

```sh
aggro_arena analyze <games.jsonl>... --engine <name>[,<name>...] [--engine <name>]...
                    [--sf /usr/games/stockfish] [--sf-nodes 400000] [--threads 12] [--sf-hash 64]
                    [--out <prefix>] [--quiet] [--no-trade-guards]
```

Reads `match` JSONL files (`game::read_jsonl`), replays every game with cozy-chess and finds
the sacrifices of the engine(s) named by `--engine`. Names are matched **exactly** against the
`white`/`black` fields, on either colour; several names may be given (comma-separated or
repeated `--engine`), e.g. to pool runs where the engine had different names. A game in which
both sides match is analysed from both sides (a warning is printed). Games where no side
matches are skipped and counted. Writes `<prefix>.json` (summary) and `<prefix>.events.jsonl`
(one line per sacrifice or blunder) and prints a text summary. The default prefix is the
first input without `.jsonl` plus `.sac`. Exit codes: 0 ok, 1 error, 2 usage, 130 interrupted.

### Definitions

* Material: P=100 N=320 B=330 R=500 Q=900 (kings ignored). **`bal(i)` = material of the
  analysed engine minus the opponent's in the position *before* ply `i`** (ply 0 = White's
  first move; `bal(n)` is the final position; indices past the end are clamped to it).
* Engine plies: the plies where the analysed engine is to move (White: even plies, Black: odd).
  The first `opening_plies` plies come from the opening file and can never start a sacrifice.
  Engine **book** moves (`book: true` in the JSONL) are analysed and flagged `book`.
* **Candidate at engine ply k**: `d2 = bal(k) - bal(k+2)`, `d4 = bal(k) - bal(k+4)`,
  `d6 = bal(k) - bal(k+6)` (games that end earlier use the final position, also when the engine
  mated). The category comes from the piece-composition change between position k and k+4:
  `net[p]` = pieces of type p lost by the engine minus pieces of that type lost by the opponent
  (N and B are pooled as minors). An **engine** promotion counts as the promoted piece
  appearing; an **opponent** promotion is neutral (its pawn is not counted as lost, the new
  piece not as gained), so allowing a promotion (`70.Ke1 a1=Q#`) is never a "queen
  sacrifice" — `d2`/`d4` still include the promotion's value, and events record
  `opponent_promotions`:

  | category | composition | thresholds |
  |---|---|---|
  | `queen` | net queens lost ≥ 1 **and the engine actually lost a queen** | d2 ≥ 300, d4 ≥ 300 |
  | `exchange` | net rook lost = 1, net minors = −1, no net queen, ≤ 1 pawn either way | d2 ≥ 120, d4 ≥ 120 |
  | `piece` | net ≥ 1 minor or ≥ 1 rook lost, not the exchange pattern | d2 ≥ 200, d4 ≥ 150 |
  | `pawn` | only pawns net lost | d2, d4, d6 ≥ 100 |

  Precedence queen > exchange > piece > pawn: the first category whose pattern *and*
  thresholds hold wins. Plain recaptures never qualify (the deficit must persist to k+4).
* **Trade guards** (an addition to the rule above; `--no-trade-guards` gives the literal rule).
  None of them applies while the engine is **mating** (its own score at k or k+2 is a mate
  score for it, or it mates by ply k+6; event field `mating`):
  (a) a candidate is dropped if `bal(k+5)` and `bal(k+6)` are both back within **±50** cp of
  the baseline, i.e. the engine recaptured with its own move at k+4 after an in-between move
  (`10.Qxd5 Qxd5 11.Nf6+ gxf6 12.Bxd5` is a queen trade, not a queen sacrifice). A
  combination that wins *more* than the material back (`bal(k+5)` > baseline + 50) is kept
  as a sacrifice (it is then `recouped`);
  (b) **check-adjusted baseline**: if the engine is in check at k (its move is forced) and
  `bal(k-2) < bal(k)`, the baseline is position k-2. In that case `bal(k)` is inflated because
  the opponent delayed a recapture with a check (`9.Nxe5 Qa5+ 10.Nd2 Nxe5`). Events record
  `baseline_ply`/`bal_baseline`, and d2/d4/d6, the composition and the recoup test use that baseline.
  (c) **persistence after the engine's own reply**: the deficit must still be at least the
  category's d4 threshold in position k+3 (after the engine's next move, field `d3`), unless
  that move was a capture the opponent recaptured on the same square (then k+3 is mid-trade).
  Sampling only after the opponent's moves made opponent-initiated even trades look like a
  sacrifice: `11...Qd8 12.Nxf6+ Bxf6 13.dxe5 Nxe5 14.Nxe5 Bxe5` showed d2/d4/d6 = 320/100/320
  although Black was never down after its own moves.
  Examples the guards exist for: the queen trade above flagged as a queen blunder, and
  `27...Rxf3 28.Nf6+ gxf6 29.Rxf3` (R for N+P, 80 cp) reported as a 180 cp exchange sacrifice.
* **Intent filter**: `e0 = scores[k]` (engine's own score when it played the move), `e1 =
  scores[k+2]` (its score one move later), `e_prev = scores[k-2]` (its previous move). Checked
  in this order:
  1. e0 is a mate score **against** the engine → `type: "desperation"` (interpositions and
     spite checks that only delay mate; excluded from sacrifices and blunders, summary
     `desperation_moves`);
  2. exactly **one legal move** at k → `type: "forced"` (no choice, so no sacrifice; summary
     `forced_losses`) — unless rule 4's collapse also holds, then it is a blunder as in 4;
  3. a mate score in the engine's favour in e0 or e1 → sacrifice (`intent: mate`);
  4. the engine's score had already **collapsed** since its previous move (`e_prev − e0 >
     150`): the opponent's move k−1 made the loss unavoidable (fork, check with one reply), so
     the loss is a **blunder of move k−2**, not a sacrifice at k (`type: "blunder"`,
     `intent: "collapsed"`, `eval_ply` = k−2 is where Stockfish judges it; summary
     `blunders_collapsed`). Example: `3.Qe2?? Nxc2+ 4.Kd1 Nxa1` is a blunder of 3.Qe2,
     not a sacrifice by 4.Kd1;
  5. the spec rule: sacrifice if `e1 ≥ e0 − 150` (`intent: sound`); a missing score (book
     move, engine prints none) counts as a sacrifice with `intent_unknown: true`; if the
     engine was mated before its next move, e1 = −mate; otherwise a **blunder**.
  Blunders are reported separately (events with `type: "blunder"`, summary `blunders`),
  never as a sacrifice. Rules 1, 2 and 4 are additions to the spec.
* **Chains / dedup**: every event opens a window of its own kind up to its ply + 4. A
  candidate inside a sacrifice's window is recorded in its `chain` (with its own intent): a
  further sacrifice extends the window by 4 plies and increases `chain_len`; a forced /
  desperation move is only listed; a **blunder neither extends the window nor is absorbed** —
  it becomes (or joins) a blunder event and counts in the blunder totals. Inside a blunder /
  forced / desperation window, candidates of the same kind are absorbed (extending that
  window); anything else starts a new event. So the next sacrifice can start at k+6 unless a
  further sacrifice extended the chain, as in the spec.
* **Context** (addition): `balanced` if |e0| < 300, `winning` if e0 ≥ 300, `losing` if
  e0 ≤ −300 (`unknown` without a score). AggroChess accepts almost any material sacrifice
  once a game is decided, so `balanced_sacrifices_per_game` measures sacrifices made in
  positions that were still open.
* **Outcome** (engine's view): `mate`: the engine delivered checkmate within 30 plies of k
  (final position checked with cozy-chess). `recouped`: at an engine-to-move position k+6,
  k+8, …, k+24 (or the final position within that range) `bal` is back to ≥ baseline − 50
  (`recouped_ply`, `recoup_score` = the engine's score there; a finished game counts as ±mate/0).
  `result`: win/draw/loss. `successful_attack` = mate, or recouped with `recoup_score ≥
  e0 − 50` (e0 missing → 0), or SF ≥ +150 for the engine at ply k+12 (`success_reason`).
* **Stockfish** (`--sf`): `--threads` independent Stockfish processes (`Threads=1`,
  `Hash=--sf-hash`, `ucinewgame` before every position, `go nodes --sf-nodes`) evaluate the
  position before ply k (`sf_before`), after the move (position k+1, `sf_after`) and, for
  sacrifices, position k+12 (`sf_k12`). Positions are sent as `position startpos moves …`
  (so Stockfish sees the history); terminal positions are scored locally. Stockfish reports
  from the **side to move**; values are normalised to the engine's view (negated when the
  opponent is to move), mates → ±3000, cp clamped to ±3000. `unsound` = `sf_after <
  sf_before − 150` and `sf_after < −100`. `practical_chance` = unsound and (the game was not
  lost, or SF ≥ +100 for the engine at some engine-to-move position k+2 … k+30;
  `sf_best_later`). Those later positions are evaluated only for unsound sacrifices in lost
  games (second pass). Blunders get `sf_before`/`sf_after` too (`blunders_confirmed_by_sf`).
  Throughput: about 4 positions/s per 3 processes at 200k nodes on this machine. A failed
  evaluation is retried once on a fresh process, then left `null` and counted in
  `stockfish.positions_failed`.

### Output

`<prefix>.events.jsonl` — one object per event: `type` (`sacrifice`/`blunder`/`forced`/
`desperation`), `category`,
`file`, `game_id`, `pairing`, `game_pair`, `opening_index`, `white`, `black`, `engine`,
`opponent`, `color`, `ply`, `move_number`, `move` (UCI), `san` (`23...Bxh3`), `book`, `fen`
(before the move), `line` (SAN, 8 plies), `bal` (`[bal(k), bal(k+2), bal(k+4), bal(k+6)]`),
`bal_k3`, `bal_k5`, `in_check`, `baseline_ply`, `bal_baseline`, `d2`, `d3`, `d4`, `d5`, `d6`,
`lost_engine`, `lost_opponent`, `net_lost` (piece counts k→k+4; opponent promotions
neutral), `opponent_promotions`, `mating`, `legal_moves`, `truncated` (game ended before
k+6), `e_prev`, `e0`, `e1`, `intent` (`sound`/`mate`/`unknown`/`blunder`/`collapsed`/
`forced`/`desperation`), `eval_ply`, `intent_unknown`, `context`, `chain_len`, `chain`, `result`, `termination`,
`game_plies`; for sacrifices also `mate`, `mate_ply`, `recouped`, `recouped_ply`,
`recoup_score`, `successful_attack`, `success_reason`, `sf_k12`, `unsound`,
`practical_chance`, `sf_best_later`(`_ply`); and `sf_before`, `sf_after` (null without `--sf`).

`<prefix>.json` — `inputs`, `engine_names`, `games_in_files`, `games_invalid`,
`games_without_engine`, `games_analysed`, `stockfish` (path, id, nodes, processes, positions
evaluated / failed), `params` (all thresholds), `overall`, `per_opponent` (same block per
opponent name) and `per_engine` when several names matched. A block contains: `games`,
`result` (W/D/L + mean score with 95% CI), `engine_moves_analysed`, `games_with_sacrifice`,
`sacrifices`, `sacrifices_per_game`, `sacrifices_per_100_moves`, histogram, `categories`,
`book_sacrifices`, `intent_unknown`, `intent_mate`, `chained_sacrifices`, `avg_chain_len`,
`sacrifices_by_context`, `balanced_sacrifices_per_game`, `games_with_balanced_sacrifice`,
`blunders`, `blunders_per_game`, `blunder_categories`, `blunders_collapsed`,
`forced_losses`, `desperation_moves`, `mates_after_sacrifice`,
`successful_attacks`, `recouped`, `score_games_with_sacrifice`,
`score_games_without_sacrifice`, `sacrifice_game_result`, `avg_move_number`; with SF also
`sf_evaluated`, `unsound`, `practical_chances`, `sound_sacrifices`, `avg_sf_before`,
`avg_sf_after`, `blunders_confirmed_by_sf`. It also has `per_category` (per category: count,
success / mate / recouped rates, game results, average d4 and move number, unsound /
practical rates and average SF drop). Proportions are `{count, of, rate, ci95}` (Wilson
95%); means are `{n, mean, sd, ci95}` (normal approximation, clipped to the natural range,
e.g. ≥ 0 for counts). A `score` block also has `points`, and its `ci95` is the Wilson
interval on score points (draws = ½), which stays inside [0, 1] and does not collapse at
0% / 100%; it is conservative when there are draws. Event-level proportions treat
sacrifices as independent. Several sacrifices in one game are correlated, so those
intervals are somewhat optimistic; the per-game metrics use the game as the unit.

Example:

```sh
aggro_arena analyze runs/x/games.jsonl --engine aggro --sf /usr/games/stockfish \
    --sf-nodes 200000 --threads 3 --out runs/x/aggro_sac
```

Validation: `cargo test --release --offline analyze` covers these cases:
- the category / intent / judge rules;
- hand-built games: a queen sacrifice leading to Légal's mate, an exchange sacrifice by Black, a book King's Gambit pawn, a hung knight (blunder), plain trades and an opening-file gambit (nothing detected), a knight fork whose forced loss is a blunder of the previous move;
- real games: a zwischenzug trade (guards (a) and (b)), an opponent promotion (no queen sacrifice) and opponent-initiated trades (guard (c)).

Effect of the 2026-11-09 rule changes on the 7-engine decomposition run
(`/root/aggroclone/runs/decomp/games.jsonl`, no SF): of the 1139 sacrifices reported before,
686 are still sacrifices, 196 are now blunders of the previous move (`collapsed`), 157 are
desperation moves while being mated, 22 forced single-move losses, 26 were opponent
promotions and 52 opponent-initiated trades (guard (c)); 61 new sacrifices appear, mostly
combinations that guard (a) used to drop (16 of the 25 that ended in or announced mate).
Totals: 747 sacrifices (queen 40, exchange 46, piece 240, pawn 421; mate within 30 plies
16.6 % instead of 9.9 %, successful 42.0 % instead of 31.0 %), 807 blunders, 167
desperation, 32 forced. Counts from runs before that date are not comparable.

The `bal`/`d*`/`e0` values of all 66 events from a 20-game real match were also re-derived by
an independent Python replayer, and 12 sacrifices and 4 blunders were checked by hand.

---

## `sactest.py` — modified vs original: sacrifice comparison

```sh
python3 testing/sactest.py [--new target/release/aggro_chess] [--old /root/aggroclone/engines/aggro_original]
        [--new-options 'Name=Value;...'] [--old-options '...'] [--games 100] [--h2h-games 100]
        [--tc 10+0.1] [--concurrency 14] [--anchors 1800,2200] [--sf /usr/games/stockfish]
        [--sf-nodes 400000] [--sf-threads 12] [--no-sf] [--seed 1] [--out RUN_DIR]
        [--report testing/results/sacrifice_report] [--skip-matches] [--dry-run]
```

Stdlib-only driver around `aggro_arena match` and `aggro_arena analyze`:

1. Creates the run directory (default `testing/runs/sactest-<YYYYmmdd-HHMMSS>`, git-ignored) and
   **copies both engine binaries** to `<run>/bin/aggro_new` and `<run>/bin/aggro_old` (sha256
   of source and copy must match, so a rebuild during the copy is detected). All matches use the
   copies, with cwd `--engine-cwd` (default the AggroChess checkout, so both engines find
   `ph-gambitbook.bin`). Each copy is probed with `uci`. The new engine gets `Threads=1`,
   `Hash=16` and `Skill Level=21` (default personality), then `--new-options`. The old engine
   gets `Threads=1`. Option names are matched case-insensitively against the advertised
   options, and an unknown name is an error, so a typo cannot silently test the default
   personality.
2. Writes and runs, one after another (each with `concurrency` parallel games):
   * `gauntlet_new.cfg`: `aggro_new` vs every anchor (`sf<Elo>`: Stockfish `Threads=1`,
     `Hash=16`, `UCI_LimitStrength=true`, `UCI_Elo=<Elo>`; default anchors 1800 and 2200 as
     in the spec — before 2026-11-09 the default was 1800,2100), `--games` games each;
   * `gauntlet_old.cfg`: the same for `aggro_old`, **same seed**, so both engines get the same
     openings and colours against each anchor;
   * `h2h.cfg`: `aggro_new` vs `aggro_old`, `--h2h-games` games (seed + 1000).
   Output goes to `<stem>.jsonl`, `.pgn`, `.jsonl.summary.json` and `.log`. A rough duration
   estimate is printed first: 500 games at 10+0.1 with 14 games in parallel take about 20 minutes.
3. Runs `analyze` for each engine on its gauntlet plus the head-to-head games
   (`--engine aggro_new` / `aggro_old`, Stockfish with `--sf-nodes` and `--sf-threads`
   processes), writing `<run>/analysis_{new,old}.json`, `.events.jsonl` and `.log`.
4. Writes `testing/results/sacrifice_report.md` and `.json` (prefix `--report`, copies in the
   run directory). The report contains:
   * the setup: dates, host, CPU model, logical CPUs, RAM, OS, Python, TC, games,
     concurrency, anchors, openings and seed, Stockfish analysis settings, and the sha256 of
     both engines (source path and copy used), the book, Stockfish and the arena;
   * the match results (W/D/L, score, Elo with 95% CI, LOS, forfeits);
   * a side-by-side **new / old / new − old** table over all games, using every metric of
     the analysis (including blunders caused by the previous move, forced material losses
     and moves while being mated, which are excluded from the sacrifice counts). Wilson CIs are used for proportions and normal CIs for per-game means;
     the difference uses a Newcombe interval for proportions and scores and a Welch interval
     for means;
   * per-category counts, success, mate and unsound rates;
   * the same comparison per opponent (each anchor, and head-to-head seen from each side);
   * example sacrifices and the definitions and caveats.
   The JSON holds the same data: run metadata, match summaries, every comparison row with
   `new`, `old`, `diff` and `diff_ci95`, the full analysis blocks and the examples.

`run.json` in the run directory records everything needed to reproduce or re-analyse a run:
engines with sha256 and options, environment, plan, start and finish times, and per-match
exit codes and durations.

* `--skip-matches` re-analyses an existing run, by default the latest `testing/runs/sactest-*`.
  Use it with other `--sf-nodes`/`--sf-threads`, `--no-sf`, or after changing `analyze`.
* `--new-options 'Sacrifice Appetite=50;Aggression=60'` tests another personality. Each run
  copies the binaries again, so use a separate `--out` (the default timestamped directory does that).
* `--dry-run` writes the configs and `run.json` and prints the plan without playing.
* Ctrl-C / SIGTERM: the arena runs in its own session and receives exactly one SIGTERM.
  sactest keeps reading its output until it exits: the running games are abandoned and the
  summary is written with `interrupted: true`. sactest then exits 130. The games played so
  far can be re-analysed with `--skip-matches --out <run>`. A second signal is forwarded too,
  and the arena then exits at once.

Smoke test (about 2 minutes; keep it away from the real report with `--report`):

```sh
python3 testing/sactest.py --games 6 --h2h-games 6 --tc 3+0.03 --concurrency 5 \
    --sf-nodes 100000 --sf-threads 4 --report testing/runs/smoke_report
```

## `regress_classic.py` — the Classic preset is AggroChess 4.5.0

```bash
python3 testing/regress_classic.py [--new target/release/aggro_chess] [--old <AggroChess 4.5.0 binary>] [--depth 12]
```

Runs a fixed-depth, single-threaded search on ten positions with both engines, the new one set to the
Classic personality (Aggression 60, Sacrifice Appetite 75, King Attack Priority 60, Material
Preservation 50, Speculative Sacrifices 50, Skill Level 21), and compares the node count and
principal variation at every depth and the best move. Exit status 0 when all match. Printed scores
are not compared: the new engine reports scores without the root sacrifice bonus.

## `fill_readme.py` — measured tables into the main README

```bash
python3 testing/fill_readme.py [--elo testing/results/elo_calibration.json] [--sac testing/results/sacrifice_report.json] [--readme README.md] [--only elo|sac]
```

Rewrites the sections between `<!-- ELO_TABLE -->` / `<!-- /ELO_TABLE -->` and `<!-- SAC_TABLE -->` /
`<!-- /SAC_TABLE -->` in `README.md` from the calibration and sacrifice-test results, so the README
shows exactly what was measured. Run it after `calibrate.py` or `sactest.py`.
