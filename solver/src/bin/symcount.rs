//! How much does colour symmetry actually save on the *reachable* set?
//!
//! The answer is exactly 2x — the reachable set is closed under `mirror` (see
//! `solver::symmetry`) and no position is its own mirror, so it is a disjoint
//! union of mirror pairs. But a store truncated at ply N cannot show that: the
//! newest ply's mirror partners have the opposite side to move and so lie one
//! ply beyond the cut. This measures how much of the 2x is visible in a given
//! store, which is the number that matters when sizing a run that has not
//! finished.
//!
//! ```text
//! saving = 1 / (1 - paired/2)     paired = fraction whose mirror is present
//!                                 paired = 1 gives the assumed 2x, 0 gives none
//! ```
//!
//! Method: sample keys pseudo-randomly by hash, mirror them, and test membership
//! with one streaming merge per bucket. Two passes over the store, no sort, no
//! temp files. At the default sample size the standard error on `paired` is
//! about 0.04 percentage points — far tighter than the decision needs.
//!
//! Usage: `symcount <store_dir> [samples]`

use solver::keystream::KeyReader;
use solver::{codec, symmetry};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Mutex;
use std::time::Instant;

const BUCKETS: usize = 256;
const DEFAULT_SAMPLES: u64 = 2_000_000;

/// splitmix64 — decorrelates the sample from any structure in the key layout,
/// which a "take every Nth" stride would not.
fn mix(mut z: u64) -> u64 {
    z = z.wrapping_add(0x9E3779B97F4A7C15);
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58476D1CE4E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D049BB133111EB);
    z ^ (z >> 31)
}

/// Samples one source, mirrors each sample, and counts how many mirrors are
/// present in `visited`. Returns (keys scanned, sampled, mirrors found).
fn measure(dir: &Path, want: u64, from_frontier: bool) -> std::io::Result<(u64, u64, u64)> {
    let nthreads = (std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(2)
        / 2)
    .max(1);

    let total: u64 = std::fs::read_to_string(dir.join("cum.txt"))
        .expect("store needs cum.txt")
        .trim()
        .parse()
        .expect("cum.txt should hold the key count");
    // keep a key when mix(key) < threshold, i.e. with probability want/total
    let threshold = if want >= total {
        u64::MAX
    } else {
        ((want as f64 / total as f64) * u64::MAX as f64) as u64
    };
    let shift = (64 - codec::key_space().leading_zeros()).saturating_sub(8);
    let vpath = |b: usize| dir.join(format!("visited_{b:03}.keys"));
    // Sampling the newest ply instead of the whole set shows whether the
    // pairing rate is still climbing with depth, which is what decides how the
    // saving extrapolates to the plies not yet enumerated.
    let spath = |b: usize| {
        if from_frontier {
            dir.join(format!("frontier_{b:03}.keys"))
        } else {
            vpath(b)
        }
    };
    // ---- pass 1: sample keys, mirror them, bucket the mirrors --------------
    let t0 = Instant::now();
    let cursor = AtomicUsize::new(0);
    let seen = AtomicUsize::new(0);
    let mirrors: Mutex<Vec<Vec<u64>>> = Mutex::new(vec![Vec::new(); BUCKETS]);
    std::thread::scope(|s| -> std::io::Result<()> {
        let mut hs = Vec::new();
        for _ in 0..nthreads {
            let (cursor, seen, mirrors, spath) = (&cursor, &seen, &mirrors, &spath);
            hs.push(s.spawn(move || -> std::io::Result<()> {
                let mut local: Vec<Vec<u64>> = vec![Vec::new(); BUCKETS];
                let mut n = 0usize;
                loop {
                    let b = cursor.fetch_add(1, Ordering::Relaxed);
                    if b >= BUCKETS {
                        break;
                    }
                    let mut r = match KeyReader::open(&spath(b))? {
                        Some(r) => r,
                        None => continue,
                    };
                    while let Some(k) = r.cur {
                        n += 1;
                        if mix(k) < threshold {
                            let m = symmetry::mirror_key(k);
                            local[((m >> shift) as usize).min(BUCKETS - 1)].push(m);
                        }
                        r.advance()?;
                    }
                }
                seen.fetch_add(n, Ordering::Relaxed);
                let mut g = mirrors.lock().unwrap();
                for b in 0..BUCKETS {
                    g[b].append(&mut local[b]);
                }
                Ok(())
            }));
        }
        for h in hs {
            h.join().expect("sampler panicked")?;
        }
        Ok(())
    })?;

    let mut mirrors = mirrors.into_inner().unwrap();
    let sampled: usize = mirrors.iter().map(|v| v.len()).sum();
    for v in mirrors.iter_mut() {
        v.sort_unstable();
    }
    let _ = t0;

    // ---- pass 2: is each mirror present? one linear merge per bucket -------
    let cursor2 = AtomicUsize::new(0);
    let hits = AtomicUsize::new(0);
    let mirrors = &mirrors;
    std::thread::scope(|s| -> std::io::Result<()> {
        let mut hs = Vec::new();
        for _ in 0..nthreads {
            let (cursor2, hits, vpath) = (&cursor2, &hits, &vpath);
            hs.push(s.spawn(move || -> std::io::Result<()> {
                let mut found = 0usize;
                loop {
                    let b = cursor2.fetch_add(1, Ordering::Relaxed);
                    if b >= BUCKETS {
                        break;
                    }
                    let want = &mirrors[b];
                    if want.is_empty() {
                        continue;
                    }
                    let mut r = match KeyReader::open(&vpath(b))? {
                        Some(r) => r,
                        None => continue,
                    };
                    let mut i = 0usize;
                    while i < want.len() {
                        match r.cur {
                            None => break,
                            Some(k) => {
                                if k < want[i] {
                                    r.advance()?;
                                } else if k == want[i] {
                                    found += 1;
                                    i += 1; // duplicates in `want` each count
                                } else {
                                    i += 1;
                                }
                            }
                        }
                    }
                }
                hits.fetch_add(found, Ordering::Relaxed);
                Ok(())
            }));
        }
        for h in hs {
            h.join().expect("prober panicked")?;
        }
        Ok(())
    })?;

    let hits = hits.load(Ordering::Relaxed);
    Ok((seen.load(Ordering::Relaxed) as u64, sampled as u64, hits as u64))
}

fn main() -> std::io::Result<()> {
    let mut args = std::env::args().skip(1);
    let dir = PathBuf::from(args.next().expect("usage: symcount <store_dir> [samples]"));
    let want: u64 = args
        .next()
        .and_then(|s| s.parse().ok())
        .unwrap_or(DEFAULT_SAMPLES);
    let t0 = Instant::now();

    // The raw rate over `visited` badly understates the truth, because half the
    // set is the newest frontier and *its* mirrors have the opposite side to
    // move -- so they first appear one ply deeper, which has not been enumerated
    // yet. Splitting the set at the frontier separates that boundary artefact
    // from the interior rate, which is the one that extrapolates.
    let (n_all, s_all, h_all) = measure(&dir, want, false)?;
    let (n_fro, s_fro, h_fro) = measure(&dir, want, true)?;

    let rate_all = h_all as f64 / s_all as f64;
    let rate_fro = h_fro as f64 / s_fro as f64;
    // interior = visited \ frontier, recovered by subtracting estimated totals
    let est_all = h_all as f64 / (s_all as f64 / n_all as f64);
    let est_fro = h_fro as f64 / (s_fro as f64 / n_fro as f64);
    let n_int = n_all - n_fro;
    let rate_int = ((est_all - est_fro) / n_int as f64).clamp(0.0, 1.0);

    let saving = |p: f64| 1.0 / (1.0 - p / 2.0);
    println!("positions whose mirror is also in the store
");
    println!("  whole store  ({n_all:>13} keys) : {:>6.2}%   -> {:.3}x", 100.0 * rate_all, saving(rate_all));
    println!("  newest ply   ({n_fro:>13} keys) : {:>6.2}%   -> {:.3}x", 100.0 * rate_fro, saving(rate_fro));
    println!("  interior     ({n_int:>13} keys) : {:>6.2}%   -> {:.3}x", 100.0 * rate_int, saving(rate_int));
    println!("
The true rate is 100% (the reachable set is closed under mirror), so");
    println!("every shortfall here is truncation: a position at ply d has its mirror");
    println!("by ply d+5, and anything past the cut cannot be seen. The newest ply is");
    println!("worst hit -- its partners are all beyond the edge.");
    println!("
elapsed {:.1}s", t0.elapsed().as_secs_f64());
    Ok(())
}
