//! Ties the BFS enumeration to the independently measured cumulative
//! distinct-position counts (reference C++ implementation). Only the small
//! plies run here; the full ladder is verified by `codeck`.

use std::collections::HashSet;

use solver::movegen::legal_moves;
use solver::{Position, START_FEN};

/// Cumulative distinct positions by ply, measured by reference/mgen.cpp.
const REFERENCE: [(usize, u64); 2] = [(6, 56_141), (8, 1_021_173)];

#[test]
fn bfs_counts_match_reference() {
    for &(ply, expected) in &REFERENCE {
        assert_eq!(bfs_cumulative(ply), expected, "cumulative distinct at ply {ply}");
    }
}

fn bfs_cumulative(maxply: usize) -> u64 {
    // Dedup by full position bytes (board + side + castling) — deliberately
    // NOT by the codec key, so this check stays independent of the codec.
    let start = Position::from_fen(START_FEN).unwrap();
    let mut seen: HashSet<[u8; 22]> = HashSet::new();
    let pack = |p: &Position| {
        let mut k = [0u8; 22];
        k[..20].copy_from_slice(&p.board);
        k[20] = p.white_to_move as u8;
        k[21] = p.castling;
        k
    };
    seen.insert(pack(&start));
    let mut frontier = vec![start];
    for _ in 0..maxply {
        let mut next = Vec::new();
        for pos in &frontier {
            for m in legal_moves(pos) {
                let mut child = *pos;
                child.make(m);
                let k = pack(&child);
                if seen.insert(k) {
                    next.push(child);
                }
            }
        }
        frontier = next;
    }
    seen.len() as u64
}

#[test]
fn codec_is_injective_over_ply4_bfs() {
    // Every distinct position reached by ply 4 must get a distinct key.
    // Dedup is again by full position bytes.
    let start = Position::from_fen(START_FEN).unwrap();
    let mut seen: HashSet<[u8; 22]> = HashSet::new();
    let pack = |p: &Position| {
        let mut k = [0u8; 22];
        k[..20].copy_from_slice(&p.board);
        k[20] = p.white_to_move as u8;
        k[21] = p.castling;
        k
    };
    let mut frontier = vec![start];
    let mut all = vec![start];
    seen.insert(pack(&start));
    for _ in 0..4 {
        let mut next = Vec::new();
        for pos in &frontier {
            for m in legal_moves(pos) {
                let mut child = *pos;
                child.make(m);
                if seen.insert(pack(&child)) {
                    next.push(child);
                    all.push(child);
                }
            }
        }
        frontier = next;
    }
    let mut keys: Vec<u64> = all.iter().map(|p| solver::codec::encode(p)).collect();
    assert_eq!(keys.len(), seen.len());
    keys.sort_unstable();
    keys.dedup();
    assert_eq!(keys.len(), seen.len(), "key collision within the ply-4 BFS");
}
