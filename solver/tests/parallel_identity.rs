//! Expansion runs on many threads; consolidation does not. This asserts that
//! the threading is invisible in the result.
//!
//! Two claims, and both are needed:
//!
//!   1. **Identity** — a 1-thread store and an 8-thread store are byte-identical,
//!      file for file. Thread scheduling decides only how the intermediate run
//!      files are cut up, never which keys end up in `visited` or `frontier`.
//!   2. **Correctness** — both agree with the independently established ply
//!      ladder. Identity alone would be satisfied by two runs that are wrong in
//!      the same way, so the ladder is what pins the answer down.
//!
//! The buffer is deliberately squeezed to 1 Mkey so eight threads each spill
//! many times and several threads write run files for the same bucket — the
//! case where a naming collision or a lost run would actually show up.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::Command;

/// Cumulative reachable positions per ply, from `docs/HANDOVER.md`. Verified
/// against a from-scratch run and, to ply 12, against the pre-compression
/// enumerator.
const LADDER: [(u32, u64); 10] = [
    (1, 10),
    (2, 79),
    (3, 448),
    (4, 2_379),
    (5, 11_872),
    (6, 56_141),
    (7, 246_709),
    (8, 1_021_173),
    (9, 3_898_949),
    (10, 13_634_481),
];

fn store_dir(tag: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("enum_identity_{tag}"));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).expect("create store dir");
    d
}

/// Runs the enumerator to `max_ply`, returning ply -> cumulative from stdout.
fn run(dir: &Path, threads: usize, max_ply: u32) -> BTreeMap<u32, u64> {
    let out = Command::new(env!("CARGO_BIN_EXE_enumerate"))
        .arg(dir)
        .arg(max_ply.to_string())
        .env("ENUM_THREADS", threads.to_string())
        .env("ENUM_BUF_MK", "1")
        .output()
        .expect("run enumerate");
    assert!(
        out.status.success(),
        "enumerate failed ({threads} threads): {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let text = String::from_utf8_lossy(&out.stdout);
    let mut got = BTreeMap::new();
    for line in text.lines() {
        let f: Vec<&str> = line.split_whitespace().collect();
        // "<ply> <new> <cumulative> <sec> [expand ...]"
        if f.len() >= 4 {
            if let (Ok(p), Ok(_), Ok(c)) =
                (f[0].parse::<u32>(), f[1].parse::<u64>(), f[2].parse::<u64>())
            {
                got.insert(p, c);
            }
        }
    }
    assert!(!got.is_empty(), "no ply lines parsed from:\n{text}");
    got
}

/// Every key file in a store, keyed by file name.
fn key_files(dir: &Path) -> BTreeMap<String, Vec<u8>> {
    let mut m = BTreeMap::new();
    for e in std::fs::read_dir(dir).expect("read store dir") {
        let e = e.expect("dir entry");
        let name = e.file_name().to_string_lossy().into_owned();
        if name.ends_with(".keys") {
            m.insert(name, std::fs::read(e.path()).expect("read key file"));
        }
    }
    m
}

#[test]
fn parallel_expansion_matches_serial_byte_for_byte_and_the_ladder() {
    const MAX_PLY: u32 = 10;

    let serial_dir = store_dir("serial");
    let par_dir = store_dir("par8");
    let serial = run(&serial_dir, 1, MAX_PLY);
    let parallel = run(&par_dir, 8, MAX_PLY);

    // 2. correctness: both runs reproduce the established ladder
    for (ply, want) in LADDER {
        if ply > MAX_PLY {
            continue;
        }
        assert_eq!(
            serial.get(&ply),
            Some(&want),
            "serial cumulative wrong at ply {ply}"
        );
        assert_eq!(
            parallel.get(&ply),
            Some(&want),
            "parallel cumulative wrong at ply {ply}"
        );
    }

    // 1. identity: same files, same bytes
    let a = key_files(&serial_dir);
    let b = key_files(&par_dir);
    let names_a: Vec<&String> = a.keys().collect();
    let names_b: Vec<&String> = b.keys().collect();
    assert_eq!(
        names_a, names_b,
        "serial and parallel stores hold different key files"
    );
    assert!(!a.is_empty(), "store has no key files");
    for (name, bytes_a) in &a {
        let bytes_b = &b[name];
        assert_eq!(
            bytes_a.len(),
            bytes_b.len(),
            "{name}: length differs (serial {} vs parallel {})",
            bytes_a.len(),
            bytes_b.len()
        );
        assert!(bytes_a == bytes_b, "{name}: contents differ");
    }

    // no run files or partial-commit markers should survive a clean finish
    for stray in ["consol.txt", "swap.txt", "consol.total"] {
        assert!(
            !par_dir.join(stray).exists(),
            "{stray} left behind after a clean finish"
        );
    }
    let leftover_runs = std::fs::read_dir(&par_dir)
        .unwrap()
        .filter_map(|e| e.ok())
        .filter(|e| e.file_name().to_string_lossy().starts_with("run_"))
        .count();
    assert_eq!(leftover_runs, 0, "run files left behind after consolidation");

    let _ = std::fs::remove_dir_all(&serial_dir);
    let _ = std::fs::remove_dir_all(&par_dir);
}

/// Thread count must not change the answer at any count, not just at 8.
#[test]
fn thread_count_does_not_change_the_result() {
    const MAX_PLY: u32 = 8;
    let base_dir = store_dir("t1");
    let base = run(&base_dir, 1, MAX_PLY);
    let base_files = key_files(&base_dir);

    for threads in [2usize, 3, 5, 16] {
        let d = store_dir(&format!("t{threads}"));
        let got = run(&d, threads, MAX_PLY);
        assert_eq!(got, base, "ply ladder differs at {threads} threads");
        assert_eq!(
            key_files(&d),
            base_files,
            "store bytes differ at {threads} threads"
        );
        let _ = std::fs::remove_dir_all(&d);
    }
    let _ = std::fs::remove_dir_all(&base_dir);
}
