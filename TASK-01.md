# TASK 01 — the rules engine, and nothing else

You are building the foundation of a solver for **microchess**, a 4×5 chess
variant. This task is *only* the rules: board, move generation, make/unmake, FEN,
and perft. **Do not write any search or solver code.** A later task does that, and
it is worthless if the rules are wrong.

Read first, in this order:
* `docs/SPEC.md` — the exact rules, each one verified against a reference engine.
* `docs/PERFT.md` and `docs/perft.txt` — your acceptance test.
* `reference/mgen.cpp` — a **working, validated** C++ generator for this exact
  game. It agrees with Fairy-Stockfish at every depth to 9. Treat it as the
  definition when `docs/SPEC.md` is ambiguous. You may port its logic directly;
  you do not have to invent anything.

## Deliverable

A Rust crate rooted at `solver/` in this worktree.

```
solver/Cargo.toml
solver/src/lib.rs        Position, Move, make/unmake, FEN in/out
solver/src/movegen.rs    pseudo-legal + legal generation
solver/src/bin/perft.rs  the perft binary
solver/tests/            tests, including the ladder
```

The binary's command line is fixed, because I verify with an exact command:

```
cargo run --release --bin perft -- <depth>            # prints: perft <depth> = <nodes>
cargo run --release --bin perft -- <depth> --divide   # <uci_move>: <nodes> lines, then "Nodes searched: <total>"
cargo run --release --bin perft -- <depth> --fen "<FEN>"
```

Print exactly those formats — one `perft N = M` line and nothing else on stdout
for the plain form. Moves are UCI coordinate strings (`d2d3`, `a1c1`, `c4c5q`).

## Acceptance — non-negotiable

`cargo run --release --bin perft -- 9` must print

```
perft 9 = 176466898
```

and depths 1–8 must match `docs/perft.txt` exactly. **A number that differs means
you changed the rules, not that you found a faster generator.** In particular
`perft 5 = 32923` means you did not implement pawn promotion; the correct value is
`32944`.

Add a test that reads `docs/perft.txt` and asserts the whole ladder, so the
baseline cannot drift silently.

## Correctness bar

1. `cargo build --release` clean, no warnings you introduced.
2. `cargo test --release` green.
3. The perft ladder above, run by you, with the output pasted into your FINDINGS.
4. A `--divide` comparison against `reference/mgen.cpp` at depth 5 from the start
   position: build it (`g++ -O2 -o mgen.exe reference/mgen.cpp`, g++ is on PATH)
   and check every per-move count matches. This localises a rules bug to one move
   instead of leaving you with a wrong total.

Things that have already bitten this codebase — do not rediscover them:
* Pawns **must** promote on the last rank, to n/b/r/q. Mandatory, not optional.
* There is **no double step**, so there is no en passant. Do not add either.
* Castling is `a1c1` / `a5c5` only, with the rook on the d-file. Rights are lost
  when the king moves, the rook moves, **or the rook's square is captured on**.
* Quote depth 9, not 8. A sibling project shipped a legality bug that depth 8
  could not see and depth 9 caught.

## Scope fence

* Do **not** modify `docs/`, `engine/`, `reference/`, `analysis/`, `microchess.py`
  or `README.md`. They are inputs. If you believe one is wrong, say so in
  FINDINGS and leave it alone.
* Do **not** write solver, search, transposition-table, or evaluation code.
* Do **not** optimise beyond what is natural to write. This task is judged on
  correctness only; speed work comes later against a pinned baseline.

## Report

Write `FINDINGS-01.md`: what you built, the exact perft output you observed, the
divide comparison result, anything in `docs/SPEC.md` you found ambiguous or wrong,
and anything you are uncertain about. **Then `git add -A` and `git commit`** on the
current branch (`agent/rules-engine`) with a message describing the change.

If you cannot reach the acceptance numbers, **say so plainly and commit what you
have with the discrepancy documented.** A truthful failure is useful; a claim of
success I can disprove in one command is not. I will re-run every number myself.

This is a mechanical porting job with a hard oracle. Do the whole thing carefully
rather than looking for a shortcut — there is a reference implementation and an
exact expected answer, so there is nothing to guess.
