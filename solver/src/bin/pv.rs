//! Debug helper: solve KPvKP and print the forced-mate line the solver's
//! values imply from a given FEN. Throwaway verification tool.
use std::io::Write;

use solver::codec;
use solver::matclass;
use solver::movegen::legal_moves;
use solver::retro::{value_name, Solver, V_LOSS, V_WIN};
use solver::Position;

fn main() {
    let fen = std::env::args().nth(1).expect("fen arg");
    let solver = Solver::solve(matclass::parse_class_name("KPvKP").unwrap());
    let mut pos = Position::from_fen(&fen).unwrap();
    let mut plies = 0;
    loop {
        let v = solver.value_of(&pos).expect("position in solved set");
        println!("ply {plies:>2}: [{}] {}", value_name(v), pos.to_fen());
        let _ = std::io::stdout().flush();
        let moves = legal_moves(&pos);
        if moves.is_empty() {
            println!("terminal");
            break;
        }
        if v == V_WIN {
            // find a child with LOSS
            let mut found = None;
            for m in &moves {
                let mut c = pos;
                c.make(*m);
                if solver.value_of(&c) == Some(V_LOSS) {
                    found = Some(*m);
                    break;
                }
            }
            match found {
                Some(m) => {
                    println!("          winning move: {}", m.uci());
                    pos.make(m);
                }
                None => {
                    println!("NO LOSS CHILD FOUND FOR WIN -- BUG");
                    break;
                }
            }
        } else if v == V_LOSS {
            // opponent to move; they should have all-WIN children; pick any
            let m = &moves[0];
            println!("          (losing side plays) {}", m.uci());
            pos.make(*m);
        } else {
            println!("draw reached");
            break;
        }
        plies += 1;
        if plies > 80 {
            println!("line too long, stopping");
            break;
        }
    }
}
