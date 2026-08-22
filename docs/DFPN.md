# df-pn from the start position: it diverges

The test that decides whether pruning rescues this. `analysis/dfpn.cpp` —
depth-first proof-number search on the validated move generator, claim
*"the player to move at the root can force a WIN"*. OR nodes are the root
player's; AND nodes the opponent's. Mate, stalemate and repetition are terminals;
a draw is not a win, so it disproves the claim.

## It works — on wins

| position | expected | result |
|---|---|---|
| `k3/4/4/4/K2Q w` (K+Q vs k) | White wins | **PROVED, 62 nodes** |
| `k3/4/4/4/K2R w` (K+R vs k) | White wins | **PROVED, 162 nodes** |

## It does not terminate — on draws

The cleanest possible control. `k3/4/4/4/K3 w` — bare kings. The **entire** class
is 540 positions and every one is a draw.

| method | result |
|---|---|
| retrograde fixed point | solved in **0.00 s** — win 0, loss 0, draw 540 |
| df-pn | **10,000,000 nodes, no answer** (pn 765,908,869, dn saturated at 2^30) |

Ten million nodes on a 540-position problem is not inefficiency, it is
non-termination. A draw has no base case in a forward proof search: it is
established only by exhausting alternatives.

## The start position

```
claim: WHITE can force a WIN from the microchess start position
   nodes    1,000,000   root pn     1,251   dn     4,317
   nodes   20,000,000   root pn     7,487   dn    28,113
   nodes   40,000,000   root pn    13,591   dn    49,115
   nodes   60,000,000   root pn    20,172   dn    64,428
   nodes   80,000,000   root pn    25,246   dn    94,962
   nodes   94,000,000   root pn    28,223   dn   137,247
```

Over 93 million nodes the proof number grew **22.6×** and the disproof number
**31.8×**. A proof needs `pn → 0`; a disproof needs `dn → 0`. **Both are receding,
so no budget terminates this search.**

The sibling Tinyhouse project measured the same signature on its own root — pn up
3.5×, dn up 3.1× over 1e9 nodes — with a far more mature df-pn than this one.

## What this settles

Pruning is real and large *for a decisive root* (see `docs/PRUNING.md`: witnesses
of 12–184 nodes against classes of millions). The microchess root is not decisive:
Fairy-Stockfish returns `cp 0` at depth 69, the position is materially symmetric,
and the search above shows neither claim closing.

**So the pruning route does not reach the answer, and the retrograde fixed point
is the only method here that terminates on a drawn root.** Its cost is the
*reachable* state space, not the class index space — which puts the requirement
back at 4–32 GB with symmetry rather than 12 TB.

### Caveat, stated plainly

This df-pn handles cycles naively — a repetition on the current search path is
scored as a draw, which is path-dependent and is itself a known source of
non-termination. A more sophisticated variant might resolve the bare-kings control
that this one cannot. Three independent lines nonetheless point the same way:
this search diverges, Tinyhouse's mature implementation diverged, and the witness
measurement shows a drawn root needs 20–90% of its class regardless of algorithm.
