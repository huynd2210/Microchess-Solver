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
//!      generate children, encode, append to a per-bucket RAM buffer. When a
//!      thread's buffered total exceeds its share of `ENUM_BUF_MK`, it sorts,
//!      dedupes and spills each bucket to a run file. Deduping before spilling
//!      is what keeps the intermediate volume bounded: at saturation a position
//!      is reached ~8.9 ways.
//!   2. **Consolidate** — per bucket, k-way merge every run file any thread
//!      wrote for it, dedupe, then stream against that bucket's `visited` file.
//!      Keys not already present become the next frontier and are merged into
//!      `visited`.
//!
//! Only one bucket is rewritten at a time, so peak disk is
//! `visited + one bucket`, not `2 x visited`.
//!
//! # Why expansion parallelises without changing the answer
//!
//! Expansion is the whole cost — `codec::encode` alone is ~520 ns/child at
//! working-set size — and it is a pure fan-out: reading the frontier and
//! generating children never inspects shared mutable state. Threads claim whole
//! frontier buckets from one atomic cursor, so every frontier key is expanded
//! exactly once, and each child lands in exactly one thread's buffer for
//! exactly one bucket. Run files carry the writing thread's id, so no two
//! threads ever touch the same file.
//!
//! Consolidation then merges *all* run files for a bucket regardless of which
//! thread wrote them, and dedupes. The merged key set is therefore a function
//! of the frontier alone — thread count and scheduling change only how the
//! intermediate run files are cut up. `visited` and `frontier` come out
//! byte-identical to a serial run, which `tests/parallel_identity.rs` asserts
//! directly rather than taking on faith. Consolidation itself stays serial: it
//! is ~2% of the ply, and keeping it serial keeps the count exact by
//! construction.
//!
//! # On-disk format
//!
//! Every key file is a **varint-delta stream**: keys ascending, each stored as
//! a LEB128 gap from its predecessor (the first is absolute). Measured at
//! 1.283 B/key on the real ply-14 set — 6.2x smaller than the raw 8 B/key that
//! capped the earlier run near ply 16. No pass materialises a bucket — reads,
//! merges and writes are all streaming, so RAM does not grow with bucket size.
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
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::time::Instant;

const BUCKETS: usize = 256;
/// Keys buffered in RAM **across all threads** before each spills, in units of
/// 2^20 keys (8 B each). Larger buffers dedupe a bigger slice of the child
/// stream and emit denser run files. Override with `ENUM_BUF_MK`.
///
/// Peak resident is bounded by roughly `3 x` this (measured: 1.11 GB at the
/// default, running ply 12). Ply-12 wall time is flat from 16 through 128 Mkeys,
/// so this is a memory knob, not a speed one — what mattered for speed was
/// bounding the retained capacity at all, which halved the ply.
const DEFAULT_BUF_MK: usize = 128;

fn bucket_of(key: u64, shift: u32) -> usize {
    ((key >> shift) as usize).min(BUCKETS - 1)
}

fn vpath(dir: &Path, b: usize) -> PathBuf {
    dir.join(format!("visited_{b:03}.keys"))
}
fn vtmp(dir: &Path, b: usize) -> PathBuf {
    dir.join(format!("visited_{b:03}.tmp"))
}
fn fpath(dir: &Path, b: usize) -> PathBuf {
    dir.join(format!("frontier_{b:03}.keys"))
}
fn fnext(dir: &Path, b: usize) -> PathBuf {
    dir.join(format!("frontier_{b:03}.next"))
}
/// Run files are namespaced by writing thread, so concurrent spills of the same
/// bucket cannot collide.
fn rpath(dir: &Path, b: usize, tid: usize, seq: usize) -> PathBuf {
    dir.join(format!("run_{b:03}_t{tid:02}_{seq:04}.keys"))
}

/// Every run file present, grouped by bucket. Read from the directory rather
/// than from in-memory counters so that consolidation resuming after a crash
/// sees exactly what expansion left behind.
fn run_files_by_bucket(dir: &Path) -> io::Result<Vec<Vec<PathBuf>>> {
    let mut out: Vec<Vec<PathBuf>> = vec![Vec::new(); BUCKETS];
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let name = entry.file_name();
        let name = name.to_string_lossy();
        let rest = match name.strip_prefix("run_") {
            Some(r) if name.ends_with(".keys") => r,
            _ => continue,
        };
        if let Ok(b) = rest[..rest.len().min(3)].parse::<usize>() {
            if b < BUCKETS {
                out[b].push(entry.path());
            }
        }
    }
    for v in out.iter_mut() {
        v.sort();
    }
    Ok(out)
}

/// Sorts, dedupes and writes out every non-empty per-bucket buffer this thread
/// holds.
///
/// Capacity is retained up to `cap_hint` so the next fill does not re-grow from
/// nothing, but no further: buckets are skewed, and a buffer that simply kept
/// whatever it peaked at would ratchet to its own high-water mark. Summed over
/// `BUCKETS x threads` buffers that is the sum of the per-bucket peaks, not the
/// peak of the sum — it exceeds the budget by the skew factor and climbs ply
/// over ply. That is what exhausted RAM during ply 16.
fn spill(
    dir: &Path,
    tid: usize,
    bufs: &mut [Vec<u64>],
    seq: &mut [usize],
    cap_hint: usize,
    nanos: &AtomicU64,
) -> io::Result<()> {
    let t = Instant::now();
    for b in 0..BUCKETS {
        if bufs[b].is_empty() {
            continue;
        }
        bufs[b].sort_unstable();
        bufs[b].dedup();
        let mut w = KeyWriter::create(&rpath(dir, b, tid, seq[b]))?;
        for k in bufs[b].iter() {
            w.push(*k)?;
        }
        w.finish()?;
        seq[b] += 1;
        bufs[b].clear();
        if bufs[b].capacity() > cap_hint {
            bufs[b].shrink_to(cap_hint);
        }
    }
    nanos.fetch_add(t.elapsed().as_nanos() as u64, Ordering::Relaxed);
    Ok(())
}

/// Expands every frontier bucket into run files, using `nthreads` workers that
/// claim buckets from a shared cursor. Consolidation discovers the resulting
/// run files from disk; the return value is only the summed spill time.
fn expand(dir: &Path, shift: u32, nthreads: usize, buf_keys: usize) -> io::Result<u64> {
    let cursor = AtomicUsize::new(0);
    let spill_nanos = AtomicU64::new(0);
    let per_thread = (buf_keys / nthreads).max(1 << 16);
    // Retained capacity per bucket. Twice the even share absorbs ordinary skew
    // without letting any one bucket hoard; see `spill`. Worst-case resident is
    // then bounded by roughly 3x `buf_keys` across all threads — live keys, plus
    // the retained floor, plus one hot bucket mid-growth — instead of drifting
    // upward with no bound at all.
    let cap_hint = ((per_thread / BUCKETS) * 2).max(4096);
    std::thread::scope(|s| -> io::Result<()> {
        let mut handles = Vec::with_capacity(nthreads);
        for tid in 0..nthreads {
            let cursor = &cursor;
            let spill_nanos = &spill_nanos;
            handles.push(s.spawn(move || -> io::Result<()> {
                let mut bufs: Vec<Vec<u64>> =
                    (0..BUCKETS).map(|_| Vec::with_capacity(cap_hint)).collect();
                let mut seq: Vec<usize> = vec![0; BUCKETS];
                let mut buffered = 0usize;
                loop {
                    // one atomic claim per bucket => each is expanded exactly once
                    let b = cursor.fetch_add(1, Ordering::Relaxed);
                    if b >= BUCKETS {
                        break;
                    }
                    let mut fr = match KeyReader::open(&fpath(dir, b))? {
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
                        if buffered >= per_thread {
                            spill(dir, tid, &mut bufs, &mut seq, cap_hint, spill_nanos)?;
                            buffered = 0;
                        }
                        fr.advance()?;
                    }
                }
                spill(dir, tid, &mut bufs, &mut seq, cap_hint, spill_nanos)
            }));
        }
        for h in handles {
            h.join().expect("expansion thread panicked")?;
        }
        Ok(())
    })?;
    Ok(spill_nanos.load(Ordering::Relaxed))
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
    let nthreads: usize = std::env::var("ENUM_THREADS")
        .ok()
        .and_then(|s| s.parse::<usize>().ok())
        .filter(|n| *n > 0)
        .unwrap_or_else(|| {
            // Expansion is bound by memory traffic through the codec tables,
            // not by ALU work, so the second thread on an SMT core buys almost
            // nothing. Measured at ply 12: expand took 37.6 s on 10 threads,
            // 37.8 s on 14 and 39.0 s on 20 -- flat past the physical core
            // count, and slightly worse once every core is doubled up. Half of
            // `available_parallelism` lands on that knee and leaves the machine
            // usable. Override with `ENUM_THREADS`.
            let logical = std::thread::available_parallelism()
                .map(|n| n.get())
                .unwrap_or(2);
            (logical / 2).max(1)
        })
        .min(BUCKETS);
    fs::create_dir_all(&dir)?;

    let shift = (64 - codec::key_space().leading_zeros()).saturating_sub(8);
    let dir = dir.as_path();
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
            if fnext(dir, b).exists() {
                let _ = fs::remove_file(fpath(dir, b));
                fs::rename(fnext(dir, b), fpath(dir, b))?;
            } else {
                let _ = fs::remove_file(fpath(dir, b));
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

    // Stale scratch from an expansion that was killed before consolidation
    // began. It must go — a run file truncated mid-write decodes to garbage —
    // but only when no consolidation is in flight, because then these very
    // files are what the resume above depends on. Doing it here rather than by
    // hand is the point: deleting them manually during a live consolidation
    // destroys the ply-17 frontier and mixes `visited` across two plies, which
    // is unrepairable and cost a full rebuild once already.
    if resume_bucket == 0 {
        let mut swept = 0usize;
        for entry in fs::read_dir(dir)? {
            let entry = entry?;
            let name = entry.file_name();
            let name = name.to_string_lossy();
            if (name.starts_with("run_") && name.ends_with(".keys"))
                || name.ends_with(".tmp")
                || name.ends_with(".next")
            {
                fs::remove_file(entry.path())?;
                swept += 1;
            }
        }
        if swept > 0 {
            println!("swept {swept} stale scratch files from an interrupted expansion");
        }
    }

    if start_ply == 0 && resume_bucket == 0 {
        let root = codec::encode(&solver::startpos());
        write_one(&vpath(dir, bucket_of(root, shift)), root)?;
        write_one(&fpath(dir, bucket_of(root, shift)), root)?;
        cumulative = 1;
        fs::write(&cum_txt, "1")?;
        println!("{:>4} {:>16} {:>18} {:>10}", "ply", "new", "cumulative", "sec");
        println!("{:>4} {:>16} {:>18} {:>10.1}", 0, 1, 1, 0.0);
    } else {
        println!("resuming after ply {start_ply}, cumulative {cumulative}");
    }
    println!(
        "threads {nthreads}, buffer {} Mkeys total ({} Mkeys/thread)",
        buf_keys >> 20,
        (buf_keys / nthreads) >> 20
    );

    let t0 = Instant::now();
    for ply in (start_ply + 1)..=max_ply {
        let tp = Instant::now();

        // ---- 1. expand -------------------------------------------------
        // Skipped when resuming mid-consolidation: the run files are on disk.
        let mut spill_cpu = 0.0f64;
        if resume_bucket == 0 {
            spill_cpu = expand(dir, shift, nthreads, buf_keys)? as f64 / 1e9;
        }
        let t_expand = tp.elapsed().as_secs_f64();

        // ---- 2. consolidate, one bucket at a time -----------------------
        // The next frontier is written beside the current one; the swap happens
        // only once every bucket is done, so an interrupted ply is resumable.
        let runs = run_files_by_bucket(dir)?;
        let mut new_total: u64 = resume_new;
        for b in resume_bucket..BUCKETS {
            if !runs[b].is_empty() {
                let mut readers = Vec::with_capacity(runs[b].len());
                for p in &runs[b] {
                    if let Some(rd) = KeyReader::open(p)? {
                        readers.push(rd);
                    }
                }
                let vis = KeyReader::open(&vpath(dir, b))?;
                let fresh =
                    merge_new(vis, KwayMerge::new(readers), &vtmp(dir, b), &fnext(dir, b))?;
                fs::rename(vtmp(dir, b), vpath(dir, b))?;
                if fresh == 0 {
                    let _ = fs::remove_file(fnext(dir, b));
                }
                new_total += fresh;
                for p in &runs[b] {
                    let _ = fs::remove_file(p);
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
            "{:>4} {:>16} {:>18} {:>10.1}   [expand {t_expand:.1} (spill-cpu {spill_cpu:.1}) consol {t_consol:.1}]",
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
