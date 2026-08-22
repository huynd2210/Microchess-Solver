# TASK 03 — the material-class solver (retrograde fixed point), bottom-up

`solver/` has a validated rules engine (task 01) and an exact injective codec +
TT (task 02, verified injective over 118,717,620 positions). This task makes it
**solve** — exactly, for one material class at a time.

Read `docs/REPETITION.md` first. It is the whole design and it explains why the
obvious approach is wrong.

## The idea in one paragraph

Captures and promotions move strictly **down** an acyclic DAG of material classes,
so every cycle lies inside a single class. Solve classes bottom-up. Inside a class,
any move that captures or promotes lands in an **already-solved** class and is an
exact leaf value. What is left is a closed subgraph of quiet moves; settle it by
fixed point:

```
initialise: checkmate => LOSS for the side to move
            stalemate => DRAW
            any move into a solved lower class => that class's value
repeat until nothing changes:
    node is WIN  if SOME child is LOSS
    node is LOSS if EVERY child is WIN
when the iteration converges, everything still unresolved is a DRAW
```

**Never detect repetition along a search path, and never store a path-dependent
value in the TT.** That is the Graph History Interaction bug documented in
`docs/REPETITION.md`; it produces silently wrong answers. Draws come only from
convergence of the fixed point above, which consults no history.

## Deliverable

```
solver/src/matclass.rs   material-class ids, the class DAG, class enumeration
solver/src/retro.rs      the retrograde fixed point over one class
solver/src/bin/solve.rs  the CLI below
```

A class is named by its two multisets, e.g. `KvK`, `KNvK`, `KRvK`, `KQvK`,
`KBNvK`, `KRvKR`. Accept that spelling on the command line.

```
cargo run --release --bin solve -- <CLASS>            solve it (and its dependencies)
cargo run --release --bin solve -- <CLASS> --dump N   print N random "FEN = VALUE" lines
```

Required stdout line, exact prefix so I can parse it:

```
class <NAME> positions <n> win <w> loss <l> draw <d> illegal <i> iters <k> time <secs>
```

`positions` counts the legal positions in the class (both sides to move);
`illegal` counts index slots rejected (kings adjacent, side-not-to-move in check,
etc.). WIN/LOSS are **from the side to move's** point of view — state this in the
code and keep it consistent, mixing the conventions is the classic bug here.

## Acceptance — I run all of it, and I have an independent oracle

These classes have known ground truth. **Every one must come out right:**

| class | expected |
|---|---|
| `KvK` | every position DRAW; win 0, loss 0 |
| `KNvK` | every position DRAW (a lone knight cannot mate) |
| `KBvK` | every position DRAW |
| `KRvK` | a mixture — White wins from most positions; **draw count > 0** (stalemates and rook-hanging cases) and **loss count > 0** (White to move, already stalemated or rook lost) |
| `KQvK` | a mixture, with far more White wins than `KRvK` |
| `KNNvK` | every position DRAW or nearly so — two knights cannot force mate in normal chess; **report what you actually get, do not force it** |

I will additionally cross-check `--dump` output against **Fairy-Stockfish**, which
is a genuinely independent engine, on a large random sample: for every position you
label WIN there must be a forced mate for the side to move, for every LOSS a forced
mate against, and for every DRAW no mate either way. Assume I will do this on
thousands of positions and that one disagreement fails the task.

Known individual positions, all verified against Fairy-Stockfish already:

* `k3/4/4/4/K3 w - - 0 1` (bare kings) — DRAW
* `k3/4/4/4/K1N1 w - - 0 1` — DRAW
* `k3/4/4/4/K2R w - - 0 1` — WIN for White (mate in 5)
* `k3/4/4/4/K2Q w - - 0 1` — WIN for White (mate in 4)
* `k3/3N/1K2/4/4 b - - 0 1` — DRAW (Black is stalemated)
* `1kR1/4/3N/2BP/K3 b - - 9 5` — LOSS for Black (checkmate)

Add these as tests.

## What I actually need out of this task

Feasibility, measured. Alongside correctness, report **microseconds per position**
for each class solved, and the peak memory. I am going to extrapolate these to the
whole game against a **one-week compute budget**, so the timing must be honest:
say whether it is single-threaded, and do not quote a best-of run.

If a class is too large to hold, say so and report the largest you could do rather
than silently sampling.

## Scope fence

* Do not modify `docs/`, `engine/`, `reference/`, `analysis/`, `microchess.py`,
  `README.md`, or the rules and codec (`movegen.rs`, `codec.rs`) except to add
  `pub` where you need access. If you think the codec is wrong, say so in FINDINGS
  and stop — do not fix it silently.
* `cargo test --release` must stay green, **including the perft ladder**
  (depth 9 = 176,466,898) and the codec injectivity tests.
* No forward search, no AO*/LAO*, no heuristics in this task. Retrograde only.
  The forward searcher is task 04 and it needs this as its ground truth.

## Report

`FINDINGS-03.md`: the fixed-point implementation, the exact `solve` output for
every class in the table, µs/position and memory per class, and — most important —
**your own honest estimate of how far this scales**, including which class first
becomes too big and why. If the numbers say the full game is out of reach on this
hardware, say that plainly; a well-argued negative is worth more than an optimistic
one I have to disprove.

`git commit` will fail (worktree `.git` is outside your sandbox). Expected. Leave
the tree clean and say so; I will commit.
