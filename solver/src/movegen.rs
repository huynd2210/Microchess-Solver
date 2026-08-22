//! Pseudo-legal and legal move generation, ported directly from the validated
//! `reference/mgen.cpp`. Geometry tables use the same layout: knight/king
//! target lists, and rook/bishop rays terminated by -1 sentinels per direction.

use crate::{Move, Position, BOARD_LEN};

#[derive(Debug)]
pub struct Geometry {
    pub knight_t: [Vec<u8>; BOARD_LEN],
    pub king_t: [Vec<u8>; BOARD_LEN],
    /// Rook rays with a -1 terminator after each of the 4 directions.
    pub rays_r: [Vec<i16>; BOARD_LEN],
    /// Bishop rays with a -1 terminator after each of the 4 directions.
    pub rays_b: [Vec<i16>; BOARD_LEN],
}

fn in_board(r: i32, f: i32) -> bool {
    (0..5).contains(&r) && (0..4).contains(&f)
}

impl Geometry {
    fn new() -> Geometry {
        let mut g = Geometry {
            knight_t: Default::default(),
            king_t: Default::default(),
            rays_r: Default::default(),
            rays_b: Default::default(),
        };
        for s in 0..BOARD_LEN {
            let r = s as i32 / 4;
            let f = s as i32 % 4;
            const KN: [(i32, i32); 8] =
                [(1, 2), (2, 1), (-1, 2), (-2, 1), (1, -2), (2, -1), (-1, -2), (-2, -1)];
            for (dr, df) in KN {
                if in_board(r + dr, f + df) {
                    g.knight_t[s].push(((r + dr) * 4 + (f + df)) as u8);
                }
            }
            const KG: [(i32, i32); 8] =
                [(1, 0), (-1, 0), (0, 1), (0, -1), (1, 1), (1, -1), (-1, 1), (-1, -1)];
            for (dr, df) in KG {
                if in_board(r + dr, f + df) {
                    g.king_t[s].push(((r + dr) * 4 + (f + df)) as u8);
                }
            }
            const RD: [(i32, i32); 4] = [(1, 0), (-1, 0), (0, 1), (0, -1)];
            for (dr, df) in RD {
                let mut rr = r + dr;
                let mut ff = f + df;
                while in_board(rr, ff) {
                    g.rays_r[s].push((rr * 4 + ff) as i16);
                    rr += dr;
                    ff += df;
                }
                g.rays_r[s].push(-1);
            }
            const BD: [(i32, i32); 4] = [(1, 1), (1, -1), (-1, 1), (-1, -1)];
            for (dr, df) in BD {
                let mut rr = r + dr;
                let mut ff = f + df;
                while in_board(rr, ff) {
                    g.rays_b[s].push((rr * 4 + ff) as i16);
                    rr += dr;
                    ff += df;
                }
                g.rays_b[s].push(-1);
            }
        }
        g
    }
}

pub fn geom() -> &'static Geometry {
    use std::sync::OnceLock;
    static GEOM: OnceLock<Geometry> = OnceLock::new();
    GEOM.get_or_init(Geometry::new)
}

/// Is square `sq` attacked by any piece of the given colour?
/// Direct port of `attacked()` in reference/mgen.cpp.
pub fn attacked(board: &[u8; BOARD_LEN], sq: u8, by_white: bool) -> bool {
    let g = geom();
    let r = sq / 4;
    let f = sq % 4;

    // Pawn attacks. A white pawn on (r-1, f±1) attacks sq; black mirrored.
    if by_white {
        if r >= 1 {
            for df in [-1_i32, 1] {
                let ff = f as i32 + df;
                if (0..4).contains(&ff) && board[((r as i32 - 1) * 4 + ff) as usize] == crate::WP {
                    return true;
                }
            }
        }
    } else if r <= 3 {
        for df in [-1_i32, 1] {
            let ff = f as i32 + df;
            if (0..4).contains(&ff) && board[((r as i32 + 1) * 4 + ff) as usize] == crate::BP {
                return true;
            }
        }
    }

    let (kn, kg, bi, ro, qu) = if by_white {
        (crate::WN, crate::WK, crate::WB, crate::WR, crate::WQ)
    } else {
        (crate::BN, crate::BK, crate::BB, crate::BR, crate::BQ)
    };

    for &t in &g.knight_t[sq as usize] {
        if board[t as usize] == kn {
            return true;
        }
    }
    for &t in &g.king_t[sq as usize] {
        if board[t as usize] == kg {
            return true;
        }
    }

    // Sliders along precomputed rays; each ray segment ends at a -1 sentinel.
    if slide_hit(board, &g.rays_b[sq as usize], bi, qu) {
        return true;
    }
    if slide_hit(board, &g.rays_r[sq as usize], ro, qu) {
        return true;
    }
    false
}

/// Walk one ray list segment by segment; true if the first piece found on a
/// segment matches either slider type.
fn slide_hit(board: &[u8; BOARD_LEN], rays: &[i16], p1: u8, p2: u8) -> bool {
    let mut i = 0;
    while i < rays.len() {
        let mut j = i;
        while j < rays.len() && rays[j] >= 0 {
            let p = board[rays[j] as usize];
            if p != crate::EMPTY {
                if p == p1 || p == p2 {
                    return true;
                }
                break;
            }
            j += 1;
        }
        while i < rays.len() && rays[i] >= 0 {
            i += 1;
        }
        i += 1;
    }
    false
}

pub fn is_white_piece(p: u8) -> bool {
    (crate::WP..=crate::WK).contains(&p)
}

pub fn is_black_piece(p: u8) -> bool {
    (crate::BP..=crate::BK).contains(&p)
}

pub fn king_sq(pos: &Position, white: bool) -> Option<u8> {
    let k = if white { crate::WK } else { crate::BK };
    pos.board.iter().position(|&p| p == k).map(|i| i as u8)
}

/// Is the side to move in check?
pub fn in_check(pos: &Position) -> bool {
    match king_sq(pos, pos.white_to_move) {
        Some(ks) => attacked(&pos.board, ks, !pos.white_to_move),
        None => false,
    }
}

/// All pseudo-legal moves for the side to move (castling checks included).
/// Direct port of `genMoves()` in reference/mgen.cpp.
pub fn gen_pseudo(pos: &Position) -> Vec<Move> {
    let g = geom();
    let mut out = Vec::new();
    let w = pos.white_to_move;
    let b = &pos.board;

    for s in 0..BOARD_LEN {
        let p = b[s];
        if w {
            if !is_white_piece(p) {
                continue;
            }
        } else if !is_black_piece(p) {
            continue;
        }
        let t = if w { p } else { p - 8 };
        let s = s as u8;

        if t == crate::WP {
            // Pawn: one forward push, diagonal captures, mandatory promotion
            // on the last rank. No double step, no en passant.
            let dr: i32 = if w { 1 } else { -1 };
            let r = s / 4;
            let f = s % 4;
            let ns = s as i32 + dr * 4;
            if b[ns as usize] == crate::EMPTY {
                let nr = ns / 4;
                if nr == 4 || nr == 0 {
                    for &pr in &[crate::WQ, crate::WR, crate::WB, crate::WN] {
                        out.push(Move { from: s, to: ns as u8, promo: pr, castle: 0 });
                    }
                } else {
                    out.push(Move { from: s, to: ns as u8, promo: 0, castle: 0 });
                }
            }
            for df in [-1_i32, 1] {
                let ff = f as i32 + df;
                if !(0..4).contains(&ff) {
                    continue;
                }
                let ts = ((r as i32 + dr) * 4 + ff) as usize;
                let q = b[ts];
                if q != crate::EMPTY && (if w { is_black_piece(q) } else { is_white_piece(q) }) {
                    let nr = ts / 4;
                    if nr == 4 || nr == 0 {
                        for &pr in &[crate::WQ, crate::WR, crate::WB, crate::WN] {
                            out.push(Move { from: s, to: ts as u8, promo: pr, castle: 0 });
                        }
                    } else {
                        out.push(Move { from: s, to: ts as u8, promo: 0, castle: 0 });
                    }
                }
            }
        } else if t == crate::WN {
            for &d in &g.knight_t[s as usize] {
                let q = b[d as usize];
                if q == crate::EMPTY || (if w { is_black_piece(q) } else { is_white_piece(q) }) {
                    out.push(Move { from: s, to: d, promo: 0, castle: 0 });
                }
            }
        } else if t == crate::WK {
            for &d in &g.king_t[s as usize] {
                let q = b[d as usize];
                if q == crate::EMPTY || (if w { is_black_piece(q) } else { is_white_piece(q) }) {
                    out.push(Move { from: s, to: d, promo: 0, castle: 0 });
                }
            }
        } else {
            // Sliders.
            if t == crate::WB || t == crate::WQ {
                push_slider_moves(b, w, s, &g.rays_b[s as usize], &mut out);
            }
            if t == crate::WR || t == crate::WQ {
                push_slider_moves(b, w, s, &g.rays_r[s as usize], &mut out);
            }
        }
    }

    // Castling. King a1->c1 with rook d1->b1 (mirrored on rank 5); b/c squares
    // empty and none of a/b/c attacked.
    if w
        && pos.castling & crate::CASTLE_W != 0
        && b[0] == crate::WK
        && b[3] == crate::WR
        && b[1] == crate::EMPTY
        && b[2] == crate::EMPTY
        && !attacked(b, 0, false)
        && !attacked(b, 1, false)
        && !attacked(b, 2, false)
    {
        out.push(Move { from: 0, to: 2, promo: 0, castle: 1 });
    }
    if !w
        && pos.castling & crate::CASTLE_B != 0
        && b[16] == crate::BK
        && b[19] == crate::BR
        && b[17] == crate::EMPTY
        && b[18] == crate::EMPTY
        && !attacked(b, 16, true)
        && !attacked(b, 17, true)
        && !attacked(b, 18, true)
    {
        out.push(Move { from: 16, to: 18, promo: 0, castle: 2 });
    }

    out
}

fn push_slider_moves(b: &[u8; BOARD_LEN], w: bool, s: u8, rays: &[i16], out: &mut Vec<Move>) {
    let mut i = 0;
    while i < rays.len() {
        let mut j = i;
        while j < rays.len() && rays[j] >= 0 {
            let d = rays[j] as u8;
            let q = b[d as usize];
            if q == crate::EMPTY {
                out.push(Move { from: s, to: d, promo: 0, castle: 0 });
            } else {
                if if w { is_black_piece(q) } else { is_white_piece(q) } {
                    out.push(Move { from: s, to: d, promo: 0, castle: 0 });
                }
                break;
            }
            j += 1;
        }
        while i < rays.len() && rays[i] >= 0 {
            i += 1;
        }
        i += 1;
    }
}

/// Legal moves: pseudo-legal moves filtered by "own king not attacked after
/// the move" (make/unmake based, matching `legalMoves()` in mgen.cpp).
pub fn legal_moves(pos: &Position) -> Vec<Move> {
    let mut scratch = *pos; // Position is a tiny Copy type
    let mover_white = scratch.white_to_move;
    let mut out = Vec::new();
    for m in gen_pseudo(&scratch) {
        let undo = scratch.make(m);
        let ok = match king_sq(&scratch, mover_white) {
            Some(ks) => !attacked(&scratch.board, ks, !mover_white),
            None => false,
        };
        scratch.unmake(m, undo);
        if ok {
            out.push(m);
        }
    }
    out
}
