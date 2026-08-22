# TASK 02 — the exact position codec, and the transposition table on top of it

`solver/` already contains a **validated** rules engine (task 01: perft ladder
exact to depth 9, cross-checked against Fairy-Stockfish on 1,619 positions). Do not
change its rules. This task adds the identity layer the solver will be built on.

Read `docs/ENCODING.md` first — it contains the design and the bit budget. Also
read `docs/REPETITION.md` and `docs/ARCHITECTURE.md` for why the material-class
decomposition is load-bearing rather than an optimisation.

## Why this task is the one that must not be wrong

A hash collision in a solver is a **silently wrong answer**. There is no symptom, no
crash, and no test that fails later. Everything downstream inherits it. The key must
be injective *by construction*, not injective *in practice*.

## Deliverable

```
solver/src/codec.rs      encode: &Position -> Key(u64)   /   decode: Key -> Position
solver/src/tt.rs         transposition table keyed by the exact Key
solver/src/bin/codeck.rs the verification binary described below
```

### The key

Per `docs/ENCODING.md`: `(material_class, placement_rank, side_to_move, castling)`
packed into a `u64`, ~52 bits. Material class = per side, subset of {B,N,R} (8) ×
pawn slot in {none,P,Q,R,B,N} (6) = 48; 48×48 = 2304. The placement rank is a
**combinatorial rank** of the piece-to-square assignment — a counting function, not
a hash. Identical same-colour pieces (two rooks after a promotion) must be ordered
canonically so the two arrangements share one rank.

`decode` must reconstruct a `Position` equal to the original in every field that the
key covers. State the invariant you rely on in a doc comment.

### The transposition table

`solver/src/tt.rs`. Hash the exact key **for bucket selection only**, and **store
the full 64-bit key in the entry and compare it on probe**. A hash collision must
cost one extra probe and never a wrong value. Size configurable in bits; simple
replacement policy is fine — this task is about correctness, not eviction strategy.
Provide `get(key) -> Option<V>` and `put(key, v)`, generic or with a placeholder
value type; the solver task will fix the payload.

## Acceptance — I run all of this myself

`cargo run --release --bin codeck -- <maxply>` must perform a BFS from the start
position and print, for each ply, the count of distinct positions, then the codec
verdicts. Required output lines (exact prefixes, so I can parse them):

```
ply <n> distinct <count>
maxkey <value>
roundtrip <ok|FAIL> <positions_checked>
injective <ok|FAIL> <distinct_keys> <distinct_positions>
```

**1 — BFS counts must match these**, which were measured by an independent C++
implementation (`reference/mgen.cpp`) and are cumulative distinct positions:

| ply | cumulative distinct |
|---:|---:|
| 6 | 56,141 |
| 8 | 1,021,173 |
| 9 | 3,898,949 |
| 10 | 13,634,481 |
| 11 | 42,373,487 |
| 12 | 118,717,620 |

**Dedup the BFS by the full position bytes, NOT by your own key.** If you dedup with
the codec, the injectivity test becomes vacuous — two positions sharing a key would
be silently merged and the count would still look plausible. This exact trap
(self-consistency instead of independent verification) is documented as having cost
the sibling project a real bug. The BFS must be able to *disagree* with your codec.

**2 — `roundtrip ok`** for every position visited: `decode(encode(p)) == p` and
`encode(decode(encode(p))) == encode(p)`.

**3 — `injective ok`**: the number of distinct keys equals the number of distinct
positions, at every ply, to at least ply 10. Not "no collisions found" — the two
counts must be *equal*.

**4 — `maxkey` < 2^52.** Print it; I check it.

**5** — `cargo test --release` green, including the task-01 perft ladder (depth 9 =
176,466,898) still passing. If the ladder breaks you changed the rules.

**6** — a TT test that inserts many keys and asserts no probe ever returns a value
stored under a different key.

## Scope fence

* Do **not** modify `docs/`, `engine/`, `reference/`, `analysis/`, `microchess.py`,
  `README.md`, or the rules in `solver/src/movegen.rs`.
* Do **not** write the search/solver. No AO*, no LAO*, no heuristics, no evaluation.
* Symmetry canonicalisation is **out of scope** — the docs mention it as a future
  option. Adding it now would make the injectivity test mean something different.

## Report

`FINDINGS-02.md`: the design of the rank function, the exact `codeck` output you
observed, the bit budget you actually achieved, and anything you are unsure of. If
you cannot make injectivity hold, **say so and commit the failing case** — a
reproducible counterexample is worth far more than a green claim I can break.

I will re-run every number. Assume I will try to construct a collision.

Note: `git commit` will fail — the worktree's `.git` points outside your sandbox.
That is expected and is not your problem to solve. Leave the tree clean and say so
in FINDINGS; I will commit.
