# Microchess: state of knowledge

**The question.** What is the game-theoretic value of the microchess start
position, and what would it cost to compute?

**The answer.** ~4–12 hours of compute and **21–44 GB of resident state**. Compute
was never the constraint. Memory is, and it does not fit this machine — which is
why the next step is an external-memory solver.

This document supersedes the conclusions in `FEASIBILITY.md` and `READINESS.md`;
their *measurements* remain valid, their verdicts do not.

**For the operational picture — what is running, what to do next — see
`HANDOVER.md`.**

---

## 1. What is verified and can be relied on

| component | evidence |
|---|---|
| **Rules** (`solver/src/movegen.rs`) | perft ladder exact to depth 9 = **176,466,898**, cross-validated against Fairy-Stockfish and an independent C++ generator; 1,619 random positions x 2 depths vs FSF, 0 mismatches; targeted castling/promotion positions, 0 mismatches |
| **Exact key** (`solver/src/codec.rs`) | injective over **118,717,620** positions — distinct keys *equal* distinct positions at every ply; max key 2^46.69, inside a u64 |
| **Transposition table** (`solver/src/tt.rs`) | hash selects the bucket, the full 64-bit key is stored and compared, so a collision costs a probe and never a value |
| **Retrograde fixed point** | values matched an independent Python+FSF oracle on **5,006** positions across 5 classes, and **400/400** on the pawn/promotion class KPvK |
| **Parallelism** | bit-identical to serial at 1/2/6/12 threads across 16 classes; **15.0x** speedup, **0.48 us/position**; safe because the update is monotone (a slot goes UNKNOWN to WIN/LOSS and never changes) |
| **Checkpoint/resume** | lossless across three kill points (between classes, during pass 1, mid-sweep with partial pages flushed); peak *private* RAM 150 MB to 2 MB via mmap, at 6.8% wall-time cost |

Three independent implementations agree on every reference class: a Python
retrograde solver driven by Fairy-Stockfish, a C++ one, and the Rust one.

## 2. Three routes; two are dead

### Forward proof search (AO*/df-pn) — diverges

`analysis/dfpn.cpp`. It proves wins fine: K+Q vs k in **62 nodes**, K+R vs k in
**162**. It cannot establish a draw:

* **Bare kings** — 540 positions, every one a draw. Retrograde: **0.00 s**.
  df-pn: **10,000,000 nodes, no answer.** That is non-termination, not slowness.
* **Start position** — over 93 M nodes the proof number grew **22.6x**
  (1,251 to 28,223) and the disproof number **31.8x** (4,317 to 137,247). A proof
  needs pn to reach 0, a disproof dn to reach 0. Both receding: no budget
  terminates it.

The sibling Tinyhouse project measured the same signature with a more mature
implementation. A draw has no base case in a forward search; it is established
only by exhausting alternatives.

*(Caveat: this df-pn uses naive path-based cycle handling. Two independent lines
agree with it anyway — see the witness measurement below, and Tinyhouse.)*

### Pruning — real, but not for the case we are in

`analysis/witness.cpp` measures the witness: the strategy DAG proving one root.

| root value | witness size |
|---|---|
| **WIN** | median 12–184 nodes, max **0.09–5.8%** of the class — a 10^2–10^4 saving |
| **DRAW** | **20–90%** of the class (KvK 20.4%, KBvK 43.7%, KNvK 90.4%) |

The microchess root is almost certainly a draw (FSF returns `cp 0` at depth 69;
the position is materially symmetric). Pruning buys ~1.1–5x there, not orders of
magnitude. Tinyhouse independently measured a drawn witness at 16–24% of its whole
game.

### Bottom-up dense tablebase — cannot reach the start position

Removed (recoverable: `git checkout bottom-up-tablebase -- <path>`). The start
position has 10 pieces; a bottom-up tablebase reaches it last. Dense per-class
arrays wall at 8 pieces on memory, and the start class alone needs **12 TB**. A
110-class run confirmed the algorithm is correct and the approach is a dead end.

### What survives: reachability-driven retrograde

Store only positions that can actually occur, keyed by the exact codec key. This is
the only route that both terminates on a drawn root and fits any plausible budget.

## 3. The measurement that decided it

The whole estimate rested on one unmeasured quantity: how many positions are
reachable. Now measured **exactly** for the largest class (`analysis/topclass.cpp`,
log in `topclass-measurement.txt`).

The largest class is the start material with no capture yet. No capture means both
pawns are stuck on the d-file (a pawn leaves its file only by capturing) and both
bishops stay on their starting colour — collapsing the index from 670,442,572,800
placements to 1,349,187,840, a **1.35 GB bitmap**. Small enough to hold entirely in
RAM, so the BFS runs to exhaustion and the count is exact.

```
EXACT reachable positions in the largest class: 732,059,560
  of 10,793,502,720 constrained slots  ->   6.78 %
  of 5,363,540,582,400 naive slots     ->   0.01365 %   = a 7,327x reduction
```

**Sparsity confirmed and understated** — I had argued ~500x from a bound; measured
is 7,327x.

**But the shape of the curve was the real finding.** It plateaus at growth ratio
about 1.0 across plies 19–22 and tails all the way to **ply 33**:

| ply | new | cumulative | ratio |
|---:|---:|---:|---:|
| 12 | 5,374,355 | 9,226,662 | 2.31 |
| 16 | 47,532,703 | 119,602,028 | 1.48 |
| 18 | 67,860,987 | 246,922,283 | 1.14 |
| 20 | 68,837,772 | 385,795,381 | ~1.00 |
| 22 | 69,342,136 | 524,263,131 | ~1.00 |
| 33 | 0 | **732,059,560** | — |

My earlier model projected a linear ratio decline, peak around ply 18, done by ply
20. It was wrong in the direction that flattered the estimate.

**Re-anchored global total.** The no-capture region multiplied its ply-12 count by
**79.3x** on the way to exhaustion. The global BFS stands at 118,717,620 at ply 12.
Same multiplier gives **about 9.4e9**, which is **2.4x my previous central
estimate** — and a *floor*, since the global region is less constrained (pawns free
after the first capture, 1,272 classes rather than one) and should tail longer.

## 4. What it costs, and why external memory is now required

At 8.8 B per position (47-bit key + 2-bit value, 70% hash load factor):

| total reachable | x2 symmetry | x4 symmetry | compute @1.5 us |
|---|---:|---:|---:|
| 9.4e9 (floor) | 41 GB | 21 GB | 3.9 h |
| 2e10 (likely) | 88 GB | 44 GB | 8.3 h |
| 3e10 | 131 GB | 66 GB | 12.5 h |

Symmetry: colour-swap + vertical flip always applies (x2); the left–right mirror
only applies once castling rights are gone, because the rook sits on the d-file —
so x4 is a late-game bonus, not a planning assumption.

**This machine:** i5-14600K (6 P-cores + 8 E-cores), 32 GB installed with **3–7 GB
actually available** (25 GB held by other applications, 46 GB committed of a 55 GB
limit), **~21 GB free disk**, SSD.

So neither RAM nor current free disk holds the working set. External memory is not
an optimisation here; it is the only remaining route on this hardware — and it needs
**more free disk than currently exists**.

### Constraints any external-memory design must respect

* **Sequential, not random.** A random-access hash on disk is fatal: ~9e9 positions
  times ~8.9 children is ~8e10 probes; at SSD random-read latency that is months.
  The design must be sort/merge based so all I/O is streaming.
* **Monotonicity is the gift.** A slot only goes UNKNOWN to WIN/LOSS, so a partially
  written page cannot corrupt (every byte is old-or-new, both legal) and any pass
  can be re-run from a partial state. Checkpointing therefore needs almost no
  machinery — this was verified, see `CHECKPOINTING.md`.
* **Keys compress.** 9.4e9 sorted keys drawn from a 2^47 space have a mean gap of
  ~15,000, so delta+varint encoding should reach ~2 B/key rather than 6 — roughly
  19 GB instead of 56 GB for the visited set. This is the single highest-leverage
  design choice and it is **unverified**.
* **Frontier must be re-expandable.** A Zobrist key is not invertible; the exact
  codec key is. Storing frontier *keys* and decoding them beats storing packed
  positions (6 B vs 11 B).

## 5. Corrections I made to my own claims

Recorded because each was wrong in a way that favoured the plan:

| claim | correction |
|---|---|
| "the start class needs 1.9 TB" | **12 TB**. I quoted a pawn-restricted count at 1 B/slot — a floor, not the implementation's requirement |
| "2,304 material classes" | **1,296**. 12 of the 48 per-side labels alias; found by the codec's own random-key test |
| "symmetry is a nice-to-have" | **required** — without it even the old central estimate exceeded 32 GB |
| "total reachable is about 3.9e9" | **at least 9.4e9**, re-anchored on the measured curve |
| parallel sweep | contained a formal data race (plain `uint8_t`); fixed with relaxed atomics, identical results |
| my own oracle | its first legality test asked whether the side to move could capture the enemy king. Fairy-Stockfish never generates king captures, so adjacent-king positions passed, and a *mated* side passed vacuously. It admitted 408 illegal positions and produced 84 phantom losses. Correct test: flip the side to move and read `Checkers:` |
| the ladder run | `pkill -f` silently failed and left **twelve** drivers appending to one file; and bash `$10` means `${1}0`, which corrupted a column |

## 6. Open

1. **The global reachable total is still a floor, not a measurement.** 9.4e9 is the
   no-capture multiplier applied to the global ply-12 count. Measuring it properly
   needs the external-memory machinery itself.
2. **Free disk.** ~21 GB against a working set of 19 GB (best case, compressed) to
   56 GB (uncompressed). The run needs headroom for merge scratch on top.
3. **The df-pn caveat.** Naive cycle handling; a better variant might resolve the
   bare-kings control. It would not change the conclusion — the witness measurement
   reaches it independently.
