//! Varint-delta key streams — the on-disk format for every sorted key set.
//!
//! A file is a sequence of LEB128 gaps between ascending keys (the first gap is
//! measured from zero, so it is the key itself). Reachable positions cluster
//! hard inside a material class, which is exactly what a gap encoding is paid
//! to exploit: measured at 1.004 B/key on the 732 M-key top class against 6.0
//! for raw ranks. The global set is sparser, but still far under the 8 B/key
//! that capped the raw enumeration near ply 16 on a 16 GB disk budget.
//!
//! Reading and writing are both streaming and hold a fixed buffer, so passes
//! over a bucket cost RAM independent of how many keys it holds.

use std::cmp::Reverse;
use std::collections::BinaryHeap;
use std::fs::File;
use std::io::{self, Read, Write};
use std::path::Path;

const IO_BUF: usize = 1 << 20;

/// Buffered writer of an ascending key sequence as LEB128 gaps.
pub struct KeyWriter {
    f: File,
    buf: Vec<u8>,
    prev: u64,
    n: u64,
}

impl KeyWriter {
    pub fn create(path: &Path) -> io::Result<Self> {
        Ok(KeyWriter {
            f: File::create(path)?,
            buf: Vec::with_capacity(IO_BUF + 16),
            prev: 0,
            n: 0,
        })
    }

    /// Appends `key`, which must be strictly greater than the previous one.
    #[inline]
    pub fn push(&mut self, key: u64) -> io::Result<()> {
        debug_assert!(self.n == 0 || key > self.prev, "keys must strictly ascend");
        let mut v = key - self.prev;
        self.prev = key;
        self.n += 1;
        while v >= 0x80 {
            self.buf.push((v as u8) | 0x80);
            v >>= 7;
        }
        self.buf.push(v as u8);
        if self.buf.len() >= IO_BUF {
            self.f.write_all(&self.buf)?;
            self.buf.clear();
        }
        Ok(())
    }

    /// Flushes and returns the number of keys written.
    pub fn finish(mut self) -> io::Result<u64> {
        if !self.buf.is_empty() {
            self.f.write_all(&self.buf)?;
            self.buf.clear();
        }
        self.f.flush()?;
        Ok(self.n)
    }
}

/// Buffered reader of a varint-delta key stream. The current key is held in
/// [`KeyReader::cur`] so several streams can be merged without materialising
/// any of them; `None` means the stream is exhausted.
pub struct KeyReader {
    f: File,
    buf: Vec<u8>,
    pos: usize,
    len: usize,
    prev: u64,
    pub cur: Option<u64>,
}

impl KeyReader {
    /// `Ok(None)` if the file does not exist — an absent bucket is an empty one.
    pub fn open(path: &Path) -> io::Result<Option<Self>> {
        let f = match File::open(path) {
            Ok(f) => f,
            Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(None),
            Err(e) => return Err(e),
        };
        let mut r = KeyReader {
            f,
            buf: vec![0u8; IO_BUF],
            pos: 0,
            len: 0,
            prev: 0,
            cur: None,
        };
        r.advance()?;
        Ok(Some(r))
    }

    #[inline]
    fn byte(&mut self) -> io::Result<Option<u8>> {
        if self.pos == self.len {
            self.len = self.f.read(&mut self.buf)?;
            self.pos = 0;
            if self.len == 0 {
                return Ok(None);
            }
        }
        let b = self.buf[self.pos];
        self.pos += 1;
        Ok(Some(b))
    }

    /// Decodes the next gap into `cur`, or sets `cur = None` at end of stream.
    #[inline]
    pub fn advance(&mut self) -> io::Result<()> {
        let mut v: u64 = 0;
        let mut shift = 0u32;
        loop {
            let b = match self.byte()? {
                Some(b) => b,
                None => {
                    if shift == 0 {
                        self.cur = None;
                        return Ok(());
                    }
                    return Err(io::Error::new(
                        io::ErrorKind::UnexpectedEof,
                        "truncated varint",
                    ));
                }
            };
            v |= ((b & 0x7f) as u64) << shift;
            if b & 0x80 == 0 {
                break;
            }
            shift += 7;
            if shift > 63 {
                return Err(io::Error::new(io::ErrorKind::InvalidData, "varint overflow"));
            }
        }
        self.prev += v;
        self.cur = Some(self.prev);
        Ok(())
    }
}

/// Ascending, deduplicated merge of several sorted key streams.
pub struct KwayMerge {
    readers: Vec<KeyReader>,
    heap: BinaryHeap<Reverse<(u64, usize)>>,
}

impl KwayMerge {
    pub fn new(readers: Vec<KeyReader>) -> Self {
        let mut heap = BinaryHeap::with_capacity(readers.len());
        for (i, r) in readers.iter().enumerate() {
            if let Some(k) = r.cur {
                heap.push(Reverse((k, i)));
            }
        }
        KwayMerge { readers, heap }
    }

    #[inline]
    pub fn next(&mut self) -> io::Result<Option<u64>> {
        let key = match self.heap.peek() {
            Some(Reverse((k, _))) => *k,
            None => return Ok(None),
        };
        // drain every stream positioned on `key`, so duplicates collapse
        while let Some(Reverse((k, i))) = self.heap.peek().copied() {
            if k != key {
                break;
            }
            self.heap.pop();
            self.readers[i].advance()?;
            if let Some(nk) = self.readers[i].cur {
                self.heap.push(Reverse((nk, i)));
            }
        }
        Ok(Some(key))
    }
}

/// Writes a single-key stream (used for the root position at ply 0).
pub fn write_one(path: &Path, key: u64) -> io::Result<()> {
    let mut w = KeyWriter::create(path)?;
    w.push(key)?;
    w.finish()?;
    Ok(())
}

/// Decodes a whole stream. Only for tests and small files — the enumeration
/// itself never materialises a bucket.
pub fn read_all(path: &Path) -> io::Result<Vec<u64>> {
    let mut out = Vec::new();
    if let Some(mut r) = KeyReader::open(path)? {
        while let Some(k) = r.cur {
            out.push(k);
            r.advance()?;
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn roundtrip(keys: &[u64]) {
        let mut p = std::env::temp_dir();
        p.push(format!("ks_test_{}.keys", keys.len() as u64 * 2654435761 % 100000));
        let mut w = KeyWriter::create(&p).unwrap();
        for k in keys {
            w.push(*k).unwrap();
        }
        assert_eq!(w.finish().unwrap(), keys.len() as u64);
        assert_eq!(read_all(&p).unwrap(), keys);
        let _ = std::fs::remove_file(&p);
    }

    #[test]
    fn empty_and_singleton() {
        roundtrip(&[]);
        roundtrip(&[0]);
        roundtrip(&[1]);
    }

    /// Gap sizes that straddle every LEB128 byte-count boundary, plus a key
    /// near the top of the 64-bit range.
    #[test]
    fn varint_boundaries() {
        let mut keys = vec![0u64];
        for shift in 0..63 {
            let last = *keys.last().unwrap();
            keys.push(last + (1u64 << shift));
            let last = *keys.last().unwrap();
            keys.push(last + (1u64 << shift) - 1);
        }
        roundtrip(&keys);
    }

    #[test]
    fn dense_and_sparse_runs() {
        let dense: Vec<u64> = (1000..3000).collect();
        roundtrip(&dense);
        let sparse: Vec<u64> = (0..2000).map(|i| i * 1_000_003 + 7).collect();
        roundtrip(&sparse);
    }

    /// A stream longer than the 1 MB I/O buffer, to exercise refill on both
    /// sides — an off-by-one there would corrupt only large files.
    #[test]
    fn spans_many_io_buffers() {
        let keys: Vec<u64> = (0..3_000_000u64).map(|i| i * 130).collect();
        roundtrip(&keys);
    }

    #[test]
    fn kway_merge_dedupes_across_streams() {
        let dir = std::env::temp_dir();
        let paths: Vec<_> = (0..3)
            .map(|i| dir.join(format!("ks_merge_{i}.keys")))
            .collect();
        let streams = [
            vec![1u64, 4, 7, 100],
            vec![1u64, 2, 7, 50, 100],
            vec![3u64, 4, 999],
        ];
        for (p, s) in paths.iter().zip(streams.iter()) {
            let mut w = KeyWriter::create(p).unwrap();
            for k in s {
                w.push(*k).unwrap();
            }
            w.finish().unwrap();
        }
        let readers: Vec<_> = paths
            .iter()
            .map(|p| KeyReader::open(p).unwrap().unwrap())
            .collect();
        let mut m = KwayMerge::new(readers);
        let mut got = Vec::new();
        while let Some(k) = m.next().unwrap() {
            got.push(k);
        }
        assert_eq!(got, vec![1, 2, 3, 4, 7, 50, 100, 999]);
        for p in &paths {
            let _ = std::fs::remove_file(p);
        }
    }
}
