# RUNBOOK — how to pick up the microchess solve

You have been handed a repository mid-project. This file is the operating
manual: what the project is, what is already true, how to build and run it, and
the specific ways it has gone wrong before. It is written to be read cold, by
someone (or something) with no memory of the previous sessions.

**If you just want to know what to do, read `docs/SOLVE.md` instead** — it is the
work order, in order, with an acceptance test at every step. This file is the
reference manual behind it.

Read this first, then `docs/FINDINGS.md` for the state of knowledge, then
`docs/HANDOVER.md` for the strategic picture. Where those two disagree with this
file, **this file wins** — see [§12](#12-known-stale-figures-in-the-other-docs).

---

## 1. The project in one page

**Goal.** Compute the game-theoretic value of the microchess start position —
whether the opening array `kbnr/3p/4/3P/KBNR w Dd - 0 1` is a win, loss, or draw
under perfect play. Not "what does an engine think", but a proof.

**Microchess** is a 4-file × 5-rank chess variant. Standard pieces and standard
movement, three differences: pawns have no double step (hence no en passant),
promotion happens on rank 5 / rank 1 and is mandatory, and castling is one move
per side (`a1c1` / `a5c5`) because there is a single rook, on the d-file.
`docs/SPEC.md` is the exact rules, every clause verified against
Fairy-Stockfish.

**Why this is solvable when the sibling project was not.** The predecessor
attempt (`Tinyhouse`, 4×4 crazyhouse) failed because captured pieces return to
the board, so material never decreases, so the whole state space is one strongly
connected blob with no decomposition and no base case for a draw. In microchess
**a captured piece is gone forever**. Material only decreases. That gives an
acyclic *material-class DAG* — captures and promotions always move strictly
down it — so cycles (and therefore repetition draws) are confined inside a single
class, where a fixed-point iteration settles them with no frontier to leak
through. This decomposition is the entire reason to expect success.
`docs/ARCHITECTURE.md` argues it in full; do not discard it.

**The plan, in two halves.**

1. **Enumerate** every reachable position, breadth-first from the start, on
   disk. *This is what is currently running, and it is half-finished.*
2. **Solve** it backwards — a retrograde fixed point walking the class DAG
   bottom-up. *This is not built.*

**Where it stands.** The enumerator is built, tested, parallel, crash-safe, and
has reached **ply 16 = 2,733,894,779 reachable positions**. It is paused on a
resource decision, not on a bug. See [§8](#8-the-open-decision).

---

## 2. What is done and what is not

| | status | evidence |
|---|---|---|
| Rules engine | **done** | perft 9 = 176,466,898, cross-checked against Fairy-Stockfish and an independent C++ generator |
| Exact position key (codec) | **done** | injective over 118,717,620 positions; max key 2^46.69, fits `u64` |
| Transposition table | **done** | hash addresses the bucket, the full key is stored and compared |
| Material-class algebra | **done** | 1,296 classes, 1,272 reachable |
| Varint key storage | **done** | 1.18 B/key measured on the real ply-16 set |
| Parallel expansion | **done** | asserted byte-identical to serial |
| Crash-safe checkpointing | **done** | idempotent three-step ply commit |
| Colour symmetry (the map itself) | **done** | `solver/src/symmetry.rs`, 11 tests |
| Colour symmetry **wired into the enumerator** | **NOT done** | this is the open decision |
| Enumeration run to completion | **NOT done** | at ply 16; peak projected at ply 21–23, with a long tail after it |
| Retrograde solver | **NOT done** | design notes only, in `docs/FINDINGS.md` §4 |

Two solving routes were tried and are **ruled out by measurement**, not by
opinion. Do not propose them again:

* **Forward proof search (df-pn / AO\*)** diverges on a drawn root. On bare
  kings — 540 positions, every one a proven draw — it burned 10,000,000 nodes
  with no answer, while the retrograde solver did it in 0.00 s. From the start
  position, over 93 M nodes both the proof and disproof numbers *grew*
  (22.6× and 31.8×). That is non-termination. `docs/DFPN.md`.
* **A dense bottom-up tablebase** cannot reach a 10-piece start position. Dense
  per-class arrays wall at 8 pieces; the start class alone needs 12 TB. A
  110-class run confirmed the algorithm was correct and the approach hopeless.
  Removed from the tree; recover with the tag (see [§11](#11-git-hygiene-before-you-push)).

---

## 3. Setup

### Prerequisites

**Rust only.** The solver crate has *zero* third-party dependencies — check
`solver/Cargo.lock`, it lists one package, itself. So the build works offline and
nothing can rot out from under you.

```bash
rustc --version
```

Developed on `rustc 1.96.0 / cargo 1.96.0`, edition 2021. Anything reasonably
recent will do; there is no unstable feature use.

**Platform.** The solver is pure `std` and builds on Windows, Linux, and macOS
alike. It was developed on Windows 11. The only Windows-specific things in the
repo are the *validation* tools, which you do not need to run the enumeration:

* `engine/fairy-stockfish.exe` — a Windows binary. On Linux, build
  Fairy-Stockfish yourself and keep `engine/microchess.ini`, which contains a
  correction the shipped variant file lacks (see [§9](#9-traps-that-already-cost-somebody-a-day)).
* `microchess.py`, `analysis/*.py` — drive that engine, so they inherit the same
  requirement.
* `analysis/*.cpp` — one-off measurement programs, already run, results recorded
  in `docs/`. You should not need to rebuild them.

### Build

There is **no workspace at the repo root**. `cargo build` from the top level
fails with `could not find Cargo.toml`. Always point at the manifest:

```bash
cargo build --release --manifest-path solver/Cargo.toml
```

or `cd solver` first. Release mode is not optional — the profile sets
`lto = true` and `codegen-units = 1`, and a debug build of the enumerator is
unusably slow.

Binaries land in `solver/target/release/` (add `.exe` on Windows):

| binary | purpose |
|---|---|
| `enumerate` | **the main event** — external-memory BFS over the reachable set |
| `perft` | rules acceptance test |
| `codeck` | codec injectivity check |
| `symcount` | measures the actual colour-symmetry saving on a real store |
| `recompress` | one-shot raw-`u64` → varint store converter (legacy; you will not need it) |
| `prodrate` | micro-benchmark: movegen+make vs codec cost, by depth |

If you find `pv.exe`, `solve.exe`, or `xcheck.exe` in `target/release/`, they are
stale artefacts of the deleted bottom-up tablebase. Their sources are not in the
tree. Ignore or delete them.

### Verify the build before trusting it

```bash
cargo test --release --manifest-path solver/Cargo.toml
```

**64 tests must pass**, in six binaries:

```
unittests src/lib.rs          34 passed
tests/bfs_counts.rs            2 passed
tests/codec.rs                 9 passed
tests/parallel_identity.rs     2 passed
tests/perft_ladder.rs          2 passed
tests/rules.rs                15 passed
```

It takes about 70 seconds; `parallel_identity` (~35 s) and `perft_ladder` (~35 s)
are the slow ones and they are the two you least want to skip. A single failure
means **stop** — every downstream number in this repo is conditional on these.

Two extra smoke checks, both fast:

```bash
solver/target/release/perft 9
```

must print `perft 9 = 176466898`.

```bash
solver/target/release/codeck 10
```

prints a `roundtrip`/`injective` pair per ply, cumulative. What matters is that
no line says `FAIL` and that the last pair reads `injective ok 13634481
13634481` — distinct keys *equal* distinct positions — followed by
`maxkey 113777253897428 = 2^46.69`. Takes ~31 s. `codeck 12` is the fuller check quoted in `README.md` and everywhere else
in this repo (118,717,620 positions); it is minutes of runtime and a few GB of
RAM, because it dedupes by **full position bytes** rather than by the codec key —
that is the point, it has to be able to disagree with the codec.

---

## 4. Running the enumeration

### The command

```bash
solver/target/release/enumerate <store_dir> [max_ply]
```

* `<store_dir>` — a directory. Created if absent. Defaults to `./enum_store`,
  which you should not rely on; always pass it explicitly.
* `[max_ply]` — stop after this ply. Defaults to 64, which is effectively "run
  to exhaustion". Pass a small number to take one step at a time.

Running it again on the same directory **resumes**. There is no separate resume
flag and no "start over" flag; to start over, use a different directory.

### Tuning

Two environment variables, both optional.

| var | default | meaning |
|---|---|---|
| `ENUM_THREADS` | `available_parallelism() / 2` | expansion worker threads |
| `ENUM_BUF_MK` | `128` | total RAM key buffer across all threads, in units of 2^20 keys |

**`ENUM_THREADS`.** Expansion is bound by memory traffic through the codec
lookup tables, not by ALU work, so SMT siblings buy nothing. Measured at ply 12:
37.6 s on 10 threads, 37.8 s on 14, 39.0 s on 20. The default lands on that knee
and leaves the machine usable. Raising it above the physical core count is
wasted; it is capped at 256 (the bucket count) regardless.

**`ENUM_BUF_MK`.** This is a **memory knob, not a speed one**. Wall time at ply
12 is flat from 16 through 128 Mkeys. Peak resident memory is bounded at roughly
3× this value: measured **1.11 GB at the default**. Lower it if RAM is tight;
there is no reason to raise it.

### Redirect and keep the log

The run prints one line per ply and nothing else. Keep it — the ladder is your
correctness check and your rate estimate.

```bash
solver/target/release/enumerate /path/to/enum_run 64 >> /path/to/enum_run.log 2>&1
```

Append (`>>`), never truncate. The existing log carries the history of every
previous run interleaved with hand-written `=== ... ===` marker lines, and that
history is how the stale-vs-current figures in this repo were untangled.

### Expected output

```
 ply              new         cumulative        sec
   0                1                  1        0.0
threads 10, buffer 128 Mkeys total (12 Mkeys/thread)
   1                9                 10        0.3   [expand 0.0 (spill-cpu 0.0) consol 0.3]
   2               69                 79        0.7   [expand 0.0 (spill-cpu 0.0) consol 0.3]
   ...
  16       1328299642         2733894779     1009.2   [expand 430.0 (spill-cpu 162.6) consol 97.2]
```

`sec` is **cumulative wall clock since this invocation started**, not per-ply.
Ply 16 alone took 1009.2 − 481.8 = 527.4 s. `expand` and `consol` are that ply's
two phases; `spill-cpu` is CPU-seconds summed across threads, so it legitimately
exceeds wall time.

When the frontier empties you get:

```
frontier exhausted -- REACHABLE SET FULLY ENUMERATED
total reachable positions: <N>
```

That `<N>` is the number the entire project has been waiting for.

### The ladder you must reproduce

Every one of these was measured twice — once in the original run, once in a
from-scratch rebuild after the store was destroyed. If your run disagrees with
any of them, something is broken; stop and find out what.

```
 ply              new         cumulative
   0                1                  1
   1                9                 10
   2               69                 79
   3              369                448
   4             1931               2379
   5             9493              11872
   6            44269              56141
   7           190568             246709
   8           774464            1021173
   9          2877776            3898949
  10          9735532           13634481
  11         28739006           42373487
  12         76344133          118717620
  13        177411843          296129463
  14        378216358          674345821
  15        731249316         1405595137
  16       1328299642         2733894779
```

(`docs/enumeration-run.log` is a *stale* copy of this, stopping at ply 14 and
from the pre-parallel serial build. The table above is authoritative.)

### You are starting from ply 0 — and that is fine

**The ply-16 store is not in the repository.** It is 4.67 GB of binary key
streams living in a machine-local scratch directory on the previous machine. It
does not travel with a `git push` and it is not worth transferring.

Rebuilding it from scratch takes **16.8 minutes** (1009.2 s on 10 threads of an
i5-14600K). The reachable-set ladder is deterministic — it does not depend on
thread count, buffer size, or machine — so your rebuilt store holds exactly the
same keys, and the varint encoding is deterministic, so the files come out
identical. Just run it.

---

## 5. What it costs

### Measured, on an i5-14600K (6 P-cores + 8 E-cores, 20 logical), 32 GB RAM, SSD

| quantity | measured |
|---|---|
| plies 0→16, wall clock, 10 threads | **1009.2 s** |
| ply 16 alone | 527.4 s |
| expansion rate (ply 16) | 430.0 s / 731,249,316 frontier keys = **0.588 µs per frontier key** |
| consolidation rate (ply 16) | 97.2 s / 2,733,894,779 visited keys = **0.036 µs per visited key** |
| peak resident memory | **1.11 GB** at `ENUM_BUF_MK=128` |
| store at rest, ply 16 | **4.67 GB** — `visited` 3.01 GB, `frontier` 1.66 GB |
| `visited` density | **1.1812 B/key** |
| `frontier` density | 1.3412 B/key |

That drive was 98% full and budget DRAM-less — 101 MB/s sequential, which is
slow. It did not bind expansion, which is CPU-bound on the codec; a faster drive
would mostly help consolidation. Do not treat 101 MB/s as representative of
anything.

Expansion is ~80% of a ply and `codec::encode` is ~85% of expansion. **A cheaper
key is by a wide margin the highest-value optimisation available** and nobody has
attempted it. Incremental key update during make, or a per-class constrained rank
like the one in `analysis/topclass.cpp`, are the obvious attacks.

### Projected, to completion

This is an extrapolation and it has moved *upward* twice. Growth ratios by ply:
2.656 → 2.324 → 2.132 → 1.933 → **1.817**. The ratio is falling, but its *rate of
fall* is decelerating (−0.192, −0.199, then only −0.117), which pushes the peak
later and compounds hard.

| model fitted to plies 14–16 | peak ply | total reachable | disk | wall clock |
|---|---:|---:|---:|---:|
| ratio falls −0.158/ply | ~21 | **3.4e10** | ~50 GB | ~8 h |
| ratio falls ×0.923/ply | ~23 | **8e10** | ~105 GB | ~24 h |
| either, with colour symmetry | | half | 25–53 GB | 4–12 h |

Do not plan against a single number. **Plan against 3.4e10–8e10**, and expect the
answer only when the run terminates. Compression will not rescue it: density gain
has nearly stalled (1.283 B/key at ply 14 → 1.198 at 15 → 1.181 at 16), so from
here bytes scale essentially linearly with keys.

### The peak-disk number is a projection, not a measurement

Nobody has ever observed a peak. At rest a store is `visited + frontier`. During
a ply it also holds run files (the expanded children, deduped only within each
thread's buffer) and, briefly, a second copy of one bucket plus the next
frontier. The 50–105 GB above is derived, not seen.

**First thing to do when you resume: poll free disk through ply 17 and record the
peak.** It is the cheapest way to convert the single largest unknown in the
budget into a fact, and it costs one `while` loop.

---

## 6. Stopping, resuming, and the store format

### Stopping

Kill it. Any time. Ctrl-C or by PID. The run is designed to be killed and there
is no clean-shutdown path to prefer.

Write the PID to a file next to the store and kill by that. **Do not use
`pkill -f`** — see [§9](#9-traps-that-already-cost-somebody-a-day).

### Checking on a run without disturbing it

Everything you need is in the store directory and is safe to read at any time.

```bash
cd <store_dir>
echo "ply=$(cat ply.txt) cum=$(cat cum.txt)"
ls run_*.keys 2>/dev/null | wc -l          # >0 means expansion has spilled
cat consol.txt 2>/dev/null                 # three ints: ply, buckets done of 256, new so far
du -sh .
df -h .
```

A store at rest — between plies, nothing running — has `ply.txt` and `cum.txt`,
110-ish `visited_*` and `frontier_*` files, and **no** `run_*`, `consol.txt`, or
`swap.txt`. That is the state you want before doing anything else to it.

`consol.txt` present means a ply is partway through consolidation. It is the only
file that tells you a resume would skip expansion, and it is the file to check
before you touch anything in the directory.

### The store, file by file

All inside `<store_dir>`. `NNN` is a bucket index, `000`–`255`; keys are
partitioned into 256 buckets by their high bits. Only ~110 buckets are ever
non-empty, because the key space is not uniformly occupied — that is expected,
not a bug.

| file | meaning |
|---|---|
| `visited_NNN.keys` | every position seen so far, in this bucket |
| `frontier_NNN.keys` | positions first seen at the most recent ply |
| `ply.txt` | last **fully committed** ply number |
| `cum.txt` | cumulative reachable count at that ply |
| `run_NNN_tTT_SSSS.keys` | **scratch** — spilled children, `TT` = writing thread |
| `visited_NNN.tmp` | **scratch** — a bucket's union mid-write |
| `frontier_NNN.next` | **scratch** — next ply's frontier, pre-swap |
| `consol.txt` | **recovery state** — `<ply> <buckets_done> <new_so_far>` |
| `swap.txt` | **recovery state** — the frontier-swap barrier |
| `consol.total` | **recovery state** — cumulative count pending commit |

Every `.keys` file is a **varint-delta stream**: keys strictly ascending, each
stored as a LEB128 gap from its predecessor (the first gap is measured from zero,
so it is the key itself). No pass ever materialises a bucket — reads, merges, and
writes all stream — so RAM does not grow with bucket size. This format is what
took the store from 8.42 GB to 1.3 GB at ply 14 and removed a hard disk ceiling.

### How a ply commits, and why a kill is always safe

Fixed order, every step idempotent on replay:

1. **Expand** — threads claim whole frontier buckets from one atomic cursor,
   generate children, and spill sorted deduped run files. Nothing existing is
   touched.
2. **Consolidate** — per bucket: k-way merge every run file for it, dedupe,
   stream against `visited_NNN.keys`, write the union to `.tmp` and the
   previously-unseen keys to `.next`, then rename `.tmp` over `visited`. Progress
   is written to `consol.txt` after **every** bucket.
3. **Commit** — write `consol.total`, then `swap.txt`, then move every `.next`
   into place, then `ply.txt`, then `cum.txt`, then delete the recovery files.

`visited` is never truncated in place; it is rewritten to a sibling and renamed
over. On restart the program reads `swap.txt` and `consol.txt` and prints what it
recovered:

```
recovered: ply 17 swap replayed, cumulative <N>
recovered: ply 17 consolidation resumes at bucket 183 (<N> new so far)
```

A resumed consolidation **skips expansion entirely** and picks up at the bucket
it reached. This is why the run files must survive a kill.

### The startup sweep

If — and only if — no consolidation is in flight, the enumerator deletes every
`run_*.keys`, `*.tmp`, and `*.next` at startup, printing:

```
swept N stale scratch files from an interrupted expansion
```

That is correct and necessary: a run file truncated mid-write decodes to garbage.
The gate matters. Doing this by hand, during a live consolidation, is exactly the
mistake described in [§9](#9-traps-that-already-cost-somebody-a-day).

---

## 7. The colour symmetry

`solver/src/symmetry.rs` implements one map, `mirror`: **swap colours, flip the
board vertically, flip the side to move, swap the castling rights.** The board
`kbnr/3p/4/3P/KBNR` is its own vertical mirror with colours exchanged, so this
sends legal positions to legal positions and legal moves to legal moves.

**The saving is exactly 2×.** That number took two attempts to get right, and the
reasoning matters because the obvious argument is wrong:

`mirror` flips the side to move, so a White-to-move position reached at an even
ply has its mirror reachable only at an odd one. The reachable set is closed
under `mirror` **only if `mirror(startpos)` — the opening array with Black to
move — is itself reachable.** In standard chess it is not, by a parity argument:
with no pawn moves and castling rights intact only knights can move, a knight
needs an even number of moves to return home, so both sides' move counts are even
and cannot differ by one.

**That argument fails in microchess.** The second rank is empty but for the
d-pawn, so the bishop is free from move one, and the bishop has an *odd* closed
walk `b1→c2→d3→b1`. Hence `1. Bc2 Na4 2. Bd3 Nc5 3. Bb1` — five plies, Black to
move, every piece home, both castling rights intact. A BFS confirms it: reachable
at ply 5. (`mirror_of_startpos_is_reachable_in_five_plies`.)

So the reachable set *is* closed under `mirror`, no position is its own mirror
(the side-to-move flip forbids it), the set is a disjoint union of mirror pairs,
and canonicalising saves **exactly 2×** — not "about 2×".

### What `symcount` measures, and why it reads low

```bash
solver/target/release/symcount <store_dir> [samples]
```

It samples keys by hash, mirrors them, and tests membership with one streaming
merge per bucket. Two passes, no sort, no temp files, ~8 s over 2.7e9 keys. On
the ply-16 store:

```
whole store   68.33%  -> 1.519x
newest ply    35.58%  -> 1.216x
interior      99.29%  -> 1.986x
```

**All three shortfalls are truncation artefacts, not evidence against 2×.** Half
the store is the newest frontier, and its mirror partners live up to five plies
deeper — past the cut, so unseeable. The interior rate (99.29%) is the one that
extrapolates, and it is heading to 100% exactly as the argument requires.

### What canonicalisation actually costs — measured

The 2× is asymptotic. What a *running* enumeration gets is much less, and the
compute price depends entirely on where you apply the map. Both measured on the
real game tree.

**Compute.** The enumerator already holds the child position `q` when it
encodes, so canonicalising needs a second *encode*, not a decode. Over
362,546,967 children expanded from a 42 M-position frontier:

```
plain   encode(q)                  678.9 ns/child   1.000x
canon   encode + mirror + encode  1166.0 ns/child   1.718x
naive   canon_key(encode(q))      1472.2 ns/child   2.169x

net vs unsymmetrised, after halving the frontier:
  good path    0.859x      <- 14% CHEAPER than not canonicalising
  naive path   1.084x      <-  8% dearer
```

**So compute is not a cost — it is a small win, if done right.** The halved
frontier more than pays for the second encode. But `symmetry::canon_key` takes a
*key*, so it decodes a position the loop was holding two lines earlier; that
26-point gap is the entire difference between the two rows. **`canon_key` is the
wrong API for the hot loop** — it exists for `symcount` and one-off lookups.
Write `min(encode(&q), encode(&mirror(&q)))` instead.

**Storage, during the run.** Nowhere near 2×. A canonical BFS against a plain one
from the same root:

```
 ply      plain new      canon new   canon/plain
   3            369            342        0.9268
   5           9493           8264        0.8705
   7         190568         158501        0.8317
   9        2877776        2294172        0.7972
  11       28739006       21432697        0.7458

cumulative at ply 11:  42,373,487 -> 32,156,772   =  0.7589   (a 1.32x saving)
```

**Per-ply counts do not halve, and neither does the cumulative total until the
run finishes.** A canonical BFS reaches each mirror pair by the shorter of the
two routes, which moves positions to earlier plies rather than deleting them.
The ratio decays toward 0.5 but is still 0.75 at ply 11. Peak disk happens near
the peak ply, not at exhaustion, so **the realised peak-disk saving is well under
2×** — treat the "~25 GB with symmetry" figure in `HANDOVER.md` as optimistic.

Two correctness properties came out of the same run, both clean: every plain
key's representative was present in the canonical set (**0 missing** — canonical
enumeration loses nothing), and every canonical key's pair was present in the
plain set (**0 invented**).

### Structural properties that survive canonicalisation

Checked, because a phase-2 storage design leans on all three:

* **`mirror` transposes the class id.** Class `widx*48 + bidx` maps to
  `bidx*48 + widx`. Verified over 3,898,949 reachable positions, 0 mismatches.
* **Class key-ranges stay contiguous.** `class_base` is monotonic in class id, so
  for `widx < bidx` the whole of a class sits below its transpose and every
  canonical key of the pair lands in the lower-id class — 0 violations over the
  same set. Half the classes simply go empty (755 touched → 450, of which 20 are
  self-transpose). **This is what keeps "the store is already sorted by class"
  true**, which `docs/SOLVE.md` §3 depends on.
* **The class DAG's topological order is unchanged.** `piece_count` and
  `nonpawn_count` both sum over the two sides, so a class and its transpose have
  identical sort keys and cannot reorder.

### The hazards this does *not* remove

Whoever wires canonicalisation into a solver must handle all three:

1. **The value identity is mover-relative.** `value(mirror(p)) == value(p)` holds
   for WIN/LOSS/DRAW *from the side to move*. A table storing White-relative
   values must **negate on lookup**. Getting this backwards inverts the answer
   with no other symptom.
2. **A canonical solve walks the class DAG**, so the map must hold at class
   granularity too — `mirror` sends a material class to its transpose,
   consistently for every position in it. Tested
   (`mirror_maps_a_class_onto_a_single_transposed_class`), but it is a property
   the solver has to actually *use* correctly.
3. **Two canonicalisations do not compose.** If the left–right mirror is ever
   added for castling-free positions, the orbit has four members and the
   representative must be `min` over the **whole orbit**. Chaining
   `canon_lr(canon_colour(k))` is neither idempotent nor well-defined. This is
   the exact bug Tinyhouse shipped, and only its round-trip test caught it — for
   the same reason, a position-level `canonical()` and the key-level
   `canon_key()` must be proven to pick the same representative the moment both
   exist.

### Why the values survive it at all

Space saved on a wrong answer is worse than no saving, so this is argued
separately from the counting.

Under this project's conventions a position's value is a function of the position
alone: the 50-move rule is ignored (removing the halfmove clock from the state)
and repetition is resolved by fixed-point iteration inside a material class
rather than by consulting the path. That value is then determined by exactly two
things — the move graph, and the labelling of terminal nodes. `mirror` preserves
both: the graph by `mirror_commutes_with_move_generation`, the terminals by
`mirror_preserves_check_and_therefore_terminal_values` (move counts and
`in_check` are both preserved, so a mate mirrors to a mate and a stalemate to a
stalemate — the only thing separating a leaf loss from a leaf draw). Value
iteration reads nothing else, so an isomorphism preserving the initial labelling
preserves every iterate and therefore the fixed point.

**The left–right mirror is a different map and is only valid once both castling
rights are gone**, because the rook sits on the d-file. Do not assume 4×.
Quantifying the castling-free fraction of the reachable set is an open,
unstarted, and cheap piece of work.

---

## 8. The disk decision

> **Already decided if you have ~120 GB or more free: take Option B, run
> unsymmetrised, add no flags.** See `docs/SOLVE.md` §1. The rest of this section
> is the reasoning, kept for whoever has less disk than that.

It is a scope call, not a technical one.

**Option A — wire canonicalisation into the enumerator first.**
Store `canon_key(k)` instead of `k`; expand either representative of a pair
(children commute with `mirror`, so the canonical child set is the same either
way). Halves storage *and* work, and halves them again for the retrograde solve
that follows. At the revised projection this is the difference between fitting on
a normal machine and not.

Cost: roughly a day of careful work. It **changes what a ply count means**, so
the validation ladder in [§4](#4-running-the-enumeration) will no longer match
and you must establish new baselines — validate them against `symcount` and
against a small-depth exhaustive check, not against the old numbers.

**Option B — keep running unsymmetrised.**
Needs 50–105 GB of free disk. Take the symmetry later, at the solve stage.
Nothing new to build, nothing to re-validate, and the existing ladder stays
meaningful the whole way.

A was recommended when the machine had ~20 GB. With disk to spare, B wins on
every axis that matters: nothing to build, nothing to re-validate, and the
validation ladder above stays meaningful the whole way — which is the only
defence against a silent wrong answer. The colour symmetry does not go to waste;
it remains available at the solve stage, where `solver/src/symmetry.rs` is
already built and tested for exactly that use.

---

## 9. Traps that already cost somebody a day

These are not hypotheticals. Every one happened.

**Never delete `run_*` / `.tmp` / `.next` from a store by hand.** They look like
scratch. While a consolidation is in flight they *are* the recovery state.
Clearing them 183/256 buckets into ply 17 left `visited` mixed across two plies
with the new frontier unrecoverable — unrepairable, and it cost a full rebuild.
**Check `consol.txt` before touching a store.** The enumerator now sweeps them
itself, correctly gated; let it.

**`pkill -f` silently fails on this setup.** It once left *twelve* driver
processes appending to one results file. Kill by PID, and use a lock file.

**Heredocs into this shell mangle multi-line content.** A `cat > file <<'EOF'` of
a Rust file died on an apostrophe in a comment; earlier, Python patch scripts got
their `\n` unescaped and two 30-minute runs executed unpatched binaries. Write
source files with an editor tool, and **verify the change landed** before
starting anything long.

**Bash `$10` means `${1}0`.** It corrupted a results column once. Use `${10}`, or
parse key/value pairs with awk.

**`bc` is not installed.** Do arithmetic in awk, or in the program.

**A benchmark number is only valid at the working set it was taken at.** The
codec was recorded at 154.7 ns/call and that figure misled the entire cost model
for weeks. It is real — and it holds only below ~3 M resident positions. At real
scale it is **~520 ns**. `prodrate` shows the cliff directly: 156.8 ns at depth
5, 154.7 at depth 6, 158.1 at depth 7, **521.4 at depth 8**. `movegen + make` is
unchanged at ~81 ns throughout, so the whole regression is codec table evictions.
**Record the scale beside every number you measure.**

**Do not test position legality by asking whether the side to move can capture
the enemy king.** Fairy-Stockfish never generates king captures, so adjacent-king
positions pass and a *mated* side passes vacuously. An oracle built this way
admitted 408 illegal positions and produced 84 phantom losses. Correct test: flip
the side to move and read `Checkers:`.

**A hash is not an identity.** Anything holding settled values needs the exact
codec key. A 64-bit Zobrist gives ~2.71 expected collisions at 1e10 positions,
and a collision in a solver is a *silently wrong answer*, not a slow one. Hash to
choose a bucket; store and compare the full key.

**The shipped Fairy-Stockfish variant file is wrong.** It omits
`promotionRank = 5`, which lets a pawn on the last rank push again and vanish.
Use `engine/microchess.ini`. If your `perft(5)` is 32923 instead of 32944, you
have reproduced that bug.

**Never kill a process you did not start, and never delete logs to get clean
output.** Copy the log aside or record a marker line and read from there. Wiping
a log for a tidy verification run once destroyed the only evidence of whether an
earlier mistake had interrupted live work — turning a recoverable error into an
unanswerable question.

---

## 10. Validation baselines — never let these drift

Any change to rules, codec, or enumeration must reproduce **all** of these.

```
perft 9                       = 176,466,898
perft 5                       = 32,944          (not 32,923 -- see the .ini trap)
codec injective over            118,717,620 positions, max key 2^46.69
visited encoding                1.1812 B/key on the ply-16 set (2,733,894,779 keys)
largest class, exact            732,059,560 reachable positions
reachable ladder                the table in section 4, plies 0-16
cargo test --release            64 passed
```

Independent ground truth for solved values is in `docs/GROUND-TRUTH.md`: five
material classes, cross-checked three ways (a Python retrograde solver driven by
Fairy-Stockfish, a C++ one, and the Rust one, all agreeing).

The tests that matter most: `perft_ladder`, `codec` injectivity, `keystream`
(LEB128 byte-count boundaries, streams spanning many I/O buffers, k-way dedup),
and `parallel_identity`. That last one is worth understanding — it runs a
1-thread and an 8-thread store to ply 10 and requires them **byte-identical file
for file**, *plus* thread counts 1/2/3/5/16 all agreeing, *plus* both matching
the ply ladder. Identity alone would be satisfied by two runs wrong in the same
way; the ladder check is what closes that. It squeezes the buffer to
`ENUM_BUF_MK=1` so threads spill repeatedly into the same buckets, which is where
a naming collision or a dropped run file would surface.

---

## 11. Git hygiene before you push

**Push the tags.** `bottom-up-tablebase` is a **tag**, not a branch, and
`git push` does not carry tags:

```bash
git push --tags
```

`README.md`, `docs/HANDOVER.md`, and `docs/CHECKPOINTING.md` all tell the reader
to run `git checkout bottom-up-tablebase -- <path>` to recover the deleted dense
tablebase and its reference checkpointing implementation
(`analysis/ckpt.cpp`). Without the tag that command fails. The commit content is
reachable from `master` regardless, so the fallback is
`git checkout 1de0598 -- <path>`.

**The `agent/*` branches are fully merged** — `agent/codec`,
`agent/rules-engine`, `agent/solver` are all 0 commits ahead of `master`. Nothing
is lost by not pushing them.

**Binaries in the tree.** `.gitignore` excludes `*.exe`, but
`engine/fairy-stockfish.exe` and `analysis/probe.exe` were force-added and *are*
tracked. That is deliberate — the engine is the rules oracle. `analysis/dfpn.exe`
is untracked and stays that way.

**No remote is configured yet.** `git remote -v` is empty.

---

## 12. Known-stale figures in the other docs

`docs/HANDOVER.md` and `docs/FINDINGS.md` are worth reading, but they were
written at different times and four figures in them are now wrong. This file
carries the corrected values.

| document | says | actually |
|---|---|---|
| `HANDOVER.md` | store is "~7 GB at ply 16" | **4.67 GB** measured (`visited` 3.01 + `frontier` 1.66) |
| `HANDOVER.md` | "53 tests" | **64** |
| `FINDINGS.md` | total reachable ≈ 9.4e9; retrograde solve 21–44 GB | superseded: **3.4e10–8e10**, so the solve estimate scales 4–10× |
| `docs/enumeration-run.log` | ladder stops at ply 14, serial build | plies 15–16 exist; see [§4](#4-running-the-enumeration) |

`FINDINGS.md` §5 ("Corrections I made to my own claims") is the most useful page
in the repository and is not stale. Read it. Every entry is a case of an earlier
claim being wrong *in the direction that flattered the plan*, which is the
failure mode this project keeps hitting.

---

## 13. After the enumeration: the retrograde solve

**See `docs/SOLVE.md` — it is the work order for this half.** Short version: a
verified retrograde solver already exists on the `bottom-up-tablebase` tag
(`solver/src/retro.rs`, `bin/solve.rs`, `bin/pv.rs`, `bin/xcheck.rs`,
`tests/solve_ground.rs`). Restored onto current `main` it builds with zero errors
and its 17 ground-truth tests pass. It failed for **one** reason — its value
array is dense over the class's whole index space, 5.4 TB for the start class —
and the enumeration is what fixes that: only 1 slot in 7,327 is reachable.

The design constraints below are what any replacement storage layer must still
respect.

**Sequential I/O, never random.** A disk-resident hash is fatal: ~3e10 positions
× ~8.9 children is ~3e11 probes; at SSD random-read latency that is months. The
design must be sort/merge based so all I/O streams. This is why the enumerator
looks the way it does, and the solver must inherit the same shape.

**Monotonicity is the gift.** A slot goes `UNKNOWN → WIN/LOSS` and never changes
again. Three consequences do all the work: a half-flushed page cannot corrupt
anything (every byte is old-or-new, both legal); re-running any pass from a
partially converged state reaches the same fixed point; so checkpointing needs
almost no machinery. This was verified end to end on the deleted implementation —
lossless across three kill points, with peak *private* RAM dropping 150 MB → 2 MB
via mmap at a 6.8% wall-time cost. `docs/CHECKPOINTING.md`.

**Queue/counter, not sweeps.** A queue-driven retrograde is O(edges) once. Naive
repeated sweeps multiply that by the sweep count. This is the single design
decision that matters most for wall time.

**Repetition is handled by the fixed point, never by the path.** Inside a class:
`node is WIN if SOME child is LOSS; node is LOSS if EVERY child is WIN; repeat
until nothing changes; everything still unresolved is a DRAW.` This consults no
history, so Graph History Interaction cannot arise. **Do not** add path-based
repetition detection on top of a transposition table — that combination is a
known, published class of silently-wrong-answer bug, and it is exactly what
`docs/REPETITION.md` exists to prevent. The reason the fixed point does not leak
the way the sibling project's did is that each material class is *closed*: every
move leaving it lands in a class already fully solved, so there is no frontier.

**The 50-move rule is deliberately ignored**, which is the standard convention for
solving a game and is what removes the halfmove clock from the state. Say so in
whatever writes up the result — "solved" means different things with and without
it.

---

## 14. Suggested first session

1. `cargo test --release --manifest-path solver/Cargo.toml` → 64 passed.
2. `perft 9` → 176466898. `codeck 10` → `injective ok 13634481 13634481`.
3. Pick a store directory on a disk with room to grow, and start the run.
   Redirect and append to a log.
4. Watch it reproduce the ladder through ply 16 (~17 minutes). If any line
   disagrees, stop and investigate — do not continue past a mismatch.
5. **Poll free disk through ply 17 and record the peak.** This is the cheapest
   available conversion of a guess into a fact.
6. Make the [§8](#8-the-open-decision) call with that number in hand.
