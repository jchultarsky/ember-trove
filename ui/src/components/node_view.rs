use std::collections::HashMap;

use common::edge::{CreateEdgeRequest, EdgeType, EdgeWithTitles};
use common::id::NodeId;
use common::node::NodeTitleEntry;
use leptos::{html::Input, prelude::*};

use crate::components::attachment_panel::AttachmentPanel;
use leptos_router::hooks::use_navigate;
use crate::components::modals::delete_confirm::DeleteConfirmModal;
use crate::components::node_meta::{status_color, status_icon, status_label, type_icon, type_label};
use crate::components::activity_panel::ActivityPanel;
use crate::components::permission_dialog::PermissionPanel;
use crate::components::share_panel::SharePanel;
use crate::components::version_panel::VersionPanel;
use crate::components::tag_bar::TagBar;
use crate::auth::{AuthStatus, use_auth_state};
use crate::components::note_panel::NotePanel;
use crate::components::task_panel::TaskPanel;
use crate::components::links_panel::LinksPanel;
use crate::components::toast::{ToastLevel, push_toast};
use crate::focus_task::schedule_focus_task;
use crate::markdown::render_markdown;

fn build_title_map(entries: &[NodeTitleEntry]) -> HashMap<String, NodeId> {
    entries.iter().map(|e| (e.title.clone(), e.id)).collect()
}

#[component]
pub fn NodeView(id: NodeId) -> impl IntoView {
    let navigate = StoredValue::new(use_navigate());
    let refresh = use_context::<RwSignal<u32>>().expect("refresh signal must be provided");

    let node = LocalResource::new(move || {
        let id = id;
        async move { crate::api::fetch_node(id).await }
    });

    // Fetch all node titles for wiki-link resolution.
    let titles = LocalResource::new(|| async move { crate::api::fetch_node_titles().await });

    let auth_state = use_auth_state();
    let deleting = RwSignal::new(false);
    let show_delete_confirm = RwSignal::new(false);

    // v2.6.2: when navigation arrived via the Kanban row click
    // (`/nodes/{id}?task=<task_id>`), find the task in the TaskPanel and
    // briefly highlight it.  See `crate::focus_task` — handles the
    // resource-load race by retrying a few times.
    schedule_focus_task();

    // Derive node title for the confirm dialog (empty string until node loads).
    let delete_item_name = Memo::new(move |_| {
        node.get()
            .and_then(|r| r.ok())
            .map(|n| n.title.clone())
            .unwrap_or_default()
    });

    let duplicating = RwSignal::new(false);
    let do_duplicate = move || {
        duplicating.set(true);
        let nav = navigate.get_value();
        wasm_bindgen_futures::spawn_local(async move {
            match crate::api::duplicate_node(id).await {
                Ok(dup) => {
                    push_toast(ToastLevel::Success, "Node duplicated.");
                    refresh.update(|n| *n += 1);
                    nav(&format!("/nodes/{}", dup.id), Default::default());
                }
                Err(e) => {
                    push_toast(ToastLevel::Error, format!("Duplicate failed: {e}"));
                }
            }
            duplicating.set(false);
        });
    };

    let do_delete = move || {
        deleting.set(true);
        show_delete_confirm.set(false);
        let id = id;
        wasm_bindgen_futures::spawn_local(async move {
            match crate::api::delete_node(id).await {
                Ok(_) => {
                    push_toast(ToastLevel::Success, "Node deleted.");
                    refresh.update(|n| *n += 1);
                    navigate.get_value()("/nodes", Default::default());
                }
                Err(e) => {
                    push_toast(ToastLevel::Error, format!("Delete failed: {e}"));
                }
            }
            deleting.set(false);
        });
    };

    view! {
        <Suspense fallback=move || view! {
            <crate::components::skeleton::SkeletonArticle />
        }>
            {move || {
                node.get().map(|result| {
                    match result {
                        Ok(n) => {
                            // Build title → NodeId map from fetched titles (empty map if not
                            // yet loaded — wiki-links will render as unresolved on first paint
                            // and resolve on the next reactive tick).
                            let title_map = titles
                                .get()
                                .and_then(|r| r.ok())
                                .map(|entries| build_title_map(&entries))
                                .unwrap_or_default();
                            let body_html = render_markdown(n.body.as_deref().unwrap_or(""), &title_map);
                            let node_type = format!("{:?}", n.node_type).to_lowercase();
                            let status = format!("{:?}", n.status).to_lowercase();
                            let edit_id = n.id;
                            // Ownership check: node owner OR any admin user.
                            let is_owner = if let AuthStatus::Authenticated(ref user) =
                                auth_state.get_untracked()
                            {
                                user.sub == n.owner_id
                                    || user.roles.contains(&"admin".to_string())
                            } else {
                                false
                            };

                            // Record this node visit in the localStorage recent list.
                            crate::recent::push_recent(
                                n.id.0,
                                n.title.clone(),
                                &node_type,
                            );

                            // Get navigate values for each inner closure that needs it.
                            let nav_wikilink = navigate.get_value();
                            let nav_back     = navigate.get_value();
                            let nav_edit     = navigate.get_value();

                            // Click delegation: intercept clicks on `.wikilink` anchors and
                            // navigate in-app instead of following the href.
                            let handle_wikilink_click = move |ev: leptos::ev::MouseEvent| {
                                use wasm_bindgen::JsCast;
                                if let Some(link) = ev
                                    .target()
                                    .and_then(|t| t.dyn_into::<web_sys::Element>().ok())
                                    .and_then(|el| el.closest(".wikilink").ok())
                                    .flatten()
                                {
                                    ev.prevent_default();
                                    if let Some(raw) = link.get_attribute("data-node-id")
                                        && let Ok(id) = raw.parse::<uuid::Uuid>() {
                                        nav_wikilink(&format!("/nodes/{id}"), Default::default());
                                    }
                                }
                            };

                            view! {
                                <div class="flex flex-col h-full">
                                    <div class="flex items-center justify-between px-6 py-4 border-b border-stone-200 dark:border-stone-800">
                                        <div class="flex items-center gap-3">
                                            <button
                                                class="text-stone-400 hover:text-stone-600 dark:hover:text-stone-300"
                                                on:click=move |_| nav_back("/nodes", Default::default())
                                            >
                                                <span class="material-symbols-outlined">"arrow_back"</span>
                                            </button>
                                            <h1 class="text-lg font-semibold text-stone-900 dark:text-stone-100">
                                                {n.title.clone()}
                                            </h1>
                                            // Type icon — same encoding as NodeList
                                            <span
                                                class="material-symbols-outlined text-stone-400 dark:text-stone-500"
                                                style="font-size: 18px;"
                                                title=type_label(&node_type)
                                            >
                                                {type_icon(&node_type)}
                                            </span>
                                            // Status icon — semantic colour, same as NodeList
                                            <span
                                                class="material-symbols-outlined"
                                                style=format!("font-size: 18px; {}",
                                                    status_color(&status))
                                                title=status_label(&status)
                                            >
                                                {status_icon(&status)}
                                            </span>
                                        </div>
                                        {
                                            // Overflow dropdown state — scoped to this rendered
                                            // node so switching nodes implicitly closes the menu.
                                            let menu_open = RwSignal::new(false);
                                            view! {
                                                <div class="flex items-center gap-1">
                                                    // Desktop cluster — all four actions visible
                                                    <div class="hidden md:flex items-center gap-1">
                                                        <a
                                                            href=format!("/api/nodes/{id}/export")
                                                            download=true
                                                            class="p-1.5 rounded-lg text-stone-400 hover:text-stone-600
                                                                dark:hover:text-stone-300 hover:bg-stone-100
                                                                dark:hover:bg-stone-800 transition-colors"
                                                            title="Export as Markdown"
                                                        >
                                                            <span class="material-symbols-outlined">"download"</span>
                                                        </a>
                                                        <button
                                                            class="p-1.5 rounded-lg text-stone-400 hover:text-stone-600
                                                                dark:hover:text-stone-300 hover:bg-stone-100
                                                                dark:hover:bg-stone-800 transition-colors"
                                                            on:click=move |_| nav_edit(&format!("/nodes/{edit_id}/edit"), Default::default())
                                                            title="Edit"
                                                        >
                                                            <span class="material-symbols-outlined">"edit"</span>
                                                        </button>
                                                        <button
                                                            class="p-1.5 rounded-lg text-stone-400 hover:text-amber-600
                                                                dark:hover:text-amber-400 hover:bg-amber-50
                                                                dark:hover:bg-amber-900/30 transition-colors
                                                                disabled:opacity-30"
                                                            on:click=move |_| do_duplicate()
                                                            disabled=move || duplicating.get()
                                                            title=move || if duplicating.get() { "Duplicating…" } else { "Duplicate" }
                                                        >
                                                            <span class="material-symbols-outlined">
                                                                {move || if duplicating.get() { "hourglass_empty" } else { "content_copy" }}
                                                            </span>
                                                        </button>
                                                        <button
                                                            class="p-1.5 rounded-lg text-stone-400 hover:text-red-600
                                                                dark:hover:text-red-400 hover:bg-red-50
                                                                dark:hover:bg-red-900/30 transition-colors
                                                                disabled:opacity-30"
                                                            on:click=move |_| show_delete_confirm.set(true)
                                                            disabled=move || deleting.get()
                                                            title=move || if deleting.get() { "Deleting…" } else { "Delete" }
                                                        >
                                                            <span class="material-symbols-outlined">
                                                                {move || if deleting.get() { "hourglass_empty" } else { "delete" }}
                                                            </span>
                                                        </button>
                                                    </div>

                                                    // Mobile overflow — single kebab opens a dropdown with
                                                    // the same four actions.  The header stops wrapping on
                                                    // narrow viewports.
                                                    <div class="md:hidden relative">
                                                        <button
                                                            class="p-1.5 rounded-lg text-stone-500 dark:text-stone-400
                                                                hover:text-stone-800 dark:hover:text-stone-100
                                                                transition-colors"
                                                            aria-haspopup="menu"
                                                            aria-expanded=move || if menu_open.get() { "true" } else { "false" }
                                                            title="Actions"
                                                            on:click=move |_| menu_open.update(|v| *v = !*v)
                                                        >
                                                            <span class="material-symbols-outlined">"more_vert"</span>
                                                        </button>
                                                        {move || menu_open.get().then(|| view! {
                                                            <>
                                                            // Click-outside backdrop
                                                            <div
                                                                class="fixed inset-0 z-20"
                                                                on:click=move |_| menu_open.set(false)
                                                            />
                                                            <div
                                                                class="absolute right-0 top-full mt-1 w-48 rounded-lg
                                                                    border border-stone-200 dark:border-stone-700
                                                                    bg-white dark:bg-stone-900 shadow-lg
                                                                    overflow-hidden z-30"
                                                                role="menu"
                                                            >
                                                                <a
                                                                    href=format!("/api/nodes/{id}/export")
                                                                    download=true
                                                                    class="flex items-center gap-2 px-3 py-2 text-sm
                                                                        text-stone-700 dark:text-stone-200
                                                                        hover:bg-stone-100 dark:hover:bg-stone-800"
                                                                    role="menuitem"
                                                                    on:click=move |_| menu_open.set(false)
                                                                >
                                                                    <span class="material-symbols-outlined" style="font-size:18px;">"download"</span>
                                                                    "Export as Markdown"
                                                                </a>
                                                                <button
                                                                    type="button"
                                                                    class="flex items-center gap-2 w-full text-left px-3 py-2 text-sm
                                                                        text-stone-700 dark:text-stone-200
                                                                        hover:bg-stone-100 dark:hover:bg-stone-800"
                                                                    role="menuitem"
                                                                    on:click=move |_| {
                                                                        menu_open.set(false);
                                                                        navigate.get_value()(&format!("/nodes/{edit_id}/edit"), Default::default());
                                                                    }
                                                                >
                                                                    <span class="material-symbols-outlined" style="font-size:18px;">"edit"</span>
                                                                    "Edit"
                                                                </button>
                                                                <button
                                                                    type="button"
                                                                    class="flex items-center gap-2 w-full text-left px-3 py-2 text-sm
                                                                        text-stone-700 dark:text-stone-200
                                                                        hover:bg-amber-50 dark:hover:bg-amber-900/30
                                                                        hover:text-amber-700 dark:hover:text-amber-400
                                                                        disabled:opacity-40"
                                                                    role="menuitem"
                                                                    disabled=move || duplicating.get()
                                                                    on:click=move |_| { menu_open.set(false); do_duplicate(); }
                                                                >
                                                                    <span class="material-symbols-outlined" style="font-size:18px;">
                                                                        {move || if duplicating.get() { "hourglass_empty" } else { "content_copy" }}
                                                                    </span>
                                                                    {move || if duplicating.get() { "Duplicating…" } else { "Duplicate" }}
                                                                </button>
                                                                <button
                                                                    type="button"
                                                                    class="flex items-center gap-2 w-full text-left px-3 py-2 text-sm
                                                                        text-red-600 dark:text-red-400
                                                                        hover:bg-red-50 dark:hover:bg-red-900/30
                                                                        disabled:opacity-40"
                                                                    role="menuitem"
                                                                    disabled=move || deleting.get()
                                                                    on:click=move |_| { menu_open.set(false); show_delete_confirm.set(true); }
                                                                >
                                                                    <span class="material-symbols-outlined" style="font-size:18px;">
                                                                        {move || if deleting.get() { "hourglass_empty" } else { "delete" }}
                                                                    </span>
                                                                    {move || if deleting.get() { "Deleting…" } else { "Delete" }}
                                                                </button>
                                                            </div>
                                                            </>
                                                        })}
                                                    </div>
                                                </div>
                                            }
                                        }
                                    </div>
                                    // Tags
                                    <div class="border-b border-stone-200 dark:border-stone-800">
                                        <TagBar node_id=id />
                                    </div>
                                    <div class="flex-1 overflow-auto p-6">
                                        <div
                                            class="prose max-w-none dark:prose-invert"
                                            inner_html=body_html
                                            on:click=handle_wikilink_click
                                        />
                                        // Task panel — shown for all node types
                                        <TaskPanel node_id=id />
                                        // Note panel — append-only; owner can add, everyone can read
                                        <NotePanel node_id=id is_owner=is_owner />
                                        // External links panel — grouped with other collapsible sections
                                        <LinksPanel node_id=id is_editor=is_owner />
                                        <EdgePanel node_id=id />
                                        <BacklinksPanel node_id=id />
                                        <AttachmentPanel node_id=id />
                                        <PermissionPanel node_id=id is_owner=is_owner />
                                        <SharePanel node_id=id is_owner=is_owner />
                                        <VersionPanel
                                            node_id=id
                                            is_editor=is_owner
                                            on_restore=Callback::new(move |_| refresh.update(|n| *n += 1))
                                        />
                                        <ActivityPanel node_id=id />
                                    </div>
                                </div>
                            }.into_any()
                        }
                        Err(e) => view! {
                            <div class="p-6 text-red-500 text-sm">{format!("Error: {e}")}</div>
                        }.into_any(),
                    }
                })
            }}
        </Suspense>

        <DeleteConfirmModal
            show=show_delete_confirm.read_only()
            item_name=Signal::derive(move || delete_item_name.get())
            on_confirm=Callback::new(move |_| do_delete())
            on_cancel=Callback::new(move |_| show_delete_confirm.set(false))
        />
    }
}

/// Shows edges (incoming + outgoing) for a node, with ability to add/remove.
#[component]
fn EdgePanel(node_id: NodeId) -> impl IntoView {
    let navigate = StoredValue::new(use_navigate());
    let open = RwSignal::new(false);
    let refresh_edges = RwSignal::new(0u32);
    let show_add = RwSignal::new(false);
    let target_id_input = RwSignal::new(String::new());
    let edge_type_input = RwSignal::new("references".to_string());
    let label_input = RwSignal::new(String::new());
    let error_msg = RwSignal::new(Option::<String>::None);
    let node_search_query = RwSignal::new(String::new());
    let search_open = RwSignal::new(false);
    let search_input_ref = NodeRef::<Input>::new();

    let picker_results: LocalResource<Vec<common::search::SearchResult>> =
        LocalResource::new(move || {
            let q = node_search_query.get();
            async move {
                if q.len() < 2 {
                    return Vec::new();
                }
                crate::api::search_nodes(&q, false, None, None, &[], "or", None, None, None, 1, 6)
                    .await
                    .map(|r| r.results)
                    .unwrap_or_default()
            }
        });

    let edges: LocalResource<Result<Vec<EdgeWithTitles>, crate::error::UiError>> =
        LocalResource::new(move || {
            let _ = refresh_edges.get();
            let is_open = open.get();
            let node_id = node_id;
            async move {
                if !is_open { return Ok(vec![]); }
                crate::api::fetch_edges_for_node(node_id).await
            }
        });

    let on_add_edge = move |_| {
        let target_str = target_id_input.get_untracked().trim().to_string();
        let et = edge_type_input.get_untracked();
        let label = label_input.get_untracked().trim().to_string();
        error_msg.set(None);

        let target_uuid = match uuid::Uuid::parse_str(&target_str) {
            Ok(u) => u,
            Err(_) => {
                error_msg.set(Some("Invalid target node UUID".to_string()));
                return;
            }
        };

        let edge_type = match et.as_str() {
            "references" => EdgeType::References,
            "contains" => EdgeType::Contains,
            "related_to" => EdgeType::RelatedTo,
            "depends_on" => EdgeType::DependsOn,
            "derived_from" => EdgeType::DerivedFrom,
            _ => EdgeType::References,
        };

        let req = CreateEdgeRequest {
            source_id: node_id,
            target_id: common::id::NodeId(target_uuid),
            edge_type,
            label: if label.is_empty() { None } else { Some(label) },
        };

        wasm_bindgen_futures::spawn_local(async move {
            match crate::api::create_edge(&req).await {
                Ok(_) => {
                    show_add.set(false);
                    target_id_input.set(String::new());
                    node_search_query.set(String::new());
                    if let Some(el) = search_input_ref.get_untracked() {
                        el.set_value("");
                    }
                    label_input.set(String::new());
                    refresh_edges.update(|n| *n += 1);
                }
                Err(e) => error_msg.set(Some(format!("{e}"))),
            }
        });
    };

    view! {
        <div class="mt-8 border-t border-stone-200 dark:border-stone-700 pt-6">
            <div class="flex items-center justify-between">
                <button
                    class="flex items-center gap-1 text-left cursor-pointer"
                    on:click=move |_| open.update(|v| *v = !*v)
                >
                    <span
                        class="material-symbols-outlined text-stone-400 dark:text-stone-500"
                        style="font-size: 16px;"
                    >
                        {move || if open.get() { "expand_more" } else { "chevron_right" }}
                    </span>
                    <span
                        class="material-symbols-outlined text-stone-400 dark:text-stone-500"
                        style="font-size: 15px;"
                    >
                        "hub"
                    </span>
                    <h2 class="text-sm font-semibold text-stone-700 dark:text-stone-300">
                        "Connections"
                    </h2>
                    <Suspense fallback=|| ()>
                    {move || {
                        edges.with(|r| r.as_ref().and_then(|res| match res {
                            Ok(v) if !v.is_empty() => Some(view! {
                                <span class="ml-1 text-xs bg-stone-200 dark:bg-stone-700
                                            text-stone-600 dark:text-stone-300
                                            rounded-full px-1.5 py-0.5">
                                    {v.len()}
                                </span>
                            }),
                            _ => None,
                        }))
                    }}
                    </Suspense>
                </button>
                {move || open.get().then(|| view! {
                    <button
                        class="p-1.5 rounded-lg text-stone-400 hover:text-stone-600
                            dark:hover:text-stone-300 hover:bg-stone-100
                            dark:hover:bg-stone-800 transition-colors"
                        on:click=move |_| show_add.update(|v| *v = !*v)
                        title=move || if show_add.get() { "Cancel" } else { "Add Edge" }
                    >
                        <span class="material-symbols-outlined" style="font-size: 16px;">
                            {move || if show_add.get() { "close" } else { "add_link" }}
                        </span>
                    </button>
                })}
            </div>

            // Add edge form + edge list (only when expanded)
            {move || open.get().then(|| view! {
            <div class="mt-4">
            // Add edge form
            {move || show_add.get().then(|| view! {
                <div class="mb-4 p-3 bg-stone-50 dark:bg-stone-900 rounded-lg space-y-2">
                    <div class="flex gap-2">
                        <div class="relative flex-1">
                            <input
                                type="text"
                                node_ref=search_input_ref
                                class="w-full px-2 py-1 text-xs rounded border border-stone-300 dark:border-stone-600
                                    bg-transparent text-stone-900 dark:text-stone-100 focus:outline-none
                                    focus:ring-1 focus:ring-amber-500"
                                placeholder="Search for a node…"
                                on:input=move |ev| {
                                    let v = event_target_value(&ev);
                                    node_search_query.set(v.clone());
                                    target_id_input.set(v);
                                    search_open.set(true);
                                }
                                on:focus=move |_| search_open.set(true)
                                on:blur=move |_| search_open.set(false)
                            />
                            <Suspense fallback=|| ()>
                            {move || {
                                let results = picker_results.get().unwrap_or_default();
                                if !search_open.get() || results.is_empty() {
                                    return None;
                                }
                                Some(view! {
                                    <div class="absolute z-30 top-full left-0 right-0 mt-0.5
                                        bg-white dark:bg-stone-900 border border-stone-200 dark:border-stone-700
                                        rounded-lg shadow-lg overflow-hidden">
                                        {results.into_iter().map(|r| {
                                            let id_str = r.node_id.to_string();
                                            let title = r.title;
                                            let title2 = title.clone();
                                            view! {
                                                <button
                                                    class="w-full text-left px-3 py-1.5 text-xs
                                                        hover:bg-stone-100 dark:hover:bg-stone-800
                                                        text-stone-700 dark:text-stone-300"
                                                    on:mousedown=move |_| {
                                                        target_id_input.set(id_str.clone());
                                                        node_search_query.set(title.clone());
                                                        if let Some(el) = search_input_ref.get_untracked() {
                                                            el.set_value(&title);
                                                        }
                                                        search_open.set(false);
                                                    }
                                                >
                                                    {title2}
                                                </button>
                                            }
                                        }).collect::<Vec<_>>()}
                                    </div>
                                })
                            }}
                            </Suspense></div>
                        <select
                            class="px-2 py-1 text-xs rounded border border-stone-300 dark:border-stone-600
                                bg-stone-50 dark:bg-stone-800 text-stone-700 dark:text-stone-300
                                focus:outline-none"
                            prop:value=move || edge_type_input.get()
                            on:change=move |ev| edge_type_input.set(event_target_value(&ev))
                        >
                            <option value="references">"References"</option>
                            <option value="contains">"Contains"</option>
                            <option value="related_to">"Related To"</option>
                            <option value="depends_on">"Depends On"</option>
                            <option value="derived_from">"Derived From"</option>
                        </select>
                    </div>
                    <div class="flex gap-2">
                        <input
                            type="text"
                            class="flex-1 px-2 py-1 text-xs rounded border border-stone-300 dark:border-stone-600
                                bg-transparent text-stone-900 dark:text-stone-100 focus:outline-none
                                focus:ring-1 focus:ring-amber-500"
                            placeholder="Label (optional)..."
                            prop:value=move || label_input.get()
                            on:input=move |ev| label_input.set(event_target_value(&ev))
                        />
                        <button
                            class="p-1.5 rounded-lg text-stone-400 hover:text-stone-600
                                dark:hover:text-stone-300 hover:bg-stone-100
                                dark:hover:bg-stone-800 transition-colors"
                            on:click=on_add_edge
                            title="Add edge"
                        >
                            <span class="material-symbols-outlined" style="font-size: 16px;">"check"</span>
                        </button>
                    </div>
                    {move || error_msg.get().map(|msg| view! {
                        <div class="text-xs text-red-500">{msg}</div>
                    })}
                </div>
            })}

            // Edge list
            <Suspense fallback=|| view! {
                <div class="text-xs text-stone-400">"Loading edges..."</div>
            }>
                {move || {
                    edges.get().map(|result| {
                        match result {
                            Ok(edge_list) if edge_list.is_empty() => {
                                view! {
                                    <div class="flex flex-col items-center gap-2 py-6">
                                        <span
                                            class="material-symbols-outlined text-stone-300 dark:text-stone-700"
                                            style="font-size: 32px;"
                                        >
                                            "hub"
                                        </span>
                                        <p class="text-xs text-stone-400 dark:text-stone-600">
                                            "No connections yet."
                                        </p>
                                    </div>
                                }.into_any()
                            }
                            Ok(edge_list) => {
                                view! {
                                    <div class="space-y-1">
                                        {edge_list.into_iter().map(|edge| {
                                            let nav = navigate.get_value();
                                            let edge_id = edge.id;
                                            let is_outgoing = edge.source_id == node_id;
                                            let other_id = if is_outgoing { edge.target_id } else { edge.source_id };
                                            let other_title = if is_outgoing { edge.target_title.clone() } else { edge.source_title.clone() };
                                            let direction = if is_outgoing { "\u{2192}" } else { "\u{2190}" };
                                            let edge_type_label = format!("{:?}", edge.edge_type).to_lowercase().replace('_', " ");
                                            let label = edge.label.clone().unwrap_or_default();
                                            view! {
                                                <div class="flex items-center justify-between py-1.5 px-2 rounded
                                                    hover:bg-stone-50 dark:hover:bg-stone-800/50 group">
                                                    <button
                                                        class="flex items-center gap-2 text-xs text-stone-600 dark:text-stone-400
                                                            hover:text-amber-600 dark:hover:text-amber-400 min-w-0"
                                                        on:click=move |_| nav(&format!("/nodes/{other_id}"), Default::default())
                                                    >
                                                        <span class="shrink-0">{direction}</span>
                                                        <span class="text-stone-400 dark:text-stone-500 shrink-0">{edge_type_label}</span>
                                                        <span class="font-medium truncate">{other_title}</span>
                                                        {(!label.is_empty()).then(|| view! {
                                                            <span class="italic text-stone-400 shrink-0">{format!("({label})")}</span>
                                                        })}
                                                    </button>
                                                    <button
                                                        class="opacity-0 group-hover:opacity-100 text-red-400 hover:text-red-600
                                                            text-xs transition-opacity"
                                                        on:click=move |_| {
                                                            wasm_bindgen_futures::spawn_local(async move {
                                                                let _ = crate::api::delete_edge(edge_id).await;
                                                                refresh_edges.update(|n| *n += 1);
                                                            });
                                                        }
                                                    >
                                                        "\u{00d7}"
                                                    </button>
                                                </div>
                                            }
                                        }).collect::<Vec<_>>()}
                                    </div>
                                }.into_any()
                            }
                            Err(e) => view! {
                                <div class="text-xs text-red-500">{format!("Error: {e}")}</div>
                            }.into_any(),
                        }
                    })
                }}
            </Suspense>
            </div>  // close mt-4
            })}  // close open.then
        </div>
    }
}

/// Shows nodes that link to this node (incoming edges from other nodes).
#[component]
fn BacklinksPanel(node_id: NodeId) -> impl IntoView {
    let navigate = StoredValue::new(use_navigate());
    let open = RwSignal::new(false);

    let backlinks = LocalResource::new(move || {
        let is_open = open.get();
        let node_id = node_id;
        async move {
            if !is_open { return Ok(vec![]); }
            crate::api::fetch_backlinks(node_id).await
        }
    });

    view! {
        <div class="mt-8 border-t border-stone-200 dark:border-stone-700 pt-6">
            <button
                class="flex items-center gap-1 text-left cursor-pointer"
                on:click=move |_| open.update(|v| *v = !*v)
            >
                <span
                    class="material-symbols-outlined text-stone-400 dark:text-stone-500"
                    style="font-size: 16px;"
                >
                    {move || if open.get() { "expand_more" } else { "chevron_right" }}
                </span>
                <span
                    class="material-symbols-outlined text-stone-400 dark:text-stone-500"
                    style="font-size: 15px;"
                >
                    "link"
                </span>
                <h2 class="text-sm font-semibold text-stone-700 dark:text-stone-300">
                    "Linked Here"
                </h2>
                <Suspense fallback=|| ()>
                {move || {
                    backlinks.with(|r| r.as_ref().and_then(|res| match res {
                        Ok(v) if !v.is_empty() => Some(view! {
                            <span class="ml-1 text-xs bg-stone-200 dark:bg-stone-700
                                        text-stone-600 dark:text-stone-300
                                        rounded-full px-1.5 py-0.5">
                                {v.len()}
                            </span>
                        }),
                        _ => None,
                    }))
                }}
                </Suspense>
            </button>
            {move || open.get().then(|| view! {
                <div class="mt-4">
                <Suspense fallback=|| view! {
                    <div class="text-xs text-stone-400">"Loading backlinks..."</div>
                }>
                    {move || {
                        backlinks.get().map(|result| {
                            match result {
                                Ok(nodes) if nodes.is_empty() => view! {
                                    <div class="flex flex-col items-center gap-2 py-6">
                                        <span
                                            class="material-symbols-outlined text-stone-300 dark:text-stone-700"
                                            style="font-size: 32px;"
                                        >
                                            "link_off"
                                        </span>
                                        <p class="text-xs text-stone-400 dark:text-stone-600">
                                            "No other notes link here."
                                        </p>
                                    </div>
                                }.into_any(),
                                Ok(nodes) => view! {
                                    <div class="space-y-1">
                                        {nodes.into_iter().map(|node| {
                                            let nav = navigate.get_value();
                                            let node_id = node.id;
                                            let title = node.title.clone();
                                            let node_type = format!("{:?}", node.node_type).to_lowercase();
                                            view! {
                                                <button
                                                    class="flex items-center gap-2 w-full text-left py-1.5 px-2 rounded
                                                        text-xs hover:bg-stone-50 dark:hover:bg-stone-800/50
                                                        text-stone-600 dark:text-stone-400
                                                        hover:text-amber-600 dark:hover:text-amber-400"
                                                    on:click=move |_| nav(&format!("/nodes/{node_id}"), Default::default())
                                                >
                                                    <span class="material-symbols-outlined text-stone-400 dark:text-stone-600"
                                                        style="font-size: 14px;">"arrow_back"</span>
                                                    <span class="font-medium truncate">{title}</span>
                                                    <span class="text-stone-400 dark:text-stone-600 shrink-0">
                                                        {format!("({node_type})")}
                                                    </span>
                                                </button>
                                            }
                                        }).collect::<Vec<_>>()}
                                    </div>
                                }.into_any(),
                                Err(e) => view! {
                                    <div class="text-xs text-red-500">{format!("Error: {e}")}</div>
                                }.into_any(),
                            }
                        })
                    }}
                </Suspense>
                </div>
            })}
        </div>
    }
}
