use common::{favorite::Favorite, id::FavoriteId};
use leptos::prelude::*;
use wasm_bindgen_futures::spawn_local;

use crate::{
    api::{delete_favorite, fetch_favorites, reorder_favorites},
    app::{FavoritesRefresh, storage_get, storage_set},
    components::{
        layout::SidebarCollapsed,
        modals::add_favorite::AddFavoriteModal,
        toast::{ToastLevel, push_toast},
    },
};
use leptos_router::hooks::use_navigate;

// localStorage keys for each sub-group's expand state.  Default false
// (collapsed) so the sidebar stays compact as the lists grow.
const LS_WEB_EXPANDED: &str = "et.fav.web.expanded";
const LS_NODES_EXPANDED: &str = "et.fav.nodes.expanded";

fn read_expanded(key: &str) -> bool {
    storage_get(key).as_deref() == Some("1")
}

fn write_expanded(key: &str, value: bool) {
    storage_set(key, if value { "1" } else { "0" });
}

/// Sidebar section that shows the user's pinned favorites split into two
/// sub-groups: external Web Links (sky accent) on top, internal Nodes (amber
/// accent) below.  Position: below search, above "All Nodes".
#[component]
pub fn FavoritesSection(collapsed: SidebarCollapsed, on_nav: Callback<()>) -> impl IntoView {
    let favorites: RwSignal<Vec<Favorite>> = RwSignal::new(vec![]);
    let show_modal = RwSignal::new(false);

    // Expand state for the two sub-groups — persisted per-user in
    // localStorage so each section remembers its own state across reloads.
    let web_expanded = RwSignal::new(read_expanded(LS_WEB_EXPANDED));
    let nodes_expanded = RwSignal::new(read_expanded(LS_NODES_EXPANDED));
    Effect::new(move |_| write_expanded(LS_WEB_EXPANDED, web_expanded.get()));
    Effect::new(move |_| write_expanded(LS_NODES_EXPANDED, nodes_expanded.get()));

    // Re-fetch whenever NodeView pins/unpins a node (FavoritesRefresh bumped).
    let fav_refresh = use_context::<FavoritesRefresh>().map(|fr| fr.0);
    Effect::new(move |_| {
        // Track the refresh counter so this effect re-runs on every bump.
        if let Some(r) = fav_refresh {
            let _ = r.get();
        }
        spawn_local(async move {
            if let Ok(favs) = fetch_favorites().await {
                favorites.set(favs);
            }
        });
    });

    let on_added = Callback::new(move |fav: Favorite| {
        favorites.update(|list| list.push(fav));
        // Bump FavoritesRefresh so any open NodeView re-fetches and syncs its pin button.
        if let Some(r) = fav_refresh {
            r.update(|n| *n += 1);
        }
    });

    let on_delete = Callback::new(move |id: FavoriteId| {
        favorites.update(|list| list.retain(|f| f.id != id));
        spawn_local(async move {
            if let Err(e) = delete_favorite(id).await {
                push_toast(ToastLevel::Error, format!("Failed to remove favorite: {e}"));
            }
        });
    });

    // Move an item up within its type-group.
    // Finds the previous item of the same type in the full list and swaps them
    // so the global position ordering is kept consistent with the server.
    let move_up = Callback::new(move |id: FavoriteId| {
        favorites.update(|list| {
            if let Some(idx) = list.iter().position(|f| f.id == id) {
                let is_url = list[idx].url.is_some();
                if let Some(prev) = (0..idx).rev().find(|&i| list[i].url.is_some() == is_url) {
                    list.swap(idx, prev);
                }
            }
        });
        let ids: Vec<uuid::Uuid> = favorites.get_untracked().iter().map(|f| f.id.0).collect();
        spawn_local(async move {
            let req = common::favorite::ReorderFavoritesRequest { ids };
            if let Err(e) = reorder_favorites(&req).await {
                push_toast(ToastLevel::Error, format!("Reorder failed: {e}"));
            }
        });
    });

    // Move an item down within its type-group.
    let move_down = Callback::new(move |id: FavoriteId| {
        favorites.update(|list| {
            if let Some(idx) = list.iter().position(|f| f.id == id) {
                let is_url = list[idx].url.is_some();
                let len = list.len();
                if let Some(next) = (idx + 1..len).find(|&i| list[i].url.is_some() == is_url) {
                    list.swap(idx, next);
                }
            }
        });
        let ids: Vec<uuid::Uuid> = favorites.get_untracked().iter().map(|f| f.id.0).collect();
        spawn_local(async move {
            let req = common::favorite::ReorderFavoritesRequest { ids };
            if let Err(e) = reorder_favorites(&req).await {
                push_toast(ToastLevel::Error, format!("Reorder failed: {e}"));
            }
        });
    });

    view! {
        // ── Collapsed: single star icon that opens the add modal ──────────────
        {move || {
            if collapsed.get() {
                return view! {
                    <button
                        class="flex items-center justify-center w-full py-2 rounded-lg
                               text-stone-700 dark:text-stone-300 hover:bg-stone-100
                               dark:hover:bg-stone-800 transition-colors cursor-pointer"
                        title="Favorites"
                        on:click=move |_| show_modal.set(true)
                    >
                        <span class="material-symbols-outlined text-stone-500 dark:text-stone-400">"star"</span>
                    </button>
                }.into_any();
            }

            // ── Expanded: split into Web Links + Nodes sub-groups ─────────────
            view! {
                <div>
                    // Section header
                    <div class="flex items-center justify-between px-3 mb-1">
                        <span class="text-xs font-semibold uppercase tracking-wider
                                     text-stone-500 dark:text-stone-400">
                            "Favorites"
                        </span>
                        <button
                            class="text-stone-400 hover:text-amber-500 dark:hover:text-amber-400
                                   transition-colors cursor-pointer"
                            title="Add favorite"
                            on:click=move |_| show_modal.set(true)
                        >
                            <span class="material-symbols-outlined" style="font-size: 18px">"add"</span>
                        </button>
                    </div>

                    {move || {
                        let list = favorites.get();

                        if list.is_empty() {
                            return view! {
                                <p class="px-3 py-1 text-xs text-stone-400 dark:text-stone-500 italic">
                                    "No favorites yet"
                                </p>
                            }.into_any();
                        }

                        // Partition preserving original relative order.
                        let (web_favs, node_favs): (Vec<_>, Vec<_>) =
                            list.iter().cloned().partition(|f| f.url.is_some());

                        let has_web   = !web_favs.is_empty();
                        let has_nodes = !node_favs.is_empty();
                        let web_len   = web_favs.len();
                        let node_len  = node_favs.len();

                        view! {
                            // ── Web Links sub-group (collapsible) ──────────────
                            {has_web.then(|| {
                                let rows_data = web_favs.clone();
                                view! {
                                    <FavoriteSubheader
                                        icon="public"
                                        icon_class="material-symbols-outlined text-sky-400 dark:text-sky-500"
                                        label_class="text-[10px] font-semibold uppercase tracking-widest \
                                                     text-sky-500/60 dark:text-sky-400/50"
                                        label="Web Links"
                                        count=web_len
                                        expanded=web_expanded
                                    />
                                    {move || {
                                        if !web_expanded.get() { return None; }
                                        let rows = rows_data.clone().into_iter().enumerate().map(|(i, fav)| {
                                            let fav_id = fav.id;
                                            let label  = fav.label.clone();
                                            let url    = fav.url.clone();
                                            view! {
                                                <FavoriteRow
                                                    label=label
                                                    url=url
                                                    node_id=fav.node_id
                                                    fav_id=fav_id
                                                    is_first=i == 0
                                                    is_last=i + 1 == web_len
                                                    on_delete=on_delete
                                                    on_move_up=move_up
                                                    on_move_down=move_down
                                                    on_nav=on_nav
                                                />
                                            }
                                        }).collect_view();
                                        Some(rows)
                                    }}
                                }
                            })}

                            // Divider — only when both groups are non-empty
                            {(has_web && has_nodes).then(|| view! {
                                <div class="mx-3 my-1.5 border-t
                                            border-stone-200 dark:border-stone-700/60" />
                            })}

                            // ── Nodes sub-group (collapsible) ──────────────────
                            {has_nodes.then(|| {
                                let rows_data = node_favs.clone();
                                view! {
                                    <FavoriteSubheader
                                        icon="star"
                                        icon_class="material-symbols-outlined text-amber-400 dark:text-amber-500"
                                        label_class="text-[10px] font-semibold uppercase tracking-widest \
                                                     text-amber-500/60 dark:text-amber-400/50"
                                        label="Nodes"
                                        count=node_len
                                        expanded=nodes_expanded
                                    />
                                    {move || {
                                        if !nodes_expanded.get() { return None; }
                                        let rows = rows_data.clone().into_iter().enumerate().map(|(i, fav)| {
                                            let fav_id = fav.id;
                                            let label  = fav.label.clone();
                                            let url    = fav.url.clone();
                                            view! {
                                                <FavoriteRow
                                                    label=label
                                                    url=url
                                                    node_id=fav.node_id
                                                    fav_id=fav_id
                                                    is_first=i == 0
                                                    is_last=i + 1 == node_len
                                                    on_delete=on_delete
                                                    on_move_up=move_up
                                                    on_move_down=move_down
                                                    on_nav=on_nav
                                                />
                                            }
                                        }).collect_view();
                                        Some(rows)
                                    }}
                                }
                            })}
                        }.into_any()
                    }}
                </div>
            }.into_any()
        }}

        // Modal
        <AddFavoriteModal
            show=Signal::derive(move || show_modal.get())
            on_close=Callback::new(move |()| show_modal.set(false))
            on_added=on_added
        />
    }
}

/// Header row for a favorites sub-group.  Acts as a button that toggles the
/// `expanded` signal; shows a chevron indicator + item count so the section
/// is useful even when collapsed.
#[component]
fn FavoriteSubheader(
    icon: &'static str,
    icon_class: &'static str,
    label_class: &'static str,
    label: &'static str,
    count: usize,
    expanded: RwSignal<bool>,
) -> impl IntoView {
    view! {
        <button
            type="button"
            class="w-full flex items-center gap-1.5 px-3 py-0.5 rounded-lg
                   hover:bg-stone-100 dark:hover:bg-stone-800 transition-colors
                   cursor-pointer text-left"
            aria-expanded=move || if expanded.get() { "true" } else { "false" }
            title=move || if expanded.get() {
                format!("Hide {label}")
            } else {
                format!("Show {label} ({count})")
            }
            on:click=move |_| expanded.update(|v| *v = !*v)
        >
            <span class="material-symbols-outlined text-stone-400 dark:text-stone-500 shrink-0"
                  style="font-size: 14px;">
                {move || if expanded.get() { "expand_more" } else { "chevron_right" }}
            </span>
            <span class=icon_class style="font-size: 12px;">{icon}</span>
            <span class=format!("{label_class} select-none")>{label}</span>
            <span class="ml-auto text-[10px] text-stone-400 dark:text-stone-500 tabular-nums select-none">
                {count}
            </span>
        </button>
    }
}

#[component]
#[allow(clippy::too_many_arguments)]
fn FavoriteRow(
    label: String,
    url: Option<String>,
    node_id: Option<common::id::NodeId>,
    fav_id: FavoriteId,
    is_first: bool,
    is_last: bool,
    on_delete: Callback<FavoriteId>,
    on_move_up: Callback<FavoriteId>,
    on_move_down: Callback<FavoriteId>,
    on_nav: Callback<()>,
) -> impl IntoView {
    let navigate = use_navigate();
    let label2 = label.clone();
    let url2 = url.clone();
    let is_url = url.is_some();

    let handle_click = move || {
        if let Some(nid) = node_id {
            navigate(&format!("/nodes/{nid}"), Default::default());
            on_nav.run(());
        } else if let Some(ref u) = url2
            && let Some(win) = web_sys::window()
        {
            let _ = win.open_with_url_and_target(u, "_blank");
        }
    };

    // Icon and accent colour differ by type.
    let (icon, icon_class) = if is_url {
        (
            "open_in_new",
            "material-symbols-outlined text-sky-400 dark:text-sky-500 shrink-0",
        )
    } else {
        (
            "star",
            "material-symbols-outlined text-amber-400 dark:text-amber-500 shrink-0",
        )
    };

    view! {
        <div class="group flex items-center gap-1 px-2 py-0.5 rounded-lg
                    hover:bg-stone-100 dark:hover:bg-stone-800 transition-colors">
            <span class=icon_class style="font-size: 14px">{icon}</span>

            <button
                class="flex-1 text-left text-sm text-stone-700 dark:text-stone-300
                       truncate cursor-pointer py-1"
                title=label.clone()
                on:click=move |_| handle_click()
            >
                {label2}
            </button>

            // Hover controls
            <div class="hidden group-hover:flex items-center gap-0.5 shrink-0">
                <button
                    class="p-0.5 text-stone-400 hover:text-stone-600 dark:hover:text-stone-300
                           cursor-pointer disabled:opacity-30 disabled:cursor-default"
                    title="Move up"
                    disabled=is_first
                    on:click=move |_| on_move_up.run(fav_id)
                >
                    <span class="material-symbols-outlined" style="font-size: 14px">
                        "keyboard_arrow_up"
                    </span>
                </button>
                <button
                    class="p-0.5 text-stone-400 hover:text-stone-600 dark:hover:text-stone-300
                           cursor-pointer disabled:opacity-30 disabled:cursor-default"
                    title="Move down"
                    disabled=is_last
                    on:click=move |_| on_move_down.run(fav_id)
                >
                    <span class="material-symbols-outlined" style="font-size: 14px">
                        "keyboard_arrow_down"
                    </span>
                </button>
                <button
                    class="p-0.5 text-stone-400 hover:text-red-500 cursor-pointer"
                    title="Remove from Favorites"
                    on:click=move |_| on_delete.run(fav_id)
                >
                    <span class="material-symbols-outlined" style="font-size: 14px">"delete"</span>
                </button>
            </div>
        </div>
    }
}
