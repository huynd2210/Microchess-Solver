//! Exact position codec: `Position` <-> injective `u64` key, per
//! `docs/ENCODING.md`. The key is a **combinatorial rank**, not a hash: it is
//! a mixed-radix perfect packing of
//!
//! ```text
//! key = (class_base[class] + placement_rank) * 8 + castling * 2 + stm_bit
//! ```
//!
//! where `class_base` is the cumulative count of placements of all preceding
//! material classes. Every value in `[0, key_space())` decodes to exactly one
//! position and encodes back to itself: the map is a bijection onto its image,
//! hence injective **by construction** — no collision can exist between two
//! distinct positions because the encoding is a counting argument, and the
//! decode side inverts the same counting argument step by step.
//!
//! # Fields covered
//!
//! The key covers exactly: `board`, `white_to_move`, `castling`.
//! It deliberately does **not** cover `halfmove_clock` or `fullmove_number`
//! (the game ignores the 50-move rule, see `docs/REPETITION.md`); [`decode`]
//! sets them to `0` and `1`.
//!
//! # The invariant relied on
//!
//! For any `Position p` such that
//!
//! 1. there is exactly one white king and exactly one black king on the board;
//! 2. the non-king material of each side is representable in the class
//!    alphabet: at most one pawn, at most one queen, at most two each of
//!    bishop/knight/rook, and **not** (a pawn together with a queen or any
//!    doubled piece) — a side's pawn exists only before its promotion, and a
//!    promotion needs a capture first, so reachable positions always satisfy
//!    this;
//!
//! we guarantee `decode(encode(p))` agrees with `p` on every field the key
//! covers, and `encode(decode(encode(p))) == encode(p)`. Positions violating
//! the invariant are rejected by [`try_encode`] with an `Err`; [`encode`]
//! panics on them. Every value below [`key_space`] decodes successfully.
//!
//! # Canonical classes and the exact domain of `decode`
//!
//! The 48-per-side index scheme contains a handful of *alias* indices: e.g.
//! `(subset={}, slot=B)` describes the same multiset as
//! `(subset={B}, slot=none)`. `encode` always emits the canonical (maximal-
//! subset) representative, so two distinct positions can never share a key —
//! but for the converse (`decode` injective) `try_decode` **rejects** keys
//! that fall in an alias class's range. The exact domain of `decode` is thus
//! the image of `encode`: canonical classes only, and on that domain
//! encode/decode are mutual inverses (a bijection between positions and a
//! subset of `[0, key_space())`).

use std::sync::OnceLock;

use crate::{
    Position, BB, BK, BN, BP, BR, BOARD_LEN, BQ, EMPTY, WB, WK, WN, WP, WR, WQ,
};

/// Number of material classes: per side, subset of {B,N,R} (8) x pawn slot
/// {none,P,Q,R,B,N} (6); 48 x 48 = 2304.
pub const NUM_CLASSES: usize = 48 * 48;

// ---------------------------------------------------------------------------
// Placement alphabet: a fixed total order over the 13 "piece types" (EMPTY
// included, so all 20 squares are ranked uniformly).
// ---------------------------------------------------------------------------

const TYPE_EMPTY: usize = 0;
const TYPE_WK: usize = 1;
const TYPE_BK: usize = 2;
const TYPE_WP: usize = 3;
const TYPE_WN: usize = 4;
const TYPE_WB: usize = 5;
const TYPE_WR: usize = 6;
const TYPE_WQ: usize = 7;
const TYPE_BP: usize = 8;
const TYPE_BN: usize = 9;
const TYPE_BB: usize = 10;
const TYPE_BR: usize = 11;
const TYPE_BQ: usize = 12;
const NUM_TYPES: usize = 13;

const ALPHABET: [u8; NUM_TYPES] = [
    EMPTY, WK, BK, WP, WN, WB, WR, WQ, BP, BN, BB, BR, BQ,
];

const fn build_type_index() -> [u8; 16] {
    let mut t = [0xFFu8; 16];
    t[EMPTY as usize] = TYPE_EMPTY as u8;
    t[WK as usize] = TYPE_WK as u8;
    t[BK as usize] = TYPE_BK as u8;
    t[WP as usize] = TYPE_WP as u8;
    t[WN as usize] = TYPE_WN as u8;
    t[WB as usize] = TYPE_WB as u8;
    t[WR as usize] = TYPE_WR as u8;
    t[WQ as usize] = TYPE_WQ as u8;
    t[BP as usize] = TYPE_BP as u8;
    t[BN as usize] = TYPE_BN as u8;
    t[BB as usize] = TYPE_BB as u8;
    t[BR as usize] = TYPE_BR as u8;
    t[BQ as usize] = TYPE_BQ as u8;
    t
}

const TYPE_INDEX: [u8; 16] = build_type_index();

const FACT: [u64; 21] = {
    let mut f = [1u64; 21];
    let mut i = 1;
    while i < 21 {
        f[i] = f[i - 1] * i as u64;
        i += 1;
    }
    f
};

/// Falling factorial from 20: `FALL[k] = 20 * 19 * ... * (20 - k + 1)`.
const FALL: [u64; 21] = {
    let mut f = [1u64; 21];
    let mut k = 1;
    while k < 21 {
        f[k] = f[k - 1] * (21 - k) as u64;
        k += 1;
    }
    f
};

// ---------------------------------------------------------------------------
// Material classes
// ---------------------------------------------------------------------------

// Pawn-slot codes (order follows docs/ENCODING.md: none, P, Q, R, B, N).
const SLOT_NONE: usize = 0;
const SLOT_P: usize = 1;
const SLOT_Q: usize = 2;
const SLOT_R: usize = 3;
const SLOT_B: usize = 4;
const SLOT_N: usize = 5;

// Subset bits within a side.
const SUB_B: usize = 1;
const SUB_N: usize = 2;
const SUB_R: usize = 4;

/// Expand a side index (`subset * 6 + slot`) into that side's non-king piece
/// counts, ordered `[P, N, B, R, Q]`. This is the class alphabet: it fixes
/// what a class *means*, so identical same-colour pieces produced by promotion
/// (e.g. two rooks = rook in the subset + promoted rook) fall out as ordinary
/// multiset counts and are ranked identically wherever they sit.
fn side_expansion(side_idx: usize) -> [u64; 5] {
    let subset = side_idx / 6;
    let slot = side_idx % 6;
    let mut c = [0u64; 5];
    if subset & SUB_B != 0 {
        c[2] += 1;
    }
    if subset & SUB_N != 0 {
        c[1] += 1;
    }
    if subset & SUB_R != 0 {
        c[3] += 1;
    }
    match slot {
        SLOT_P => c[0] += 1,
        SLOT_N => c[1] += 1,
        SLOT_B => c[2] += 1,
        SLOT_R => c[3] += 1,
        SLOT_Q => c[4] += 1,
        _ => {}
    }
    c
}

/// Canonical side index for observed counts `[P, N, B, R, Q]`.
///
/// Convention (**maximal subset**): every present piece type among {B,N,R}
/// goes into the subset; the single pawn slot must then explain the pawn (if
/// any), else the queen (if any), else the one doubled piece, else nothing.
/// Reachability argument: a side owns its original B/N/R (at most one each)
/// plus at most one promotion product (the pawn is consumed by promoting), so
/// at most one type can be doubled and a pawn excludes every promotion
/// product. Anything violating that is rejected with `Err` — it is outside
/// the class alphabet and encoding it would not be injective.
fn canon_side(p: u64, n: u64, b: u64, r: u64, q: u64) -> Result<usize, String> {
    if p > 1 || q > 1 || b > 2 || n > 2 || r > 2 {
        return Err(format!(
            "material not representable in the class alphabet: P={p} N={n} B={b} R={r} Q={q}"
        ));
    }
    let doubles = (b == 2) as u32 + (n == 2) as u32 + (r == 2) as u32;
    let subset = if b > 0 { SUB_B } else { 0 }
        | if n > 0 { SUB_N } else { 0 }
        | if r > 0 { SUB_R } else { 0 };
    let slot = if p == 1 {
        if q == 1 || doubles > 0 {
            return Err(format!(
                "pawn alongside queen/doubled piece is unrepresentable: P={p} N={n} B={b} R={r} Q={q}"
            ));
        }
        SLOT_P
    } else if q == 1 {
        if doubles > 0 {
            return Err(format!(
                "queen alongside doubled piece is unrepresentable: P={p} N={n} B={b} R={r} Q={q}"
            ));
        }
        SLOT_Q
    } else if doubles > 1 {
        return Err(format!(
            "two doubled piece types need two promotions: P={p} N={n} B={b} R={r} Q={q}"
        ));
    } else if b == 2 {
        SLOT_B
    } else if n == 2 {
        SLOT_N
    } else if r == 2 {
        SLOT_R
    } else {
        SLOT_NONE
    };
    Ok(subset * 6 + slot)
}

/// A side index is *canonical* iff every slot piece type also appears in the
/// subset (the maximal-subset convention of [`canon_side`]). Non-canonical
/// indices expand to the same multiset as some canonical twin (e.g.
/// `(subset={}, slot=B)` ≡ `(subset={B}, slot=none)`), so they carry no
/// positions of their own; `encode` never emits them and `try_decode`
/// rejects them, keeping the key space a bijection onto its image.
fn side_is_canonical(side_idx: usize) -> bool {
    let subset = side_idx / 6;
    let slot = side_idx % 6;
    match slot {
        SLOT_B => subset & SUB_B != 0,
        SLOT_N => subset & SUB_N != 0,
        SLOT_R => subset & SUB_R != 0,
        _ => true,
    }
}

// ---------------------------------------------------------------------------
// Precomputed tables
// ---------------------------------------------------------------------------

struct Tables {
    /// Per class, the 13-type count vector (index 0 = EMPTY count).
    counts: Vec<[u64; NUM_TYPES]>,
    /// `bases[c]` = sum of placements of all classes `< c`, in rank space.
    /// Strictly increasing; `bases[NUM_CLASSES]` = total placements.
    bases: Vec<u64>,
    total_space: u64,
}

fn multinomial(c: &[u64; NUM_TYPES]) -> u64 {
    // Placements = 20! / ((20-k)! * prod_t c_t!) with k = number of pieces.
    // Computed as falling factorial followed by exact sequential divisions
    // (each prefix of the denominator divides what remains).
    let k: usize = c[1..].iter().sum::<u64>() as usize;
    let mut acc = FALL[k];
    for &cnt in &c[1..] {
        acc /= FACT[cnt as usize];
    }
    acc
}

fn tables() -> &'static Tables {
    static TABLES: OnceLock<Tables> = OnceLock::new();
    TABLES.get_or_init(|| {
        let mut counts = Vec::with_capacity(NUM_CLASSES);
        for widx in 0..48usize {
            for bidx in 0..48usize {
                let mut c = [0u64; NUM_TYPES];
                c[TYPE_WK] = 1;
                c[TYPE_BK] = 1;
                let w = side_expansion(widx);
                c[TYPE_WP] = w[0];
                c[TYPE_WN] = w[1];
                c[TYPE_WB] = w[2];
                c[TYPE_WR] = w[3];
                c[TYPE_WQ] = w[4];
                let b = side_expansion(bidx);
                c[TYPE_BP] = b[0];
                c[TYPE_BN] = b[1];
                c[TYPE_BB] = b[2];
                c[TYPE_BR] = b[3];
                c[TYPE_BQ] = b[4];
                let pieces: u64 = c[1..].iter().sum();
                c[TYPE_EMPTY] = BOARD_LEN as u64 - pieces;
                counts.push(c);
            }
        }
        let mut bases = vec![0u64; NUM_CLASSES + 1];
        for i in 0..NUM_CLASSES {
            bases[i + 1] = bases[i] + multinomial(&counts[i]);
        }
        let total_space = bases[NUM_CLASSES] * 8;
        Tables { counts, bases, total_space }
    })
}

/// Total number of distinct keys (= number of distinct representable
/// (placement, castling, side-to-move) states). Every value in `[0,
/// key_space())` decodes to a unique position. Far below `2^52` — see the
/// bit-budget discussion in FINDINGS-02.
pub fn key_space() -> u64 {
    tables().total_space
}

// ---------------------------------------------------------------------------
// Encode
// ---------------------------------------------------------------------------

pub fn try_encode(pos: &Position) -> Result<u64, String> {
    let mut seen_counts = [0u64; NUM_TYPES];
    let mut wk = 0u32;
    let mut bk = 0u32;
    for &p in &pos.board {
        let ti = TYPE_INDEX[p as usize];
        if ti == 0xFF {
            return Err(format!("invalid piece code {p} on the board"));
        }
        seen_counts[ti as usize] += 1;
        match ti as usize {
            TYPE_WK => wk += 1,
            TYPE_BK => bk += 1,
            _ => {}
        }
    }
    if wk != 1 || bk != 1 {
        return Err(format!("need exactly one king per side, found WK={wk} BK={bk}"));
    }

    let widx = canon_side(
        seen_counts[TYPE_WP],
        seen_counts[TYPE_WN],
        seen_counts[TYPE_WB],
        seen_counts[TYPE_WR],
        seen_counts[TYPE_WQ],
    )?;
    let bidx = canon_side(
        seen_counts[TYPE_BP],
        seen_counts[TYPE_BN],
        seen_counts[TYPE_BB],
        seen_counts[TYPE_BR],
        seen_counts[TYPE_BQ],
    )?;
    let class = widx * 48 + bidx;

    let t = tables();
    // The board's piece counts must coincide with the class alphabet (they do
    // by construction of canon_side; this guards against future drift).
    let mut rem = t.counts[class];
    if rem != seen_counts {
        return Err("internal: board counts disagree with class alphabet".to_string());
    }

    // Combinatorial rank of the square assignment, lexicographic over squares
    // 0..20 with types in ALPHABET order. At each square, every type that
    // sorts before the actual one contributes a block of
    //   W(remaining counts minus that type, remaining cells)
    //   = W(current) * count[type] / remaining_cells
    // arrangements (identity: removing one of c_t equal items from a
    // multinomial multiplies the count by c_t / cells).
    let mut w = multinomial(&rem);
    let mut rank: u64 = 0;
    for sq in 0..BOARD_LEN {
        let ti = TYPE_INDEX[pos.board[sq] as usize] as usize;
        let denom = (BOARD_LEN - sq) as u64;
        for t2 in 0..ti {
            if rem[t2] != 0 {
                rank += w * rem[t2] / denom;
            }
        }
        w = w * rem[ti] / denom;
        rem[ti] -= 1;
    }
    debug_assert_eq!(w, 1);

    let stm_bit = u64::from(!pos.white_to_move);
    Ok((t.bases[class] + rank) * 8 + u64::from(pos.castling & 3) * 2 + stm_bit)
}

/// Injective exact encoding. Panics only on positions violating the documented
/// invariant (unreachable in play; use [`try_encode`] for arbitrary input).
pub fn encode(pos: &Position) -> u64 {
    try_encode(pos).unwrap_or_else(|e| panic!("codec::encode: {e}"))
}

// ---------------------------------------------------------------------------
// Decode
// ---------------------------------------------------------------------------

pub fn try_decode(key: u64) -> Result<Position, String> {
    let t = tables();
    if key >= t.total_space {
        return Err(format!("key {key} out of range (key_space = {})", t.total_space));
    }
    let rem = (key & 7) as usize;
    let white_to_move = rem & 1 == 0;
    let castling = (rem >> 1) as u8;
    let q = key >> 3;

    // Largest class whose base does not exceed q. bases[0] = 0 <= q always.
    let class = t.bases[..NUM_CLASSES].partition_point(|&b| b <= q) - 1;
    // Reject alias (non-canonical) classes: they expand to the same multiset
    // as their canonical twin, so accepting them would let two different keys
    // decode to the same position.
    if !side_is_canonical(class / 48) || !side_is_canonical(class % 48) {
        return Err(format!("key {key} lies in a non-canonical (alias) material class"));
    }
    let mut rank = q - t.bases[class];
    let mut cnt = t.counts[class];
    let mut w = multinomial(&cnt);

    let mut board = [EMPTY; BOARD_LEN];
    for sq in 0..BOARD_LEN {
        let denom = (BOARD_LEN - sq) as u64;
        let mut chosen = NUM_TYPES;
        for ti in 0..NUM_TYPES {
            if cnt[ti] == 0 {
                continue;
            }
            let block = w * cnt[ti] / denom;
            if rank >= block {
                rank -= block;
                continue;
            }
            chosen = ti;
            w = block;
            break;
        }
        if chosen == NUM_TYPES {
            return Err(format!("corrupt key {key}: no type fits square {sq}"));
        }
        board[sq] = ALPHABET[chosen];
        cnt[chosen] -= 1;
    }
    if rank != 0 {
        return Err(format!("corrupt key {key}: residual rank {rank}"));
    }
    Ok(Position {
        board,
        white_to_move,
        castling,
        halfmove_clock: 0,
        fullmove_number: 1,
    })
}

/// Exact inverse of [`encode`] on the covered fields. Panics on keys >=
/// [`key_space`] (use [`try_decode`] for arbitrary input).
pub fn decode(key: u64) -> Position {
    try_decode(key).unwrap_or_else(|e| panic!("codec::decode: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn key_space_fits_budget() {
        // The whole point of the design: everything under 2^52.
        assert!(key_space() < 1 << 52, "key_space = {}", key_space());
    }

    #[test]
    fn decode_rejects_alias_class_keys() {
        // (subset={}, slot=B) is the alias of the canonical ({B}, none).
        let alias_side = 0 * 6 + SLOT_B;
        assert!(!side_is_canonical(alias_side));
        let alias_class = alias_side * 48; // black side index 0 = {}/none, canonical
        assert!(!side_is_canonical(alias_class % 48) || !side_is_canonical(alias_class / 48));
        let base = tables().bases[alias_class];
        // Any key inside the alias range must be rejected...
        let width = tables().bases[alias_class + 1] - base;
        assert!(width > 0);
        let key = base * 8;
        assert!(try_decode(key).is_err(), "alias-class key must not decode");
        // ...and its canonical twin decodes to a position that re-encodes to
        // itself.
        let canon_side_idx = SUB_B * 6 + SLOT_NONE;
        let canon_class = canon_side_idx * 48;
        let cbase = tables().bases[canon_class];
        let pos = try_decode(cbase * 8).expect("canonical twin must decode");
        assert_eq!(encode(&pos), cbase * 8);
    }

    #[test]
    fn startpos_roundtrip() {
        let pos = crate::startpos();
        let k = encode(&pos);
        let back = decode(k);
        assert_eq!(back.board, pos.board);
        assert_eq!(back.white_to_move, pos.white_to_move);
        assert_eq!(back.castling, pos.castling);
        assert_eq!(encode(&back), k);
    }

    #[test]
    fn canon_side_expands_back_to_the_same_counts() {
        // The property that matters: for every count vector the class alphabet
        // can represent, canonicalising and re-expanding is the identity.
        // (canon_side deliberately maps several side indices to one canonical
        // representative when they describe the same multiset.)
        for p in 0..=1u64 {
            for n in 0..=2u64 {
                for b in 0..=2u64 {
                    for r in 0..=2u64 {
                        for q in 0..=1u64 {
                            let doubles =
                                (b == 2) as u32 + (n == 2) as u32 + (r == 2) as u32;
                            let reachable = !(p == 1 && (q == 1 || doubles > 0))
                                && !(p == 0 && q == 1 && doubles > 0)
                                && !(doubles > 1);
                            if !reachable {
                                assert!(canon_side(p, n, b, r, q).is_err(),
                                    "({p},{n},{b},{r},{q}) should be rejected");
                                continue;
                            }
                            let idx = canon_side(p, n, b, r, q).unwrap();
                            let e = side_expansion(idx);
                            assert_eq!([p, n, b, r, q], e, "counts ({p},{n},{b},{r},{q}) -> idx {idx} -> {e:?}");
                        }
                    }
                }
            }
        }
    }
}
