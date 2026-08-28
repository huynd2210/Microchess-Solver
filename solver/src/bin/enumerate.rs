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
//!   2. **Consolidate** — per bucket, k-way merge its run files, dedupe, then
//!      stream against that bucket's `visited` file. Keys not already present
//!      become the next frontier and are merged into `visited`.
//!
//! Only one bucket is rewritten at a time, so peak disk is
//! `visited + one bucket`, not `2 x visited`.
//!
//! # On-disk format
//!
//! Every key file is a **varint-delta stream**: keys ascending, each stored as
//! a LEB128 gap from its predecessor (the first is absolute). Measured at
//! 1.004 B/key on the dense top-class set against 6.0 for raw; the global set
//! is sparser but still far under the 8 B/key that capped the raw run near
//! ply 16. No pass materialises a bucket — reads, merges and writes are all
//! streaming, so RAM does not grow with bucket size.
//!
//! # Restartability
//!
//! Everything is sequential I/O and every pass is restartable. A ply commits in
//! this order: `consol.txt` (per-bucket progress) -> `swap.txt` (frontier swap
//! barrier) -> `ply.txt`. Each step is idempotent on replay, so an interrupted
//! run — including one killed by a full disk mid-write — resumes without
//! corruption and without redoing completed buckets. `visited` is never
//! truncated in place: it is rewritten to a sibling and renamed over.

use solver::keystream::{write_one, KeyReader, KeyWriter, KwayMerge};
use solver::{codec, movegen};
use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::time::Instant;

const BUCKETS: usize = 256;
/// Keys buffered in RAM before a sort+dedupe+spill, in units of 2^20 keys
/// (8 B each). Larger buffers dedupe a bigger slice of the child stream and
/// emit denser — hence cheaper — run files. Override with `ENUM_BUF_MK`.
const DEFAULT_BUF_MK: usize = 192;

fn bucket_of(key: u64, shift: u32) -> usize {
    ((key >> shift) as usize).min(BUCKETS - 1)
}

/// Streams `visited` against the merged candidates, writing the union to
/// `vis_out` and the previously-unseen keys to `fresh_out`. Returns the number
/// of fresh keys. Neither side is ever held in RAM.
fn merge_new(
    visited: Option<KeyReader>,
    mut cand: KwayMerge,
    vis_out: &Path,
    fresh_out: &Path,
) -> io::Result<u64> {
    let mut vis = visited;
    let mut union = KeyWriter::create(vis_out)?;
    let mut fresh = KeyWriter::create(fresh_out)?;
    let mut b = cand.next()?;
    loop {
        let a = vis.as_ref().and_then(|r| r.cur);
        match (a, b) {
            (None, None) => break,
            (Some(x), None) => {
                union.push(x)?;
                vis.as_mut().unwrap().advance()?;
            }
            (None, Some(y)) => {
                union.push(y)?;
                fresh.push(y)?;
                b = cand.next()?;
            }
            (Some(x), Some(y)) => {
                if x < y {
                    union.push(x)?;
                    vis.as_mut().unwrap().advance()?;
                } else if y < x {
                    union.push(y)?;
                    fresh.push(y)?;
                    b = cand.next()?;
                } else {
                    union.push(x)?;
                    vis.as_mut().unwrap().advance()?;
                    b = cand.next()?;
                }
            }
        }
    }
    union.finish()?;
    fresh.finish()
}

fn read_u64(path: &Path) -> Option<u64> {
    fs::read_to_string(path).ok().and_then(|s| s.trim().parse().ok())
}

fn main() -> io::Result<()> {
    let mut args = std::env::args().skip(1);
    let dir = PathBuf::from(args.next().unwrap_or_else(|| "enum_store".into()));
    let max_ply: u32 = args.next().and_then(|s| s.parse().ok()).unwrap_or(64);
    let buf_keys: usize = std::env::var("ENUM_BUF_MK")
        .ok()
        .and_then(|s| s.parse::<usize>().ok())
        .unwrap_or(DEFAULT_BUF_MK)
        << 20;
    fs::create_dir_all(&dir)?;

    let shift = (64 - codec::key_space().leading_zeros()).saturating_sub(8);
    let vpath = |b: usize| dir.join(format!("visited_{b:03}.keys"));
    let vtmp = |b: usize| dir.join(format!("visited_{b:03}.tmp"));
    let fpath = |b: usize| dir.join(format!("frontier_{b:03}.keys"));
    let fnext = |b: usize| dir.join(format!("frontier_{b:03}.next"));
    let rpath = |b: usize, r: usize| dir.join(format!("run_{b:03}_{r:04}.keys"));
    let ply_txt = dir.join("ply.txt");
    let cum_txt = dir.join("cum.txt");
    let consol_txt = dir.join("consol.txt");
    let total_txt = dir.join("consol.total");
    let swap_txt = dir.join("swap.txt");

    // Applies the end-of-ply frontier swap. Idempotent: a `.next` still present
    // is moved into place, one already moved is simply absent. A bucket with no
    // `.next` produced no fresh keys, so its frontier is dropped.
    let do_swap = || -> io::Result<()> {
        for b in 0..BUCKETS {
            if fnext(b).exists() {
                let _ = fs::remove_file(fpath(b));
                fs::rename(fnext(b), fpath(b))?;
            } else {
                let _ = fs::remove_file(fpath(b));
            }
        }
        Ok(())
    };

    let mut start_ply = read_u64(&ply_txt).unwrap_or(0) as u32;
    let mut cumulative: u64 = read_u64(&cum_txt).unwrap_or(0);
    // A ply that reached the swap barrier is finished; replay the swap.
    if let Some(p) = read_u64(&swap_txt) {
        if p as u32 == start_ply + 1 {
            do_swap()?;
            cumulative = read_u64(&total_txt).unwrap_or(cumulative);
            fs::write(&ply_txt, p.to_string())?;
            fs::write(&cum_txt, cumulative.to_string())?;
            start_ply = p as u32;
            println!("recovered: ply {p} swap replayed, cumulative {cumulative}");
        }
        let _ = fs::remove_file(&swap_txt);
        let _ = fs::remove_file(&consol_txt);
        let _ = fs::remove_file(&total_txt);
    }
    // A ply interrupted mid-consolidation resumes at the bucket it reached.
    let mut resume_bucket = 0usize;
    let mut resume_new: u64 = 0;
    if let Ok(s) = fs::read_to_string(&consol_txt) {
        let f: Vec<u64> = s.split_whitespace().filter_map(|t| t.parse().ok()).collect();
        if f.len() == 3 && f[0] as u32 == start_ply + 1 {
            resume_bucket = f[1] as usize;
            resume_new = f[2];
            println!(
                "recovered: ply {} consolidation resumes at bucket {resume_bucket} ({resume_new} new so far)",
                f[0]
            );
        } else {
            let _ = fs::remove_file(&consol_txt);
        }
    }

    if start_ply == 0 && resume_bucket == 0 {
        let root = codec::encode(&solver::startpos());
        write_one(&vpath(bucket_of(root, shift)), root)?;
        write_one(&fpath(bucket_of(root, shift)), root)?;
        cumulative = 1;
        fs::write(&cum_txt, "1")?;
        println!("{:>4} {:>16} {:>18} {:>10}", "ply", "new", "cumulative", "sec");
        println!("{:>4} {:>16} {:>18} {:>10.1}", 0, 1, 1, 0.0);
    } else {
        println!(
            "resuming after ply {start_ply}, cumulative {cumulative}, buffer {} Mkeys",
            buf_keys >> 20
        );
    }

    let t0 = Instant::now();
    for ply in (start_ply + 1)..=max_ply {
        let mut runs: Vec<usize> = vec![0; BUCKETS];
        let tp = Instant::now();
        let (mut t_gen, mut t_spill) = (0.0f64, 0.0f64);

        // ---- 1. expand -------------------------------------------------
        // Skipped when resuming mid-consolidation: the run files are on disk,
        // so the surviving ones are counted rather than regenerated.
        if resume_bucket > 0 {
            for b in 0..BUCKETS {
                let mut r = 0;
                while rpath(b, r).exists() {
                    r += 1;
                }
                runs[b] = r;
            }
        } else {
            let mut bufs: Vec<Vec<u64>> = vec![Vec::new(); BUCKETS];
            let mut buffered = 0usize;
            let spill_secs = std::cell::Cell::new(0.0f64);
            let spill = |bufs: &mut Vec<Vec<u64>>, runs: &mut Vec<usize>| -> io::Result<()> {
                for b in 0..BUCKETS {
                    if bufs[b].is_empty() {
                        continue;
                    }
                    let ts = Instant::now();
                    bufs[b].sort_unstable();
                    bufs[b].dedup();
                    let mut w = KeyWriter::create(&rpath(b, runs[b]))?;
                    for k in bufs[b].iter() {
                        w.push(*k)?;
                    }
                    w.finish()?;
                    runs[b] += 1;
                    bufs[b].clear();
                    spill_secs.set(spill_secs.get() + ts.elapsed().as_secs_f64());
                }
                Ok(())
            };

            for b in 0..BUCKETS {
                let mut fr = match KeyReader::open(&fpath(b))? {
                    Some(r) => r,
                    None => continue,
                };
                while let Some(key) = fr.cur {
                    let pos = codec::decode(key);
                    for m in movegen::legal_moves(&pos).iter() {
                        let mut q = pos;
                        q.make(*m);
                        let ck = codec::encode(&q);
                        bufs[bucket_of(ck, shift)].push(ck);
                        buffered += 1;
                    }
                    if buffered >= buf_keys {
                        spill(&mut bufs, &mut runs)?;
                        buffered = 0;
                    }
                    fr.advance()?;
                }
            }
            spill(&mut bufs, &mut runs)?;
            t_spill = spill_secs.get();
            t_gen = tp.elapsed().as_secs_f64() - t_spill;
        }
        let t_expand = tp.elapsed().as_secs_f64();

        // ---- 2. consolidate, one bucket at a time -----------------------
        // The next frontier is written beside the current one; the swap happens
        // only once every bucket is done, so an interrupted ply is resumable.
        let mut new_total: u64 = resume_new;
        for b in resume_bucket..BUCKETS {
            if runs[b] > 0 {
                let mut readers = Vec::with_capacity(runs[b]);
                for r in 0..runs[b] {
                    if let Some(rd) = KeyReader::open(&rpath(b, r))? {
                        readers.push(rd);
                    }
                }
                let vis = KeyReader::open(&vpath(b))?;
                let fresh = merge_new(vis, KwayMerge::new(readers), &vtmp(b), &fnext(b))?;
                fs::rename(vtmp(b), vpath(b))?;
                if fresh == 0 {
                    let _ = fs::remove_file(fnext(b));
                }
                new_total += fresh;
                for r in 0..runs[b] {
                    let _ = fs::remove_file(rpath(b, r));
                }
            }
            // Recording progress after every bucket is what makes replay cheap.
            fs::write(&consol_txt, format!("{ply} {} {new_total}", b + 1))?;
        }

        let t_consol = tp.elapsed().as_secs_f64() - t_expand;

        // ---- 3. commit --------------------------------------------------
        cumulative += new_total;
        fs::write(&total_txt, cumulative.to_string())?;
        fs::write(&swap_txt, ply.to_string())?;
        do_swap()?;
        fs::write(&ply_txt, ply.to_string())?;
        fs::write(&cum_txt, cumulative.to_string())?;
        let _ = fs::remove_file(&swap_txt);
        let _ = fs::remove_file(&consol_txt);
        let _ = fs::remove_file(&total_txt);
        resume_bucket = 0;
        resume_new = 0;

        println!(
            "{:>4} {:>16} {:>18} {:>10.1}   [gen {t_gen:.1} spill {t_spill:.1} consol {t_consol:.1}]",
            ply,
            new_total,
            cumulative,
            t0.elapsed().as_secs_f64()
        );
        io::stdout().flush()?;
        if new_total == 0 {
            println!("frontier exhausted -- REACHABLE SET FULLY ENUMERATED");
            break;
        }
    }
    println!("total reachable positions: {cumulative}");
    Ok(())
}
