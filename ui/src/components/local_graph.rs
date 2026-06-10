//! Local graph panel — the current node and its direct connections.
//!
//! Research consensus (see the 2026-06-09 usability review): the *local*
//! graph stays useful at any vault size, while the global graph degrades
//! into a hairball. This panel renders one hop — outgoing edge targets plus
//! incoming sources — in a small radial SVG. Clicking a neighbor navigates.
//! Heavyweight machinery (drag, zoom, persisted positions) deliberately
//! stays in the full `GraphView`.

use std::collections::HashSet;

use common::{id::NodeId, node::Node, node::NodeType};
use leptos::prelude::*;
use leptos_router::hooks::use_navigate;

use crate::components::graph_view::{
    diamond_points, hexagon_points, node_fill, node_stroke_color, triangle_points,
};

/// Cap on rendered neighbors — beyond this the radial layout gets cramped;
/// a "+N more" hint points at the full graph instead.
const MAX_NEIGHBORS: usize = 12;

const VIEW_W: f64 = 340.0;
const VIEW_H: f64 = 240.0;

fn shape_svg(nt: &NodeType, cx: f64, cy: f64, highlight: bool) -> AnyView {
    let fill = node_fill(nt);
    let stroke = if highlight {
        "#f59e0b"
    } else {
        node_stroke_color(nt)
    };
    let sw = if highlight { "3" } else { "1.5" };
    match nt {
        NodeType::Article => view! {
            <circle cx=cx cy=cy r="10" fill=fill stroke=stroke style=format!("stroke-width:{sw};") />
        }
        .into_any(),
        NodeType::Project => view! {
            <polygon points=diamond_points(cx, cy) fill=fill stroke=stroke style=format!("stroke-width:{sw};") />
        }
        .into_any(),
        NodeType::Area => view! {
            <rect x=cx - 10.0 y=cy - 8.0 width="20" height="16" rx="4"
                  fill=fill stroke=stroke style=format!("stroke-width:{sw};") />
        }
        .into_any(),
        NodeType::Resource => view! {
            <polygon points=hexagon_points(cx, cy) fill=fill stroke=stroke style=format!("stroke-width:{sw};") />
        }
        .into_any(),
        NodeType::Reference => view! {
            <polygon points=triangle_points(cx, cy) fill=fill stroke=stroke style=format!("stroke-width:{sw};") />
        }
        .into_any(),
    }
}

fn short_title(s: &str) -> String {
    if s.chars().count() <= 16 {
        s.to_string()
    } else {
        let cut: String = s.chars().take(15).collect();
        format!("{cut}…")
    }
}

#[component]
pub fn LocalGraphPanel(node_id: NodeId, title: String) -> impl IntoView {
    let navigate = StoredValue::new(use_navigate());
    let open = RwSignal::new(false);
    let title_sv = StoredValue::new(title);

    // One hop in both directions, deduped, fetched lazily on first open.
    let neighbors = LocalResource::new(move || {
        let is_open = open.get();
        async move {
            if !is_open {
                return Ok::<Vec<Node>, crate::error::UiError>(vec![]);
            }
            let outgoing = crate::api::fetch_neighbors(node_id).await?;
            let incoming = crate::api::fetch_backlinks(node_id)
                .await
                .unwrap_or_default();
            let mut seen: HashSet<NodeId> = HashSet::new();
            seen.insert(node_id);
            let mut all = Vec::new();
            for n in outgoing.into_iter().chain(incoming) {
                if seen.insert(n.id) {
                    all.push(n);
                }
            }
            Ok(all)
        }
    });

    view! {
        <div class="mt-8 border-t border-stone-200 dark:border-stone-700 pt-6">
            <button
                class="flex items-center gap-1 text-left cursor-pointer"
                on:click=move |_| open.update(|v| *v = !*v)
            >
                <span class="material-symbols-outlined text-stone-400 dark:text-stone-500"
                      style="font-size: 16px;">
                    {move || if open.get() { "expand_more" } else { "chevron_right" }}
                </span>
                <span class="material-symbols-outlined text-stone-400 dark:text-stone-500"
                      style="font-size: 15px;">"hub"</span>
                <h2 class="text-sm font-semibold text-stone-700 dark:text-stone-300">
                    "Local Graph"
                </h2>
            </button>
            {move || open.get().then(|| view! {
                <div class="mt-4">
                <Suspense fallback=|| view! {
                    <div class="text-xs text-stone-400">"Loading…"</div>
                }>
                    {move || {
                        neighbors.get().map(|result| match result {
                            Err(e) => view! {
                                <div class="text-xs text-red-500">{format!("Error: {e}")}</div>
                            }.into_any(),
                            Ok(all) if all.is_empty() => view! {
                                <p class="text-xs text-stone-400 dark:text-stone-600 py-4 text-center">
                                    "No connections yet — add edges or wikilinks."
                                </p>
                            }.into_any(),
                            Ok(all) => {
                                let extra = all.len().saturating_sub(MAX_NEIGHBORS);
                                let shown: Vec<Node> = all.into_iter().take(MAX_NEIGHBORS).collect();
                                let n = shown.len() as f64;
                                let (cx, cy) = (VIEW_W / 2.0, VIEW_H / 2.0);
                                let (rx, ry) = (VIEW_W / 2.0 - 50.0, VIEW_H / 2.0 - 36.0);
                                view! {
                                    <svg
                                        viewBox=format!("0 0 {VIEW_W} {VIEW_H}")
                                        class="w-full max-w-md mx-auto select-none"
                                        role="img"
                                        aria-label="Local graph of directly connected nodes"
                                    >
                                        // Spokes first (under the shapes).
                                        {shown.iter().enumerate().map(|(i, _)| {
                                            let angle = (i as f64) * std::f64::consts::TAU / n
                                                - std::f64::consts::FRAC_PI_2;
                                            let x = cx + rx * angle.cos();
                                            let y = cy + ry * angle.sin();
                                            view! {
                                                <line x1=cx y1=cy x2=x y2=y
                                                      stroke="#a8a29e" style="stroke-width:1;opacity:0.55;" />
                                            }
                                        }).collect_view()}
                                        // Center node.
                                        <g>
                                            {shape_svg(&{
                                                // Center type isn't fetched here; a neutral
                                                // amber-highlighted circle reads as "you are here".
                                                NodeType::Article
                                            }, cx, cy, true)}
                                            <text x=cx y=cy + 24.0 text-anchor="middle"
                                                  class="fill-stone-700 dark:fill-stone-200"
                                                  style="font-size:10px;font-weight:600;">
                                                {short_title(&title_sv.get_value())}
                                            </text>
                                        </g>
                                        // Neighbors.
                                        {shown.into_iter().enumerate().map(|(i, nb)| {
                                            let angle = (i as f64) * std::f64::consts::TAU / n
                                                - std::f64::consts::FRAC_PI_2;
                                            let x = cx + rx * angle.cos();
                                            let y = cy + ry * angle.sin();
                                            let nb_id = nb.id;
                                            let label = short_title(&nb.title);
                                            let full_title = nb.title.clone();
                                            view! {
                                                <g
                                                    class="cursor-pointer"
                                                    on:click=move |_| {
                                                        navigate.get_value()(
                                                            &format!("/nodes/{nb_id}"),
                                                            Default::default(),
                                                        );
                                                    }
                                                >
                                                    <title>{full_title}</title>
                                                    {shape_svg(&nb.node_type, x, y, false)}
                                                    <text x=x y=y + 22.0 text-anchor="middle"
                                                          class="fill-stone-600 dark:fill-stone-300"
                                                          style="font-size:9px;">
                                                        {label}
                                                    </text>
                                                </g>
                                            }
                                        }).collect_view()}
                                    </svg>
                                    {(extra > 0).then(|| view! {
                                        <p class="text-[10px] text-stone-400 dark:text-stone-600 text-center mt-1">
                                            {format!("+{extra} more — see the full ")}
                                            <button
                                                class="underline hover:text-amber-600 dark:hover:text-amber-400"
                                                on:click=move |_| navigate.get_value()("/graph", Default::default())
                                            >"graph"</button>
                                        </p>
                                    })}
                                }.into_any()
                            }
                        })
                    }}
                </Suspense>
                </div>
            })}
        </div>
    }
}
