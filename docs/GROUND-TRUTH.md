# Independent ground truth for TASK-03 (PI-owned, do not hand to the agent)

Produced by `oracle.py`: a retrograde solver written from scratch in Python whose
move generation and legality come from **Fairy-Stockfish over UCI**. It shares no
code with `solver/`. Validated: 4/4 known positions correct and **120/120** random
KRvK positions agree with Fairy-Stockfish's own mate scores.

Values are from the side to move's point of view.

| class | legal positions | win | loss | draw | illegal slots | iters |
|---|---:|---:|---:|---:|---:|---:|
| KvK   |   540 |    0 |    0 |  540 |  220 |  1 |
| KNvK  |  8904 |    0 |    0 | 8904 | 4776 |  1 |
| KBvK  |  8712 |    0 |    0 | 8712 | 4968 |  1 |
| KRvK  |  7950 | 3090 | 3672 | 1188 | 5730 | 10 |
| KQvK  |  6942 | 2082 | 3472 | 1388 | 6738 |  8 |

Sanity: KvK legal = 2 x (20x19 - 110 ordered adjacent-king pairs) = 2 x 270 = 540. Exact.

Known individual positions (each confirmed against Fairy-Stockfish):

| FEN | value |
|---|---|
| `k3/4/4/4/K3 w - - 0 1`    | DRAW |
| `k3/4/4/4/K1N1 w - - 0 1`  | DRAW |
| `k3/4/4/4/K2R w - - 0 1`   | WIN (mate 5) |
| `k3/4/4/4/K2Q w - - 0 1`   | WIN (mate 4) |
| `k3/3N/1K2/4/4 b - - 0 1`  | DRAW (stalemate) |
| `1kR1/4/3N/2BP/K3 b - - 9 5` | LOSS (checkmate) |

## A trap this oracle fell into first

The first version tested legality by asking "can the side to move capture the enemy
king?". **That is wrong**: Fairy-Stockfish never generates king captures, so
adjacent-king positions passed as legal — and a *mated* side has no moves at all,
so the test vacuously returned "legal". It admitted 408 illegal positions into
KNvK and produced 84 phantom "losses" with zero wins, which is self-contradictory
(a reachable mate implies a winning predecessor).

The correct test: **flip the side to move and check `Checkers:`** — a position is
legal iff the side that just moved is not in check. If the agent's `illegal` counts
differ from the table above, this is the first thing to check.
