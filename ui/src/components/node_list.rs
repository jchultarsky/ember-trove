use std::collections::HashSet;

use common::{
    id::{NodeId, TagId},
    node::Node,
    tag::Tag,
};
use leptos::prelude::*;
use uuid::Uuid;

use crate::components::modals::delete_confirm::DeleteConfirmModal;
use crate::components::node_meta::{
    status_color, status_icon, status_label, type_icon, type_label,
};
use crate::components::toast::{ToastLevel, push_toast};
use leptos_router::hooks::use_navigate;

// ── Sorting ────────────────────────────────────────────────────────────────────

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SortKey {
    NameAsc,
    NameDesc,
    ModifiedDesc,
    ModifiedAsc,
}

impl SortKey {
    fn label(self) -> &'static str {
        match self {
            SortKey::NameAsc => "Title A→Z",
            SortKey::NameDesc => "Title Z→A",
            SortKey::ModifiedDesc => "Newest first",
            SortKey::ModifiedAsc => "Oldest first",
        }
    }

    fn icon(self) -> &'static str {
        match self {
            SortKey::NameAsc | SortKey::NameDesc => "sort_by_alpha",
            SortKey::ModifiedDesc | SortKey::ModifiedAsc => "schedule",
        }
    }

    fn sort_nodes(self, mut nodes: Vec<Node>) -> Vec<Node> {
        match self {
            SortKey::NameAsc => nodes.sort_by_key(|n| n.title.to_lowercase()),
            SortKey::NameDesc => nodes.sort_by_key(|n| std::cmp::Reverse(n.title.to_lowercase())),
            SortKey::ModifiedDesc => nodes.sort_by_key(|n| std::cmp::Reverse(n.updated_at)),
            SortKey::ModifiedAsc => nodes.sort_by_key(|n| n.updated_at),
        }
        nodes
    }
}

const SORT_OPTIONS: &[SortKey] = &[
    SortKey::ModifiedDesc,
    SortKey::ModifiedAsc,
    SortKey::NameAsc,
    SortKey::NameDesc,
];

// ── Preview helper ─────────────────────────────────────────────────────────────

/// Strip basic Markdown and return up to 300 chars as a plain-text preview.
fn body_preview(body: &str) -> Option<String> {
    let text: String = body
        .lines()
        .map(str::trim)
        .filter(|l| {
            !l.is_empty()
                && !l.starts_with('#')
                && !l.starts_with("```")
                && !l.starts_with("---")
                && !l.starts_with("===")
        })
        .collect::<Vec<_>>()
        .join(" ");
    if text.is_empty() {
        return None;
    }
    let clean = text.replace("**", "").replace("__", "").replace('`', "");
    let chars: Vec<char> = clean.chars().collect();
    if chars.is_empty() {
        return None;
    }
    let preview: String = chars.iter().take(300).collect();
    Some(if chars.len() > 300 {
        format!("{preview}\u{2026}")
    } else {
        preview
    })
}

// ── NodeList ───────────────────────────────────────────────────────────────────

#[component]
pub fn NodeList() -> impl IntoView {
    let navigate = use_navigate();
    let refresh = use_context::<RwSignal<u32>>().expect("refresh signal must be provided");
    let tag_filter = use_context::<RwSignal<Option<Tag>>>().unwrap_or_else(|| RwSignal::new(None));

    let node_type_filter =
        use_context::<RwSignal<Option<String>>>().unwrap_or_else(|| RwSignal::new(None));

    let status_filter: RwSignal<Option<String>> = RwSignal::new(None);
    let show_archived = RwSignal::new(false);
    let sort_key = RwSignal::new(SortKey::ModifiedDesc);
    let show_sort_menu = RwSignal::new(false);

    // Bulk selection state — shared between NodeList (action bar) and NodeCards (checkboxes).
    let selected_ids: RwSignal<HashSet<Uuid>> = RwSignal::new(HashSet::new());
    let show_apply_menu = RwSignal::new(false);
    let show_remove_menu = RwSignal::new(false);
    let confirm_bulk_delete = RwSignal::new(false);

    let nodes = LocalResource::new(move || {
        let _ = refresh.get();
        let status = status_filter.get();
        let tag = tag_filter.get();
        let include_archived = show_archived.get();
        async move {
            crate::api::fetch_nodes_filtered(
                status.as_deref(),
                tag.map(|t| t.id.0),
                include_archived,
            )
            .await
        }
    });

    // All available tags — used by the per-card tag picker.
    let all_tags = LocalResource::new(crate::api::fetch_tags);

    view! {
        <div class="flex flex-col h-full">
            <div class="flex items-center justify-between px-6 py-4 border-b border-stone-200 dark:border-stone-800">
                <h1 class="text-lg font-semibold text-stone-900 dark:text-stone-100">
                    {move || match node_type_filter.get().as_deref() {
                        Some("project")   => "Projects",
                        Some("area")      => "Areas",
                        Some("resource")  => "Resources",
                        Some("reference") => "References",
                        Some("article") | Some(_) => "Articles",
                        None              => "All Nodes",
                    }}
                </h1>
                <div class="flex items-center gap-2">
                    // Active tag-filter badge
                    {move || tag_filter.get().map(|tag| {
                        let name = tag.name.clone();
                        let color = tag.color.clone();
                        view! {
                            <button
                                class="flex items-center gap-1 px-2 py-0.5 text-xs rounded-full text-white
                                    transition-colors hover:opacity-80"
                                style=format!("background-color: {color}")
                                on:click=move |_| tag_filter.set(None)
                                title="Clear tag filter"
                            >
                                <span class="material-symbols-outlined" style="font-size:11px;">"label"</span>
                                {name}
                                " \u{00d7}"
                            </button>
                        }
                    })}

                    // Sort picker — shows current label so it's discoverable
                    <div class="relative">
                        <button
                            class="flex items-center gap-1 px-2 py-1 rounded-lg text-xs
                                text-stone-500 dark:text-stone-400
                                hover:text-stone-700 dark:hover:text-stone-200
                                hover:bg-stone-100 dark:hover:bg-stone-800
                                border border-stone-200 dark:border-stone-700
                                transition-colors"
                            on:click=move |_| show_sort_menu.update(|v| *v = !*v)
                        >
                            <span class="material-symbols-outlined" style="font-size: 14px;">
                                {move || sort_key.get().icon()}
                            </span>
                            {move || sort_key.get().label()}
                            <span class="material-symbols-outlined" style="font-size: 14px;">"expand_more"</span>
                        </button>
                        {move || show_sort_menu.get().then(|| view! {
                            <div
                                class="fixed inset-0 z-10"
                                on:click=move |_| show_sort_menu.set(false)
                            />
                            <div class="absolute right-0 top-full mt-1 z-20 w-44
                                bg-white dark:bg-stone-900 rounded-xl shadow-xl
                                border border-stone-200 dark:border-stone-700 overflow-hidden">
                                {SORT_OPTIONS.iter().map(|&opt| {
                                    view! {
                                        <button
                                            class=move || {
                                                let active = sort_key.get() == opt;
                                                let base = "w-full text-left px-3 py-2 text-xs flex items-center gap-2 transition-colors";
                                                if active {
                                                    format!("{base} bg-amber-50 dark:bg-amber-900/20 text-amber-700 dark:text-amber-400 font-medium")
                                                } else {
                                                    format!("{base} text-stone-700 dark:text-stone-300 hover:bg-stone-50 dark:hover:bg-stone-800")
                                                }
                                            }
                                            on:click=move |_| {
                                                sort_key.set(opt);
                                                show_sort_menu.set(false);
                                            }
                                        >
                                            <span class="material-symbols-outlined" style="font-size: 14px;">
                                                {opt.icon()}
                                            </span>
                                            {opt.label()}
                                        </button>
                                    }
                                }).collect::<Vec<_>>()}
                            </div>
                        })}
                    </div>

                    <button
                        class="p-1.5 rounded-lg text-stone-400 hover:text-stone-600 dark:hover:text-stone-300
                            hover:bg-stone-100 dark:hover:bg-stone-800 transition-colors"
                        on:click=move |_| navigate("/nodes/new", Default::default())
                        title="New node"
                    >
                        <span class="material-symbols-outlined">"add"</span>
                    </button>
                </div>
            </div>

            // Status filter pills + archived toggle
            <div class="flex items-center gap-1 px-6 py-2 border-b border-stone-100 dark:border-stone-800">
                // Primary status pills (active statuses only)
                {[
                    ("All", None),
                    ("Draft", Some("draft")),
                    ("Published", Some("published")),
                ].iter().map(|&(label, value)| {
                    let value_owned: Option<String> = value.map(|s| s.to_string());
                    let value_cmp = value_owned.clone();
                    view! {
                        <button
                            class=move || {
                                // When showing archived, "All" pill is not the active view.
                                let archived_on = show_archived.get();
                                let active = !archived_on && status_filter.get() == value_cmp;
                                let base = "px-2.5 py-0.5 text-xs rounded-full font-medium transition-colors";
                                if active {
                                    format!("{base} bg-amber-600 text-white")
                                } else {
                                    format!("{base} bg-stone-100 dark:bg-stone-800 text-stone-600 dark:text-stone-400 hover:bg-stone-200 dark:hover:bg-stone-700")
                                }
                            }
                            on:click={
                                let value_set = value_owned.clone();
                                move |_| {
                                    status_filter.set(value_set.clone());
                                    show_archived.set(false);
                                }
                            }
                        >
                            {label}
                        </button>
                    }
                }).collect::<Vec<_>>()}

                // Spacer pushes the archived toggle to the right
                <span class="flex-1" />

                // Archived toggle — visually separate from the primary pills
                <button
                    class=move || {
                        let active = show_archived.get();
                        let base = "flex items-center gap-1 px-2 py-0.5 text-xs rounded-full font-medium transition-colors border";
                        if active {
                            format!("{base} bg-stone-600 text-white border-stone-600 dark:bg-stone-500 dark:border-stone-500")
                        } else {
                            format!("{base} border-stone-300 dark:border-stone-600 text-stone-400 dark:text-stone-500 hover:text-stone-600 dark:hover:text-stone-300 hover:border-stone-400 dark:hover:border-stone-500")
                        }
                    }
                    on:click=move |_| {
                        show_archived.update(|v| *v = !*v);
                        // Clear status pill when toggling archived view
                        if show_archived.get() {
                            status_filter.set(None);
                        }
                    }
                    title="Toggle visibility of archived nodes"
                >
                    <span class="material-symbols-outlined" style="font-size:11px;">"inventory_2"</span>
                    {move || if show_archived.get() { "Hide archived" } else { "Show archived" }}
                </button>
            </div>

            // Bulk action bar — shown when ≥1 nodes are selected
            {move || {
                let sel = selected_ids.get();
                if sel.is_empty() { return None; }
                let count = sel.len();
                let all_tags_snap = all_tags.get().and_then(|r| r.ok()).unwrap_or_default();
                let all_tags_apply  = StoredValue::new(all_tags_snap.clone());
                let all_tags_remove = StoredValue::new(all_tags_snap);
                Some(view! {
                    <div class="flex items-center gap-2 px-6 py-2 bg-amber-50 dark:bg-amber-900/20
                                border-b border-amber-200 dark:border-amber-800">
                        <span class="text-xs font-semibold text-amber-700 dark:text-amber-400">
                            {format!("{count} selected")}
                        </span>
                        // Apply tag dropdown
                        <div class="relative">
                            <button
                                class="flex items-center gap-1 px-2 py-1 text-xs rounded-lg
                                    bg-amber-100 dark:bg-amber-900/40
                                    text-amber-700 dark:text-amber-400
                                    hover:bg-amber-200 dark:hover:bg-amber-900/60
                                    border border-amber-300 dark:border-amber-700
                                    transition-colors"
                                on:click=move |_| {
                                    show_apply_menu.update(|v| *v = !*v);
                                    show_remove_menu.set(false);
                                }
                            >
                                <span class="material-symbols-outlined" style="font-size:13px;">"add"</span>
                                "Apply tag"
                            </button>
                            {move || show_apply_menu.get().then(|| {
                                let tags = all_tags_apply.get_value();
                                let sel_snap = selected_ids.get();
                                view! {
                                    <div class="fixed inset-0 z-10" on:click=move |_| show_apply_menu.set(false) />
                                    <div class="absolute left-0 top-full mt-1 z-20 w-48
                                        bg-white dark:bg-stone-900 rounded-xl shadow-xl
                                        border border-stone-200 dark:border-stone-700 overflow-hidden">
                                        {if tags.is_empty() {
                                            view! {
                                                <div class="px-3 py-2 text-xs text-stone-400">"No tags defined"</div>
                                            }.into_any()
                                        } else {
                                            view! {
                                                <div>
                                                {tags.into_iter().map(|tag| {
                                                    let tag_id = tag.id;
                                                    let color  = tag.color.clone();
                                                    let name   = tag.name.clone();
                                                    let ids: Vec<NodeId> = sel_snap.iter().map(|&u| NodeId(u)).collect();
                                                    view! {
                                                        <button
                                                            class="w-full text-left px-3 py-1.5 text-xs flex items-center gap-2
                                                                hover:bg-stone-50 dark:hover:bg-stone-800 transition-colors"
                                                            on:click=move |_| {
                                                                show_apply_menu.set(false);
                                                                let ids = ids.clone();
                                                                wasm_bindgen_futures::spawn_local(async move {
                                                                    let mut ok = 0usize;
                                                                    for nid in &ids {
                                                                        if crate::api::attach_tag(*nid, tag_id).await.is_ok() {
                                                                            ok += 1;
                                                                        }
                                                                    }
                                                                    push_toast(ToastLevel::Success,
                                                                        format!("Applied tag to {ok} node(s)."));
                                                                    selected_ids.set(HashSet::new());
                                                                    refresh.update(|n| *n += 1);
                                                                });
                                                            }
                                                        >
                                                            <span class="w-2 h-2 rounded-full shrink-0"
                                                                  style=format!("background-color:{color}") />
                                                            <span class="text-stone-700 dark:text-stone-300">{name}</span>
                                                        </button>
                                                    }
                                                }).collect::<Vec<_>>()}
                                                </div>
                                            }.into_any()
                                        }}
                                    </div>
                                }
                            })}
                        </div>
                        // Remove tag dropdown
                        <div class="relative">
                            <button
                                class="flex items-center gap-1 px-2 py-1 text-xs rounded-lg
                                    bg-stone-100 dark:bg-stone-800
                                    text-stone-600 dark:text-stone-400
                                    hover:bg-stone-200 dark:hover:bg-stone-700
                                    border border-stone-300 dark:border-stone-600
                                    transition-colors"
                                on:click=move |_| {
                                    show_remove_menu.update(|v| *v = !*v);
                                    show_apply_menu.set(false);
                                }
                            >
                                <span class="material-symbols-outlined" style="font-size:13px;">"remove"</span>
                                "Remove tag"
                            </button>
                            {move || show_remove_menu.get().then(|| {
                                let tags = all_tags_remove.get_value();
                                let sel_snap = selected_ids.get();
                                view! {
                                    <div class="fixed inset-0 z-10" on:click=move |_| show_remove_menu.set(false) />
                                    <div class="absolute left-0 top-full mt-1 z-20 w-48
                                        bg-white dark:bg-stone-900 rounded-xl shadow-xl
                                        border border-stone-200 dark:border-stone-700 overflow-hidden">
                                        {if tags.is_empty() {
                                            view! {
                                                <div class="px-3 py-2 text-xs text-stone-400">"No tags defined"</div>
                                            }.into_any()
                                        } else {
                                            view! {
                                                <div>
                                                {tags.into_iter().map(|tag| {
                                                    let tag_id = tag.id;
                                                    let color  = tag.color.clone();
                                                    let name   = tag.name.clone();
                                                    let ids: Vec<NodeId> = sel_snap.iter().map(|&u| NodeId(u)).collect();
                                                    view! {
                                                        <button
                                                            class="w-full text-left px-3 py-1.5 text-xs flex items-center gap-2
                                                                hover:bg-stone-50 dark:hover:bg-stone-800 transition-colors"
                                                            on:click=move |_| {
                                                                show_remove_menu.set(false);
                                                                let ids = ids.clone();
                                                                wasm_bindgen_futures::spawn_local(async move {
                                                                    let mut ok = 0usize;
                                                                    for nid in &ids {
                                                                        if crate::api::detach_tag(*nid, tag_id).await.is_ok() {
                                                                            ok += 1;
                                                                        }
                                                                    }
                                                                    push_toast(ToastLevel::Success,
                                                                        format!("Removed tag from {ok} node(s)."));
                                                                    selected_ids.set(HashSet::new());
                                                                    refresh.update(|n| *n += 1);
                                                                });
                                                            }
                                                        >
                                                            <span class="w-2 h-2 rounded-full shrink-0"
                                                                  style=format!("background-color:{color}") />
                                                            <span class="text-stone-700 dark:text-stone-300">{name}</span>
                                                        </button>
                                                    }
                                                }).collect::<Vec<_>>()}
                                                </div>
                                            }.into_any()
                                        }}
                                    </div>
                                }
                            })}
                        </div>
                        // Bulk delete
                        <button
                            class="flex items-center gap-1 px-2 py-1 text-xs rounded-lg
                                bg-red-50 dark:bg-red-900/20
                                text-red-600 dark:text-red-400
                                hover:bg-red-100 dark:hover:bg-red-900/40
                                border border-red-200 dark:border-red-800
                                transition-colors"
                            on:click=move |_| confirm_bulk_delete.set(true)
                        >
                            <span class="material-symbols-outlined" style="font-size:13px;">"delete"</span>
                            "Delete"
                        </button>
                        // Bulk export (uses existing full-export endpoint)
                        <a
                            href="/api/export"
                            target="_blank"
                            class="flex items-center gap-1 px-2 py-1 text-xs rounded-lg
                                bg-stone-100 dark:bg-stone-800
                                text-stone-600 dark:text-stone-400
                                hover:bg-stone-200 dark:hover:bg-stone-700
                                border border-stone-300 dark:border-stone-600
                                transition-colors no-underline"
                        >
                            <span class="material-symbols-outlined" style="font-size:13px;">"download"</span>
                            "Export ZIP"
                        </a>
                        <span class="flex-1"/>
                        // Clear selection
                        <button
                            class="flex items-center gap-1 px-2 py-1 text-xs rounded-lg
                                text-stone-500 dark:text-stone-400
                                hover:text-stone-700 dark:hover:text-stone-200
                                hover:bg-stone-100 dark:hover:bg-stone-800 transition-colors"
                            on:click=move |_| selected_ids.set(HashSet::new())
                        >
                            <span class="material-symbols-outlined" style="font-size:13px;">"close"</span>
                            "Clear"
                        </button>
                    </div>
                }.into_any())
            }}

            <div class="flex-1 overflow-auto">
                <Suspense fallback=move || view! {
                    <ul class="divide-y divide-stone-200 dark:divide-stone-800">
                        {(0..6).map(|_| view! {
                            <li class="px-6 py-4">
                                <div class="flex items-start justify-between gap-3">
                                    <div class="min-w-0 flex-1 space-y-2">
                                        <div class="flex items-center gap-2">
                                            <div class="w-4 h-4 rounded bg-stone-200 dark:bg-stone-700 animate-pulse flex-shrink-0" />
                                            <div class="h-3.5 rounded bg-stone-200 dark:bg-stone-700 animate-pulse w-40" />
                                            <div class="w-4 h-4 rounded bg-stone-100 dark:bg-stone-800 animate-pulse flex-shrink-0" />
                                        </div>
                                        <div class="h-2.5 rounded bg-stone-100 dark:bg-stone-800 animate-pulse w-64" />
                                    </div>
                                    <div class="h-2.5 rounded bg-stone-100 dark:bg-stone-800 animate-pulse w-20 shrink-0 mt-1" />
                                </div>
                            </li>
                        }).collect::<Vec<_>>()}
                    </ul>
                }>
                    {move || {
                        nodes.get().map(|result| {
                            match result {
                                Ok(list) if list.is_empty() => view! {
                                    <div class="flex flex-col items-center justify-center h-full gap-3 p-12">
                                        <span
                                            class="material-symbols-outlined text-stone-300 dark:text-stone-700"
                                            style="font-size: 48px;"
                                        >
                                            "description"
                                        </span>
                                        <p class="text-stone-400 dark:text-stone-600 text-sm text-center">
                                            "No nodes found."
                                        </p>
                                    </div>
                                }.into_any(),
                                Ok(list) => {
                                    // Client-side type filter applied after fetch.
                                    let type_f = node_type_filter.get();
                                    let filtered: Vec<Node> = if let Some(ref nt) = type_f {
                                        list.into_iter()
                                            .filter(|n| {
                                                format!("{:?}", n.node_type).to_lowercase() == *nt
                                            })
                                            .collect()
                                    } else {
                                        list
                                    };
                                    if filtered.is_empty() {
                                        return view! {
                                            <div class="flex flex-col items-center justify-center h-full gap-3 p-12">
                                                <span
                                                    class="material-symbols-outlined text-stone-300 dark:text-stone-700"
                                                    style="font-size: 48px;"
                                                >
                                                    "description"
                                                </span>
                                                <p class="text-stone-400 dark:text-stone-600 text-sm text-center">
                                                    "No nodes found."
                                                </p>
                                            </div>
                                        }.into_any();
                                    }
                                    // Pinned nodes always float to the top,
                                    // then secondary sort by the chosen SortKey.
                                    let mut sorted = sort_key.get().sort_nodes(filtered);
                                    sorted.sort_by_key(|n| !n.pinned); // stable: false < true, so pinned (true) → !true = false sorts first
                                    // Snapshot available tags (empty vec if resource hasn't resolved yet).
                                    let available_tags = all_tags
                                        .get()
                                        .and_then(|r| r.ok())
                                        .unwrap_or_default();
                                    view! {
                                        <NodeCards
                                            nodes=sorted
                                            available_tags=available_tags
                                            selected_ids=selected_ids
                                        />
                                    }.into_any()
                                }
                                Err(e) => view! {
                                    <div class="p-6 text-red-500 text-sm">{format!("Error: {e}")}</div>
                                }.into_any(),
                            }
                        })
                    }}
                </Suspense>
            </div>

            // Bulk delete confirmation modal
            <DeleteConfirmModal
                show=Signal::derive(move || confirm_bulk_delete.get())
                item_name=Signal::derive(move || {
                    let n = selected_ids.get().len();
                    format!("{n} selected node{}", if n == 1 { "" } else { "s" })
                })
                on_confirm=Callback::new(move |()| {
                    confirm_bulk_delete.set(false);
                    let ids: Vec<NodeId> = selected_ids
                        .get_untracked()
                        .iter()
                        .map(|&u| NodeId(u))
                        .collect();
                    leptos::task::spawn_local(async move {
                        let mut ok = 0usize;
                        for nid in &ids {
                            if crate::api::delete_node(*nid).await.is_ok() {
                                ok += 1;
                            }
                        }
                        push_toast(ToastLevel::Success, format!("Deleted {ok} node(s)."));
                        selected_ids.set(HashSet::new());
                        refresh.update(|n| *n += 1);
                    });
                })
                on_cancel=Callback::new(move |()| confirm_bulk_delete.set(false))
            />
        </div>
    }
}

// ── NodeCards ──────────────────────────────────────────────────────────────────

#[component]
fn NodeCards(
    nodes: Vec<Node>,
    available_tags: Vec<Tag>,
    selected_ids: RwSignal<HashSet<Uuid>>,
) -> impl IntoView {
    let navigate = use_navigate();
    let tag_filter = use_context::<RwSignal<Option<Tag>>>().unwrap_or_else(|| RwSignal::new(None));
    let refresh = use_context::<RwSignal<u32>>().expect("refresh signal must be provided");

    // Wrap in StoredValue so it is Copy and can be captured across per-card closures.
    let available_tags = StoredValue::new(available_tags);

    view! {
        <ul class="divide-y divide-stone-200 dark:divide-stone-800">
            {nodes.into_iter().map(|node| {
                let id      = node.id;
                let nt      = format!("{:?}", node.node_type).to_lowercase();
                let st      = format!("{:?}", node.status).to_lowercase();
                let updated = node.updated_at.format("%b %d, %Y").to_string();
                let tags    = node.tags.clone();
                // Snapshot of currently-applied tags for this card (used in picker state).
                let node_tags_stored = StoredValue::new(node.tags.clone());
                let pinned  = node.pinned;

                // Per-card picker visibility signal.
                let show_picker = RwSignal::new(false);

                let t_icon  = type_icon(&nt);
                let t_label = type_label(&nt);
                let s_icon  = status_icon(&st);
                let s_label = status_label(&st);
                let s_color = status_color(&st);

                let node_uuid = id.0;
                let nav = navigate.clone();
                view! {
                    <li
                        class="px-4 py-3.5 hover:bg-stone-50 dark:hover:bg-stone-900/60
                               cursor-pointer transition-colors"
                        on:click=move |_| {
                            // Only navigate if nothing is selected (bulk mode off)
                            // — prevents accidental navigation while selecting.
                            if selected_ids.get_untracked().is_empty() {
                                nav(&format!("/nodes/{id}"), Default::default());
                            }
                        }
                    >
                        <div class="flex items-start gap-3">
                            // Checkbox — always visible; click toggles selection
                            <input
                                type="checkbox"
                                class="mt-1 w-4 h-4 shrink-0 cursor-pointer"
                                prop:checked=move || selected_ids.get().contains(&node_uuid)
                                on:click=move |ev| {
                                    use leptos::ev::MouseEvent;
                                    let ev: MouseEvent = ev;
                                    ev.stop_propagation();
                                    selected_ids.update(|s| {
                                        if s.contains(&node_uuid) {
                                            s.remove(&node_uuid);
                                        } else {
                                            s.insert(node_uuid);
                                        }
                                    });
                                }
                            />
                        <div class="flex items-start justify-between gap-4 flex-1 min-w-0">
                            <div class="min-w-0 flex-1">

                                // ── Row 1: type icon · title · status icon ──────────
                                <div class="flex items-center gap-2">
                                    // Type icon — subtle, with tooltip
                                    <span
                                        class="material-symbols-outlined text-stone-400
                                               dark:text-stone-500 flex-shrink-0"
                                        style="font-size: 16px;"
                                        title=t_label
                                    >
                                        {t_icon}
                                    </span>

                                    // Title
                                    <span class="text-sm font-medium text-stone-900
                                                 dark:text-stone-100 truncate">
                                        {node.title.clone()}
                                    </span>

                                    // Status icon — semantic colour, with tooltip
                                    <span
                                        class="material-symbols-outlined flex-shrink-0"
                                        style=format!("font-size: 15px; {s_color}")
                                        title=s_label
                                    >
                                        {s_icon}
                                    </span>

                                    // Pin indicator
                                    {pinned.then(|| view! {
                                        <span
                                            class="material-symbols-outlined text-amber-500 dark:text-amber-400 flex-shrink-0"
                                            style="font-size: 13px;"
                                            title="Pinned"
                                        >
                                            "push_pin"
                                        </span>
                                    })}
                                </div>

                                // ── Row 2: tag pills + inline tag picker ─────────────
                                <div class="flex flex-wrap gap-1 mt-1.5 items-center">
                                    // Existing tag pills (clicking sets list-level tag filter).
                                    {tags.into_iter().map(|tag| {
                                        let tf    = tag.clone();
                                        let color = tag.color.clone();
                                        let name  = tag.name.clone();
                                        let tip   = format!("Filter: {name}");
                                        view! {
                                            <button
                                                class="px-2 py-0.5 text-xs rounded-full
                                                       text-white hover:opacity-80
                                                       transition-opacity whitespace-nowrap"
                                                style=format!("background-color: {color}")
                                                title=tip
                                                on:click=move |ev| {
                                                    ev.stop_propagation();
                                                    tag_filter.set(Some(tf.clone()));
                                                }
                                            >
                                                {name}
                                            </button>
                                        }
                                    }).collect::<Vec<_>>()}

                                    // ── Tag picker button + dropdown ─────────────────
                                    <div class="relative">
                                        <button
                                            class="flex items-center justify-center w-5 h-5 rounded-full
                                                   text-stone-300 dark:text-stone-600
                                                   hover:text-amber-500 dark:hover:text-amber-400
                                                   hover:bg-stone-100 dark:hover:bg-stone-800
                                                   transition-colors"
                                            title="Add or remove tags"
                                            on:click=move |ev| {
                                                ev.stop_propagation();
                                                show_picker.update(|v| *v = !*v);
                                            }
                                        >
                                            <span class="material-symbols-outlined" style="font-size: 14px;">
                                                "label"
                                            </span>
                                        </button>

                                        {move || show_picker.get().then(|| {
                                            let all = available_tags.get_value();
                                            let current_ids: Vec<TagId> = node_tags_stored
                                                .get_value()
                                                .iter()
                                                .map(|t| t.id)
                                                .collect();
                                            view! {
                                                // Click-outside overlay — closes picker without navigating.
                                                <div
                                                    class="fixed inset-0 z-10"
                                                    on:click=move |ev: web_sys::MouseEvent| {
                                                        ev.stop_propagation();
                                                        show_picker.set(false);
                                                    }
                                                />
                                                // Dropdown panel
                                                <div class="absolute left-0 top-full mt-1 z-20 w-52
                                                    bg-white dark:bg-stone-900 rounded-xl shadow-xl
                                                    border border-stone-200 dark:border-stone-700
                                                    overflow-hidden">
                                                    {if all.is_empty() {
                                                        view! {
                                                            <div class="px-3 py-2 text-xs
                                                                text-stone-400 dark:text-stone-500">
                                                                "No tags defined"
                                                            </div>
                                                        }.into_any()
                                                    } else {
                                                        view! {
                                                            <div>
                                                                {all.into_iter().map(|tag| {
                                                                    let tag_id  = tag.id;
                                                                    let color   = tag.color.clone();
                                                                    let name    = tag.name.clone();
                                                                    let applied = current_ids.contains(&tag_id);
                                                                    let node_id = id;
                                                                    view! {
                                                                        <button
                                                                            class="w-full text-left px-3 py-1.5 text-xs
                                                                                flex items-center gap-2 transition-colors
                                                                                hover:bg-stone-50 dark:hover:bg-stone-800"
                                                                            on:click=move |ev| {
                                                                                ev.stop_propagation();
                                                                                show_picker.set(false);
                                                                                let r = refresh;
                                                                                wasm_bindgen_futures::spawn_local(async move {
                                                                                    let res = if applied {
                                                                                        crate::api::detach_tag(node_id, tag_id).await
                                                                                    } else {
                                                                                        crate::api::attach_tag(node_id, tag_id).await
                                                                                    };
                                                                                    if res.is_ok() {
                                                                                        r.update(|n| *n += 1);
                                                                                    }
                                                                                });
                                                                            }
                                                                        >
                                                                            // Colour swatch
                                                                            <span
                                                                                class="w-2 h-2 rounded-full flex-shrink-0"
                                                                                style=format!("background-color: {color}")
                                                                            />
                                                                            // Tag name
                                                                            <span class="flex-1 text-stone-700 dark:text-stone-300">
                                                                                {name}
                                                                            </span>
                                                                            // Checkmark if currently applied
                                                                            {applied.then(|| view! {
                                                                                <span
                                                                                    class="material-symbols-outlined
                                                                                        text-amber-500 dark:text-amber-400"
                                                                                    style="font-size: 14px;"
                                                                                >
                                                                                    "check"
                                                                                </span>
                                                                            })}
                                                                        </button>
                                                                    }
                                                                }).collect::<Vec<_>>()}
                                                            </div>
                                                        }.into_any()
                                                    }}
                                                </div>
                                            }
                                        })}
                                    </div>
                                </div>

                                // ── Row 3: body preview ──────────────────────────────
                                {node.body.as_deref().and_then(body_preview).map(|preview| view! {
                                    <p class="text-xs text-stone-500 dark:text-stone-400 mt-1 line-clamp-3">
                                        {preview}
                                    </p>
                                })}
                            </div>

                            // Right column: date + edge count badge
                            <div class="flex flex-col items-end gap-1 shrink-0 mt-0.5">
                                <span class="text-xs text-stone-400 dark:text-stone-500 whitespace-nowrap">
                                    {updated}
                                </span>
                                // Edge count badge — only shown when node has edges.
                                {(node.edge_count > 0).then(|| {
                                    let count = node.edge_count;
                                    view! {
                                        <span class="inline-flex items-center gap-0.5
                                                     text-xs text-stone-400 dark:text-stone-500"
                                              title=format!("{count} edge(s)")>
                                            <span class="material-symbols-outlined"
                                                  style="font-size: 12px;">
                                                "link"
                                            </span>
                                            {count}
                                        </span>
                                    }
                                })}
                            </div>
                        </div>
                        </div>
                    </li>
                }
            }).collect::<Vec<_>>()}
        </ul>
    }
}
