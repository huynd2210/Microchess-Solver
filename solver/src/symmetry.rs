//! The colour symmetry: swap colours, flip the board vertically, flip the side
//! to move, and swap the castling rights.
//!
//! Microchess is `kbnr/3p/4/3P/KBNR` — a board that is already its own vertical
//! mirror with colours exchanged. The *rules* are symmetric under this map for
//! any position: it sends legal positions to legal positions and legal moves to
//! legal moves, and it negates the game value.
//!
//! # What it buys: exactly 2x, and why that is not obvious
//!
//! For the **retrograde solve** the saving is a clean 2x: `mirror` pairs a
//! material class with its transpose, so settling one settles the other.
//!
//! For the **enumeration** the same 2x holds, but it turns on a fact specific to
//! this variant. `mirror` flips the side to move, so a White-to-move position
//! reached at an even ply has its mirror reachable only at an odd one — the
//! reachable set is closed under `mirror` only if `mirror(startpos)`, the
//! opening array with Black to move, is itself reachable.
//!
//! In standard chess it is not, and the proof is familiar: with no pawn moves
//! and castling rights intact, only knights can move, and a knight needs an even
//! number of moves to return home, so the two sides' move counts are both even
//! and can never differ by one. **That proof fails here.** Microchess's second
//! rank is empty but for the d-pawn, so the bishop is free immediately, and the
//! bishop has an *odd* closed walk: `b1-c2-d3-b1`. Hence `1. Bc2 Na4 2. Bd3 Nc5
//! 3. Bb1` — five plies, Black to move, every piece home, both castling rights
//! intact.
//!
//! So the reachable set *is* closed under `mirror`: P reachable by `m` implies
//! `mirror(P)` reachable by (path to `mirror(start)`) then `mirror(m)`, which
//! also bounds the detour — a position at ply d has its mirror by ply d+5. And
//! since `mirror` flips the side to move, no position is its own mirror. The
//! reachable set is therefore a disjoint union of mirror pairs and canonical
//! enumeration saves **exactly 2x**. A store truncated at some ply will measure
//! less than that, purely because the deepest positions' partners lie beyond the
//! cut; `symcount` separates that artefact from the real rate.
//!
//! Enumerating canonical forms is sound for a separate reason:
//! `mirror(children(p)) == children(mirror(p))` as sets, so expanding either
//! representative of a canonical pair yields the same canonical children. All of
//! this is asserted in the tests rather than assumed.
//!
//! # Why the *values* survive it
//!
//! Space saved on a wrong answer is worse than no saving, so the value argument
//! is kept separate from the counting one.
//!
//! Under this project's conventions a position's value is a function of the
//! position alone — the 50-move rule is ignored, which takes the halfmove clock
//! out of the state, and repetition is resolved by fixed-point iteration inside
//! a material class rather than by consulting the path (`docs/REPETITION.md`).
//! That value is then determined by exactly two things: the move graph, and the
//! labelling of terminal nodes.
//!
//! `mirror` preserves both. The graph, by `mirror_commutes_with_move_generation`.
//! The terminals, by `mirror_preserves_check_and_therefore_terminal_values`:
//! move counts are preserved, and so is `in_check`, so a mate mirrors to a mate
//! and a stalemate to a stalemate — the one distinction between a loss and a
//! draw at a leaf. Value iteration reads nothing else, so an isomorphism that
//! preserves the initial labelling preserves every iterate and therefore the
//! fixed point:
//!
//! ```text
//! value(mirror(p)) == value(p)      from the mover's point of view
//! ```
//!
//! Two hazards this does *not* remove, both for whoever wires canonicalisation
//! into the solver:
//!
//! * The identity holds for **mover-relative** values (WIN/LOSS/DRAW for the
//!   side to move). A table storing White-relative values must negate on
//!   lookup, and getting that backwards inverts the answer with no other
//!   symptom.
//! * A canonical solve walks the class DAG, so it must also hold at class
//!   granularity — `mirror` sends a class to its transpose, consistently for
//!   every position in it, which is
//!   `mirror_maps_a_class_onto_a_single_transposed_class`.

use crate::{codec, Position, BOARD_LEN, CASTLE_B, CASTLE_W, EMPTY};

/// Colour bit in the piece encoding: white pieces are 1..=6, black 9..=14.
const COLOUR_BIT: u8 = 8;

/// Swaps colours, flips the board vertically, flips the side to move, and swaps
/// the castling rights. An involution.
pub fn mirror(p: &Position) -> Position {
    let mut q = *p;
    q.board = [EMPTY; BOARD_LEN];
    for i in 0..BOARD_LEN {
        let (rank, file) = (i / 4, i % 4);
        let j = (4 - rank) * 4 + file;
        let piece = p.board[i];
        q.board[j] = if piece == EMPTY {
            EMPTY
        } else {
            piece ^ COLOUR_BIT
        };
    }
    q.white_to_move = !p.white_to_move;
    q.castling = ((p.castling & CASTLE_W) << 1) | ((p.castling & CASTLE_B) >> 1);
    q
}

/// `mirror` lifted to codec keys. The key encodes (placement, castling, side to
/// move) and no clocks, so this is exactly the key of the mirrored position.
pub fn mirror_key(key: u64) -> u64 {
    codec::encode(&mirror(&codec::decode(key)))
}

/// The representative of `key`'s mirror pair: the smaller of the two.
/// Idempotent, and equal for a key and its mirror.
pub fn canon_key(key: u64) -> u64 {
    let m = mirror_key(key);
    if m < key {
        m
    } else {
        key
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{movegen, startpos, BK, BP, WK, WP};

    fn child_keys(p: &Position) -> Vec<u64> {
        let mut v: Vec<u64> = movegen::legal_moves(p)
            .iter()
            .map(|m| {
                let mut q = *p;
                q.make(*m);
                codec::encode(&q)
            })
            .collect();
        v.sort_unstable();
        v.dedup();
        v
    }

    /// Least ply at which any of `targets` first appears, searching to `limit`.
    fn first_ply_reaching(targets: &[u64], limit: u32) -> Option<u32> {
        use std::collections::HashSet;
        let want: HashSet<u64> = targets.iter().copied().collect();
        let start = startpos();
        let mut seen: HashSet<u64> = HashSet::new();
        seen.insert(codec::encode(&start));
        let mut frontier = vec![start];
        for ply in 1..=limit {
            let mut next = Vec::new();
            for p in &frontier {
                for m in movegen::legal_moves(p).iter() {
                    let mut q = *p;
                    q.make(*m);
                    let k = codec::encode(&q);
                    if seen.insert(k) {
                        if want.contains(&k) {
                            return Some(ply);
                        }
                        next.push(q);
                    }
                }
            }
            frontier = next;
        }
        None
    }

    /// Walks the real game tree so the properties are checked on positions that
    /// actually arise — castling rights half-lost, pawns mid-board, checks.
    fn walk(depth: u32) -> Vec<Position> {
        let mut frontier = vec![startpos()];
        let mut all = frontier.clone();
        for _ in 0..depth {
            let mut next = Vec::new();
            for p in &frontier {
                for m in movegen::legal_moves(p).iter() {
                    let mut q = *p;
                    q.make(*m);
                    next.push(q);
                }
            }
            all.extend(next.iter().copied());
            frontier = next;
        }
        all
    }

    #[test]
    fn mirror_is_an_involution() {
        for p in walk(5) {
            assert_eq!(mirror(&mirror(&p)).board, p.board);
            assert_eq!(mirror(&mirror(&p)).white_to_move, p.white_to_move);
            assert_eq!(mirror(&mirror(&p)).castling, p.castling);
        }
    }

    /// The opening array is its own colour-swapped vertical mirror, but the side
    /// to move flips — so the start position is *not* a fixed point, and its
    /// mirror is not reachable. This is the fact that stops the enumeration
    /// saving being a free 2x.
    #[test]
    fn startpos_board_is_symmetric_but_the_position_is_not_fixed() {
        let s = startpos();
        let m = mirror(&s);
        assert_eq!(m.board, s.board, "opening array should mirror onto itself");
        assert_ne!(m.white_to_move, s.white_to_move);
        assert_ne!(codec::encode(&m), codec::encode(&s));
    }

    #[test]
    fn mirror_maps_pieces_and_squares_correctly() {
        let s = startpos();
        // white king on a1 (index 0) -> black king on a5 (index 16)
        assert_eq!(s.board[0], WK);
        assert_eq!(mirror(&s).board[16], BK);
        // white pawn on d2 (rank 1, file 3 => index 7) mirrors to rank 3 =>
        // index 15, which is d4 — where the black pawn already stands, since
        // the opening array is its own mirror.
        assert_eq!(s.board[7], WP);
        assert_eq!(mirror(&s).board[15], BP);
        assert_eq!(s.board[15], BP);
    }

    #[test]
    fn castling_rights_swap_sides() {
        let mut p = startpos();
        p.castling = CASTLE_W;
        assert_eq!(mirror(&p).castling, CASTLE_B);
        p.castling = CASTLE_B;
        assert_eq!(mirror(&p).castling, CASTLE_W);
        p.castling = CASTLE_W | CASTLE_B;
        assert_eq!(mirror(&p).castling, CASTLE_W | CASTLE_B);
        p.castling = 0;
        assert_eq!(mirror(&p).castling, 0);
    }

    /// The property that makes enumerating canonical forms sound: expanding
    /// either representative of a mirror pair yields the same set of children,
    /// up to mirroring. Without this, collapsing onto representatives would
    /// silently lose successors.
    #[test]
    fn mirror_commutes_with_move_generation() {
        for p in walk(5) {
            let direct = child_keys(&mirror(&p));
            let mut mirrored: Vec<u64> = child_keys(&p).into_iter().map(mirror_key).collect();
            mirrored.sort_unstable();
            mirrored.dedup();
            assert_eq!(direct, mirrored, "children(mirror(p)) != mirror(children(p))");
        }
    }

    #[test]
    fn mirror_key_is_an_involution_and_canon_is_stable() {
        for p in walk(5) {
            let k = codec::encode(&p);
            assert_eq!(mirror_key(mirror_key(k)), k);
            let c = canon_key(k);
            assert_eq!(canon_key(c), c, "canon must be idempotent");
            assert_eq!(canon_key(mirror_key(k)), c, "a pair must share one canon");
            assert!(c == k || c == mirror_key(k));
        }
    }

    /// The opening array with Black to move **is** reachable, in five plies —
    /// and this single fact is what makes the symmetry worth a full 2x.
    ///
    /// The standard-chess argument says it is unreachable: with no pawn moves
    /// and castling rights intact, only knights can move, and a knight needs an
    /// even number of moves to return home, so both sides' move counts are even
    /// and can never differ by one. That argument does not survive here.
    /// Microchess's second rank is empty but for the d-pawn, so the bishop is
    /// free from move one — and the bishop has an *odd* closed walk:
    /// `b1-c2-d3-b1`. Three White moves against two Black knight moves is five
    /// plies, Black to move, every piece home, both castling rights intact.
    ///
    ///     1. Bc2 Na4  2. Bd3 Nc5  3. Bb1
    #[test]
    fn mirror_of_startpos_is_reachable_in_five_plies() {
        let start = startpos();
        let target = mirror_key(codec::encode(&start));
        assert_eq!(
            first_ply_reaching(&[target], 8),
            Some(5),
            "the opening array with Black to move should arise after 1.Bc2 Na4 2.Bd3 Nc5 3.Bb1"
        );
    }

    /// Because `mirror(start)` is reachable, the reachable set is *closed* under
    /// `mirror`: if P is reachable by move sequence `m`, then `mirror(P)` is
    /// reachable by (path to mirror(start)) followed by `mirror(m)`. Since
    /// `mirror` flips the side to move, no position is its own mirror, so the
    /// set is a disjoint union of mirror pairs and canonicalising saves exactly
    /// 2x — not "about 2x".
    ///
    /// The construction also bounds the detour: a position at ply d has its
    /// mirror by ply d+5. This checks that for every position up to ply 4.
    #[test]
    fn reachable_set_is_closed_under_mirror() {
        use std::collections::HashSet;
        const SHALLOW: u32 = 4;
        const DEEP: u32 = SHALLOW + 5;

        let mut seen: HashSet<u64> = HashSet::new();
        let start = startpos();
        seen.insert(codec::encode(&start));
        let mut frontier = vec![start];
        let mut shallow: Vec<u64> = vec![codec::encode(&start)];
        for ply in 1..=DEEP {
            let mut next = Vec::new();
            for p in &frontier {
                for m in movegen::legal_moves(p).iter() {
                    let mut q = *p;
                    q.make(*m);
                    let k = codec::encode(&q);
                    if seen.insert(k) {
                        if ply <= SHALLOW {
                            shallow.push(k);
                        }
                        next.push(q);
                    }
                }
            }
            frontier = next;
        }
        let missing = shallow
            .iter()
            .filter(|k| !seen.contains(&mirror_key(**k)))
            .count();
        assert_eq!(
            missing, 0,
            "{missing} of {} positions within ply {SHALLOW} lack a mirror by ply {DEEP}",
            shallow.len()
        );
    }

    /// Legality is preserved in both directions, so the map never invents or
    /// destroys positions.
    #[test]
    fn mirror_preserves_move_count() {
        for p in walk(5) {
            assert_eq!(
                movegen::legal_moves(&p).len(),
                movegen::legal_moves(&mirror(&p)).len()
            );
        }
    }

    /// The fact the *solve* rests on, as opposed to the enumeration.
    ///
    /// A position's value is fixed by the graph plus the labelling of its
    /// terminals. `mirror_commutes_with_move_generation` gives the graph
    /// isomorphism; this gives the terminal labelling. Together they force
    /// `value(mirror(p)) == value(p)` from the mover's point of view, because
    /// value iteration reads nothing else — so an isomorphism that preserves
    /// the initial labels preserves every iterate and hence the fixed point.
    ///
    /// Checkmate and stalemate are the same node count apart: both have no legal
    /// moves, and only `in_check` separates a loss from a draw. If `mirror`
    /// preserved move counts but flipped check, mates and stalemates would swap
    /// and the solve would return confidently wrong values with no other symptom.
    #[test]
    fn mirror_preserves_check_and_therefore_terminal_values() {
        for p in walk(6) {
            let m = mirror(&p);
            assert_eq!(
                movegen::in_check(&p),
                movegen::in_check(&m),
                "mirror flipped the check status"
            );
            assert_eq!(
                movegen::legal_moves(&p).is_empty(),
                movegen::legal_moves(&m).is_empty()
            );
        }

        // No mate or stalemate occurs within six plies of the start, so the
        // loop above never actually exercises a terminal. These are the
        // reference terminals from docs/SPEC.md, also used by tests/rules.rs.
        let mate = Position::from_fen("1kR1/4/3N/2BP/K3 b - - 0 1").unwrap();
        assert!(movegen::legal_moves(&mate).is_empty() && movegen::in_check(&mate));
        let mm = mirror(&mate);
        assert!(
            movegen::legal_moves(&mm).is_empty() && movegen::in_check(&mm),
            "a mate must mirror to a mate, never to a stalemate"
        );

        let stale = Position::from_fen("k3/3N/1K2/4/4 b - - 0 1").unwrap();
        assert!(movegen::legal_moves(&stale).is_empty() && !movegen::in_check(&stale));
        let sm = mirror(&stale);
        assert!(
            movegen::legal_moves(&sm).is_empty() && !movegen::in_check(&sm),
            "a stalemate must mirror to a stalemate, never to a mate"
        );
    }

    /// A canonical solve walks the material-class DAG bottom-up, so `mirror`
    /// must land inside that DAG rather than somewhere outside it: it should
    /// send a class to its transpose (White's material swapped with Black's),
    /// and do so consistently for every position of that class.
    #[test]
    fn mirror_maps_a_class_onto_a_single_transposed_class() {
        use std::collections::HashMap;
        let mut seen: HashMap<usize, usize> = HashMap::new();
        for p in walk(6) {
            let k = codec::encode(&p);
            let (a, b) = (codec::class_of_key(k), codec::class_of_key(mirror_key(k)));
            match seen.get(&a) {
                Some(prev) => assert_eq!(
                    *prev, b,
                    "class {a} mirrored to {b} here but {prev} earlier — not a class-level map"
                ),
                None => {
                    seen.insert(a, b);
                }
            }
            // and the map is an involution on classes too
            assert_eq!(codec::class_of_key(mirror_key(mirror_key(k))), a);
        }
        assert!(seen.len() > 1, "sample covered only one material class");
    }
}
