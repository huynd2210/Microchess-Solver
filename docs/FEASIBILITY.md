# Feasibility: can microchess be solved in one week on this machine?

> **SUPERSEDED — see `FINDINGS.md`.** The verdict below ("a one-week solve fits,
> with margin") rested on an extrapolated reachable total of 3.9e9. That total has
> since been re-anchored on an exact measurement to **at least 9.4e9**, and the
> storage requirement now exceeds this machine. The per-class timings and the
> ground-truth table below remain valid.

**Yes, with margin.** Central estimate ~11 hours single-threaded; the pessimistic
corner still fits inside a week. The binding constraint is storage, not compute,
and storage fits in RAM.

## The two numbers

**Rate — measured, not assumed.** An independent C++ retrograde solver
(`analysis/retro.cpp`, built on the perft-validated generator) solving every
"white material vs bare black king" class bottom-up, single-threaded, naive
repeated-scan fixed point:

| class | positions | iters | time | µs/position |
|---|---:|---:|---:|---:|
| KRvk | 7,950 | 12 | 0.06 s | 7.3 |
| KNBvk | 137,996 | 21 | 2.79 s | 20.2 |
| KNBRvk | 1,936,252 | 8 | 25.2 s | 13.0 |
| **KNBRQvk** | **24,374,736** | — | **186.3 s** | **7.6** |

Cost per position does **not** grow with piece count — it fell, because more
material converges in fewer iterations. Working figure: **10 µs/position**.

**Size — extrapolated, and this is the load-bearing assumption.** Distinct
reachable positions by BFS, confirmed by two independent implementations at every
ply (118,717,620 at ply 12). Per-ply growth decays steadily — 4.66, 4.31, 4.06,
3.72, 3.38, 2.95, 2.66 — declining a mean of 0.330 per ply. Projecting that decline
to where the ratio crosses 1:

| | total reachable | peak ply |
|---|---:|---:|
| faster decay (−30%) | 1.80e9 | 16 |
| **measured mean decline** | **3.91e9** | **18** |
| slower decay (+30%) | 1.48e10 | 20 |

## The verdict

One week = 604,800 core-seconds.

| reachable total | time @ 10 µs | fraction of a week | with a 3× penalty for both-sided classes |
|---|---:|---:|---:|
| 1.8e9 (low) | 5.0 h | 1/34 | 15 h |
| **3.9e9 (central)** | **10.8 h** | **1/16** | **32 h** |
| 1.48e10 (high) | 41.1 h | 1/4 | 123 h — still inside a week |

Even the worst corner — the pessimistic size estimate *and* a 3× cost penalty for
classes with material on both sides (my measurements used a bare black king, which
has fewer replies) — lands at 1.4× inside the budget.

Storage at 4 bytes/position with the ~4× symmetry reduction: **1.8–14.8 GB**, which
fits the 32 GB of RAM.

## What would break this

* **The size extrapolation.** It is a projection of a decaying ratio, not a
  measurement. If growth persists two plies longer than projected the total is ~5×
  higher and the margin is gone. Measuring the true total is the single most
  valuable next experiment.
* **Disk: only 4 GB free of 953 GB.** The solve must be RAM-resident and there is
  no room for large checkpoints. Free space before a long run, or a crash costs the
  whole computation.
* **Flat per-class indexing does not work.** Summed over reachable classes the
  index space is 4.319e12, needing 0.14 µs/position to fit a week — an order of
  magnitude beyond the measured rate. Storage must follow *reachable* positions
  (my probe wasted 3.3× on 20^k indexing even for 5 pieces). This is why the
  approach must be reachability-driven, which is what AO*/LAO* with the exact codec
  gives.

## Correctness — three independent implementations agree

Values from the side to move's point of view; see `docs/GROUND-TRUTH.md`.

| class | positions | win | loss | draw |
|---|---:|---:|---:|---:|
| KvK | 540 | 0 | 0 | 540 |
| KNvK | 8,904 | 0 | 0 | 8,904 |
| KBvK | 8,712 | 0 | 0 | 8,712 |
| KRvK | 7,950 | 3,090 | 3,672 | 1,188 |
| KQvK | 6,942 | 2,082 | 3,472 | 1,388 |

Agreeing on every cell: (1) `analysis/oracle.py`, a Python retrograde solver whose
move generation comes from **Fairy-Stockfish over UCI**; (2) `analysis/retro.cpp`,
a C++ retrograde solver on the perft-validated generator. Additionally 120/120
random KRvK positions match Fairy-Stockfish's own mate scores, and all six known
individual positions are correct. `KvK = 540` is exact by hand:
2 × (20×19 − 110 ordered adjacent-king pairs).
