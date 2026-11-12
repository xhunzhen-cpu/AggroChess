# AggroChess Ultimate Sacrificial Edition 5.4.0

*Brilliant play, everywhere.* 5.3.0 made the engine's existing brilliancy check sound.
This release turns it into the engine's brilliant-play layer: it now finds and plays the
brilliancies the search itself cannot reach — **underpromotions** above all — and it does
that **at every Skill Level**, scaled by the `Brilliance` option, without disturbing a
single measured rating.

## Underpromotions (new)

The ordinary search ranks every non-queen promotion below every other move and the
quiescence search never looks at them, so AggroChess could not play one even when it was
the only win. The brilliancy look now nominates knight, bishop and rook promotions and
judges them on the neutral evaluation (`SacParams::neutral()`, full-window, root
reductions off via the new `Searcher::exact_root`).

Saavedra's position (`8/2P5/8/8/3r4/8/2K5/k7 w - - 0 5`) is the test: **`1.c8=R!!`** is
played at Brilliance 100 at *every* level from 1 to 21, and `c8=Q` at Brilliance 0 — the
behaviour is driven by the option, not by the level.

## Brilliant play at Skill Levels 1–20 (new)

The look used to run only at Skill Level 21. It now runs at every level, on its own
**verification budget** (`brilliant::VERIFY_NODES`, about 30 000 nodes at most, a few
milliseconds), taken from nowhere: the level's own search, its move and its measured
strength are untouched. Two rules make this safe, and both come from measurement:

* **A weakened engine's look may only promote a brilliancy, never demote the level's own
  move.** Measured during development: at level 8 a depth-3 neutral look "rejected" the
  Greek gift (`Bxh7+`). A look that shallow has no right to veto; it may only add.
* **The look's depth is its own, not the level's.** Level 5 caps the level's search at
  four plies; Saavedra's stalemate point is nine plies away. Sharing the level's cap made
  the look judge the wrong position entirely (measured: `c8=R` scored 843 vs a +1474
  "best move" — both meaningless), so the weak-level look runs to `VERIFY_DEPTH` and is
  stopped by its node budget instead.

The new **only-move defence** covers the other brilliant pattern a weak level cannot see:
in a bad position whose second-best candidate is far below its best, that gap is the
evidence that the best move is the only one that holds, and it is played
(`info string brilliant defence: …`).

`Brilliance` now also scales how much the look may spend at a weak level, so
`Brilliance 0` restores the level's pre-5.4 behaviour exactly.

## What the audit fleet changed

Ten read-only lenses (promotion paths, Skill Level budgets, the look's own correctness,
only-move/desperado/stalemate detectors, chess.com's actual published criteria, the test
harness, the bench/parity invariants, the UCI surface, panic and thread safety, and the
docs) ran over the 5.3.0 tree before a line was written. Their findings shaped the
implementation:

* `NeutralGuard` (RAII) replaces the duplicated save/restore around the neutral search, so
  every exit path — early return, `?`, or a future edit — restores the evaluation.
* `SacParams::neutral()` is now *actually* neutral: `check_see`, `ext_depth` and
  `zone_order` were still personality values, which biased the neutral comparison toward
  sacrifices. The full-strength numbers below are unchanged (the compare is done at
  `Brilliance` default and the bench is bit-identical), but the look no longer grades a
  sacrifice with a sacrifice-friendly pruning margin.
* `exact_root` and the full-window iteration keep the underpromotion group from being
  LMR-reduced out of judgement.
* The look's two halves now *share* one budget instead of each taking the full amount
  (the weakness case was spending twice what it was given).

## Gates

| Gate | 5.3.0 | 5.4.0 |
|---|---|---|
| `bench` | 287284 nodes | **287284 nodes** (unchanged) |
| Classic preset fixed-depth parity vs 4.5.0 | ALL MATCH | **ALL MATCH** |
| `ucitest` | 43/43 | **43/43** |
| `cargo test --release` | 49 passed / 2 ignored | **52 passed / 2 ignored** |
| Saavedra `c8=R!!`, Brilliance 100, levels 1–21 | not played below 21 | **played at every level** |
| Saavedra, Brilliance 0 | `c8=Q` | **`c8=Q`** (levels 1–21) |

Three new unit tests pin the static classification: the underpromotion class (and the
queen promotion's exclusion), the Saavedra rook promotion with quiet king moves excluded,
and the Greek gift's `Bxh7+` as a sacrifice candidate.

## Not done, and why

* **Stalemate-box heuristics** were designed but not shipped: `eval::stalemate_box_scale`
  can fire while the strong side still has a piece move that frees the king, and the
  neutral search compensates for that at depth but not at the weak-level look's budget.
  Shipping it there would have played losing "studies" as if they were draws.
* **Capping the quiet side of the look to the top *K* moves** (the audit's largest single
  speed win, aimed at the −56 Elo cost measured on the Classic preset) changes which
  sacrifices are taken, so it needs a match, not a code review. It is the next thing to
  measure, not something to ship on inspection.
* **The `Brilliance`-driven re-ranking of Skill Level candidates** (feeding the level's
  own `cp_loss` allowance with the brilliant move as an extra candidate) was considered
  and rejected: it would make the level's behaviour depend on the look's scores, which is
  exactly the coupling the two rules above exist to prevent.
