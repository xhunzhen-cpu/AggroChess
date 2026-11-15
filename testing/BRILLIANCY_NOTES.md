# Brilliancy: the owner-full metric and the plan layer

_2026-11-15. Status: metric settled and instrumented; two levers and one
architectural change built and bite-verified; match results pending._

## 1. The metric of record

The objective is **brilliants per game** — chess.com-style badges — subject only to an
Elo floor. The project's existing scorer counted a much looser thing, so the first job
was to make the number honest and to make the shapes the owner counts *visible*.

The owner counts **three** badge shapes. Only the first is in the arena's event stream,
so the other two come from separate detectors and are merged — **with deduplication**,
which is mandatory and was previously missing.

| shape | events | rate | source |
|---|---|---|---|
| sacrifice, HONEST (moved piece is the sacrificed piece) | 63 | 0.0420/g | `honest_score.honest()` |
| underpromotion, badge-shaped | 5 | 0.0033/g | Stockfish-gated, engine side |
| ignored attack, Stockfish-sound | 52 | 0.0347/g | `/tmp/ign` detector, all flags graded |
| **OWNER-FULL** | **120** | **0.0800/g** | |

Measured on `bng_ctrl_s11`, a complete 1500-game control (both sides byte-identical, so
it doubles as a shipped-behaviour baseline).

### Deduplication is not optional

A naive sum double-counts. Measured on the control arm:

* `ignored ∩ loose-sacrifice` = **14** events — an ignored attack *can* coincide with a
  loose sacrifice (a piece dies near the move).
* `ignored ∩ HONEST` = **0** — an ignored attack loses a *different* piece, so it can
  never share a `(game_id, ply)` with a moved-piece sacrifice.

Because the two shapes are disjoint on the honest leg, the union is their sum; but the
union must be taken on `(game_id, ply)` keys, never by adding rates.

### Numbers in the handover that are wrong

* **`0.0515/g` ignored-attack "honest" rate — refuted.** The old `final_sf.py`
  incremented its denominator before the eval-band `continue`, so `16/80` divided an
  in-band numerator by an all-graded denominator. Grading all 386 flags at depths
  12/14/16 gives **0.0347/g** (52/1500). The conditional reading `structural × sound/in-band`
  is equally population-mixed; the coherent version collapses to the same 0.0347.
* **`0.0140/g` underpromotion — a both-colour figure.** `145` underpromotions over 1500
  games is one engine self-playing itself; the engine-side rate is **80** (0.0533/g), and
  only **5–8** are badge-shaped once Stockfish gates soundness — **0.0033–0.0053/g**.
* The **`0.2573/g` structural** ignored-attack rate is a *candidate* rate: 224 of its 386
  flags fail Stockfish's own eval band.

### The arena is blind, and it is source-confirmed

Every engine underpromotion is filed `category="pawn"` (verified against the event
stream: 23/23 promotion events, all `cat=pawn`, all `net={'P':1}`). `score_tiers.tier()`
requires `category ∈ {queen, exchange, piece}`, so **100% are rejected**. Only 23 of the
engine's 80 underpromotions generate any event at all. The ignored-attack shape is
invisible for a different reason: it loses a piece the classifier never models.

`testing/owner_full.py` is the portable scorer. It refuses to grade an arm whose summary
is not `complete`, documents the dedup rule, and carries `--selftest` pinning all three
predicates.

## 2. Why knob-tuning was exhausted

Six independent 1500-game lever families are null against a calibrated noise floor
(±0.0147/g, from five provable null controls). The reason is structural, and the funnel
shows it (var side, 1500 games):

```
all sac events                2217   1.4780/g
piece+ (not pawn)              824   0.5493/g
not mating                     741   0.4940/g
not already winning            542   0.3613/g
not lost                       234   0.1560/g
sound (LOOSE metric)           116   0.0773/g
HONEST (moved piece)            63   0.0420/g
```

The engine *already* sacrifices ~1.5×/game and lands 0.087/g sound ones. The collapse to
0.042/g happens at the last gate: **the moved piece must be the sacrificed piece**. So
the failure is not selection among sacrifices, and not supply — it is that the engine
usually does not play the move that gives material away.

Three levers were built this session. Two are ordinary knobs, provably live:

* **`x_winsacoff`** — every existing term in `sac_params_for` only ever *loosens* the
  appetite in a decided position; nothing suppresses it, and 199 piece+ events per 1500
  games die at the "already winning" gate.
* **`x_hanglive`** — `x_hang` measured inert, and the reason is now proven: its term sits
  under `if !full { break; }`, and the search only ever calls `compute(board, false)`.
  Same binary, same magnitude: `x_hang=200` → 0/30 moves changed; `x_hanglive=200` →
  16/30. `x_hanglive` puts the tolerance in the personality layer the search actually adds.

## 3. The plan layer — designed *for* brilliants

The third change is architectural, and it is the one that answers the brief.

Every other brilliancy mechanism in the engine is **reactive**: `brilliant.rs`, the Tal
look, and the sacrifice bonus all judge sacrifices the search has *already found*.
`src/plan.rs` points the other way. Instead of asking "is this move a sacrifice?", it
asks "does this move leave the opponent something they can win?" — and re-ranks
near-equal root moves toward doing so.

* **`offer_gain(board, mv)`** — can the opponent win **the piece that just moved** by
  static exchange? That is the chess.com brilliant shape itself, and precisely the gate
  the funnel dies on.
* **`ignore_gain(board, mv)`** — how many of our pieces *other than the moved one* the
  opponent can win after the move? The ignored-attack shape, 43% of owner-full.
* **`plan_pick`** — among root moves within `x_plan` centipawns of the best, pick the
  largest plan value. It mirrors the existing Human Style taste: it re-ranks near-equal
  moves and never re-values, and it defers to any earlier deliberate choice
  (`strategy_swap`), so the strength cost is bounded by the margin.

Modes 1 and 2 are disjoint by construction (mode 2 excludes the moved piece), so they can
never double-count; mode 3 combines them.

**Bite-verified end to end**: 13 unit tests; full suite 70 green; and on a real game
(ply 28 of the control PGN) it preferred `d2e4` over `h3g4` while offering a 320cp piece —
about one fire per eight eligible positions, which is the intended frequency for a
targeted re-ranker. Off by default (`x_plan=0`), so the build is bit-identical unless set.

### Gotchas worth recording

* `Board::generate_moves_for(mask, …)` masks by **FROM** square (which pieces may move),
  not by destination.
* `Board::null_move()` flips the side to move; when the side you need is already to move,
  use the board itself.
* A debug trace at the top of a block can silently vanish because a *preceding* block set
  `strategy_swap`; check that flag first.

## 4. What is running, and what is not

Six 1500-game arms measure the two knobs (`ws1_s*`, `as470_s*`, `hl*_s*`); plan-layer arms
follow as slots free. Weapons-grade honesty about the ceiling: the honest sacrifice leg
tops out near **0.087/g** in the current event mix, so 5/game is not reachable by tuning —
it needs the engine to *create* badgeable positions, which is what the plan layer is for.
The one untapped asset remains the 323-line curated gambit book, live in real play but
invisible to every match (they start at ply 9); it measured +0.0133/g at 600 games, inside
noise, and any opening work must drop the `openings` line to be visible at all.
