# SOLVE — the work order for actually solving it

`docs/RUNBOOK.md` tells you how to operate the machinery. **This file tells you
what to do.** It is a work order, in order, with an acceptance test at every
step. It is not research and it does not ask you to make design decisions —
where a decision was open, it has been closed here and the reasoning is stated
so you can tell if your situation differs.

Read this top to bottom once before starting anything.

---

## The shape of the job

Solving microchess is two phases:

| phase | what | who | how long |
|---|---|---|---|
| **1. Enumerate** | list every reachable position, on disk | **runner** — one command | 8–24 h |
| **2. Retrograde solve** | assign WIN/LOSS/DRAW backwards from the terminals | **builder** — a bounded code change, then a run | days |

Phase 1 needs no code written. Phase 2 does — but far less than you would
expect, because **the retrograde solver already exists and is verified**. See
[§4](#4-phase-2--what-already-exists).

**Do phase 1 first regardless.** It is valuable on its own: it settles the last
unmeasured quantity in the entire project, and its output is the input phase 2
needs. Nothing in phase 2 can be sized until phase 1 finishes.

---

## 1. Decisions already made — do not reopen these

**With ~150 GB of free disk, run WITHOUT colour symmetry.**

`docs/RUNBOOK.md` §8 presents this as an open fork. It is closed by your disk
budget. The unsymmetrised run needs 50–105 GB across the projected range; 150 GB
covers the pessimistic end with headroom. Implementing canonicalisation would
save half the disk you already have, at the cost of about a day of careful work
and — worse — invalidating every validation baseline in the repo, which is your
only defence against a silent wrong answer.

So: **do not implement canonicalisation.** Do not pass any symmetry flag. Run it
plain. The colour symmetry stays available as a phase-2 lever if a class turns
out not to fit in RAM, and `solver/src/symmetry.rs` is already built and tested
for exactly that.

**The 50-move rule is ignored.** This is deliberate and standard for solving a
game; it removes the halfmove clock from the state. Do not add it. Say so when
you report the result — "solved" means a different thing with and without it.

**Values are from the side to move's point of view**, everywhere, always. Not
White's. Mixing these up is the classic bug of this task and it inverts the
answer with no other symptom.

---

## 2. Phase 1 — enumerate

### Do this

```bash
cargo build --release --manifest-path solver/Cargo.toml
cargo test  --release --manifest-path solver/Cargo.toml     # 64 must pass
```

Pick a directory on the 150 GB volume. Then:

```bash
solver/target/release/enumerate /path/to/enum_run 64 >> /path/to/enum_run.log 2>&1
```

That is the whole of phase 1. It resumes if killed — just run the same command
again. Details, tuning knobs, and the store format are in `docs/RUNBOOK.md` §4
and §6.

### Acceptance test

The run must reproduce this ladder exactly. Check it as the lines appear; the
first sixteen plies take about 17 minutes.

```
 ply              new         cumulative
  12         76344133          118717620
  13        177411843          296129463
  14        378216358          674345821
  15        731249316         1405595137
  16       1328299642         2733894779
```

**If any line disagrees, stop.** Do not continue past a mismatch and do not
"fix" it by rerunning — something is wrong with the build or the machine, and
every number downstream inherits it. The full ladder from ply 0 is in
`docs/RUNBOOK.md` §4.

Past ply 16 you are in new territory and there is nothing to compare against.
That is expected. The run ends with:

```
frontier exhausted -- REACHABLE SET FULLY ENUMERATED
total reachable positions: <N>
```

### Two things to record while it runs

These cost nothing and both are needed later.

1. **Peak free-disk drawdown.** Nobody has ever measured it. Poll `df` every
   minute into a file for the whole run. This is the single largest unknown in
   the budget and one loop settles it.
2. **The per-ply log.** Keep it. Append, never truncate.

### What to report back

The final `total reachable positions`, the per-ply table, the peak disk figure,
and the wall clock. That total is the number the whole project has been waiting
for — everything else in this repo has been an estimate of it.

---

## 3. Between the phases — the one measurement that sizes phase 2

Before writing any solver code, run a **per-class histogram** of the enumerated
set: how many reachable positions does each of the 1,272 material classes hold?

This is a single streaming pass over the store with no sorting, because of a
structural fact worth internalising:

> A material class occupies a **contiguous range of key space**. The codec key
> is `class_base(c) * 8 + placement_rank * 8 + castle * 2 + black_to_move`, so
> every key of class `c` lies in `[class_base(c)*8, (class_base(c)+placements(c))*8)`.
> The store is sorted by key. **Therefore the store is already sorted by class**,
> and splitting it into per-class key files is one pass with zero sorting.

Use `codec::class_of_key(key)` to attribute each key. Write the histogram, and
while you are there write the per-class key files — phase 2 wants both.

### Why this measurement decides the design

Phase 2 holds **one class's value array in RAM at a time**, one byte per
reachable position. So the histogram's maximum tells you directly whether the
simple design works:

| largest class | design |
|---|---|
| under ~4e9 positions (~4 GB) | **in-RAM value array per class.** Simple, and the existing solver is already shaped this way |
| above that | stream that class instead: sort-merge join edges against values rather than random-access lookups |

The one class measured exactly so far is the start material with no captures
(`KBNRPvKBNRP`) at **732,059,560** reachable positions — 732 MB of values, which
fits comfortably. But note carefully: that class is the largest by *index space*
(5.36e12 naive slots), **not necessarily by reachable count**. It is unusually
constrained — no capture means both pawns are frozen on the d-file and both
bishops are stuck on their starting colour. Classes one capture down have free
pawns and may well hold *more* reachable positions. Nobody knows yet. The
histogram is how you find out, and it is cheap.

---

## 4. Phase 2 — what already exists

**Do not write a retrograde solver from scratch.** One exists, it is verified
against an independent oracle, and it still compiles against current `main`.
This was checked, not assumed:

```bash
git checkout bottom-up-tablebase -- \
  solver/src/retro.rs solver/src/bin/solve.rs solver/src/bin/pv.rs \
  solver/src/bin/xcheck.rs solver/tests/solve_ground.rs
# then add `pub mod retro;` to solver/src/lib.rs
```

Result of doing exactly that on current `main`: **builds with zero errors**, and
`cargo test --release --test solve_ground` gives **17 passed, 0 failed** in
0.73 s. It also reproduces `docs/GROUND-TRUTH.md` — the independently derived
table produced by a from-scratch Python solver driven by Fairy-Stockfish, sharing
no code with this one:

```
class KvK   positions   2160  win     0  loss     0  draw  2160  illegal   880  iters  1
class KNvK  positions  35616  win     0  loss     0  draw 35616  illegal 19104  iters  1
class KBvK  positions  34848  win     0  loss     0  draw 34848  illegal 19872  iters  1
class KRvK  positions  31800  win 12360  loss 14688  draw  4752  illegal 22920  iters 10
class KQvK  positions  27768  win  8328  loss 13888  draw  5552  illegal 26952  iters  7
```

**These are exactly 4× the numbers in `docs/GROUND-TRUTH.md`, and that is
correct.** The oracle counted `placements × 2` (side to move only); `solve`
counts all four castling-right combinations too, `placements × 8`. Divide by 4 to
compare. Every win/loss/draw count matches to the unit. Iteration counts may
differ by one between implementations (whether the confirming sweep is counted);
values must not.

**Know this before you touch it, or you will "fix" a working solver.**

### What you get back

| file | what |
|---|---|
| `solver/src/retro.rs` | the fixed point, terminal classification, legality, per-class solve |
| `solver/src/bin/solve.rs` | driver: `solve <CLASS>` solves a class and its full downward closure |
| `solver/src/bin/pv.rs` | principal variation extraction — how you get the *strategy*, not just the value |
| `solver/src/bin/xcheck.rs` | cross-check against Fairy-Stockfish |
| `solver/tests/solve_ground.rs` | the 17 ground-truth tests |
| `analysis/ckpt.cpp` | the reference checkpointing design, verified lossless across three kill points |

`solver/src/matclass.rs` (the class DAG, `successors()`, the topological order)
is **already on `main`** and was never removed.

---

## 5. Phase 2 — the one thing that must change

The removed solver failed for exactly one reason, and it is a storage reason,
not an algorithmic one.

`Solved.vals` is a **dense** array covering every index slot of the class:

```rust
pub struct Solved {
    pub vals: Vec<u8>,   // indexed by  rank*8 + castle*2 + black_to_move
    ...
}
```

For the start class that is 5,363,540,582,400 slots — 5.4 TB at one byte each,
and `docs/FINDINGS.md` records the implementation's real requirement as 12 TB.
Hopeless. But only **732,059,560** of those slots are reachable: a 7,327×
reduction, measured exactly.

**So: replace the dense array indexed by placement rank with a sparse array
indexed by position within that class's enumerated reachable key list.** That is
the whole change. Phase 1 produces exactly the list you need.

### The concrete change

1. **Per-class key list.** From §3 you already have, per class, its reachable
   keys in ascending order. `vals[i]` is now the value of the `i`-th key.
2. **`value_by_key`.** Currently arithmetic (`key - base8`). Becomes a lookup of
   `key` in the sorted key list — binary search is the correct, simple baseline.
3. **Pass 0 gets simpler, not harder.** The dense version had to test every slot
   for legality (adjacent kings, wrong side in check, pawn on its promotion
   rank) and reject most of them. **Reachable positions are legal by
   construction**, so that entire rejection path becomes dead. Keep
   `classify_terminal` — you still need checkmate vs stalemate.
4. **Everything else is untouched.** The bottom-up class order, the fixed point,
   the terminal labelling, the dependency lookups, the value convention.

### The invariant that makes this safe, and that you should assert

**Every child of a reachable position is reachable.** That is what the BFS
closure means. So every lookup — same-class or into a dependency — must hit.
Assert it rather than returning a default: a miss means the enumeration and the
solve disagree about the position space, and a silent default there is a wrong
answer with no symptom.

### If binary search proves too slow

It might. At ~3e10 positions × ~8.9 children that is ~3e11 lookups, each ~30
cache-hostile comparisons. Measure it on a mid-size class before assuming either
way. If it does not hold up, the fix is known and is the standard external-memory
move: **do not do random lookups at all.** Generate all `(parent, child)` edges,
sort by child, and sort-merge join against the child value array — O(edges) of
pure sequential I/O, once per sweep. `docs/FINDINGS.md` §4 flags this as the
decision that matters most for wall time: a queue/counter retrograde is O(edges)
once, whereas naive repeated sweeps multiply that by the sweep count.

---

## 6. Phase 2 — acceptance, in this order

Do not skip a step. Each one catches a different class of error.

1. **`cargo test --release`** — 64 existing tests, plus the 17 restored
   ground-truth tests, all passing.
2. **Reproduce the five-class table** in §4 exactly. Values to the unit;
   iteration counts may differ by one.
3. **`pawn_class_solves_through_promotion_dependencies`** must pass — this is
   the one that exercises promotion edges into a different class, which is the
   part of the DAG most likely to break under a storage change.
4. **Cross-check with `xcheck`** against Fairy-Stockfish on random positions
   from a mid-size class.
5. **Then, and only then, the start class.**

The answer you are after is the value of the start position:

```
kbnr/3p/4/3P/KBNR w Dd - 0 1
```

It is very likely a **draw** — Fairy-Stockfish returns `cp 0` at depth 69 and
the position is materially symmetric — but that is a prior, not a result. If your
solver says otherwise, that is interesting and it needs checking, not
suppressing.

Then extract the strategy with `pv`. A value without a witness is half an answer.

---

## 7. Things that will waste your time if you don't know them

**Don't add path-based repetition detection.** Repetition is handled by the fixed
point converging — "everything still unresolved at convergence is a DRAW". It
consults no history, so Graph History Interaction cannot arise. Adding
path-based loop detection on top of a transposition table is a known, published
class of silently-wrong-answer bug, and `docs/REPETITION.md` exists specifically
to stop you doing it.

**Don't test legality by asking whether the side to move can capture the enemy
king.** Fairy-Stockfish never generates king captures, so adjacent-king positions
pass and a *mated* side passes vacuously. This admitted 408 illegal positions and
produced 84 phantom losses in the oracle. Correct test: flip the side to move and
read `Checkers:`.

**Don't use a hash as an identity.** A 64-bit Zobrist gives ~2.71 expected
collisions at 1e10 positions and a collision in a solver is a silently wrong
answer. Use the exact codec key. Hash only to pick a bucket, then compare the
full key.

**Don't delete `run_*` / `.tmp` / `.next` from an enumeration store by hand.**
While a consolidation is in flight they *are* the recovery state. Doing this once
cost a full rebuild. Check `consol.txt` first. The full trap list is
`docs/RUNBOOK.md` §9 and every entry there is something that already happened.

**Don't trust a benchmark without its scale.** The codec was recorded at
154.7 ns/call and it misled the cost model for weeks; at real working-set size it
is ~520 ns. Record the scale beside every number you measure.

---

## 8. If you only read one section

```bash
# 1. build and verify
cargo build --release --manifest-path solver/Cargo.toml
cargo test  --release --manifest-path solver/Cargo.toml        # 64 pass

# 2. enumerate -- this is phase 1, it is the whole job for now
solver/target/release/enumerate /path/to/enum_run 64 >> /path/to/enum_run.log 2>&1

# 3. check the ladder matches docs/RUNBOOK.md section 4 through ply 16, then let it run
# 4. report: total reachable, per-ply table, peak disk, wall clock
```

Do not start phase 2 until phase 1 has finished and you have the per-class
histogram from §3. Phase 2 cannot be sized without it, and guessing at the size
is how this project has gone wrong four times already.
