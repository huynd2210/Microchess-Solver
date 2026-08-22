#!/usr/bin/env python3
"""Command-line helper for playing Microchess (4x5 variant) with Fairy-Stockfish.

Usage:
    python microchess.py best [FEN] [--depth N | --movetime MS]
    python microchess.py eval [FEN] [--depth N]
    python microchess.py moves [FEN]          # list legal moves in UCI notation
    python microchess.py show [FEN]           # pretty-print the board
    python microchess.py perft N [FEN]

FEN defaults to the Microchess start position.
Moves are in UCI coordinate notation, e.g. "d2d3", castling = "a1c1"/"a5c5".
Scores are reported as "cp <n>" or "mate <n>" (from the engine's search).
"""
import argparse
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).parent / "analysis"))
from eng import UciEngine  # noqa: E402

ENGINE = Path(__file__).parent / "engine" / "fairy-stockfish.exe"
VARIANT_INI = Path(__file__).parent / "engine" / "microchess.ini"
START_FEN = "kbnr/3p/4/3P/KBNR w - - 0 1"


def get_engine():
    return UciEngine(ENGINE, {
        "VariantPath": str(VARIANT_INI.resolve().as_posix()),
        "UCI_Variant": "microchess",
    })


def score_of(info_lines):
    """Extract 'cp N' or 'mate N' from the last info line that has a score."""
    for line in reversed(info_lines):
        t = line.split()
        if "score" in t:
            i = t.index("score")
            return "%s %s pv %s" % (t[i+1], t[i+2], " ".join(t[t.index("pv")+1:]) if "pv" in t else "")
    return None


def best(fen, depth=None, movetime=None):
    eng = get_engine()
    try:
        eng.position(fen=fen)
        kw = {"depth": depth} if depth else {"movetime": movetime or 1000}
        info, move = eng.go(**kw)
        return move, score_of(info)
    finally:
        eng.quit()


def moves(fen):
    eng = get_engine()
    try:
        return eng.moves(fen)
    finally:
        eng.quit()


def perft(n, fen):
    eng = get_engine()
    try:
        return eng.perft(fen, n)
    finally:
        eng.quit()


def show(fen):
    board, turn = fen.split()[0], fen.split()[1]
    print("  a b c d")
    for i, row in enumerate(board.split("/")):
        cells = []
        for ch in row:
            cells += ["."] * int(ch) if ch.isdigit() else [ch]
        print(5 - i, " ".join(cells))
    print("Side to move:", "White" if turn == "w" else "Black")


def main():
    ap = argparse.ArgumentParser(description="Microchess engine driver")
    sub = ap.add_subparsers(dest="cmd", required=True)

    p = sub.add_parser("best");     p.add_argument("fen", nargs="?", default=START_FEN)
    p.add_argument("--depth", type=int); p.add_argument("--movetime", type=int)
    p = sub.add_parser("eval");     p.add_argument("fen", nargs="?", default=START_FEN); p.add_argument("--depth", type=int, default=12)
    p = sub.add_parser("moves");    p.add_argument("fen", nargs="?", default=START_FEN)
    p = sub.add_parser("show");     p.add_argument("fen", nargs="?", default=START_FEN)
    p = sub.add_parser("perft");    p.add_argument("n", type=int); p.add_argument("fen", nargs="?", default=START_FEN)

    a = ap.parse_args()
    if a.cmd == "best":
        move, score = best(a.fen, a.depth, a.movetime)
        print(move, "" if score is None else "(%s)" % score)
    elif a.cmd == "eval":
        _, score = best(a.fen, depth=a.depth)
        print(score or "?")
    elif a.cmd == "moves":
        print("\n".join(moves(a.fen)))
    elif a.cmd == "show":
        show(a.fen)
    elif a.cmd == "perft":
        print(perft(a.n, a.fen))


if __name__ == "__main__":
    main()
