# FINDINGS — Task 03: the material-class solver (retrograde fixed point)

**Verdict: done, exact, and independently cross-checked.** All six ground-truth
classes come out right, every solved class passes a built-in post-solve audit,
and **1,960 randomly sampled positions across nine classes agree with
Fairy-Stockfish** (zero unexplained disagreements; one explained convention
divergence, §5). Full `cargo test --release` is green including the depth-9
perft ladder (176,466,898) and codec injectivity tests. The tree is left clean
and uncommitted for you (`git commit` fails from this sandbox as expected).

Everything below is **single-threaded**, fresh-run wall clock, no best-of.

## 1. What was built

* `solver/src/matclass.rs` — class ids (= the codec's `widx*48+bidx`),
  name parsing/printing (`KBNvK`, case-insensitive, any letter order),
  the DAG successor edges (captures + promotions), and the topological-order
  invariant.
* `solver/src/retro.rs` — the per-class retrograde fixed point of
  `docs/REPETITION.md`, exactly as specified there. No forward search, no
  heuristics, no repetition detection anywhere.
* `solver/src/bin/solve.rs` — the CLI, emitting exactly
  `class <NAME> positions <n> win <w> loss <l> draw <d> illegal <i> iters <k> time <secs>`.
* Extras: `solver/src/bin/xcheck.rs` (Fairy-Stockfish cross-check harness used
  for the numbers in §4), `solver/src/bin/pv.rs` (walks the strategy a solved
  value array implies from a FEN — useful to eyeball forced lines),
  `solver/tests/solve_ground.rs` (ground-truth + property tests).
* `codec.rs`/`lib.rs`: only additive changes — `pub` on `canon_side`,
  `side_expansion`, plus four accessor functions (`class_base`,
  `class_placements`, `class_side_material`, `class_of_key`). **No codec bug
  found**; the combinatorial rank held up as the sole identity mechanism, as
  task 02 promised.

### Value convention

Every value is **from the side to move's point of view** (stated at the top of
`retro.rs` and enforced by the terminal assignment: checkmate ⇒ LOSS for the
mover, stalemate ⇒ DRAW). WIN/LOSS/DRAW counts in the summary line are over
all legal positions, both sides to move.

### The algorithm, concretely

A class's states are `(placement, castling rights, side to move)`, indexed
exactly by the codec key minus the class base — so a child lookup after a move
is one `encode` plus one array read, and the TT-of-record *is* the value array
(perfect rank indexing; the hash TT of task 02 is unused here because nothing
evicts).

1. **Pass 0**: enumerate every placement × castling × stm slot; reject illegal
   slots (kings adjacent or stacked, side-not-to-move in check, pawn sitting on
   its promotion rank — unreachable since promotion is mandatory, and unsafe
   for movegen); assign terminals (mate ⇒ LOSS, stalemate ⇒ DRAW).
2. **Graph**: captures/promotions land in already-solved lower classes and are
   pre-collapsed to constant leaf values; quiet moves become same-class edges
   (CSR adjacency). Classes above `2^24` slots skip the cache and stream moves
   from packed boards each sweep instead (measured: identical values, ~4.9×
   slower, ~14× less memory).
3. **Fixed point**, until nothing changes:
   `node is WIN if SOME child is LOSS; node is LOSS if EVERY child is WIN`;
   leftover UNKNOWN after convergence becomes DRAW. Sweeps alternate direction
   to halve propagation depth. No history is consulted anywhere, so GHI cannot
   arise by construction.
4. **Audit** (cached mode, always on): re-walks the finished graph and asserts
   WIN ⇔ ∃LOSS-child, LOSS ⇒ ∀children WIN, DRAW ⇒ no LOSS-child and not
   all-WIN. This checks the fixed point against the graph itself, independent
   of how the values were derived.

Dependencies are solved first in topological order. One subtlety worth
recording: **no scalar potential orders this DAG**. A capture reduces piece
count, but a promotion raises material quality at equal count, so my first
attempt ("potential strictly decreases along every edge") was wrong — it would
have solved promotion targets too late, and none of the six required classes
has pawns, so nothing caught it until I wrote the edge-invariant test. The
correct order key is `(piece_count asc, nonpawn_count desc)`; the test now
checks the invariant over all 2,304 classes and a pawn class (`KPvK`) is in
the test suite to keep the promotion path exercised.

## 2. Required output — all six classes, fresh runs

```
class KvK positions 2160 win 0 loss 0 draw 2160 illegal 880 iters 1 time 0.004
class KNvK positions 35616 win 0 loss 0 draw 35616 illegal 19104 iters 1 time 0.050
class KBvK positions 34848 win 0 loss 0 draw 34848 illegal 19872 iters 1 time 0.051
class KRvK positions 31800 win 12360 loss 14688 draw 4752 illegal 22920 iters 10 time 0.052
class KQvK positions 27768 win 8328 loss 13888 draw 5552 illegal 26952 iters 7 time 0.048
class KNNvK positions 278888 win 784 loss 256 draw 277848 illegal 186232 iters 2 time 0.441
```

Against the acceptance table:

* **KvK / KNvK / KBvK**: every position DRAW, win 0, loss 0. ✓
* **KRvK**: mixture; draws 4,752 > 0 (stalemates and rook-hanging draws) and
  losses 14,688 > 0. ✓ (The losses are Black-to-move mating nets — a lone
  black king can never mate, so no White-to-move position loses.)
* **KQvK**: mixture ✓ — but see §5.1: the raw WIN count comes out *lower*
  than KRvK, contrary to the task table's parenthetical, and the data says why.
* **KNNvK**: 99.63% draws (win 784, loss 256 of 278,888) — two knights cannot
  force mate, but isolated existing-mate and stalemate-trap positions exist.
  Reported as measured, nothing forced. ✓

The structure behind these counts (from the per-side-to-move breakdown the
binary prints to stderr): **every legal White-to-move position in KRvK and
KQvK is a WIN**; all LOSSes and DRAWes sit on Black-to-move slots.

Per-class timing/memory (peak working set of the whole process, dependencies
included):

| class | positions | µs/position | peak RSS |
|---|---|---|---|
| KvK | 2,160 | 1.9 | 4.4 MiB |
| KNvK | 35,616 | 1.4 | 7.4 MiB |
| KBvK | 34,848 | 1.5 | 7.4 MiB |
| KRvK | 31,800 | 1.6 | 7.4 MiB |
| KQvK | 27,768 | 1.7 | 7.4 MiB |
| KNNvK | 278,888 | 1.6 | 22.6 MiB |

(The tiny classes are dominated by the enumeration pass — legal-move
generation per slot — which is why they all sit near ~1.5 µs regardless of
fixed-point work.)

## 3. Scaling measurements (the honest part)

Probes up the size ladder, same code, fresh runs:

| class | pieces | slots (placements×8) | mode | iters | time | µs/pos | peak RSS |
|---|---|---|---|---|---|---|---|
| KRvKR | 4 | 0.93M | cached | 8 | 0.92 s | 2.1 | 42 MiB |
| KQvKQ | 4 | 0.93M | cached | 9 | 0.70 s | 2.3 | 26 MiB |
| KPvKP | 4 | 0.93M | cached | 12 | 0.64 s | 1.6 | 63 MiB |
| KRRvKR | 5 | 14.9M | cached | 14 | 11.4 s | 4.0 | 303 MiB |
| KBNvKR | 5 | 14.9M | cached | 12 | 46.6 s | 6.5 | 591 MiB |
| KRRvKR | 5 | 14.9M | **streamed** | 14 | 55.3 s | 19.2 | 22 MiB |

(Streamed KRRvKR produced byte-identical value counts to the cached run — a
free cross-mode consistency check.)

Largest solved end-to-end: **6-piece class, see below.**

```
class KRPvKRP positions 121970856 win 38794344 loss 39182376 draw 43994136 illegal 101099304 iters 18 time 1752.759
```

That is 223M slots (27.9M placements) solved in 29 min single-threaded,
~14.4 µs/position, peak RSS 2.1 GiB (value array 223 MB + packed boards 280 MB
+ dependency classes), streamed mode, audited values. It also demonstrates the
memory regime: everything up to and including 7-piece classes fits comfortably;
nothing was sampled or truncated — every class above was solved completely or
not attempted.

### Extrapolation model (anchored on the measurements)

Streamed throughput is ~266 ns per slot-visit; sweeps needed stayed in
1..18 for everything solved and grows slowly (log-like in mate depth), so I
budget 15–25 sweeps for bigger classes. Memory is linear and cheap:
vals 1 B/slot + boards 1.25 B/slot (+ edges ~150 B/slot only below 16.7M
slots). Class sizes are exact combinatorial counts: k distinct-type pieces =
20·19·…·(20−k+1) placements.

| biggest class size | slots | memory (arrays) | est. time single-threaded | verdict on this box |
|---|---|---|---|---|
| 6 pieces | 223M | 0.5 GB | 29 min **measured** | done |
| 7 pieces | 3.13G | ~7 GB | ~5 h per heavy class | feasible |
| 8 pieces | 40.6G | **~91 GB** | ~2.5 days/class | **first infeasible: RAM** |
| 9 pieces | 488G | ~1.1 TB | ~month/class | out |
| 10 pieces (`KBNRPvKBNRP`, the start class) | 5.36T | **~12 TB** | ~330 days for ONE class | hopelessly out |

**Which class first becomes too big, and why:** the 8-piece classes
(e.g. `KBNRvKBNR`, smallest of them at 5.08G placements). Memory kills before
time does — ~91 GB for the two flat arrays alone on a machine where 7-piece
classes already want 7 GB — and the time curve (days per class, times
thousands of classes in the full-game closure) is separately disqualifying.
Six P-cores of trivially parallel sweeps buy back maybe 5×, i.e. nothing
against four more orders of magnitude.

### How far this scales toward the full game — plainly

**Class-complete solving does not reach the full game on a one-week budget on
this hardware, by roughly three orders of magnitude in memory and more in
time.** The bottom-up closure of the start position contains the 10-piece
start class itself; 12 TB of resident state and a year-plus of single-core
sweeps is not a rounding error you engineer away. What *is* within reach on
this budget: every class up to 7 pieces (all of them collectively, not just
one), which would make the solver an excellent ground-truth oracle for
task 04's forward searcher on any position with ≤ 7 pieces — likely covering
the vast majority of positions the forward search actually touches. If you
want the true root value, the measured numbers argue for the alternative
already sketched in `docs/ARCHITECTURE.md`: solve the **reachable subset**
(est. 1e9–1e10 states ≈ 20 GB) instead of complete classes — different
machinery (forward closure + retrograde over the induced subgraph), same
fixed point.

## 4. Independent verification against Fairy-Stockfish

`solve --dump` samples are cross-checked by `xcheck` (drives FSF with
`VariantPath=microchess.ini`, fixed-depth search, compares mate scores):

| class | sampled | agree | notes |
|---|---|---|---|
| KvK | 120 | 120 | all draws |
| KNvK | 120 | 120 | all draws |
| KBvK | 120 | 120 | all draws |
| KRvK | 150 | 150 | |
| KQvK | 150 | 150 | |
| KRvKR | 150 | 150 | |
| KQvKQ | 150 | 150 | |
| KNNvK | 800 | 800 | incl. 4 rare WIN/LOSS labels confirmed |
| KPvKP | 200 | 199 + 1 | the 1 is §5.2, not an error |

WIN/LOSS agreements are proofs (FSF exhibited the forced mate direction);
DRAW agreement is horizon-limited (no mate found at depth 24–26 either way)
but combined with the internal audit is strong. Two FSF quirks surfaced and
are handled/documented in `xcheck.rs`: options must be set after the UCI
handshake or the engine silently stays on 8x8 chess, and it reports `mate 0`
for positions that are already checkmate.

## 5. Things worth knowing (bugs found, conventions, divergences)

### 5.1 KQvK has FEWER raw WIN labels than KRvK — the task table's guess is off

Measured: KQvK win 8,328 vs KRvK win 12,360, though the task table expected
"far more". The reason is legality, not values: the queen checks far more
often than the rook, so many more queen placements are outright illegal as
White-to-move slots (side-not-to-move in check), shrinking KQvK's legal
WTM pool from 12,360 to 8,328. Conditional on being legal and White to move,
both classes are **100% wins**. The solver is right and FSF agrees on every
sampled position; the test suite pins this explanation.

### 5.2 The 50-move rule is the one place FSF must disagree with us

`docs/REPETITION.md` mandates ignoring the 50-move rule; Fairy-Stockfish
enforces it. One KPvKP sample (`1p2/1Pk1/4/1K2/4 b`) came out **WIN** in our
solver and DRAW in FSF. The PV walker shows why: the win is a long zugzwang
grind (capture the blocked pawn, drive, promote, mate) whose forced line
passes halfmove clock ~80 without reaching mate — a win under unlimited play,
a claimable draw under real rules. FSF at depth 60 and `go mate 40` finds no
mate because there is none inside its rule set/horizon. Our label follows the
task convention; expect such positions in your own cross-check if you sample
pawn classes deeply enough (1 in 200 here).

### 5.3 State-space accounting choices (documented, deliberate)

* `positions` counts decodable states: placements × castling-rights variants
  (4) × side to move (2), minus illegals. Rights variants without their pieces
  home are unreachable-but-decodable; their rights are provably inert (the
  castle move needs the pieces home), carry exactly their stripped twin's
  value (unit-tested), and are emitted in `--dump` where engines that strip
  inconsistent rights will still agree on the label.
* `illegal` additionally counts pawns on their promotion ranks — unreachable
  (promotion is mandatory) and unsafe for move generation.
* `time` covers solving the named class only; dependency time goes to stderr.
* `iters` counts full fixed-point sweeps including the confirming sweep.

## 6. Tests added

* Known positions (Fairy-Stockfish-verified by you): bare-kings DRAW, KNvK and
  KBvK draws, `K2R` mate-in-5 WIN, `K2Q` mate-in-4 WIN, stalemated-Black DRAW
  — all via full class solves. The checkmate position
  `1kR1/4/3N/2BP/K3 b - - 9 5` is asserted at terminal level (in-check, no
  moves ⇒ LOSS): solving its class (`KRBNPvK`, 27.9M placements, hours in
  release) does not belong in CI; the stalemate twin gets both treatments.
* Aggregate properties per acceptance table; per-side-to-move structure tests;
  DAG invariant over all 2,304 classes; name round-trip over all classes;
  promotion-dependency path via `KPvK`; inert-castling-rights value identity;
  CLI contract tests parsing the exact stdout line and dump format.
* Pre-existing suites untouched and green (perft ladder depth 9 = 176,466,898,
  codec injectivity incl. the 118,717,620-position BFS replay binaries).

## 7. Tree state

Uncommitted (sandbox cannot run `git commit`). Expected diff when you commit:
modified `solver/src/lib.rs` (module registration), `solver/src/codec.rs`
(additive `pub`s + 4 accessors); new files `solver/src/{matclass,retro}.rs`,
`solver/src/bin/{solve,xcheck,pv}.rs`, `solver/tests/solve_ground.rs`,
this report. Working tree otherwise clean; `cargo test --release` fully green.
