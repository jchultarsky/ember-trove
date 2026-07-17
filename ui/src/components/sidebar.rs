use leptos::prelude::*;
use leptos_router::hooks::{use_location, use_navigate};

use crate::{
    auth::{AuthState, AuthStatus},
    components::{
        change_password_modal::ChangePasswordModal, favorites_section::FavoritesSection,
        layout::SidebarCollapsed, search_bar::SearchBar,
    },
};

#[component]
pub fn Sidebar(
    auth_state: AuthState,
    collapsed: SidebarCollapsed,
    on_nav: Callback<()>,
) -> impl IntoView {
    let navigate = StoredValue::new(use_navigate());
    let show_change_pw = RwSignal::new(false);

    let on_logout = move |_| {
        wasm_bindgen_futures::spawn_local(async move {
            if let Ok(url) = crate::api::fetch_logout_url().await
                && let Some(window) = web_sys::window()
            {
                let _ = window.location().set_href(&url);
            }
        });
    };

    let node_type_filter = expect_context::<RwSignal<Option<String>>>();

    // Macro to reduce cloning boilerplate for each nav closure.
    // Each SidebarLink on_click needs its own clone of navigate + on_nav.
    macro_rules! nav {
        ($path:expr) => {{
            let n = navigate.get_value();
            move || {
                n($path, Default::default());
                on_nav.run(());
            }
        }};
    }

    view! {
        <nav class="flex-1 overflow-y-auto px-2 py-4 space-y-1">
            // Search — top of sidebar
            {move || {
                if collapsed.get() {
                    let n = navigate.get_value();
                    view! {
                        <SidebarLink
                            icon="search" label="Search"
                            on_click=move || n("/search", Default::default())
                            collapsed=collapsed
                        />
                    }.into_any()
                } else {
                    view! {
                        <div class="px-1 mb-1">
                            <SearchBar />
                        </div>
                    }.into_any()
                }
            }}
            <div class="border-t border-stone-200 dark:border-stone-700 my-3" />
            // ── Section 1: Daily workflow ──────────────────────────────────────
            // Tasks (My Day / Inbox / Calendar) share one sidebar entry; the
            // view itself exposes the three via a tab bar for clearer IA.
            // (The morning-ritual /plan entry was removed in v2.6.1 — the
            // Kanban surface made it redundant.)
            <SidebarLink icon="task_alt"       label="Tasks"     on_click=nav!("/tasks/my-day") collapsed=collapsed />
            <SidebarLink icon="sticky_note_2"  label="Notes"     on_click=nav!("/notes")     collapsed=collapsed />
            <SidebarLink icon="dashboard"      label="Dashboard" on_click=nav!("/dashboard") collapsed=collapsed />
            <SidebarLink icon="share"          label="Graph"     on_click=nav!("/graph")     collapsed=collapsed />
            <div class="border-t border-stone-200 dark:border-stone-700 my-3" />
            // ── Section 2: Knowledge base ──────────────────────────────────────
            <SidebarLink
                icon="segment" label="All Nodes"
                on_click={
                    let n = navigate.get_value();
                    move || { node_type_filter.set(None); n("/nodes", Default::default()); on_nav.run(()); }
                }
                collapsed=collapsed
            />
            // Type-specific sub-links (hidden when sidebar is collapsed)
            {move || {
                if collapsed.get() { return None; }
                Some(view! {
                    <div class="ml-3 border-l border-stone-200 dark:border-stone-700 pl-2 space-y-0.5">
                        <TypeFilterLink icon="description"  label="Articles"   value="article"   node_type_filter=node_type_filter on_nav=on_nav />
                        <TypeFilterLink icon="rocket_launch" label="Projects"  value="project"   node_type_filter=node_type_filter on_nav=on_nav />
                        <TypeFilterLink icon="category"     label="Areas"      value="area"      node_type_filter=node_type_filter on_nav=on_nav />
                        <TypeFilterLink icon="bookmarks"    label="Resources"  value="resource"  node_type_filter=node_type_filter on_nav=on_nav />
                        <TypeFilterLink icon="menu_book"    label="References" value="reference" node_type_filter=node_type_filter on_nav=on_nav />
                    </div>
                }.into_any())
            }}
            // Favorites — pinned nodes and external URLs
            <FavoritesSection collapsed=collapsed on_nav=on_nav />
            // Recent nodes — re-reads localStorage on every location change
            <RecentSection collapsed=collapsed on_nav=on_nav />
            <div class="border-t border-stone-200 dark:border-stone-700 my-3" />
            // ── Section 3: Content tools ───────────────────────────────────────
            <SidebarLink icon="label"        label="Tags"      on_click=nav!("/tags")      collapsed=collapsed />
            <SidebarLink icon="content_copy" label="Templates" on_click=nav!("/templates") collapsed=collapsed />
            <SidebarLink icon="webhook"      label="Webhooks"  on_click=nav!("/webhooks")  collapsed=collapsed />
            // ── Section 4: Admin (only visible to admins) ─────────────────────
            {move || {
                if let AuthStatus::Authenticated(ref u) = auth_state.get()
                    && u.roles.contains(&"admin".to_string())
                {
                    Some(view! {
                        <div>
                            <div class="border-t border-stone-200 dark:border-stone-700 my-3" />
                            <SidebarLink icon="group"          label="Users"       on_click=nav!("/admin/users")       collapsed=collapsed />
                            <SidebarLink icon="manage_accounts" label="Permissions" on_click=nav!("/admin/permissions") collapsed=collapsed />
                            <SidebarLink icon="backup"         label="Backup"      on_click=nav!("/admin/backup")      collapsed=collapsed />
                            <SidebarLink
                                icon="download"
                                label="Export ZIP"
                                on_click=move || {
                                    if let Some(window) = web_sys::window() {
                                        let _ = window.open_with_url("/api/export");
                                    }
                                }
                                collapsed=collapsed
                            />
                        </div>
                    }.into_any())
                } else {
                    None
                }
            }}
        </nav>
        // User / logout section
        <div class="px-2 py-3 border-t border-stone-200 dark:border-stone-800">
            {move || {
                let is_collapsed = collapsed.get();
                if let AuthStatus::Authenticated(ref user) = auth_state.get() {
                    let name = user.name.clone()
                        .or_else(|| user.email.clone())
                        .unwrap_or_else(|| user.sub.clone());
                    if is_collapsed {
                        Some(view! {
                            <div class="flex flex-col gap-1">
                                <button
                                    class="flex items-center justify-center w-full p-2 rounded-lg
                                        text-stone-500 hover:bg-stone-100 dark:hover:bg-stone-800
                                        dark:text-stone-400 cursor-pointer"
                                    title=format!("{name} — Change password")
                                    on:click=move |_| show_change_pw.set(true)
                                >
                                    <span class="material-symbols-outlined">"lock_reset"</span>
                                </button>
                                <button
                                    on:click=on_logout
                                    class="flex items-center justify-center w-full p-2 rounded-lg
                                        text-stone-500 hover:bg-stone-100 dark:hover:bg-stone-800
                                        dark:text-stone-400 cursor-pointer"
                                    title=format!("{name} — Logout")
                                >
                                    <span class="material-symbols-outlined">"logout"</span>
                                </button>
                            </div>
                        }.into_any())
                    } else {
                        Some(view! {
                            <div class="space-y-1 px-1">
                                <span class="block text-xs text-stone-500 dark:text-stone-400 truncate">
                                    {name}
                                </span>
                                <div class="flex items-center justify-between">
                                    <button
                                        class="text-xs text-stone-400 hover:text-amber-500
                                               dark:hover:text-amber-400 cursor-pointer
                                               flex items-center gap-1 transition-colors"
                                        on:click=move |_| show_change_pw.set(true)
                                    >
                                        <span class="material-symbols-outlined" style="font-size: 13px;">
                                            "lock_reset"
                                        </span>
                                        "Change password"
                                    </button>
                                    <button
                                        class="text-xs text-stone-400 hover:text-stone-600
                                               dark:hover:text-stone-300 cursor-pointer"
                                        on:click=on_logout
                                    >
                                        "Logout"
                                    </button>
                                </div>
                            </div>
                        }.into_any())
                    }
                } else {
                    None
                }
            }}
        </div>

        // Change-password modal
        {move || show_change_pw.get().then(|| view! {
            <ChangePasswordModal on_close=Callback::new(move |_| show_change_pw.set(false)) />
        })}
    }
}

// ── Recent nodes ─────────────────────────────────────────────────────────────

#[component]
fn RecentSection(collapsed: SidebarCollapsed, on_nav: Callback<()>) -> impl IntoView {
    // Re-read on every location change so new visits appear immediately.
    let location = use_location();
    let navigate = StoredValue::new(use_navigate());

    move || {
        let _path = location.pathname.get(); // reactive dependency
        let entries = crate::recent::read_recent();

        if entries.is_empty() {
            return None;
        }

        let is_collapsed = collapsed.get();

        Some(
            view! {
                <div>
                    {(!is_collapsed).then(|| view! {
                        <p class="px-3 mb-1 text-[10px] font-semibold uppercase tracking-widest
                                   text-stone-400 dark:text-stone-500 select-none">
                            "Recent"
                        </p>
                    })}
                    {entries.into_iter().map(|entry| {
                        let id = entry.id;
                        let title = entry.title.clone();
                        let icon = entry.icon.clone();
                        let label = entry.title.clone();
                        let n = navigate.get_value();
                        view! {
                            <button
                                class=move || {
                                    let base = "flex items-center w-full rounded-lg text-sm \
                                        text-stone-600 dark:text-stone-400 \
                                        hover:bg-stone-100 dark:hover:bg-stone-800 \
                                        hover:text-stone-800 dark:hover:text-stone-200 \
                                        transition-colors cursor-pointer py-1.5";
                                    if collapsed.get() {
                                        format!("{base} justify-center px-0")
                                    } else {
                                        format!("{base} px-3 gap-2")
                                    }
                                }
                                title=title.clone()
                                on:click=move |_| {
                                    n(&format!("/nodes/{id}"), Default::default());
                                    on_nav.run(());
                                }
                            >
                                <span class="material-symbols-outlined shrink-0
                                             text-stone-400 dark:text-stone-500"
                                      style="font-size: 16px;">
                                    {icon.clone()}
                                </span>
                                <span class="truncate text-xs"
                                      class:hidden=move || collapsed.get()>
                                    {label.clone()}
                                </span>
                            </button>
                        }
                    }).collect_view()}
                </div>
            }
            .into_any(),
        )
    }
}

// ── TypeFilterLink ────────────────────────────────────────────────────────────

#[component]
fn TypeFilterLink(
    icon: &'static str,
    label: &'static str,
    value: &'static str,
    node_type_filter: RwSignal<Option<String>>,
    on_nav: Callback<()>,
) -> impl IntoView {
    let navigate = StoredValue::new(use_navigate());
    view! {
        <button
            class=move || {
                let active = node_type_filter.get().as_deref() == Some(value);
                let base = "flex items-center w-full gap-2 px-2 py-1.5 rounded-lg text-xs \
                            font-medium transition-colors cursor-pointer";
                if active {
                    format!("{base} bg-amber-50 dark:bg-amber-900/20 \
                             text-amber-700 dark:text-amber-400")
                } else {
                    format!("{base} text-stone-600 dark:text-stone-400 \
                             hover:bg-stone-100 dark:hover:bg-stone-800 \
                             hover:text-stone-800 dark:hover:text-stone-200")
                }
            }
            on:click=move |_| {
                node_type_filter.set(Some(value.to_string()));
                navigate.get_value()("/nodes", Default::default());
                on_nav.run(());
            }
            title=label
        >
            <span class="material-symbols-outlined" style="font-size: 14px;">{icon}</span>
            {label}
        </button>
    }
}

// ── SidebarLink ───────────────────────────────────────────────────────────────

#[component]
fn SidebarLink(
    icon: &'static str,
    label: &'static str,
    #[prop(into)] on_click: Callback<()>,
    collapsed: SidebarCollapsed,
) -> impl IntoView {
    view! {
        <button
            class=move || {
                let base = "flex items-center w-full rounded-lg text-sm font-medium \
                    text-stone-700 dark:text-stone-300 hover:bg-stone-100 dark:hover:bg-stone-800 \
                    transition-colors cursor-pointer py-2";
                if collapsed.get() {
                    format!("{base} justify-center px-0")
                } else {
                    format!("{base} px-3 gap-3")
                }
            }
            title=label
            on:click=move |_| on_click.run(())
        >
            <span class="material-symbols-outlined text-stone-500 dark:text-stone-400">{icon}</span>
            <span
                class="truncate"
                class:hidden=move || collapsed.get()
            >
                {label}
            </span>
        </button>
    }
}
