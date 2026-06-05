//! Admin panel — user management via Amazon Cognito Identity Provider API.
//!
//! Only accessible to users with the `admin` group.  The sidebar hides
//! the link for non-admins, but all API calls are independently protected by
//! the backend.

use common::admin::{AdminUser, CreateAdminUserRequest, UpdateUserRolesRequest};
use leptos::prelude::*;

use crate::api;

// ── Role chip colours ─────────────────────────────────────────────────────────

fn role_chip_class(role: &str) -> &'static str {
    match role {
        "admin" => "bg-purple-100 text-purple-700 dark:bg-purple-900/40 dark:text-purple-300",
        "user" => "bg-amber-100 text-amber-700 dark:bg-amber-900/40 dark:text-amber-300",
        _ => "bg-stone-100 text-stone-600 dark:bg-stone-800 dark:text-stone-400",
    }
}

// ── Component ─────────────────────────────────────────────────────────────────

#[component]
pub fn AdminView() -> impl IntoView {
    let refresh = RwSignal::new(0u32);

    // ── Create-user form state ────────────────────────────────────────────────
    let show_create = RwSignal::new(false);
    let form_email = RwSignal::new(String::new());
    let form_first_name = RwSignal::new(String::new());
    let form_last_name = RwSignal::new(String::new());
    let form_send_email = RwSignal::new(true);
    let create_error = RwSignal::new(Option::<String>::None);
    let creating = RwSignal::new(false);

    // Available realm roles (fetched once).
    let available_roles =
        LocalResource::new(|| async move { api::list_realm_roles().await.unwrap_or_default() });

    // Selected roles for the create form — use a Vec<String> signal.
    let form_roles: RwSignal<Vec<String>> = RwSignal::new(vec!["user".to_string()]);

    // ── User list ─────────────────────────────────────────────────────────────
    let users = LocalResource::new(move || {
        let _ = refresh.get();
        async move { api::list_admin_users().await }
    });

    // ── Create handler ────────────────────────────────────────────────────────
    let on_create = move |_| {
        let email = form_email.get_untracked().trim().to_string();
        let first_name = form_first_name.get_untracked().trim().to_string();
        let last_name = form_last_name.get_untracked().trim().to_string();
        let send_email = form_send_email.get_untracked();
        let initial_roles = form_roles.get_untracked();

        if email.is_empty() {
            create_error.set(Some("Email is required.".to_string()));
            return;
        }

        create_error.set(None);
        creating.set(true);

        let req = CreateAdminUserRequest {
            email,
            first_name,
            last_name,
            initial_roles,
            send_welcome_email: send_email,
        };

        wasm_bindgen_futures::spawn_local(async move {
            match api::create_admin_user(&req).await {
                Ok(_) => {
                    show_create.set(false);
                    form_email.set(String::new());
                    form_first_name.set(String::new());
                    form_last_name.set(String::new());
                    form_roles.set(vec!["user".to_string()]);
                    refresh.update(|n| *n += 1);
                }
                Err(e) => create_error.set(Some(format!("Failed: {e}"))),
            }
            creating.set(false);
        });
    };

    view! {
        <div class="p-6">
            // ── Page header ───────────────────────────────────────────────────
            <div class="flex items-center justify-between mb-6">
                <div>
                    <h1 class="text-xl font-semibold text-stone-900 dark:text-stone-100">
                        "User Management"
                    </h1>
                    <p class="text-sm text-stone-500 dark:text-stone-400 mt-0.5">
                        "Create and manage Cognito users and groups."
                    </p>
                </div>
                <button
                    class="flex items-center gap-2 px-3 py-2 rounded-lg bg-amber-600
                        text-white text-sm font-medium hover:bg-amber-700 transition-colors"
                    on:click=move |_| {
                        create_error.set(None);
                        show_create.update(|v| *v = !*v);
                    }
                >
                    <span class="material-symbols-outlined" style="font-size: 16px;">
                        {move || if show_create.get() { "close" } else { "person_add" }}
                    </span>
                    {move || if show_create.get() { "Cancel" } else { "Add User" }}
                </button>
            </div>

            // ── Create user form ──────────────────────────────────────────────
            {move || show_create.get().then(|| {
                let roles_snapshot = available_roles.get().unwrap_or_default();
                view! {
                    <div class="mb-6 p-5 bg-white dark:bg-stone-900 rounded-xl border border-stone-200 dark:border-stone-700 shadow-sm space-y-4">
                        <h2 class="text-sm font-semibold text-stone-700 dark:text-stone-300">
                            "New User"
                        </h2>

                        // Row 1: email (used as Cognito username)
                        <div>
                            <label class="block text-xs text-stone-500 dark:text-stone-400 mb-1">
                                "Email *"
                            </label>
                            <input
                                type="email"
                                class="w-full px-3 py-1.5 text-sm rounded-lg border border-stone-300 dark:border-stone-600
                                    bg-transparent text-stone-900 dark:text-stone-100 focus:outline-none focus:ring-2 focus:ring-amber-500"
                                placeholder="j.doe@example.com"
                                prop:value=move || form_email.get()
                                on:input=move |ev| form_email.set(event_target_value(&ev))
                            />
                        </div>

                        // Row 2: first + last name
                        <div class="grid grid-cols-1 sm:grid-cols-2 gap-3">
                            <div>
                                <label class="block text-xs text-stone-500 dark:text-stone-400 mb-1">
                                    "First Name"
                                </label>
                                <input
                                    type="text"
                                    class="w-full px-3 py-1.5 text-sm rounded-lg border border-stone-300 dark:border-stone-600
                                        bg-transparent text-stone-900 dark:text-stone-100 focus:outline-none focus:ring-2 focus:ring-amber-500"
                                    placeholder="Jane"
                                    prop:value=move || form_first_name.get()
                                    on:input=move |ev| form_first_name.set(event_target_value(&ev))
                                />
                            </div>
                            <div>
                                <label class="block text-xs text-stone-500 dark:text-stone-400 mb-1">
                                    "Last Name"
                                </label>
                                <input
                                    type="text"
                                    class="w-full px-3 py-1.5 text-sm rounded-lg border border-stone-300 dark:border-stone-600
                                        bg-transparent text-stone-900 dark:text-stone-100 focus:outline-none focus:ring-2 focus:ring-amber-500"
                                    placeholder="Doe"
                                    prop:value=move || form_last_name.get()
                                    on:input=move |ev| form_last_name.set(event_target_value(&ev))
                                />
                            </div>
                        </div>

                        // Row 3: roles
                        <div>
                            <label class="block text-xs text-stone-500 dark:text-stone-400 mb-1">
                                "Initial Roles"
                            </label>
                            <div class="flex flex-wrap gap-2">
                                {roles_snapshot.into_iter().map(|role: String| {
                                    let role_clone = role.clone();
                                    let checked = move || form_roles.get().contains(&role_clone);
                                    let role_for_toggle = role.clone();
                                    let chip_class = role_chip_class(&role);
                                    view! {
                                        <label class="flex items-center gap-1.5 cursor-pointer select-none">
                                            <input
                                                type="checkbox"
                                                class="cursor-pointer"
                                                prop:checked=checked
                                                on:change=move |_| {
                                                    form_roles.update(|roles| {
                                                        if roles.contains(&role_for_toggle) {
                                                            roles.retain(|r| r != &role_for_toggle);
                                                        } else {
                                                            roles.push(role_for_toggle.clone());
                                                        }
                                                    });
                                                }
                                            />
                                            <span class={format!("px-2 py-0.5 text-xs rounded-full font-medium {chip_class}")}>
                                                {role.clone()}
                                            </span>
                                        </label>
                                    }
                                }).collect::<Vec<_>>()}
                            </div>
                        </div>

                        // Row 4: send welcome email toggle + submit
                        <div class="flex items-center justify-between pt-1">
                            <label class="flex items-center gap-2 cursor-pointer select-none text-sm text-stone-600 dark:text-stone-400">
                                <input
                                    type="checkbox"
                                    class="cursor-pointer"
                                    prop:checked=move || form_send_email.get()
                                    on:change=move |ev| {
                                        use wasm_bindgen::JsCast;
                                        let checked = ev.target()
                                            .and_then(|t| t.dyn_into::<web_sys::HtmlInputElement>().ok())
                                            .map(|el| el.checked())
                                            .unwrap_or(false);
                                        form_send_email.set(checked);
                                    }
                                />
                                "Send password-setup email"
                            </label>
                            <button
                                class="px-4 py-2 rounded-lg bg-amber-600 text-white text-sm font-medium
                                    hover:bg-amber-700 disabled:opacity-40 transition-colors"
                                on:click=on_create
                                disabled=move || creating.get()
                            >
                                {move || if creating.get() { "Creating…" } else { "Create User" }}
                            </button>
                        </div>

                        // Error message
                        {move || create_error.get().map(|msg| view! {
                            <div class="text-xs text-red-500 pt-1">{msg}</div>
                        })}
                    </div>
                }
            })}

            // ── User table ────────────────────────────────────────────────────
            <Suspense fallback=|| view! {
                <div class="space-y-2">
                    {(0..4).map(|_| view! {
                        <div class="h-12 rounded-lg bg-stone-100 dark:bg-stone-800 animate-pulse" />
                    }).collect::<Vec<_>>()}
                </div>
            }>
                {move || {
                    users.get().map(|result: Result<Vec<AdminUser>, _>| match result {
                        Err(e) => view! {
                            <div class="rounded-lg border border-red-200 dark:border-red-900 bg-red-50 dark:bg-red-950/30
                                px-4 py-3 text-sm text-red-600 dark:text-red-400">
                                {format!("Error loading users: {e}")}
                            </div>
                        }.into_any(),
                        Ok(list) if list.is_empty() => view! {
                            <div class="flex flex-col items-center gap-3 py-16 text-center">
                                <span class="material-symbols-outlined text-stone-300 dark:text-stone-700"
                                    style="font-size: 48px;">"person_off"</span>
                                <p class="text-sm text-stone-400 dark:text-stone-600">
                                    "No users found in this pool."
                                </p>
                            </div>
                        }.into_any(),
                        Ok(list) => view! {
                            <div class="bg-white dark:bg-stone-900 rounded-xl border border-stone-200
                                dark:border-stone-700 shadow-sm overflow-hidden">
                                // Table header
                                <div class="grid grid-cols-[1fr_1fr_auto_auto] gap-4 px-4 py-2.5
                                    border-b border-stone-200 dark:border-stone-800
                                    text-xs font-medium text-stone-500 dark:text-stone-400 uppercase tracking-wide">
                                    <span>"User"</span>
                                    <span>"Email"</span>
                                    <span>"Roles"</span>
                                    <span></span>
                                </div>
                                // Rows
                                {list.into_iter().map(|user| {
                                    view! { <UserRow user=user refresh=refresh available_roles=available_roles /> }
                                }).collect::<Vec<_>>()}
                            </div>
                        }.into_any(),
                    })
                }}
            </Suspense>
        </div>
    }
}

// ── UserRow ───────────────────────────────────────────────────────────────────

#[component]
fn UserRow(
    user: AdminUser,
    refresh: RwSignal<u32>,
    available_roles: LocalResource<Vec<String>>,
) -> impl IntoView {
    let user_id = user.id.clone();
    let display_name = user.display_name();
    let email = user.email.clone().unwrap_or_default();
    let enabled = user.enabled;
    let realm_roles = user.realm_roles.clone();

    // Delete confirmation state (two-click pattern).
    let confirm_delete = RwSignal::new(false);
    let deleting = RwSignal::new(false);

    // Role editor state.
    let show_roles = RwSignal::new(false);
    let edited_roles: RwSignal<Vec<String>> = RwSignal::new(realm_roles.clone());
    let saving_roles = RwSignal::new(false);
    let roles_error = RwSignal::new(Option::<String>::None);

    // Separate clone for the role-editor closure — the delete closure moves `user_id`.
    let user_id_for_roles = user_id.clone();

    view! {
        <div class="border-b border-stone-100 dark:border-stone-800 last:border-0">
            // Main row
            <div class="grid grid-cols-[1fr_1fr_auto_auto] gap-4 items-center px-4 py-3 group hover:bg-stone-50 dark:hover:bg-stone-800/40">
                // Name + status
                <div class="flex items-center gap-2 min-w-0">
                    <div class={format!(
                        "flex-shrink-0 w-7 h-7 rounded-full flex items-center justify-center text-xs font-semibold {}",
                        if enabled {
                            "bg-amber-100 text-amber-700 dark:bg-amber-900/40 dark:text-amber-300"
                        } else {
                            "bg-stone-100 text-stone-400 dark:bg-stone-800 dark:text-stone-600"
                        }
                    )}>
                        {display_name.chars().next().unwrap_or('?').to_uppercase().to_string()}
                    </div>
                    <div class="min-w-0">
                        <span class="text-sm font-medium text-stone-900 dark:text-stone-100 truncate block">
                            {display_name.clone()}
                        </span>
                        <span class="text-xs text-stone-400 dark:text-stone-500 truncate block">
                            {format!("@{}", user.username)}
                        </span>
                    </div>
                </div>

                // Email
                <span class="text-sm text-stone-600 dark:text-stone-400 truncate">{email}</span>

                // Role chips
                <div class="flex flex-wrap gap-1">
                    {realm_roles.iter().map(|role| {
                        let chip_class = role_chip_class(role);
                        view! {
                            <span class={format!("px-1.5 py-0.5 text-[10px] rounded-full font-medium {chip_class}")}>
                                {role.clone()}
                            </span>
                        }
                    }).collect::<Vec<_>>()}
                </div>

                // Actions
                <div class="flex items-center gap-1">
                    // Edit roles button
                    <button
                        class="p-1.5 rounded-lg text-stone-400 hover:text-amber-600 dark:hover:text-amber-400
                            hover:bg-amber-50 dark:hover:bg-amber-900/20 transition-colors"
                        title="Edit roles"
                        on:click=move |_| {
                            roles_error.set(None);
                            show_roles.update(|v| *v = !*v);
                        }
                    >
                        <span class="material-symbols-outlined" style="font-size: 16px;">
                            "manage_accounts"
                        </span>
                    </button>

                    // Delete button (two-click confirm) — inline closures to satisfy FnMut bound.
                    {move || {
                        let uid_del = user_id.clone();
                        if confirm_delete.get() {
                            view! {
                                <div class="flex items-center gap-1">
                                    <span class="text-xs text-red-500 whitespace-nowrap">"Delete?"</span>
                                    <button
                                        class="p-1.5 rounded-lg text-white bg-red-500 hover:bg-red-600 transition-colors disabled:opacity-40"
                                        on:click=move |_| {
                                            deleting.set(true);
                                            let id = uid_del.clone();
                                            wasm_bindgen_futures::spawn_local(async move {
                                                match api::delete_admin_user(&id).await {
                                                    Ok(_) => refresh.update(|n| *n += 1),
                                                    Err(e) => {
                                                        tracing::error!("delete user failed: {e}");
                                                        deleting.set(false);
                                                        confirm_delete.set(false);
                                                    }
                                                }
                                            });
                                        }
                                        disabled=move || deleting.get()
                                    >
                                        <span class="material-symbols-outlined" style="font-size: 14px;">"check"</span>
                                    </button>
                                    <button
                                        class="p-1.5 rounded-lg text-stone-400 hover:text-stone-600 dark:hover:text-stone-300
                                            hover:bg-stone-100 dark:hover:bg-stone-800 transition-colors"
                                        on:click=move |_| confirm_delete.set(false)
                                    >
                                        <span class="material-symbols-outlined" style="font-size: 14px;">"close"</span>
                                    </button>
                                </div>
                            }.into_any()
                        } else {
                            view! {
                                <button
                                    class="p-1.5 rounded-lg text-stone-300 dark:text-stone-700
                                        hover:text-red-500 dark:hover:text-red-400
                                        hover:bg-red-50 dark:hover:bg-red-900/20 transition-colors"
                                    title="Delete user"
                                    on:click=move |_| confirm_delete.set(true)
                                >
                                    <span class="material-symbols-outlined" style="font-size: 16px;">"delete"</span>
                                </button>
                            }.into_any()
                        }
                    }}
                </div>
            </div>

            // Role editor panel (expands below row)
            {move || show_roles.get().then(|| {
                let available = available_roles.get().unwrap_or_default();
                view! {
                    <div class="px-4 pb-4 pt-2 bg-stone-50 dark:bg-stone-800/50 border-t border-stone-100 dark:border-stone-800">
                        <div class="flex flex-wrap gap-2 mb-3">
                            {available.into_iter().map(|role: String| {
                                let role_clone = role.clone();
                                let checked = move || edited_roles.get().contains(&role_clone);
                                let role_for_toggle = role.clone();
                                let chip_class = role_chip_class(&role);
                                view! {
                                    <label class="flex items-center gap-1.5 cursor-pointer select-none">
                                        <input
                                            type="checkbox"
                                            class="cursor-pointer"
                                            prop:checked=checked
                                            on:change=move |_| {
                                                edited_roles.update(|roles| {
                                                    if roles.contains(&role_for_toggle) {
                                                        roles.retain(|r| r != &role_for_toggle);
                                                    } else {
                                                        roles.push(role_for_toggle.clone());
                                                    }
                                                });
                                            }
                                        />
                                        <span class={format!("px-2 py-0.5 text-xs rounded-full font-medium {chip_class}")}>
                                            {role.clone()}
                                        </span>
                                    </label>
                                }
                            }).collect::<Vec<_>>()}
                        </div>
                        <div class="flex items-center gap-2">
                            <button
                                class="px-3 py-1.5 rounded-lg bg-amber-600 text-white text-xs font-medium
                                    hover:bg-amber-700 disabled:opacity-40 transition-colors"
                                on:click={
                                    let uid_r = user_id_for_roles.clone();
                                    move |_| {
                                        saving_roles.set(true);
                                        roles_error.set(None);
                                        let id = uid_r.clone();
                                        let roles = edited_roles.get_untracked();
                                        wasm_bindgen_futures::spawn_local(async move {
                                            let req = UpdateUserRolesRequest { roles };
                                            match api::set_user_roles(&id, &req).await {
                                                Ok(_) => {
                                                    show_roles.set(false);
                                                    refresh.update(|n| *n += 1);
                                                }
                                                Err(e) => roles_error.set(Some(format!("Save failed: {e}"))),
                                            }
                                            saving_roles.set(false);
                                        });
                                    }
                                }
                                disabled=move || saving_roles.get()
                            >
                                {move || if saving_roles.get() { "Saving…" } else { "Save Roles" }}
                            </button>
                            <button
                                class="px-3 py-1.5 rounded-lg text-xs text-stone-500 hover:text-stone-700 dark:hover:text-stone-300
                                    hover:bg-stone-100 dark:hover:bg-stone-700 transition-colors"
                                on:click=move |_| show_roles.set(false)
                            >
                                "Cancel"
                            </button>
                            {move || roles_error.get().map(|msg| view! {
                                <span class="text-xs text-red-500">{msg}</span>
                            })}
                        </div>
                    </div>
                }
            })}
        </div>
    }
}
