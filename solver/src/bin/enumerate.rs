//! External-memory enumeration of the reachable state space.
//!
//! This is the first half of the solve, and it settles the one quantity the
//! whole feasibility estimate rests on: how many positions are actually
//! reachable. Everything so far has extrapolated it from ply 12.
//!
//! # Design
//!
//! Keys are partitioned into `BUCKETS` by their high bits. Per ply:
//!   1. **Expand** — read each frontier bucket, decode keys to positions,
//!      generate children, encode, append to a per-bucket RAM buffer. When the
//!      total buffered exceeds `BUF_KEYS`, sort + dedupe + spill each bucket to
//!      a run file. Deduping before spilling is what keeps the intermediate
//!      volume bounded: at saturation a position is reached ~8.9 ways.
//!   2. **Consolidate** — per bucket, merge its run files, dedupe, then stream
//!      against that bucket's `visited` file. Keys not already present become
//!      the next frontier and are merged into `visited`.
//!
//! Only one bucket is rewritten at a time, so peak disk is
//! `visited + one bucket`, not `2 x visited`.
//!
//! Everything is sequential I/O and every pass is restartable: `visited` and
//! `frontier` on disk after ply N are a complete checkpoint.

use solver::{codec, movegen};
use std::collections::HashSet;
use std::fs::{self, File, OpenOptions};
use std::io::{BufReader, BufWriter, Read, Write};
use std::path::{Path, PathBuf};
use std::time::Instant;

const BUCKETS: usize = 256;
/// Keys buffered in RAM before a sort+dedupe+spill. 32 M keys = 256 MB.
const BUF_KEYS: usize = 32 << 20;

fn bucket_of(key: u64, shift: u32) -> usize {
    ((key >> shift) as usize).min(BUCKETS - 1)
}

fn write_keys(path: &Path, keys: &[u64]) -> std::io::Result<()> {
    let mut w = BufWriter::with_capacity(1 << 20, File::create(path)?);
    for k in keys {
        w.write_all(&k.to_le_bytes())?;
    }
    w.flush()
}

fn append_keys(path: &Path, keys: &[u64]) -> std::io::Result<()> {
    let f = OpenOptions::new().create(true).append(true).open(path)?;
    let mut w = BufWriter::with_capacity(1 << 20, f);
    for k in keys {
        w.write_all(&k.to_le_bytes())?;
    }
    w.flush()
}

fn read_keys(path: &Path) -> std::io::Result<Vec<u64>> {
    if !path.exists() {
        return Ok(Vec::new());
    }
    let mut r = BufReader::with_capacity(1 << 20, File::open(path)?);
    let mut buf = Vec::new();
    r.read_to_end(&mut buf)?;
    Ok(buf
        .chunks_exact(8)
        .map(|c| u64::from_le_bytes(c.try_into().unwrap()))
        .collect())
}

/// Sorted union of `a` (already sorted) and `b` (already sorted), plus the
/// elements of `b` that were absent from `a`.
fn merge_new(a: &[u64], b: &[u64]) -> (Vec<u64>, Vec<u64>) {
    let mut union = Vec::with_capacity(a.len() + b.len());
    let mut fresh = Vec::new();
    let (mut i, mut j) = (0usize, 0usize);
    while i < a.len() || j < b.len() {
        if j >= b.len() || (i < a.len() && a[i] < b[j]) {
            union.push(a[i]);
            i += 1;
        } else if i >= a.len() || b[j] < a[i] {
            union.push(b[j]);
            fresh.push(b[j]);
            j += 1;
        } else {
            union.push(a[i]);
            i += 1;
            j += 1;
        }
    }
    (union, fresh)
}

fn main() -> std::io::Result<()> {
    let mut args = std::env::args().skip(1);
    let dir = PathBuf::from(args.next().unwrap_or_else(|| "enum_store".into()));
    let max_ply: u32 = args
        .next()
        .and_then(|s| s.parse().ok())
        .unwrap_or(64);
    fs::create_dir_all(&dir)?;

    let shift = (64 - codec::key_space().leading_zeros()).saturating_sub(8);
    let vpath = |b: usize| dir.join(format!("visited_{b:03}.keys"));
    let fpath = |b: usize| dir.join(format!("frontier_{b:03}.keys"));
    let rpath = |b: usize, r: usize| dir.join(format!("run_{b:03}_{r:04}.keys"));

    // resume: cumulative count is the sum of visited bucket sizes
    let mut cumulative: u64 = (0..BUCKETS)
        .map(|b| {
            fs::metadata(vpath(b)).map(|m| m.len() / 8).unwrap_or(0)
        })
        .sum();
    let start_ply = fs::read_to_string(dir.join("ply.txt"))
        .ok()
        .and_then(|s| s.trim().parse::<u32>().ok())
        .unwrap_or(0);

    if start_ply == 0 {
        let root = codec::encode(&solver::startpos());
        write_keys(&vpath(bucket_of(root, shift)), &[root])?;
        write_keys(&fpath(bucket_of(root, shift)), &[root])?;
        cumulative = 1;
        println!("{:>4} {:>16} {:>18} {:>10}", "ply", "new", "cumulative", "sec");
        println!("{:>4} {:>16} {:>18} {:>10.1}", 0, 1, 1, 0.0);
    } else {
        println!("resuming after ply {start_ply}, cumulative {cumulative}");
    }

    let t0 = Instant::now();
    for ply in (start_ply + 1)..=max_ply {
        // ---- 1. expand -------------------------------------------------
        let mut bufs: Vec<Vec<u64>> = vec![Vec::new(); BUCKETS];
        let mut runs: Vec<usize> = vec![0; BUCKETS];
        let mut buffered = 0usize;
        let mut spill = |bufs: &mut Vec<Vec<u64>>, runs: &mut Vec<usize>| -> std::io::Result<()> {
            for b in 0..BUCKETS {
                if bufs[b].is_empty() {
                    continue;
                }
                bufs[b].sort_unstable();
                bufs[b].dedup();
                let r = runs[b];
                append_keys(&rpath(b, r), &bufs[b])?;
                runs[b] = r + 1;
                bufs[b].clear();
            }
            Ok(())
        };

        for b in 0..BUCKETS {
            let front = read_keys(&fpath(b))?;
            for key in front {
                let pos = codec::decode(key);
                for m in movegen::legal_moves(&pos).iter() {
                    let mut q = pos;
                    q.make(*m);
                    let ck = codec::encode(&q);
                    bufs[bucket_of(ck, shift)].push(ck);
                    buffered += 1;
                }
                if buffered >= BUF_KEYS {
                    spill(&mut bufs, &mut runs)?;
                    buffered = 0;
                }
            }
        }
        spill(&mut bufs, &mut runs)?;

        // ---- 2. consolidate, one bucket at a time -----------------------
        let mut new_total: u64 = 0;
        for b in 0..BUCKETS {
            if runs[b] == 0 {
                let _ = fs::remove_file(fpath(b));
                continue;
            }
            let mut cand: Vec<u64> = Vec::new();
            for r in 0..runs[b] {
                let p = rpath(b, r);
                cand.extend_from_slice(&read_keys(&p)?);
                let _ = fs::remove_file(p);
            }
            cand.sort_unstable();
            cand.dedup();
            let vis = read_keys(&vpath(b))?;
            let (union, fresh) = merge_new(&vis, &cand);
            write_keys(&vpath(b), &union)?;
            if fresh.is_empty() {
                let _ = fs::remove_file(fpath(b));
            } else {
                write_keys(&fpath(b), &fresh)?;
            }
            new_total += fresh.len() as u64;
        }

        cumulative += new_total;
        fs::write(dir.join("ply.txt"), ply.to_string())?;
        println!(
            "{:>4} {:>16} {:>18} {:>10.1}",
            ply,
            new_total,
            cumulative,
            t0.elapsed().as_secs_f64()
        );
        use std::io::Write as _;
        std::io::stdout().flush()?;
        if new_total == 0 {
            println!("frontier exhausted -- REACHABLE SET FULLY ENUMERATED");
            break;
        }
    }
    println!("total reachable positions: {cumulative}");
    let _ = HashSet::<u64>::new(); // keep the import honest if unused elsewhere
    Ok(())
}
