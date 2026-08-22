#!/usr/bin/env python3
"""Estimate the number of reachable/legal positions in 4x5 microchess.

Model: 20 squares. Each side has a king + at most 4 other pieces.
Non-king pieces start as {B,N,R,P}; the pawn may promote once, so a side's
non-king multiset is any multiset of size <=4 over {B,N,R,P,Q} where at most
one duplicate pair exists (the promoted piece duplicating an original).
We compute both a loose bound (any multiset) and this tighter bound,
times square placements, times side-to-move, with legality heuristics.
"""
from math import comb, factorial
from itertools import combinations_with_replacement

SQ = 20
TYPES = "BNRPQ"

# --- piece-multiset arrangements for one side (excluding king) -------------
def arrangements(multiset):
    # number of ways to place multiset members on distinct squares = k!/prod(c!)
    k = len(multiset)
    denom = 1
    for t in set(multiset):
        denom *= factorial(multiset.count(t))
    return factorial(k) // denom

def side_arrangements(loose):
    """total arrangements summed over all allowed non-king multisets, by size"""
    per_size = [0] * 5  # index = number of non-king pieces
    for k in range(0, 5):
        if loose:
            combos = combinations_with_replacement(TYPES, k)
        else:
            # at most one duplicated type (promotion), max multiplicity 2
            combos = [c for c in combinations_with_replacement(TYPES, k)
                      if all(c.count(t) <= 2 for t in set(c))
                      and sum(1 for t in set(c) if c.count(t) == 2) <= 1]
        per_size[k] = sum(arrangements(c) for c in combos)
    return per_size

# --- square placements -----------------------------------------------------
def placements(w, b):
    return comb(SQ, w) * comb(SQ - w, b)

# --- legality heuristics ----------------------------------------------------
# P(w kings adjacent | w+b pieces on 20 squares): approx via expected pairs
def king_adjacent_factor(w, b):
    # P(kings adjacent) ~ (adjacent pairs * remaining squares) / total pairs
    adj_pairs = 12 * 3 + 8 * 2  # horizontal 12*3? compute properly: 4x5 grid
    # 4 cols x 5 rows: horizontal adjacencies = (4-1)*5 = 15, vertical = 4*(5-1)=16 -> 31
    adj_pairs = 15 + 16
    total_pairs = comb(SQ, 2)
    p = adj_pairs / total_pairs
    return 1 - p

# P(side not to move in check): rough 0.9 (sparse board, few sliders)
CHECK_FACTOR = 0.9
# pawn placement restrictions: minor, ~0.95
PAWN_FACTOR = 0.95

loose = side_arrangements(True)
tight = side_arrangements(False)
print("non-king arrangements by piece count (w=loose, t=tight):")
for k in range(5):
    print(f"  k={k}: loose={loose[k]:6d}  tight={tight[k]:6d}")

total_loose = total_tight = 0
for w in range(1, 6):
    for b in range(1, 6):
        pl = placements(w, b)
        f = king_adjacent_factor(w, b) * CHECK_FACTOR * PAWN_FACTOR
        total_loose += pl * loose[w-1] * loose[b-1] * f
        total_tight += pl * tight[w-1] * tight[b-1] * f

print(f"\npositions (x side-to-move=2, castling rights avg ~2):")
print(f"  loose bound: {total_loose*2*2:.3e}")
print(f"  tight bound: {total_tight*2*2:.3e}")

# dominance breakdown: material count pairs
print("\nbreakdown by (w,b) pieces, tight, incl. 2x2 factors:")
rows = []
for w in range(1, 6):
    for b in range(1, 6):
        v = placements(w, b) * tight[w-1] * tight[b-1] * 4
        rows.append((v, w, b))
for v, w, b in sorted(rows, reverse=True)[:8]:
    print(f"  w={w} b={b}: {v:.3e}")
