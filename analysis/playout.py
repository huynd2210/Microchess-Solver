#!/usr/bin/env python3
"""Play FSF against itself on a microchess position until game end.

Usage: python playout.py [startFEN] [movetime_ms] [maxPlies] [threads] [hashMB]
Uses a persistent engine process (one variant per process!) and prints each
move as it is played (flushed) so progress can be monitored live.
"""
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).parent))
from eng import UciEngine  # noqa: E402

ENGINE = Path(__file__).parent.parent / "engine" / "fairy-stockfish.exe"
INI = Path(__file__).parent.parent / "engine" / "microchess.ini"

def main():
    fen       = sys.argv[1] if len(sys.argv) > 1 else "k3/4/4/3P/KBNR w - - 0 1"
    movetime  = int(sys.argv[2]) if len(sys.argv) > 2 else 30000
    maxplies  = int(sys.argv[3]) if len(sys.argv) > 3 else 60
    threads   = int(sys.argv[4]) if len(sys.argv) > 4 else 14
    hash_mb   = int(sys.argv[5]) if len(sys.argv) > 5 else 8192
    print(f"start={fen}  movetime={movetime}ms  threads={threads}  hash={hash_mb}MB", flush=True)

    eng = UciEngine(ENGINE, {
        "VariantPath": str(INI.resolve().as_posix()),
        "UCI_Variant": "microchess",
        "Threads": threads,
        "Hash": hash_mb,
    })
    moves, reps = [], {}
    result = "ply cap reached"
    for ply in range(1, maxplies + 1):
        eng.position(fen=fen, moves=moves)
        _, best = eng.go(movetime=movetime)
        cur, _ = eng.display()
        if cur is None:
            result = "engine failed at ply %d" % ply; break
        key = " ".join(cur.split()[:4])
        reps[key] = reps.get(key, 0) + 1
        halfmove = int(cur.split()[4])
        if best in (None, "(none)", "0000"):
            mover = "White" if cur.split()[1] == "w" else "Black"
            in_check = any("Checkers:" in l and not l.endswith(":") for l in eng.display()[1])
            result = f"game over at ply {ply}: {mover} has no reply -> " + \
                     ("CHECKMATE" if in_check else "STALEMATE (draw)")
            break
        moves.append(best)
        print("ply %3d: %s   (%s)" % (ply, best, cur), flush=True)
        if reps[key] >= 3:
            result = "draw: threefold repetition"; break
        if halfmove >= 100:
            result = "draw: 50-move rule"; break
    eng.quit()
    print(result, flush=True)
    print("total plies:", len(moves), flush=True)
    print("moves:", " ".join(moves), flush=True)

if __name__ == "__main__":
    main()
