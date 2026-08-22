"""Independent retrograde solver for small microchess material classes.

Shares NO code with solver/: move generation and legality come from
Fairy-Stockfish over UCI, the fixed point is written here from scratch.
Purpose: an oracle the Rust solver can be checked against.

Values are from the SIDE TO MOVE's point of view: WIN / LOSS / DRAW.
"""
import sys, itertools, time
R = "C:/Woodchop/Code/Microchess"
sys.path.insert(0, R + "/analysis")
from eng import UciEngine

FILES = "abcd"
def sq_name(i): return FILES[i % 4] + str(i // 4 + 1)

def make_fen(pieces, stm):
    """pieces: dict square-index -> piece char. Returns a FEN with no castling."""
    rows = []
    for r in range(4, -1, -1):
        row, empty = "", 0
        for f in range(4):
            p = pieces.get(r * 4 + f)
            if p is None: empty += 1
            else:
                if empty: row += str(empty); empty = 0
                row += p
        if empty: row += str(empty)
        rows.append(row)
    return "/".join(rows) + f" {stm} - - 0 1"

class Oracle:
    def __init__(self):
        self.E = UciEngine(R + "/engine/fairy-stockfish.exe",
                           {"VariantPath": R + "/engine/microchess.ini",
                            "UCI_Variant": "microchess"})
        self.mv_cache = {}
    def moves(self, fen):
        if fen not in self.mv_cache:
            self.mv_cache[fen] = self.E.moves(fen)
        return self.mv_cache[fen]
    def child(self, fen, mv):
        self.E.position(fen=fen, moves=[mv])
        f, _ = self.E.display()
        return " ".join(f.split()[:4]) + " 0 1"      # drop clocks
    def in_check(self, fen):
        self.E.position(fen=fen); self.E.send("d")
        for l in self.E.wait_for("Checkers:"):
            if l.startswith("Checkers:"):
                return l.split(":", 1)[1].strip() != ""
        return False
    def legal_position(self, fen):
        """Legal iff the side that just moved is NOT in check.

        Testing "can the side to move capture the enemy king" does NOT work:
        Fairy-Stockfish never generates king captures, so adjacent-king
        positions pass that test and slip through as legal. Instead flip the
        side to move and read Checkers: for the other side.
        """
        parts = fen.split()
        flipped = " ".join([parts[0], "b" if parts[1] == "w" else "w"] + parts[2:])
        return not self.in_check(flipped)

def solve_class(orc, white, black, verbose=True):
    """white/black: lists of piece chars, e.g. ['K','R'] and ['k']."""
    t0 = time.time()
    n = len(white) + len(black)
    all_pieces = white + black
    positions = []
    for squares in itertools.permutations(range(20), n):
        pieces = dict(zip(squares, all_pieces))
        for stm in "wb":
            fen = make_fen(pieces, stm)
            positions.append(fen)
    # dedup identical FENs arising from identical pieces (e.g. two knights)
    positions = sorted(set(positions))
    legal = [f for f in positions if orc.legal_position(f)]
    if verbose: print(f"    slots {len(positions)}, legal {len(legal)}", flush=True)
    # build successor graph
    succ = {}
    for f in legal:
        succ[f] = [orc.child(f, m) for m in orc.moves(f)]
    # initialise
    val = {}
    for f in legal:
        if not succ[f]:
            val[f] = "LOSS" if orc.in_check(f) else "DRAW"   # mate / stalemate
    # fixed point
    iters = 0
    while True:
        iters += 1; changed = False
        for f in legal:
            if f in val: continue
            cvals = [val.get(c) for c in succ[f]]
            if any(v == "LOSS" for v in cvals):
                val[f] = "WIN"; changed = True
            elif all(v == "WIN" for v in cvals):
                val[f] = "LOSS"; changed = True
        if not changed: break
    for f in legal:
        val.setdefault(f, "DRAW")       # unresolved at convergence == draw
    w = sum(1 for f in legal if val[f] == "WIN")
    l = sum(1 for f in legal if val[f] == "LOSS")
    d = sum(1 for f in legal if val[f] == "DRAW")
    print(f"  class {''.join(white)}v{''.join(black)} positions {len(legal)} "
          f"win {w} loss {l} draw {d} illegal {len(positions)-len(legal)} "
          f"iters {iters} time {time.time()-t0:.1f}s", flush=True)
    return val

if __name__ == "__main__":
    orc = Oracle()
    which = sys.argv[1:] or ["KvK", "KNvK", "KBvK"]
    spec = {"KvK": (["K"], ["k"]), "KNvK": (["K","N"], ["k"]), "KBvK": (["K","B"], ["k"]),
            "KRvK": (["K","R"], ["k"]), "KQvK": (["K","Q"], ["k"])}
    out = {}
    for name in which:
        out[name] = solve_class(orc, *spec[name])
    import json, pathlib
    pathlib.Path(sys.argv[0]).with_name("oracle_values.json").write_text(
        json.dumps({k: v for k, v in out.items()}))
    orc.E.quit()
