# Solver architecture — decisions and their reasons

Goal: the **game-theoretic value of the start position**, plus a strategy
witnessing it. Arbitrary-position queries are not a requirement.

Fairy-Stockfish plays this game; it cannot solve it. Measured: it returns `cp 0`
for bare kings (a proven draw) and `cp 0` for the start position (unknown) —
identical output, so its evaluation carries no proof content. Brute-force tree
search is out by ~30 orders of magnitude (EBF 8.89; depth 20 ≈ 4.8e18 nodes ≈
11,700 core-years). The tractable object is the **state graph**, not the tree.

## Why AO*, and the failure mode it must avoid

AO* is a heuristic AND/OR graph search: OR nodes are the mover's choices, AND
nodes the opponent's. It expands the most promising tip of the current best
partial solution graph and back-propagates.

The known failure mode is precise, and a sibling project (`C:\Woodchop\Code\Tinyhouse`,
a df-pn solver for 4×4 crazyhouse) died on it: **a forward proof search has no base
case for a draw.** A draw is only established by exhausting alternatives, so drawn
regions can only be settled by a fixed point over a *closed* set of positions. Any
depth-bounded region has a frontier whose successors are missing; those tips get
evicted as draw candidates and the eviction cascades back to the root. Tinyhouse
proved **zero draws** across eleven heuristics at every budget, and its root's proof
and disproof numbers both *grew* over 1e9 nodes — divergence, not slow progress.

## Why microchess is not Tinyhouse

Read this before proposing anything: it is the reason to expect a different outcome.

Tinyhouse is crazyhouse — captured pieces enter a pocket and return to the board.
**Material is conserved**, so there is no monotone progress measure and no
decomposition. Every position can reach every other; the whole ~1e12 state space is
one strongly connected blob.

**In microchess a captured piece is gone forever.** Material is monotonically
non-increasing, so:

* Positions partition into **material classes** — per side, subsets of {B,N,R}
  crossed with a pawn slot in {none, P, Q, R, B, N} give 48 labels but only
  **36 distinct multisets** (12 alias: a promoted bishop with the original
  captured is the same material as the original bishop). So **1,296 classes**, of
  which 24 are unreachable, leaving **1,272**. Captures and promotions move
  strictly *down* this DAG; they never move up. See `docs/ENCODING.md`.
* The class DAG is **acyclic**. Cycles — hence repetition draws — exist only
  *within* a single class, among the non-capture, non-promotion moves.
* Therefore the game can be solved **bottom-up**, endgame-tablebase style: solve
  all lower classes first, and within a class every capture or promotion is an
  exact leaf value looked up from an already-solved class. What remains inside a
  class is a small cyclic subgraph, settled by retrograde fixed point — which
  handles draws correctly and has no frontier to leak through.

This is exactly the decomposition Tinyhouse could not have. **AO* supplies the
forward, heuristic-guided proof; the class decomposition supplies the terminal
values that stop the draw cascade.** AO* alone, run flat on the whole game, will
reproduce the Tinyhouse result.

## Sizing (measured, and where it is only estimated)

* Perft EBF 8.89. Native enumeration ~13.0 M nodes/s single-threaded.
* Distinct reachable positions by BFS with dedup: 1,021,173 at ply 8;
  13,634,481 at ply 10; **118,717,620 at ply 12**, with per-ply growth decaying
  4.7× → 3.4× → 2.7×. The decay is the reason to expect this to terminate.
* Total reachable is **estimated 1e9–1e10 and not yet measured**. The exact
  placement count for the no-capture region alone (both pawns stuck on the d-file,
  both bishops colour-bound) is 1,349,187,840 — ×2 for side to move.
* Driving Fairy-Stockfish as a rules oracle runs at 4,172 positions/s versus
  13,052,285/s native. Never call the engine inside the solver; it is for
  validation only.

## Transposition table and encoding — the correctness trap

**A hash collision in a solver is a silently wrong answer**, not a slow one.
Expected 64-bit-Zobrist collisions by scale: 0.00 at 1e8, 0.03 at 1e9,
**2.71 at 1e10**, 271 at 1e11. The existing `reference/mgen.cpp` uses a 64-bit
Zobrist and is fine for the ply-12 BFS it was written for; it is **not** safe as a
solver's identity function.

Two tables with different requirements — keep them separate:

* **Exact store** (settled values, and the retrograde arrays): identity must be
  exact. Use a **perfect combinatorial rank** — position ↔ integer, injective by
  construction — not a hash. Tinyhouse does this in `src/codec.rs` and it is the
  right pattern to copy.
* **Search TT** (heuristic estimates, move ordering, AO* bookkeeping): may be
  lossy. A collision here costs nodes, not correctness, so a cheap 64-bit hash is
  appropriate and it may be evicted freely.

Symmetry: once castling rights are exhausted the position has a left–right mirror
symmetry, and there is a colour-swap + vertical-flip symmetry throughout. Together
worth up to ~4× on storage. Canonicalisation and the rank must agree on the same
representative — Tinyhouse broke exactly this and its round-trip test caught it.

## What "fast" means here

The target is **proofs per unit wall time**, not nodes per second. A node budget
silently rewards whichever heuristic is cheapest to evaluate. Benchmark under a
wall-clock budget.

Measurement discipline, inherited from Tinyhouse where getting it wrong inverted a
whole round of results: this box is an i5-14600K with 6 P-cores (logical 0–11) and
8 E-cores (logical 12–19), and identical code measures 330,917 n/s on cpu 2 versus
155,923 on cpu 16. **Pin to a P-core**, interleave A/B repeatedly, swap arm order
between blocks, and check the machine is quiet first.
