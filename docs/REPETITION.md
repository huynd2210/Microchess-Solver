# Draws, repetition, and why path-based loop detection is unsound

**Decision: the 50-move rule is ignored.** The solver answers the value under
unlimited play, where the only draw mechanisms are stalemate and repetition. This
is the standard convention for "solving" a game and it removes the halfmove clock
from the state.

## The rule as first proposed, and its two defects

> "Track in the TT whether that state has shown up before. If yes, they form a
> closed loop, and every state in that loop is a draw."

The intuition is right — leftover cycles are draws — but this mechanism is unsound
twice over.

**1. A state on a cycle can still be a win.** A cycle proves only that a draw is
*achievable*. It is a lower bound, not the value. If a node on the cycle also has a
move into a won position, its value is WIN. Marking every state on the loop as a
draw demotes real wins.

**2. Graph History Interaction — the silent one.** Whether a repetition occurs
depends on the **path**, not the state. Position `P` reached via path A may repeat
an earlier position (draw); the same `P` reached via path B may not. Caching
"`P` = draw" in the TT from path A and reading it back on path B is a wrong answer
with no symptom. This is a known, published class of solver bug, and *transposition
table + path-based repetition* is exactly the combination that produces it. A TT
entry must be a fact about the position, never about the route taken to it.

## The sound mechanism — which the material decomposition gives us for free

Cycles can only exist among moves that leave material unchanged, because captures
and promotions move strictly **down** the acyclic material-class DAG (see
`docs/ARCHITECTURE.md`). Therefore **every cycle lies inside one material class.**

Solve bottom-up. Inside a class:

* every capture and every promotion leads to an **already-solved lower class** and
  is an exact leaf value, not a cycle;
* what remains is a finite, closed subgraph of quiet moves. Compute its value by
  **fixed point (value iteration)**, which is AO* extended to cyclic graphs — the
  algorithm is **LAO\***, and the loop-handling step is exactly what plain AO*
  lacks:

```
repeat until nothing changes:
    node is WIN  if SOME child is LOSS
    node is LOSS if EVERY child is WIN
everything still unresolved when the iteration converges is a DRAW
```

That last line is the original intuition, made sound. It is "leftover after
convergence = draw", not "loop seen on this path = draw". It consults no history,
so GHI cannot arise, and defect 1 disappears because a winning alternative resolves
the node before the leftover rule applies.

**Why this does not hit the Tinyhouse wall.** Their fixed point leaked because a
depth-bounded region always has a frontier with absent successors, and the eviction
cascaded to the root. Here each material class is **closed**: every move out of it
lands in a class that is already fully solved. There is no frontier to leak
through. This is the whole reason the class decomposition is load-bearing rather
than a mere optimisation.
