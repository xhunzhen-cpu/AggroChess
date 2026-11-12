# AggroChess Ultimate Sacrificial Edition 5.3.0

*Fleet release*: every change in this version came out of a fourteen-lens audit
of the 5.2.0 code (search, evaluation, time management, protocol, NNUE, human
timing, testing, packaging), was verified against the source and — where it
could change play — measured in engine matches. Two live-reproduced
kill-the-engine bugs were fixed on the way.

## Time management and rules correctness (measured +19 Elo, 300 games, 95% CI +5..+38)

* The iterative deepening loop no longer starts an iteration that cannot
  finish before the hard limit: it projects the next depth's cost and keeps the
  last completed result instead of burning the clock on an aborted one.
* A forced reply (one legal move) is answered at once: no thinking, no
  forcing-line priming.
* `go nodes N` at full strength now honours small node budgets to within a few
  dozen nodes (it overshot ~10x before); helper threads on the same budget
  included.
* `movestogo 1` plans eleven twelfths of the clock instead of a quarter: the
  last move before a fresh clock should not be starved.
* **Repetition is FIDE-correct**: positions differing only in a
  *non-capturable* en-passant square are the same position (cozy-chess's hash
  always includes the ep bit; the repetition key now ignores it when nobody
  can capture). A new unit test pins the behaviour.
* The mate prover no longer claims mates the 50-move rule turns into draws
  (probing stops 20 plies before the limit; the ordinary search, which models
  the halfmove clock exactly, keeps the say).

## UCI protocol (two engine-killers, both reproduced before fixing)

* `setoption value 100 name Hash` (value before name — some GUIs send this)
  panicked and, with `panic=abort`, killed the engine for the rest of the
  session. The parser now takes either token order and shrugs off malformed
  lines. *(live-reproduced)*
* A failed search-thread or Lazy-SMP-helper spawn panicked the same way; a
  helper that cannot start is now skipped and a failed search thread answers
  with a legal move — the game continues instead of dying mid-session.
* **`go ponder` / `ponderhit` are implemented** (the tokens were silently
  ignored: pondering GUIs desynced as the engine answered on its own clock).
  The ponder now runs open-ended in its own thread, `ponderhit` converts to
  the stored clock on the hash table the ponder warmed (1.2 s to bestmove from
  a warm TT, measured), `stop` during a ponder answers at once. Pondered
  replies skip the human act, and the opening book is not consulted for a
  ponder `go`.
* Helper threads get 16 MiB stacks (64 MiB × 128 threads reserved 8 GiB).

## Polyglot book

* Book lookups hash the en-passant file **only when an en-passant capture is
  actually possible** — the rule every `.bin` book was generated with. Before,
  every position right after an unanswerable double push (~1 in 16) hashed to
  a key no book entry ever had. *(unit-tested)*
* `AGGRO_SEED` now makes **book picks** reproducible too (the book's PRNG
  reseeded from the wall clock on every move); two engines with the same seed
  play identical games.

## Evaluation speed

* The personality layer (the engine's character: extra king danger, trade
  aversion, king-attack rewards — added on top of the NNUE at every node) now
  answers from an exact per-thread cache keyed by position hash and a
  settings version (bumped by every change the layer depends on, including
  the brilliancy look's mid-search engine-side flip). The bench node count is
  **bit-identical**: the cache is exact, not approximate.

## Testing

* `tests/puzzles.epd` / `puzzles_extra.epd` — the twelve flagship
  Tal-style studies (Plaskett's Puzzle, Shirov's Bh3, …) — were reference
  material no tool executed. `testing/epd.py` runs them: the c3 field's
  expected moves, plus fortress eval anchors, at multiple budgets.
* New unit tests: the FIDE repetition key, the Polyglot ep hashing.

## Before → after, at the gates

| Gate | 5.2.0 | 5.3.0 |
|---|---|---|
| `bench` | 287284 nodes | 287284 nodes (unchanged) |
| Classic preset fixed-depth parity vs 4.5.0 | ALL MATCH | ALL MATCH |
| `ucitest` | 43/43 | 43/43 |
| vs 5.2.0, 10+0.1, 300 games | — | **+19 Elo** (time management batch) |

Head-to-head, the draw-correctness and personality-cache batches measured as
neutral (their CI spans zero) — they are correctness and speed changes, not
strength changes; the +19 comes from the time-management fixes that started
the release.
