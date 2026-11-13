# AggroChess Ultimate Sacrificial Edition 5.2 ⚔️

**AggroChess Ultimate Sacrificial Edition** (USE) is a modified version of [AggroChess](https://github.com/PhelRin/AggroChess) 4.5.0 by Rinarto Gibson, the aggressive, sacrificial UCI chess engine written in Rust in the spirit of Mikhail Tal. It keeps AggroChess's search, NNUE evaluation, gambit book and UCI implementation, and adds:

* **a more sacrificial default personality**: in balanced positions about 1.5 times as many purposeful sacrifices per move as AggroChess 4.5.0 (4.9 against 3.2 per 100 moves), more piece sacrifices and Tal chains, sacrifices earlier in the game and from equal positions rather than won ones, and an appetite for sacrifice that no longer switches off after the first one. The price is measured too: more of its sacrifices are objectively unsound, and 5.0 was about 130 Elo weaker than 4.5.0 head-to-head (see [Sacrifice testing](#sacrifice-testing)); 5.1 measured about 80 Elo stronger than 5.0 (see [Brilliance](#brilliance));
* **five personality controls** as UCI options: `Aggression`, `Sacrifice Appetite`, `King Attack Priority`, `Material Preservation`, `Speculative Sacrifices`;
* **human-like pacing and style** (`Human Think Time`, `Human Style`, 5.2, both off by default): the reply is held back by a human think calibrated to the position and the time control, and near-equal moves are chosen the way a person chooses (keeping every measured rating valid until you switch it on);
* **a brilliancy check** (`Brilliance`, 5.1): a second look after every timed search that swaps a quiet move for a *sound* sacrifice when one is nearly as good, and an unsound sacrifice for a clearly better safe move;
* **a `Skill Level` 1–21 strength limiter** in the style of Patricia's, independent of the personality, with a [measured rating for every level](#measured-strength-of-every-skill-level);
* standard GUI options (`Hash`, `Clear Hash`, `MultiPV`, `Move Overhead`) and fixes for `go infinite` and `stop`;
* a self-contained **testing system** (`testing/`): a match runner, a UCI conformance test, a sacrifice analyser and an Elo calibration tool.

Setting the personality to the *Classic* values reproduces AggroChess 4.5.0's search exactly (same moves and node counts), so the original engine is one preset away.

> This is a modified version of AggroChess, distributed under the GNU GPL v3 (see `LICENSE`). Modifications by the Ultimate Sacrificial Edition, November 2026: personality options, Skill Level, MultiPV, the brilliancy check, human think time & style, UCI fixes and the testing tools. The original 4.5.0 README is kept in [`docs/README_AggroChess_4.5.0.md`](docs/README_AggroChess_4.5.0.md).

## Installing it in a chess GUI

AggroChess is a standard UCI engine: one executable, with the neural network and the gambit book built in.

1. Build it (see below), or unpack a prebuilt package from `dist/`
   (`aggrochess-use-*-windows-x86_64-avx2.zip` needs a 64-bit CPU with AVX2, essentially any desktop CPU from 2013 onwards; the `linux-x86_64-meteorlake` build additionally uses the AVXv2 set of Intel's 12th-generation-or-later cores and is best on those). Verify a download with `./aggro_chess bench` — it must print `287284 nodes`.
   `ph-gambitbook.bin` next to the executable (or in the GUI's working directory for the engine) is optional: a Polyglot book used after the built-in gambit lines run out.
2. Add it to your GUI:
   * **Cute Chess**: *Tools → Settings → Engines → +*, choose the executable, protocol UCI. The options appear under *Advanced*.
   * **Arena**: *Engines → Install New Engine…*, pick the executable, confirm UCI. Options: *Engines → Manage → Details → UCI*.
   * **Banksia GUI**: *Engines → Add*, select the file; the options are in the engine's configuration panel.
   * **lichess-bot, Coliseum, Nibbler, En Croissant** and other UCI front ends work the same way.
3. Set `Threads`, `Hash` and, if you like, the strength and personality options described below.

### Hand-relay operator recipe (playing other bots by hand)

The scenario this edition was tuned for: you relay the engine's moves by hand, on another board or site, at a fixed thinking time. Set exactly this once:

| Option | Value | Why |
|---|---|---|
| `Human Think Time` | 100 | full human pacing |
| `Human Style` | 70 | natural move choice among near-equals |
| `Human Base Minutes` | 5 | the frame you are actually playing at |
| `Human Increment Seconds` | 0 | |
| `Relay Hint` | 2000 | `info string relay hint: <move>` appears 2.00 s before the reply |

Then send it fixed `go movetime 300` per move (or use the GUI's fixed-time mode). Opening moves, recaptures and free pieces are answered in a blink; quiet middlegame moves think 1–3 s; real sacrifices get the long human look. The engine also answers `go ponder` / `ponderhit` correctly, so pondering GUIs stay in sync.

## Building from source

1. Install [Rust](https://www.rust-lang.org/) (stable).
2. Build:
   ```bash
   cargo build --release
   ```
3. The binary is `target/release/aggro_chess` (`aggro_chess.exe` on Windows).

`cargo test --release` runs the test suite (49 tests, plus 2 ignored developer scans: mates, the Greek Gift, sacrificial mates, the Albin pawn that must *not* be sacrificed, swindle timing, every opening-book line, SEE, evaluation symmetry, incremental network updates, the personality mapping, every Skill Level playing legal moves in time, Skill Level never touching the personality, …). `aggro_chess bench` prints a node count and speed (287284 nodes with the default personality).

## UCI options

| Option | Type | Default | Range | What it does |
|---|---|---|---|---|
| `Threads` | spin | logical CPUs (max 8) | 1–128 | Search threads (Lazy SMP). Skill Levels 1–20 always search with one thread. |
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
| `Brilliance` | spin | **100** | 0–100 | The brilliancy check: plays a sacrifice that is objectively nearly as good as the best move, and drops one that a safe move clearly beats. 0 = off. See [Brilliance](#brilliance). |
| `Human Think Time` | spin | **0** | 0–100 | How strongly the engine *pauses before answering* like a human would, out of the clock it has been given. **0 = off** (replies the moment the search ends, exactly as before). |
| `Human Base Minutes` | spin | **0** | 0–480 | The time frame you are playing (1, 3, 5, 15 …). 0 = auto: the pace is learned from the clock the GUI reports instead. |
| `Human Increment Seconds` | spin | **0** | 0–60 | The increment of that time frame. |
| `Human Style` | spin | **0** | 0–100 | Among moves the search treats as nearly equal, prefer the natural human choice (development, checks, captures, centre, sacrifices). 0 = off. |
| `Relay Hint` | spin | **0** | 0–10000 ms | Hand-relay setups: show the chosen move this many ms before the think ends (`info string relay hint`). 0 = off. |
| `BookPath` | string | ph-gambitbook.bin | | Optional Polyglot book used after the built-in gambit lines. |

`Skill_Level` (Patricia's spelling) is accepted as an alias of `Skill Level`. Changing a personality option clears the hash table (stored scores contain the old personality).

The scores the engine reports **exclude the sacrifice bonus** (AggroChess 4.5.0 included it), so a GUI and its adjudication see the engine's objective view of a sacrificial line. They still contain the personality's evaluation terms, so with the default personality the engine is somewhat optimistic about its own attacks; for neutral analysis use the *Analysis partner* settings below.

## Brilliance

New in 5.1, extended in 5.4. After the main search of a timed move, the engine sorts the legal moves into *brilliant* candidates and the rest. A candidate is

* a **material sacrifice** — the moved piece is lost by static exchange (a minor piece for pawns, the exchange or more) or the move leaves something worth more than two pawns en prise. This covers the Tal/Qxh7+ kind, capturing (`Rxh7+!`) or quiet (`Qh6!!`), the exchange sacrifice, and the **desperado** (a capture whose piece cannot be saved);
* a **knight, bishop or rook promotion**. The ordinary search ranks these last and the quiescence search never looks at them, so the brilliancy look is the only thing that can ever play one — Saavedra's `1.c8=R!!` is the test position.

The look then searches the candidate group and the best ordinary move to the same depth with the **neutral evaluation** (personality off, no sacrifice bonus, no contempt) on a fresh hash table. The neutral evaluation matters: the attacking personality is optimistic about its own attacks, which is exactly what makes a sacrifice unsound, so it does not get to judge the sacrifice.

* the search chose a quiet move, and the best sacrifice is at most *margin* below it and not losing (above *floor*) → **the sacrifice is played**: a sound sacrifice of the kind chess.com marks as "brilliant";
* the search chose a sacrifice worth less than +3 pawns, and the best safe move is more than *veto* above it → **the safe move is played**: the sacrifice was unsound;
* a **underpromotion** rates no worse than the best ordinary move (with a little more room when it checks) → **the underpromotion is played**;
* in a bad position, the search's second-best candidate is far below its best → that best move is the **only move that holds**, and it is played (`info string brilliant defence: …`).

| Brilliance | share of the move the look may spend | margin | floor | veto |
|---|---|---|---|---|
| 0 | — (off) | — | — | — |
| 25 | 1/4 of 30% | 5 cp | +25 cp | 238 cp |
| 50 | 1/2 of 30% | 20 cp | 0 | 175 cp |
| 75 | 3/4 of 30% | 45 cp | −25 cp | 112 cp |
| **100** (default) | 30% | 80 cp | −50 cp | 50 cp |

The engine prints `info string brilliancy look: …` for each check. At full strength the look runs only in timed games (not with `go depth`, `go nodes`, `go infinite`, MultiPV > 1), so the Classic preset with `go depth` still matches 4.5.0 exactly; for 4.5.0 behaviour in games, set Brilliance to 0 as well.

### At every Skill Level

From 5.4 the look runs at Skill Levels 1–20 too, scaled by `Brilliance`, with two deliberate differences:

* **it gets its own verification budget.** A weakened engine's own budget cannot judge a sacrifice — level 5 searches about 600 nodes at depth 4, and Saavedra's stalemate point is nine plies away. The look therefore runs a separate search of about 30 000 nodes at most (a few milliseconds), *not* taken from the level's allowance, so the level's own move and its measured strength are untouched. With Brilliance 0 the look adds nothing; a level whose own search can already see the brilliancy (levels 10–21 in the Saavedra position at 300 ms) still plays it on its own merits.
* **it may only promote a brilliancy, never demote the level's own move.** Measured: at level 8 a depth-3 neutral look rejected the Greek gift (`Bxh7+`), which a look that shallow has no right to do. So at a Skill Level the look plays a sacrifice or underpromotion the neutral search endorses, but never vetoes the level's choice. The only-move defence is the exception, and it is not a judgement call: when the second-best candidate is far below the best in a bad position, that gap *is* the proof that the first move is the only one that holds.

### Measured effect

Measured 2026-11-09, 10+0.1, one thread, same openings and colours (seed 1), analysed with `aggro_arena analyze` + Stockfish 16 (400k nodes per position) and counted with `testing/brilliant.py` (tables in [`testing/results/brilliance.md`](testing/results/brilliance.md)). "Brilliant-style" = a piece, exchange or queen sacrifice (pawns don't count) that Stockfish rates as costing at most 50 cp and that does not leave the engine worse than −1; the strict count also requires that the position was not already won (Stockfish < +3 before it), as chess.com does. 5.0 is the previous default build (Brilliance did not exist).

| build | games vs SF 1800 | score vs SF 1800 | piece+ sacrifices | sound by SF | brilliant-style per game (strict) | brilliant-style per game (incl. won positions) |
|---|---|---|---|---|---|---|
| 5.0 (no brilliancy check) | 40 | 46.2% | 29 | 21 (72%) | 0.12 | 0.17 |
| 5.1, Brilliance 75 | 60 | 62.5% | 34 | 31 (91%) | 0.13 | 0.30 |
| **5.1, Brilliance 100** | 60 | 62.5% | 40 | 32 (80%) | 0.15 | 0.37 |

Head-to-head, 5.1 (Brilliance 75) against 5.0: **+42 =14 −24, 61.3%, about +80 Elo** (80 games). The brilliancy check removes unsound sacrifices, which makes the engine both stronger and more often *right* when it sacrifices, and it plays about twice as many sound, brilliant-style sacrifices as 5.0.

What it cannot do is produce several sound sacrifices *every game*: a sacrifice is only sound when the position offers one, and in these games that was the case about once every three games. The counter also misses sacrifices the opponent declined (a piece left en prise that is never taken), which chess.com still marks as brilliant. These are small samples (40–60 games) with wide error bars. The Skill Level ratings below were measured with 5.0, so Skill Level 21 is now somewhat stronger than its listed rating. From 5.4 the brilliancy look also runs at levels 1–20, on its own small verification budget (see [At every Skill Level](#at-every-skill-level)): it can add a brilliant move the level could see, but it never changes the level's own search or vetoes its choice, so the ladder below keeps its shape.

## Human think time

`Human Think Time` makes the engine pause before sending its `bestmove`, imitating a person
thinking over the board. With it **off** (the default) nothing changes at all: same move,
same instant reply. Turn it on once in your GUI's engine settings and set the time frame you
play (`Human Base Minutes`, `Human Increment Seconds`; or leave both 0 and the engine reads
the rhythm from the clock the GUI reports on every move).

It is a pure act: **the move is always exactly the one the search picked**, printed later.
The pause is calibrated to the position:

* **no-brainers are played instantly** — a single legal reply, a free piece snapped up, a
  clean recapture on the square that was just taken, castling: a glance (~0.1–0.5 s), not a think;
* **the opening is played from theory** — the first moves get a fraction of the normal
  think, easing up to a normal think by move 8 (a long "I'm out of book" think only when
  the position is genuinely sharp);
* **a decisive advantage is converted briskly** (up +4 or more);
* complexity is counted only where the search is deep enough to have an opinion: a
  small-budget search (fixed 300 ms per move) wobbling between moves is *noise*, not
  difficulty — so against lower-rated bots and in simple positions the replies stay fast.
* every move otherwise gets a small human "look" (~1–3 s in a 5-minute game),
* wide and genuinely sharp positions (deep-search instability, real tactics) think longer,
* a move the engine itself sees as a sacrifice still gets the long Tal-style think,
* per-move randomness with mean 1 (about a quarter of moves brisk, the rare 2–3x "really
  sat on it" move) so no two games share a rhythm.

In a 5+3 game with `Human Think Time` 100 the replies land in roughly 1–20 s; at 1+1 the
same shape scales down to ~0.3–4 s.

Fixed-time games (`go movetime`, the "time limit per move" mode some GUIs and bots use)
act too. The engine has no clock to read there, so set `Human Base Minutes` to the game you
are playing: a 5+0 platform game driven through a 300 ms-per-move search thinks in the
5-minute rhythm. With the realism rules above, the same 300 ms rig measured (Think Time
100): opening moves ~0.1–0.5 s, quiet middlegame ~1.5–2.5 s, a hard Najdorf tabiya ~5 s,
and a sharp sacrificial position reaches the long Tal thinks — while a free queen is still
snapped up in half a second. Without a frame the movetime itself implies one
(movetime × 40), capped at **5× the movetime** so a plain fixed-time caller always gets a
reply within fixed-time patience.

**`Relay Hint` (ms, 0 = off)** — for hand-relay setups: you operate the engine by hand,
playing its moves on another board or site (engine-vs-bot testing, exhibition relays). The
chosen move is printed on an `info string relay hint: <move>` line this many milliseconds
*before* the think ends, so you can read and relay it while the pause finishes: with 2000
the hint lands 2.00 s before `bestmove` (measured). It changes nothing except when the
operator sees the move; the move itself and the `bestmove` line are untouched.

**It cannot lose on time, structurally:**

* the pause never exceeds 1/12 of the remaining clock, *and* below ~2–3 s left (itself
  scaled by the time frame) the engine stops acting and replies instantly;
* `go infinite`, `go movetime`, `go depth`, `go nodes` and `searchmoves` are never delayed —
  analysis stays instant;
* book moves are not acted on: the opening is answered from theory, at once. Your first
  few book moves reply instantly by design; when the engine leaves the book it eases in
  with the short theory-glances of the realism rules, so there is no cliff;
* a `stop` from the GUI during the pause ends it immediately;
* a genuinely long search already *is* the think: the pause shrinks by the search time, and
  never pads beyond the target.

Measured (5+3 and 1+1 games, `AGGRO_HUMANDBG=1` shows each pause): search of 2.0 s in a
1+1 game paused 4.7 s more (complexity 36/100); the same engine down to its last 5 s
paused 0.46 s (complexity 100) and stopped acting below that; a 14.6 s deep-mode think was
padded by only 2.2 s.

`Human Style` (same family, separate switch) is the move-choice side of human-like play:
when several moves are objectively within ~0–25 cp of each other, the engine prefers the
one a human reaches for — developing a minor piece, castling, central pawn pushes, checks
and captures, offering material (this is still AggroChess). It only re-ranks near-equal
candidates the full search produced (the search itself, the brilliancy check and the Tal
looks are untouched), so in sharp positions nothing changes: it still plays the Greek gift
with `Human Style` at 100 (a regression test asserts this).

## Personality settings

Five controls, each 0–100, independent of the Skill Level and of each other. With the **Classic** values (Aggression 60, Sacrifice Appetite 75, King Attack Priority 60, Material Preservation 50, Speculative Sacrifices 50) the engine searches exactly like AggroChess 4.5.0: same moves, same principal variations, same node counts (checked by `testing/regress_classic.py`). The defaults go well beyond that.

| Control | Low values | High values | What it changes inside the engine (Classic → default) |
|---|---|---|---|
| **Aggression** | calm, happy to trade and to draw | keeps pieces and queens on, avoids draws, wants activity and threats | trade-aversion weight 120 → 180; contempt 50 → 75; new bonus for protected pieces in your half and for attacked enemy pieces (0 → 60); forcing tie-break 12 → 18 |
| **King Attack Priority** | the attack on your king counts objectively | your king is what it plays for | extra king danger 220% → 250% (capped at 22 pawns); pieces aimed at the king zone, heavy pieces on open files near the king and the h6/a6 king clamp 120 → 150; new pawn-storm bonus; king-zone moves searched earlier (2500 → 3250). Above 60 the slope is half that below it: the steeper mapping was measured to cost about 140 Elo on its own |
| **Sacrifice Appetite** | sacrifices only when they are sound anyway | hungry for sacrifices all game | sacrifice bonus for the game's first sacrifice 300 → **20** (measured: at 340 it bought junk inside the badge window; the pressure ramp still adds up to +450 later, so the first sacrifice is nearly sound and the appetite grows through the game); **appetite after it 60 → 180** (4.5.0 nearly switched it off: one sacrifice per game, then near-sound play); chained sacrifices 150 → 190; hunger for a first sacrifice starts at move 20 → 14; new gambit bonus for pawn sacrifices aimed at the king or opening lines (0 → 40) |
| **Speculative Sacrifices** | sacrifices must be nearly sound | will sacrifice into objectively worse positions | a sacrificial line may score down to −3.1 → −4.3 pawns and still earn the bonus; full bonus with 3 → 2 attacking pieces behind it (partial bonuses 60%/25% → 76%/35%); leaving material hanging at the root counts as a sacrifice (off → on); its checking sacrifices survive pruning longer (200 → 320 cp per ply) and its sacrifices are extended from depth 4 → 3 |
| **Material Preservation** | a piece down with an attack feels fine | material is sacred | 50 is neutral (4.5.0). Below 50, up to 30% of a material deficit is forgiven while its pieces bear on your king (default 15: up to 21%); above 50 a deficit costs up to 25% extra |

**Sacrifice versus blunder.** The bonus only goes to a move that gives up about two pawns or more (about one pawn for the gambit bonus) **and** is aimed at the king with attackers behind it, or is an exchange sacrifice, or leaves material for an attack; and only if the line it starts stays above the floor. The size of the bonus is the engine's tolerance for unsoundness: with the default it will play a sacrifice its own search judges up to about 0.2 pawns worse than the best quiet move when the game is young, rising to about 1.8 pawns later as the pressure ramp builds, but never the four-pawn give-aways a bigger appetite produced in testing. The first-sacrifice bonus was measured to be the wrong lever for brilliancy: on the same 600-game draws, shrinking it from 340 to 20 raised the strict chess.com brilliant rate from 0.065 to 0.113 per game **and** was ~50 Elo *stronger* head-to-head (bases 0/20/40/120/220/340 give 0.108/0.113/0.092/0.093/0.078/0.065, monotone in the bonus). The big bonus bought junk: it fired *more* sacrifices in total but *fewer* that stand up as brilliants. A piece simply left hanging with nothing aimed at your king gets no bonus at all.

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
| **Ultimate Tal** (default) | 21 | 90 | 95 | 90 | 15 | 90 | Full search strength, the full sacrificial personality, Brilliance 100: the most sacrificial setting measured, and its sacrifices are the soundest. |
| **Kamikaze** | 21 | 100 | 100 | 100 | 0 | 100 | Every dial at the top. Measured against the default (100 games each, [report](testing/results/kamikaze_vs_default.md)): the same strength (52.5% head-to-head) but *not* more sacrifices (3.4 against 4.0 per 100 balanced moves, within noise). The extra attack weight makes every attacking move look good, so a sacrifice stands out less. |
| **Classic AggroChess 4.5** | 21 | 60 | 75 | 60 | 50 | 50 | Also set Brilliance 0. Identical search to AggroChess 4.5.0: one big sacrifice per game, then near-sound play. AggroChess 4.5.0 scored 68% against the default (100 games at 10+0.1, about +131 Elo). |
| **Sound attacker** | 21 | 70 | 55 | 75 | 50 | 30 | Attacks hard, sacrifices mostly when it can see it through (not measured separately). |
| **Analysis partner** | 21 | 0 | 0 | 0 | 50 | 0 | Personality off: no sacrifice bonus, symmetric evaluation, no contempt. Use with `MultiPV` 3–5. |
| **Club sparring** | 10–14 | 90 | 95 | 90 | 15 | 90 | A beatable, very sharp opponent. |
| **Beginner coach** | 3–7 | 90 | 95 | 90 | 15 | 90 | Recognisable attacking chess with sacrifices you can learn from (and punish). |
| **Human-feel opponent** | 21 | 90 | 95 | 90 | 15 | 90 | Plus `Human Think Time` 60–100 with your time frame set, and `Human Style` 50–100. Full strength, but it thinks and picks like a person. |
| **Rated bot, feels human** | any | 90 | 95 | 90 | 15 | 90 | Choose a `Skill Level`, keep every personality option, and add the two human options above: weak levels keep their character while pausing like a human. |
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

During development a first default (a larger, permanent sacrifice bonus) was measured and rejected: it gave up material more often but mostly in four-pawn howlers, produced *fewer* purposeful sacrifices per move than 4.5.0 and lost about 160 Elo. A later default raised the first-sacrifice bonus further, to 340; that too was measured out on 2026-11-13 and reverted — at 340 the bonus buys junk *inside* the badge window, so shrinking it to 20 raised the strict brilliant rate and strength at once (see "Sacrifice versus blunder" above). The current mapping tolerates sacrifices up to about 0.2 pawns below the best alternative when the game is young, growing to about 1.8 pawns through the pressure ramp. Turning every control up to 100 (*Kamikaze*) did not add sacrifices in a 100-game test, so **the defaults are the most sacrificial setting measured**. For fewer sacrifices, lower Sacrifice Appetite and Speculative Sacrifices towards the Classic preset.

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
| `testing/brilliant.py` | Counts brilliant-style (sound piece-or-more) sacrifices in a `sactest.py` run directory. |

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
