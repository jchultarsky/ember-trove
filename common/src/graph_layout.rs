//! Force-directed cluster layout for the knowledge graph.
//!
//! Replaces the BFS-layered "rows" auto-arrange with a spring-embedder
//! (Fruchterman–Reingold family): edges attract, all nodes repel. The
//! equilibrium reproduces the geometry users build by hand — hubs become
//! star centres, satellite rings widen with hub degree, and weakly-connected
//! groups drift apart into distinct clusters.
//!
//! Design constraints (why this lives in `common/`, host-tested):
//! - **Deterministic.** No wall-clock, no `Math.random`: jitter comes from a
//!   splitmix64 hash of the node UUID, so the same graph always yields the
//!   same layout and tests are exact.
//! - **Mental-map preserving.** Callers pass the current positions as
//!   `seeds`; a mostly-seeded component is refined gently around where the
//!   user left it (its centroid is re-anchored) instead of being recomputed
//!   from scratch. `pinned` nodes never move at all — the initial page load
//!   pins every saved position and only floats never-placed nodes.
//! - **Readable.** A post-pass enforces `MIN_SPACING` between node centres,
//!   and disconnected components without seeds are packed in a grid below
//!   the seeded layout rather than scattered.

use std::collections::{HashMap, HashSet};

use uuid::Uuid;

/// Minimum clearance between node centres (glyph + title pill + tag dots).
pub const MIN_SPACING: f64 = 90.0;
/// Spring rest length — the distance an edge "wants" to be. Tuned to the
/// median hand-made edge length observed in real layouts (~280px).
pub const IDEAL_EDGE: f64 = 220.0;
/// Gap between packed disconnected components.
pub const COMPONENT_GAP: f64 = 200.0;
/// Node glyph envelope for viewport fitting: shape radius + title pill + tag
/// dots vertically; node diameter + title text width horizontally.
pub const NODE_ENVELOPE_W: f64 = 80.0;
pub const NODE_ENVELOPE_H: f64 = 90.0;

/// Pan/zoom `(pan_x, pan_y, zoom)` that frames the given positions in the
/// viewport with padding, centred. Zoom is clamped to [0.5, 3.0] so nodes
/// stay readable: a graph larger than ~2x the viewport is centred at 50%
/// rather than shrunk into illegibility.
pub fn fit_transform(
    positions: &HashMap<Uuid, (f64, f64)>,
    viewport_w: f64,
    viewport_h: f64,
) -> (f64, f64, f64) {
    if positions.is_empty() {
        return (0.0, 0.0, 1.0);
    }
    let mut min_x = f64::MAX;
    let mut max_x = f64::MIN;
    let mut min_y = f64::MAX;
    let mut max_y = f64::MIN;
    for &(x, y) in positions.values() {
        min_x = min_x.min(x - NODE_ENVELOPE_W / 2.0);
        max_x = max_x.max(x + NODE_ENVELOPE_W / 2.0);
        min_y = min_y.min(y - NODE_ENVELOPE_H / 2.0);
        max_y = max_y.max(y + NODE_ENVELOPE_H / 2.0);
    }
    let graph_w = (max_x - min_x).max(1.0);
    let graph_h = (max_y - min_y).max(1.0);
    let padding = 60.0;
    let fit_zoom = ((viewport_w - padding * 2.0) / graph_w)
        .min((viewport_h - padding * 2.0) / graph_h)
        .clamp(0.5, 3.0);
    let fit_pan_x = viewport_w / 2.0 - (min_x + max_x) / 2.0 * fit_zoom;
    let fit_pan_y = viewport_h / 2.0 - (min_y + max_y) / 2.0 * fit_zoom;
    (fit_pan_x, fit_pan_y, fit_zoom)
}

/// Compute a clustered layout for the graph.
///
/// - `seeds`: known positions to start from (typically the current layout).
/// - `pinned`: nodes that must not move at all (their seed is authoritative).
///   Every pinned node should also have a seed; a pinned node without one is
///   treated as unpinned.
///
/// Returns a position for every id in `node_ids`. When `pinned` is empty the
/// result is normalised so the minimum corner sits at (0, 0); with pins the
/// caller's coordinate frame is preserved.
pub fn cluster_layout(
    node_ids: &[Uuid],
    edge_pairs: &[(Uuid, Uuid)],
    seeds: &HashMap<Uuid, (f64, f64)>,
    pinned: &HashSet<Uuid>,
) -> HashMap<Uuid, (f64, f64)> {
    // Stable internal order regardless of caller order → deterministic output.
    let mut ids: Vec<Uuid> = node_ids.to_vec();
    ids.sort_unstable();
    ids.dedup();
    if ids.is_empty() {
        return HashMap::new();
    }

    let idx_of: HashMap<Uuid, usize> = ids.iter().enumerate().map(|(i, id)| (*id, i)).collect();
    // Undirected, deduped, self-loop-free edge list as index pairs.
    let mut edge_set: HashSet<(usize, usize)> = HashSet::new();
    for (s, t) in edge_pairs {
        if let (Some(&a), Some(&b)) = (idx_of.get(s), idx_of.get(t))
            && a != b
        {
            edge_set.insert((a.min(b), a.max(b)));
        }
    }
    let mut edges: Vec<(usize, usize)> = edge_set.into_iter().collect();
    edges.sort_unstable();

    let n = ids.len();
    let pinned_mask: Vec<bool> = ids
        .iter()
        .map(|id| pinned.contains(id) && seeds.contains_key(id))
        .collect();

    let components = find_components(n, &edges);

    let mut pos: Vec<(f64, f64)> = vec![(0.0, 0.0); n];
    let mut seeded_bbox: Option<(f64, f64, f64, f64)> = None; // min_x, min_y, max_x, max_y
    let mut unseeded: Vec<Vec<usize>> = Vec::new();

    for comp in &components {
        let comp_seeds: Vec<usize> = comp
            .iter()
            .copied()
            .filter(|&i| seeds.contains_key(&ids[i]))
            .collect();
        if comp_seeds.is_empty() {
            unseeded.push(comp.clone());
            continue;
        }
        layout_seeded_component(
            comp,
            &comp_seeds,
            &edges,
            &ids,
            seeds,
            &pinned_mask,
            &mut pos,
        );
        for &i in comp {
            let (x, y) = pos[i];
            seeded_bbox = Some(match seeded_bbox {
                None => (x, y, x, y),
                Some((ax, ay, bx, by)) => (ax.min(x), ay.min(y), bx.max(x), by.max(y)),
            });
        }
    }

    pack_unseeded_components(&unseeded, &edges, seeded_bbox, &mut pos);

    resolve_collisions(&mut pos, &pinned_mask, &ids);

    // Normalise to a (0,0) min corner only for fully fresh layouts — any seed
    // (or pin) means the caller's coordinate frame is meaningful and preserved.
    if !pinned_mask.iter().any(|&p| p) && !ids.iter().any(|id| seeds.contains_key(id)) {
        let min_x = pos.iter().map(|p| p.0).fold(f64::INFINITY, f64::min);
        let min_y = pos.iter().map(|p| p.1).fold(f64::INFINITY, f64::min);
        for p in &mut pos {
            p.0 -= min_x;
            p.1 -= min_y;
        }
    }

    ids.iter()
        .enumerate()
        .map(|(i, id)| (*id, pos[i]))
        .collect()
}

/// Lay out one component that has at least one seeded member, in the global
/// coordinate frame. Seeded nodes start at their seed; unseeded members start
/// near their seeded neighbours (or the component's seed centroid). A mostly
/// seeded component is refined gently and re-anchored on its seed centroid so
/// the user's placement survives; a mostly new one gets a full anneal.
fn layout_seeded_component(
    comp: &[usize],
    comp_seeds: &[usize],
    edges: &[(usize, usize)],
    ids: &[Uuid],
    seeds: &HashMap<Uuid, (f64, f64)>,
    pinned_mask: &[bool],
    pos: &mut [(f64, f64)],
) {
    let seed_of = |i: usize| seeds.get(&ids[i]).copied();
    let centroid: (f64, f64) = {
        let (sx, sy) = comp_seeds
            .iter()
            .filter_map(|&i| seed_of(i))
            .fold((0.0, 0.0), |(ax, ay), (x, y)| (ax + x, ay + y));
        let c = comp_seeds.len() as f64;
        (sx / c, sy / c)
    };

    let comp_set: HashSet<usize> = comp.iter().copied().collect();
    let comp_edges: Vec<(usize, usize)> = edges
        .iter()
        .copied()
        .filter(|(a, b)| comp_set.contains(a) && comp_set.contains(b))
        .collect();
    for &i in comp {
        pos[i] = match seed_of(i) {
            Some(p) => p,
            None => {
                // Mean of seeded neighbours, else the seed centroid, plus
                // deterministic jitter so coincident starts can separate.
                let nbr: Vec<(f64, f64)> = edges
                    .iter()
                    .filter(|(a, b)| *a == i || *b == i)
                    .map(|&(a, b)| if a == i { b } else { a })
                    .filter(|j| comp_set.contains(j))
                    .filter_map(seed_of)
                    .collect();
                let base = if nbr.is_empty() {
                    centroid
                } else {
                    let (sx, sy) = nbr
                        .iter()
                        .fold((0.0, 0.0), |(ax, ay), (x, y)| (ax + x, ay + y));
                    (sx / nbr.len() as f64, sy / nbr.len() as f64)
                };
                let angle = hash01(&ids[i], 1) * std::f64::consts::TAU;
                let r = MIN_SPACING * (0.8 + hash01(&ids[i], 2));
                (base.0 + r * angle.cos(), base.1 + r * angle.sin())
            }
        };
    }

    // Gentle refinement keeps the user's chosen edge lengths as spring rest
    // lengths: zero force at the current arrangement, so deliberate long
    // bridges between clusters are preserved (not contracted toward
    // IDEAL_EDGE) and deliberately *tight* satellites are respected too —
    // the only floor is MIN_SPACING, the readability guarantee. Edges with a
    // new endpoint get the default rest length and settle at ring distance.
    let gentle = comp_seeds.len() * 2 >= comp.len();
    let rest_edges: Vec<(usize, usize, f64)> = comp_edges
        .iter()
        .map(|&(a, b)| {
            let rest = match (gentle, seed_of(a), seed_of(b)) {
                (true, Some(pa), Some(pb)) => (pa.0 - pb.0).hypot(pa.1 - pb.1).max(MIN_SPACING),
                _ => IDEAL_EDGE,
            };
            (a, b, rest)
        })
        .collect();
    force_refine(comp, &rest_edges, pos, pinned_mask, gentle);

    // Re-anchor: unless pins already fix the frame, translate so the seeded
    // members' centroid returns to where the user had it.
    if !comp.iter().any(|&i| pinned_mask[i]) {
        let (sx, sy) = comp_seeds
            .iter()
            .map(|&i| pos[i])
            .fold((0.0, 0.0), |(ax, ay), (x, y)| (ax + x, ay + y));
        let c = comp_seeds.len() as f64;
        let (dx, dy) = (centroid.0 - sx / c, centroid.1 - sy / c);
        for &i in comp {
            pos[i].0 += dx;
            pos[i].1 += dy;
        }
    }
}

/// Lay out fully-unseeded components locally (deterministic sunflower spiral
/// start → full anneal), then pack them in a grid below the seeded bounding
/// box (or at the origin when nothing is seeded).
fn pack_unseeded_components(
    comps: &[Vec<usize>],
    edges: &[(usize, usize)],
    seeded_bbox: Option<(f64, f64, f64, f64)>,
    pos: &mut [(f64, f64)],
) {
    if comps.is_empty() {
        return;
    }
    let no_pins = vec![false; pos.len()];
    // Local layout + bounding box per component.
    let mut boxes: Vec<(f64, f64)> = Vec::new(); // (w, h)
    for comp in comps {
        for (rank, &i) in comp.iter().enumerate() {
            let angle = rank as f64 * 2.399_963; // golden angle
            let r = MIN_SPACING * (rank as f64).sqrt();
            pos[i] = (r * angle.cos(), r * angle.sin());
        }
        let comp_set: HashSet<usize> = comp.iter().copied().collect();
        let rest_edges: Vec<(usize, usize, f64)> = edges
            .iter()
            .filter(|(a, b)| comp_set.contains(a) && comp_set.contains(b))
            .map(|&(a, b)| (a, b, IDEAL_EDGE))
            .collect();
        force_refine(comp, &rest_edges, pos, &no_pins, false);
        let min_x = comp.iter().map(|&i| pos[i].0).fold(f64::INFINITY, f64::min);
        let min_y = comp.iter().map(|&i| pos[i].1).fold(f64::INFINITY, f64::min);
        let max_x = comp
            .iter()
            .map(|&i| pos[i].0)
            .fold(f64::NEG_INFINITY, f64::max);
        let max_y = comp
            .iter()
            .map(|&i| pos[i].1)
            .fold(f64::NEG_INFINITY, f64::max);
        for &i in comp {
            pos[i].0 -= min_x;
            pos[i].1 -= min_y;
        }
        boxes.push((max_x - min_x, max_y - min_y));
    }

    // Grid: components in row-major order, cell sizes from per-column max
    // width / per-row max height so nothing can overlap a neighbour.
    let (base_x, base_y) = match seeded_bbox {
        Some((min_x, _, _, max_y)) => (min_x, max_y + COMPONENT_GAP),
        None => (0.0, 0.0),
    };
    let cols = (comps.len() as f64).sqrt().ceil() as usize;
    let rows = comps.len().div_ceil(cols);
    let mut col_w = vec![0.0_f64; cols];
    let mut row_h = vec![0.0_f64; rows];
    for (ci, &(w, h)) in boxes.iter().enumerate() {
        col_w[ci % cols] = col_w[ci % cols].max(w + COMPONENT_GAP);
        row_h[ci / cols] = row_h[ci / cols].max(h + COMPONENT_GAP);
    }
    for (ci, comp) in comps.iter().enumerate() {
        let ox = base_x + col_w[..ci % cols].iter().sum::<f64>();
        let oy = base_y + row_h[..ci / cols].iter().sum::<f64>();
        for &i in comp {
            pos[i].0 += ox;
            pos[i].1 += oy;
        }
    }
}

/// Spring-embedder refinement over one component.
///
/// `rest_edges` are `(i, j, rest_length)` triples already filtered to this
/// component. Two regimes:
/// - **Full** (`gentle = false`, fresh layouts): classic Fruchterman–Reingold
///   attraction `d²/k` toward IDEAL_EDGE, hot start — clusters form from
///   scratch.
/// - **Gentle** (`gentle = true`, seeded layouts): Hooke springs
///   `k·(d − rest)/rest` — zero force when an edge sits at its rest length,
///   so a hand-placed arrangement is a near-equilibrium and only local
///   overlaps get worked out. Cool start, fewer iterations.
///
/// Pinned nodes exert forces but never move.
fn force_refine(
    comp: &[usize],
    rest_edges: &[(usize, usize, f64)],
    pos: &mut [(f64, f64)],
    pinned_mask: &[bool],
    gentle: bool,
) {
    let m = comp.len();
    if m <= 1 {
        return;
    }

    let k = IDEAL_EDGE;
    // O(m²·iters): fine for realistic graphs; degrade iterations for huge ones.
    let base_iters: u32 = if gentle { 150 } else { 300 };
    let iters = if m > 300 {
        (base_iters * 300 * 300 / (m as u32 * m as u32)).max(40)
    } else {
        base_iters
    };
    let temp_start = if gentle { k * 0.6 } else { k * 3.0 };
    // Gentle mode resolves *local* crowding only: long-range repulsion would
    // accumulate across a large seeded layout and shove whole clusters around
    // (the exact mental-map damage gentle mode exists to avoid). Its strength
    // is also scaled down to the readability radius — full k² repulsion would
    // inflate deliberately tight hand-made satellite rings toward IDEAL_EDGE.
    let repulse_cutoff = if gentle { k * 1.25 } else { f64::INFINITY };
    let repulse_k2 = if gentle {
        MIN_SPACING * MIN_SPACING
    } else {
        k * k
    };

    let local_idx: HashMap<usize, usize> = comp.iter().enumerate().map(|(a, &i)| (i, a)).collect();

    for iter in 0..iters {
        let mut disp: Vec<(f64, f64)> = vec![(0.0, 0.0); m];
        // Repulsion between all pairs in the component.
        for a in 0..m {
            for b in (a + 1)..m {
                let (i, j) = (comp[a], comp[b]);
                let dx = pos[i].0 - pos[j].0;
                let dy = pos[i].1 - pos[j].1;
                let d = (dx * dx + dy * dy).sqrt().max(1.0);
                if d > repulse_cutoff {
                    continue;
                }
                let f = repulse_k2 / d;
                disp[a].0 += dx / d * f;
                disp[a].1 += dy / d * f;
                disp[b].0 -= dx / d * f;
                disp[b].1 -= dy / d * f;
            }
        }
        // Attraction along edges.
        for &(i, j, rest) in rest_edges {
            let (Some(&a), Some(&b)) = (local_idx.get(&i), local_idx.get(&j)) else {
                continue;
            };
            let dx = pos[i].0 - pos[j].0;
            let dy = pos[i].1 - pos[j].1;
            let d = (dx * dx + dy * dy).sqrt().max(1.0);
            let f = if gentle {
                // Hooke: signed, so a compressed spring pushes apart.
                k * (d - rest) / rest
            } else {
                d * d / k
            };
            disp[a].0 -= dx / d * f;
            disp[a].1 -= dy / d * f;
            disp[b].0 += dx / d * f;
            disp[b].1 += dy / d * f;
        }
        let temp = temp_start * (1.0 - iter as f64 / iters as f64).max(0.01);
        for (a, &i) in comp.iter().enumerate() {
            if pinned_mask[i] {
                continue;
            }
            let (dx, dy) = disp[a];
            let mag = (dx * dx + dy * dy).sqrt().max(0.001);
            let step = mag.min(temp);
            pos[i].0 += dx / mag * step;
            pos[i].1 += dy / mag * step;
        }
    }
}

/// Relaxation sweeps pushing any pair closer than `MIN_SPACING` apart.
/// Pinned nodes absorb no displacement (the other node takes the full push);
/// a pinned–pinned violation is left alone (the user's frame wins).
fn resolve_collisions(pos: &mut [(f64, f64)], pinned_mask: &[bool], ids: &[Uuid]) {
    let n = pos.len();
    for _sweep in 0..40 {
        let mut moved = false;
        for i in 0..n {
            for j in (i + 1)..n {
                if pinned_mask[i] && pinned_mask[j] {
                    continue;
                }
                let dx = pos[i].0 - pos[j].0;
                let dy = pos[i].1 - pos[j].1;
                let d = (dx * dx + dy * dy).sqrt();
                if d >= MIN_SPACING {
                    continue;
                }
                moved = true;
                // Direction to separate along; deterministic for coincident pairs.
                let (ux, uy) = if d > 1e-6 {
                    (dx / d, dy / d)
                } else {
                    let angle = hash01(&ids[i], 7 + j as u64) * std::f64::consts::TAU;
                    (angle.cos(), angle.sin())
                };
                let push = (MIN_SPACING - d) / 2.0 + 0.5;
                match (pinned_mask[i], pinned_mask[j]) {
                    (false, false) => {
                        pos[i].0 += ux * push;
                        pos[i].1 += uy * push;
                        pos[j].0 -= ux * push;
                        pos[j].1 -= uy * push;
                    }
                    (true, false) => {
                        pos[j].0 -= ux * push * 2.0;
                        pos[j].1 -= uy * push * 2.0;
                    }
                    (false, true) => {
                        pos[i].0 += ux * push * 2.0;
                        pos[i].1 += uy * push * 2.0;
                    }
                    (true, true) => {}
                }
            }
        }
        if !moved {
            break;
        }
    }
}

/// Connected components (indices), BFS in stable index order.
fn find_components(n: usize, edges: &[(usize, usize)]) -> Vec<Vec<usize>> {
    let mut adj: Vec<Vec<usize>> = vec![Vec::new(); n];
    for &(a, b) in edges {
        adj[a].push(b);
        adj[b].push(a);
    }
    let mut seen = vec![false; n];
    let mut comps = Vec::new();
    for start in 0..n {
        if seen[start] {
            continue;
        }
        let mut comp = vec![start];
        seen[start] = true;
        let mut head = 0;
        while head < comp.len() {
            let cur = comp[head];
            head += 1;
            for &nb in &adj[cur] {
                if !seen[nb] {
                    seen[nb] = true;
                    comp.push(nb);
                }
            }
        }
        comp.sort_unstable();
        comps.push(comp);
    }
    comps
}

/// Deterministic pseudo-random in [0, 1) from a UUID + salt (splitmix64).
fn hash01(id: &Uuid, salt: u64) -> f64 {
    let bits = id.as_u128();
    let mut z = (bits as u64) ^ ((bits >> 64) as u64) ^ salt.wrapping_mul(0x9E37_79B9_7F4A_7C15);
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^= z >> 31;
    (z >> 11) as f64 / (1u64 << 53) as f64
}

#[cfg(test)]
mod tests {
    use super::*;

    fn uuid(n: u128) -> Uuid {
        Uuid::from_u128(n)
    }

    /// A star: hub id 0, satellites 1..=count.
    fn star(hub: u128, first_sat: u128, count: u128) -> (Vec<Uuid>, Vec<(Uuid, Uuid)>) {
        let ids: Vec<Uuid> = std::iter::once(uuid(hub))
            .chain((first_sat..first_sat + count).map(uuid))
            .collect();
        let edges = ids[1..].iter().map(|s| (ids[0], *s)).collect();
        (ids, edges)
    }

    fn dist(a: (f64, f64), b: (f64, f64)) -> f64 {
        (a.0 - b.0).hypot(a.1 - b.1)
    }

    fn stddev(vals: &[f64]) -> f64 {
        let mean = vals.iter().sum::<f64>() / vals.len() as f64;
        (vals.iter().map(|v| (v - mean).powi(2)).sum::<f64>() / vals.len() as f64).sqrt()
    }

    #[test]
    fn empty_and_single_node() {
        let empty = cluster_layout(&[], &[], &HashMap::new(), &HashSet::new());
        assert!(empty.is_empty());

        let id = uuid(1);
        let one = cluster_layout(&[id], &[], &HashMap::new(), &HashSet::new());
        assert_eq!(one.len(), 1);

        let seeds = HashMap::from([(id, (42.0, 17.0))]);
        let pinned = HashSet::from([id]);
        let kept = cluster_layout(&[id], &[], &seeds, &pinned);
        assert_eq!(kept.get(&id), Some(&(42.0, 17.0)));
    }

    #[test]
    fn star_satellites_ring_the_hub_not_rows() {
        let (ids, edges) = star(0, 1, 12);
        let out = cluster_layout(&ids, &edges, &HashMap::new(), &HashSet::new());
        let hub = out[&ids[0]];
        let radii: Vec<f64> = ids[1..].iter().map(|s| dist(out[s], hub)).collect();
        for r in &radii {
            assert!(
                (IDEAL_EDGE * 0.5..IDEAL_EDGE * 3.0).contains(r),
                "satellite radius {r} outside ring band"
            );
        }
        // A row layout collapses one axis; a ring spreads both.
        let xs: Vec<f64> = ids[1..].iter().map(|s| out[s].0).collect();
        let ys: Vec<f64> = ids[1..].iter().map(|s| out[s].1).collect();
        assert!(
            stddev(&xs) > 40.0,
            "satellites collapsed in x: {}",
            stddev(&xs)
        );
        assert!(
            stddev(&ys) > 40.0,
            "satellites collapsed in y: {}",
            stddev(&ys)
        );
    }

    #[test]
    fn connected_pairs_end_closer_than_unconnected() {
        // Two 8-satellite stars joined by a single bridge edge.
        let (mut ids, mut edges) = star(0, 1, 8);
        let (ids2, edges2) = star(100, 101, 8);
        ids.extend(&ids2);
        edges.extend(edges2);
        edges.push((uuid(0), uuid(100)));

        let out = cluster_layout(&ids, &edges, &HashMap::new(), &HashSet::new());
        let mean = |pairs: Vec<f64>| pairs.iter().sum::<f64>() / pairs.len() as f64;
        let intra: Vec<f64> = (1..=8)
            .flat_map(|a| ((a + 1)..=8).map(move |b| (a, b)))
            .map(|(a, b)| dist(out[&uuid(a)], out[&uuid(b)]))
            .collect();
        let cross: Vec<f64> = (1..=8)
            .flat_map(|a| (101..=108).map(move |b| (a, b)))
            .map(|(a, b)| dist(out[&uuid(a)], out[&uuid(b)]))
            .collect();
        assert!(
            mean(intra) < mean(cross),
            "same-cluster nodes should sit closer than cross-cluster"
        );
        let (h1, h2) = (out[&uuid(0)], out[&uuid(100)]);
        assert!(dist(h1, h2) > IDEAL_EDGE, "cluster hubs should separate");
    }

    #[test]
    fn min_spacing_enforced_on_dense_clique() {
        let ids: Vec<Uuid> = (0..8).map(uuid).collect();
        let edges: Vec<(Uuid, Uuid)> = ids
            .iter()
            .enumerate()
            .flat_map(|(a, &ia)| ids[a + 1..].iter().map(move |&ib| (ia, ib)))
            .collect();
        let out = cluster_layout(&ids, &edges, &HashMap::new(), &HashSet::new());
        for (a, ia) in ids.iter().enumerate() {
            for ib in &ids[a + 1..] {
                let d = dist(out[ia], out[ib]);
                assert!(d >= MIN_SPACING * 0.95, "pair at {d} < MIN_SPACING");
            }
        }
    }

    #[test]
    fn deterministic_and_order_independent() {
        let (ids, edges) = star(0, 1, 10);
        let a = cluster_layout(&ids, &edges, &HashMap::new(), &HashSet::new());
        let b = cluster_layout(&ids, &edges, &HashMap::new(), &HashSet::new());
        assert_eq!(a, b);
        let mut rev: Vec<Uuid> = ids.clone();
        rev.reverse();
        let mut rev_edges: Vec<(Uuid, Uuid)> = edges.clone();
        rev_edges.reverse();
        let c = cluster_layout(&rev, &rev_edges, &HashMap::new(), &HashSet::new());
        assert_eq!(a, c);
    }

    #[test]
    fn pinned_nodes_do_not_move() {
        let (ids, edges) = star(0, 1, 6);
        // Seed everything in a plausible ring; pin the hub and one satellite.
        let mut seeds = HashMap::new();
        seeds.insert(ids[0], (1000.0, 1000.0));
        for (i, s) in ids[1..].iter().enumerate() {
            let angle = i as f64 / 6.0 * std::f64::consts::TAU;
            seeds.insert(
                *s,
                (1000.0 + 250.0 * angle.cos(), 1000.0 + 250.0 * angle.sin()),
            );
        }
        let pinned = HashSet::from([ids[0], ids[1]]);
        let out = cluster_layout(&ids, &edges, &seeds, &pinned);
        assert_eq!(out[&ids[0]], seeds[&ids[0]], "pinned hub moved");
        assert_eq!(out[&ids[1]], seeds[&ids[1]], "pinned satellite moved");
    }

    #[test]
    fn seeded_layout_preserves_mental_map() {
        // Two hand-placed stars far apart; refinement must keep left left,
        // right right, and not fling anything far from where the user put it.
        let (mut ids, mut edges) = star(0, 1, 8);
        let (ids2, edges2) = star(100, 101, 8);
        ids.extend(&ids2);
        edges.extend(edges2);
        edges.push((uuid(0), uuid(100)));

        let mut seeds = HashMap::new();
        for (hub, first, cx) in [(0u128, 1u128, 500.0), (100, 101, 2000.0)] {
            seeds.insert(uuid(hub), (cx, 800.0));
            for i in 0..8u128 {
                let angle = i as f64 / 8.0 * std::f64::consts::TAU;
                seeds.insert(
                    uuid(first + i),
                    (cx + 260.0 * angle.cos(), 800.0 + 260.0 * angle.sin()),
                );
            }
        }
        let out = cluster_layout(&ids, &edges, &seeds, &HashSet::new());
        for id in &ids {
            let moved = dist(out[id], seeds[id]);
            assert!(
                moved < IDEAL_EDGE * 1.5,
                "node {id} flung {moved}px from seed"
            );
        }
        assert!(
            out[&uuid(0)].0 < out[&uuid(100)].0 - 800.0,
            "cluster left/right order not preserved"
        );
    }

    #[test]
    fn unseeded_component_packed_beside_seeded_layout() {
        let (mut ids, mut edges) = star(0, 1, 6);
        let mut seeds = HashMap::new();
        seeds.insert(ids[0], (1000.0, 1000.0));
        for (i, s) in ids[1..].iter().enumerate() {
            let angle = i as f64 / 6.0 * std::f64::consts::TAU;
            seeds.insert(
                *s,
                (1000.0 + 250.0 * angle.cos(), 1000.0 + 250.0 * angle.sin()),
            );
        }
        // New, never-positioned triangle.
        let tri: Vec<Uuid> = (200..203).map(uuid).collect();
        ids.extend(&tri);
        edges.push((tri[0], tri[1]));
        edges.push((tri[1], tri[2]));
        edges.push((tri[2], tri[0]));

        let out = cluster_layout(&ids, &edges, &seeds, &HashSet::new());
        for t in &tri {
            for s in seeds.keys() {
                assert!(
                    dist(out[t], out[s]) >= MIN_SPACING,
                    "packed node overlaps star"
                );
            }
            assert!(
                dist(out[t], (1000.0, 1000.0)) < 4000.0,
                "packed component exiled too far away"
            );
        }
    }

    #[test]
    fn disconnected_components_do_not_overlap() {
        let mut ids: Vec<Uuid> = (0..3).map(uuid).collect();
        ids.extend((10..13).map(uuid));
        let edges = vec![
            (uuid(0), uuid(1)),
            (uuid(1), uuid(2)),
            (uuid(2), uuid(0)),
            (uuid(10), uuid(11)),
            (uuid(11), uuid(12)),
            (uuid(12), uuid(10)),
        ];
        let out = cluster_layout(&ids, &edges, &HashMap::new(), &HashSet::new());
        for a in 0..3u128 {
            for b in 10..13u128 {
                assert!(
                    dist(out[&uuid(a)], out[&uuid(b)]) >= MIN_SPACING,
                    "components overlap"
                );
            }
        }
    }

    #[test]
    fn fit_transform_frames_content() {
        // Empty → identity.
        assert_eq!(
            fit_transform(&HashMap::new(), 1280.0, 720.0),
            (0.0, 0.0, 1.0)
        );

        // A far-away cluster is brought into view: every node's screen
        // position (graph * zoom + pan) lands inside the viewport.
        let positions: HashMap<Uuid, (f64, f64)> = [
            (uuid(1), (2000.0, 1500.0)),
            (uuid(2), (2400.0, 1600.0)),
            (uuid(3), (2200.0, 1900.0)),
        ]
        .into_iter()
        .collect();
        let (pan_x, pan_y, zoom) = fit_transform(&positions, 1280.0, 720.0);
        for &(x, y) in positions.values() {
            let sx = x * zoom + pan_x;
            let sy = y * zoom + pan_y;
            assert!(
                (0.0..=1280.0).contains(&sx),
                "screen x {sx} out of viewport"
            );
            assert!((0.0..=720.0).contains(&sy), "screen y {sy} out of viewport");
        }
        assert!((0.5..=3.0).contains(&zoom));

        // Content much larger than the viewport hits the readability floor
        // (0.5) instead of shrinking further; it is still centred.
        let huge: HashMap<Uuid, (f64, f64)> =
            [(uuid(1), (0.0, 0.0)), (uuid(2), (10_000.0, 8_000.0))]
                .into_iter()
                .collect();
        let (hx, hy, hz) = fit_transform(&huge, 1280.0, 720.0);
        assert_eq!(hz, 0.5);
        let centre_sx = 5_000.0 * hz + hx;
        let centre_sy = 4_000.0 * hz + hy;
        assert!(
            (centre_sx - 640.0).abs() < 1.0,
            "not centred x: {centre_sx}"
        );
        assert!(
            (centre_sy - 360.0).abs() < 1.0,
            "not centred y: {centre_sy}"
        );
    }

    #[test]
    fn hub_ring_radius_grows_with_degree() {
        let (mut ids, mut edges) = star(0, 1, 20);
        let (small_ids, small_edges) = star(100, 101, 5);
        ids.extend(&small_ids);
        edges.extend(small_edges);
        let out = cluster_layout(&ids, &edges, &HashMap::new(), &HashSet::new());
        let mean_radius = |hub: u128, first: u128, count: u128| {
            let h = out[&uuid(hub)];
            (first..first + count)
                .map(|s| dist(out[&uuid(s)], h))
                .sum::<f64>()
                / count as f64
        };
        let big = mean_radius(0, 1, 20);
        let small = mean_radius(100, 101, 5);
        assert!(
            big > small * 1.1,
            "deg-20 ring ({big:.0}) should be wider than deg-5 ring ({small:.0})"
        );
    }
}
