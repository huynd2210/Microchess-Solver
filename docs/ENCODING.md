# Position encoding — an exact key, not a hash

**A hash collision in a solver is a silently wrong answer.** Expected 64-bit
Zobrist collisions: 0.03 at 1e9 distinct positions, **2.71 at 1e10**, 271 at 1e11.
The identity of a position must therefore be *injective by construction*.

## Why the material-class decomposition is required, not just convenient

Rank the board naively — 20 squares, 13 values each — and you need
`log2(13^20) = 74.0 bits`. That does not fit in a `u64`.

Decomposing by material class does fit:

| field | range | bits |
|---|---|---|
| material class | 1,296 | 10.3 |
| placement rank within the class | ≤ 232,890,577,920 | 37.8 |
| side to move | 2 | 1 |
| castling rights | 4 | 2 |
| **total** | | **≈ 51 bits → fits `u64`** |
| *observed max key over 42.4M real positions* | 113,875,406,384,881 | *46.7* |

Material classes: per side, any subset of {B,N,R} (8) × a pawn slot in
{none, P, Q, R, B, N} (6) gives 48 *labels* — but only **36 distinct
multisets**, because 12 of those labels alias. `(S={}, slot=B)` and
`(S={B}, slot=none)` are both `{K,B}`: a promoted bishop with the original
captured is the same material as the original bishop. So there are
**36 × 36 = 1,296 material classes**, not 2,304.

> This correction came from the task-02 implementation, whose random-key
> sampling test found `decode` was not injective until it began rejecting the
> non-canonical alias class ids. `encode` must map each material multiset to one
> canonical class id, and `decode` must reject the others — otherwise a key the
> encoder never produces decodes to a position that re-encodes differently.

**24 of those classes are unreachable and can be rejected.** Measured: *no pawn can
promote without a capture* — 0 promotions across 40,000,811 no-capture positions to
ply 14. The two pawns start on the d-file facing each other, block each other, and
a pawn can only leave its file by capturing. So any class holding a promoted piece
has had at least one capture and therefore at most 9 pieces on board; the only
reachable 10-piece class is `KBNRP vs kbnrp`. 24 of the 1,296 classes are dropped,
leaving **1,272 reachable classes** and a placement bound of **4.319e12** (down
from 1.225e13 counted naively).

## The transposition table

Hashing is still wanted — for *bucket selection only*.

* Compute the exact `u64` key as above.
* Hash it to choose a bucket.
* **Store the full exact key in the entry and compare it on probe.**

A hash collision then costs one extra probe and never a wrong value. This is the
one design that satisfies both "we need hashing for the TT" and "Zobrist collisions
are unacceptable" — the hash is an *address*, the key is the *identity*.

Two tables with different requirements, kept separate:

* **exact store** — settled values. Identity must be exact; entries must never be
  silently overwritten by a different position.
* **search TT** — heuristic estimates, move ordering, LAO* bookkeeping. May be
  evicted freely; a mistake here costs nodes, not correctness.

## Symmetry (optional, measure before adopting)

* Colour swap + vertical flip is a symmetry throughout (flip side to move and swap
  castling rights).
* Left–right mirror is a symmetry **only once castling rights are gone**, since the
  rook sits on the d-file.

Together worth up to ~4× on storage. If used, `canonical()` and `canonical_key()`
must pick the **same** representative — Tinyhouse broke exactly this, and only its
round-trip test caught it.
