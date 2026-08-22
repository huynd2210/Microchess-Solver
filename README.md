# Microchess engine setup

Stockfish itself can't play custom variants; this setup uses
**Fairy-Stockfish 14** (a Stockfish derivative with a variant framework),
configured with a `microchess` variant definition.

## The game

4x5 board, white at bottom, **uppercase = White** (standard FEN convention):

```
  a b c d
5 k b n r    black
4 . . . p
3 . . . .
2 P . . .
1 K B N R    white
```

- Start FEN: `kbnr/3p/4/3P/KBNR w - - 0 1`
- Castling allowed: king `a1`/`a5` with rook `d1`/`d5`. King goes to the
  c-file (`a1c1`, rook ends on `b1`). Legal once b/c squares of the back rank
  are empty and not attacked.
- No pawn double step → no en passant.
- Pawn promotes on the last rank (q/r/b/n).

Note: Fairy-Stockfish also ships a *built-in* variant called `micro`, but that
is a different game (it has lance pieces). Do not use it — use our
custom-defined `microchess`.

## Files

| File | Purpose |
|---|---|
| `engine/fairy-stockfish.exe` | Fairy-Stockfish 14 binary |
| `engine/microchess.ini` | The microchess variant definition |
| `engine/variants-official.ini` | Official reference config (documentation) |
| `microchess.py` | CLI driver around the UCI engine |

## Quick usage

```powershell
python microchess.py show                 # pretty-print start position
python microchess.py moves                # legal moves at start
python microchess.py best --depth 14      # best move + cp score
python microchess.py best "kbnr/3p/4/3P/KBNR b - - 0 1" --movetime 500
python microchess.py perft 3
```

Moves use UCI coordinate notation (`d2d3`); castling is `a1c1` / `a5c5`.

## Talking to the engine directly (for integration)

The two-step load is required — defining the variant is separate from
selecting it:

```
setoption name VariantPath value <abs path>/engine/microchess.ini
setoption name UCI_Variant value microchess
position fen kbnr/3p/4/3P/KBNR w - - 0 1
go depth 14        # or go movetime 1000 / go wtime .. btime ..
```

Engine replies stream `info depth ... score cp ... pv ...` lines followed by
`bestmove <move>`.
