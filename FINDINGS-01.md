# FINDINGS — Task 01: microchess rules engine (Rust, `solver/`)

> **Commit status:** all work is complete and verified, but the final
> `git add -A && git commit` **could not be run from this session**: the
> worktree's `.git` directory lives at `C:/Woodchop/Code/Microchess/.git`,
> outside the sandbox-writable session workspace, and both sandbox escalations
> (`workspace-write`, `danger-full-access`) were denied (no approval channel
> available). The working tree is left staged-ready: please run
> `git add -A ; git commit -m "Task 01: microchess rules engine (see FINDINGS-01.md)"`
> on branch `agent/rules-engine`. Nothing else is outstanding.

## What was built

A dependency-free Rust crate rooted at `solver/`, implementing the rules only —
no search, no evaluation, no TT:

* `solver/src/lib.rs` — `Position` (`[u8; 20]` board, `idx = rank*4 + file`,
  a1 = 0, d5 = 19; piece codes identical to `reference/mgen.cpp`), `Move`
  (`from`/`to`/`promo`/`castle`), make/unmake with an `Undo` record
  (captured piece + castling rights + halfmove clock + fullmove number), FEN
  in/out, `perft()` and `divide()`.
* `solver/src/movegen.rs` — pseudo-legal generation and legality filtering,
  ported line-for-line from the validated `reference/mgen.cpp`: precomputed
  knight/king target tables, rook/bishop rays with -1 segment terminators,
  pawn push/diagonal capture with mandatory promotion on the last rank, and
  the `a1c1` / `a5c5` castling rule with its exact attack conditions.
* `solver/src/bin/perft.rs` — the fixed CLI:
  * `perft <depth>` → prints exactly `perft <depth> = <nodes>`
  * `perft <depth> --divide` → `<uci>: <nodes>` lines (sorted by UCI string)
    then `Nodes searched: <total>`
  * `perft <depth> --fen "<FEN>"` → same as plain from the given position
* `solver/tests/perft_ladder.rs` — reads `docs/perft.txt` and asserts **the
  whole ladder including depth 9**, so the baseline cannot drift silently.
* `solver/tests/rules.rs` — promotion mandatory (4 choices, no bare last-rank
  push), no double step, rights lost on king move / rook move / capture ON d1,
  castling blocked through/into/out-of check, pinned knight generates nothing,
  SPEC's mate and stalemate FENs, clock updates, exhaustive make/unmake
  round-trip walk to depth 4, divide-sums-match-perft.

Design decisions worth knowing for the next task:

* **The halfmove clock is carried in the state** (`Position.halfmove_clock`,
  reset by `make` on any pawn move or capture, restored by `unmake`). Perft
  itself ignores it, matching the reference generator and Fairy-Stockfish
  perft semantics. The 50-move-rule draw decision is therefore available to
  the future solver at each node without extra computation; repetition is NOT
  tracked yet (state graph cyclicality is explicitly deferred to the solver
  task per docs/ARCHITECTURE.md).
* `Position::from_fen` accepts both `Dd` (docs/SPEC.md notation) and `Kk`
  castling field spellings; `to_fen` emits `D`/`d`. The e.p. field must be `-`.
* Castling rights are cleared exactly as in mgen.cpp: king moves, rook moves,
  or d1/d5 is captured on (the reference's extra `from == a1/a5` tests are
  subsumed by the king-move test but were ported verbatim).

## Observed perft output (run by me, release build)

```
$ cargo run --release --bin perft -- <d>   for d = 1..9

perft 1 = 9
perft 2 = 69
perft 3 = 525
perft 4 = 3957
perft 5 = 32944
perft 6 = 272861
perft 7 = 2338307
perft 8 = 19860602
perft 9 = 176466898
```

All nine depths match `docs/perft.txt` exactly, including
**perft 9 = 176466898** and the promotion canary **perft 5 = 32944** (not the
promotion-less 32923).

`cargo test --release`: 22 tests, all green (lib 5, ladder 2, rules 15),
no warnings introduced.

## Divide comparison against reference/mgen.cpp

Built with `g++ -O2 -o mgen.exe ../reference/mgen.cpp` and compared byte-exact
(line sets AND every per-move count) at three positions:

1. Start position, depth 5 — **identical**, all 9 root moves:
   `a1a2: 3519, a1b2: 5085, b1a2: 4354, b1c2: 4952, b1d3: 3303,
   c1a2: 3487, c1b3: 1859, c1d3: 2995, d2d3: 3390`,
   `Nodes searched: 32944`.
2. A castling-available position reached by the move sequence
   `c1d3 c5a4 b1c2` (FEN `kb1r/n2p/3N/2BP/K2R b Dd - 0 1`), depth 4 —
   **identical** to `mgen divide 4 c1d3 c5a4 b1c2`, including the `a1c1`
   castle move and all six black replies.
3. The child after `a4b2` of the same position, depth 3 — **identical**
   (13 white moves including `a1c1`).

During this comparison I hit three discrepancies that were ALL errors in my
hand-written test FENs / move sequences (wrong side to move, wrong piece
letters, a dropped bishop), never in either engine; I list them only because
the task asks for honesty about what differed. Final state: zero differences.

Timing note (not judged): depth 9 runs in ~31 s wall clock with this plain
copy-make/make-unmake implementation — fine for correctness work; optimisation
is explicitly out of scope here.

## Ambiguities / notes on the inputs

Nothing in `docs/SPEC.md` is wrong as far as I can verify. Two things worth
recording:

1. **Castling is not available from the start position.** SPEC lists castling
   as a rule with king `a1`→`c1`, but the start position has B on b1 and N on
   c1, and the rules require b1/c1 empty. The first castle therefore needs
   development first (confirmed identical behaviour in the reference). This
   surprised me initially and produced a wrong test expectation until I
   checked it against mgen.cpp — worth knowing when hand-writing FENs later.
2. **FEN castling-field spelling differs between inputs**: SPEC's start FEN
   uses `Dd`; mgen.cpp's dump mode emits `Kk`. Both denote the same single
   right per side. My FEN writer emits `D`/`d` (SPEC style) and the parser
   accepts both spellings, so neither convention breaks.

Minor observation on the reference (left untouched, per scope fence):
`makeMove` clears the right when a piece moves FROM a1/a5 even if it isn't the
king — unreachable dead logic while rights are intact (king occupies those
squares), harmless, ported as-is.

## Uncertainties

* None about the numbers: the binary reproduces `docs/perft.txt` exactly and
  matches the validated C++ generator move-by-move at every position I tested.
* One deliberate interpretation: `legal_moves` treats a position with a
  missing king as having no legal moves (defensive; cannot arise from legal
  play). Only affects hand-written test FENs.
