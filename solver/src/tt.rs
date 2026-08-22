//! Transposition table keyed by the **exact** codec key
//! ([`crate::codec`]), per docs/ENCODING.md:
//!
//! * the full 64-bit exact key is stored **in the entry**;
//! * a hash of the key chooses the bucket only — it is an *address*, never an
//!   identity;
//! * probe compares the stored key in full, so a bucket-hash collision costs
//!   one extra miss and can never return a value stored under a different
//!   key. There is no mechanism by which `get(k)` returns a value inserted
//!   with `put(k' != k)`.
//!
//! Replacement policy is deliberately simple (this task is about correctness,
//! not eviction): update in place if the key is present, else take the first
//! free slot of the 4-way bucket, else replace one slot chosen from the key.
//! The table may be evicted freely — it is the "search TT" of
//! docs/ARCHITECTURE.md; settled values belong in an exact store.

const ASSOC: usize = 4;

pub fn splitmix64(z0: u64) -> u64 {
    let mut z = z0.wrapping_add(0x9E37_79B9_7F4A_7C15);
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^ (z >> 31)
}

/// Bucket-selection hash of the exact key. Address only — never identity.
#[inline]
pub fn bucket_hash(key: u64) -> u64 {
    // Mix twice so that low-entropy keys (dense small integers, as produced by
    // the combinatorial codec at shallow plies) still spread over buckets.
    splitmix64(splitmix64(key))
}

#[derive(Clone, Copy)]
struct Entry<V: Copy> {
    key: u64,
    val: Option<V>,
}

fn empty_entry<V: Copy>() -> Entry<V> {
    Entry { key: 0, val: None }
}

fn empty_entry_array<V: Copy>() -> [Entry<V>; ASSOC] {
    [empty_entry::<V>(), empty_entry::<V>(), empty_entry::<V>(), empty_entry::<V>()]
}

pub struct Tt<V: Copy> {
    buckets: Vec<[Entry<V>; ASSOC]>,
    mask: u64,
}

impl<V: Copy> Tt<V> {
    /// A table with `2^bucket_bits` buckets x [`ASSOC`] entries.
    pub fn new(bucket_bits: usize) -> Self {
        assert!((1..=31).contains(&bucket_bits), "bucket_bits out of range");
        let n = 1usize << bucket_bits;
        Tt { buckets: vec![empty_entry_array::<V>(); n], mask: (n - 1) as u64 }
    }

    #[inline]
    pub fn bucket_of(&self, key: u64) -> usize {
        (bucket_hash(key) & self.mask) as usize
    }

    pub fn len_entries(&self) -> usize {
        self.buckets.len() * ASSOC
    }

    /// Look up `key`. Returns `Some(v)` only if `v` was stored by
    /// `put(key, v)` with the same key.
    #[inline]
    pub fn get(&self, key: u64) -> Option<V> {
        for e in &self.buckets[self.bucket_of(key)] {
            if e.val.is_some() && e.key == key {
                return e.val;
            }
        }
        None
    }

    /// Store `(key, val)`, replacing any previous entry under the same key.
    pub fn put(&mut self, key: u64, val: V) {
        let idx = self.bucket_of(key);
        let b = &mut self.buckets[idx];
        for e in b.iter_mut() {
            if e.val.is_some() && e.key == key {
                e.val = Some(val);
                return;
            }
        }
        for e in b.iter_mut() {
            if e.val.is_none() {
                *e = Entry { key, val: Some(val) };
                return;
            }
        }
        // Bucket full: simple replacement — evict a deterministic victim.
        let victim = (bucket_hash(key) >> 60) as usize % ASSOC;
        b[victim] = Entry { key, val: Some(val) };
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Value derived from its key, so any cross-key leak shows up immediately.
    fn fk(k: u64) -> u64 {
        k.wrapping_mul(0x1234_5678_9ABC_DEF1) ^ 0xA5A5_A5A5_5A5A_5A5A
    }

    fn key_stream() -> impl Iterator<Item = u64> {
        (0u64..).map(|i| i.wrapping_mul(0x9E37_79B9_7F4A_7C15).wrapping_add(0xD1B5_4A32_D192_ED03))
    }

    #[test]
    fn no_probe_returns_a_foreign_value() {
        // Small table, far more inserts than entry slots: heavy eviction.
        let mut tt = Tt::<u64>::new(12); // 4096 buckets x 4 = 16384 entries
        let keys: Vec<u64> = key_stream().take(200_000).collect();
        for &k in &keys {
            tt.put(k, fk(k));
        }
        // The core safety property, checked over inserted AND absent keys:
        // whatever comes back must be the probed key's own value or nothing.
        for &k in &keys {
            match tt.get(k) {
                Some(v) => assert_eq!(v, fk(k), "foreign value returned for key {k}"),
                None => {}
            }
        }
        for k in key_stream().skip(200_000).take(100_000) {
            assert_eq!(tt.get(k), None, "absent key {k} returned a value");
        }
    }

    #[test]
    fn every_key_readable_when_no_bucket_overflows() {
        // Deterministic no-eviction check: pick keys that land in pairwise
        // distinct buckets, so every insert finds a free slot. All must read
        // back exactly.
        let mut tt = Tt::<u64>::new(18);
        let mut used_buckets = std::collections::HashSet::new();
        let mut keys = Vec::new();
        for k in key_stream() {
            let b = tt.bucket_of(k);
            if used_buckets.insert(b) {
                keys.push(k);
                if keys.len() == 100_000 {
                    break;
                }
            }
        }
        for &k in &keys {
            tt.put(k, fk(k));
        }
        for &k in &keys {
            assert_eq!(tt.get(k), Some(fk(k)), "lost value for key {k}");
        }
        // Absent keys (different buckets) return None.
        for k in key_stream().skip(500_000).take(50_000) {
            assert_eq!(tt.get(k), None, "absent key {k} returned a value");
        }
    }

    #[test]
    fn forced_bucket_collisions_never_leak_across_keys() {
        let tt_probe = Tt::<u64>::new(8); // tiny: 256 buckets
        // Collect many keys that all land in the same bucket.
        let target = tt_probe.bucket_of(1);
        let mut same_bucket = Vec::new();
        for k in key_stream().take(2_000_000) {
            if tt_probe.bucket_of(k) == target {
                same_bucket.push(k);
                if same_bucket.len() == 16 {
                    break;
                }
            }
        }
        assert_eq!(same_bucket.len(), 16, "could not construct bucket collisions");

        let mut tt = Tt::<u64>::new(8);
        for &k in &same_bucket {
            tt.put(k, fk(k));
        }
        // Only ASSOC of them can be resident; whichever are resident must be
        // exactly right, and nothing else may come back.
        let mut resident = 0;
        for &k in &same_bucket {
            match tt.get(k) {
                Some(v) => {
                    assert_eq!(v, fk(k));
                    resident += 1;
                }
                None => {}
            }
        }
        assert_eq!(resident, ASSOC, "expected exactly {ASSOC} survivors in one bucket");
    }

    #[test]
    fn overwrite_same_key_is_idempotent_identity() {
        let mut tt = Tt::<u64>::new(4);
        tt.put(7, 1);
        tt.put(7, 2);
        tt.put(42, 3);
        assert_eq!(tt.get(7), Some(2));
        assert_eq!(tt.get(42), Some(3));
    }

    #[test]
    fn key_zero_works() {
        // Key 0 is a valid codec key; used-ness is tracked separately from the
        // key value so this must behave like any other key.
        let mut tt = Tt::<u64>::new(4);
        assert_eq!(tt.get(0), None);
        tt.put(0, 99);
        assert_eq!(tt.get(0), Some(99));
        assert_eq!(tt.get(1 << 40), None);
    }
}
