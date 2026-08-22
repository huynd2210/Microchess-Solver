# The ladder run

```
bash run/drive.sh                 # start or resume; a lock file prevents a second instance
CAP=400000000 TMO=1800 bash run/drive.sh
```

Solves every material class bottom-up. **Checkpoint granularity is one class**: each
completed class is appended to `results.tsv` the moment it is known, so a kill loses
at most the class in flight, and restarting skips everything already recorded.

`solve.exe` reports every class in the dependency closure it solves, not just the
target, so all of them are recorded — the recomputation becomes progress rather than
waste.

| file | what it is |
|---|---|
| `ladder.txt` | 1,272 reachable classes, ordered by piece count (dependencies first) |
| `results.tsv` | class, pieces, slots, positions, win, loss, draw, illegal, iters, secs |
| `run.log` | per-invocation timing, skips and failures |
| `driver.lock` | single-instance guard |

`results.tsv` is the partial-results channel and is readable while the run continues.

## Two bugs worth not repeating

* **`pkill -f drive.sh` does not work here.** Three "restarts" left **twelve**
  drivers running concurrently, all appending to the same file — 95 rows for 61
  distinct classes, and a 4-piece count of 81 against a ladder that only has 49.
  Kill by PID via WMI, and the lock file now makes it impossible.
* **`$10` in bash is `${1}0`**, so the draw column recorded the literal `class0`.
  The driver now parses `key value` pairs with awk instead of positional fields,
  which is also order-independent.

## Measured pace and what it reaches

11.6 s per class single-threaded, and class size grows roughly 15× per piece added:

| tier | classes | at this rate | with the 15× parallel speedup already verified |
|---|---:|---:|---:|
| 2–4p | 60 | **done** | — |
| 5p | 146 | 0.5 h | minutes |
| 6p | 284 | 13.7 h | 0.9 h |
| 7p | 362 | 11 days | 17.5 h |
| 8p | 289 | 131 days | 9 days |
| 9p | 130 | 884 days | 59 days |

So this run reaches a complete tablebase through **6 pieces** overnight, and 7 pieces
is the practical frontier. 8 pieces is where memory kills it before time does
(see `docs/READINESS.md`).

The single-threaded Rust solver is the bottleneck. `analysis/ckpt.cpp` is 15× faster
and checkpointed, but only covers "white pieces vs a bare king"; porting its parallel
sweep and mmap store into `solver/src/retro.rs` is the highest-value next change.
