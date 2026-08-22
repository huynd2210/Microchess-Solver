//! `codeck <maxply>` — verification binary for the exact position codec.
//!
//! Performs a BFS from the start position, deduplicating by **full position
//! bytes** (board nibbles + side-to-move + castling), *never* by the codec
//! key, so the BFS is an independent oracle that can disagree with the codec.
//! For every distinct position it then checks
//!
//!   1. `decode(encode(p)) == p` on the covered fields, and
//!   2. `encode(decode(encode(p))) == encode(p)`,
//!
//! and compares the number of distinct keys against the number of distinct
//! positions at every ply. Output lines (stable prefixes for parsing):
//!
//! ```text
//! ply <n> distinct <count>
//! roundtrip <ok|FAIL> <positions_checked>
//! injective <ok|FAIL> <distinct_keys> <distinct_positions>
//! maxkey <value>
//! ```
//!
//! `ply`/`roundtrip`/`injective` are printed once per ply with cumulative
//! numbers; `maxkey` is the global maximum key observed, printed once at the
//! end. Any FAIL exits with status 1 after printing the offending FEN to
//! stderr.

use std::env;
use std::io::Write;
use std::process::exit;
use std::time::Instant;

use solver::codec;
use solver::movegen::legal_moves;
use solver::{Position, BOARD_LEN, EMPTY, START_FEN};

// ---------------------------------------------------------------------------
// Packed position record: 20 board squares x 4 bits (piece codes are 0..=14)
// + one byte holding side-to-move (bit 0) and castling rights (bits 1..3).
// This record — full position bytes, not the codec key — is the BFS identity.
// ---------------------------------------------------------------------------

#[derive(Clone, Copy)]
struct Rec([u8; 11]);

fn pack(pos: &Position) -> Rec {
    let mut r = [0u8; 11];
    for j in 0..10 {
        r[j] = pos.board[2 * j] | (pos.board[2 * j + 1] << 4);
    }
    r[10] = (!pos.white_to_move as u8) | ((pos.castling & 3) << 1);
    Rec(r)
}

fn unpack(rec: Rec) -> Position {
    let mut b = [EMPTY; BOARD_LEN];
    for j in 0..10 {
        b[2 * j] = rec.0[j] & 0xF;
        b[2 * j + 1] = rec.0[j] >> 4;
    }
    Position {
        board: b,
        white_to_move: rec.0[10] & 1 == 0,
        castling: rec.0[10] >> 1,
        halfmove_clock: 0,
        fullmove_number: 1,
    }
}

// ---------------------------------------------------------------------------
// Open-addressing hash set over packed records (linear probing, load <= 0.7).
// Sized to hold >100M records without rehash storms.
// ---------------------------------------------------------------------------

struct RecSet {
    /// Each slot: [used flag][11-byte record]; empty slots all-zero.
    slots: Vec<[u8; 12]>,
    mask: u64,
    count: usize,
}

fn rec_hash(rec: &[u8; 11]) -> u64 {
    let lo = u64::from_le_bytes(rec[0..8].try_into().unwrap());
    let hi = u32::from_le_bytes(rec[7..11].try_into().unwrap());
    solver::tt::splitmix64(lo ^ ((hi as u64) << 32))
}

impl RecSet {
    fn new(cap_slots: usize) -> Self {
        assert!(cap_slots.is_power_of_two());
        RecSet { slots: vec![[0u8; 12]; cap_slots], mask: (cap_slots - 1) as u64, count: 0 }
    }

    fn num_slots(&self) -> usize {
        self.slots.len()
    }

    fn grow(&mut self) {
        let old = std::mem::take(&mut self.slots);
        let ncap = old.len() * 2;
        self.slots = vec![[0u8; 12]; ncap];
        self.mask = (ncap - 1) as u64;
        self.count = 0;
        for slot in &old {
            if slot[0] != 0 {
                let rec: [u8; 11] = slot[1..12].try_into().unwrap();
                self.insert_raw(&rec);
            }
        }
    }

    /// Insert without load-factor check.
    fn insert_raw(&mut self, rec: &[u8; 11]) -> bool {
        let h = rec_hash(rec);
        let mut i = (h & self.mask) as usize;
        loop {
            let s = &self.slots[i];
            if s[0] == 0 {
                self.slots[i][0] = 1;
                self.slots[i][1..12].copy_from_slice(rec);
                self.count += 1;
                return true;
            }
            if s[1..12] == *rec {
                return false;
            }
            i = (i + 1) & self.mask as usize;
        }
    }

    fn insert(&mut self, rec: &[u8; 11]) -> bool {
        if (self.count + 1) * 10 > self.num_slots() * 7 {
            self.grow();
        }
        self.insert_raw(rec)
    }
}

// ---------------------------------------------------------------------------
// Codec verification of one position; returns its exact key.
// ---------------------------------------------------------------------------

fn verify(pos: &Position, checked: &mut u64, maxkey: &mut u64) -> u64 {
    let k1 = codec::try_encode(pos)
        .unwrap_or_else(|e| panic!("codec rejected a reachable position: {e}\n{}", pos.to_fen()));
    let back = codec::decode(k1);
    let fields_equal = back.board == pos.board
        && back.white_to_move == pos.white_to_move
        && back.castling == pos.castling;
    let k2 = codec::encode(&back);
    if !fields_equal || k2 != k1 {
        eprintln!("roundtrip FAIL\n  original: {}\n  decoded:  {}", pos.to_fen(), back.to_fen());
        println!("roundtrip FAIL {checked}");
        exit(1);
    }
    *checked += 1;
    if k1 > *maxkey {
        *maxkey = k1;
    }
    k1
}

/// Merge two sorted key vectors into a new sorted vector; returns it plus the
/// number of distinct values overall.
fn merge_count(a: &[u64], b: &[u64]) -> (Vec<u64>, u64) {
    let mut out = Vec::with_capacity(a.len() + b.len());
    let (mut i, mut j) = (0usize, 0usize);
    let mut uniq = 0u64;
    while i < a.len() || j < b.len() {
        let v = if j >= b.len() || (i < a.len() && a[i] <= b[j]) {
            let v = a[i];
            i += 1;
            v
        } else {
            let v = b[j];
            j += 1;
            v
        };
        if out.last() != Some(&v) {
            uniq += 1;
        }
        out.push(v);
    }
    (out, uniq)
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let maxply: usize = match args.get(1) {
        Some(s) => s.parse().unwrap_or_else(|e| panic!("bad maxply {s:?}: {e}")),
        None => {
            eprintln!("usage: codeck <maxply>  (no argument given; defaulting to 10)");
            10
        }
    };

    let start = Position::from_fen(START_FEN).expect("built-in start FEN must parse");

    let mut seen = RecSet::new(1 << 22);
    let root_rec = pack(&start);
    seen.insert(&root_rec.0);

    let mut frontier: Vec<Rec> = vec![root_rec];
    let mut checked: u64 = 0;
    let mut maxkey: u64 = 0;

    // Cumulative sorted vector of the keys of all distinct positions seen so
    // far, used for the per-ply global injectivity verdict.
    let mut merged_keys: Vec<u64> = Vec::new();

    // Ply 0.
    let k = verify(&start, &mut checked, &mut maxkey);
    merged_keys.push(k);
    println!("ply 0 distinct 1");
    println!("roundtrip ok {checked}");
    println!("injective ok {} {}", merged_keys.len(), 1);
    let _ = std::io::stdout().flush();

    let t0 = Instant::now();
    for d in 1..=maxply {
        let mut next: Vec<Rec> = Vec::with_capacity(frontier.len() * 2);
        let mut cur_keys: Vec<u64> = Vec::new();

        for rec in &frontier {
            let pos = unpack(*rec);
            for m in legal_moves(&pos) {
                let mut child = pos;
                child.make(m);
                let crec = pack(&child);
                if seen.insert(&crec.0) {
                    next.push(crec);
                    cur_keys.push(verify(&child, &mut checked, &mut maxkey));
                }
            }
        }

        cur_keys.sort_unstable();
        let (nm, uniq) = merge_count(&merged_keys, &cur_keys);
        merged_keys = nm;

        let cumulative = seen.count as u64;
        println!("ply {d} distinct {cumulative}");
        println!("roundtrip ok {checked}");
        let ok = uniq == cumulative;
        println!(
            "injective {} {uniq} {cumulative}",
            if ok { "ok" } else { "FAIL" }
        );
        let _ = std::io::stdout().flush();
        if !ok {
            eprintln!(
                "injectivity violated at ply {d}: {uniq} distinct keys vs {cumulative} distinct positions"
            );
            exit(1);
        }

        frontier = next;
        eprintln!(
            "[codeck] ply {d}: cumulative {cumulative}, checked {checked}, {:.1}s",
            t0.elapsed().as_secs_f64()
        );
        if frontier.is_empty() {
            eprintln!("[codeck] frontier exhausted — state space fully enumerated");
            break;
        }
    }

    println!("maxkey {maxkey}");
    eprintln!(
        "[codeck] maxkey {maxkey} = 2^{:.2} (budget 2^52: {}); positions checked: {checked}",
        (maxkey as f64).log2(),
        maxkey < (1u64 << 52)
    );
}
