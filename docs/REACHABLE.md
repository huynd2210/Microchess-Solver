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

---

# Key compression — measured

The external-memory design hinges on bytes-per-key, so it was measured on the real
732,059,560-key set rather than assumed (`analysis/keycompress.cpp`, log in
`keycompress-measurement.txt`). The bitmap from the BFS *is* a sorted set, so
walking it in order gives the exact gap distribution.

```
keys                : 732,059,560
mean gap            : 14.74
varint delta B/key  : 1.004   ->  0.74 GB
bitmap       B/key  : 1.843   ->  1.35 GB
raw 6-byte   B/key  : 6.000   ->  4.39 GB

gap histogram by varint size:
  1 byte:  728,869,437  (99.56%)
  2 byte:    3,187,878  ( 0.44%)
  3 byte:        1,999  ( 0.00%)
  4 byte:          246  ( 0.00%)
```

**99.56% of gaps fit in a single varint byte.** A geometric-gap model predicted
1.000 B/key against a measured 1.004 — **0.4% error**, so the model can be trusted
to extrapolate to densities that cannot be measured directly.

## What that means for the whole game

| design | B/key | 9.4e9 (floor) | 2e10 (likely) | 3e10 |
|---|---:|---:|---:|---:|
| **per-class constrained index** | 1.35 | **13 GB** | **22 GB** | 31 GB |
| raw codec key (naive index) | 2.25 | 21 GB | 41 GB | 59 GB |
| raw 6-byte keys, uncompressed | 6.00 | 56 GB | 120 GB | 180 GB |
| hash table, 8.8 B/entry *(the earlier plan)* | 8.80 | 83 GB | 176 GB | 264 GB |

**Compression is worth 4–6.5× over the hash-table plan** — the difference between
83 GB and 13 GB at the floor. It is the single change that brings the run back
inside this machine.

Against **~31 GB free disk**:

* at the 9.4e9 floor, the visited set is 13–21 GB — **fits**;
* at 2e10 it is 22–41 GB — fits only with constrained per-class indexing, and only
  just;
* at 3e10 it does not fit.

Two caveats that eat the headroom:

1. **Sort/merge needs scratch.** External BFS writes runs before merging them, and
   a retrograde pass rewrites the set rather than updating in place, so peak usage
   is roughly 2× the set size transiently. 13 GB set → ~26 GB peak, which is at the
   edge of 31 GB free.
2. **Delta encoding is sequential-access only.** That suits sort/merge exactly, but
   it forecloses random probing — the design cannot fall back on a hash for
   convenience anywhere in the hot path.

---

# Global enumeration — plies 13 and 14 now measured

`solver/src/bin/enumerate.rs` (external-memory BFS, validated against every known
ply count and a resume cycle before launch) extended the global curve past ply 12
for the first time. Log: `enumeration-run.log`.

| ply | new | cumulative | ratio |
|---:|---:|---:|---:|
| 12 | 76,344,133 | 118,717,620 | 2.656 |
| **13** | **177,411,843** | **296,129,463** | **2.324** |
| **14** | **378,216,358** | **674,345,821** | **2.132** |

**The global curve decays more slowly than the no-capture region at every
comparable ply** (r13 2.32 vs 1.98, r14 2.13 vs 1.89). That was the reasoning
behind treating 9.4e9 as a floor rather than an estimate, and it is now confirmed
by measurement rather than argued.

Re-projecting from plies 11-14:

| decline assumption | peak | total reachable |
|---|---:|---:|
| faster (-40%) | ply 18 | 4.5e9 |
| **measured** | **ply 19** | **8.4e9** |
| slower (+40%) | ply 22 | 3.4e10 |

Still a 7x spread. Only finishing the enumeration closes it, and that needs the
key compression described in `HANDOVER.md` — the run stalls near ply 16-17 with
keys stored raw at 8 bytes.
