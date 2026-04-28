use leptos::{ev, prelude::*};
use leptos_router::components::{Redirect, Route, Routes};
use leptos_router::hooks::{use_location, use_navigate, use_params_map};
use leptos_router::path;
use wasm_bindgen::JsCast;
use wasm_bindgen::closure::Closure;

use crate::{
    app::{AppVersion, ShowCapture},
    auth::{AuthState, AuthStatus},
    components::{
        admin_view::AdminView,
        backup_view::BackupView,
        bulk_permissions_view::BulkPermissionsView,
        dark_mode_toggle::DarkModeToggle,
        graph_view::GraphView,
        modals::{
            command_palette::CommandPalette,
            create_node::CreateNodeModal,
            fast_capture::FastCaptureModal,
            help::HelpModal,
        },
        node_editor::NodeEditor,
        node_list::NodeList,
        node_view::NodeView,
        notes_view::NotesView,
        project_dashboard::ProjectDashboard,
        search_view::SearchView,
        sidebar::Sidebar,
        tag_manager::TagManager,
        tasks_view::{TasksTab, TasksView},
        templates_view::TemplatesView,
        toast::ToastOverlay,
    },
};
use common::id::NodeId;

/// Newtype around the `show_shortcuts` signal so the sidebar header
/// (and any future surface) can open the help modal via context, not
/// prop threading.  Keeps the legacy "shortcuts" name for the signal
/// itself since the keyboard reference is still tab #1.
#[derive(Clone, Copy)]
pub struct ShowHelp(pub RwSignal<bool>);

// ── Route param wrappers ─────────────────────────────────────────────────────

#[component]
fn NodeViewRoute() -> impl IntoView {
    let params = use_params_map();
    move || {
        let id = params.with(|p| p.get("id").and_then(|s| s.parse::<NodeId>().ok()));
        match id {
            Some(id) => view! { <NodeView id=id /> }.into_any(),
            None => view! { <p class="p-6 text-red-500">"Invalid node ID"</p> }.into_any(),
        }
    }
}

#[component]
fn NodeEditRoute() -> impl IntoView {
    let params = use_params_map();
    move || {
        let id = params.with(|p| p.get("id").and_then(|s| s.parse::<NodeId>().ok()));
        match id {
            Some(id) => view! { <NodeEditor node=Some(id) /> }.into_any(),
            None => view! { <p class="p-6 text-red-500">"Invalid node ID"</p> }.into_any(),
        }
    }
}

/// Whether the sidebar should render in icon-only mode.
///
/// Read-only `Signal` so children cannot toggle it directly. The Layout owns
/// the underlying `RwSignal<bool>` (the user's desktop preference) and derives
/// this signal so it returns `false` at mobile breakpoints — the slide-in
/// drawer should always be fully expanded since it auto-hides on selection.
pub type SidebarCollapsed = Signal<bool>;

#[component]
pub fn Layout(auth_state: AuthState) -> impl IntoView {
    // User-controlled collapse preference (only mutated by the desktop toggle).
    let collapsed_state: RwSignal<bool> = RwSignal::new(false);
    // Tracks whether the viewport is at the mobile breakpoint (`< md`, 768px).
    let is_mobile: RwSignal<bool> = RwSignal::new(false);

    if let Some(window) = web_sys::window()
        && let Ok(Some(mql)) = window.match_media("(max-width: 767px)")
    {
        is_mobile.set(mql.matches());
        let cb = Closure::wrap(Box::new(move |e: web_sys::MediaQueryListEvent| {
            is_mobile.set(e.matches());
        }) as Box<dyn FnMut(_)>);
        let _ = mql.add_event_listener_with_callback("change", cb.as_ref().unchecked_ref());
        // Leak the closure for the lifetime of the page; Layout lives the full session.
        cb.forget();
    }

    // Effective collapsed state passed to children: forced `false` on mobile.
    let collapsed: SidebarCollapsed =
        Signal::derive(move || !is_mobile.get() && collapsed_state.get());

    let mobile_open: RwSignal<bool> = RwSignal::new(false);
    let show_capture = use_context::<ShowCapture>()
        .expect("ShowCapture context missing")
        .0;

    let refresh = use_context::<RwSignal<u32>>().expect("refresh signal must be provided");

    let close_mobile = move || mobile_open.set(false);

    // ── Global keyboard shortcuts ──────────────────────────────────────────
    // Must live here (inside the Router) so use_navigate() is available.
    let navigate = use_navigate();
    let location = use_location();
    let show_shortcuts = RwSignal::new(false);
    // v2.10.0 — surface the help-modal toggle as a context so the sidebar
    // header (and any other corner of the UI) can open it without
    // threading the signal through props.
    provide_context(ShowHelp(show_shortcuts));

    // v2.8.0 — Cmd-K command palette. Opened via Cmd+K / Ctrl+K (any
    // view) or `/` (repurposed from full-page navigation to /search).
    // The palette's "Create node titled '<query>'" action stamps the
    // pending title so the structured CreateNodeModal can pre-fill it.
    let show_palette: RwSignal<bool> = RwSignal::new(false);
    let palette_create_title: RwSignal<String> = RwSignal::new(String::new());
    let show_structured_from_palette: RwSignal<bool> = RwSignal::new(false);

    // Cmd-K / Ctrl-K — separate listener because the main shortcut
    // handler below short-circuits on any modifier key (which is
    // correct for /, n, g, etc.).  This one only fires on the modifier
    // path and doesn't care about input focus: ⌘K is a system-wide
    // affordance that should work even mid-edit.
    {
        let _palette_handle = window_event_listener(ev::keydown, move |ev: web_sys::KeyboardEvent| {
            if (ev.meta_key() || ev.ctrl_key())
                && !ev.shift_key()
                && !ev.alt_key()
                && ev.key().eq_ignore_ascii_case("k")
            {
                ev.prevent_default();
                show_palette.update(|v| *v = !*v);
            }
        });
    }

    let handle = {
        let navigate = navigate.clone();
        window_event_listener(ev::keydown, move |ev: web_sys::KeyboardEvent| {
            if ev.ctrl_key() || ev.meta_key() || ev.alt_key() {
                return;
            }
            let is_editable = web_sys::window()
                .and_then(|w| w.document())
                .and_then(|d| d.active_element())
                .map(|el| {
                    let tag = el.tag_name().to_uppercase();
                    if matches!(tag.as_str(), "INPUT" | "TEXTAREA" | "SELECT" | "BUTTON") {
                        return true;
                    }
                    el.get_attribute("contenteditable")
                        .map(|v| v != "false")
                        .unwrap_or(false)
                })
                .unwrap_or(false);
            if is_editable {
                return;
            }

            match ev.key().as_str() {
                "?" => show_shortcuts.update(|v| *v = !*v),
                "n" => show_capture.set(true),
                "g" => navigate("/graph", Default::default()),
                "/" => {
                    // v2.8.0: `/` opens the Cmd-K palette over the
                    // current view instead of navigating to the
                    // full-page `/search` (which is still reachable
                    // by typing the URL or via the legacy redirect).
                    ev.prevent_default();
                    show_palette.set(true);
                }
                "d" => {
                    // Duplicate the currently open node (only when on /nodes/<uuid>).
                    let path = location.pathname.get_untracked();
                    let segs: Vec<&str> = path.trim_matches('/').split('/').collect();
                    if segs.len() == 2 && segs[0] == "nodes"
                        && let Ok(node_id) = segs[1].parse::<NodeId>() {
                        let nav = navigate.clone();
                        wasm_bindgen_futures::spawn_local(async move {
                            match crate::api::duplicate_node(node_id).await {
                                Ok(dup) => {
                                    crate::components::toast::push_toast(
                                        crate::components::toast::ToastLevel::Success,
                                        "Node duplicated.",
                                    );
                                    refresh.update(|n| *n += 1);
                                    nav(&format!("/nodes/{}", dup.id), Default::default());
                                }
                                Err(e) => crate::components::toast::push_toast(
                                    crate::components::toast::ToastLevel::Error,
                                    format!("Duplicate failed: {e}"),
                                ),
                            }
                        });
                    }
                }
                "Escape" => {
                    if show_shortcuts.get_untracked() {
                        show_shortcuts.set(false);
                    } else {
                        let path = location.pathname.get_untracked();
                        if path.starts_with("/nodes") {
                            navigate("/nodes", Default::default());
                        }
                    }
                }
                _ => {}
            }
        })
    };
    on_cleanup(move || handle.remove());

    view! {
        <AuthGate auth_state=auth_state>
            <div class="flex flex-col md:flex-row h-screen overflow-hidden bg-stone-50 dark:bg-stone-950">

                // ── Mobile top bar ──────────────────────────────────────────────────────
                <header class="md:hidden flex-shrink-0 flex items-center justify-between
                               px-4 py-3 border-b border-stone-200 dark:border-stone-800
                               bg-stone-50 dark:bg-stone-950 z-10">
                    <div class="flex items-center gap-2">
                        <div class="w-7 h-7 flex-shrink-0">
                            <svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 64 64" class="w-full h-full">
                                <defs>
                                    <linearGradient id="flame-m" x1="0" y1="1" x2="0" y2="0">
                                        <stop offset="0%" stop-color="#f59e0b"/>
                                        <stop offset="50%" stop-color="#ef4444"/>
                                        <stop offset="100%" stop-color="#f97316"/>
                                    </linearGradient>
                                </defs>
                                <path d="M32 4 C24 16, 12 24, 12 38 C12 50, 20 60, 32 60 C44 60, 52 50, 52 38 C52 24, 40 16, 32 4Z"
                                    fill="url(#flame-m)" opacity="0.9"/>
                                <path d="M32 22 C28 30, 22 34, 22 42 C22 50, 26 54, 32 54 C38 54, 42 50, 42 42 C42 34, 36 30, 32 22Z"
                                    fill="#fbbf24" opacity="0.85"/>
                                <circle cx="32" cy="38" r="3" fill="#ffffff" opacity="0.9"/>
                            </svg>
                        </div>
                        <span class="font-semibold text-stone-900 dark:text-stone-100 text-sm">
                            "Ember Trove"
                        </span>
                    </div>
                    <button
                        on:click=move |_| mobile_open.update(|o| *o = !*o)
                        class="p-2 rounded-lg text-stone-500 hover:bg-stone-100
                               dark:hover:bg-stone-800 dark:text-stone-400 cursor-pointer"
                        title="Toggle menu"
                    >
                        <span class="material-symbols-outlined" style="font-size: 22px;">
                            {move || if mobile_open.get() { "close" } else { "menu" }}
                        </span>
                    </button>
                </header>

                // ── Mobile backdrop ────────────────────────────────────────────────────
                {move || mobile_open.get().then(|| view! {
                    <div
                        class="fixed inset-0 z-30 bg-black/40 md:hidden"
                        on:click=move |_| mobile_open.set(false)
                    />
                })}

                // ── Sidebar ────────────────────────────────────────────────────────────
                <aside
                    class=move || {
                        let base = "flex flex-col border-r border-stone-200 dark:border-stone-800 \
                                    bg-stone-50 dark:bg-stone-950 transition-all duration-200";
                        let mobile = if mobile_open.get() {
                            "fixed inset-y-0 left-0 z-40 w-72 translate-x-0"
                        } else {
                            "fixed inset-y-0 left-0 z-40 w-72 -translate-x-full md:translate-x-0"
                        };
                        let desktop = if collapsed_state.get() {
                            "md:relative md:inset-auto md:w-16 md:flex-shrink-0 md:transform-none"
                        } else {
                            "md:relative md:inset-auto md:w-64 md:flex-shrink-0 md:transform-none"
                        };
                        format!("{base} {mobile} {desktop}")
                    }
                >
                    <SidebarHeader collapsed=collapsed />
                    <Sidebar auth_state=auth_state collapsed=collapsed on_nav=Callback::new(move |_| close_mobile()) />

                    <button
                        on:click=move |_| collapsed_state.update(|c| *c = !*c)
                        class="hidden md:flex absolute right-0 top-1/2 -translate-y-1/2 translate-x-1/2 z-20
                            w-5 h-5 rounded-full
                            bg-white dark:bg-stone-900
                            border border-stone-200 dark:border-stone-700
                            shadow-sm items-center justify-center
                            text-stone-400 hover:text-stone-600 dark:hover:text-stone-300
                            hover:border-stone-400 dark:hover:border-stone-500
                            hover:shadow-md transition-all cursor-pointer"
                        title=move || if collapsed_state.get() { "Expand sidebar" } else { "Collapse sidebar" }
                    >
                        <span
                            class="material-symbols-outlined"
                            style="font-size: 14px; line-height: 1;"
                        >
                            {move || if collapsed_state.get() { "chevron_right" } else { "chevron_left" }}
                        </span>
                    </button>
                </aside>

                <main class="flex-1 overflow-auto flex flex-col min-w-0">
                    <Routes fallback=|| view! { <Redirect path="/tasks/my-day" /> }>
                        <Route path=path!("/")                 view=|| view! { <Redirect path="/tasks/my-day" /> } />
                        <Route path=path!("/tasks")            view=|| view! { <Redirect path="/tasks/my-day" /> } />
                        <Route path=path!("/tasks/my-day")     view=|| view! { <TasksView active=TasksTab::MyDay    /> } />
                        <Route path=path!("/tasks/inbox")      view=|| view! { <TasksView active=TasksTab::Inbox    /> } />
                        <Route path=path!("/tasks/calendar")   view=|| view! { <TasksView active=TasksTab::Calendar /> } />
                        // Legacy URL redirects — preserve bookmarks, PWA shortcuts, and any
                        // external links dating back before the Tasks consolidation.
                        <Route path=path!("/my-day")    view=|| view! { <Redirect path="/tasks/my-day"    /> } />
                        <Route path=path!("/inbox")     view=|| view! { <Redirect path="/tasks/inbox"     /> } />
                        <Route path=path!("/calendar")  view=|| view! { <Redirect path="/tasks/calendar"  /> } />
                        <Route path=path!("/dashboard") view=ProjectDashboard />
                        <Route path=path!("/graph")     view=GraphView />
                        <Route path=path!("/search")    view=SearchView />
                        <Route path=path!("/nodes")     view=NodeList />
                        <Route path=path!("/nodes/new") view=|| view! { <NodeEditor node=None /> } />
                        <Route path=path!("/nodes/:id")      view=NodeViewRoute />
                        <Route path=path!("/nodes/:id/edit") view=NodeEditRoute />
                        <Route path=path!("/tags")           view=TagManager />
                        <Route path=path!("/notes")          view=NotesView />
                        <Route path=path!("/templates")      view=TemplatesView />
                        <Route path=path!("/admin/users")       view=AdminView />
                        <Route path=path!("/admin/permissions") view=BulkPermissionsView />
                        <Route path=path!("/admin/backup")      view=BackupView />
                    </Routes>
                </main>
            </div>

            // Fast-capture modal — primary `n`-shortcut surface.  One
            // textarea, lands a Task in the Inbox.  "More fields…" hands off
            // to the structured CreateNodeModal below, pre-filled with the
            // user's draft so nothing is lost.
            {
                let show_structured = RwSignal::new(false);
                let structured_prefill = RwSignal::new(String::new());
                view! {
                    <FastCaptureModal
                        show=show_capture.read_only()
                        on_close=Callback::new(move |_| show_capture.set(false))
                        on_more_fields=Callback::new(move |draft: String| {
                            structured_prefill.set(draft);
                            show_capture.set(false);
                            show_structured.set(true);
                        })
                    />
                    <CreateNodeModal
                        show=show_structured.read_only()
                        on_close=Callback::new(move |_| show_structured.set(false))
                        initial_body=structured_prefill.read_only()
                    />
                }
            }

            // v2.10.0 — Help modal: Shortcuts / Concepts / Workflow tabs.
            // Triggered by `?` (kept the legacy `show_shortcuts` signal name
            // since it still gates the same modal — just a richer one now)
            // or by the (?) icon in SidebarHeader.
            <HelpModal
                show=show_shortcuts.read_only()
                on_close=Callback::new(move |_| show_shortcuts.set(false))
            />

            // v2.8.0 — Cmd-K command palette.  Repurposes `/` and adds
            // ⌘K / Ctrl-K.  "Create node" picks open the structured
            // modal pre-filled with the typed title.
            <CommandPalette
                show=show_palette.read_only()
                on_close=Callback::new(move |_| show_palette.set(false))
                on_create=Callback::new(move |title: String| {
                    palette_create_title.set(title);
                    show_structured_from_palette.set(true);
                })
            />
            <CreateNodeModal
                show=show_structured_from_palette.read_only()
                on_close=Callback::new(move |_| show_structured_from_palette.set(false))
                initial_title=palette_create_title.read_only()
            />

            // Toast notification overlay
            <ToastOverlay />
        </AuthGate>
    }
}

/// Sidebar header: banner icon + title + (?) help + dark-mode toggle.
#[component]
fn SidebarHeader(collapsed: SidebarCollapsed) -> impl IntoView {
    let app_version = use_context::<AppVersion>().expect("AppVersion must be provided");
    // v2.10.0 — discoverability for the `?` shortcut.  Optional via
    // use_context: any layout that doesn't provide ShowHelp just
    // doesn't render the button (no crash).
    let help_toggle: Option<ShowHelp> = use_context();
    view! {
        <div class="flex items-center border-b border-stone-200 dark:border-stone-800 px-3 py-4 gap-2">
            <div class="flex-shrink-0 w-8 h-8">
                <svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 64 64" class="w-full h-full">
                    <defs>
                        <linearGradient id="flame" x1="0" y1="1" x2="0" y2="0">
                            <stop offset="0%" stop-color="#f59e0b"/>
                            <stop offset="50%" stop-color="#ef4444"/>
                            <stop offset="100%" stop-color="#f97316"/>
                        </linearGradient>
                    </defs>
                    <path d="M32 4 C24 16, 12 24, 12 38 C12 50, 20 60, 32 60 C44 60, 52 50, 52 38 C52 24, 40 16, 32 4Z"
                        fill="url(#flame)" opacity="0.9"/>
                    <path d="M32 22 C28 30, 22 34, 22 42 C22 50, 26 54, 32 54 C38 54, 42 50, 42 42 C42 34, 36 30, 32 22Z"
                        fill="#fbbf24" opacity="0.85"/>
                    <circle cx="32" cy="38" r="3" fill="#ffffff" opacity="0.9"/>
                    <circle cx="26" cy="45" r="2" fill="#ffffff" opacity="0.7"/>
                    <circle cx="38" cy="45" r="2" fill="#ffffff" opacity="0.7"/>
                    <line x1="32" y1="38" x2="26" y2="45" stroke="#ffffff" stroke-width="0.8" opacity="0.5"/>
                    <line x1="32" y1="38" x2="38" y2="45" stroke="#ffffff" stroke-width="0.8" opacity="0.5"/>
                    <line x1="26" y1="45" x2="38" y2="45" stroke="#ffffff" stroke-width="0.8" opacity="0.5"/>
                </svg>
            </div>
            <div
                class="flex-1 flex items-center justify-between min-w-0 overflow-hidden"
                class:hidden=move || collapsed.get()
            >
                // Title and version stacked vertically — keeps the title on
                // its own line so it never gets truncated by the (?) +
                // dark-mode icons on its right.  Version lives under the
                // title as a small footnote.
                <div class="flex flex-col min-w-0 leading-tight">
                    <span class="font-semibold text-stone-900 dark:text-stone-100 truncate">
                        "Ember Trove"
                    </span>
                    {move || {
                        let v = app_version.0.get();
                        (!v.is_empty()).then(|| view! {
                            <span class="text-[10px] font-mono text-stone-400 dark:text-stone-500 select-none">
                                {format!("v{v}")}
                            </span>
                        })
                    }}
                </div>
                <div class="flex items-center gap-1">
                    {help_toggle.map(|h| view! {
                        <button
                            type="button"
                            class="flex-shrink-0 p-1 rounded text-stone-400 \
                                   hover:text-amber-600 dark:hover:text-amber-500 \
                                   hover:bg-stone-100 dark:hover:bg-stone-800 \
                                   transition-colors cursor-pointer"
                            title="Help (?)"
                            on:click=move |_| h.0.update(|v| *v = !*v)
                        >
                            <span class="material-symbols-outlined" style="font-size:18px;">
                                "help_outline"
                            </span>
                        </button>
                    })}
                    <DarkModeToggle />
                </div>
            </div>
        </div>
    }
}

/// Auth gate: spinner → login redirect → render app.
#[component]
fn AuthGate(auth_state: AuthState, children: ChildrenFn) -> impl IntoView {
    move || match auth_state.get() {
        AuthStatus::Loading => view! {
            <div class="flex items-center justify-center h-screen bg-stone-50 dark:bg-stone-950">
                <div class="text-stone-400 dark:text-stone-500 text-sm">"Loading..."</div>
            </div>
        }.into_any(),
        AuthStatus::Unauthenticated => {
            wasm_bindgen_futures::spawn_local(async {
                if let Ok(url) = crate::api::fetch_login_url().await
                    && let Some(window) = web_sys::window()
                {
                    let _ = window.location().set_href(&url);
                }
            });
            view! {
                <div class="flex items-center justify-center h-screen bg-stone-50 dark:bg-stone-950">
                    <div class="text-stone-400 dark:text-stone-500 text-sm">"Redirecting to login..."</div>
                </div>
            }.into_any()
        }
        AuthStatus::Authenticated(_) => children().into_any(),
    }
}
