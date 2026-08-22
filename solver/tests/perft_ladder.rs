//! The acceptance ladder: reads docs/perft.txt and asserts every depth,
//! so the baseline cannot drift silently.

use std::fs;
use std::path::PathBuf;

use solver::{perft, Position, START_FEN};

fn perft_ladder_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../docs/perft.txt")
}

fn read_ladder() -> Vec<(u32, u64)> {
    let text = fs::read_to_string(perft_ladder_path())
        .expect("docs/perft.txt must exist next to the solver crate");
    text.lines()
        .filter(|l| !l.trim().is_empty())
        .map(|l| {
            let mut it = l.split_whitespace();
            let d: u32 = it.next().expect("depth").parse().expect("depth int");
            let n: u64 = it.next().expect("nodes").parse().expect("node count");
            (d, n)
        })
        .collect()
}

#[test]
fn perft_matches_docs_ladder() {
    let ladder = read_ladder();
    assert!(!ladder.is_empty(), "docs/perft.txt is empty");
    // The acceptance bar explicitly includes depth 9 = 176466898.
    assert_eq!(ladder.last().unwrap(), &(9, 176_466_898));
    let pos = Position::from_fen(START_FEN).unwrap();
    for &(depth, expected) in &ladder {
        let got = perft(&pos, depth);
        assert_eq!(
            got,
            expected,
            "perft {depth} mismatch: got {got}, docs/perft.txt says {expected}"
        );
    }
}

#[test]
fn perft5_is_not_the_missing_promotion_value() {
    // Regression canary from docs/SPEC.md: 32923 = promotions missing.
    let pos = Position::from_fen(START_FEN).unwrap();
    assert_ne!(perft(&pos, 5), 32_923);
    assert_eq!(perft(&pos, 5), 32_944);
}
