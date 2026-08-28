# Handover — microchess solve

Read `FINDINGS.md` for the full state of knowledge. This is the operational
situation: what is running, what is built, and what to do next.

---

## Sitrep

**The goal.** Compute the game-theoretic value of the microchess start position.

**Where we are.** Everything needed to *support* a solve is built and verified.
The solver itself is not. The enumeration phase — the first half, and the phase
that settles the last unmeasured quantity — is built, validated, and **paused at a
clean checkpoint**.

**The run.** `enumerate.exe` reached **ply 14: 674,345,821 positions** and was
stopped deliberately, not by failure. Checkpoint is intact and resumable.

```
store : <scratch>/enum_run          7.9 GB, checkpoint at ply 14
log   : <scratch>/enum_run.log
resume: solver/target/release/enumerate.exe <store_dir> 64
```

Partial-ply scratch (`run_*.keys`) has been cleared, so the checkpoint is clean.
Resuming re-runs ply 15 from scratch, which is correct — stale run files would also
have been correct (sort+dedupe+merge-against-visited absorbs them) but clearing
removes the question.

### What the run has already bought

Two plies nobody had measured before:

| ply | new | cumulative | ratio |
|---:|---:|---:|---:|
| 12 | 76,344,133 | 118,717,620 | 2.656 |
| **13** | **177,411,843** | **296,129,463** | **2.324** |
| **14** | **378,216,358** | **674,345,821** | **2.132** |

The global curve decays **more slowly than the no-capture region** at every
comparable ply (r13 2.32 vs 1.98, r14 2.13 vs 1.89). That was the prediction behind
calling 9.4e9 a *floor* rather than an estimate, and it is now confirmed by
measurement.

Re-projecting from plies 11–14:

| decline assumption | peak | total reachable |
|---|---:|---:|
| faster (−40%) | ply 18 | 4.5e9 |
| **measured** | **ply 19** | **8.4e9** |
| slower (+40%) | ply 22 | 3.4e10 |

The spread is still 7×. Only finishing the enumeration closes it.

---

## Why the run stopped where it did

Not a crash — a known ceiling. `visited` stores keys **raw at 8 bytes**. With
~19 GB free disk that caps out near 2.5–3e9 keys, around ply 16–17. Stopping at
ply 14 was a choice; the wall was a few plies away.

**This is the next thing to fix, and the fix is already measured.**

---

## Next step: key compression

Measured, not assumed (`analysis/keycompress.cpp`, on the real 732 M-key set):

```
mean gap            : 14.74
varint delta B/key  : 1.004     <- 99.56% of gaps fit in ONE byte
bitmap       B/key  : 1.843
raw 6-byte   B/key  : 6.000
```

A geometric-gap model predicted 1.000 against a measured 1.004 — **0.4% error** — so
it can be trusted to extrapolate to the global key density, where it gives
**2.25 B/key**.

| visited format | B/key | 8.4e9 keys | fits ~19 GB free? |
|---|---:|---:|---|
| raw u64 *(what is running now)* | 8.00 | 67 GB | no — stalls at ~ply 16 |
| **delta + varint** | **2.25** | **19 GB** | **just barely** |
| delta + varint, ×2 colour symmetry | 2.25 | 9 GB | comfortably |

### What to change

`solver/src/bin/enumerate.rs`, three functions, nothing else:

* `write_keys` — emit varint deltas instead of `u64::to_le_bytes`. Keys are already
  sorted at every call site.
* `read_keys` — decode the deltas back. Prefer a streaming iterator to a `Vec`, so
  a bucket never has to be fully resident.
* `merge_new` — already a linear two-pointer merge; make it consume two iterators
  and emit varints, so no bucket is ever materialised.

Everything else (bucketing, spill, checkpointing, resume) is unchanged and already
validated. **Re-validate against the ply table below after the change** — the
counts must be identical.

### After compression, in order

1. **Finish the enumeration.** Closes the 4.5e9–3.4e10 spread to a single number.
   This is the last unmeasured input to every cost estimate in the project.
2. **Colour symmetry (×2).** Always valid (colour swap + vertical flip). Halves
   both storage and work. The left–right mirror is only valid once castling rights
   are gone — do not assume ×4.
3. **Then the retrograde solve.** Design notes in `FINDINGS.md` §4. The one
   decision that matters: a **queue/counter** retrograde is O(edges) once (~1.7 h
   of compute at 12 threads); naive sweeps multiply by the sweep count (10–15 h).

---

## Cost, as currently understood

Measured inputs: 81.1 ns/child movegen+make, 154.7 ns/child for the exact codec
key, 74.7 M keys/s radix sort per thread, 101 MB/s sequential on *this* drive
(97% full, budget DRAM-less — not representative).

| | |
|---|---|
| compute, queue/counter retrograde, 12 threads | **~1.7 h** |
| compute, naive sweeps | 10–15 h |
| total I/O traffic | 4–6 TB |
| wall clock on decent NVMe (≥2 GB/s) | **2–4 h**, CPU-bound |
| wall clock on this drive | ~14 h, I/O-bound |
| peak resident data | 50–90 GB |

**Two thirds of the compute is the codec key** (155 ns against 81 ns for the move
itself). A cheaper key — incremental update, or a per-class constrained rank like
the one built for the top class in `analysis/topclass.cpp` — nearly halves total
compute. That is the highest-value optimisation available, and it is untouched.

Capacity and traffic are different things: a few hundred GB of drive is ample for
the 50–90 GB resident; the 4–6 TB is throughput *through* that space over the run,
and is what sets wall clock.

---

## Validation baselines — never let these drift

Any change to rules, codec or enumeration must reproduce all of these.

```
perft 9                     = 176,466,898
codec injective over          118,717,620 positions, max key 2^46.69
```

Reachable positions per ply, cumulative:

```
 1: 10            6: 56,141        11: 42,373,487
 2: 79            7: 246,709       12: 118,717,620
 3: 448           8: 1,021,173     13: 296,129,463
 4: 2,379         9: 3,898,949     14: 674,345,821
 5: 11,872       10: 13,634,481
```

Independent ground truth for solved values is in `GROUND-TRUTH.md` (five material
classes, cross-checked three ways). The largest class is exactly enumerated at
**732,059,560** positions.

---

## Traps already paid for

* **`pkill -f` silently fails here.** It once left *twelve* driver processes
  appending to one results file. Kill by PID via WMI, and use a lock file.
* **Bash `$10` is `${1}0`.** Use `${10}`, or parse key/value pairs with awk.
* **Python patch scripts get their `\n` unescaped** in this shell. Two 30-minute
  runs executed unpatched binaries because a heredoc edit silently did not apply.
  **Verify the change landed in the binary** (`strings foo.exe | grep ...`) before
  starting anything long.
* **Do not test position legality** by asking whether the side to move can capture
  the enemy king. Fairy-Stockfish never generates king captures, so adjacent-king
  positions pass and a *mated* side passes vacuously. Flip the side to move and
  read `Checkers:`.
* **A hash is not an identity.** Anything holding settled values needs the exact
  codec key; 64-bit Zobrist gives ~2.7 expected collisions at 1e10 positions, and a
  collision in a solver is a silently wrong answer.
* **`git checkout bottom-up-tablebase -- <path>`** recovers the removed dense
  tablebase if any of it is wanted back.
