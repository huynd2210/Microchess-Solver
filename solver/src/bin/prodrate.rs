//! How fast can the CPU actually produce the records an external-memory run
//! would write? If this is below the drive's write bandwidth, the disk is not
//! the bottleneck and its speed does not matter.
//!
//! Three stages, timed separately over the same work:
//!   1. movegen + make            (the floor: what any design must pay)
//!   2. + exact codec key         (what a delta-encoded visited set needs)
//!   3. bytes/sec implied by (2) at 6 bytes per child record
use solver::{codec, movegen, startpos, Position};
use std::time::Instant;

/// Expand every position at `depth` plies from the start, counting children.
fn walk(depth: u32, do_encode: bool) -> (u64, u64) {
    let start = startpos();
    let mut frontier = vec![start];
    let mut children_seen: u64 = 0;
    let mut keysum: u64 = 0;
    for _ in 0..depth {
        let mut next = Vec::with_capacity(frontier.len() * 9);
        for p in &frontier {
            for m in movegen::legal_moves(p).iter() {
                let mut q = *p;
                q.make(*m);
                children_seen += 1;
                if do_encode {
                    // fold the key in so the optimiser cannot delete the work
                    keysum = keysum.wrapping_add(codec::encode(&q));
                }
                next.push(q);
            }
        }
        frontier = next;
    }
    (children_seen, keysum)
}

fn main() {
    let depth: u32 = std::env::args()
        .nth(1)
        .and_then(|s| s.parse().ok())
        .unwrap_or(6);

    // stage 1: movegen + make only
    let t0 = Instant::now();
    let (n1, _) = walk(depth, false);
    let s1 = t0.elapsed().as_secs_f64();

    // stage 2: movegen + make + exact key
    let t0 = Instant::now();
    let (n2, ks) = walk(depth, true);
    let s2 = t0.elapsed().as_secs_f64();
    assert_eq!(n1, n2);
    std::hint::black_box(ks);

    let per_child_gen = s1 / n1 as f64 * 1e9;
    let per_child_enc = (s2 - s1) / n1 as f64 * 1e9;
    let rate = n1 as f64 / s2;

    println!("depth {depth}: {n1} child positions expanded");
    println!("  movegen + make      : {s1:.2} s   {per_child_gen:.1} ns/child");
    println!("  + exact codec key   : {s2:.2} s   {per_child_enc:.1} ns/child for the key alone");
    println!("  production rate     : {:.2} M children/s (1 thread)", rate / 1e6);
    println!(
        "  at 6 B per record   : {:.0} MB/s (1 thread), {:.0} MB/s (12 threads)",
        rate * 6.0 / 1e6,
        rate * 6.0 * 12.0 / 1e6
    );
}
