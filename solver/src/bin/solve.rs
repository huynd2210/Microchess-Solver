//! `solve <CLASS> [--dump N]` — solve one material class exactly, together
//! with every class it can capture or promote down into (see
//! `docs/REPETITION.md`; retrograde fixed point only, no forward search).
//!
//! stdout is a single summary line (plus `--dump` lines when asked):
//!
//! ```text
//! class <NAME> positions <n> win <w> loss <l> draw <d> illegal <i> iters <k> time <secs>
//! ```
//!
//! `positions` counts the legal slots of the class (both sides to move,
//! castling-right variants included); `illegal` counts rejected index slots.
//! WIN/LOSS are from **the side to move's** point of view. Progress, per-class
//! dependency timings and peak memory go to stderr.

use std::io::Write;
use std::time::Instant;

use solver::codec;
use solver::matclass;
use solver::retro::{self, Solver, V_ILLEGAL};
use solver::tt::splitmix64;

const USAGE: &str = "usage: solve <CLASS> [--dump N]\n  CLASS     material class, e.g. KvK KNvK KRvK KQvK KBNvK KRvKR\n  --dump N  print N random \"FEN = VALUE\" lines after the summary";

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let mut class_arg: Option<String> = None;
    let mut dump: Option<usize> = None;
    let mut it = args.iter();
    while let Some(a) = it.next() {
        match a.as_str() {
            "--dump" => {
                let n = it.next().unwrap_or_else(|| die("--dump needs N"));
                dump = Some(n.parse().unwrap_or_else(|_| die(format!("bad --dump N {n:?}").as_str())));
            }
            "-h" | "--help" => {
                println!("{USAGE}");
                return;
            }
            s => {
                if class_arg.is_some() {
                    die(format!("unexpected extra argument {s:?}\n{USAGE}").as_str());
                }
                class_arg = Some(s.to_string());
            }
        }
    }
    let name = class_arg.unwrap_or_else(|| die(USAGE));
    let class = match matclass::parse_class_name(&name) {
        Ok(c) => c,
        Err(e) => die(&format!("bad class {name:?}: {e}")),
    };

    let t0 = Instant::now();
    let solver = Solver::solve(class);
    let s = solver.get(class);

    // The required machine-readable line. `time` covers solving THIS class
    // (enumeration + fixed point); dependency-class time went to stderr.
    println!(
        "class {} positions {} win {} loss {} draw {} illegal {} iters {} time {:.3}",
        matclass::class_name(class),
        s.positions,
        s.wins,
        s.losses,
        s.draws,
        s.illegal,
        s.iters,
        s.secs
    );
    let _ = std::io::stdout().flush();

    if let Some(ndump) = dump {
        dump_lines(&solver, class, ndump);
    }

    eprintln!(
        "[solve] total wall {:.3}s, peak working set {:.1} MiB, single-threaded",
        t0.elapsed().as_secs_f64(),
        peak_working_set() as f64 / 1048576.0
    );
}

fn die(msg: &str) -> ! {
    eprintln!("error: {msg}");
    std::process::exit(2);
}

/// Print `n` uniformly random legal positions of the solved class as
/// `<FEN> = <WIN|LOSS|DRAW>` (value from the side to move's view).
/// Deterministic seed so runs are reproducible.
fn dump_lines(solver: &Solver, class: usize, n: usize) {
    let s = solver.get(class);
    let base8 = codec::class_base(class) * 8;
    let slots = s.placements * 8;
    assert!(slots > 0);
    let mut z = 0x243F_6A88_85A3_08D3u64;
    let mut next = move || {
        z = splitmix64(z);
        z
    };
    let mut printed = 0usize;
    let mut attempts = 0u64;
    while printed < n {
        attempts += 1;
        assert!(
            attempts <= 1000 + 1000 * n as u64,
            "dump: too few legal slots in class"
        );
        let slot = (next() % slots) as usize;
        let v = s.vals[slot];
        if v == V_ILLEGAL {
            continue;
        }
        let pos = codec::decode(base8 + slot as u64);
        println!("{} = {}", pos.to_fen(), retro::value_name(v));
        printed += 1;
    }
    let _ = std::io::stdout().flush();
}

// Peak memory via the Windows process counters (no external crates). Returns
// 0 when unavailable (non-Windows), in which case stderr simply omits the
// number's meaning rather than inventing one.
#[cfg(windows)]
fn peak_working_set() -> u64 {
    #[repr(C)]
    #[allow(non_snake_case)]
    struct ProcessMemoryCounters {
        cb: u32,
        PageFaultCount: u32,
        PeakWorkingSetSize: usize,
        WorkingSetSize: usize,
        QuotaPeakPagedPoolUsage: usize,
        QuotaPagedPoolUsage: usize,
        QuotaPeakNonPagedPoolUsage: usize,
        QuotaNonPagedPoolUsage: usize,
        PagefileUsage: usize,
        PeakPagefileUsage: usize,
    }
    #[link(name = "psapi")]
    extern "system" {
        fn GetProcessMemoryInfo(
            process: isize,
            counters: *mut ProcessMemoryCounters,
            cb: u32,
        ) -> i32;
        fn GetCurrentProcess() -> isize;
    }
    unsafe {
        let mut pmc: ProcessMemoryCounters = std::mem::zeroed();
        pmc.cb = std::mem::size_of::<ProcessMemoryCounters>() as u32;
        if GetProcessMemoryInfo(GetCurrentProcess(), &mut pmc, pmc.cb) != 0 {
            pmc.PeakWorkingSetSize as u64
        } else {
            0
        }
    }
}

#[cfg(not(windows))]
fn peak_working_set() -> u64 {
    0
}
