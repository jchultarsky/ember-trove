use common::id::NodeId;
use common::ontology::{Capability, allows};
use common::project_focus::holds_focus;
use leptos::prelude::*;

use crate::auth::{AuthStatus, use_auth_state};
use crate::components::activity_panel::ActivityPanel;
use crate::components::attachment_panel::AttachmentPanel;
use crate::components::history_panel::HistoryPanel;
use crate::components::links_panel::LinksPanel;
use crate::components::modals::delete_confirm::{DeleteConfirmModal, node_delete_details};
use crate::components::node_connections::{ConnectionsPanel, NodeLinks, UnlinkedMentionsPanel};
use crate::components::node_meta::{
    status_color, status_icon, status_label, type_icon, type_label,
};
use crate::components::permission_dialog::PermissionPanel;
use crate::components::project_focus::{FocusMenuItem, FocusRetry, FocusToggle};
use crate::components::tag_bar::TagBar;
use crate::components::task_panel::TaskPanel;
use crate::components::toast::{ToastLevel, push_toast};
use crate::components::version_panel::VersionPanel;
use crate::focus_task::schedule_focus_task;
use crate::markdown::render_body;
use leptos_router::hooks::use_navigate;

#[component]
pub fn NodeView(id: NodeId) -> impl IntoView {
    let navigate = StoredValue::new(use_navigate());
    let refresh = expect_context::<RwSignal<u32>>();
    // Bumped when this page changes tasks other than through the task
    // list (a cascade cancels them), and read by the "still open" notice.
    let task_refresh = expect_context::<crate::app::TaskRefresh>().0;

    let node = LocalResource::new(move || {
        let id = id;
        // With the reader's count of unmet dependencies (S7b): the
        // header's "Blocked" chip needs no request of its own.
        async move { crate::api::fetch_viewed_node(id).await }
    });

    // The node's body changed under this page (a version restore, the
    // Status section moving into the Brief): fetch it again. The `refresh`
    // context only reloads lists, so the page used to keep showing the body
    // it had loaded.
    let reload_node = Callback::new(move |()| {
        node.refetch();
        refresh.update(|n| *n += 1);
    });

    // Resolve just the titles this body links (#134) — it used to download
    // every visible node title on each node page view.
    let body = Signal::derive(move || {
        node.get()
            .and_then(|r| r.ok())
            .and_then(|n| n.node.body)
            .unwrap_or_default()
    });
    let title_map = crate::wikilink::use_wikilink_map(body);

    let auth_state = use_auth_state();
    let deleting = RwSignal::new(false);
    // Section jumps and counts for the outline (#131).
    crate::components::node_section::NodeSections::provide();
    let show_delete_confirm = RwSignal::new(false);

    // v2.6.2: when navigation arrived via the Kanban row click
    // (`/nodes/{id}?task=<task_id>`), find the task in the TaskPanel and
    // briefly highlight it.  See `crate::focus_task` — handles the
    // resource-load race by retrying a few times.
    schedule_focus_task();
    // Same treatment for `/nodes/{id}?note=<note_id>` — clicking a node-attached
    // note in the central Notes feed lands on it inside the History section.
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
                // The page may be gone by the time this returns (the user
                // went elsewhere): `try_` access, never a panic (#281).
                Ok(_) => {
                    push_toast(ToastLevel::Success, "Node deleted.");
                    let _ = refresh.try_update(|n| *n += 1);
                    if let Some(go) = navigate.try_get_value() {
                        go("/nodes", Default::default());
                    }
                }
                Err(e) => {
                    push_toast(ToastLevel::Error, format!("Delete failed: {e}"));
                }
            }
            let _ = deleting.try_set(false);
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
                            // Links render unresolved for a moment, then resolve. Only
                            // the body re-renders when they do: reading the map here, in
                            // the page closure, would rebuild every section (#134).
                            let body_src = n.body.clone().unwrap_or_default();
                            // Body HTML and its heading outline come from one parse
                            // (#185), so the outline can't point at a missing heading.
                            let rendered =
                                Memo::new(move |_| render_body(&body_src, &title_map.get()));
                            let body_html = move || rendered.with(|r| r.html.clone());
                            let outline = Signal::derive(move || {
                                rendered.with(|r| r.visible_outline().map(<[_]>::to_vec))
                                    .unwrap_or_default()
                            });
                            let node_type = n.node_type.as_str().to_string();
                            let status = n.status.as_str().to_string();
                            let edit_id = n.id;
                            // Ownership check: node owner OR any admin user.
                            let is_owner = if let AuthStatus::Authenticated(ref user) =
                                auth_state.get_untracked()
                            {
                                user.sub == n.owner_id
                                    || user.is_admin()
                            } else {
                                false
                            };
                            // The node's own owner, without the admin bypass: people
                            // are owner-only (#147), so the admin can't change them.
                            let is_canonical_owner = matches!(
                                auth_state.get_untracked(),
                                AuthStatus::Authenticated(ref user) if user.sub == n.owner_id
                            );

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
                            // Computed here: the panels' wrapper takes its
                            // children by value.
                            let tasks_allowed = allows(&n.node_type, Capability::Tasks);
                            // One counter for the node's home and links: the
                            // header's home chip and Connections both read it.
                            let home_refresh = RwSignal::new(0u32);
                            let links = NodeLinks::new(id, home_refresh);
                            let served_blocked_by = n.blocked_by;
                            // Links this reader isn't shown (#349): the
                            // header's marker and Connections share it.
                            let links_not_shown = RwSignal::new(n.links_not_shown);
                            let lifecycle_status = n.status.clone();
                            let has_brief = allows(&n.node_type, Capability::Brief);
                            let body_class = if has_brief && n.body.as_deref().is_some_and(|b| !b.trim().is_empty()) {
                                "border-t border-stone-200 dark:border-stone-700 pt-6"
                            } else {
                                ""
                            };

                            // Focus (#282): whether this project is in the
                            // viewer's own Focus. It is private to them and
                            // not part of the node, so it loads apart; the
                            // control shows once the answer is known. An
                            // archived project never shows in Focus, nor
                            // one that is parked or closed (S6a), so it
                            // has no control.
                            //
                            // The declared state (S6a) belongs to the page:
                            // the state chip writes it, and the Focus
                            // control and the task list follow it at once.
                            let project_state = RwSignal::new(n.project_state);
                            let project_state_at = RwSignal::new(n.project_state_at);
                            let (kind, lifecycle) = (n.node_type.clone(), n.status.clone());
                            // Memos: the chip sets the state when it is chosen
                            // and again when the save answers, usually to the
                            // same value. Only a real change may reload Focus
                            // or rebuild the task rows.
                            let state_now = Memo::new(move |_| project_state.get());
                            let focusable = Memo::new(move |_| {
                                holds_focus(&kind, &lifecycle, state_now.get())
                            });
                            let focus_state = RwSignal::new(None::<bool>);
                            // A failed read is said: the control offers to
                            // load again. It used to leave no control.
                            let focus_failed = RwSignal::new(false);
                            // Each read is numbered and only the latest
                            // answer is kept: one sent before a change of
                            // state must not land over the one sent after.
                            let focus_read = StoredValue::new(0u32);
                            let load_focus = Callback::new(move |()| {
                                focus_failed.set(false);
                                focus_read.update_value(|n| *n += 1);
                                let this = focus_read.get_value();
                                wasm_bindgen_futures::spawn_local(async move {
                                    let result = crate::api::fetch_focus_item(edit_id).await;
                                    if focus_read.try_get_value() != Some(this) {
                                        return;
                                    }
                                    match result {
                                        Ok(item) => {
                                            let _ = focus_state.try_set(Some(item.is_some()));
                                        }
                                        Err(_) => {
                                            let _ = focus_failed.try_set(true);
                                        }
                                    }
                                });
                            });
                            let focus_retry = FocusRetry {
                                failed: focus_failed,
                                run: load_focus,
                            };
                            if focusable.get_untracked() {
                                load_focus.run(());
                            }
                            // A change of state makes what was known about
                            // Focus stale (closing removed the entry on the
                            // server), so the control shows nothing until
                            // it is read again. Not here, though: the chip
                            // changes the state when it is chosen, before
                            // the server has it, and a read sent now would
                            // answer for the old state. The chip says when
                            // the choice has settled.
                            Effect::new(move |seen: Option<()>| {
                                focusable.track();
                                if seen.is_some() {
                                    focus_state.set(None);
                                }
                            });
                            let reload_focus = Callback::new(move |()| {
                                if focusable.get_untracked() {
                                    load_focus.run(());
                                }
                            });
                            // A settled change of state may have changed
                            // more than the state: closing cancels tasks
                            // and writes History (S6b). The task list, the
                            // History section and the "still open" notice
                            // read again.
                            let state_writes = RwSignal::new(0u32);
                            // Anything on the page that writes the node's
                            // History says so here, and the section reads
                            // its changes again: a change of state, of the
                            // status label, of the Brief (S6d).
                            let history_writes = RwSignal::new(0u32);
                            let history_changed = Callback::new(move |()| {
                                let _ = history_writes.try_update(|n| *n += 1);
                            });
                            let state_settled = Callback::new(move |()| {
                                reload_focus.run(());
                                let _ = state_writes.try_update(|n| *n += 1);
                                let _ = task_refresh.try_update(|n| *n += 1);
                                history_changed.run(());
                            });
                            let still_open_refresh = Signal::derive(move || {
                                state_writes.get().wrapping_add(task_refresh.get())
                            });
                            let caught_up = Callback::new(move |saved: common::node::Node| {
                                let _ = project_state.try_set(saved.project_state);
                                let _ = project_state_at.try_set(saved.project_state_at);
                                state_settled.run(());
                            });

                            view! {
                                <div class="flex flex-col h-full">
                                    // At phone width the title takes the row and the
                                    // icons and status chip wrap under it (#262): in one
                                    // unwrapping row the title was squeezed to a few
                                    // characters a line and the chip ran off the screen.
                                    <div class="flex items-start sm:items-center justify-between gap-2 px-4 md:px-6 py-4 border-b border-stone-200 dark:border-stone-800">
                                        <div class="flex flex-wrap items-center gap-x-3 gap-y-1 min-w-0 flex-1">
                                            <button
                                                title="Back to nodes"
                                                aria-label="Back to nodes"
                                                class="text-stone-400 hover:text-stone-600 dark:hover:text-stone-300"
                                                on:click=move |_| nav_back("/nodes", Default::default())
                                            >
                                                <span aria-hidden="true" class="material-symbols-outlined">"arrow_back"</span>
                                            </button>
                                            <h1 class="text-lg font-semibold text-stone-900 dark:text-stone-100 min-w-0 break-words max-w-full">
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
                                            // Project status (#143): the chip, editable by editors.
                                            {allows(&n.node_type, Capability::ProjectStatus).then(|| view! {
                                                <crate::components::project_status::ProjectStatusChip
                                                    node_id=id
                                                    initial=n.project_status.clone()
                                                    editable=is_owner
                                                    on_saved=history_changed
                                                />
                                            })}
                                            // The declared state (S6a), beside the label.
                                            {allows(&n.node_type, Capability::ProjectStatus).then(|| view! {
                                                <crate::components::project_state::ProjectStateChip
                                                    node_id=id
                                                    state=project_state
                                                    at=project_state_at
                                                    editable=is_owner
                                                    on_settled=state_settled
                                                />
                                            })}
                                            // The home (owner request, 2026-11-12): the
                                            // dashboard groups projects by Area, and the
                                            // only way to set one was in Connections.
                                            <crate::components::node_home::HomeChip
                                                node_id=id
                                                node_type=n.node_type.clone()
                                                can_edit=is_owner
                                                refresh=home_refresh
                                            />
                                            // "Blocked" (S7b): it waits for something
                                            // that is not finished. Said, never enforced.
                                            <crate::components::node_connections::NodeBlockedChip
                                                node_id=id
                                                node_type=n.node_type.clone()
                                                status=lifecycle_status.clone()
                                                project_state=project_state
                                                served=served_blocked_by
                                                links=links
                                            />
                                            <crate::components::node_connections::LinksNotShownChip
                                                not_shown=links_not_shown
                                            />
                                        </div>
                                        {
                                            // Overflow dropdown state — scoped to this rendered
                                            // node so switching nodes implicitly closes the menu.
                                            let menu_open = RwSignal::new(false);
                                            view! {
                                                <div class="flex items-center gap-1 shrink-0">
                                                    // Section outline — every size (#131; it replaces
                                                    // #123's Jump to tasks / notes buttons).
                                                    <crate::components::node_section::SectionOutline />
                                                    // Desktop cluster — all four actions visible
                                                    <div class="hidden md:flex items-center gap-1">
                                                        <a
                                                            href=format!("/api/nodes/{id}/export")
                                                            download=true
                                                            // Renew the session first (#190).
                                                            on:click=move |ev| {
                                                                ev.prevent_default();
                                                                crate::api::start_download(format!("/api/nodes/{id}/export"));
                                                            }
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
                                                            <span aria-hidden="true" class="material-symbols-outlined">"edit"</span>
                                                        </button>
                                                        {move || focusable.get().then(|| view! {
                                                            <FocusToggle node_id=edit_id state=focus_state retry=focus_retry />
                                                        })}
                                                        <button
                                                            class="p-1.5 rounded-lg text-stone-400 hover:text-amber-600
                                                                dark:hover:text-amber-400 hover:bg-amber-50
                                                                dark:hover:bg-amber-900/30 transition-colors
                                                                disabled:opacity-30"
                                                            on:click=move |_| do_duplicate()
                                                            disabled=move || duplicating.get()
                                                            title=move || if duplicating.get() { "Duplicating…" } else { "Duplicate" }
                                                        >
                                                            <span aria-hidden="true" class="material-symbols-outlined">
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
                                                            <span aria-hidden="true" class="material-symbols-outlined">
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
                                                            <span aria-hidden="true" class="material-symbols-outlined">"more_vert"</span>
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
                                                                    on:click=move |ev| {
                                                                        ev.prevent_default();
                                                                        menu_open.set(false);
                                                                        crate::api::start_download(format!("/api/nodes/{id}/export"));
                                                                    }
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
                                                                {move || focusable.get().then(|| view! {
                                                                    <FocusMenuItem
                                                                        node_id=edit_id
                                                                        state=focus_state
                                                                        retry=focus_retry
                                                                        on_click=Callback::new(move |()| menu_open.set(false))
                                                                    />
                                                                })}
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
                                        // Something still open under a closed project
                                        // (S6b): said to those who have the state chip.
                                        {(is_owner && allows(&n.node_type, Capability::ProjectStatus)).then(|| view! {
                                            <crate::components::project_state::StillOpenNotice
                                                node_id=id
                                                state=state_now
                                                refresh=still_open_refresh
                                                on_closed=caught_up
                                            />
                                        })}
                                        // The Brief (#282): where a Project stands now. It
                                        // leads the page, above the body (owner decision,
                                        // 2026-11-11). Projects only (§3.2); kinds never
                                        // change, so no other node has one to show.
                                        {has_brief.then(|| view! {
                                            <crate::components::node_brief::BriefPanel
                                                node_id=id
                                                can_edit=is_owner
                                                body=n.body.clone()
                                                body_version=n.updated_at
                                                on_body_changed=reload_node
                                                on_changed=history_changed
                                            />
                                        })}
                                        // Under a Brief, a body that has text gets the
                                        // rule a section would draw above itself.
                                        <div class=body_class data-testid="node-body">
                                            <crate::components::body_outline::BodyOutline entries=outline />
                                            <div
                                                class="prose max-w-none dark:prose-invert"
                                                inner_html=body_html
                                                on:click=handle_wikilink_click
                                            />
                                        </div>
                                        // Tasks and People belong to Projects (§3.2, #216);
                                        // elsewhere they show only while older items remain.
                                        // One editor-prefs fetch per node for both panels (P1 #2).
                                        <crate::components::task_row::EditorPrefsScope>
                                        <TaskPanel node_id=id allowed=tasks_allowed
                                            can_fix=is_canonical_owner
                                            project_state=state_now />
                                        // History (S6d): the node's notes and the changes to it, in one
                                        // chronology. The owner adds notes; everyone who can read
                                        // the node reads them.
                                        <HistoryPanel node_id=id is_owner=is_owner
                                            changes_refresh=history_writes />
                                        </crate::components::task_row::EditorPrefsScope>
                                        // People (#147): only the node's own owner may change them.
                                        <crate::components::node_people::NodePeoplePanel node_id=id can_edit=is_canonical_owner
                                            allowed=allows(&n.node_type, Capability::People) />
                                        // External links panel — grouped with other collapsible sections
                                        <LinksPanel node_id=id is_editor=is_owner />
                                        // Connections and Unlinked Mentions share one read
                                        // of the node's links (S7a).
                                        <ConnectionsPanel node_id=id node_type=n.node_type.clone() can_edit=is_owner
                                            status=lifecycle_status.clone() project_state=project_state
                                            links=links not_shown=links_not_shown />
                                        <crate::components::local_graph::LocalGraphPanel node_id=id title=n.title.clone() />
                                        <UnlinkedMentionsPanel node_id=id title=n.title.clone() links=links />
                                        <AttachmentPanel node_id=id />
                                        <PermissionPanel node_id=id is_owner=is_owner />
                                        <VersionPanel
                                            node_id=id
                                            is_editor=is_owner
                                            on_restore=reload_node
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
            details=node_delete_details(
                show_delete_confirm.into(),
                Signal::derive(move || vec![id]),
            )
        />
    }
}
