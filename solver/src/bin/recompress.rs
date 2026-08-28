//! One-shot converter: raw 8-byte key files -> varint-delta streams.
//!
//! The ply-14 checkpoint was written when `visited` stored keys raw. At 8 B/key
//! that store caps out around ply 16 against the free disk, which is why the
//! run was stopped. This rewrites it in place, in the format
//! `solver::keystream` defines, without re-running the enumeration.
//!
//! # Safety
//!
//! Replacing the checkpoint is irreversible — it is 30 minutes of ply-14
//! compute and everything before it — so no raw file is removed until its
//! replacement has been **read back and compared key-for-key** against it.
//! A bucket is converted to a sibling `.vz`, verified, then renamed over. An
//! interrupted run leaves the raw file untouched and redoes that bucket.
//! Progress is recorded per bucket, so a restart resumes rather than rescans.
//!
//! Usage: `recompress <store_dir> [--dry-run]`

use solver::keystream::{KeyReader, KeyWriter};
use std::fs::{self, File};
use std::io::{self, BufReader, Read, Write};
use std::path::{Path, PathBuf};

const BUCKETS: usize = 256;

/// Streams a raw little-endian u64 file, checking as it goes that keys ascend
/// strictly — the varint format encodes gaps, so a non-ascending input would
/// silently produce garbage rather than fail.
struct RawReader {
    r: BufReader<File>,
    prev: Option<u64>,
}

impl RawReader {
    fn open(path: &Path) -> io::Result<Option<Self>> {
        match File::open(path) {
            Ok(f) => Ok(Some(RawReader {
                r: BufReader::with_capacity(1 << 20, f),
                prev: None,
            })),
            Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(e),
        }
    }

    fn next(&mut self) -> io::Result<Option<u64>> {
        let mut b = [0u8; 8];
        let mut got = 0;
        while got < 8 {
            match self.r.read(&mut b[got..])? {
                0 => break,
                n => got += n,
            }
        }
        if got == 0 {
            return Ok(None);
        }
        if got != 8 {
            return Err(io::Error::new(
                io::ErrorKind::UnexpectedEof,
                "raw key file length is not a multiple of 8",
            ));
        }
        let k = u64::from_le_bytes(b);
        if let Some(p) = self.prev {
            if k <= p {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    format!("raw keys not strictly ascending: {p} then {k}"),
                ));
            }
        }
        self.prev = Some(k);
        Ok(Some(k))
    }
}

/// Encodes `src` to `dst`, then re-reads `dst` and compares it key-for-key
/// against `src`. Returns the key count. Nothing is deleted here.
fn convert_verified(src: &Path, dst: &Path) -> io::Result<u64> {
    let mut raw = match RawReader::open(src)? {
        Some(r) => r,
        None => return Ok(0),
    };
    let mut w = KeyWriter::create(dst)?;
    let mut n = 0u64;
    while let Some(k) = raw.next()? {
        w.push(k)?;
        n += 1;
    }
    let written = w.finish()?;
    assert_eq!(written, n, "writer key count disagrees with input");

    // read-back: the encoded stream must reproduce the input exactly
    let mut raw = RawReader::open(src)?.expect("source vanished mid-convert");
    let mut back = KeyReader::open(dst)?;
    let mut checked = 0u64;
    loop {
        let want = raw.next()?;
        let got = back.as_ref().and_then(|r| r.cur);
        if want != got {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!("verify failed at key {checked}: raw {want:?} vs varint {got:?}"),
            ));
        }
        if want.is_none() {
            break;
        }
        back.as_mut().unwrap().advance()?;
        checked += 1;
    }
    assert_eq!(checked, n, "verify pass saw a different key count");
    Ok(n)
}

fn main() -> io::Result<()> {
    let mut args = std::env::args().skip(1);
    let dir = PathBuf::from(args.next().unwrap_or_else(|| "enum_store".into()));
    let dry_run = std::env::args().any(|a| a == "--dry-run");

    let progress = dir.join("recompress.progress");
    let start = fs::read_to_string(&progress)
        .ok()
        .and_then(|s| s.trim().parse::<usize>().ok())
        .unwrap_or(0);
    if start > 0 {
        println!("resuming conversion at bucket {start}");
    }
    if dry_run {
        println!("DRY RUN — nothing is replaced");
    }

    let mut vis_keys = 0u64;
    let mut fro_keys = 0u64;
    let mut raw_bytes = 0u64;
    let mut new_bytes = 0u64;

    // buckets already converted still count toward the totals
    let size = |p: &Path| fs::metadata(p).map(|m| m.len()).unwrap_or(0);

    for b in 0..BUCKETS {
        for (name, done_counter) in [("visited", 0usize), ("frontier", 1usize)] {
            let src = dir.join(format!("{name}_{b:03}.keys"));
            let tmp = dir.join(format!("{name}_{b:03}.vz"));
            if !src.exists() {
                continue;
            }
            if b < start {
                // already converted; measure what is there
                let n = {
                    let mut r = KeyReader::open(&src)?;
                    let mut c = 0u64;
                    while let Some(_) = r.as_ref().and_then(|x| x.cur) {
                        c += 1;
                        r.as_mut().unwrap().advance()?;
                    }
                    c
                };
                new_bytes += size(&src);
                raw_bytes += n * 8;
                if done_counter == 0 {
                    vis_keys += n;
                } else {
                    fro_keys += n;
                }
                continue;
            }
            let before = size(&src);
            let n = convert_verified(&src, &tmp)?;
            let after = size(&tmp);
            raw_bytes += before;
            new_bytes += after;
            if done_counter == 0 {
                vis_keys += n;
            } else {
                fro_keys += n;
            }
            if dry_run {
                fs::remove_file(&tmp)?;
            } else {
                // verified above, so replacing the raw file is safe
                fs::rename(&tmp, &src)?;
            }
        }
        if !dry_run {
            fs::write(&progress, (b + 1).to_string())?;
        }
        if b % 16 == 15 {
            println!(
                "  bucket {b:>3}: {vis_keys:>13} visited, {fro_keys:>13} frontier, \
                 {:.3} B/key",
                new_bytes as f64 / (vis_keys + fro_keys).max(1) as f64
            );
            io::stdout().flush()?;
        }
    }

    let total = vis_keys + fro_keys;
    println!("\nvisited  keys : {vis_keys}");
    println!("frontier keys : {fro_keys}");
    println!("raw   bytes   : {raw_bytes} ({:.2} GB)", raw_bytes as f64 / 1e9);
    println!("varint bytes  : {new_bytes} ({:.2} GB)", new_bytes as f64 / 1e9);
    println!("B/key         : {:.4}", new_bytes as f64 / total.max(1) as f64);
    println!("compression   : {:.2}x", raw_bytes as f64 / new_bytes.max(1) as f64);

    if !dry_run {
        fs::write(dir.join("cum.txt"), vis_keys.to_string())?;
        let _ = fs::remove_file(&progress);
        println!("\ncum.txt written: {vis_keys}");
    }
    Ok(())
}
