# The reachable-set measurement — exact for the largest class

This is the number the whole feasibility estimate rested on, and it was the one
thing I had never measured. Now measured exactly for the biggest material class.

## Method

The largest class is the start material with no capture yet (`KBNPRvKBNPR`). No
capture means both pawns are stuck on the d-file — a pawn only leaves its file by
capturing — and both bishops are on their starting square colour. That collapses
the index from 670,442,572,800 placements to **1,349,187,840**, which is a
1.35 GB bitmap: small enough to hold the entire class in RAM, so the BFS runs to
exhaustion and the count is **exact, not sampled and not extrapolated**.

`analysis/topclass.cpp`, full log in `docs/topclass-measurement.txt`.

## Result

```
EXACT reachable positions in the largest class: 732,059,560
  of 10,793,502,720 constrained index slots  ->  6.78 % dense
  of 5,363,540,582,400 naive index slots     ->  0.01365 % dense
```

**Reachability is a 7,327× reduction** on that class — better than the ~500× I had
argued from a bound. The sparsity claim is confirmed and then some.

## But the shape of the curve is bad news

The BFS ran to **ply 33**, peaking around ply 19–22 at ~70M new positions per ply
and then decaying slowly:

| ply | new | cumulative | ratio |
|---:|---:|---:|---:|
| 12 | 5,374,355 | 9,226,662 | 2.31 |
| 14 | 20,125,860 | 40,000,811 | 1.89 |
| 16 | 47,532,703 | 119,602,028 | 1.48 |
| 18 | 67,860,987 | 246,922,283 | 1.14 |
| 20 | 68,837,772 | 385,795,381 | ~1.00 |
| 22 | 69,342,136 | 524,263,131 | ~1.00 |
| 33 | 0 | **732,059,560** | — |

My earlier model projected the ratio declining linearly to 1 and terminating
shortly after — peak around ply 18, done by ply 20. The real curve **plateaus at
ratio ≈ 1 for several plies and then has a long tail**. That model was too
optimistic.

## Re-anchoring the global estimate

The no-capture region went from 9,226,662 at ply 12 to 732,059,560 final — a
**79.3×** multiplier. The global BFS stands at **118,717,620** at ply 12. Applying
the same multiplier:

**≈ 9.4e9 total reachable positions — 2.4× my previous central estimate of 3.9e9.**

And this is a **floor**, not a central value: the global region is *less* constrained
than the no-capture one (pawns are free to leave the d-file once captures start, and
there are 1,272 classes rather than one), so its tail should be longer, not shorter.

## What it costs

At 8.8 B per position (47-bit key + 2-bit value at a 70% hash load factor):

| total reachable | RAM ×2 symmetry | RAM ×4 symmetry | time @1.5 µs |
|---|---:|---:|---:|
| 9.4e9 (floor) | 41 GB | 21 GB | 3.9 h |
| 2e10 (likely) | 88 GB | 44 GB | 8.3 h |
| 3e10 | 131 GB | 66 GB | 12.5 h |

This machine has 32 GB installed and **3–7 GB actually available**.

**Time was never the problem — 4 to 12 hours is fine. Storage is, and it now looks
out of reach here**: the floor needs 21 GB even with the full ×4 symmetry reduction,
and the likely case needs 44 GB.
