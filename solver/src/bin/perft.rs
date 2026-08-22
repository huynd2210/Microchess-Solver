//! Perft driver for the microchess rules engine.
//!
//! Usage (fixed CLI, see TASK-01):
//!   perft <depth>                 -> "perft <depth> = <nodes>"
//!   perft <depth> --divide        -> "<uci>: <nodes>" lines + "Nodes searched: <total>"
//!   perft <depth> --fen "<FEN>"   -> same as plain, from the given position

use solver::{divide, perft, Position, START_FEN};

fn main() {
    let args: Vec<String> = std::env::args().collect();

    let mut depth: Option<u32> = None;
    let mut fen: Option<String> = None;
    let mut do_divide = false;

    let mut i = 1;
    while i < args.len() {
        match args[i].as_str() {
            "--divide" => do_divide = true,
            "--fen" => {
                if let Some(f) = args.get(i + 1) {
                    fen = Some(f.clone());
                    i += 1;
                } else {
                    eprintln!("--fen requires an argument");
                    std::process::exit(2);
                }
            }
            other => {
                if other.starts_with("--fen=") {
                    fen = Some(other["--fen=".len()..].to_string());
                } else {
                    match other.parse::<u32>() {
                        Ok(d) => depth = Some(d),
                        Err(_) => {
                            eprintln!("unrecognised argument: {other:?}");
                            std::process::exit(2);
                        }
                    }
                }
            }
        }
        i += 1;
    }

    let depth = match depth {
        Some(d) => d,
        None => {
            eprintln!("usage: perft <depth> [--fen \"<FEN>\"] [--divide]");
            std::process::exit(2);
        }
    };

    let pos = match &fen {
        Some(f) => match Position::from_fen(f) {
            Ok(p) => p,
            Err(e) => {
                eprintln!("bad FEN {f:?}: {e}");
                std::process::exit(2);
            }
        },
        None => Position::from_fen(START_FEN).expect("start FEN must parse"),
    };

    if do_divide {
        let rows = divide(&pos, depth);
        let total: u64 = rows.iter().map(|(_, n)| n).sum();
        for (uci, n) in &rows {
            println!("{uci}: {n}");
        }
        println!("Nodes searched: {total}");
    } else {
        let nodes = perft(&pos, depth);
        println!("perft {depth} = {nodes}");
    }
}
