//! Material-class ids, names, and the class DAG (docs/ARCHITECTURE.md).
//!
//! A class id is exactly the codec's `widx * 48 + bidx` (see [`crate::codec`]);
//! a side index is `subset({B,N,R}) * 6 + pawn_slot`, expanded to non-king
//! piece counts `[P, N, B, R, Q]` by [`codec::side_expansion`]. Names are
//! `K` + white letters + `v` + `K` + black letters with the letters in
//! canonical order `B N P Q R`, doubled for doubled pieces (`KNNvK`,
//! `KRRvKR`). Parsing accepts any letter order and any case.
//!
//! Captures and promotions move strictly **down** the DAG. This module
//! provides the direct-successor edges; [`crate::retro`] solves bottom-up.
//! The edge invariant ([`edge_descends`]): a successor is either strictly
//! lighter (capture), or equally heavy with strictly more non-pawn material
//! (promotion trades P for N/B/R/Q at equal piece count). Sorting classes
//! ascending by `(piece_count, Reverse(nonpawn_count))` is a topological
//! order of the DAG — no single scalar potential can serve, because
//! promotions *raise* material quality while captures lower piece count.

use std::collections::BTreeSet;

use crate::codec;

/// Side indices per colour (matches the codec's class layout).
pub const SIDES: usize = 48;

/// Piece-count vector `[P, N, B, R, Q]` parsed from the letters after `K`.
fn parse_side_letters(chars: &[char]) -> Result<[u64; 5], String> {
    if chars.first() != Some(&'K') {
        let s: String = chars.iter().collect();
        return Err(format!("each side must start with 'K': {s:?}"));
    }
    let mut c = [0u64; 5];
    for &ch in &chars[1..] {
        match ch {
            'P' => c[0] += 1,
            'N' => c[1] += 1,
            'B' => c[2] += 1,
            'R' => c[3] += 1,
            'Q' => c[4] += 1,
            other => return Err(format!("unknown piece letter {other:?}")),
        }
    }
    Ok(c)
}

/// Parse a class name such as `KvK`, `KNvK`, `KBNvK`, `KRvKR`, `KNNvK`.
/// Case-insensitive; the separator is `v`. Returns the codec class id.
pub fn parse_class_name(name: &str) -> Result<usize, String> {
    let up: Vec<char> = name.chars().map(|c| c.to_ascii_uppercase()).collect();
    let splits: Vec<usize> =
        up.iter().enumerate().filter_map(|(i, &c)| (c == 'V').then_some(i)).collect();
    if splits.len() != 1 {
        return Err(format!(
            "class name needs exactly one 'v' separator: {name:?}"
        ));
    }
    let w = parse_side_letters(&up[..splits[0]])
        .map_err(|e| format!("white side of {name:?}: {e}"))?;
    let b = parse_side_letters(&up[splits[0] + 1..])
        .map_err(|e| format!("black side of {name:?}: {e}"))?;
    let widx = codec::canon_side(w[0], w[1], w[2], w[3], w[4])
        .map_err(|e| format!("white side of {name:?}: {e}"))?;
    let bidx = codec::canon_side(b[0], b[1], b[2], b[3], b[4])
        .map_err(|e| format!("black side of {name:?}: {e}"))?;
    Ok(widx * SIDES + bidx)
}

fn side_name(m: &[u64; 5]) -> String {
    // Canonical letter order B N P Q R.
    let mut s = String::from("K");
    for (letter, idx) in [('B', 2usize), ('N', 1), ('P', 0), ('Q', 4), ('R', 3)] {
        for _ in 0..m[idx] {
            s.push(letter);
        }
    }
    s
}

/// Canonical name of a class id, e.g. `KBNvK`.
pub fn class_name(class: usize) -> String {
    let (w, b) = codec::class_side_material(class);
    format!("{}v{}", side_name(&w), side_name(&b))
}

/// Direct successors in the class DAG: every single capture (either side
/// loses one piece to the other) and every promotion (either side's pawn
/// becomes N/B/R/Q). Sorted, deduplicated; empty only for `KvK`.
pub fn successors(class: usize) -> Vec<usize> {
    let (w0, b0) = codec::class_side_material(class);
    let mut out = BTreeSet::new();
    let mut add = |w: [u64; 5], b: [u64; 5]| {
        if let (Ok(wi), Ok(bi)) = (
            codec::canon_side(w[0], w[1], w[2], w[3], w[4]),
            codec::canon_side(b[0], b[1], b[2], b[3], b[4]),
        ) {
            out.insert(wi * SIDES + bi);
        }
    };
    // Captures.
    for i in 0..5 {
        if w0[i] > 0 {
            let mut w = w0;
            w[i] -= 1;
            add(w, b0);
        }
        if b0[i] > 0 {
            let mut b = b0;
            b[i] -= 1;
            add(w0, b);
        }
    }
    // Promotions (pawn slot index 0 -> N/B/R/Q at indices 1..=4).
    for pr in 1..5 {
        if w0[0] > 0 {
            let mut w = w0;
            w[0] -= 1;
            w[pr] += 1;
            add(w, b0);
        }
        if b0[0] > 0 {
            let mut b = b0;
            b[0] -= 1;
            b[pr] += 1;
            add(w0, b);
        }
    }
    out.remove(&class);
    out.into_iter().collect()
}

/// Total non-king piece count of the class.
pub fn piece_count(class: usize) -> u64 {
    let (w, b) = codec::class_side_material(class);
    w.iter().sum::<u64>() + b.iter().sum::<u64>()
}

/// Non-pawn, non-king piece count of the class.
pub fn nonpawn_count(class: usize) -> u64 {
    let (w, b) = codec::class_side_material(class);
    w.iter().skip(1).sum::<u64>() + b.iter().skip(1).sum::<u64>()
}

/// Strict order certificate for DAG edges: along every edge `A -> B`
/// (a capture or a promotion out of `A`), either `B` is strictly lighter
/// than `A`, or equally heavy with strictly more non-pawn material. Hence
/// sorting ascending by `(piece_count, Reverse(nonpawn_count))` is a
/// topological order with every dependency before its dependent.
pub fn edge_descends(a: usize, b: usize) -> bool {
    let (pa, qa) = (piece_count(a), nonpawn_count(a));
    let (pb, qb) = (piece_count(b), nonpawn_count(b));
    pb < pa || (pb == pa && qb > qa)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn known_names_parse_to_known_ids() {
        assert_eq!(parse_class_name("KvK").unwrap(), 0);
        // KNvK: white subset={N}, no pawn slot -> idx SUB_N*6 = 2*6 = 12.
        assert_eq!(parse_class_name("KNvK").unwrap(), 12 * SIDES);
        assert_eq!(parse_class_name("krvk").unwrap(), parse_class_name("KRvK").unwrap());
        assert_eq!(parse_class_name("KBNvK").unwrap(), parse_class_name("KNBvK").unwrap());
        assert!(parse_class_name("KBvKN").is_ok());
        // Unrepresentable materials are rejected, not silently mapped.
        assert!(parse_class_name("KPQvK").is_err()); // pawn alongside queen
        assert!(parse_class_name("KPPvK").is_err()); // two pawns
        assert!(parse_class_name("XvK").is_err());
        assert!(parse_class_name("KKvK").is_err());
        assert!(parse_class_name("Kv").is_err());
    }

    #[test]
    fn names_round_trip_over_all_classes() {
        for c in 0..codec::NUM_CLASSES {
            let n = class_name(c);
            let c2 = parse_class_name(&n).unwrap_or_else(|e| panic!("{n}: {e}"));
            assert_eq!(class_name(c2), n, "name round trip failed for class {c}");
        }
    }

    #[test]
    fn dag_edges_go_strictly_down() {
        assert!(successors(parse_class_name("KvK").unwrap()).is_empty());
        let krk = parse_class_name("KRvK").unwrap();
        assert_eq!(successors(krk), vec![0], "KRvK can only capture down to KvK");
        for c in 0..codec::NUM_CLASSES {
            for &s in &successors(c) {
                assert!(
                    edge_descends(c, s),
                    "edge {} -> {} violates the topological invariant",
                    class_name(c),
                    class_name(s)
                );
            }
        }
    }

    #[test]
    fn promotion_edges_exist() {
        // KPvK must reach KQvK, KRvK, KBvK, KNvK directly.
        let succ: Vec<String> =
            successors(parse_class_name("KPvK").unwrap()).iter().map(|&c| class_name(c)).collect();
        for expect in ["KQvK", "KRvK", "KBvK", "KNvK"] {
            assert!(succ.contains(&expect.to_string()), "KPvK successors lack {expect}: {succ:?}");
        }
    }
}
