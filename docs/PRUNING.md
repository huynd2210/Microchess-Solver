# Does pruning rescue this? Measured, not argued.

**The 12 TB figure was a property of retrograde analysis, not of the problem.**
Retrograde cannot prune by construction — it runs the fixed point backwards over
every slot in a class. AO*/LAO* prunes: it only needs a **witness**, the strategy
DAG proving one root's value. So: how big is a witness?

Measured on classes solved exhaustively (`analysis/witness.cpp`), sampling every
101st slot. At our nodes one move suffices; at the opponent's every reply must be
covered — whose node it is comes from side-to-move parity against the root, *not*
from the value, since a DRAW node can belong to either player.

## A won root prunes enormously

| class | class size | WIN witness median | max | max as % of class |
|---|---:|---:|---:|---:|
| KRvk | 7,950 | 78 | 461 | 5.80% |
| KNBvk | 137,996 | 184 | 2,842 | 2.06% |
| KNRvk | 128,088 | 70 | 817 | 0.64% |
| KBQvk | 112,008 | 27 | 139 | 0.12% |
| KRQvk | 105,040 | 12 | 92 | **0.09%** |

**Pruning factor 10²–10⁴, and it improves with class size** — a decisive root needs
a few dozen positions out of millions.

## A drawn root does not

| class | class size | DRAW witness median | as % of class |
|---|---:|---:|---:|
| KvK | 540 | 110 | 20.4% |
| KBvK | 8,712 | 3,747 | 43.7% |
| KNvK | 8,904 | 8,043 | **90.4%** |

A draw is only established by exhausting alternatives, so there is nothing to
prune away: the witness *is* most of the drawn component. This matches the sibling
Tinyhouse project, which measured a drawn witness at **16–24% of the whole game**.

## Why this decides nothing in our favour

The microchess start position is almost certainly a **draw** — Fairy-Stockfish
returns `cp 0` at depth 69 after 60 s, and the position is materially symmetric.
So the case that prunes well is the case we are probably not in.

Net effect on the requirement:

| approach | storage needed |
|---|---|
| retrograde over all classes | 4.3e12 slots → **12 TB** — impossible |
| reachability-driven (visit only reachable) | 1.8e9–1.5e10 → 4–32 GB with symmetry |
| AO* pruned, **drawn** root | 20–90% of reachable → same order, ~1.1–5× better |
| AO* pruned, **decisive** root | 0.1–6% of reachable → trivial |

**Pruning is a bonus of about 1.1–5×, not the fix.** The fix remains
reachability-driven storage; pruning shaves it further, and would collapse the
problem entirely if the root turns out to be decisive.

## Caveats on these numbers

* Witnesses are measured **within one class**; out-of-class moves (captures,
  promotions) are treated as leaves, which is legitimate only because lower classes
  are already solved. The full-game witness spans classes and is the sum of the
  in-class pieces.
* Roots whose only drawing move is a capture leave the class in one step and
  report a witness of 1. Those are excluded and counted separately as a boundary
  artefact — 62 to 245 per class sampled.
* The reliable drawn figures are the all-draw classes (KNvK, KBvK), where nothing
  escapes. Those are also degenerate: no mating material, so the drawn component is
  the entire class. The honest analogue for a rich drawn root is Tinyhouse's
  16–24%.
