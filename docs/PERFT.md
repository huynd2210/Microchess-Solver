# Perft baseline — the acceptance test for any rules implementation

From the start position `kbnr/3p/4/3P/KBNR w Dd - 0 1`.

These numbers are **cross-validated between two independent implementations**:
Fairy-Stockfish 14 with the corrected `engine/microchess.ini`, and the C++
generator in `reference/mgen.cpp`. They agreed exactly at every depth.

| depth | nodes |
|---:|---:|
| 1 | 9 |
| 2 | 69 |
| 3 | 525 |
| 4 | 3,957 |
| 5 | 32,944 |
| 6 | 272,861 |
| 7 | 2,338,307 |
| 8 | 19,860,602 |
| 9 | **176,466,898** |

Machine-readable: `docs/perft.txt`.

**Quote depth 9.** Tinyhouse shipped a legality bug that `perft 8` could not see
and `perft 9` caught; one ply of extra margin is cheap insurance.

A wrong number here means you changed the rules, not that you found a faster
generator. In particular `perft(5) = 32923` means pawn promotion is missing.
