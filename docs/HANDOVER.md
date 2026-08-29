# Handover — microchess solve

Read `FINDINGS.md` for the full state of knowledge. This is the operational
situation: what is running, what is built, and what to do next.

---

## Sitrep

**The goal.** Compute the game-theoretic value of the microchess start position.

**Where we are.** Everything needed to *support* a solve is built and verified.
The solver itself is not. The enumeration phase — the first half, and the phase
that settles the last unmeasured quantity — is built, validated, and **running**.

**The run.** Varint-compressed storage, parallel expansion, bounded buffers.

```
store : <scratch>/enum_run          ~7 GB at ply 16
log   : <scratch>/enum_run.log      appended across runs, never truncated
pid   : <scratch>/enum_run.pid      kill by this, never `pkill -f`
resume: solver/target/release/enumerate.exe <store_dir> 64
tune  : ENUM_THREADS (default = logical/2), ENUM_BUF_MK (default 128)
```

Resuming is always safe: the store is a checkpoint after every ply and is
crash-safe *within* a ply too (see **Restartability** below). **Never clean a
store by hand** — the enumerator sweeps stale scratch itself, and only when it is
safe to. See the traps.

**Blocked on disk.** At ply 16 the store is ~7 GB and the next ply needs several
more; the revised projection wants 50–105 GB to finish. That decision has to be
made before the run can continue.

### Plies measured so far

| ply | new | cumulative | ratio |
|---:|---:|---:|---:|
| 12 | 76,344,133 | 118,717,620 | 2.656 |
| 13 | 177,411,843 | 296,129,463 | 2.324 |
| 14 | 378,216,358 | 674,345,821 | 2.132 |
| **15** | **731,249,316** | **1,405,595,137** | **1.933** |
| **16** | **1,328,299,642** | **2,733,894,779** | **1.817** |

The global curve decays **more slowly than the no-capture region** at every
comparable ply (r13 2.32 vs 1.98, r14 2.13 vs 1.89). That was the prediction
behind calling 9.4e9 a *floor* rather than an estimate, and it is confirmed by
measurement.

### The projection has moved sharply upward — read this before planning anything

The ratio's *decline* is decelerating. Per-ply drop: −0.192, −0.199, then only
**−0.117** from ply 15 to 16. A later peak compounds hard.

| model fitted to plies 14–16 | peak | total reachable |
|---|---:|---:|
| ratio falls −0.158/ply | ply ~21 | **3.4e10** |
| ratio falls ×0.923/ply | ply ~23 | **8e10** |

**This document previously carried 8.4e9 as the central estimate. That is low by
roughly 4–10×, and the old *upper* bound of 3.4e10 is now the optimistic case.**
The old figure was extrapolated from ply 12; this one is fitted to five measured
plies, three of which did not exist when the old number was written.

Compression will not rescue it: density gain has nearly stalled (1.1982 B/key at
ply 15, 1.1812 at ply 16), so bytes now scale almost linearly with keys.

Consequences, which reach past the enumeration:

| | 3.4e10 | 8e10 |
|---|---:|---:|
| peak disk (visited + frontier + runs) | ~50 GB | ~105 GB |
| enumeration wall clock | ~8 h | ~24 h |
| with colour symmetry (×2) | ~25 GB, 4 h | ~53 GB, 12 h |

`FINDINGS.md` sizes the retrograde solve at 21–44 GB resident **on the 8.4e9
figure**; that step scales with the same quantity and is therefore 4–10× larger
than recorded. **Colour symmetry is no longer an optimisation, it is close to a
precondition.**

Only finishing the enumeration settles it — but the range to plan against is now
3.4e10–8e10, not 4.5e9–3.4e10.

---

## What changed since the ply-14 pause

Two blockers were removed. Both are done, tested, and committed.

### 1. Key compression — the disk ceiling is gone

`visited` stored keys raw at 8 B, which capped the store near ply 16. Keys now go
to disk as **LEB128 delta-gaps** (`solver/src/keystream.rs`), and every pass —
read, k-way merge, union write — is streaming, so no bucket is ever materialised
and RAM does not track bucket size.

**Measured on the real ply-14 set: 1.283 B/key, 6.23× smaller.** The store went
8.42 GB → 1.3 GB; free disk went 15 GB → 22 GB. This beats the 2.25 B/key that
was projected from a uniform-gap model, because reachable keys cluster hard
inside a material class — exactly what a gap encoding is paid to exploit.

Density improves as the set fills, but the gain has flattened: 1.283 B/key at ply
14, 1.198 at 15, 1.181 at 16. Do not plan on it reaching 1.0 — from here bytes
scale essentially linearly with keys.

`recompress` converts a raw store in place. It reads each encoded bucket back and
compares key-for-key **before** replacing the raw file, and records per-bucket
progress so an interrupted conversion resumes.

### 2. Parallel expansion — ~5× on the part that costs

Expansion is the whole cost of a ply. Threads claim whole frontier buckets from
one atomic cursor; run files carry the writing thread's id. Consolidation stays
serial (~10% of a ply), merging every run file for a bucket regardless of writer,
which keeps the count exact by construction.

**Measured at ply 12: 76.6 s wall against 299 s serial; expansion 39.0 s against
194 s.** Scaling is flat past the physical core count — expand took 37.6 s on 10
threads, 37.8 on 14, 39.0 on 20 — because the loop is bound by memory traffic
through the codec tables, not ALU work, so SMT buys nothing. Default is half of
`available_parallelism`.

That the threading is invisible in the result is **asserted, not argued**:
`tests/parallel_identity.rs` runs a 1-thread and an 8-thread store to ply 10 and
requires them byte-identical file for file, plus thread counts 1/2/3/5/16 all
agreeing, plus both matching the ply ladder — identity alone would be satisfied
by two runs wrong in the same way. It squeezes the buffer to 1 Mkey so threads
spill repeatedly into the same buckets, which is where a naming collision or a
dropped run would surface.

### 3. Restartability — a ply is now crash-safe partway through

Previously consolidation overwrote frontier buckets while updating `visited` in
place. An interrupted ply left a store that could neither resume (visited was
partly advanced) nor re-expand (the frontier it needed was gone). With free disk
close to the projected peak, that was a likely event, not a hypothetical.

Now a ply commits in a fixed order — `consol.txt` (per-bucket progress) →
`swap.txt` (frontier swap barrier) → `ply.txt` — and every step is idempotent on
replay. The next frontier is written beside the current one and swapped only once
all buckets are done; `visited` is rewritten to a sibling and renamed over, never
truncated in place. Run files are discovered from the directory, so a resumed
consolidation sees exactly what expansion left behind.

### 4. Bounded spill buffers — ply 16 was exhausting RAM

Retaining per-bucket buffer capacity across spills let each of the
`BUCKETS × threads` buffers ratchet to its own high-water mark, so resident memory
tracked the *sum of per-bucket peaks* rather than the peak of the sum. Bucket
occupancy is skewed, so it overshot the budget and climbed ply over ply until
ply 16 died on a failed 64 MB allocation.

Buffers are now preallocated at an even share (×2 for skew) and shrunk back to it
after each spill; peak resident is bounded at roughly `3 × ENUM_BUF_MK`, measured
at **1.11 GB** against 3.5 GB and rising before. The ratchet cost speed too —
ply 12 went **76.6 s → 34.5 s** once several GB of fragmented buffers stopped
evicting the codec tables. Wall time is flat from 16 to 128 Mkeys, so the budget
is a memory knob, not a speed one.

---

## Next, in order

1. **Colour symmetry (×2) — now first, ahead of finishing the run.** Always valid
   (colour swap + vertical flip). Halves storage *and* work, and at the revised
   projection it is the difference between fitting on this machine and not. The
   left–right mirror is only valid once castling rights are gone — do not assume
   ×4. It changes what a ply count *means*, so validate it against the ladder
   below rather than assuming the counts should still match.
2. **Finish the enumeration.** Closes the 3.4e10–8e10 spread to a single number.
   The last unmeasured input to every cost estimate in the project.
3. **A cheaper key — now the highest-value optimisation by a wide margin.** See
   the corrected cost below.
4. **Then the retrograde solve.** Design notes in `FINDINGS.md` §4. The decision
   that matters: a **queue/counter** retrograde is O(edges) once; naive sweeps
   multiply by the sweep count.

---

## Cost — corrected, and worse than previously recorded

**`codec::encode` costs ~520 ns/child, not the 154.7 ns this document used to
claim.** The old figure is real but only holds below roughly 3 M resident
positions. `prodrate` reproduces 154.7 ns exactly at depth 6 and holds to depth 7,
then falls off a cache cliff:

```
depth 5   81.7 ns movegen+make   156.8 ns codec key
depth 6   88.1                   154.7          <- where the old figure came from
depth 7   80.1                   158.1
depth 8   82.2                   521.4          <- real working-set behaviour
```

It is not the build (A/B'd with and without LTO) and not the spill buffer size
(A/B'd 32 vs 192 Mkeys — no effect). `movegen + make` is unchanged at ~81 ns, so
the regression is entirely the codec's table lookups being evicted.

Consequences:

* **Compute, not disk, is now the binding constraint.** Compression turned a hard
  disk ceiling into ~1.3 GB at ply 14; nothing suggests disk will bind again.
* The enumeration is roughly **8–24 h of wall clock** across the revised
  projection, rather than the 2–4 h this document used to imply. Measured rates
  at ply 16, after the buffer fix: **0.568 µs per frontier key** to expand,
  **0.039 µs per visited key** to consolidate.
* **A cheaper key is worth more than anything else on the list.** It is ~85% of
  expansion cost. Incremental key update, or a per-class constrained rank like
  the one in `analysis/topclass.cpp`, is the obvious attack. Untouched.

Other measured inputs, unchanged: 74.7 M keys/s radix sort per thread, 101 MB/s
sequential on *this* drive (98% full, budget DRAM-less — not representative).

Capacity and traffic are different things: a few hundred GB of drive is ample for
the resident set; the multi-TB figure is throughput *through* that space over the
run.

---

## Validation baselines — never let these drift

Any change to rules, codec or enumeration must reproduce all of these.

```
perft 9                     = 176,466,898
codec injective over          118,717,620 positions, max key 2^46.69
visited encoding              1.283 B/key on the ply-14 set (674,345,821 keys)
```

Reachable positions per ply, cumulative:

```
 1: 10            6: 56,141        11: 42,373,487    16: 2,733,894,779
 2: 79            7: 246,709       12: 118,717,620
 3: 448           8: 1,021,173     13: 296,129,463
 4: 2,379         9: 3,898,949     14: 674,345,821
 5: 11,872       10: 13,634,481    15: 1,405,595,137
```

Plies 15 and 16 are new. They were measured once, then reproduced by a
from-scratch rebuild after the store was lost, so they are confirmed twice.

`cargo test --release` covers all of it — 53 tests. The ones that matter most
here: `perft_ladder`, `codec` injectivity, `keystream` (LEB128 byte-count
boundaries, streams spanning many I/O buffers, k-way dedup), and
`parallel_identity` (serial/parallel byte-identity + the ladder).

A from-scratch re-run of the enumerator reproduced the ladder exactly through ply
12 after the compression change, and again after parallelisation.

Independent ground truth for solved values is in `GROUND-TRUTH.md` (five material
classes, cross-checked three ways). The largest class is exactly enumerated at
**732,059,560** positions.

---

## Traps already paid for

* **Never delete `run_*` / `.next` / `.tmp` from a store by hand.** They look
  like scratch, but while a consolidation is in flight they *are* the recovery
  state. Clearing them 183/256 buckets into ply 17 left `visited` mixed across
  two plies with the new frontier unrecoverable, and cost a full rebuild. The
  enumerator now sweeps them itself at startup, and only when no consolidation
  is in progress. Check `consol.txt` before touching a store.
* **`pkill -f` silently fails here.** It once left *twelve* driver processes
  appending to one results file. Kill by PID via WMI, and use a lock file.
* **Heredocs into this shell mangle multi-line content.** A `cat > file <<'EOF'`
  of a Rust file died on an apostrophe in a comment; earlier, Python patch
  scripts got their `\n` unescaped and two 30-minute runs executed unpatched
  binaries. Write source files with the editor tool, and **verify the change
  landed** before starting anything long.
* **Bash `$10` is `${1}0`.** Use `${10}`, or parse key/value pairs with awk.
* **`bc` is not installed.** Do arithmetic in awk, or in the program.
* **Benchmark numbers are only valid at the working set they were taken at.**
  The 154.7 ns codec figure was right and still misled the whole cost model for
  weeks because nobody recorded that it was a depth-6 measurement. Record the
  scale beside the number.
* **Do not test position legality** by asking whether the side to move can
  capture the enemy king. Fairy-Stockfish never generates king captures, so
  adjacent-king positions pass and a *mated* side passes vacuously. Flip the side
  to move and read `Checkers:`.
* **A hash is not an identity.** Anything holding settled values needs the exact
  codec key; 64-bit Zobrist gives ~2.7 expected collisions at 1e10 positions, and
  a collision in a solver is a silently wrong answer.
* **`git checkout bottom-up-tablebase -- <path>`** recovers the removed dense
  tablebase if any of it is wanted back.
