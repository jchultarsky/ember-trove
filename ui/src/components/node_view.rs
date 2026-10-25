use std::collections::HashMap;

use common::edge::{CreateEdgeRequest, EdgeType, EdgeWithTitles};
use common::id::NodeId;
use common::node::NodeTitleEntry;
use leptos::prelude::*;

use crate::auth::{AuthStatus, use_auth_state};
use crate::components::activity_panel::ActivityPanel;
use crate::components::attachment_panel::AttachmentPanel;
use crate::components::links_panel::LinksPanel;
use crate::components::modals::delete_confirm::DeleteConfirmModal;
use crate::components::node_meta::{
    status_color, status_icon, status_label, type_icon, type_label,
};
use crate::components::note_panel::NotePanel;
use crate::components::permission_dialog::PermissionPanel;
use crate::components::share_panel::SharePanel;
use crate::components::tag_bar::TagBar;
use crate::components::task_panel::TaskPanel;
use crate::components::toast::{ToastLevel, push_toast};
use crate::components::version_panel::VersionPanel;
use crate::focus_task::schedule_focus_task;
use crate::markdown::render_markdown;
use leptos_router::hooks::use_navigate;

fn build_title_map(entries: &[NodeTitleEntry]) -> HashMap<String, NodeId> {
    entries.iter().map(|e| (e.title.clone(), e.id)).collect()
}

/// A node-page section the header's "Jump to …" actions can reveal (#123).
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum NodeSection {
    Tasks,
    Notes,
}

/// The pending "Jump to …" request, provided by `NodeView`. Each section
/// handles its own (`on_section_jump`), so the scroll and focus logic lives
/// with the section — Notes, for one, has to expand itself first.
#[derive(Clone, Copy)]
pub struct SectionJump(pub RwSignal<Option<NodeSection>>);

/// Reveal `root` when the header asks for `section`: run `before` (e.g.
/// expand), focus the section (a `tabindex="-1"` region, so keyboard and
/// screen-reader users land there too) and align it to the top.
pub fn on_section_jump(
    section: NodeSection,
    root: NodeRef<leptos::html::Div>,
    before: impl Fn() + 'static,
) {
    let Some(SectionJump(jump)) = use_context::<SectionJump>() else {
        return;
    };
    Effect::new(move |_| {
        if jump.get() != Some(section) {
            return;
        }
        before();
        if let Some(el) = root.get_untracked() {
            let _ = el.focus();
            el.scroll_into_view_with_bool(true);
        }
        jump.set(None);
    });
}

#[component]
pub fn NodeView(id: NodeId) -> impl IntoView {
    let navigate = StoredValue::new(use_navigate());
    let refresh = expect_context::<RwSignal<u32>>();

    let node = LocalResource::new(move || {
        let id = id;
        async move { crate::api::fetch_node(id).await }
    });

    // Fetch all node titles for wiki-link resolution.
    let titles = LocalResource::new(|| async move { crate::api::fetch_node_titles().await });

    let auth_state = use_auth_state();
    let deleting = RwSignal::new(false);
    let section_jump = RwSignal::new(None::<NodeSection>);
    provide_context(SectionJump(section_jump));
    let show_delete_confirm = RwSignal::new(false);

    // v2.6.2: when navigation arrived via the Kanban row click
    // (`/nodes/{id}?task=<task_id>`), find the task in the TaskPanel and
    // briefly highlight it.  See `crate::focus_task` — handles the
    // resource-load race by retrying a few times.
    schedule_focus_task();
    // Same treatment for `/nodes/{id}?note=<note_id>` — clicking a node-attached
    // note in the central Notes feed lands on it inside the NotePanel.
    crate::focus_note::schedule_focus_note();

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
        // Transition, not Suspense (#123): a Suspense swaps back to the
        // skeleton whenever ANY resource read inside it reloads — e.g. the
        // task list after adding a task — which unmounted the whole page for
        // a frame and reset the scroll position to the top.
        <Transition fallback=move || view! {
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
                            let node_type = n.node_type.as_str().to_string();
                            let status = n.status.as_str().to_string();
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
                                                        // Node-scoped jumps (#123). The sidebar's Tasks / Notes
                                                        // are the global views; these reveal this node's own.
                                                        <button
                                                            class="p-1.5 rounded-lg text-stone-400 hover:text-stone-600
                                                                dark:hover:text-stone-300 hover:bg-stone-100
                                                                dark:hover:bg-stone-800 transition-colors"
                                                            on:click=move |_| section_jump.set(Some(NodeSection::Tasks))
                                                            title="Jump to tasks"
                                                            aria-label="Jump to tasks"
                                                        >
                                                            <span class="material-symbols-outlined" aria-hidden="true">"check_box"</span>
                                                        </button>
                                                        <button
                                                            class="p-1.5 rounded-lg text-stone-400 hover:text-stone-600
                                                                dark:hover:text-stone-300 hover:bg-stone-100
                                                                dark:hover:bg-stone-800 transition-colors"
                                                            on:click=move |_| section_jump.set(Some(NodeSection::Notes))
                                                            title="Jump to notes"
                                                            aria-label="Jump to notes"
                                                        >
                                                            <span class="material-symbols-outlined" aria-hidden="true">"sticky_note_2"</span>
                                                        </button>
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
                                                                <button
                                                                    type="button"
                                                                    class="flex items-center gap-2 w-full text-left px-3 py-2 text-sm
                                                                        text-stone-700 dark:text-stone-200
                                                                        hover:bg-stone-100 dark:hover:bg-stone-800"
                                                                    role="menuitem"
                                                                    on:click=move |_| {
                                                                        menu_open.set(false);
                                                                        section_jump.set(Some(NodeSection::Tasks));
                                                                    }
                                                                >
                                                                    <span class="material-symbols-outlined" style="font-size:18px;">"check_box"</span>
                                                                    "Jump to tasks"
                                                                </button>
                                                                <button
                                                                    type="button"
                                                                    class="flex items-center gap-2 w-full text-left px-3 py-2 text-sm
                                                                        text-stone-700 dark:text-stone-200
                                                                        hover:bg-stone-100 dark:hover:bg-stone-800"
                                                                    role="menuitem"
                                                                    on:click=move |_| {
                                                                        menu_open.set(false);
                                                                        section_jump.set(Some(NodeSection::Notes));
                                                                    }
                                                                >
                                                                    <span class="material-symbols-outlined" style="font-size:18px;">"sticky_note_2"</span>
                                                                    "Jump to notes"
                                                                </button>
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
                                        <crate::components::local_graph::LocalGraphPanel node_id=id title=n.title.clone() />
                                        <BacklinksPanel node_id=id title=n.title.clone() />
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
        </Transition>

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
    // The edge target is a committed node, never typed text (#126): the
    // old text field doubled as the UUID slot, so typing a title and
    // pressing ✓ failed with "Invalid target node UUID".
    let edge_target = RwSignal::new(None::<(NodeId, String)>);
    let confirm_ref = NodeRef::<leptos::html::Button>::new();
    // After a commit, focus ✓ so Enter creates the edge. Deferred: when the
    // picker commits, ✓ is still disabled until this tick renders, and a
    // disabled button refuses focus.
    let focus_confirm = RwSignal::new(false);
    Effect::new(move |_| {
        if focus_confirm.get() && edge_target.with(|t| t.is_some()) {
            focus_confirm.set(false);
            if let Some(el) = confirm_ref.get_untracked() {
                set_timeout(
                    move || {
                        let _ = el.focus();
                    },
                    std::time::Duration::ZERO,
                );
            }
        }
    });
    let edge_type_input = RwSignal::new("references".to_string());
    let label_input = RwSignal::new(String::new());
    let error_msg = RwSignal::new(Option::<String>::None);

    let edges: LocalResource<Result<Vec<EdgeWithTitles>, crate::error::UiError>> =
        LocalResource::new(move || {
            let _ = refresh_edges.get();
            let is_open = open.get();
            let node_id = node_id;
            async move {
                if !is_open {
                    return Ok(vec![]);
                }
                crate::api::fetch_edges_for_node(node_id).await
            }
        });

    let on_add_edge = move |_| {
        // ✓ is disabled until a node is committed; this is the backstop.
        let Some((target_id, _)) = edge_target.get_untracked() else {
            return;
        };
        let et = edge_type_input.get_untracked();
        let label = label_input.get_untracked().trim().to_string();
        error_msg.set(None);

        let edge_type = EdgeType::parse(&et).unwrap_or(EdgeType::References);

        let req = CreateEdgeRequest {
            source_id: node_id,
            target_id,
            edge_type,
            label: if label.is_empty() { None } else { Some(label) },
        };

        wasm_bindgen_futures::spawn_local(async move {
            match crate::api::create_edge(&req).await {
                Ok(_) => {
                    show_add.set(false);
                    edge_target.set(None);
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
                        <div class="flex-1 min-w-0">
                            <crate::components::node_picker::NodePicker
                                selected=edge_target
                                placeholder="Search for a node…"
                                exclude=node_id
                                // Committed: Enter again (or a click) on ✓
                                // creates the edge; type and label can
                                // still be set first.
                                on_commit=Callback::new(move |()| focus_confirm.set(true))
                            />
                        </div>
                        <select
                            class="px-2 py-1 text-xs rounded-lg border border-stone-300 dark:border-stone-600
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
                            class="flex-1 px-2 py-1 text-xs rounded-lg border border-stone-300 dark:border-stone-600
                                bg-transparent text-stone-900 dark:text-stone-100 focus:outline-none
                                focus:ring-1 focus:ring-amber-500"
                            placeholder="Label (optional)..."
                            prop:value=move || label_input.get()
                            on:input=move |ev| label_input.set(event_target_value(&ev))
                        />
                        <button
                            node_ref=confirm_ref
                            class="p-1.5 rounded-lg text-stone-400 hover:text-stone-600
                                dark:hover:text-stone-300 hover:bg-stone-100
                                dark:hover:bg-stone-800 transition-colors
                                disabled:opacity-40 disabled:cursor-not-allowed"
                            on:click=on_add_edge
                            disabled=move || edge_target.with(|t| t.is_none())
                            title="Add edge"
                            aria-label="Add edge"
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
                                            let edge_type_label = edge.edge_type.label();
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
                                                                match crate::api::delete_edge(edge_id).await {
                                                                    Ok(_) => refresh_edges.update(|n| *n += 1),
                                                                    Err(e) => crate::components::toast::push_toast(
                                                                        crate::components::toast::ToastLevel::Error,
                                                                        format!("Couldn't remove edge: {e}"),
                                                                    ),
                                                                }
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

/// Shows nodes that link to this node (incoming edges from other nodes),
/// plus "Mentions": nodes whose text matches this node's title without
/// linking to it, each with a one-click convert-to-wikilink action.
#[component]
fn BacklinksPanel(node_id: NodeId, title: String) -> impl IntoView {
    let navigate = StoredValue::new(use_navigate());
    let open = RwSignal::new(false);
    // Bumped after a successful "link it" so both lists refetch (the rewritten
    // node becomes a real backlink via the server's wikilink sync).
    let refresh = RwSignal::new(0u32);
    let title_sv = StoredValue::new(title);

    let backlinks = LocalResource::new(move || {
        let is_open = open.get();
        let _ = refresh.get();
        let node_id = node_id;
        async move {
            if !is_open {
                return Ok(vec![]);
            }
            crate::api::fetch_backlinks(node_id).await
        }
    });

    // Unlinked mentions: full-text hits on this node's title, minus the node
    // itself and anything that already links here. Only node-body matches —
    // note/task hits can't be rewritten into wikilinks.
    let mentions = LocalResource::new(move || {
        let is_open = open.get();
        let _ = refresh.get();
        let title = title_sv.get_value();
        async move {
            if !is_open || title.trim().is_empty() {
                return Ok::<_, crate::error::UiError>(vec![]);
            }
            let linked: std::collections::HashSet<NodeId> = crate::api::fetch_backlinks(node_id)
                .await
                .unwrap_or_default()
                .iter()
                .map(|n| n.id)
                .collect();
            let resp = crate::api::search_nodes(
                &title,
                false,
                None,
                None,
                &[],
                "or",
                Some("relevance"),
                None,
                None,
                1,
                25,
            )
            .await?;
            Ok(resp
                .results
                .into_iter()
                .filter(|r| {
                    r.node_id != node_id
                        && !linked.contains(&r.node_id)
                        && r.match_source.as_deref() == Some("node")
                })
                .take(10)
                .collect::<Vec<_>>())
        }
    });

    // Convert the first plain-text mention in `mention_id`'s body into a
    // wikilink to this node. The PATCH triggers the server's wikilink sync,
    // so on refresh the node moves from Mentions up to Linked Here.
    let linking = RwSignal::new(false);
    let do_link = move |mention_id: NodeId| {
        if linking.get_untracked() {
            return;
        }
        linking.set(true);
        let title = title_sv.get_value();
        wasm_bindgen_futures::spawn_local(async move {
            let result = async {
                let n = crate::api::fetch_node(mention_id).await?;
                let body = n.body.unwrap_or_default();
                match common::markdown::link_first_mention(&body, &title) {
                    Some(new_body) => {
                        let req = common::node::UpdateNodeRequest {
                            title: None,
                            body: Some(new_body),
                            metadata: None,
                            status: None,
                        };
                        crate::api::update_node(mention_id, &req).await.map(Some)
                    }
                    None => Ok(None),
                }
            }
            .await;
            linking.set(false);
            use crate::components::toast::{ToastLevel, push_toast};
            match result {
                Ok(Some(_)) => {
                    push_toast(ToastLevel::Success, "Mention linked.");
                    refresh.update(|n| *n += 1);
                }
                Ok(None) => push_toast(
                    ToastLevel::Info,
                    "No literal mention found — the match may be a different word form.",
                ),
                Err(e) => push_toast(ToastLevel::Error, format!("Couldn't link: {e}")),
            }
        });
    };

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
                                            let node_type = node.node_type.as_str().to_string();
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

                // ── Unlinked mentions ─────────────────────────────────────
                <Suspense fallback=|| ()>
                    {move || {
                        mentions.get().and_then(|result| {
                            let hits = result.ok().filter(|v| !v.is_empty())?;
                            Some(view! {
                                <div class="mt-4">
                                    <p class="text-xs font-semibold uppercase tracking-wide
                                              text-stone-400 dark:text-stone-500 mb-1">
                                        "Mentions"
                                    </p>
                                    <div class="space-y-1">
                                        {hits.into_iter().map(|hit| {
                                            let nav = navigate.get_value();
                                            let hit_id = hit.node_id;
                                            let hit_title = hit.title.clone();
                                            view! {
                                                <div class="flex items-center gap-1 group">
                                                    <button
                                                        class="flex items-center gap-2 flex-1 min-w-0 text-left py-1.5 px-2 rounded
                                                            text-xs hover:bg-stone-50 dark:hover:bg-stone-800/50
                                                            text-stone-600 dark:text-stone-400
                                                            hover:text-amber-600 dark:hover:text-amber-400"
                                                        title="Open the mentioning node"
                                                        on:click=move |_| nav(&format!("/nodes/{hit_id}"), Default::default())
                                                    >
                                                        <span class="material-symbols-outlined text-stone-400 dark:text-stone-600"
                                                            style="font-size: 14px;">"alternate_email"</span>
                                                        <span class="font-medium truncate">{hit_title}</span>
                                                    </button>
                                                    <button
                                                        class="text-xs px-2 py-0.5 rounded border border-stone-200 dark:border-stone-700
                                                            text-stone-500 dark:text-stone-400
                                                            hover:border-amber-400 hover:text-amber-600 dark:hover:text-amber-400
                                                            disabled:opacity-50 transition-colors shrink-0"
                                                        title="Turn the first plain-text mention into a wikilink"
                                                        disabled=move || linking.get()
                                                        on:click=move |_| do_link(hit_id)
                                                    >
                                                        "Link"
                                                    </button>
                                                </div>
                                            }
                                        }).collect::<Vec<_>>()}
                                    </div>
                                </div>
                            })
                        })
                    }}
                </Suspense>
                </div>
            })}
        </div>
    }
}
