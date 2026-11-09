# AggroChess Ultimate Sacrificial Edition 5.0 ⚔️

**AggroChess Ultimate Sacrificial Edition** (USE) is a modified version of [AggroChess](https://github.com/PhelRin/AggroChess) 4.5.0 by Rinarto Gibson, the aggressive, sacrificial UCI chess engine written in Rust in the spirit of Mikhail Tal. It keeps AggroChess's search, NNUE evaluation, gambit book and UCI implementation, and adds:

* **a more sacrificial default personality**: in balanced positions about 1.5 times as many purposeful sacrifices per move as AggroChess 4.5.0 (4.9 against 3.2 per 100 moves), more piece sacrifices and Tal chains, sacrifices earlier in the game and from equal positions rather than won ones, and an appetite for sacrifice that no longer switches off after the first one. The price is measured too: more of its sacrifices are objectively unsound, and it is about 130 Elo weaker than 4.5.0 head-to-head (see [Sacrifice testing](#sacrifice-testing));
* **five personality controls** as UCI options: `Aggression`, `Sacrifice Appetite`, `King Attack Priority`, `Material Preservation`, `Speculative Sacrifices`;
* **a `Skill Level` 1–21 strength limiter** in the style of Patricia's, independent of the personality, with a [measured rating for every level](#measured-strength-of-every-skill-level);
* standard GUI options (`Hash`, `Clear Hash`, `MultiPV`, `Move Overhead`) and fixes for `go infinite` and `stop`;
* a self-contained **testing system** (`testing/`): a match runner, a UCI conformance test, a sacrifice analyser and an Elo calibration tool.

Setting the personality to the *Classic* values reproduces AggroChess 4.5.0's search exactly (same moves and node counts), so the original engine is one preset away.

> This is a modified version of AggroChess, distributed under the GNU GPL v3 (see `LICENSE`). Modifications by the Ultimate Sacrificial Edition, November 2026: personality options, Skill Level, MultiPV, UCI fixes and the testing tools. The original 4.5.0 README is kept in [`docs/README_AggroChess_4.5.0.md`](docs/README_AggroChess_4.5.0.md).

## Installing it in a chess GUI

AggroChess is a standard UCI engine: one executable, with the neural network and the gambit book built in.

1. Build it (see below). `ph-gambitbook.bin` next to the executable (or in the GUI's working directory for the engine) is optional: a Polyglot book used after the built-in gambit lines run out.
2. Add it to your GUI:
   * **Cute Chess**: *Tools → Settings → Engines → +*, choose the executable, protocol UCI. The options appear under *Advanced*.
   * **Arena**: *Engines → Install New Engine…*, pick the executable, confirm UCI. Options: *Engines → Manage → Details → UCI*.
   * **Banksia GUI**: *Engines → Add*, select the file; the options are in the engine's configuration panel.
   * **lichess-bot, Coliseum, Nibbler, En Croissant** and other UCI front ends work the same way.
3. Set `Threads`, `Hash` and, if you like, the strength and personality options described below.

## Building from source

1. Install [Rust](https://www.rust-lang.org/) (stable).
2. Build:
   ```bash
   cargo build --release
   ```
3. The binary is `target/release/aggro_chess` (`aggro_chess.exe` on Windows).

`cargo test --release` runs the test suite (37 tests, plus 2 ignored developer scans: mates, the Greek Gift, sacrificial mates, the Albin pawn that must *not* be sacrificed, swindle timing, every opening-book line, SEE, evaluation symmetry, incremental network updates, the personality mapping, every Skill Level playing legal moves in time, Skill Level never touching the personality, …). `aggro_chess bench` prints a node count and speed (287284 nodes with the default personality).

## UCI options

| Option | Type | Default | Range | What it does |
|---|---|---|---|---|
| `Threads` | spin | CPU cores (max 8) | 1–128 | Search threads (Lazy SMP). Skill Levels 1–20 always search with one thread. |
| `Hash` | spin | 128 | 1–4096 MB | Transposition table size. Allocated on `isready`. |
| `Clear Hash` | button | | | Empties the transposition table. |
| `MultiPV` | spin | 1 | 1–64 | Number of best lines shown in analysis. |
| `Move Overhead` | spin | 50 | 0–5000 ms | Time kept back per move for GUI and network lag. |
| `Skill Level` | spin | **21** | 1–21 | Playing strength. 21 = full strength, no weakening at all. See [Skill Level](#skill-level). |
| `Aggression` | spin | **90** | 0–100 | How sharp a game it wants: trade aversion, contempt, active pieces and threats, forcing moves. |
| `Sacrifice Appetite` | spin | **95** | 0–100 | How much, and how often, it wants to sacrifice. |
| `King Attack Priority` | spin | **90** | 0–100 | How much the attack on your king is worth. |
| `Material Preservation` | spin | **15** | 0–100 | How much a material deficit hurts (low: keeps attacking a piece down). |
| `Speculative Sacrifices` | spin | **90** | 0–100 | How *unsound* a sacrifice may be. |
| `BookPath` | string | ph-gambitbook.bin | | Optional Polyglot book used after the built-in gambit lines. |

`Skill_Level` (Patricia's spelling) is accepted as an alias of `Skill Level`. Changing a personality option clears the hash table (stored scores contain the old personality).

The scores the engine reports **exclude the sacrifice bonus** (AggroChess 4.5.0 included it), so a GUI and its adjudication see the engine's objective view of a sacrificial line. They still contain the personality's evaluation terms, so with the default personality the engine is somewhat optimistic about its own attacks; for neutral analysis use the *Analysis partner* settings below.

## Personality settings

Five controls, each 0–100, independent of the Skill Level and of each other. With the **Classic** values (Aggression 60, Sacrifice Appetite 75, King Attack Priority 60, Material Preservation 50, Speculative Sacrifices 50) the engine searches exactly like AggroChess 4.5.0: same moves, same principal variations, same node counts (checked by `testing/regress_classic.py`). The defaults go well beyond that.

| Control | Low values | High values | What it changes inside the engine (Classic → default) |
|---|---|---|---|
| **Aggression** | calm, happy to trade and to draw | keeps pieces and queens on, avoids draws, wants activity and threats | trade-aversion weight 120 → 180; contempt 50 → 75; new bonus for protected pieces in your half and for attacked enemy pieces (0 → 60); forcing tie-break 12 → 18 |
| **King Attack Priority** | the attack on your king counts objectively | your king is what it plays for | extra king danger 220% → 250% (capped at 22 pawns); pieces aimed at the king zone, heavy pieces on open files near the king and the h6/a6 king clamp 120 → 150; new pawn-storm bonus; king-zone moves searched earlier (2500 → 3250). Above 60 the slope is half that below it: the steeper mapping was measured to cost about 140 Elo on its own |
| **Sacrifice Appetite** | sacrifices only when they are sound anyway | hungry for sacrifices all game | sacrifice bonus for the game's first sacrifice 300 → 340; **appetite after it 60 → 180** (4.5.0 nearly switched it off: one sacrifice per game, then near-sound play); chained sacrifices 150 → 190; hunger for a first sacrifice starts at move 20 → 14; new gambit bonus for pawn sacrifices aimed at the king or opening lines (0 → 40) |
| **Speculative Sacrifices** | sacrifices must be nearly sound | will sacrifice into objectively worse positions | a sacrificial line may score down to −3.1 → −4.3 pawns and still earn the bonus; full bonus with 3 → 2 attacking pieces behind it (partial bonuses 60%/25% → 76%/35%); leaving material hanging at the root counts as a sacrifice (off → on); its checking sacrifices survive pruning longer (200 → 320 cp per ply) and its sacrifices are extended from depth 4 → 3 |
| **Material Preservation** | a piece down with an attack feels fine | material is sacred | 50 is neutral (4.5.0). Below 50, up to 30% of a material deficit is forgiven while its pieces bear on your king (default 15: up to 21%); above 50 a deficit costs up to 25% extra |

**Sacrifice versus blunder.** The bonus only goes to a move that gives up about two pawns or more (about one pawn for the gambit bonus) **and** is aimed at the king with attackers behind it, or is an exchange sacrifice, or leaves material for an attack; and only if the line it starts stays above the floor. The size of the bonus is the engine's tolerance for unsoundness: with the default it will play a sacrifice its own search judges up to about 1.8 pawns worse than the best quiet move all game long (3.4 pawns for the game's first sacrifice), but not the four-pawn give-aways a bigger appetite produced in testing. A piece simply left hanging with nothing aimed at your king gets no bonus at all.

## Skill Level

`Skill Level` runs from 1 to 21, like Patricia's `Skill_Level`. **21 is full strength**: none of the code below runs, and the search is exactly the unrestricted one. Levels 1–20 weaken the engine in four ways (`src/skill.rs`):

1. **MultiPV candidates.** The root is searched for 2–8 candidate moves, each with a real score from the engine's own point of view (sacrifice bonus included).
2. **Controlled search limits.** A node budget per move that grows about 1.5× per level (150 nodes at level 1 to 185,000 at level 18), a depth cap at levels 1–7, a more careless search at the lowest levels (extra late-move reductions at 1–6, no quiet checks in quiescence at 1–3, no check extensions at 1–2), and no long-think extras (Tal's look, forcing-line priming, rescue look, swindles) or helper threads. The weak levels move almost instantly and play the same at any time control; levels 19–20 use the clock.
3. **A centipawn-loss budget** (as in Patricia). Each move adds the level's `cp_loss` to an allowance for mistakes, capped at `max_budget`; the chosen move's loss against the best candidate is paid out of it. Over a game the average loss per move is about `cp_loss`, delivered as a mix of best moves, inaccuracies and the occasional real mistake (bounded by `max_budget`), not as random howlers.
4. **Probabilistic, in-character choice.** Each move draws a random target loss within the allowance and plays the candidate closest to it, tilted towards sacrifices, checks, captures and moves at your king by amounts that come from the *personality* options only. A forced mate is never thrown away, and a move that walks into a mate the best line avoids is never chosen.

A weakened engine also leaves its opening book after a level-dependent number of plies (levels 1–10).

The Skill Level never touches the personality options, and the personality options never touch the strength limits (both are tested): Skill Level 10 with Sacrifice Appetite 100 is a weaker engine that still hunts for sacrifices, and Skill Level 21 with Sacrifice Appetite 25 is a full-strength engine with a calmer style. Set `AGGRO_SEED=<number>` in the environment for reproducible Skill Level move choices.

### Measured strength of every Skill Level

<!-- ELO_TABLE -->
Measured with `testing/calibrate.py` (status: **complete**, 2026-11-09 04:41:28 UTC). Every level played Stockfish 16 with `UCI_LimitStrength` (the only fixed ratings) and its neighbouring levels; all games were rated together by maximum likelihood with the Stockfish levels fixed at their nominal `UCI_Elo`.

**These are estimates on the Stockfish 16 `UCI_Elo` scale at 10 s + 0.1 s on the test machine, with the default personality. They are not FIDE or human ratings, and not CCRL ratings.** See the caveats below.

| Skill Level | Intended strength | Estimated Elo | 95% CI | Games (W/D/L) | Confidence |
|---:|:---|---:|:---:|:---:|:---|
| 1 | Extremely weak | 182 | [-10, 316] | 170 (54/7/109) | extrapolated |
| 2 | Beginner | 225 | [59, 360] | 218 (78/9/131) | extrapolated |
| 3 | Beginner | 364 | [212, 475] | 212 (96/6/110) | extrapolated |
| 4 | Beginner | 558 | [418, 664] | 184 (100/7/77) | extrapolated |
| 5 | Lower intermediate | 575 | [412, 666] | 180 (76/7/97) | extrapolated |
| 6 | Lower intermediate | 613 | [471, 700] | 180 (66/5/109) | extrapolated |
| 7 | Lower intermediate | 745 | [617, 826] | 250 (82/6/162) | extrapolated |
| 8 | Intermediate | 824 | [728, 906] | 180 (73/4/103) | extrapolated |
| 9 | Intermediate | 988 | [901, 1058] | 180 (93/5/82) | extrapolated |
| 10 | Intermediate | 1023 | [953, 1078] | 216 (77/7/132) | extrapolated |
| 11 | Upper intermediate | 1076 | [1015, 1140] | 194 (65/6/123) | extrapolated |
| 12 | Upper intermediate | 1135 | [1070, 1186] | 184 (53/8/123) | extrapolated |
| 13 | Upper intermediate | 1294 | [1245, 1338] | 184 (80/11/93) | anchored |
| 14 | Advanced | 1336 | [1290, 1390] | 184 (79/11/94) | anchored |
| 15 | Advanced | 1366 | [1327, 1418] | 184 (71/13/100) | anchored |
| 16 | Advanced | 1464 | [1410, 1511] | 184 (69/17/98) | anchored |
| 17 | Strong | 1539 | [1485, 1595] | 184 (76/14/94) | anchored |
| 18 | Strong | 1565 | [1508, 1620] | 186 (72/12/102) | anchored |
| 19 | Strong | 1681 | [1629, 1727] | 200 (73/15/112) | anchored |
| 20 | Near full strength | 1758 | [1710, 1813] | 200 (80/22/98) | anchored |
| 21 | Full engine strength | 1845 | [1790, 1906] | 172 (88/21/63) | anchored |

**Monotonicity:** every level is rated above the one below it.

Confidence labels: *anchored* = played the Stockfish anchors directly and lies inside their range (1320–2500); *extrapolated* = more than 100 below the lowest anchor, so the number rests on the chain of games between neighbouring levels and the logistic Elo model; *low-sample* = fewer than 100 games.

**Time-control check.** Some levels were measured again at 60+0.6 (the slower time control associated with Stockfish's `UCI_Elo` calibration) against their nearest anchors:

| Skill Level | Elo at 10+0.1 | Elo at 60+0.6 | 95% CI at 60+0.6 | Drift | Games (W/D/L) | Confidence |
|---:|---:|---:|:---:|---:|:---:|:---|
| 6 | 613 | 101 | [−∞, 384] | -511 (CI unbounded) | 60 (1/0/59) | low-sample, chained via L11 |
| 11 | 1076 | 809 | [−∞, 968] | -268 (CI unbounded) | 120 (62/0/58) | anchored |
| 16 | 1464 | 1295 | [1182, 1385] | -169 ± 113 | 60 (19/4/37) | low-sample |
| 19 | 1681 | 1631 | [1530, 1723] | -50 ± 109 | 60 (31/3/26) | low-sample |
| 21 | 1845 | 1730 | [1619, 1813] | -115 ± 113 | 60 (24/1/35) | low-sample |

Drift = rating at the slower time control minus rating at the main one, both against the same fixed Stockfish anchors; the ± is an approximate 95% interval. Drifts clearly different from zero: level 16 (-169), level 21 (-115); for the others the data cannot tell a change from none.

**Test environment.** Intel(R) Core(TM) Ultra 5 125H (18 logical CPUs, 12.5 GB RAM), Linux-6.8.0-147-generic-x86_64-with-glibc2.39; 12 games at a time, one thread per engine, 16 MB hash; opponents Stockfish 16 with `UCI_LimitStrength` at UCI_Elo 1320, 1500, 1700, 1900, 2100, 2300, 2500; time control 10+0.1; openings from `8moves_v3.pgn` (each played twice with colours reversed); 2688 games at 10+0.1 plus 300 at 60+0.6; engine sha256 `90c49ff0e3403197…`. Full report: `testing/results/elo_calibration.md`.

**Caveats.** (1) The scale is Stockfish's: its `UCI_Elo` was fitted by the Stockfish team against the Stash engine at 120 s + 1 s and is described as roughly CCRL Blitz (older Stockfish READMEs say 60 s + 0.6 s, CCRL 40/4); at 10 s + 0.1 s on this machine the absolute numbers may be shifted. (2) Levels below 1320 are extrapolated through the chain. (3) The ratings belong to the default personality and this hardware; a different personality, time control or machine changes them, especially for levels 19–21, which use the clock. Re-run `testing/calibrate.py` to measure your setup.
<!-- /ELO_TABLE -->

**Why the levels "drift" at 60+0.6.** Levels 1–18 have a fixed node budget, so they play the same at any time control: in the calibration run of 2026-11-09, Skill Level 11 averaged 18 ms per move at 10+0.1 and 17 ms at 60+0.6. Stockfish's limited strength does use the extra time: its `UCI_Elo 1320` setting averaged 0.30 s per move at 10+0.1 and 2.0 s at 60+0.6, and its score against the same Skill Level 11 rose from 77% (70 games) to 95% (60 games). The negative drifts therefore mostly show the anchors getting stronger with time, and the table's numbers are tied to 10+0.1: against Stockfish at longer time controls the weakened levels rate lower.

## Recommended configurations

| Purpose | Skill Level | Aggression | Sacrifice Appetite | King Attack Priority | Material Preservation | Speculative Sacrifices | Notes |
|---|---|---|---|---|---|---|---|
| **Ultimate Tal** (default) | 21 | 90 | 95 | 90 | 15 | 90 | Full search strength, the full sacrificial personality. |
| **Kamikaze** | 21 | 100 | 100 | 100 | 0 | 100 | Every dial at the top: the most sacrifices and the most chaos. Expect it to be weaker than the default (not measured). For fun. |
| **Classic AggroChess 4.5** | 21 | 60 | 75 | 60 | 50 | 50 | Identical search to AggroChess 4.5.0: one big sacrifice per game, then near-sound play. AggroChess 4.5.0 scored 68% against the default (100 games at 10+0.1, about +131 Elo). |
| **Sound attacker** | 21 | 70 | 55 | 75 | 50 | 30 | Attacks hard, sacrifices mostly when it can see it through (not measured separately). |
| **Analysis partner** | 21 | 0 | 0 | 0 | 50 | 0 | Personality off: no sacrifice bonus, symmetric evaluation, no contempt. Use with `MultiPV` 3–5. |
| **Club sparring** | 10–14 | 90 | 95 | 90 | 15 | 90 | A beatable, very sharp opponent. |
| **Beginner coach** | 3–7 | 90 | 95 | 90 | 15 | 90 | Recognisable attacking chess with sacrifices you can learn from (and punish). |
| **Gentle beginner** | 1–3 | 40 | 40 | 50 | 60 | 30 | Weak and less wild. |

## Sacrifice testing

<!-- SAC_TABLE -->
Measured with `testing/sactest.py` (2026-11-09): the Ultimate Sacrificial Edition with the default personality and Skill Level 21 (**new**, sha256 `90c49ff0e340…`) against AggroChess 4.5.0 (**old**, `a010a1024c43…`). Each played 60 games against each of Stockfish 16 at UCI_Elo 1800, 2000 (same openings and colours for both), plus 100 games head-to-head; time control 10+0.1, one thread each, 14 games at a time. Every game was then analysed by `aggro_arena analyze`, with Stockfish checking each sacrifice.

| Match | Games | W/D/L | Score | Elo difference [95% CI] |
|---|---:|:---:|---:|:---:|
| new vs sf1800 | 60 | 22/13/25 | 47.5% | -17 [-98, 61] |
| new vs sf2000 | 60 | 20/3/37 | 35.8% | -101 [-200, -16] |
| old vs sf1800 | 60 | 39/8/13 | 71.7% | +161 [79, 264] |
| old vs sf2000 | 60 | 38/2/20 | 65.0% | +108 [22, 208] |
| new vs old | 100 | 25/14/61 | 32.0% | -131 [-204, -68] |

| Metric | new | old |
|---|---|---|
| **Sacrifices per 100 engine moves in balanced positions** | 4.86 | 3.23 |
| Material given per 100 balanced moves (sacrifices + blunders) | 8.85 | 5.50 |
| Sacrifices in balanced positions, per game | 1.02 [0.88–1.16] | 0.65 [0.53–0.76] |
| Games with a sacrifice in a balanced position | 138/220 = 62.7% [56–69%] | 102/220 = 46.4% [40–53%] |
| Sacrifices per game (all positions) | 1.81 [1.64–1.98] | 1.54 [1.38–1.70] |
| Queen / exchange / piece / pawn sacrifices | 11 / 19 / 151 / 217 | 10 / 22 / 104 / 203 |
| Chained sacrifices (Tal chains) | 39 | 27 |
| Average move number of a sacrifice | 29.5 [27.6–31.3] | 34.9 [32.6–37.2] |
| Stockfish eval before the sacrifice (cp, engine's view) | -140 [-225–-54] | 198 [72–324] |
| Sacrifices followed by mate within 30 plies | 39/398 = 9.8% [7–13%] | 77/339 = 22.7% [19–27%] |
| Successful attacks (mate, material back with the attack still on, or Stockfish +1.5 twelve plies later) | 143/398 = 35.9% [31–41%] | 176/339 = 51.9% [47–57%] |
| Objectively unsound (Stockfish: worse by 1.5+ pawns and below −1) | 72/398 = 18.1% [15–22%] | 16/339 = 4.7% [3–8%] |
| …of those, practical chances (still scored, or Stockfish later swung to +1) | 34/72 = 47.2% [36–59%] | 10/16 = 62.5% [39–82%] |
| Blunders per game (own eval collapsed after the reply) | 1.95 [1.77–2.13] | 1.37 [1.24–1.51] |
| Score in all analysed games | 0.373 [0.311–0.438] | 0.682 [0.618–0.740] |

Balanced = the engine's own score within ±3 pawns when it moved. Intervals are 95% (Wilson for proportions, normal for means); sacrifices within one game are treated as independent, so event-level intervals are somewhat optimistic. A *sacrifice* is a material deficit of about two pawns or more (one pawn for a gambit) that the engine started and that persists for two moves, which the engine's own evaluation saw coming; when its evaluation collapses instead, it is counted as a *blunder*. Moves made while being mated, forced moves and trades are excluded. The full report, with per-opponent and per-category tables and example games, is [`testing/results/sacrifice_report.md`](testing/results/sacrifice_report.md); the detection rules are in [`testing/README.md`](testing/README.md).
<!-- /SAC_TABLE -->

**Reading the results.** The default personality does what it was built for: in positions its own search sees as balanced it starts a sacrifice about 50% more often than AggroChess 4.5.0, sacrifices pieces more often, chains sacrifices, and sacrifices about five moves earlier, typically from roughly equal positions (Stockfish −1.4 on average) where 4.5.0 mostly sacrificed when already better (+2.0). Those sacrifices are much more speculative: Stockfish judges 18% of them clearly unsound (4.5.0: 5%), fewer end in mate, and the engine scores about 30 percentage points less than 4.5.0 against the same opponents. That is the trade the Ultimate Sacrificial Edition makes on purpose. It also blunders more by the analyser's definition (its own evaluation collapses after the reply), largely because its optimistic evaluation of its own attacks is corrected a move later; Stockfish confirms only a quarter of those as real blunders (4.5.0: half).

During development a first default (a larger, permanent sacrifice bonus) was measured and rejected: it gave up material more often but mostly in four-pawn howlers, produced *fewer* purposeful sacrifices per move than 4.5.0 and lost about 160 Elo. The current mapping tolerates sacrifices up to about 1.8 pawns below the best alternative all game (3.4 pawns for the game's first sacrifice). For even more sacrifices, raise Sacrifice Appetite and Speculative Sacrifices to 100 (expect more unsound ones); for fewer, lower them, down to the Classic preset.

## How it plays

* **Sacrifices, and plans them:** a sacrifice aimed at your king (or an exchange sacrifice) earns a bonus, scaled by how many attackers stand behind it, as long as the line stays above the floor. The bonus also applies at full strength to its next move and at half strength to the move after, so it steers towards positions where a sacrifice will be available. Until it has sacrificed in a game, the appetite keeps growing; with the default personality a large part of it stays afterwards, and a sacrifice a move or two after another one (a Tal chain) keeps an even bigger bonus while the first one is working.
* **Gambits:** besides the built-in gambit book, a pawn given for the attack or to open lines in the middlegame earns a smaller gambit bonus.
* **Attacks:** a Stockfish-style king-danger model that grows quadratically with the attackers, safe checks and holes around your king, multiplied (250% by default) for the danger it creates against *your* king, not how it judges its own. Pieces get credit for aiming at your king early, rooks and queens for open files next to it, pawns for storming it and for clamping it from h6/a6.
* **Initiative:** with Aggression above the classic value it values protected pieces planted in your half and every piece of yours it attacks.
* **Material:** with Material Preservation below 50, being a piece down while its pieces swarm your king feels much less bad, so it keeps attacking instead of scrambling for the material.
* **Forcing moves first:** when moves are close, it prefers checks, captures and moves into your king zone.
* **Avoids draws:** contempt 75 by default (50 in 4.5.0), so it treats a draw as a loss and plays on.
* **Finishes beautifully:** with a forced mate, it prefers a mate that starts with a sacrifice (a queen sac above all) if that still mates within two moves of the fastest one. Instead of a plain Re8#, you get Qd8+! Bxd8 Re8#. This works at every Skill Level.
* **Swindles when worse:** at full strength, between about 1.5 and 25 pawns down, it looks at its best move plus up to five that are nearly as good, checks every reply you could make to each, and plays the one where most of your natural replies would throw your advantage away.
* **Keeps pieces on:** every trade costs it, queen trades a lot, and more again when it's behind. Only real trades count: a sacrifice is not a trade.

### Opening book

About 300 hand-picked lines covering both colours, all built in (no file needed):

* **White:** King's Gambit, Evans Gambit, Danish Gambit, Scotch Gambit, Fried Liver, Smith-Morra, Vienna Gambit, Blackmar-Diemer, Milner-Barry (vs the French), Alekhine-Chatard, Staunton Gambit (vs the Dutch), the Tennison Gambit and the Halloween Gambit, plus sharp English lines after 1.c4.
* **Black:** the three-pawn Scandinavian (1.e4 d5 2.exd5 c6 3.dxc6 Nf6 4.cxb7 Bxb7), Stafford, Traxler, Schliemann, Latvian Gambit, Albin Counter-Gambit (with the Lasker trap), Englund Gambit, Benko, Budapest and From's Gambit, plus answers to 1.c4, 1.Nf3, 1.f4, 1.b3, 1.g3, 1.b4 and other offbeat first moves.

Every line is checked for legality by a test, and every book move was screened with a neutral analysis to remove real blunders. The gambits stay in.

## Under the hood

* **Evaluation (`src/nnue.rs`, `src/eval.rs`):** an NNUE with 768 inputs, a 256-neuron hidden layer for each side, SCReLU activation and 8 output buckets, updated incrementally, trained only on AggroChess's own self-play games. The personality layer (only for the engine's own side) is added on top: the extra king danger it creates, pieces aimed at the king, heavy pieces near it, the king clamp and pawn storms (King Attack Priority), trade aversion, activity and threats (Aggression), and the material term (Material Preservation). The classical evaluation (PeSTO tables, pawn structure, mobility, threats, king danger) takes over against a bare king and in small pawnless endgames.
* **Personality mapping (`src/personality.rs`):** turns the five 0–100 controls into the internal weights and search parameters, so that the Classic values give exactly 4.5.0's numbers.
* **Search (`src/search.rs`):** iterative deepening, aspiration windows, principal variation search, a lock-free shared hash table and Lazy SMP. Null-move pruning, reverse futility pruning, razoring, late move reductions and pruning, futility pruning, singular extensions, check and sacrifice extensions, lenient static-exchange pruning for moves into the enemy king zone, and the long-think features of 4.3–4.5 (Tal's look, forcing-line priming, the mate prover, the rescue look). New: root MultiPV, the Skill Level budget, an external stop that reaches every side search, and an uninterruptible emergency search when a `stop` arrives before depth 1 is finished.
* **Skill Level (`src/skill.rs`):** the level table and the move choice described above.
* **Training (`src/datagen.rs`, `trainer/`):** `aggro_chess datagen` plays self-play games with the personality switched off (whatever the personality options say) and writes positions labelled with the search score and the game result; the trainer writes `aggro.nnue`, which is built into the engine. See the original README for the retraining steps.

## Testing tools

Everything is in `testing/` and needs only Rust and Python 3 (standard library); see [`testing/README.md`](testing/README.md).

| Tool | What it does |
|---|---|
| `testing/arena` (`aggro_arena`) | Match runner (`match`), UCI conformance test (`ucitest`), opening converter (`openings`), sacrifice analyser (`analyze`), result summaries (`summary`). |
| `testing/calibrate.py` | Recalibrates the Skill Level ratings against Stockfish's `UCI_Elo` anchors (scout, main and time-control-check stages; resumable). |
| `testing/elo.py` | Joint maximum-likelihood ratings with anchors, bootstrap confidence intervals. |
| `testing/sactest.py` | Sacrifice comparison of two engines (or two personalities) against the same opponents and head-to-head. |
| `testing/regress_classic.py` | Proves the Classic preset searches exactly like AggroChess 4.5.0. |

To recalibrate the Skill Levels (for example after changing the defaults or on other hardware):

```bash
cd testing/arena && cargo build --release && cd ../..
python3 testing/calibrate.py --engine target/release/aggro_chess --games 140 --concurrency 14 \
    --tc-check 60+0.6 --tc-levels 6,11,16,19,21 --tc-games 60
# results: testing/results/elo_calibration.md / .json ; interrupted runs continue with --resume
```

To re-run the sacrifice comparison:

```bash
python3 testing/sactest.py --new target/release/aggro_chess --old <path to AggroChess 4.5.0> --games 80 --h2h-games 80
```

## History

AggroChess 1.0–4.5 by Rinarto Gibson introduced the sacrifice bonus, the gambit book, the NNUE trained on its own games (4.0), the "no cheap sacrifices" rule (4.1), Tal's look, chained sacrifices and the king clamp (4.3), and the rescue look and sharper long-think search (4.5). The release notes are in `RELEASE_NOTES_v4.*.md` and the full 4.5.0 README in `docs/`.

## License

GNU General Public License v3.0, as the original AggroChess (`LICENSE`). This modified version is distributed under the same license; the original copyright and attribution are kept.

---
AggroChess © 2026 SHIFT / Rinarto Gibson · [Play AggroChess online](https://playaggro.netlify.app/) · [Discord](https://discord.com/invite/uwrQMSsjRM)
