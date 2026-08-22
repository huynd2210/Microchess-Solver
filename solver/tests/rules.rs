//! Rules-focused tests: promotion, castling rights, pins, check legality,
//! clocks, make/unmake integrity.

use solver::movegen::{in_check, legal_moves};
use solver::{perft, Position, Undo, START_FEN};

fn legal_ucis(fen: &str) -> Vec<String> {
    let pos = Position::from_fen(fen).unwrap();
    legal_moves(&pos).iter().map(|m| m.uci()).collect()
}

#[test]
fn start_has_9_legal_moves() {
    let u = legal_ucis(START_FEN);
    assert_eq!(u.len(), 9, "moves: {u:?}");
    // Castling is NOT available from the start: b1/c1 are still occupied by
    // the bishop and knight (the reference generator agrees - it requires
    // b1/c1 empty before generating a1c1).
    assert!(!u.contains(&"a1c1".to_string()), "moves: {u:?}");
}

#[test]
fn castling_generated_once_knight_and_bishop_leave_home() {
    // King a1, rook d1, b1/c1 empty, right intact -> a1c1 must be generated.
    let fen = "kbnr/3p/4/3P/K2R w Dd - 0 1";
    let u = legal_ucis(fen);
    assert!(u.contains(&"a1c1".to_string()), "castle should be legal here: {u:?}");
}

#[test]
fn promotion_is_mandatory_and_generates_four_choices() {
    // White pawn on c4 (FEN row 2 = rank 4), kings tucked away in the corners.
    let fen = "k3/2P1/4/4/K3 w - - 0 1";
    let u = legal_ucis(fen);
    let promos: Vec<&String> = u.iter().filter(|s| s.starts_with("c4c5")).collect();
    assert_eq!(promos.len(), 4, "moves: {u:?}");
    for suffix in ["q", "r", "b", "n"] {
        assert!(
            promos.iter().any(|s| s.ends_with(suffix)),
            "missing promotion to {suffix}: {u:?}"
        );
    }
    // No bare c4c5: a pawn may never remain a pawn on the last rank.
    assert!(!u.contains(&"c4c5".to_string()));
}

#[test]
fn pawn_cannot_push_to_occupied_square_and_no_double_step() {
    // White pawn d2, black pawn d3 blocking the push.
    let fen = "k3/4/3p/3P/K3 w - - 0 1";
    let u = legal_ucis(fen);
    assert!(!u.contains(&"d2d3".to_string()), "blocked push generated: {u:?}");
    assert!(!u.iter().any(|s| s.starts_with("d2d4")), "double step generated: {u:?}");
}

#[test]
fn castling_right_lost_when_rook_moves() {
    let mut pos = Position::from_fen(START_FEN).unwrap();
    let m = pos.move_from_uci("d1d2").unwrap(); // rook leaves d1
    let undo = pos.make(m);
    assert_eq!(pos.castling & solver::CASTLE_W, 0);
    pos.unmake(m, undo);
    assert_eq!(pos.castling & solver::CASTLE_W, solver::CASTLE_W);
}

#[test]
fn castling_right_lost_when_king_moves() {
    let mut pos = Position::from_fen(START_FEN).unwrap();
    let m = pos.move_from_uci("a1a2").unwrap();
    pos.make(m);
    assert_eq!(pos.castling, solver::CASTLE_B);
}

#[test]
fn castling_right_lost_when_rook_square_is_captured_on() {
    // Both rights intact; black rook d5 runs down the open d-file capturing
    // the white rook on d1 -> the white right dies (capture ON d1), and the
    // black right dies too (the rook left its home square d5).
    let fen = "kbnr/4/4/4/KBNR b Dd - 0 1";
    let mut pos = Position::from_fen(fen).unwrap();
    let m = pos.move_from_uci("d5d1").unwrap();
    pos.make(m);
    assert_eq!(pos.castling & solver::CASTLE_W, 0, "capture on d1 must kill the white right");
    assert_eq!(pos.castling & solver::CASTLE_B, 0, "rook leaving d5 must kill the black right");
}

#[test]
fn castling_illegal_when_destination_attacked() {
    // Black rook on c5 attacks c1 through the empty c-file; b1/c1 are empty
    // and the right is intact, so only the attack rule forbids a1c1.
    let fen = "k1r1/4/4/4/K2R w D - 0 1";
    let u = legal_ucis(fen);
    assert!(!u.contains(&"a1c1".to_string()), "castle into an attacked square: {u:?}");
}

#[test]
fn castling_illegal_when_in_check() {
    // Black rook a5 checks the king on a1 down the open a-file.
    let fen = "r2k/4/4/4/K2R w D - 0 1";
    let u = legal_ucis(fen);
    assert!(!u.contains(&"a1c1".to_string()), "castling out of check allowed: {u:?}");
}

#[test]
fn pinned_knight_has_no_moves() {
    // Black rook a5 pins the white knight a4 against the king a1. Every knight
    // move leaves the a-file, so a4 must generate nothing.
    let fen = "r2k/4/N3/4/K3 w - - 0 1";
    let u = legal_ucis(fen);
    assert!(!u.iter().any(|s| s.starts_with("a4")), "pinned knight moved: {u:?}");
}

#[test]
fn checkmate_position_has_no_legal_moves() {
    // From docs/SPEC.md: 1kR1/4/3N/2BP/K3 b -> mate in 0.
    let pos = Position::from_fen("1kR1/4/3N/2BP/K3 b - - 0 1").unwrap();
    assert!(legal_moves(&pos).is_empty());
    assert!(in_check(&pos));
}

#[test]
fn stalemate_position_has_no_legal_moves_and_no_check() {
    // From docs/SPEC.md: k3/3N/1K2/4/4 b -> stalemate.
    let pos = Position::from_fen("k3/3N/1K2/4/4 b - - 0 1").unwrap();
    assert!(legal_moves(&pos).is_empty());
    assert!(!in_check(&pos));
}

#[test]
fn clocks_update_on_make() {
    let mut pos = Position::from_fen(START_FEN).unwrap();
    let m = pos.move_from_uci("d2d3").unwrap(); // pawn move resets the clock
    pos.make(m);
    assert_eq!(pos.halfmove_clock, 0);
    assert_eq!(pos.fullmove_number, 1); // white moved, fullmove unchanged
    let reply = legal_moves(&pos).into_iter().next().unwrap();
    pos.make(reply);
    assert_eq!(pos.fullmove_number, 2); // black moved
    assert_eq!(pos.halfmove_clock, 1); // non-pawn, non-capture (a quiet reply exists)
}

#[test]
fn make_unmake_deep_walk_from_start() {
    // Exhaustive walk to depth 4 checking FEN restoration after every unmake.
    let root = Position::from_fen(START_FEN).unwrap();
    let fen = root.to_fen();
    let mut pos = root;
    walk(&mut pos, &fen, 4);
    // Tie the walk to the oracle.
    assert_eq!(perft(&Position::from_fen(START_FEN).unwrap(), 4), 3_957);
}

fn walk(pos: &mut Position, _root_fen: &str, depth: u32) {
    let baseline = pos.to_fen();
    for m in legal_moves(pos) {
        let undo: Undo = pos.make(m);
        if depth > 1 {
            walk(pos, &baseline, depth - 1);
        }
        pos.unmake(m, undo);
        assert_eq!(pos.to_fen(), baseline, "unmake failed after {}", m.uci());
    }
}

#[test]
fn divide_sums_match_perft() {
    let pos = Position::from_fen(START_FEN).unwrap();
    for d in 1..=4 {
        let rows = solver::divide(&pos, d);
        let total: u64 = rows.iter().map(|(_, n)| n).sum();
        assert_eq!(total, perft(&pos, d), "divide/perft disagree at depth {d}");
    }
}
