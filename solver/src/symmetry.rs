//! The colour symmetry: swap colours, flip the board vertically, flip the side
//! to move, and swap the castling rights.
//!
//! Microchess is `kbnr/3p/4/3P/KBNR` — a board that is already its own vertical
//! mirror with colours exchanged. The *rules* are symmetric under this map for
//! any position: it sends legal positions to legal positions and legal moves to
//! legal moves, and it negates the game value.
//!
//! # What it does and does not buy
//!
//! For the **retrograde solve** the saving is a clean 2x: `mirror` pairs a
//! material class with its transpose, so settling one settles the other for
//! free.
//!
//! For the **enumeration** it is not obviously 2x, and the difference matters.
//! `mirror` flips the side to move, and a position with White to move is only
//! ever reached at an even ply while its mirror, with Black to move, is only
//! ever reached at an odd one. `mirror(startpos)` — the opening array with Black
//! to move — is not reachable at all, since pawns cannot retreat to restore it.
//! So the reachable set is *not* closed under `mirror`, and collapsing it onto
//! canonical representatives saves somewhere between nothing and half. The
//! factor is an empirical question; `symcount` measures it on a real store.
//!
//! Enumerating canonical forms is still *correct* whatever that factor is:
//! `mirror(children(p)) == children(mirror(p))` as sets, so expanding either
//! representative of a canonical pair yields the same canonical children. That
//! identity is asserted in the tests rather than assumed.

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
}
