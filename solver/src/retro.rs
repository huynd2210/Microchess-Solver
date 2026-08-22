//! Retrograde fixed point over one material class (docs/REPETITION.md).
//!
//! # Value convention — READ FIRST
//!
//! Every value produced here is **from the side to move's point of view**:
//!
//! * [`V_WIN`]  — the side to move forces mate: some legal move leads to a
//!   position whose value is `LOSS` (for whoever is to move there);
//! * [`V_LOSS`] — the side to move is checkmated now, stalemated into a lost
//!   terminal never happens (stalemate is a draw), or every legal move hands
//!   the opponent a position that is `WIN` for them;
//! * [`V_DRAW`] — neither provable: with unlimited play the position drifts
//!   forever (repetition, stalemate traps, insufficient force). Draws come
//!   **only** from convergence of the fixed point below; no history of any
//!   path is consulted anywhere, so Graph History Interaction cannot arise.
//!
//! Mixing this convention up with "White's view" is the classic bug of this
//! task; everything below is written strictly in side-to-move terms and the
//! terminal assignment (`checkmate => LOSS`, `stalemate => DRAW`) pins the
//! orientation.
//!
//! # Algorithm (per class, bottom-up over `crate::matclass`)
//!
//! A class's slots are `(placement, castling rights, side to move)`,
//! indexed `rank*8 + castle*2 + black_to_move` — exactly the codec key minus
//! the class base, so a child lookup after a move is one encode + one array
//! read. Pass 0 enumerates all slots, rejects illegal placements and assigns
//! terminals. Every capture/promotion lands in an already-solved lower class
//! (dependencies solved first in topological order), so it is a constant
//! leaf value. What remains is a closed quiet-move subgraph, settled by:
//!
//! ```text
//! repeat until nothing changes:
//!     node is WIN  if SOME child is LOSS
//!     node is LOSS if EVERY child is WIN
//! everything still unresolved when this converges is a DRAW
//! ```
//!
//! For classes that fit the edge-cache budget the move graph is materialised
//! once as a CSR adjacency (same-class edges as slot ids, dependency edges
//! pre-collapsed to their final constant value), making each sweep a pure
//! integer pass; larger classes stream moves from packed boards every sweep.

use std::collections::HashMap;
use std::time::Instant;

use crate::codec;
use crate::matclass;
use crate::movegen::{attacked, in_check, king_sq, legal_moves};
use crate::{Position, BOARD_LEN, BP, EMPTY, WP};

pub const V_LOSS: u8 = 0;
pub const V_DRAW: u8 = 1;
pub const V_WIN: u8 = 2;
pub const V_ILLEGAL: u8 = 3;
const V_UNKNOWN: u8 = 255;

pub fn value_name(v: u8) -> &'static str {
    match v {
        V_LOSS => "LOSS",
        V_DRAW => "DRAW",
        V_WIN => "WIN",
        V_ILLEGAL => "ILLEGAL",
        _ => "UNKNOWN",
    }
}

/// Terminal value of `pos`: `Some(V_LOSS)` if checkmate, `Some(V_DRAW)` if
/// stalemate, `None` if legal moves exist.
pub fn classify_terminal(pos: &Position) -> Option<u8> {
    if !legal_moves(pos).is_empty() {
        return None;
    }
    Some(if in_check(pos) { V_LOSS } else { V_DRAW })
}

/// A decoded placement can still be outside legal play: kings adjacent, the
/// side *not* to move in check (the previous mover left his king en prise),
/// or a pawn sitting on its promotion rank (unreachable — promotion is
/// mandatory — and unsafe for move generation, which would step off-board).
fn illegal_placement(pos: &Position) -> bool {
    for sq in 0..BOARD_LEN {
        let p = pos.board[sq];
        if p == WP && sq >= 16 {
            return true;
        }
        if p == BP && sq < 4 {
            return true;
        }
    }
    let (wk, bk) = match (king_sq(pos, true), king_sq(pos, false)) {
        (Some(a), Some(b)) => (a, b),
        _ => return true, // classes guarantee both kings; belt and braces
    };
    if (wk / 4).abs_diff(bk / 4) <= 1 && (wk % 4).abs_diff(bk % 4) <= 1 {
        return true; // adjacent (incl. diagonal) or stacked kings
    }
    // The side NOT to move must not be in check.
    let k = king_sq(pos, !pos.white_to_move).unwrap();
    attacked(&pos.board, k, pos.white_to_move)
}

// ---------------------------------------------------------------------------
// Slot plumbing: packed board records + slot <-> Position
// ---------------------------------------------------------------------------

type Rec = [u8; 10];

fn pack_board(board: &[u8; BOARD_LEN]) -> Rec {
    let mut r = [0u8; 10];
    for j in 0..10 {
        r[j] = board[2 * j] | (board[2 * j + 1] << 4);
    }
    r
}

/// Rebuild the position of a slot from its placement record. Slot layout:
/// `rank*8 + castle*2 + black_to_move`.
fn slot_position(rec: &Rec, slot: u64) -> Position {
    let mut b = [EMPTY; BOARD_LEN];
    for j in 0..10 {
        b[2 * j] = rec[j] & 0xF;
        b[2 * j + 1] = rec[j] >> 4;
    }
    Position {
        board: b,
        white_to_move: slot & 1 == 0,
        castling: ((slot >> 1) & 3) as u8,
        halfmove_clock: 0,
        fullmove_number: 1,
    }
}

// ---------------------------------------------------------------------------
// Child edges
// ---------------------------------------------------------------------------

/// One edge out of a node: either another slot of the same class (its value
/// evolves during iteration) or a constant, already-solved dependency value.
#[derive(Clone, Copy)]
enum Child {
    Same(usize),
    Const(u8),
}

const EDGE_SAME: u64 = 1 << 63;

/// Slots at or above this stream moves every sweep instead of caching edges.
const DEFAULT_EDGE_CACHE_LIMIT: u64 = 1 << 24;

/// Slots at or above this stream moves every sweep instead of caching edges.
/// Overridable with `SOLVER_EDGE_CACHE_LIMIT` (used for measurement).
fn edge_cache_limit() -> u64 {
    static LIMIT: std::sync::OnceLock<u64> = std::sync::OnceLock::new();
    *LIMIT.get_or_init(|| {
        std::env::var("SOLVER_EDGE_CACHE_LIMIT")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(DEFAULT_EDGE_CACHE_LIMIT)
    })
}

fn const_child_value(deps: &HashMap<usize, Solved>, child_key: u64) -> u8 {
    let cls = codec::class_of_key(child_key);
    let sol = deps.get(&cls).unwrap_or_else(|| {
        panic!(
            "retro: child class {} not solved before its parent — DAG order broken",
            matclass::class_name(cls)
        )
    });
    let v = sol.vals[(child_key - codec::class_base(cls) * 8) as usize];
    assert!(v <= V_WIN, "retro: dependency value {v} is not settled");
    v
}

struct Graph<'a> {
    cached: bool,
    starts: Vec<u32>,
    edges: Vec<u64>,
    recs: &'a [Rec],
}

impl<'a> Graph<'a> {
    /// Invoke `f` on every child; `f` returns `false` to stop early.
    fn children(
        &self,
        slot: usize,
        base8: u64,
        n_slots: u64,
        deps: &HashMap<usize, Solved>,
        f: &mut dyn FnMut(Child) -> bool,
    ) {
        if self.cached {
            let (a, b) = (self.starts[slot] as usize, self.starts[slot + 1] as usize);
            for &e in &self.edges[a..b] {
                let c = if e & EDGE_SAME != 0 {
                    Child::Same((e & !EDGE_SAME) as usize)
                } else {
                    Child::Const(e as u8)
                };
                if !f(c) {
                    return;
                }
            }
        } else {
            let pos = slot_position(&self.recs[slot / 8], slot as u64);
            for m in legal_moves(&pos) {
                let ck = {
                    let mut c = pos;
                    c.make(m);
                    codec::encode(&c)
                };
                let c = if ck >= base8 && ck < base8 + n_slots {
                    Child::Same((ck - base8) as usize)
                } else {
                    Child::Const(const_child_value(deps, ck))
                };
                if !f(c) {
                    return;
                }
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Results and the solver front end
// ---------------------------------------------------------------------------

/// The solved value array of one material class.
///
/// Indexing: `vals[rank*8 + castle*2 + black_to_move]`. All values are **from
/// the side to move's point of view** (see module docs).
pub struct Solved {
    pub class: usize,
    pub vals: Vec<u8>,
    /// Distinct square placements of the class's material (codec count).
    pub placements: u64,
    /// Legal positions = wins + losses + draws (both sides to move).
    pub positions: u64,
    pub wins: u64,
    pub losses: u64,
    pub draws: u64,
    /// Index slots rejected (adjacent kings, wrong side in check, pawn on
    /// its promotion rank).
    pub illegal: u64,
    /// Full fixed-point sweeps performed (including the confirming sweep).
    pub iters: u32,
    /// Wall time spent solving THIS class only (dependencies excluded).
    pub secs: f64,
}

impl Solved {
    pub fn value_by_key(&self, key: u64) -> Option<u8> {
        let base8 = codec::class_base(self.class) * 8;
        let n = self.placements * 8;
        if key < base8 || key >= base8 + n {
            return None;
        }
        Some(self.vals[(key - base8) as usize])
    }
}

/// Solves one target class together with its full downward closure.
pub struct Solver {
    solved: HashMap<usize, Solved>,
}

impl Solver {
    /// Solve `target` bottom-up: every class reachable by captures or
    /// promotions, in topological order, single-threaded throughout.
    pub fn solve(target: usize) -> Solver {
        let t0 = Instant::now();
        let mut closure = std::collections::BTreeSet::new();
        let mut stack = vec![target];
        while let Some(c) = stack.pop() {
            if closure.insert(c) {
                stack.extend(matclass::successors(c));
            }
        }
        let mut order: Vec<usize> = closure.into_iter().collect();
        // Bottom-up topological order. Along every DAG edge the successor is
        // either strictly lighter (a capture) or equally heavy with strictly
        // more non-pawn material (a promotion trades P for N/B/R/Q). Sorting
        // ascending by piece count and DESCENDING by non-pawn count therefore
        // puts every dependency before its dependent.
        order.sort_by(|&a, &b| {
            let (pa, qa) = (matclass::piece_count(a), matclass::nonpawn_count(a));
            let (pb, qb) = (matclass::piece_count(b), matclass::nonpawn_count(b));
            (pa, qb).cmp(&(pb, qa))
        });
        eprintln!(
            "[retro] solving {} + {} dependencies",
            matclass::class_name(target),
            order.len() - 1
        );
        let mut solved = HashMap::new();
        for c in order {
            let s = solve_class(c, &solved);
            solved.insert(c, s);
        }
        eprintln!("[retro] total wall incl. dependencies: {:.3}s", t0.elapsed().as_secs_f64());
        Solver { solved }
    }

    pub fn get(&self, class: usize) -> &Solved {
        self.solved.get(&class).unwrap_or_else(|| {
            panic!("class {} not solved", matclass::class_name(class))
        })
    }

    /// Value of an arbitrary position, provided its class was solved.
    /// Returns `Some(V_ILLEGAL)` for illegal placements.
    pub fn value_of(&self, pos: &Position) -> Option<u8> {
        let key = codec::try_encode(pos).ok()?;
        let cls = codec::class_of_key(key);
        self.solved.get(&cls)?.value_by_key(key)
    }
}

// ---------------------------------------------------------------------------
// The per-class solve
// ---------------------------------------------------------------------------

fn solve_class(class: usize, deps: &HashMap<usize, Solved>) -> Solved {
    let t0 = Instant::now();
    let name = matclass::class_name(class);
    let placements = codec::class_placements(class);
    let base8 = codec::class_base(class) * 8;
    let n = placements * 8;
    assert!(n <= (1u64 << 40), "class {name} absurdly large ({n} slots)");
    eprintln!("[retro] {name}: placements {placements}, slots {n}");

    let mut recs: Vec<Rec> = vec![[0u8; 10]; placements as usize];
    let mut vals: Vec<u8> = vec![V_UNKNOWN; n as usize];
    // positions_ct counts every non-illegal slot (terminals included);
    // term_* count checkmate / stalemate slots found in pass 0.
    let (mut positions_ct, mut illegal_ct, mut term_loss, mut term_draw) =
        (0u64, 0u64, 0u64, 0u64);

    // ---- pass 0: enumerate, reject illegal, assign terminals ---------------
    for rank in 0..placements {
        let proto = codec::decode(base8 + rank * 8); // castling 0, White to move
        recs[rank as usize] = pack_board(&proto.board);
        for slot in rank * 8..rank * 8 + 8 {
            let pos = slot_position(&recs[rank as usize], slot);
            if illegal_placement(&pos) {
                vals[slot as usize] = V_ILLEGAL;
                illegal_ct += 1;
                continue;
            }
            positions_ct += 1;
            match classify_terminal(&pos) {
                Some(v) => {
                    vals[slot as usize] = v;
                    if v == V_LOSS {
                        term_loss += 1;
                    } else {
                        term_draw += 1;
                    }
                }
                None => {}
            }
        }
    }

    // ---- move graph ---------------------------------------------------------
    let cached = n < edge_cache_limit();
    let graph = if cached {
        let mut starts: Vec<u32> = Vec::with_capacity(n as usize + 1);
        let mut edges: Vec<u64> = Vec::new();
        for slot in 0..n as usize {
            starts.push(edges.len() as u32);
            if vals[slot] != V_UNKNOWN {
                continue;
            }
            let pos = slot_position(&recs[slot / 8], slot as u64);
            for m in legal_moves(&pos) {
                let ck = {
                    let mut c = pos;
                    c.make(m);
                    codec::encode(&c)
                };
                debug_assert!(codec::try_decode(ck).is_ok(), "child key must decode");
                if ck >= base8 && ck < base8 + n {
                    edges.push(((ck - base8) as u64) | EDGE_SAME);
                } else {
                    edges.push(u64::from(const_child_value(deps, ck)));
                }
            }
        }
        starts.push(edges.len() as u32);
        Graph { cached: true, starts, edges, recs: &recs }
    } else {
        Graph { cached: false, starts: Vec::new(), edges: Vec::new(), recs: &recs }
    };

    // ---- the fixed point ----------------------------------------------------
    // WIN  if SOME child is LOSS; LOSS if EVERY child is WIN.
    // Values are monotone UNKNOWN -> {WIN, LOSS}; sweep direction alternates
    // so long propagation chains converge in about half the sweeps.
    let mut wins = 0u64;
    let mut swept_losses = 0u64;
    let mut iters: u32 = 0;
    loop {
        iters += 1;
        let mut changed = false;
        let forward = iters & 1 == 1;
        for i in 0..n {
            let slot = if forward { i as usize } else { (n - 1 - i) as usize };
            if vals[slot] != V_UNKNOWN {
                continue;
            }
            let mut win = false;
            let mut all_win = true;
            let mut any = false;
            graph.children(slot, base8, n, deps, &mut |ch| {
                any = true;
                let v = match ch {
                    Child::Same(s) => vals[s],
                    Child::Const(v) => v,
                };
                if v == V_LOSS {
                    win = true;
                    return false;
                }
                if v != V_WIN {
                    all_win = false; // DRAW or still-unknown child
                }
                true
            });
            if win {
                vals[slot] = V_WIN;
                wins += 1;
                changed = true;
            } else if any && all_win {
                vals[slot] = V_LOSS;
                swept_losses += 1;
                changed = true;
            }
        }
        if !changed {
            break;
        }
    }
    let losses = term_loss + swept_losses;

    // Convergence leftover: neither WIN nor LOSS provable => DRAW.
    let mut residual_draws = 0u64;
    for slot in 0..n as usize {
        if vals[slot] == V_UNKNOWN {
            vals[slot] = V_DRAW;
            residual_draws += 1;
        }
    }
    let draws = term_draw + residual_draws;
    assert_eq!(wins + losses + draws, positions_ct, "{name}: value accounting");

    let secs = t0.elapsed().as_secs_f64();
    if cached {
        audit(&graph, &vals, base8, n, deps);
    }
    // Per-side-to-move breakdown (slot parity: even = White to move).
    let (mut ww, mut bw) = (0u64, 0u64);
    let (mut wl, mut bl) = (0u64, 0u64);
    let (mut wd, mut bdraw) = (0u64, 0u64);
    for slot in 0..n as usize {
        let v = vals[slot];
        let black_stm = slot & 1 == 1;
        match v {
            V_WIN => { if black_stm { bw += 1 } else { ww += 1 } }
            V_LOSS => { if black_stm { bl += 1 } else { wl += 1 } }
            V_DRAW => { if black_stm { bdraw += 1 } else { wd += 1 } }
            _ => {}
        }
    }
    eprintln!(
        "[retro] {name}: positions {positions_ct} win {wins} loss {losses} draw {draws} illegal {illegal_ct} iters {iters} {:.3}s{}",
        secs,
        if cached { "" } else { " (streamed)" }
    );
    eprintln!(
        "[retro] {name}: White-to-move: win {ww} loss {wl} draw {wd} | Black-to-move: win {bw} loss {bl} draw {bdraw}"
    );
    Solved {
        class,
        vals,
        placements,
        positions: positions_ct,
        wins,
        losses,
        draws,
        illegal: illegal_ct,
        iters,
        secs,
    }
}

/// Post-solve consistency audit over the whole class graph (cached mode):
/// WIN nodes really have a LOSS child, LOSS nodes really have only WIN
/// children, DRAW nodes have no LOSS child and not all-WIN children. This
/// checks the fixed point against the graph itself, independent of how the
/// values were derived.
fn audit(graph: &Graph, vals: &[u8], base8: u64, n: u64, deps: &HashMap<usize, Solved>) {
    for slot in 0..n as usize {
        let v = vals[slot];
        assert!(v != V_UNKNOWN, "audit: slot {slot} left unresolved");
        if v == V_ILLEGAL {
            continue;
        }
        let mut any = false;
        let mut saw_loss = false;
        let mut all_win = true;
        graph.children(slot, base8, n, deps, &mut |ch| {
            any = true;
            let cv = match ch {
                Child::Same(s) => vals[s],
                Child::Const(cv) => cv,
            };
            assert!(cv <= V_WIN, "audit: bad child value {cv}");
            if cv == V_LOSS {
                saw_loss = true;
            }
            if cv != V_WIN {
                all_win = false;
            }
            true
        });
        let fen = codec::decode(base8 + slot as u64).to_fen();
        match v {
            V_WIN => assert!(saw_loss, "audit: WIN without LOSS child: {fen}"),
            // `all_win` is trivially true for terminal mates (no children).
            V_LOSS => assert!(all_win, "audit: LOSS with a non-WIN child: {fen}"),
            _ => assert!(
                !saw_loss && !(any && all_win),
                "audit: DRAW node contradicts the rules: {fen}"
            ),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::movegen::{in_check, legal_moves};

    #[test]
    fn known_mate_is_a_terminal_loss_for_the_side_to_move() {
        // Verified against Fairy-Stockfish: Black (to move) is checkmated.
        let pos = Position::from_fen("1kR1/4/3N/2BP/K3 b - - 9 5").unwrap();
        assert!(in_check(&pos));
        assert!(legal_moves(&pos).is_empty());
        assert_eq!(classify_terminal(&pos), Some(V_LOSS));
    }

    #[test]
    fn known_stalemate_is_a_terminal_draw() {
        // Verified against Fairy-Stockfish: Black (to move) is stalemated.
        let pos = Position::from_fen("k3/3N/1K2/4/4 b - - 0 1").unwrap();
        assert!(!in_check(&pos));
        assert!(legal_moves(&pos).is_empty());
        assert_eq!(classify_terminal(&pos), Some(V_DRAW));
    }

    #[test]
    fn normal_positions_are_not_terminals() {
        let pos = Position::from_fen("k3/4/4/4/K3 w - - 0 1").unwrap();
        assert_eq!(classify_terminal(&pos), None);
    }

    #[test]
    fn illegal_placements_are_detected() {
        // Adjacent kings (a5/a4).
        let pos = Position::from_fen("k3/K3/4/4/4 w - - 0 1").unwrap();
        assert!(illegal_placement(&pos));
        // Diagonally adjacent kings (a1/b2).
        let pos = Position::from_fen("4/4/4/1k2/K3 w - - 0 1").unwrap();
        assert!(illegal_placement(&pos));
        // Kings two apart, fine.
        let pos = Position::from_fen("4/k3/4/4/K3 w - - 0 1").unwrap();
        assert!(!illegal_placement(&pos));
        // Side not to move in check: White just moved?? construct: White to
        // move but BLACK in check -> illegal.
        let pos = Position::from_fen("kR2/4/4/4/K3 w - - 0 1").unwrap();
        assert!(illegal_placement(&pos));
        // Same geometry, Black to move: perfectly legal.
        let pos = Position::from_fen("kR2/4/4/4/K3 b - - 0 1").unwrap();
        assert!(!illegal_placement(&pos));
        // Pawn on its promotion rank (unreachable, unsafe for movegen).
        let pos = Position::from_fen("P3/k3/4/4/K3 w - - 0 1").unwrap();
        assert!(illegal_placement(&pos));
        let pos = Position::from_fen("4/4/4/4/pK1R w - - 0 1").unwrap();
        assert!(illegal_placement(&pos));
        // Kings stacked on the same file two apart is fine.
        let pos = Position::from_fen("k3/4/4/4/K2R w - - 0 1").unwrap();
        assert!(!illegal_placement(&pos));
    }
}
