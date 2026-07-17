/// Visual knowledge graph — SVG node-link diagram with hierarchical layout.
///
/// Fetches all nodes and edges, runs a BFS-layered smart layout that places
/// root nodes at the top and connected nodes in layers below.  An "Auto-arrange"
/// button re-computes positions.  Saved positions from the API are overlaid.
/// Nodes are draggable; positions are persisted to the DB on mouse-up.
/// The canvas supports pan (drag on background) and zoom (wheel).
///
/// Node shapes convey type: circle=Article, diamond=Project, rounded-rect=Area,
/// hexagon=Resource, triangle=Reference.  Each type has a distinct fill colour.
///
/// Edge lines convey type via colour + dash pattern; text labels are omitted.
/// Hovering an edge shows a pop-up card with the relationship type, direction,
/// and optional custom label.
///
/// SVG presentation attributes are set via `style` because Leptos 0.8 writes
/// `attr:` prefixes literally for unknown SVG elements.
use std::collections::{HashMap, HashSet};

use leptos::prelude::*;
use uuid::Uuid;
use wasm_bindgen_futures::spawn_local;
use web_sys::{MouseEvent, TouchEvent, WheelEvent};

use common::{
    edge::{CreateEdgeRequest, Edge, EdgeType},
    id::{EdgeId, NodeId, TagId},
    node::{Node, NodeType},
};

use crate::{
    api::{
        create_edge, delete_edge, fetch_all_edges, fetch_nodes, fetch_positions, save_position,
        save_positions,
    },
    components::node_meta::{status_color_hex, status_label, type_icon, type_label},
};
use leptos_router::hooks::use_navigate;

const W: f64 = 3000.0;
const H: f64 = 2000.0;
const MARGIN: f64 = 150.0;
/// Effective radius used for edge start/end offset (conservative for all shapes).
const NODE_R: f64 = 22.0;
const ARROW_OFFSET: f64 = NODE_R + 4.0;
/// Y offset of the title text below the node centre.
const TEXT_Y_OFFSET: f64 = 33.0;

/// Minimap overlay dimensions (pixels) and scale factors relative to the canvas.
const MINI_W: f64 = 160.0;
const MINI_H: f64 = 106.667; // 160 × (2000/3000)
const MINI_SCALE_X: f64 = MINI_W / W;
const MINI_SCALE_Y: f64 = MINI_H / H;

/// Minimum zoom (out) and maximum zoom (in).
const ZOOM_MIN: f64 = 0.05;
const ZOOM_MAX: f64 = 16.0;

// ── Hover card data ──────────────────────────────────────────────────────────

/// Data for the node summary card shown on hover.
#[derive(Clone)]
struct NodeHoverInfo {
    title: String,
    node_type: String,
    status: String,
    body_preview: Option<String>,
    node_id: Uuid,
}

#[derive(Clone)]
struct EdgeHover {
    edge_id: Uuid,
    type_label: &'static str,
    custom_label: Option<String>,
    src_title: String,
    tgt_title: String,
    src_id: Uuid,
    tgt_id: Uuid,
}

// ── Node-type helpers ────────────────────────────────────────────────────────

pub(crate) fn node_fill(nt: &NodeType) -> &'static str {
    match nt {
        NodeType::Article => "#d97706",
        NodeType::Project => "#2563eb",
        NodeType::Area => "#16a34a",
        NodeType::Resource => "#9333ea",
        NodeType::Reference => "#dc2626",
    }
}

pub(crate) fn node_stroke_color(nt: &NodeType) -> &'static str {
    match nt {
        NodeType::Article => "#92400e",
        NodeType::Project => "#1e40af",
        NodeType::Area => "#166534",
        NodeType::Resource => "#6b21a8",
        NodeType::Reference => "#991b1b",
    }
}

/// Diamond (rotated square) points for Project nodes.
pub(crate) fn diamond_points(cx: f64, cy: f64) -> String {
    let r = 22.0_f64;
    format!(
        "{:.1},{:.1} {:.1},{:.1} {:.1},{:.1} {:.1},{:.1}",
        cx,
        cy - r,
        cx + r,
        cy,
        cx,
        cy + r,
        cx - r,
        cy,
    )
}

/// Regular-hexagon points for Resource nodes.
pub(crate) fn hexagon_points(cx: f64, cy: f64) -> String {
    let r = 20.0_f64;
    (0..6)
        .map(|i| {
            let a = std::f64::consts::PI / 3.0 * i as f64 - std::f64::consts::PI / 6.0;
            format!("{:.1},{:.1}", cx + r * a.cos(), cy + r * a.sin())
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// Upward-pointing equilateral triangle for Reference nodes.
pub(crate) fn triangle_points(cx: f64, cy: f64) -> String {
    let r = 22.0_f64;
    let hx = r * 0.866_f64; // sqrt(3)/2
    format!(
        "{:.1},{:.1} {:.1},{:.1} {:.1},{:.1}",
        cx,
        cy - r,
        cx + hx,
        cy + r * 0.5,
        cx - hx,
        cy + r * 0.5,
    )
}

// ── Edge-type helpers ────────────────────────────────────────────────────────

fn edge_color(et: &EdgeType) -> &'static str {
    match et {
        EdgeType::References => "#d97706",
        EdgeType::Contains => "#22c55e",
        EdgeType::RelatedTo => "#a855f7",
        EdgeType::DependsOn => "#f97316",
        EdgeType::DerivedFrom => "#ec4899",
        EdgeType::WikiLink => "#60a5fa",
    }
}

/// CSS stroke-dasharray; "none" → solid line.
fn edge_dash(et: &EdgeType) -> &'static str {
    match et {
        EdgeType::References => "none",
        EdgeType::Contains => "none",
        EdgeType::RelatedTo => "8,4",
        EdgeType::DependsOn => "3,4",
        EdgeType::DerivedFrom => "8,3,2,3",
        EdgeType::WikiLink => "4,2",
    }
}

fn edge_stroke_width(et: &EdgeType) -> f64 {
    match et {
        EdgeType::Contains => 2.5,
        EdgeType::WikiLink => 1.0,
        _ => 1.5,
    }
}

fn edge_label(et: &EdgeType) -> &'static str {
    match et {
        EdgeType::References => "References",
        EdgeType::Contains => "Contains",
        EdgeType::RelatedTo => "Related to",
        EdgeType::DependsOn => "Depends on",
        EdgeType::DerivedFrom => "Derived from",
        EdgeType::WikiLink => "Wiki link",
    }
}

fn edge_marker_id(et: &EdgeType) -> &'static str {
    match et {
        EdgeType::References => "arrow-references",
        EdgeType::Contains => "arrow-contains",
        EdgeType::RelatedTo => "arrow-related-to",
        EdgeType::DependsOn => "arrow-depends-on",
        EdgeType::DerivedFrom => "arrow-derived-from",
        EdgeType::WikiLink => "arrow-wiki-link",
    }
}

// ── SVG marker injection ─────────────────────────────────────────────────────

fn inject_svg_markers() {
    let Some(win) = web_sys::window() else { return };
    let Some(doc) = win.document() else { return };
    let Some(svg) = doc.get_element_by_id("graph-svg") else {
        return;
    };
    if svg.query_selector("marker").ok().flatten().is_some() {
        return;
    }

    let ns = "http://www.w3.org/2000/svg";
    let Ok(defs) = doc.create_element_ns(Some(ns), "defs") else {
        return;
    };

    const ARROWS: &[(&str, &str)] = &[
        ("arrow-references", "#d97706"),
        ("arrow-contains", "#22c55e"),
        ("arrow-related-to", "#a855f7"),
        ("arrow-depends-on", "#f97316"),
        ("arrow-derived-from", "#ec4899"),
        ("arrow-wiki-link", "#60a5fa"),
    ];

    for (id, color) in ARROWS {
        let Ok(marker) = doc.create_element_ns(Some(ns), "marker") else {
            continue;
        };
        let _ = marker.set_attribute("id", id);
        let _ = marker.set_attribute("markerWidth", "8");
        let _ = marker.set_attribute("markerHeight", "6");
        let _ = marker.set_attribute("refX", "6");
        let _ = marker.set_attribute("refY", "3");
        let _ = marker.set_attribute("orient", "auto");

        let Ok(path) = doc.create_element_ns(Some(ns), "path") else {
            continue;
        };
        let _ = path.set_attribute("d", "M 0 0 L 6 3 L 0 6 Z");
        let _ = path.set_attribute("fill", color);
        let _ = marker.append_child(&path);
        let _ = defs.append_child(&marker);
    }

    if let Some(first) = svg.first_child() {
        let _ = svg.insert_before(&defs, Some(&first));
    } else {
        let _ = svg.append_child(&defs);
    }
}

// ── Misc helpers ─────────────────────────────────────────────────────────────

/// Truncate to at most `n` characters, appending "…" if clipped.
fn truncate(s: &str, n: usize) -> String {
    let mut chars = s.chars();
    let taken: String = chars.by_ref().take(n).collect();
    if chars.next().is_some() {
        format!("{taken}\u{2026}")
    } else {
        taken
    }
}

/// Strip Markdown syntax and return up to 100 chars as a plain-text preview.
fn node_body_preview(body: &str) -> Option<String> {
    let text: String = body
        .lines()
        .map(str::trim)
        .filter(|l| {
            !l.is_empty() && !l.starts_with('#') && !l.starts_with("```") && !l.starts_with("---")
        })
        .collect::<Vec<_>>()
        .join(" ");
    if text.is_empty() {
        return None;
    }
    let clean = text.replace("**", "").replace("__", "").replace('`', "");
    Some(truncate(&clean, 100))
}

/// Compute the SVG path `d` for an edge: starts at the source shape boundary,
/// ends just before the target so the arrowhead is visible.
fn compute_path(x1: f64, y1: f64, x2: f64, y2: f64) -> String {
    let dx = x2 - x1;
    let dy = y2 - y1;
    let len = (dx * dx + dy * dy).sqrt();
    if len < NODE_R + ARROW_OFFSET + 2.0 {
        return String::new();
    }
    let ux = dx / len;
    let uy = dy / len;
    format!(
        "M {:.1} {:.1} L {:.1} {:.1}",
        x1 + ux * NODE_R,
        y1 + uy * NODE_R,
        x2 - ux * ARROW_OFFSET,
        y2 - uy * ARROW_OFFSET,
    )
}

// ── Layout ───────────────────────────────────────────────────────────────────

fn force_layout_expanded(
    node_ids: &[Uuid],
    edge_pairs: &[(Uuid, Uuid)],
    cw: f64,
    ch: f64,
    margin: f64,
) -> HashMap<Uuid, (f64, f64)> {
    let n = node_ids.len();
    if n == 0 {
        return HashMap::new();
    }
    if n == 1 {
        let mut m = HashMap::new();
        m.insert(node_ids[0], (cw / 2.0, ch / 2.0));
        return m;
    }

    let uw = cw - 2.0 * margin;
    let uh = ch - 2.0 * margin;

    let mut px: Vec<f64> = (0..n)
        .map(|_| margin + js_sys::Math::random() * uw)
        .collect();
    let mut py: Vec<f64> = (0..n)
        .map(|_| margin + js_sys::Math::random() * uh)
        .collect();

    let k = (uw * uh / n as f64).sqrt();

    // Pre-compute id→index so the attractive-force loop is O(m) per iter,
    // not O(m·n) from `node_ids.iter().position(...)`. Edges to/from IDs
    // not in `node_ids` (orphans from deleted nodes) are skipped here too.
    let idx_of: HashMap<Uuid, usize> = node_ids
        .iter()
        .enumerate()
        .map(|(i, id)| (*id, i))
        .collect();
    let edge_idx: Vec<(usize, usize)> = edge_pairs
        .iter()
        .filter_map(|(src, tgt)| Some((*idx_of.get(src)?, *idx_of.get(tgt)?)))
        .collect();

    for iter in 0..200_u32 {
        let mut disp_x = vec![0.0_f64; n];
        let mut disp_y = vec![0.0_f64; n];

        for i in 0..n {
            for j in (i + 1)..n {
                let ddx = px[i] - px[j];
                let ddy = py[i] - py[j];
                let dist = (ddx * ddx + ddy * ddy).sqrt().max(1.0);
                let force = k * k / dist;
                let fx = ddx / dist * force;
                let fy = ddy / dist * force;
                disp_x[i] += fx;
                disp_y[i] += fy;
                disp_x[j] -= fx;
                disp_y[j] -= fy;
            }
        }

        for &(si, ti) in &edge_idx {
            let ddx = px[si] - px[ti];
            let ddy = py[si] - py[ti];
            let dist = (ddx * ddx + ddy * ddy).sqrt().max(1.0);
            let force = dist * dist / k;
            let fx = ddx / dist * force;
            let fy = ddy / dist * force;
            disp_x[si] -= fx;
            disp_y[si] -= fy;
            disp_x[ti] += fx;
            disp_y[ti] += fy;
        }

        let temp = 200.0_f64 * (1.0 - iter as f64 / 200.0).max(0.01);
        for i in 0..n {
            let mag = (disp_x[i] * disp_x[i] + disp_y[i] * disp_y[i])
                .sqrt()
                .max(0.001);
            let step = mag.min(temp);
            px[i] = (px[i] + disp_x[i] / mag * step).clamp(margin, cw - margin);
            py[i] = (py[i] + disp_y[i] / mag * step).clamp(margin, ch - margin);
        }
    }

    node_ids
        .iter()
        .enumerate()
        .map(|(i, id)| (*id, (px[i], py[i])))
        .collect()
}

// ── Smart Auto-Arrange Layout ────────────────────────────────────────────────

/// Minimum clearance per-node to prevent text/tag overlap.
/// Node shape radius (~22) + title pill (at cy+22..cy+36) + tag dots (at cy+42, r=5)
/// = ~69px below center, ~22px above center → ~91px total vertical envelope.
const NODE_H: f64 = 90.0;
/// Horizontal envelope: node diameter + title text width (~5px/char avg).
const NODE_W: f64 = 80.0;

/// Vertical spacing between BFS layers (center-to-center).
const LAYER_SPACING: f64 = 100.0;
/// Spacing between disconnected components (grid cell size).
const COMPONENT_SPACING: f64 = 200.0;

/// Result of smart layout computation: positions + auto-fit transform.
struct LayoutResult {
    positions: HashMap<Uuid, (f64, f64)>,
    fit_pan_x: f64,
    fit_pan_y: f64,
    fit_zoom: f64,
}

/// Compute the in-degree for each node from edge pairs.
fn compute_in_degree(node_ids: &[Uuid], edge_pairs: &[(Uuid, Uuid)]) -> HashMap<Uuid, usize> {
    let mut deg: HashMap<Uuid, usize> = node_ids.iter().map(|id| (*id, 0)).collect();
    for (_src, tgt) in edge_pairs {
        if let Some(d) = deg.get_mut(tgt) {
            *d += 1;
        }
    }
    deg
}

/// Build adjacency list (undirected) for BFS traversal.
fn build_adjacency(node_ids: &[Uuid], edge_pairs: &[(Uuid, Uuid)]) -> HashMap<Uuid, Vec<Uuid>> {
    let mut adj: HashMap<Uuid, Vec<Uuid>> = node_ids.iter().map(|id| (*id, Vec::new())).collect();
    for (src, tgt) in edge_pairs {
        if let Some(v) = adj.get_mut(src) {
            v.push(*tgt);
        }
        if let Some(v) = adj.get_mut(tgt) {
            v.push(*src);
        }
    }
    adj
}

/// Find connected components via BFS.
fn find_components(node_ids: &[Uuid], edge_pairs: &[(Uuid, Uuid)]) -> Vec<Vec<Uuid>> {
    let adj = build_adjacency(node_ids, edge_pairs);
    let mut visited = std::collections::HashSet::new();
    let mut components = Vec::new();

    for &nid in node_ids {
        if visited.contains(&nid) {
            continue;
        }
        let mut component = Vec::new();
        let mut queue = vec![nid];
        visited.insert(nid);
        while let Some(cur) = queue.pop() {
            component.push(cur);
            if let Some(neighbors) = adj.get(&cur) {
                for &nb in neighbors {
                    if visited.insert(nb) {
                        queue.push(nb);
                    }
                }
            }
        }
        components.push(component);
    }
    components
}

/// Assign BFS layers starting from root nodes (in-degree == 0).
/// If no roots exist, pick the highest-degree node as the starting point.
fn assign_layers(
    component: &[Uuid],
    edge_pairs: &[(Uuid, Uuid)],
    in_degree: &HashMap<Uuid, usize>,
) -> Vec<Vec<Uuid>> {
    // Find roots (in-degree 0) within this component.
    let comp_set: std::collections::HashSet<Uuid> = component.iter().copied().collect();
    let mut roots: Vec<Uuid> = component
        .iter()
        .filter(|id| in_degree.get(id).copied().unwrap_or(0) == 0)
        .copied()
        .collect();

    // If no roots, pick the node with highest total degree as artificial root.
    if roots.is_empty() {
        let mut deg: HashMap<Uuid, usize> = component.iter().map(|id| (*id, 0)).collect();
        for (src, tgt) in edge_pairs {
            if comp_set.contains(src) {
                *deg.entry(*src).or_default() += 1;
            }
            if comp_set.contains(tgt) {
                *deg.entry(*tgt).or_default() += 1;
            }
        }
        roots.push(
            component
                .iter()
                .max_by_key(|id| deg[id])
                .copied()
                .unwrap_or(component[0]),
        );
    }

    // BFS layering.
    let adj = build_adjacency(component, edge_pairs);
    let mut layers: Vec<Vec<Uuid>> = Vec::new();
    let mut visited = std::collections::HashSet::new();

    let current_layer = roots;
    for r in &current_layer {
        visited.insert(*r);
    }
    layers.push(current_layer);

    while let Some(layer) = layers.last() {
        let mut next_layer = Vec::new();
        for &nid in layer {
            if let Some(neighbors) = adj.get(&nid) {
                for &nb in neighbors {
                    if visited.insert(nb) {
                        next_layer.push(nb);
                    }
                }
            }
        }
        if next_layer.is_empty() {
            break;
        }
        layers.push(next_layer);
    }

    layers
}

/// Compute total degree for sorting within layers (hubs toward center).
fn compute_total_degree(node_ids: &[Uuid], edge_pairs: &[(Uuid, Uuid)]) -> HashMap<Uuid, usize> {
    let mut deg: HashMap<Uuid, usize> = node_ids.iter().map(|id| (*id, 0)).collect();
    for (src, tgt) in edge_pairs {
        if let Some(d) = deg.get_mut(src) {
            *d += 1;
        }
        if let Some(d) = deg.get_mut(tgt) {
            *d += 1;
        }
    }
    deg
}

/// Hierarchical initial placement for a single component.
/// Roots at top, BFS layers below, hubs centered within each layer.
/// Positions start at (origin_x, origin_y) with no overlap.
fn place_component(
    component: &[Uuid],
    edge_pairs: &[(Uuid, Uuid)],
    in_degree: &HashMap<Uuid, usize>,
    origin_x: f64,
    origin_y: f64,
) -> HashMap<Uuid, (f64, f64)> {
    let layers = assign_layers(component, edge_pairs, in_degree);
    let mut positions = HashMap::new();

    // Pre-compute total degree for all nodes (for hub sorting).
    let deg = compute_total_degree(component, edge_pairs);

    for (_li, layer) in layers.iter().enumerate() {
        let y = origin_y + _li as f64 * LAYER_SPACING;
        let n = layer.len() as f64;
        let layer_w = (n - 1.0) * NODE_W;
        // Center each layer within the component's max width.
        let start_x = origin_x - layer_w / 2.0;

        // Sort: hubs toward center, others outward.  Defensive lookup —
        // even with edge filtering at the smart_layout entry, a future caller
        // path could pass through here with a stray ID; default-to-0 avoids
        // a HashMap-index panic in production.
        let mut sorted = layer.clone();
        sorted.sort_by(|a, b| {
            let da = deg.get(a).copied().unwrap_or(0);
            let db = deg.get(b).copied().unwrap_or(0);
            db.cmp(&da)
        });

        for (i, nid) in sorted.iter().enumerate() {
            let x = start_x + i as f64 * NODE_W;
            positions.insert(*nid, (x, y));
        }
    }

    positions
}

/// Smart layout: hierarchical BFS layering with no force simulation.
/// Roots placed at top, layers fan out below, hubs centered within layers.
/// Disconnected components arranged in a grid.
/// Returns positions + auto-fit pan/zoom that guarantees readability.
fn smart_layout(
    nodes: &[Node],
    edge_pairs: &[(Uuid, Uuid)],
    viewport_w: f64,
    viewport_h: f64,
) -> LayoutResult {
    let node_ids: Vec<Uuid> = nodes.iter().map(|n| n.id.0).collect();
    let n = node_ids.len();
    if n == 0 {
        return LayoutResult {
            positions: HashMap::new(),
            fit_pan_x: 0.0,
            fit_pan_y: 0.0,
            fit_zoom: 1.0,
        };
    }
    if n == 1 {
        return LayoutResult {
            positions: [node_ids[0]].iter().map(|id| (*id, (0.0, 0.0))).collect(),
            fit_pan_x: viewport_w / 2.0 - NODE_W / 2.0,
            fit_pan_y: viewport_h / 2.0 - NODE_H / 2.0,
            fit_zoom: 1.0,
        };
    }

    // Drop orphan edges (endpoints not in the current node set). Without this,
    // BFS in `find_components` / `assign_layers` walks into UUIDs that have no
    // entry in `compute_total_degree`'s map, panicking the sort closure in
    // `place_component`. Orphans appear when an edge row outlives its node
    // (or when nodes are paginated but edges are not).
    let id_set: HashSet<Uuid> = node_ids.iter().copied().collect();
    let edge_pairs: Vec<(Uuid, Uuid)> = edge_pairs
        .iter()
        .copied()
        .filter(|(s, t)| id_set.contains(s) && id_set.contains(t))
        .collect();
    let edge_pairs = edge_pairs.as_slice();

    let in_degree = compute_in_degree(&node_ids, edge_pairs);
    let components = find_components(&node_ids, edge_pairs);

    // ── Phase 1: Hierarchical placement (no force simulation needed) ────────
    let mut all_positions = HashMap::new();

    if components.len() == 1 {
        let comp = &components[0];
        let pos = place_component(comp, edge_pairs, &in_degree, 0.0, 0.0);
        all_positions.extend(pos);
    } else {
        // Multiple components: arrange in a grid, each independently laid out.
        // Two-pass: first compute each component's bounding box and accumulate
        // per-column max widths + per-row max heights, then place with cumulative
        // offsets so wider/taller components in one cell cannot overlap neighbours.
        let cols = (components.len() as f64).sqrt().ceil() as usize;
        let rows = components.len().div_ceil(cols);
        struct CompPlacement {
            pos: HashMap<Uuid, (f64, f64)>,
            min_x: f64,
            min_y: f64,
        }
        let mut placements: Vec<CompPlacement> = Vec::new();
        let mut col_widths = vec![0.0_f64; cols];
        let mut row_heights = vec![0.0_f64; rows];

        for (ci, comp) in components.iter().enumerate() {
            let col = ci % cols;
            let row = ci / cols;
            let pos = place_component(comp, edge_pairs, &in_degree, 0.0, 0.0);
            let c_min_x = pos.values().map(|p| p.0).fold(f64::INFINITY, f64::min);
            let c_max_x = pos.values().map(|p| p.0).fold(f64::NEG_INFINITY, f64::max);
            let c_min_y = pos.values().map(|p| p.1).fold(f64::INFINITY, f64::min);
            let c_max_y = pos.values().map(|p| p.1).fold(f64::NEG_INFINITY, f64::max);
            let c_w = c_max_x - c_min_x + COMPONENT_SPACING;
            let c_h = c_max_y - c_min_y + COMPONENT_SPACING;
            col_widths[col] = col_widths[col].max(c_w);
            row_heights[row] = row_heights[row].max(c_h);
            placements.push(CompPlacement {
                pos,
                min_x: c_min_x,
                min_y: c_min_y,
            });
        }

        // Cumulative column/row offsets derived from the widest/tallest cell.
        let col_offsets: Vec<f64> = (0..cols).map(|i| col_widths[..i].iter().sum()).collect();
        let row_offsets: Vec<f64> = (0..rows).map(|i| row_heights[..i].iter().sum()).collect();

        for (ci, p) in placements.into_iter().enumerate() {
            let col = ci % cols;
            let row = ci / cols;
            let ox = col_offsets[col];
            let oy = row_offsets[row];
            for (nid, (px, py)) in p.pos {
                all_positions.insert(nid, (px - p.min_x + ox, py - p.min_y + oy));
            }
        }
    }

    // ── Phase 1.5: Translate so all positions fall inside drag-and-drop bounds ─
    // Drag clamping enforces [eff_margin, eff_{W,H} - eff_margin].  Auto-arrange
    // produces positions near (0,0) which is outside that range, causing a
    // visible snap the first time the user clicks a freshly-arranged node.
    {
        let grow_factor = (n as f64 / 50.0).clamp(1.0, 4.0);
        let eff_margin = MARGIN * grow_factor;
        let raw_min_x = all_positions
            .values()
            .map(|p| p.0)
            .fold(f64::INFINITY, f64::min);
        let raw_min_y = all_positions
            .values()
            .map(|p| p.1)
            .fold(f64::INFINITY, f64::min);
        let shift_x = eff_margin - raw_min_x;
        let shift_y = eff_margin - raw_min_y;
        for pos in all_positions.values_mut() {
            pos.0 += shift_x;
            pos.1 += shift_y;
        }
    }

    // ── Phase 2: Compute bounding box and auto-fit transform ────────────────
    let mut min_x = f64::MAX;
    let mut max_x = f64::MIN;
    let mut min_y = f64::MAX;
    let mut max_y = f64::MIN;
    for &(x, y) in all_positions.values() {
        min_x = min_x.min(x - NODE_W / 2.0);
        max_x = max_x.max(x + NODE_W / 2.0);
        min_y = min_y.min(y - NODE_H / 2.0);
        max_y = max_y.max(y + NODE_H / 2.0);
    }

    let graph_w = max_x - min_x;
    let graph_h = max_y - min_y;
    let graph_cx = (min_x + max_x) / 2.0;
    let graph_cy = (min_y + max_y) / 2.0;

    // Fit the graph into the viewport with padding.
    let padding = 60.0;
    let fit_w = viewport_w - padding * 2.0;
    let fit_h = viewport_h - padding * 2.0;
    // Zoom: fit the graph, but never go below 0.5x (nodes stay readable).
    let fit_zoom = (fit_w / graph_w).min(fit_h / graph_h).clamp(0.5, 3.0);
    // Center the graph in the viewport.
    let fit_pan_x = viewport_w / 2.0 - graph_cx * fit_zoom;
    let fit_pan_y = viewport_h / 2.0 - graph_cy * fit_zoom;

    LayoutResult {
        positions: all_positions,
        fit_pan_x,
        fit_pan_y,
        fit_zoom,
    }
}

// ── Component ────────────────────────────────────────────────────────────────

#[component]
pub fn GraphView() -> impl IntoView {
    let navigate = use_navigate();
    let loading = RwSignal::new(true);
    let error_msg = RwSignal::new(Option::<String>::None);
    let nodes_sig: RwSignal<Vec<Node>> = RwSignal::new(vec![]);
    let edges_sig: RwSignal<Vec<Edge>> = RwSignal::new(vec![]);
    let positions: RwSignal<HashMap<Uuid, (f64, f64)>> = RwSignal::new(HashMap::new());

    let pan_x = RwSignal::new(0.0_f64);
    let pan_y = RwSignal::new(0.0_f64);
    let zoom = RwSignal::new(1.0_f64);
    // Editable zoom percentage input (synced bidirectionally with `zoom`).
    let zoom_input: RwSignal<String> = RwSignal::new("100".to_string());
    let panning = RwSignal::new(false);
    let last_mx = RwSignal::new(0.0_f64);
    let last_my = RwSignal::new(0.0_f64);

    // Touch state — single-finger pan + two-finger pinch-to-zoom.
    let last_tx = RwSignal::new(0.0_f64);
    let last_ty = RwSignal::new(0.0_f64);
    let pinch_start_dist = RwSignal::new(0.0_f64);
    let pinch_start_zoom = RwSignal::new(1.0_f64);

    let drag_node: RwSignal<Option<Uuid>> = RwSignal::new(None);
    let drag_offset: RwSignal<(f64, f64)> = RwSignal::new((0.0, 0.0));
    let did_drag = RwSignal::new(false);

    // Re-layout trigger: when set to true, re-runs the force simulation.
    let re_layout: RwSignal<bool> = RwSignal::new(false);
    // Loading indicator for smart layout computation.
    let re_layouting: RwSignal<bool> = RwSignal::new(false);

    let edge_hover: RwSignal<Option<EdgeHover>> = RwSignal::new(None);
    let node_hover: RwSignal<Option<NodeHoverInfo>> = RwSignal::new(None);

    // ── Edge-create mode signals ──────────────────────────────────────────────
    // Activated by the "Add Edge" toolbar button.
    let edge_create_mode = RwSignal::new(false);
    // First node clicked while in edge-create mode becomes the source.
    let edge_src: RwSignal<Option<Uuid>> = RwSignal::new(None);
    // When both src and tgt are chosen, this holds (src, tgt) to show the popup.
    let edge_pair: RwSignal<Option<(Uuid, Uuid)>> = RwSignal::new(None);
    // Popup form state.
    let new_edge_type = RwSignal::new("references".to_string());
    let new_edge_label = RwSignal::new(String::new());
    let edge_saving = RwSignal::new(false);

    // ── Type-visibility filter signals ───────────────────────────────────────
    // Each signal controls whether nodes of that type are rendered.
    let show_articles: RwSignal<bool> = RwSignal::new(true);
    let show_projects: RwSignal<bool> = RwSignal::new(true);
    let show_areas: RwSignal<bool> = RwSignal::new(true);
    let show_resources: RwSignal<bool> = RwSignal::new(true);
    let show_references: RwSignal<bool> = RwSignal::new(true);

    // ── Orphans filter ───────────────────────────────────────────────────────
    // When on, only nodes with no edges at all are shown — a maintenance lens
    // for spotting notes that never got linked into the graph.
    let show_orphans_only: RwSignal<bool> = RwSignal::new(false);

    // ── Tag filter signal ────────────────────────────────────────────────────
    // When Some(tag_id), only nodes with that tag are rendered.
    // Set by clicking a tag dot; cleared by clicking the same dot or the × button.
    let tag_filter: RwSignal<Option<TagId>> = RwSignal::new(None);
    // Guard: prevents the <g> on:click from firing after a dot click
    // (Leptos event delegation fires parent handlers even after stop_propagation).
    let dot_clicked = RwSignal::new(false);

    Effect::new(move |_| {
        spawn_local(async move {
            match fetch_nodes().await {
                Err(e) => {
                    error_msg.set(Some(format!("{e}")));
                    loading.set(false);
                }
                Ok(nodes) => {
                    let edges = fetch_all_edges().await.unwrap_or_default();
                    let node_ids: Vec<Uuid> = nodes.iter().map(|n| n.id.0).collect();
                    let edge_pairs: Vec<(Uuid, Uuid)> = edges
                        .iter()
                        .map(|e| (e.source_id.0, e.target_id.0))
                        .collect();

                    // Auto-grow: increase effective canvas area based on node count.
                    let n = node_ids.len() as f64;
                    let grow_factor = (n / 50.0).clamp(1.0, 4.0);
                    let eff_w = W * grow_factor;
                    let eff_h = H * grow_factor;
                    let eff_margin = MARGIN * grow_factor;

                    let mut layout =
                        force_layout_expanded(&node_ids, &edge_pairs, eff_w, eff_h, eff_margin);

                    if let Ok(saved) = fetch_positions().await {
                        for pos in saved {
                            layout.insert(pos.node_id.0, (pos.x, pos.y));
                        }
                    }

                    positions.set(layout);
                    nodes_sig.set(nodes);
                    edges_sig.set(edges);
                    loading.set(false);

                    spawn_local(async {
                        gloo_timers::future::TimeoutFuture::new(50).await;
                        inject_svg_markers();
                    });
                }
            }
        });
    });

    // ── Re-layout effect: runs smart layout, overriding ALL saved positions ──
    Effect::new(move |_| {
        if !re_layout.get() {
            return;
        }
        re_layouting.set(true);
        let nodes = nodes_sig.get_untracked();
        let edges = edges_sig.get_untracked();

        // Use a timeout to let the spinner render before blocking computation.
        spawn_local(async move {
            gloo_timers::future::TimeoutFuture::new(80).await;

            let edge_pairs: Vec<(Uuid, Uuid)> = edges
                .iter()
                .map(|e| (e.source_id.0, e.target_id.0))
                .collect();

            let viewport_w = web_sys::window()
                .and_then(|w| w.inner_width().ok())
                .and_then(|v| v.as_f64())
                .unwrap_or(1200.0);
            let viewport_h = web_sys::window()
                .and_then(|w| w.inner_height().ok())
                .and_then(|v| v.as_f64())
                .unwrap_or(800.0);

            let result = smart_layout(&nodes, &edge_pairs, viewport_w, viewport_h);

            // Persist all positions to the DB in a single batch call.
            let batch: Vec<(NodeId, f64, f64)> = result
                .positions
                .iter()
                .map(|(id, (x, y))| (NodeId(*id), *x, *y))
                .collect();
            if let Err(e) = save_positions(&batch).await {
                tracing::error!("Failed to save auto-arrange positions: {e}");
            }

            positions.set(result.positions);
            pan_x.set(result.fit_pan_x);
            pan_y.set(result.fit_pan_y);
            zoom.set(result.fit_zoom);
            zoom_input.set(format!("{:.0}", result.fit_zoom * 100.0));

            re_layout.set(false);
            re_layouting.set(false);

            spawn_local(async {
                gloo_timers::future::TimeoutFuture::new(50).await;
                inject_svg_markers();
            });
        });
    });

    view! {
        <div class="relative w-full h-full overflow-hidden bg-stone-50 dark:bg-stone-950 select-none">
            // ── Legend overlay ───────────────────────────────────────────────
            // ── Toolbar (top-right) ──────────────────────────────────────────
            // Single unified container: action buttons + zoom controls,
            // all sharing the same visual language.
            <div class="absolute top-3 right-3 z-10 flex items-center gap-0
                        bg-white/85 dark:bg-stone-900/90 backdrop-blur-md
                        border border-stone-200 dark:border-stone-700
                        rounded-xl overflow-hidden shadow-lg">
                // ── Action buttons ─────────────────────────────────────────
                // Add Edge button — toggles edge-create mode.
                <button
                    class=move || {
                        let base = "h-8 px-2.5 text-xs font-medium \
                                    cursor-pointer transition-colors flex items-center gap-1 \
                                    border-r border-stone-200 dark:border-stone-700";
                        if edge_create_mode.get() {
                            format!("{base} bg-amber-500/90 text-white")
                        } else {
                            format!("{base} text-stone-600 dark:text-stone-300 \
                                     hover:bg-stone-50 dark:hover:bg-stone-800")
                        }
                    }
                    title="Add a new edge between two nodes (click source then target)"
                    on:click=move |_| {
                        let entering = !edge_create_mode.get_untracked();
                        edge_create_mode.set(entering);
                        if !entering {
                            edge_src.set(None);
                            edge_pair.set(None);
                        }
                    }
                >
                    <span class="material-symbols-outlined" style="font-size: 14px;">"add_link"</span>
                    {move || if edge_create_mode.get() { "Cancel" } else { "Add Edge" }}
                </button>
                // Fit button
                <button
                    class="h-8 px-2.5 text-xs font-medium
                           text-stone-600 dark:text-stone-300
                           hover:bg-stone-50 dark:hover:bg-stone-800 cursor-pointer transition-colors
                           border-r border-stone-200 dark:border-stone-700"
                    title="Fit all nodes into view"
                    on:click=move |_| {
                        pan_x.set(0.0);
                        pan_y.set(0.0);
                        zoom.set(1.0);
                        zoom_input.set("100".to_string());
                    }
                >
                    "Fit"
                </button>
                // Auto-arrange button
                <button
                    class=move || {
                        let base = "h-8 px-2.5 text-xs font-medium \
                                    cursor-pointer transition-colors \
                                    border-r border-stone-200 dark:border-stone-700";
                        if re_layouting.get() {
                            format!("{base} bg-amber-500/90 text-white opacity-70 cursor-wait")
                        } else {
                            format!("{base} text-stone-600 dark:text-stone-300 \
                                     hover:bg-stone-50 dark:hover:bg-stone-800")
                        }
                    }
                    title="Auto-arrange nodes to prevent overlap (overrides manual positions)"
                    disabled=move || re_layouting.get()
                    on:click=move |_| {
                        re_layout.set(true);
                    }
                >
                    {move || {
                        if re_layouting.get() {
                            view! {
                                <span class="flex items-center gap-1">
                                    <span class="inline-block animate-spin" style="font-size: 12px;">"⟳"</span>
                                    "Arranging…"
                                </span>
                            }
                            .into_any()
                        } else {
                            view! { "Auto-arrange" }.into_any()
                        }
                    }}
                </button>

                // ── Zoom controls ──────────────────────────────────────────
                // Zoom-out button | manual input | Zoom-in button
                <div class="flex items-center">
                    <button
                        class="h-8 w-8 flex items-center justify-center
                               text-stone-500 dark:text-stone-400
                               hover:bg-stone-100 dark:hover:bg-stone-800
                               hover:text-stone-700 dark:hover:text-stone-200
                               cursor-pointer transition-colors"
                        title="Zoom out (−)"
                        on:click=move |_| {
                            zoom.update(|z| *z = (*z * 0.8).clamp(ZOOM_MIN, ZOOM_MAX));
                            zoom_input.set(format!("{:.0}", zoom.get_untracked() * 100.0));
                        }
                    >
                        <span class="material-symbols-outlined" style="font-size: 16px;">"remove"</span>
                    </button>
                    // Manual zoom input — type a percentage and press Enter.
                    <input
                        type="number"
                        min="5"
                        max="1600"
                        step="5"
                        class="w-14 h-6 text-center text-xs font-medium
                               bg-transparent text-stone-700 dark:text-stone-200
                               border-x border-stone-200 dark:border-stone-700
                               focus:outline-none focus:bg-stone-100 dark:focus:bg-stone-800
                               tabular-nums appearance-none
                               [&::-webkit-inner-spin-button]:appearance-none
                               [&::-webkit-outer-spin-button]:appearance-none
                               [-moz-appearance:textfield]"
                        prop:value=move || zoom_input.get()
                        on:keydown=move |ev: web_sys::KeyboardEvent| {
                            if ev.key() == "Enter" {
                                let val = event_target_value(&ev);
                                if let Ok(pct) = val.parse::<f64>() {
                                    let new_zoom = (pct / 100.0).clamp(ZOOM_MIN, ZOOM_MAX);
                                    zoom.set(new_zoom);
                                    zoom_input.set(format!("{:.0}", pct));
                                }
                                ev.prevent_default();
                            }
                        }
                        on:blur=move |ev: web_sys::FocusEvent| {
                            // On blur, snap to current zoom if invalid.
                            let val = event_target_value(&ev);
                            if let Ok(pct) = val.parse::<f64>() {
                                let new_zoom = (pct / 100.0).clamp(ZOOM_MIN, ZOOM_MAX);
                                zoom.set(new_zoom);
                                zoom_input.set(format!("{:.0}", new_zoom * 100.0));
                            } else {
                                zoom_input.set(format!("{:.0}", zoom.get_untracked() * 100.0));
                            }
                        }
                    />
                    <button
                        class="h-8 w-8 flex items-center justify-center
                               text-stone-500 dark:text-stone-400
                               hover:bg-stone-100 dark:hover:bg-stone-800
                               hover:text-stone-700 dark:hover:text-stone-200
                               cursor-pointer transition-colors"
                        title="Zoom in (+)"
                        on:click=move |_| {
                            zoom.update(|z| *z = (*z * 1.25).clamp(ZOOM_MIN, ZOOM_MAX));
                            zoom_input.set(format!("{:.0}", zoom.get_untracked() * 100.0));
                        }
                    >
                        <span class="material-symbols-outlined" style="font-size: 16px;">"add"</span>
                    </button>
                    <span class="px-1.5 text-[10px] font-medium text-stone-400 dark:text-stone-500
                               select-none">"%"</span>
                </div>
            </div>

            // ── Add Edge popup ───────────────────────────────────────────────
            // Shows when both source and target have been selected in edge-create mode.
            {move || edge_pair.get().map(|(_src_id, _tgt_id)| {
                view! {
                    <div class="absolute inset-0 z-20 flex items-center justify-center pointer-events-none">
                        <div
                            class="pointer-events-auto bg-white dark:bg-stone-900
                                   rounded-xl shadow-2xl border border-stone-200 dark:border-stone-700
                                   p-5 flex flex-col gap-3 w-72"
                            on:click=|ev: MouseEvent| ev.stop_propagation()
                        >
                            <h3 class="text-sm font-semibold text-stone-900 dark:text-stone-100">
                                "New Edge"
                            </h3>
                            // Edge type select
                            <div class="flex flex-col gap-1">
                                <label class="text-xs text-stone-500 dark:text-stone-400 uppercase tracking-wide font-medium">
                                    "Type"
                                </label>
                                <select
                                    class="w-full px-3 py-1.5 rounded-lg text-sm
                                           bg-stone-50 dark:bg-stone-800
                                           border border-stone-200 dark:border-stone-700
                                           text-stone-900 dark:text-stone-100
                                           focus:outline-none focus:ring-2 focus:ring-amber-500"
                                    prop:value=move || new_edge_type.get()
                                    on:change=move |ev| new_edge_type.set(event_target_value(&ev))
                                >
                                    <option value="references">"References"</option>
                                    <option value="contains">"Contains"</option>
                                    <option value="related_to">"Related to"</option>
                                    <option value="depends_on">"Depends on"</option>
                                    <option value="derived_from">"Derived from"</option>
                                </select>
                            </div>
                            // Optional label
                            <div class="flex flex-col gap-1">
                                <label class="text-xs text-stone-500 dark:text-stone-400 uppercase tracking-wide font-medium">
                                    "Label "
                                    <span class="normal-case font-normal">"(optional)"</span>
                                </label>
                                <input
                                    type="text"
                                    placeholder="e.g. uses, extends…"
                                    class="w-full px-3 py-1.5 rounded-lg text-sm
                                           bg-stone-50 dark:bg-stone-800
                                           border border-stone-200 dark:border-stone-700
                                           text-stone-900 dark:text-stone-100
                                           placeholder-stone-400
                                           focus:outline-none focus:ring-2 focus:ring-amber-500"
                                    prop:value=move || new_edge_label.get()
                                    on:input=move |ev| new_edge_label.set(event_target_value(&ev))
                                />
                            </div>
                            // Buttons
                            <div class="flex gap-2 justify-end pt-1">
                                <button
                                    class="px-3 py-1.5 text-sm rounded-lg
                                           text-stone-600 dark:text-stone-400
                                           hover:bg-stone-100 dark:hover:bg-stone-800 transition-colors"
                                    on:click=move |_| {
                                        edge_pair.set(None);
                                        edge_src.set(None);
                                        edge_create_mode.set(false);
                                        new_edge_type.set("references".to_string());
                                        new_edge_label.set(String::new());
                                    }
                                >
                                    "Cancel"
                                </button>
                                <button
                                    class="px-3 py-1.5 text-sm font-medium rounded-lg
                                           bg-amber-600 hover:bg-amber-700 text-white
                                           disabled:opacity-50 disabled:cursor-not-allowed transition-colors"
                                    disabled=move || edge_saving.get()
                                    on:click=move |_| {
                                        let Some((src, tgt)) = edge_pair.get_untracked() else { return };
                                        let et = match new_edge_type.get_untracked().as_str() {
                                            "contains"     => EdgeType::Contains,
                                            "related_to"   => EdgeType::RelatedTo,
                                            "depends_on"   => EdgeType::DependsOn,
                                            "derived_from" => EdgeType::DerivedFrom,
                                            _              => EdgeType::References,
                                        };
                                        let lbl = {
                                            let s = new_edge_label.get_untracked();
                                            if s.trim().is_empty() { None } else { Some(s) }
                                        };
                                        let req = CreateEdgeRequest {
                                            source_id: NodeId(src),
                                            target_id: NodeId(tgt),
                                            edge_type: et,
                                            label: lbl,
                                        };
                                        edge_saving.set(true);
                                        spawn_local(async move {
                                            if let Ok(new_edge) = create_edge(&req).await {
                                                edges_sig.update(|v| v.push(new_edge));
                                            }
                                            edge_saving.set(false);
                                            edge_pair.set(None);
                                            edge_src.set(None);
                                            edge_create_mode.set(false);
                                            new_edge_type.set("references".to_string());
                                            new_edge_label.set(String::new());
                                        });
                                    }
                                >
                                    {move || if edge_saving.get() { "Saving…" } else { "Create" }}
                                </button>
                            </div>
                        </div>
                    </div>
                }
            })}

            // ── Legend + type filter overlay (bottom-left) ───────────────────
            <div class="absolute bottom-4 left-4 z-10
                        bg-white/80 dark:bg-stone-900/85 backdrop-blur-sm
                        border border-stone-200 dark:border-stone-700
                        rounded-xl p-3 flex flex-col gap-2 text-xs">
                // Node type toggles — click to show/hide
                <p class="font-semibold text-stone-500 dark:text-stone-400 uppercase tracking-wide
                           text-[10px] mb-0.5">"Nodes (click to filter)"</p>
                <div class="flex flex-col gap-1">
                    <LegendToggle label="Article"   color="#d97706" shape="circle"   show=show_articles />
                    <LegendToggle label="Project"   color="#2563eb" shape="diamond"  show=show_projects />
                    <LegendToggle label="Area"      color="#16a34a" shape="rect"     show=show_areas />
                    <LegendToggle label="Resource"  color="#9333ea" shape="hexagon"  show=show_resources />
                    <LegendToggle label="Reference" color="#dc2626" shape="triangle" show=show_references />
                </div>
                // Orphans-only maintenance lens.
                <button
                    class=move || {
                        let base = "flex items-center gap-1.5 mt-1 pt-1 border-t \
                                    border-stone-200 dark:border-stone-700 cursor-pointer \
                                    text-[10px] font-semibold uppercase tracking-wide";
                        if show_orphans_only.get() {
                            format!("{base} text-amber-600 dark:text-amber-400")
                        } else {
                            format!("{base} text-stone-500 dark:text-stone-400 \
                                     hover:text-stone-700 dark:hover:text-stone-200")
                        }
                    }
                    title="Show only nodes with no links (good for cleanup)"
                    on:click=move |_| show_orphans_only.update(|v| *v = !*v)
                >
                    <span class="material-symbols-outlined" style="font-size:12px;">"link_off"</span>
                    {move || if show_orphans_only.get() { "Orphans only ✓" } else { "Orphans only" }}
                </button>
                // Tag filter active indicator — shown when a tag dot has been clicked.
                {move || tag_filter.get().map(|_| view! {
                    <div class="flex items-center gap-1.5 mt-1 pt-1
                                border-t border-stone-200 dark:border-stone-700">
                        <span class="text-amber-600 dark:text-amber-400 text-[10px] font-semibold
                                     uppercase tracking-wide">"Tag filter active"</span>
                        <button
                            class="ml-auto text-stone-400 hover:text-stone-600 dark:hover:text-stone-200
                                   text-xs font-bold leading-none"
                            title="Clear tag filter"
                            on:click=move |_| tag_filter.set(None)
                        >"×"</button>
                    </div>
                })}
                // Edge line styles (non-interactive)
                <p class="font-semibold text-stone-500 dark:text-stone-400 uppercase tracking-wide
                           text-[10px] mt-1 mb-0.5">"Edges"</p>
                <div class="flex flex-col gap-1">
                    <LegendEdge label="References"  color="#d97706" dash="none"    width="1.5" />
                    <LegendEdge label="Contains"    color="#22c55e" dash="none"    width="2.5" />
                    <LegendEdge label="Related to"  color="#a855f7" dash="8,4"     width="1.5" />
                    <LegendEdge label="Depends on"  color="#f97316" dash="3,4"     width="1.5" />
                    <LegendEdge label="Derived from" color="#ec4899" dash="8,3,2,3" width="1.5" />
                    <LegendEdge label="Wiki link"   color="#60a5fa" dash="4,2"     width="1.0" />
                </div>
            </div>

            // ── Graph minimap (bottom-right) ─────────────────────────────────
            // Small fixed-size overview of the full W×H canvas. Nodes are
            // coloured dots; edges are faint lines. An amber viewport rect
            // shows the currently visible area. Clicking pans the main view
            // to centre on the clicked graph coordinate.
            {move || {
                if loading.get() || nodes_sig.get().is_empty() {
                    return None;
                }
                let nodes = nodes_sig.get();
                let edges = edges_sig.get();
                let pos = positions.get();

                let edge_lines: Vec<AnyView> = edges
                    .iter()
                    .filter_map(|e| {
                        let (x1, y1) = pos.get(&e.source_id.0).copied()?;
                        let (x2, y2) = pos.get(&e.target_id.0).copied()?;
                        let mx1 = x1 * MINI_SCALE_X;
                        let my1 = y1 * MINI_SCALE_Y;
                        let mx2 = x2 * MINI_SCALE_X;
                        let my2 = y2 * MINI_SCALE_Y;
                        Some(
                            view! {
                                <line
                                    x1=format!("{mx1:.1}")
                                    y1=format!("{my1:.1}")
                                    x2=format!("{mx2:.1}")
                                    y2=format!("{my2:.1}")
                                    style="stroke: rgba(255,255,255,0.30); stroke-width: 0.5;"
                                />
                            }
                            .into_any(),
                        )
                    })
                    .collect();

                let node_dots: Vec<AnyView> = nodes
                    .iter()
                    .map(|n| {
                        let (nx, ny) =
                            pos.get(&n.id.0).copied().unwrap_or((W / 2.0, H / 2.0));
                        let mx = nx * MINI_SCALE_X;
                        let my = ny * MINI_SCALE_Y;
                        let color = node_fill(&n.node_type);
                        view! {
                            <circle
                                cx=format!("{mx:.1}")
                                cy=format!("{my:.1}")
                                r="2.5"
                                style=format!("fill: {color};")
                            />
                        }
                        .into_any()
                    })
                    .collect();

                Some(view! {
                    <div
                        class="absolute top-16 right-3 z-10 rounded-lg overflow-hidden \
                               border border-stone-600/50 shadow-lg"
                        style="background: rgba(12,12,12,0.80); cursor: crosshair;"
                        on:click=move |ev: MouseEvent| {
                            ev.stop_propagation();
                            let mx = ev.offset_x() as f64;
                            let my = ev.offset_y() as f64;
                            let gx = mx / MINI_SCALE_X;
                            let gy = my / MINI_SCALE_Y;
                            let z = zoom.get_untracked();
                            let vw = web_sys::window()
                                .and_then(|w| w.inner_width().ok())
                                .and_then(|v| v.as_f64())
                                .unwrap_or(1200.0);
                            let vh = web_sys::window()
                                .and_then(|w| w.inner_height().ok())
                                .and_then(|v| v.as_f64())
                                .unwrap_or(800.0);
                            pan_x.set(vw / 2.0 - gx * z);
                            pan_y.set(vh / 2.0 - gy * z);
                        }
                    >
                        <svg
                            width=format!("{}", MINI_W as u32)
                            height=format!("{}", MINI_H as u32)
                            style="display: block; pointer-events: none;"
                        >
                            // Viewport indicator — reactive to pan & zoom changes.
                            <rect
                                x=move || {
                                    format!("{:.1}", -pan_x.get() / zoom.get() * MINI_SCALE_X)
                                }
                                y=move || {
                                    format!("{:.1}", -pan_y.get() / zoom.get() * MINI_SCALE_Y)
                                }
                                width=move || {
                                    let vw = web_sys::window()
                                        .and_then(|w| w.inner_width().ok())
                                        .and_then(|v| v.as_f64())
                                        .unwrap_or(1200.0);
                                    format!("{:.1}", vw / zoom.get() * MINI_SCALE_X)
                                }
                                height=move || {
                                    let vh = web_sys::window()
                                        .and_then(|w| w.inner_height().ok())
                                        .and_then(|v| v.as_f64())
                                        .unwrap_or(800.0);
                                    format!("{:.1}", vh / zoom.get() * MINI_SCALE_Y)
                                }
                                style="fill: rgba(245,158,11,0.12); \
                                       stroke: #f59e0b; stroke-width: 1px;"
                            />
                            {edge_lines}
                            {node_dots}
                        </svg>
                    </div>
                })
            }}

            {move || {
                if loading.get() {
                    return view! {
                        <div class="flex items-center justify-center h-full text-stone-400 dark:text-stone-600">
                            <span class="text-sm">"Loading graph\u{2026}"</span>
                        </div>
                    }
                    .into_any();
                }
                if let Some(err) = error_msg.get() {
                    return view! {
                        <div class="flex items-center justify-center h-full text-red-400">
                            <span class="text-sm">{err}</span>
                        </div>
                    }
                    .into_any();
                }
                let all_nodes = nodes_sig.get();
                if all_nodes.is_empty() {
                    return view! {
                        <div class="flex items-center justify-center h-full text-stone-400 dark:text-stone-600">
                            <span class="text-sm">
                                "No nodes yet. Create some notes to see the graph."
                            </span>
                        </div>
                    }
                    .into_any();
                }

                // Apply type-visibility filter + optional tag filter.
                let active_tag = tag_filter.get();
                // Orphan lens: degree computed over ALL edges, so a node only
                // counts as orphaned when nothing links it anywhere — not
                // merely when its neighbors are filtered out of view.
                let orphans_only = show_orphans_only.get();
                let linked_ids: std::collections::HashSet<Uuid> = if orphans_only {
                    edges_sig
                        .get()
                        .iter()
                        .flat_map(|e| [e.source_id.0, e.target_id.0])
                        .collect()
                } else {
                    std::collections::HashSet::new()
                };
                let nodes: Vec<Node> = all_nodes
                    .into_iter()
                    .filter(|n| {
                        let type_visible = match &n.node_type {
                            NodeType::Article   => show_articles.get(),
                            NodeType::Project   => show_projects.get(),
                            NodeType::Area      => show_areas.get(),
                            NodeType::Resource  => show_resources.get(),
                            NodeType::Reference => show_references.get(),
                        };
                        let tag_visible = active_tag
                            .map(|tid| n.tags.iter().any(|t| t.id == tid))
                            .unwrap_or(true);
                        let orphan_visible = !orphans_only || !linked_ids.contains(&n.id.0);
                        type_visible && tag_visible && orphan_visible
                    })
                    .collect();

                // Retain only edges where both endpoints are visible.
                let visible_ids: std::collections::HashSet<Uuid> =
                    nodes.iter().map(|n| n.id.0).collect();
                let edges: Vec<Edge> = edges_sig
                    .get()
                    .into_iter()
                    .filter(|e| {
                        visible_ids.contains(&e.source_id.0)
                            && visible_ids.contains(&e.target_id.0)
                    })
                    .collect();

                // Title lookup map for hover cards.
                let title_map: HashMap<Uuid, String> = nodes
                    .iter()
                    .map(|n| (n.id.0, n.title.clone()))
                    .collect();

                // ── Edge SVGs ────────────────────────────────────────────────
                // Each edge: transparent wide hit-area path + visible styled path.
                // No text label on the edge — visual encoding (colour + dash) carries type.
                let edge_svgs: Vec<_> = edges
                    .iter()
                    .map(|edge| {
                        let src = edge.source_id.0;
                        let tgt = edge.target_id.0;
                        let color = edge_color(&edge.edge_type);
                        let dash = edge_dash(&edge.edge_type);
                        let sw = edge_stroke_width(&edge.edge_type);
                        let marker_id = edge_marker_id(&edge.edge_type);

                        let hover_info = EdgeHover {
                            edge_id: edge.id.0,
                            type_label: edge_label(&edge.edge_type),
                            custom_label: edge.label.clone(),
                            src_title: title_map.get(&src).cloned().unwrap_or_default(),
                            tgt_title: title_map.get(&tgt).cloned().unwrap_or_default(),
                            src_id: src,
                            tgt_id: tgt,
                        };
                        let hover_info_out = hover_info.clone();

                        let dash_part = if dash == "none" {
                            String::new()
                        } else {
                            format!(" stroke-dasharray: {dash};")
                        };
                        let path_style = format!(
                            "stroke: {color}; stroke-width: {sw}; stroke-opacity: 0.85; \
                             fill: none; marker-end: url(#{marker_id});{dash_part}"
                        );

                        // Borrow the positions map (`.with`) and read only the two
                        // endpoints, instead of cloning the entire HashMap per edge
                        // on every mousemove during a drag.
                        let d_vis = move || {
                            positions.with(|pos| {
                                let (x1, y1) = pos.get(&src).copied().unwrap_or((0.0, 0.0));
                                let (x2, y2) = pos.get(&tgt).copied().unwrap_or((0.0, 0.0));
                                compute_path(x1, y1, x2, y2)
                            })
                        };
                        let d_hit = move || {
                            positions.with(|pos| {
                                let (x1, y1) = pos.get(&src).copied().unwrap_or((0.0, 0.0));
                                let (x2, y2) = pos.get(&tgt).copied().unwrap_or((0.0, 0.0));
                                compute_path(x1, y1, x2, y2)
                            })
                        };

                        view! {
                            <g
                                on:mouseover=move |_| edge_hover.set(Some(hover_info.clone()))
                                on:mouseout=move |_| edge_hover.set(None)
                            >
                                // Wide transparent hit area for easier hovering.
                                <path
                                    d=d_hit
                                    style="stroke: transparent; stroke-width: 14; fill: none;"
                                />
                                <path d=d_vis style=path_style />
                                <title>{hover_info_out.type_label}</title>
                            </g>
                        }
                        .into_any()
                    })
                    .collect();

                // ── Node SVGs ────────────────────────────────────────────────
                // Shape encodes type; full title shown below in a dark pill.
                let node_svgs: Vec<_> = nodes
                    .iter()
                    .map(|node| {
                        let id = node.id.0;
                        let node_id: NodeId = node.id;
                        let title = node.title.clone();
                        let title_text = title.clone();
                        let node_type = node.node_type.clone();
                        let fill = node_fill(&node_type);
                        let stroke_c = node_stroke_color(&node_type);

                        // Data for the hover summary card.
                        let hover_info = NodeHoverInfo {
                            title: node.title.clone(),
                            node_type: format!("{:?}", node.node_type).to_lowercase(),
                            status: format!("{:?}", node.status).to_lowercase(),
                            body_preview: node.body.as_deref().and_then(node_body_preview),
                            node_id: node.id.0,
                        };

                        // Estimate background-pill width from title char count.
                        let bg_w = (title.chars().count() as f64 * 5.4 + 10.0_f64).max(32.0_f64);

                        let is_pinned = node.pinned;
                        // Material Symbols ligature for the node-type icon.
                        let icon_glyph: &'static str =
                            type_icon(&format!("{:?}", node_type).to_lowercase());
                        // Collect (TagId, colour) pairs (up to 5) for the dot overlay.
                        let tag_colors: Vec<(TagId, String)> = node
                            .tags
                            .iter()
                            .take(5)
                            .map(|t| (t.id, t.color.clone()))
                            .collect();
                        let shape_el: AnyView = match node_type {
                            NodeType::Article => view! {
                                <circle
                                    cx=move || {
                                        format!(
                                            "{:.1}",
                                            positions.with(|m| m.get(&id).map(|p| p.0).unwrap_or(W / 2.0))
                                        )
                                    }
                                    cy=move || {
                                        format!(
                                            "{:.1}",
                                            positions.with(|m| m.get(&id).map(|p| p.1).unwrap_or(H / 2.0))
                                        )
                                    }
                                    r="20"
                                    style=format!(
                                        "fill: {fill}; stroke: {stroke_c}; stroke-width: 1.5px;"
                                    )
                                />
                            }
                            .into_any(),
                            NodeType::Project => view! {
                                <polygon
                                    points=move || {
                                        let (cx, cy) = positions
                                            .with(|m| m.get(&id).copied().unwrap_or((W / 2.0, H / 2.0)));
                                        diamond_points(cx, cy)
                                    }
                                    style=format!(
                                        "fill: {fill}; stroke: {stroke_c}; stroke-width: 1.5px;"
                                    )
                                />
                            }
                            .into_any(),
                            NodeType::Area => view! {
                                <rect
                                    x=move || {
                                        format!(
                                            "{:.1}",
                                            positions
                                                .get()
                                                .get(&id)
                                                .map(|p| p.0 - 22.0)
                                                .unwrap_or(W / 2.0 - 22.0)
                                        )
                                    }
                                    y=move || {
                                        format!(
                                            "{:.1}",
                                            positions
                                                .get()
                                                .get(&id)
                                                .map(|p| p.1 - 15.0)
                                                .unwrap_or(H / 2.0 - 15.0)
                                        )
                                    }
                                    width="44"
                                    height="30"
                                    rx="6"
                                    style=format!(
                                        "fill: {fill}; stroke: {stroke_c}; stroke-width: 1.5px;"
                                    )
                                />
                            }
                            .into_any(),
                            NodeType::Resource => view! {
                                <polygon
                                    points=move || {
                                        let (cx, cy) = positions
                                            .with(|m| m.get(&id).copied().unwrap_or((W / 2.0, H / 2.0)));
                                        hexagon_points(cx, cy)
                                    }
                                    style=format!(
                                        "fill: {fill}; stroke: {stroke_c}; stroke-width: 1.5px;"
                                    )
                                />
                            }
                            .into_any(),
                            NodeType::Reference => view! {
                                <polygon
                                    points=move || {
                                        let (cx, cy) = positions
                                            .with(|m| m.get(&id).copied().unwrap_or((W / 2.0, H / 2.0)));
                                        triangle_points(cx, cy)
                                    }
                                    style=format!(
                                        "fill: {fill}; stroke: {stroke_c}; stroke-width: 1.5px;"
                                    )
                                />
                            }
                            .into_any(),
                        };

                        view! {
                            <g
                                // Stable hook for e2e specs (graph.spec.ts) — the
                                // only non-visual way to address a node on the canvas.
                                data-node-id=node_id.to_string()
                                style=move || {
                                    if edge_create_mode.get() {
                                        "cursor: crosshair;"
                                    } else {
                                        "cursor: grab;"
                                    }
                                }
                                on:click=move |ev: MouseEvent| {
                                    // Suppress the click that follows a drag.
                                    if did_drag.get_untracked() {
                                        did_drag.set(false);
                                        ev.stop_propagation();
                                        return;
                                    }
                                    // Suppress if a tag dot was just clicked.
                                    if dot_clicked.get_untracked() {
                                        dot_clicked.set(false);
                                        ev.stop_propagation();
                                        return;
                                    }
                                    // Suppress clicks during auto-arrange.
                                    if re_layouting.get_untracked() {
                                        ev.stop_propagation();
                                        return;
                                    }
                                    // Edge-create mode: first click = source, second = target.
                                    if edge_create_mode.get_untracked() {
                                        ev.stop_propagation();
                                        match edge_src.get_untracked() {
                                            None => { edge_src.set(Some(id)); }
                                            Some(src) if src == id => { edge_src.set(None); }
                                            Some(src) => { edge_pair.set(Some((src, id))); }
                                        }
                                    }
                                }
                                on:dblclick={
                                    let nav = navigate.clone();
                                    move |ev: MouseEvent| {
                                        ev.stop_propagation();
                                        if !edge_create_mode.get_untracked() {
                                            nav(&format!("/nodes/{node_id}"), Default::default());
                                        }
                                    }
                                }
                                on:mouseover=move |ev: MouseEvent| {
                                    ev.stop_propagation();
                                    node_hover.set(Some(hover_info.clone()));
                                }
                                on:mouseout=move |_| node_hover.set(None)
                                on:mousedown=move |ev: MouseEvent| {
                                    // Disable node dragging while in edge-create mode or during auto-arrange.
                                    if edge_create_mode.get_untracked() || re_layouting.get_untracked() {
                                        return;
                                    }
                                    ev.stop_propagation();
                                    ev.prevent_default();
                                    did_drag.set(false);
                                    let (nx, ny) = positions
                                        .with_untracked(|m| m.get(&id).copied().unwrap_or((0.0, 0.0)));
                                    let mx = (ev.client_x() as f64 - pan_x.get_untracked())
                                        / zoom.get_untracked();
                                    let my = (ev.client_y() as f64 - pan_y.get_untracked())
                                        / zoom.get_untracked();
                                    drag_offset.set((mx - nx, my - ny));
                                    drag_node.set(Some(id));
                                }
                            >
                                <title>{title}</title>
                                // Amber dashed ring when this node is the selected edge source.
                                {move || (edge_src.get() == Some(id)).then(|| view! {
                                    <circle
                                        cx=move || {
                                            format!(
                                                "{:.1}",
                                                positions.with(|m| m.get(&id).map(|p| p.0).unwrap_or(W / 2.0))
                                            )
                                        }
                                        cy=move || {
                                            format!(
                                                "{:.1}",
                                                positions.with(|m| m.get(&id).map(|p| p.1).unwrap_or(H / 2.0))
                                            )
                                        }
                                        r="32"
                                        style="fill: none; stroke: #f59e0b; stroke-width: 2px; \
                                               stroke-dasharray: 5,3; opacity: 0.95;"
                                    />
                                })}
                                // Amber outer ring for pinned nodes — drawn behind the shape.
                                {is_pinned.then(|| view! {
                                    <circle
                                        cx=move || {
                                            format!(
                                                "{:.1}",
                                                positions.with(|m| m.get(&id).map(|p| p.0).unwrap_or(W / 2.0))
                                            )
                                        }
                                        cy=move || {
                                            format!(
                                                "{:.1}",
                                                positions.with(|m| m.get(&id).map(|p| p.1).unwrap_or(H / 2.0))
                                            )
                                        }
                                        r="29"
                                        style="fill: none; stroke: #f59e0b; stroke-width: 2.5px; opacity: 0.9;"
                                    />
                                })}
                                {shape_el}
                                // Node-type icon centred on the shape (Material Symbols ligature).
                                <text
                                    x=move || {
                                        format!(
                                            "{:.1}",
                                            positions.with(|m| m.get(&id).map(|p| p.0).unwrap_or(W / 2.0))
                                        )
                                    }
                                    y=move || {
                                        format!(
                                            "{:.1}",
                                            positions.with(|m| m.get(&id).map(|p| p.1).unwrap_or(H / 2.0))
                                        )
                                    }
                                    style="text-anchor: middle; dominant-baseline: central; \
                                           font-family: 'Material Symbols Outlined'; \
                                           font-size: 14px; font-weight: 400; \
                                           fill: rgba(255,255,255,0.92); pointer-events: none; \
                                           user-select: none;"
                                >
                                    {icon_glyph}
                                </text>
                                // Semi-transparent pill behind title text.
                                <rect
                                    x=move || {
                                        let cx = positions
                                            .get()
                                            .get(&id)
                                            .map(|p| p.0)
                                            .unwrap_or(W / 2.0);
                                        format!("{:.1}", cx - bg_w / 2.0)
                                    }
                                    y=move || {
                                        let cy = positions
                                            .get()
                                            .get(&id)
                                            .map(|p| p.1)
                                            .unwrap_or(H / 2.0);
                                        format!("{:.1}", cy + 22.0)
                                    }
                                    width=format!("{:.1}", bg_w)
                                    height="14"
                                    rx="3"
                                    style="fill: rgba(0,0,0,0.62); pointer-events: none;"
                                />
                                // Full title — no truncation.
                                <text
                                    x=move || {
                                        format!(
                                            "{:.1}",
                                            positions.with(|m| m.get(&id).map(|p| p.0).unwrap_or(W / 2.0))
                                        )
                                    }
                                    y=move || {
                                        format!(
                                            "{:.1}",
                                            positions
                                                .get()
                                                .get(&id)
                                                .map(|p| p.1 + TEXT_Y_OFFSET)
                                                .unwrap_or(H / 2.0 + TEXT_Y_OFFSET)
                                        )
                                    }
                                    style="text-anchor: middle; font-size: 9px; \
                                           font-weight: 600; fill: #ffffff; pointer-events: none;"
                                >
                                    {title_text}
                                </text>
                                // Tag colour dots — rendered after the title pill so they always
                                // paint on top. Positioned at cy+42, below the pill (cy+22..cy+36).
                                // Clicking a dot sets/clears the tag filter. The active dot gets
                                // an amber outline and is slightly larger.
                                {tag_colors.iter().enumerate().map(|(i, (tag_id, color))| {
                                    let tag_id = *tag_id;
                                    let color = color.clone();
                                    let n_f = tag_colors.len() as f64;
                                    let i_f = i as f64;
                                    view! {
                                        <circle
                                            cx=move || {
                                                let cx = positions
                                                    .get()
                                                    .get(&id)
                                                    .map(|p| p.0)
                                                    .unwrap_or(W / 2.0);
                                                format!(
                                                    "{:.1}",
                                                    cx + (i_f - (n_f - 1.0) / 2.0) * 9.0
                                                )
                                            }
                                            cy=move || {
                                                format!(
                                                    "{:.1}",
                                                    positions
                                                        .get()
                                                        .get(&id)
                                                        .map(|p| p.1)
                                                        .unwrap_or(H / 2.0)
                                                        + 42.0
                                                )
                                            }
                                            r=move || {
                                                if tag_filter.get() == Some(tag_id) { "5.0" } else { "3.5" }
                                            }
                                            style=move || {
                                                if tag_filter.get() == Some(tag_id) {
                                                    format!("fill: {color}; stroke: #d97706; stroke-width: 2px; cursor: pointer; pointer-events: auto;")
                                                } else {
                                                    format!("fill: {color}; stroke: #ffffff; stroke-width: 1px; cursor: pointer; pointer-events: auto;")
                                                }
                                            }
                                            on:click=move |ev: MouseEvent| {
                                                ev.stop_propagation();
                                                dot_clicked.set(true);
                                                tag_filter.update(|f| {
                                                    *f = if *f == Some(tag_id) { None } else { Some(tag_id) };
                                                });
                                            }
                                        />
                                    }
                                }).collect_view()}
                            </g>
                        }
                        .into_any()
                    })
                    .collect();

                let svg_cursor = move || {
                    if drag_node.get().is_some() || panning.get() {
                        "cursor: grabbing;"
                    } else if edge_create_mode.get() {
                        "cursor: crosshair;"
                    } else {
                        "cursor: default;"
                    }
                };

                // ── Edge hover card ──────────────────────────────────────────
                // Rendered last so it sits on top of all edges and nodes.
                // Contains type label, direction, optional custom label, and a
                // Delete button (pointer-events: auto on the button only).
                let hover_card = move || {
                    edge_hover.get().map(|h| {
                        let (x1, y1, x2, y2) = positions.with(|pos| {
                            let (sx, sy) = pos.get(&h.src_id).copied().unwrap_or((W / 2.0, H / 2.0));
                            let (tx, ty) = pos.get(&h.tgt_id).copied().unwrap_or((W / 2.0, H / 2.0));
                            (sx, sy, tx, ty)
                        });
                        let cx = (x1 + x2) / 2.0;
                        let cy = (y1 + y2) / 2.0 - 24.0;

                        let relation = h.type_label.to_string();
                        let arrow_line = format!(
                            "{} \u{2192} {}",
                            truncate(&h.src_title, 20),
                            truncate(&h.tgt_title, 20)
                        );
                        let custom_lbl = h
                            .custom_label
                            .as_deref()
                            .filter(|s| !s.is_empty())
                            .map(|s| truncate(s, 28));

                        // Extra row for the Delete button.
                        let line_count = if custom_lbl.is_some() { 3.0_f64 } else { 2.0_f64 };
                        let card_h = line_count * 14.0 + 28.0; // +18 for delete button row
                        let max_chars = [
                            relation.len(),
                            arrow_line.len(),
                            custom_lbl.as_deref().map(str::len).unwrap_or(0),
                            12, // "Delete edge" button text minimum
                        ]
                        .into_iter()
                        .max()
                        .unwrap_or(0);
                        let card_w = (max_chars as f64 * 5.8 + 16.0_f64).max(100.0_f64);

                        let cx_s = format!("{:.1}", cx);
                        let card_x = format!("{:.1}", cx - card_w / 2.0);
                        let card_y = format!("{:.1}", cy - 5.0);
                        let y_rel = format!("{:.1}", cy + 9.0);
                        let y_arrow = format!("{:.1}", cy + 23.0);
                        let y_cust = format!("{:.1}", cy + 37.0);

                        // Delete button positioned at card bottom.
                        let y_btn_top = cy - 5.0 + card_h - 18.0;
                        let btn_w = 80.0_f64;
                        let btn_x = cx - btn_w / 2.0;
                        let y_btn_text = y_btn_top + 11.0;
                        let edge_id = h.edge_id;

                        view! {
                            <g style="pointer-events: none;">
                                <rect
                                    x=card_x
                                    y=card_y
                                    width=format!("{:.1}", card_w)
                                    height=format!("{:.1}", card_h)
                                    rx="5"
                                    style="fill: rgba(12,12,12,0.92); \
                                           stroke: rgba(255,255,255,0.18); stroke-width: 0.5;"
                                />
                                <text
                                    x=cx_s.clone()
                                    y=y_rel
                                    style="text-anchor: middle; font-size: 10px; \
                                           font-weight: 700; fill: #fbbf24;"
                                >
                                    {relation}
                                </text>
                                <text
                                    x=cx_s.clone()
                                    y=y_arrow
                                    style="text-anchor: middle; font-size: 8.5px; fill: #d4d4d4;"
                                >
                                    {arrow_line}
                                </text>
                                {custom_lbl.map(|lbl| {
                                    view! {
                                        <text
                                            x=cx_s
                                            y=y_cust
                                            style="text-anchor: middle; font-size: 8.5px; \
                                                   font-style: italic; fill: #a3a3a3;"
                                        >
                                            {lbl}
                                        </text>
                                    }
                                })}
                                // Delete button — pointer-events: auto so it's clickable.
                                <g
                                    style="pointer-events: auto; cursor: pointer;"
                                    on:click=move |ev: MouseEvent| {
                                        ev.stop_propagation();
                                        let eid = EdgeId(edge_id);
                                        edge_hover.set(None);
                                        spawn_local(async move {
                                            if delete_edge(eid).await.is_ok() {
                                                edges_sig.update(|v| v.retain(|e| e.id.0 != edge_id));
                                            }
                                        });
                                    }
                                >
                                    <rect
                                        x=format!("{:.1}", btn_x)
                                        y=format!("{:.1}", y_btn_top)
                                        width=format!("{:.1}", btn_w)
                                        height="16"
                                        rx="3"
                                        style="fill: rgba(220,38,38,0.85); \
                                               stroke: rgba(255,255,255,0.2); stroke-width: 0.5;"
                                    />
                                    <text
                                        x=format!("{:.1}", cx)
                                        y=format!("{:.1}", y_btn_text)
                                        style="text-anchor: middle; font-size: 8.5px; \
                                               font-weight: 600; fill: #ffffff;"
                                    >
                                        "Delete edge"
                                    </text>
                                </g>
                            </g>
                        }
                        .into_any()
                    })
                };

                view! {
                    <svg
                        id="graph-svg"
                        class="w-full h-full"
                        style=svg_cursor
                        on:mousedown=move |ev: MouseEvent| {
                            if drag_node.get_untracked().is_none() {
                                panning.set(true);
                                last_mx.set(ev.client_x() as f64);
                                last_my.set(ev.client_y() as f64);
                            }
                        }
                        on:mousemove=move |ev: MouseEvent| {
                            if let Some(nid) = drag_node.get_untracked() {
                                ev.prevent_default();
                                did_drag.set(true);
                                // Compute expanded bounds same as the layout effect.
                                let n = positions.with(|m| m.len()) as f64;
                                let grow_factor = (n / 50.0).clamp(1.0, 4.0);
                                let eff_w = W * grow_factor;
                                let eff_h = H * grow_factor;
                                let eff_margin = MARGIN * grow_factor;

                                let mx = (ev.client_x() as f64 - pan_x.get_untracked())
                                    / zoom.get_untracked();
                                let my = (ev.client_y() as f64 - pan_y.get_untracked())
                                    / zoom.get_untracked();
                                let (ox, oy) = drag_offset.get_untracked();
                                let new_x = (mx - ox).clamp(eff_margin, eff_w - eff_margin);
                                let new_y = (my - oy).clamp(eff_margin, eff_h - eff_margin);
                                positions.update(|map| {
                                    map.insert(nid, (new_x, new_y));
                                });
                            } else if panning.get_untracked() {
                                let mx = ev.client_x() as f64;
                                let my = ev.client_y() as f64;
                                pan_x.update(|p| *p += mx - last_mx.get_untracked());
                                pan_y.update(|p| *p += my - last_my.get_untracked());
                                last_mx.set(mx);
                                last_my.set(my);
                            }
                        }
                        on:mouseup=move |_ev: MouseEvent| {
                            if let Some(nid) = drag_node.get_untracked() {
                                let (x, y) = positions
                                    .with_untracked(|m| m.get(&nid).copied().unwrap_or((0.0, 0.0)));
                                spawn_local(async move {
                                    let _ = save_position(nid, x, y).await;
                                });
                                drag_node.set(None);
                            }
                            panning.set(false);
                        }
                        on:mouseleave=move |_: MouseEvent| {
                            if let Some(nid) = drag_node.get_untracked() {
                                let (x, y) = positions
                                    .with_untracked(|m| m.get(&nid).copied().unwrap_or((0.0, 0.0)));
                                spawn_local(async move {
                                    let _ = save_position(nid, x, y).await;
                                });
                                drag_node.set(None);
                            }
                            panning.set(false);
                        }
                        on:wheel=move |ev: WheelEvent| {
                            ev.prevent_default();
                            let factor = if ev.delta_y() > 0.0 { 0.9_f64 } else { 1.1_f64 };
                            zoom.update(|z| *z = (*z * factor).clamp(ZOOM_MIN, ZOOM_MAX));
                            zoom_input.set(format!("{:.0}", zoom.get_untracked() * 100.0));
                        }
                        // ── Touch: single-finger pan, two-finger pinch-to-zoom ──────
                        on:touchstart=move |ev: TouchEvent| {
                            ev.prevent_default();
                            let touches = ev.touches();
                            match touches.length() {
                                1 => {
                                    if let Some(t) = touches.get(0) {
                                        last_tx.set(t.client_x() as f64);
                                        last_ty.set(t.client_y() as f64);
                                        panning.set(true);
                                    }
                                }
                                2 => {
                                    panning.set(false);
                                    if let (Some(t0), Some(t1)) =
                                        (touches.get(0), touches.get(1))
                                    {
                                        let dx = t1.client_x() as f64 - t0.client_x() as f64;
                                        let dy = t1.client_y() as f64 - t0.client_y() as f64;
                                        pinch_start_dist
                                            .set((dx * dx + dy * dy).sqrt());
                                        pinch_start_zoom.set(zoom.get_untracked());
                                    }
                                }
                                _ => {}
                            }
                        }
                        on:touchmove=move |ev: TouchEvent| {
                            ev.prevent_default();
                            let touches = ev.touches();
                            match touches.length() {
                                1 => {
                                    if panning.get_untracked()
                                        && let Some(t) = touches.get(0)
                                    {
                                        let tx = t.client_x() as f64;
                                        let ty = t.client_y() as f64;
                                        pan_x.update(|p| {
                                            *p += tx - last_tx.get_untracked()
                                        });
                                        pan_y.update(|p| {
                                            *p += ty - last_ty.get_untracked()
                                        });
                                        last_tx.set(tx);
                                        last_ty.set(ty);
                                    }
                                }
                                2 => {
                                    if let (Some(t0), Some(t1)) =
                                        (touches.get(0), touches.get(1))
                                    {
                                        let dx = t1.client_x() as f64 - t0.client_x() as f64;
                                        let dy = t1.client_y() as f64 - t0.client_y() as f64;
                                        let dist = (dx * dx + dy * dy).sqrt();
                                        let start = pinch_start_dist.get_untracked();
                                        if start > 0.0 {
                                            let new_zoom = (pinch_start_zoom
                                                .get_untracked()
                                                * dist
                                                / start)
                                                .clamp(ZOOM_MIN, ZOOM_MAX);
                                            zoom.set(new_zoom);
                                            zoom_input.set(format!("{:.0}", new_zoom * 100.0));
                                        }
                                    }
                                }
                                _ => {}
                            }
                        }
                        on:touchend=move |_: TouchEvent| {
                            panning.set(false);
                        }
                        on:touchcancel=move |_: TouchEvent| {
                            panning.set(false);
                        }
                    >
                        <g transform=move || {
                            format!(
                                "translate({:.1},{:.1}) scale({:.3})",
                                pan_x.get(),
                                pan_y.get(),
                                zoom.get(),
                            )
                        }>
                            {edge_svgs}
                            {node_svgs}
                            {hover_card}
                            // Node summary card — shown on hover, rendered last so it's on top.
                            {move || {
                                node_hover.get().map(|h| {
                                    let (nx, ny) = positions
                                        .with(|m| m.get(&h.node_id).copied().unwrap_or((W / 2.0, H / 2.0)));

                                    let type_lbl = type_label(&h.node_type);
                                    let status_lbl = status_label(&h.status);
                                    let status_col = status_color_hex(&h.status);

                                    // Card dimensions
                                    let card_w = (h.title.chars().count() as f64 * 6.0 + 20.0)
                                        .clamp(120.0_f64, 240.0_f64);
                                    let has_preview = h.body_preview.is_some();
                                    let card_h = if has_preview { 66.0_f64 } else { 50.0_f64 };
                                    let cx = nx;
                                    let cy = ny - NODE_R - card_h - 8.0;

                                    let cx_s  = format!("{:.1}", cx);
                                    let card_x = format!("{:.1}", cx - card_w / 2.0);
                                    let card_y = format!("{:.1}", cy);
                                    let y_title  = format!("{:.1}", cy + 13.0);
                                    let y_meta   = format!("{:.1}", cy + 27.0);
                                    let y_prev   = format!("{:.1}", cy + 41.0);
                                    let meta_txt = format!("{type_lbl}  ·  {status_lbl}");

                                    let preview_text = h.body_preview.clone();

                                    view! {
                                        <g style="pointer-events: none;">
                                            // Card background
                                            <rect
                                                x=card_x
                                                y=card_y
                                                width=format!("{:.1}", card_w)
                                                height=format!("{:.1}", card_h)
                                                rx="6"
                                                style="fill: rgba(12,12,12,0.92); \
                                                       stroke: rgba(255,255,255,0.18); \
                                                       stroke-width: 0.5;"
                                            />
                                            // Title
                                            <text
                                                x=cx_s.clone()
                                                y=y_title
                                                style="text-anchor: middle; font-size: 10.5px; \
                                                       font-weight: 700; fill: #f5f5f4;"
                                            >
                                                {h.title.clone()}
                                            </text>
                                            // Type · Status meta line
                                            <text
                                                x=cx_s.clone()
                                                y=y_meta
                                                style=format!(
                                                    "text-anchor: middle; font-size: 8px; fill: {};",
                                                    status_col
                                                )
                                            >
                                                {meta_txt}
                                            </text>
                                            // Body preview (optional)
                                            {preview_text.map(|preview| view! {
                                                <text
                                                    x=cx_s
                                                    y=y_prev
                                                    style="text-anchor: middle; font-size: 7.5px; \
                                                           fill: #a8a29e;"
                                                >
                                                    {preview}
                                                </text>
                                            })}
                                            // "Double-click to open" hint
                                            <text
                                                x=format!("{:.1}", cx)
                                                y=format!("{:.1}", cy + card_h - 5.0)
                                                style="text-anchor: middle; font-size: 7px; \
                                                       fill: rgba(255,255,255,0.3);"
                                            >
                                                "double-click to open"
                                            </text>
                                        </g>
                                    }
                                    .into_any()
                                })
                            }}
                        </g>
                    </svg>
                }
                .into_any()
            }}

            // ── Full-screen spinner during auto-arrange computation ──────────
            {move || re_layouting.get().then(|| view! {
                <div class="absolute inset-0 z-30 flex flex-col items-center justify-center \
                            bg-stone-900/40 backdrop-blur-sm">
                    <div class="bg-white dark:bg-stone-900 rounded-2xl shadow-2xl \
                                border border-stone-200 dark:border-stone-700 \
                                px-8 py-6 flex flex-col items-center gap-3">
                        <svg class="animate-spin h-8 w-8 text-amber-600"
                             viewBox="0 0 24 24" fill="none"
                             style="animation-duration: 0.8s;">
                            <circle cx="12" cy="12" r="10" stroke="currentColor"
                                    stroke-width="3" stroke-opacity="0.25"/>
                            <path d="M12 2a10 10 0 0 1 10 10" stroke="currentColor"
                                  stroke-width="3" stroke-linecap="round"/>
                        </svg>
                        <span class="text-sm font-medium text-stone-700 dark:text-stone-300">
                            "Arranging nodes\u{2026}"
                        </span>
                        <span class="text-xs text-stone-400 dark:text-stone-500">
                            "This may take a moment for large graphs"
                        </span>
                    </div>
                </div>
            })}
        </div>
    }
}

// ── Legend sub-components ────────────────────────────────────────────────────

/// Clickable legend row that toggles node-type visibility.
#[component]
fn LegendToggle(
    label: &'static str,
    color: &'static str,
    shape: &'static str,
    show: RwSignal<bool>,
) -> impl IntoView {
    let icon_view = legend_shape_icon(color, shape);
    view! {
        <button
            class=move || {
                let dimmed = if show.get() { "" } else { " opacity-40" };
                format!(
                    "flex items-center gap-1.5 cursor-pointer select-none w-full text-left\
                     hover:opacity-80 transition-opacity{dimmed}"
                )
            }
            title=move || if show.get() { format!("Hide {label}") } else { format!("Show {label}") }
            on:click=move |_| show.update(|v| *v = !*v)
        >
            {icon_view}
            <span class="text-stone-700 dark:text-stone-300">{label}</span>
            {move || (!show.get()).then(|| view! {
                <span class="ml-auto text-stone-400 dark:text-stone-500 text-[10px]">"hidden"</span>
            })}
        </button>
    }
}

/// Builds the small SVG icon used in legend rows (shared by LegendToggle and LegendShape).
fn legend_shape_icon(color: &'static str, shape: &'static str) -> AnyView {
    match shape {
        "diamond" => view! {
            <svg width="14" height="14" viewBox="0 0 14 14" style="flex-shrink:0;">
                <polygon
                    points="7,1 13,7 7,13 1,7"
                    style=format!("fill:{color}; stroke:{color}; stroke-width:0.5;")
                />
            </svg>
        }
        .into_any(),
        "rect" => view! {
            <svg width="14" height="14" viewBox="0 0 14 14" style="flex-shrink:0;">
                <rect
                    x="1" y="3" width="12" height="8" rx="2"
                    style=format!("fill:{color}; stroke:{color}; stroke-width:0.5;")
                />
            </svg>
        }
        .into_any(),
        "hexagon" => view! {
            <svg width="14" height="14" viewBox="0 0 14 14" style="flex-shrink:0;">
                <polygon
                    points="7,1 12.2,4 12.2,10 7,13 1.8,10 1.8,4"
                    style=format!("fill:{color}; stroke:{color}; stroke-width:0.5;")
                />
            </svg>
        }
        .into_any(),
        "triangle" => view! {
            <svg width="14" height="14" viewBox="0 0 14 14" style="flex-shrink:0;">
                <polygon
                    points="7,1 13,13 1,13"
                    style=format!("fill:{color}; stroke:{color}; stroke-width:0.5;")
                />
            </svg>
        }
        .into_any(),
        _ => view! {
            <svg width="14" height="14" viewBox="0 0 14 14" style="flex-shrink:0;">
                <circle
                    cx="7" cy="7" r="6"
                    style=format!("fill:{color}; stroke:{color}; stroke-width:0.5;")
                />
            </svg>
        }
        .into_any(),
    }
}

#[component]
fn LegendEdge(
    label: &'static str,
    color: &'static str,
    dash: &'static str,
    width: &'static str,
) -> impl IntoView {
    let dash_attr = if dash == "none" {
        String::new()
    } else {
        format!(" stroke-dasharray:{dash};")
    };
    let line_style = format!("stroke:{color}; stroke-width:{width}; fill:none;{dash_attr}");

    view! {
        <div class="flex items-center gap-1.5">
            <svg width="20" height="10" viewBox="0 0 20 10" style="flex-shrink:0;">
                <line x1="1" y1="5" x2="19" y2="5" style=line_style />
            </svg>
            <span class="text-stone-700 dark:text-stone-300">{label}</span>
        </div>
    }
}
