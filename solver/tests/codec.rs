//! Codec-focused acceptance tests: round-trips, canonical ordering of
//! identical pieces, exhaustive injectivity on small material classes, the
//! 2^52 bit budget, and rejection of positions outside the class alphabet.

use solver::codec;
use solver::{Position, BOARD_LEN, BK, EMPTY, WK};

fn rt(fen: &str) {
    let pos = Position::from_fen(fen).expect(fen);
    let k1 = codec::try_encode(&pos).unwrap_or_else(|e| panic!("{fen}: {e}"));
    let back = codec::decode(k1);
    assert_eq!(back.board, pos.board, "board mismatch for {fen}");
    assert_eq!(back.white_to_move, pos.white_to_move, "stm mismatch for {fen}");
    assert_eq!(back.castling, pos.castling, "castling mismatch for {fen}");
    assert_eq!(codec::encode(&back), k1, "re-encode mismatch for {fen}");
}

#[test]
fn roundtrip_over_representative_positions() {
    for fen in [
        "kbnr/3p/4/3P/KBNR w Dd - 0 1", // start
        "kbnr/3p/4/3P/KBNR b Dd - 0 1",
        "kbnr/3p/4/3P/K2R w Dd - 0 1",  // castling actually available
        "kbnr/3p/4/3P/K2R b Dd - 0 1",
        "k3/2P1/4/4/K3 w - - 0 1",      // promotion about to happen
        "k3/2Q1/4/4/K3 b - - 0 1",      // promoted queen
        "k3/2N1/4/4/K3 b - - 0 1",
        "k3/4/4/RR2/K3 w - - 0 1",      // two white rooks (doubled via promotion)
        "k3/4/4/rr2/K3 b - - 0 1",      // two black rooks
        "k3/4/4/N1N1/K3 w - - 0 1",     // doubled knights
        "k1b1/4/4/4/KB2 b - - 0 1",     // doubled bishops, one per side
        "1kR1/4/3N/2BP/K3 b - - 0 1",   // checkmate
        "k3/3N/1K2/4/4 b - - 0 1",      // stalemate
        "kbnr/4/4/4/KBNR w D - 0 1",    // pawns gone, one right
    ] {
        rt(fen);
    }
}

#[test]
fn identical_pieces_do_not_double_count() {
    // Two indistinguishable white rooks: the codec ranks the *multiset*
    // placement, so there are exactly as many keys as unordered square pairs.
    // The exhaustive check lives in `exhaustive_injective_doubled_rook_class`
    // below (it pins the count to 20*19*18*17/2); here we pin the property
    // that moving both rooks to different squares changes the key (they are
    // different positions), while the material class stays the same.
    let a = Position::from_fen("k3/4/4/RR2/K3 w - - 0 1").unwrap();
    let c = Position::from_fen("k3/4/4/R1R1/K3 w - - 0 1").unwrap();
    assert_ne!(codec::encode(&a), codec::encode(&c));
    // Determinism.
    assert_eq!(codec::encode(&a), codec::encode(&Position::from_fen("k3/4/4/RR2/K3 w - - 0 1").unwrap()));
}

#[test]
fn stm_and_castling_change_the_key() {
    let mut pos = Position::from_fen("kbnr/3p/4/3P/KBNR w Dd - 0 1").unwrap();
    let k_w_dd = codec::encode(&pos);
    pos.white_to_move = false;
    let k_b_dd = codec::encode(&pos);
    assert_ne!(k_w_dd, k_b_dd);
    pos.white_to_move = true;
    pos.castling = 0;
    assert_ne!(k_w_dd, codec::encode(&pos));
}

/// Exhaustive injectivity over every placement of a small material class:
/// white king, black king, one white knight.
#[test]
fn exhaustive_injective_single_knight_class() {
    let mut keys = Vec::new();
    for wk in 0..BOARD_LEN as u8 {
        for bk in 0..BOARD_LEN as u8 {
            if bk == wk {
                continue;
            }
            for n in 0..BOARD_LEN as u8 {
                if n == wk || n == bk {
                    continue;
                }
                let mut board = [EMPTY; BOARD_LEN];
                board[wk as usize] = WK;
                board[bk as usize] = BK;
                board[n as usize] = solver::WN;
                let pos = Position {
                    board,
                    white_to_move: true,
                    castling: 0,
                    halfmove_clock: 0,
                    fullmove_number: 1,
                };
                let k = codec::encode(&pos);
                let back = codec::decode(k);
                assert_eq!(back.board, board);
                keys.push(k);
            }
        }
    }
    keys.sort_unstable();
    keys.dedup();
    assert_eq!(keys.len(), 20 * 19 * 18, "keys must be pairwise distinct");
}

/// Exhaustive injectivity with *identical* pieces: two white rooks (the class
/// `K + RR` vs bare king), enumerated over unordered rook pairs.
#[test]
fn exhaustive_injective_doubled_rook_class() {
    let mut keys = Vec::new();
    for wk in 0..BOARD_LEN as u8 {
        for bk in 0..BOARD_LEN as u8 {
            if bk == wk {
                continue;
            }
            for r1 in 0..BOARD_LEN as u8 {
                if r1 == wk || r1 == bk {
                    continue;
                }
                for r2 in (r1 + 1)..BOARD_LEN as u8 {
                    if r2 == wk || r2 == bk {
                        continue;
                    }
                    let mut board = [EMPTY; BOARD_LEN];
                    board[wk as usize] = WK;
                    board[bk as usize] = BK;
                    board[r1 as usize] = solver::WR;
                    board[r2 as usize] = solver::WR;
                    let pos = Position {
                        board,
                        white_to_move: false,
                        castling: 0,
                        halfmove_clock: 0,
                        fullmove_number: 1,
                    };
                    let k = codec::encode(&pos);
                    let back = codec::decode(k);
                    assert_eq!((back.board, back.white_to_move), (board, false));
                    keys.push(k);
                }
            }
        }
    }
    keys.sort_unstable();
    keys.dedup();
    assert_eq!(keys.len(), 20 * 19 * 18 * 17 / 2, "keys must be pairwise distinct");
}

#[test]
fn key_space_is_dense_bijection_on_samples() {
    // Every key below key_space() is either (a) a canonical key: decode and
    // re-encode are the identity, or (b) an alias-class key, rejected by
    // try_decode. Nothing else can happen.
    let space = codec::key_space();
    assert!(space < 1 << 52, "key_space {space} exceeds the 2^52 budget");
    let mut x: u64 = 0x243F_6A88_85A3_08D3;
    let mut ok = 0u64;
    let mut alias = 0u64;
    for _ in 0..200_000 {
        x = x
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        let k = x % space;
        match codec::try_decode(k) {
            Ok(pos) => {
                assert_eq!(codec::encode(&pos), k, "decode/encode not identity at key {k}");
                ok += 1;
            }
            Err(e) => {
                assert!(e.contains("non-canonical"), "unexpected decode error: {e}");
                alias += 1;
            }
        }
    }
    assert!(ok > 100_000, "too few canonical keys hit: {ok} ok / {alias} alias");
    println!("{ok} canonical / {alias} alias out of 200000 samples");
}

#[test]
fn decode_rejects_out_of_range() {
    let space = codec::key_space();
    assert!(codec::try_decode(space).is_err());
    assert!(codec::try_decode(u64::MAX).is_err());
}

#[test]
#[should_panic(expected = "out of range")]
fn decode_panics_out_of_range() {
    codec::decode(codec::key_space());
}

#[test]
fn encode_rejects_positions_outside_the_alphabet() {
    // No white king.
    assert!(codec::try_encode(&Position::from_fen("kbnr/3p/4/3P/4 w - - 0 1").unwrap()).is_err());
    // Two white kings.
    assert!(codec::try_encode(&Position::from_fen("k3/4/4/3K/K3 w - - 0 1").unwrap()).is_err());
    // Pawn and queen for the same side: unreachable (promotion consumes the
    // pawn) and outside the class alphabet.
    assert!(codec::try_encode(&Position::from_fen("k3/4/4/QP2/K3 w - - 0 1").unwrap()).is_err());
    // Pawn plus doubled rook, same side: likewise unrepresentable.
    assert!(codec::try_encode(&Position::from_fen("k3/4/4/RRP1/K3 w - - 0 1").unwrap()).is_err());
    // Queen plus doubled rook, same side.
    assert!(codec::try_encode(&Position::from_fen("k3/4/4/RRQ1/K3 w - - 0 1").unwrap()).is_err());
    // Two doubled piece types, same side (would need two promotions).
    assert!(codec::try_encode(&Position::from_fen("k3/4/4/BBRR/K3 w - - 0 1").unwrap()).is_err());
    // Three rooks, one side.
    assert!(codec::try_encode(&Position::from_fen("k3/4/4/RRR1/K3 w - - 0 1").unwrap()).is_err());
}
