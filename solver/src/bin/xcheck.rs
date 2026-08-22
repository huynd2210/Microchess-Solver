//! `xcheck <CLASS> <N> <DEPTH> <variant.ini>` — independent cross-check of the
//! solved class against Fairy-Stockfish.
//!
//! Samples N deterministic random legal slots of the solved class (only slots
//! whose castling rights are geometrically consistent, so the FEN means the
//! same thing to both engines), queries Fairy-Stockfish with a fixed-depth
//! search per position, and compares:
//!
//! ```text
//! solver WIN  <=> FSF reports `score mate +n` for the side to move
//! solver LOSS <=> FSF reports `score mate -n`
//! solver DRAW <=> FSF reports no mate score at the given depth
//! ```
//!
//! A deep-enough fixed-depth search proves the WIN/LOSS directions outright;
//! DRAW is confirmed negatively (no forced mate found either way). Prints one
//! line per disagreement and exits non-zero on any.

use std::io::{BufRead, BufReader, Write};
use std::process::{Command, Stdio};

use solver::codec;
use solver::matclass;
use solver::retro::{self, Solver, V_DRAW, V_LOSS, V_WIN};
use solver::tt::splitmix64;

struct FsF {
    child: std::process::Child,
    stdin: std::process::ChildStdin,
    stdout: BufReader<std::process::ChildStdout>,
}

impl FsF {
    fn spawn(exe: &str, variant_ini: &str) -> FsF {
        let mut child = Command::new(exe)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .expect("spawn fairy-stockfish");
        let mut stdin = child.stdin.take().unwrap();
        let stdout = BufReader::new(child.stdout.take().unwrap());
        writeln!(stdin, "uci").unwrap();
        let mut f = FsF { child, stdin, stdout };
        f.until("uciok").expect("engine did not answer uciok");
        // VariantPath must be set after the handshake, and the variant
        // selected explicitly, or the engine silently stays on 8x8 chess.
        writeln!(f.stdin, "setoption name VariantPath value {variant_ini}").unwrap();
        writeln!(f.stdin, "setoption name UCI_Variant value microchess").unwrap();
        f.send("isready");
        f.until("readyok").expect("engine not ready");
        f
    }

    fn send(&mut self, cmd: &str) {
        writeln!(self.stdin, "{cmd}").unwrap();
    }

    fn until(&mut self, token: &str) -> Result<(), String> {
        loop {
            let mut line = String::new();
            let n = self.stdout.read_line(&mut line).map_err(|e| e.to_string())?;
            if n == 0 {
                return Err(format!("engine EOF before {token}"));
            }
            if line.contains(token) {
                return Ok(());
            }
        }
    }

    /// Last `score` reported before bestmove: `(mate_in_for_stm, cp)`.
    fn search(&mut self, fen: &str, depth: u32) -> Result<(Option<i32>, i64), String> {
        self.send("ucinewgame");
        self.send("isready");
        self.until("readyok")?;
        self.send(&format!("position fen {fen}"));
        self.send(&format!("go depth {depth}"));
        let mut mate: Option<i32> = None;
        let mut cp: i64 = 0;
        loop {
            let mut line = String::new();
            let n = self.stdout.read_line(&mut line).map_err(|e| e.to_string())?;
            if n == 0 {
                return Err("engine EOF during search".into());
            }
            if let Some(idx) = line.find(" score ") {
                let rest = &line[idx + 7..];
                if let Some(v) = rest.strip_prefix("mate ") {
                    mate = v.split_whitespace().next().and_then(|t| t.parse().ok());
                } else if let Some(v) = rest.strip_prefix("cp ") {
                    cp = v.split_whitespace().next().and_then(|t| t.parse().ok()).unwrap_or(0);
                }
            }
            if line.starts_with("bestmove") {
                break;
            }
            if line.contains("Game over") || line.contains("game over") {
                break;
            }
        }
        Ok((mate, cp))
    }
}

impl Drop for FsF {
    fn drop(&mut self) {
        let _ = writeln!(self.stdin, "quit");
        let _ = self.child.wait();
    }
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.len() != 4 {
        eprintln!("usage: xcheck <CLASS> <N> <DEPTH> <path/to/microchess.ini>");
        std::process::exit(2);
    }
    let class = matclass::parse_class_name(&args[0]).expect("bad class");
    let n: usize = args[1].parse().expect("bad N");
    let depth: u32 = args[2].parse().expect("bad depth");
    let ini = args[3].clone();

    let solver = Solver::solve(class);
    let s = solver.get(class);
    let base8 = codec::class_base(class) * 8;
    let slots = s.placements * 8;

    // Geometrically-consistent castling rights only: FSF and we must agree on
    // what the FEN says. (Rights without their pieces are inert for us too.)
    let consistent = |slot: usize| -> bool {
        let castle = ((slot >> 1) & 3) as u8;
        let pos = codec::decode(base8 + slot as u64);
        if castle & 1 != 0 && !(pos.board[0] == solver::WK && pos.board[3] == solver::WR) {
            return false;
        }
        if castle & 2 != 0 && !(pos.board[16] == solver::BK && pos.board[19] == solver::BR) {
            return false;
        }
        true
    };

    let mut z = 0xDEAD_BEEF_CAFE_1234u64;
    let mut next = move || {
        z = splitmix64(z);
        z
    };

    let mut eng = FsF::spawn(&engine_exe(), &ini);
    let (mut agree, mut disagree, mut checked) = (0u32, 0u32, 0u32);
    let (mut w_ok, mut l_ok, mut d_ok) = (0u32, 0u32, 0u32);
    let mut attempts = 0u64;
    while checked < n as u32 {
        attempts += 1;
        assert!(attempts <= 1000 + 100 * n as u64, "xcheck: cannot sample enough slots");
        let slot = (next() % slots) as usize;
        let v = s.vals[slot];
        if v == V_LOSS || v == V_DRAW || v == V_WIN {
            if !consistent(slot) {
                continue;
            }
        } else {
            continue; // ILLEGAL
        }
        let fen = codec::decode(base8 + slot as u64).to_fen();
        let (mate, _cp) = eng.search(&fen, depth).expect("engine search failed");
        // FSF reports `mate 0` for positions that are already checkmate
        // (verified by hand on two samples: stm is mated with no moves).
        let fsf_v = match mate {
            Some(m) if m > 0 => V_WIN,
            Some(m) if m <= 0 => V_LOSS,
            _ => V_DRAW,
        };
        checked += 1;
        match (v, fsf_v) {
            (V_WIN, V_WIN) => w_ok += 1,
            (V_LOSS, V_LOSS) => l_ok += 1,
            (V_DRAW, V_DRAW) => d_ok += 1,
            _ => {
                disagree += 1;
                println!(
                    "MISMATCH {} solver {} fsf {} (mate {:?})",
                    fen,
                    retro::value_name(v),
                    retro::value_name(fsf_v),
                    mate
                );
            }
        }
    }
    let _ = std::io::stdout().flush();
    println!(
        "xcheck {} n {checked} agree {} disagree {disagree} (win-ok {w_ok} loss-ok {l_ok} draw-ok {d_ok})",
        matclass::class_name(class),
        agree = w_ok + l_ok + d_ok
    );
    if disagree > 0 {
        std::process::exit(1);
    }
}

fn engine_exe() -> String {
    // Expect fairy-stockfish.exe next to the workspace engine/ dir; fall back
    // to PATH.
    for cand in ["../engine/fairy-stockfish.exe", "../../engine/fairy-stockfish.exe"] {
        if std::path::Path::new(cand).exists() {
            return cand.to_string();
        }
    }
    "fairy-stockfish".to_string()
}
