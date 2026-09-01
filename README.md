# Microchess

A 4x5 chess variant, and an attempt to solve it — to compute the game-theoretic
value of the start position.

**Start here: [docs/RUNBOOK.md](docs/RUNBOOK.md)** — setup, how to run the
enumeration, what to watch out for, and which figures elsewhere in this repo have
gone stale. Then [docs/HANDOVER.md](docs/HANDOVER.md) for the strategic picture
and [docs/FINDINGS.md](docs/FINDINGS.md) for the state of knowledge — what is
verified, what is ruled out, and what it would cost.

## Short version

Rules, exact position encoding, and a verified parallel retrograde fixed point all
exist and are cross-checked against independent implementations. Two solving routes
are ruled out by measurement: forward proof search diverges on a drawn root, and a
dense bottom-up tablebase cannot reach the 10-piece start position.

The surviving route stores only reachable positions. Reachability is a **7,327x**
reduction — measured exactly on the largest class, which holds **732,059,560**
positions. Extrapolating that measured curve puts the whole game at **at least
9.4e9** positions: 4–12 hours of compute, but 21–44 GB of resident state, which
does not fit this machine. Hence external memory.

The external-memory enumeration is built and has reached **ply 16 =
2,733,894,779** positions, which revises that 9.4e9 upward to **3.4e10–8e10** and
scales the cost estimates with it. See [docs/RUNBOOK.md](docs/RUNBOOK.md) §5
and §12.

## Layout

| path | what |
|---|---|
| `docs/RUNBOOK.md` | **how to build, run and resume it** — read this first |
| `docs/FINDINGS.md` | the state of knowledge |
| `docs/SPEC.md` | the exact rules, each verified against the engine |
| `docs/PERFT.md`, `docs/perft.txt` | the rules acceptance test (depth 9 = 176,466,898) |
| `docs/ENCODING.md` | the exact injective u64 key and why a hash will not do |
| `docs/REPETITION.md` | draws, repetition, and the graph-history-interaction trap |
| `docs/REACHABLE.md` | the exact reachable-set measurement |
| `docs/DFPN.md`, `docs/PRUNING.md` | why forward search and pruning do not get there |
| `docs/ARCHITECTURE.md` | design reasoning, and the Tinyhouse prior art |
| `docs/GROUND-TRUTH.md` | independently derived values for cross-checking |
| `solver/` | Rust: rules, codec, transposition table, material classes |
| `analysis/` | measurement tools: oracle, df-pn, witness, overlap, top-class BFS |
| `engine/` | Fairy-Stockfish plus the corrected `microchess.ini` |

## Quick use

```bash
cargo run --release --manifest-path solver/Cargo.toml --bin perft -- 9   # 176466898
cargo run --release --manifest-path solver/Cargo.toml --bin codeck -- 12 # codec check
cargo test --release --manifest-path solver/Cargo.toml                   # 64 tests
python microchess.py best "k3/4/4/3P/KBNR w - - 0 1" --movetime 3000     # mate 5
```

`git checkout bottom-up-tablebase -- <path>` recovers the removed dense tablebase.
