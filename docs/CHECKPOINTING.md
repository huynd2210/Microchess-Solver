# Checkpointing and disk offload

> The reference implementation this describes (`analysis/ckpt.cpp`) was removed
> with the bottom-up tablebase; recover it with
> `git checkout bottom-up-tablebase -- analysis/ckpt.cpp`. The **design and its
> verification still stand** and carry over directly to the external-memory
> solver — monotonicity is what makes checkpointing nearly free, and that does not
> depend on where the values live.

`analysis/ckpt.cpp` — the parallel retrograde solver with a memory-mapped,
resumable store. Verified working; this is the reference design for the Rust port.

```
ckpt.exe retro <maxpieces> <threads> --store <dir>     # run, or resume
```

## Why checkpointing needs almost no machinery here

The retrograde update is **monotone**: a slot goes `UNKNOWN → WIN/LOSS` and never
changes again. Three consequences do all the work:

1. **A half-flushed page cannot corrupt anything.** Every byte is either its old
   or its new value, and both are legal. There is no torn state to repair.
2. **Re-running any sweep from a partially converged array reaches the same fixed
   point.** So a crash needs no rollback — resume and keep sweeping.
3. Therefore the only thing that must be recorded is whether **pass 1** (legality
   + terminals) finished, because re-running pass 1 would reset solved slots back
   to `UNKNOWN` — still correct, but it would throw the work away.

## On-disk layout

```
<dir>/<class>.val     the value array, one sparse file per class, memory-mapped
<dir>/progress.txt    "done <class>" per finished class, "pass1 <class>" in flight
<dir>/results.tsv     appended as each class completes -- readable mid-run
```

`progress.txt` is written temp-then-`MoveFileEx(..., WRITE_THROUGH)`, so it is
never observed torn. Class files are sparse, so a 128 MB logical array costs only
the pages actually touched.

`results.tsv` is the partial-results channel: `class, positions, win, loss, draw,
iters, seconds`, one line per completed class, appended immediately. It can be read
while the solve is still running.

## Verified: kill and resume is lossless

Three kill points, each resumed and compared against an uninterrupted reference run
over 16 classes (checksums are FNV over the whole value array):

| kill point | classes done at kill | result |
|---|---|---|
| between classes | 13 of 16 | **identical on all 16** |
| during pass 1 | 15 of 16 | **identical on all 16** |
| during the sweeps, partial updates flushed | 15 of 16 | **identical on all 16** |

Resuming keeps the partial work: the interrupted 24,374,736-position class
finished in 17.4 s on resume versus 30.2 s from scratch.

## Verified: the RAM actually moves to disk

Largest class, 12 threads, measured by polling the process:

| backing | peak working set | **peak private (true RAM cost)** |
|---|---:|---:|
| heap | 154 MB | **150 MB** |
| mmap store | 148 MB | **2 MB** |

Private memory is what the process owns and the OS cannot reclaim. With the store,
essentially all of it becomes file-backed and evictable under pressure — the
difference between a solve that survives memory pressure and one that is killed.
**75× less unreclaimable RAM.**

Cost: **6.8%** wall time (27.49 s → 29.36 s), identical checksum.

## Why this matters on this machine

32 GB installed, but only **2.8 GB available** — 25.3 GB is held by other running
applications and commit is 46.2 GB of a 55 GB limit. A solver that allocates its
working set privately would be killed or would thrash. Disk backing is not an
optimisation here, it is what makes the run possible. Free disk is 29.2 GB.
