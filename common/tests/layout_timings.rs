//! Timings of the graph layout (performance phase P0, 2026-11-02).
//!
//! Ignored by default: it measures, it doesn't test. Run it in release mode
//! (the WASM is size-optimised, so expect the browser to be slower still):
//!
//!   cargo test -p common --release --test layout_timings -- --ignored --nocapture
//!
//! Each size is laid out twice: from scratch (Auto-arrange), and with every
//! node seeded and pinned, which is what the graph view does on every load
//! once positions are saved.

use std::collections::{HashMap, HashSet};
use std::time::Instant;

use common::graph_layout::cluster_layout;
use uuid::Uuid;

/// A graph shaped like the seeded data: about 0.6 edges per node, some hubs.
fn graph(n: usize) -> (Vec<Uuid>, Vec<(Uuid, Uuid)>) {
    let ids: Vec<Uuid> = (0..n as u128).map(|i| Uuid::from_u128(i + 1)).collect();
    let mut edges = Vec::new();
    for i in 0..n {
        if i % 3 == 0 {
            edges.push((ids[(i / 20) * 20 % n], ids[i]));
        }
        if i % 4 == 0 {
            edges.push((ids[i], ids[(i * 31 + 1) % n]));
        }
    }
    edges.retain(|(a, b)| a != b);
    (ids, edges)
}

#[test]
#[ignore = "timing harness, run on demand"]
fn layout_timings() {
    println!(
        "\n{:>6} {:>7} {:>12} {:>12}",
        "nodes", "edges", "scratch ms", "pinned ms"
    );
    for n in [100, 300, 500, 1_000, 2_000] {
        let (ids, edges) = graph(n);
        let none = HashMap::new();
        let t = Instant::now();
        let laid = cluster_layout(&ids, &edges, &none, &HashSet::new());
        let scratch = t.elapsed();
        let pinned: HashSet<Uuid> = ids.iter().copied().collect();
        let t = Instant::now();
        let again = cluster_layout(&ids, &edges, &laid, &pinned);
        let pinned_ms = t.elapsed();
        assert_eq!(again.len(), ids.len());
        println!(
            "{n:>6} {:>7} {:>12.1} {:>12.1}",
            edges.len(),
            scratch.as_secs_f64() * 1e3,
            pinned_ms.as_secs_f64() * 1e3
        );
    }
}
