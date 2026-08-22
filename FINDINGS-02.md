# FINDINGS — Task 02: exact position codec + transposition table

**Verdict: done and verified.** All six reference BFS counts reproduced exactly
through ply 12, `roundtrip ok` and `injective ok` at every ply, maxkey =
2^46.69 (budget 2^52), full `cargo test --release` green including the task-01
perft ladder (depth 9 = 176,466,898). The tree is left uncommitted for you
(`git commit` is expected to fail from this sandbox; `git status` shows the
new files: `solver/src/codec.rs`, `solver/src/tt.rs`,
`solver/src/bin/codeck.rs`, `solver/tests/codec.rs`,
`solver/tests/bfs_counts.rs`, plus the one-line module registration in
`solver/src/lib.rs`).

## 1. The rank function

`key = (class_base[class] + placement_rank) * 8 + castling * 2 + stm_bit`

* **Material class** (per side, exactly as docs/ENCODING.md): subset of
  {B,N,R} (8) x pawn slot {none,P,Q,R,B,N} (6) = 48; class = `widx*48 + bidx`,
  2,304 classes. The class expands to a 13-type count vector over the fixed
  alphabet `[EMPTY, WK, BK, WP, WN, WB, WR, WQ, BP, BN, BB, BR, BQ]` — EMPTY
  participates as an ordinary type so all 20 squares rank uniformly.
* **Placement rank**: lexicographic combinatorial rank of the square
  assignment. Walking squares 0..19, every type that sorts before the actual
  one contributes a block of `W(counts - e_t, cells-1)` arrangements, where
  `W = cells!/prod(counts!)` is the multinomial. The blocks are evaluated in
  O(1) each via the identity `W(c - e_t, m-1) = W(c, m) * c_t / m`, so encode
  and decode are ~20x13 integer ops with ~40 divisions total. Identical
  same-colour pieces (two promoted rooks) are one type with count 2, so the
  two physical arrangements are the same multiset placement by construction —
  there is nothing to canonicalise between them, and the exhaustive doubled-
  rook test pins the key count to exactly `20*19*18*17/2` unordered
  placements.
* **Perfect packing**: `class_base` is the prefix sum over classes of each
  class's placement count (`20!/((20-k)! * prod c_t!)`, computed with exact
  sequential divisions). The key space is dense: `key_space()` = total
  placements * 8, and **every** canonical key in `[0, key_space())` decodes.

## 2. Bit budget actually achieved

`key_space() = 113,875,485,044,336 = 2^46.69` — under the 2^52 budget with 5+
bits to spare. The largest single class is `KBNRP vs kbnrp` with
`20!/10! = 670,442,572,800` placements (39.3 bits of rank). Note the doc's
37.8-bit / 232,890,577,920 figure is *not* the per-class maximum; the true
per-class maximum is the 10-piece class above. The design does not depend on
that estimate — the prefix-sum packing absorbs whatever each class needs.

## 3. Exact `codeck` output observed (final binary, ply 12)

```
ply 0 distinct 1
roundtrip ok 1
injective ok 1 1
ply 1 distinct 10
...
ply 6 distinct 56141
ply 7 distinct 246709
ply 8 distinct 1021173
ply 9 distinct 3898949
ply 10 distinct 13634481
ply 11 distinct 42373487
ply 12 distinct 118717620
roundtrip ok 118717620
injective ok 118717620 118717620
maxkey 113875485044336
```

Every ply carries its own `roundtrip ok <cum>` and `injective ok <keys>
<positions>` line (cumulative numbers); all six reference values —
56,141 / 1,021,173 / 3,898,949 / 13,634,481 / 42,373,487 / 118,717,620 —
match exactly. Timing: 22.6 s to ply 10, 249.8 s to ply 12, single-threaded,
peak RSS estimated at ~6 GB (the ply-12 dedup set alone is 2^28 slots x 12
bytes = 3.2 GB; keys + frontier add ~2 GB more).

The BFS dedups by **full position bytes** (board nibbles + side + castling,
11-byte records in a hand-rolled open-addressing set) — never by the codec
key — so the counts are an independent oracle that could have disagreed.

## 4. Things I got wrong first, and fixed (you asked for honesty)

* **Alias classes.** The 48-per-side index scheme contains redundant indices:
  `(subset={}, slot=B)` expands to the same multiset as
  `(subset={B}, slot=none)`, likewise for N and R. My first `decode` accepted
  keys in those ranges, and a random-sampling test immediately found keys that
  decoded to a position which re-encoded to a *different* key — not an
  `encode` collision (encode always emits the canonical/maximal-subset
  representative), but decode was not injective. Fix: `try_decode` rejects
  alias-class keys (`Err("non-canonical (alias) material class")`), making the
  codec a clean bijection between positions and the canonical subset of
  `[0, key_space())`. The invariant is documented in `codec.rs`.
* **Two doubled piece types on one side** (e.g. B=2 *and* R=2): my first
  `canon_side` silently mis-ranked these instead of rejecting them. They are
  unreachable (a side promotes its single pawn at most once), but an
  unrepresentable input must be an error, not a wrong rank — fixed and now
  rejected, with a test.
* Two of my own tests were initially wrong (a "swap identical pieces" test
  that actually moved both rooks to different squares; a TT test that assumed
  no eviction in an overflowing bucket). The codec/TT code was right; the
  tests were rewritten to assert the actual contract.

## 5. Transposition table

`tt.rs`: 4-way buckets, `2^bits` buckets. Bucket selection hashes the exact
key with a double splitmix64 (the codec emits dense small integers at shallow
plies, so a single mixing round felt thin); the **full key is stored in the
entry and compared on probe**, so a wrong value is structurally impossible —
`get(k)` returns `Some(v)` only if `v` was stored by `put(k, v)`. Replacement:
update-in-place, else first free slot, else a key-derived victim. Tests cover:
200k inserts with heavy eviction (every returned value is the probed key's
own), 100k absent probes (None), forced same-bucket collisions (exactly 4
survivors, no leaks), key 0, and overwrite semantics.

## 6. Tests added

* `tests/codec.rs` — round-trips over 14 representative FENs (promotions,
  doubled pieces, castling, mate, stalemate); exhaustive injectivity over the
  single-knight class (6,840 placements) and the doubled-rook class (116,280
  placements); 200k random keys checked for decode/re-encode identity with
  alias rejection; rejection of all unrepresentable materials; key_space <
  2^52.
* `tests/bfs_counts.rs` — byte-dedup BFS reproduces ply 6 = 56,141 and
  ply 8 = 1,021,173, and the codec is injective over the ply-4 BFS.
* `tt.rs` unit tests as above; `codec.rs` unit tests for the class algebra.

## 7. Uncertainties / notes

* **Alias ranges waste a little key space** (kept because the docs fix the
  class count at 2,304). The rejected ranges are small (single-piece classes,
  ~10^2–10^3 placements each), so key_space stays ~2^46.69. If you'd rather
  have a perfectly dense space, dropping the 12 alias side-indices per pair
  would shrink it marginally; I judged doc alignment worth more.
* The key covers board + side + castling only; `halfmove_clock` and
  `fullmove_number` are outside the key per docs/REPETITION.md, and `decode`
  sets them to 0 / 1.
* Symmetry canonicalisation is untouched, per the scope fence.
* `maxkey` is the maximum key *observed* in the BFS. The structural bound is
  `key_space() = 113,875,485,044,336`; both are far below 2^52.
* Runtime/memory at ply 12: ~250 s single-threaded, ~6 GB peak. If you need it
  faster the BFS parallelises trivially per frontier chunk, but I left it
  single-threaded to keep the verification surface minimal.
