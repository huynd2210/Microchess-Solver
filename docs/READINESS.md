# Readiness for the full solve — NOT READY

> **SUPERSEDED — see `FINDINGS.md`.** Written while the bottom-up dense tablebase
> was still the plan; that approach has been removed (recoverable via the
> `bottom-up-tablebase` tag) because it cannot reach the 10-piece start position.
> The verified-component table and the measured numbers below remain valid.

Compute is solved. Correctness is solved. **Storage layout is not**, and it is out
by roughly three orders of magnitude. Do not start the run.

## What is verified and ready

| component | evidence |
|---|---|
| rules engine | perft ladder exact to depth 9 (176,466,898); 1,619 positions vs Fairy-Stockfish, 0 mismatches |
| exact codec | injective over **118,717,620** positions; distinct keys == distinct positions at every ply; max key 2^46.69 |
| retrograde algorithm | values match an independent Python+FSF oracle on **5,006** positions across 5 classes; **400/400** on the pawn/promotion class KPvK; KRvKR shows exact colour symmetry |
| parallelism | **bit-identical to serial** at 1/2/6/12 threads across 15 classes; 15.0× speedup, **0.48 µs/position** |

Three independent implementations agree on every reference class: a Python
retrograde solver driven by Fairy-Stockfish (`analysis/oracle.py`), a C++ one
(`analysis/pretro.cpp`), and the Rust one (`solver/src/retro.rs`).

## The blocker: flat per-class arrays cannot reach the top of the DAG

`solver/src/retro.rs` allocates a dense array per material class. Class sizes are
exact combinatorial counts, and they explode:

| class | slots as indexed (placements×8) | memory at the solver's 2.25 B/slot |
|---|---:|---:|
| KBNRvK (measured) | 14,883,840 | 36 s, 595 MiB |
| KRPvKRP (measured, 6-piece) | 223,000,000 | 29 min, 2.1 GiB |
| KBNRPvKBNR (9-piece) | 3,900,756,787,200 | ~8.8 TB |
| **KBNRPvKBNRP** (the start class) | **5,363,540,582,400** | **~12 TB** |

**Correction.** An earlier draft quoted 1.9 TB for the start class. That was the
*pawn-restricted* placement count at 1 byte/slot — a theoretical floor, not what
the code needs. `solver/src/retro.rs` indexes placements as 20P10 =
670,442,572,800 (pawns are rejected on the promotion rank at the legality stage,
but slots are still allocated for them) and stores 1 B of value + 1.25 B of packed
board per slot. The real requirement is **12 TB**, matching the agent's own figure.
Even the absolute floor — pawn-restricted placements at 2 bits/slot — is 0.47 TB.
The conclusion is unchanged across every density; only the size of the gap moves.

The solver does have a **streamed** mode (~4.9× slower, ~14× less memory), verified
by the agent to give byte-identical counts. It does not help here: the saving is on
the edge cache, which is only used below 16.7M slots anyway. The value array alone
is 5.4 TB.

The agent reached the same conclusion independently and said so plainly: 8-piece
classes are the first infeasible ones (~91 GB by its accounting), the start class
~12 TB and ~330 days for that one class, and *"class-complete solving does not
reach the full game on a one-week budget on this hardware, by roughly three orders
of magnitude in memory and more in time."*

This box has 32 GB RAM (~12 GB free) and **4 GB free disk**.

## Why this is fixable, not fatal

The flat array sizes the *whole class*; the solve only needs the **reachable**
subset, and for the biggest class that is ~172× smaller. In the start class no
captures have happened, so both pawns are stuck on the d-file and both bishops are
colour-bound: **1,349,187,840** placements against 232,890,577,920 — the exact
figure, computed in `analysis/nocap_analytic`-style enumeration and consistent with
the measured fact that no pawn promotes without a capture (0 across 40,000,811
no-capture positions to ply 14).

Total reachable across all classes extrapolates to **1.8e9 – 1.5e10** (central
3.9e9) from the measured BFS decay curve. At the verified parallel rate of
0.48 µs/position that is **31 minutes to 2 hours of compute** — compute was never
the constraint.

## What must be built before a go signal

1. **Reachability-driven storage** for classes above ~7 pieces: the exact codec key
   in a compact hash set, instead of a dense per-class array. The codec and TT
   already exist and were built for exactly this. **With symmetry — it is not
   optional.** A sparse entry must carry its key: 47-bit key + 2-bit value = 6.1 B,
   ~8.8 B/position at a 70% load factor.

   | total reachable | no symmetry | ×2 (colour) | ×4 (colour+mirror) |
   |---|---:|---:|---:|
   | 1.8e9 (low) | 15.8 GB | 7.9 GB | 3.9 GB |
   | 3.9e9 (central) | **34.1 GB** | 17.1 GB | 8.5 GB |
   | 1.48e10 (high) | 129.5 GB | 64.8 GB | **32.4 GB** |

   Without symmetry the *central* estimate already exceeds this 32 GB box, and the
   high estimate does not fit even with it. And ×4 is optimistic: colour-swap +
   vertical flip always applies (×2), but the left-right mirror only applies once
   castling rights are gone, because the rook sits on the d-file.
2. **Drop the 4× castling inflation.** Slots are `placements × 2 × 4`, but three of
   the four castling-rights states are impossible for most positions (the solver
   currently enumerates e.g. `4/3K/4/k2R/4 b d` — Black rights with no black rook
   and the king off a5). Rights must be a constrained dimension, not free bits.
3. **Port the verified parallel sweep into Rust.** The C++ prototype proves the
   design: monotone updates mean lock-free in-place sweeps converge to the same
   fixed point, confirmed bit-identical at 12 threads with iteration counts varying
   (21/20/19/19) while results do not.
4. **Measure the total reachable count.** This is the load-bearing assumption and
   it is *not* measured. The BFS is confirmed only to ply 12 (118,717,620); an
   attempt to reach ply 13 was abandoned twice because it exhausts RAM on this
   box. Everything above rests on projecting the growth-ratio decline
   (4.66 → 2.66, mean −0.330/ply) past the last data point. If the decline
   flattens, the total rises steeply and the storage plan fails before the
   compute plan does.
5. **Free disk space.** 4 GB free of 953 GB leaves no room for checkpoints, and a
   crash mid-run would cost the whole computation.

## On splitting the root into subtrees

Measured rather than argued: BFS from each of the 9 root moves to depth 10 reaches
36,680,621 positions summed, against a union of 13,634,481 — a **2.69× duplication
factor**, with 1.7× load imbalance, and it worsens with depth as subtrees converge
on shared material classes. A shared TT removes the duplication but then the
"subtrees" are not independent: every thread contends on one structure and the
split buys no locality.

The better decomposition for this architecture is the material-class DAG:
classes at the same level are **completely independent** (no shared state at all),
and within a class the index range partitions cleanly. That is what was implemented
and verified bit-identical. Keep the shared-TT/subtree idea for the forward
*exploration* phase, where it does apply.
