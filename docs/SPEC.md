# Microchess — the exact rules the solver must implement

Every statement below was verified against Fairy-Stockfish 14 with the corrected
`engine/microchess.ini`, and cross-checked against an independent C++ generator
(`reference/mgen.cpp`). Do not take this file on trust either: `docs/PERFT.md`
is the machine-checkable form.

## Board

4 files (`a`–`d`) × 5 ranks. White at the bottom. Square index convention used by
the reference generator: `idx = rank*4 + file`, so `a1 = 0`, `d1 = 3`, `a5 = 16`,
`d5 = 19`.

```
  a b c d
5 k b n r     black
4 . . . p
3 . . . .
2 P . . .
1 K B N R     white
```

Start FEN: `kbnr/3p/4/3P/KBNR w Dd - 0 1`

## Pieces

King, queen, rook, bishop, knight, pawn — all move exactly as in standard chess.
There are no fairy pieces. Sliders (Q/R/B) do slide, so **pins exist** (unlike the
Tinyhouse project, whose optimisations relied on their absence — do not port that
reasoning here).

## Pawns

* Move one square forward; capture one square diagonally forward.
* **No double step**, therefore **no en passant**. The FEN e.p. field is always `-`.
* **Promotion on the last rank** (rank 5 for White, rank 1 for Black) to `n`, `b`,
  `r`, or `q`. Promotion is mandatory — a pawn cannot remain a pawn there.

> This is the rule the shipped `.ini` silently omitted. Without
> `promotionRank = 5` a pawn on the last rank could push again and be deleted from
> the board. If your `perft(5)` is 32923 rather than 32944 you have reproduced
> that bug.

## Castling

One rook per side, on the d-file. Rights are written `D` (White) and `d` (Black).

* White: `a1c1` — king `a1`→`c1`, rook `d1`→`b1`.
* Black: `a5c5` — king `a5`→`c5`, rook `d5`→`b5`.

Legal only when: the right is intact, the king and rook are on their home squares,
`b1`/`c1` (resp. `b5`/`c5`) are empty, and none of `a1`,`b1`,`c1` (resp.
`a5`,`b5`,`c5`) is attacked by the opponent. Rights are lost when the king moves,
when the rook moves, or when the rook's square is captured on.

## Terminal conditions — all verified

| condition | value | verification |
|---|---|---|
| checkmate | loss for the side to move | `1kR1/4/3N/2BP/K3 b` → `mate 0`, 0 legal moves |
| stalemate | **draw** | `k3/3N/1K2/4/4 b` → 0 legal moves, `cp 0` |
| 50-move rule | draw, triggers when the halfmove clock reaches 100 | `k3/4/4/4/K2Q w - - 90 60` is `mate 4`; at `94` it is `cp 0` |
| threefold repetition | draw | standard |

Two consequences the solver must not paper over:

1. **The halfmove clock is part of the game state.** A position that is a win with
   a clock of 90 is a draw with a clock of 94. Either carry the clock in the state,
   or compute distance-to-zeroing and apply the 50-move rule afterwards — but say
   in the FINDINGS which you did, because "solved" means different things.
2. **Repetition makes the state graph cyclic**, so a draw has no base case in a
   forward proof search. See `docs/ARCHITECTURE.md`.
