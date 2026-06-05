//! Bulk permission management view — lists every permission row in the system,
//! grouped by node, with inline role editing and revocation.
//!
//! Data sources (fetched in parallel via `LocalResource`):
//!  - `GET /permissions`          — all permission rows
//!  - `GET /nodes/titles`         — node-id → title map
//!  - `GET /admin/users`          — subject-id → email/name map (admin only)

use std::collections::HashMap;

use common::{
    id::{NodeId, PermissionId},
    permission::{Permission, PermissionRole, UpdatePermissionRequest},
};
use leptos::prelude::*;

use crate::components::toast::{ToastLevel, push_toast};
use leptos_router::hooks::use_navigate;

// ── Helpers ───────────────────────────────────────────────────────────────────

fn role_label(role: &PermissionRole) -> &'static str {
    match role {
        PermissionRole::Owner => "Owner",
        PermissionRole::Editor => "Editor",
        PermissionRole::Viewer => "Viewer",
    }
}

fn role_badge_class(role: &PermissionRole) -> &'static str {
    match role {
        PermissionRole::Owner => {
            "px-2 py-0.5 rounded-full text-xs font-medium \
             bg-amber-100 text-amber-800 dark:bg-amber-900/30 dark:text-amber-300"
        }
        PermissionRole::Editor => {
            "px-2 py-0.5 rounded-full text-xs font-medium \
             bg-sky-100 text-sky-800 dark:bg-sky-900/30 dark:text-sky-300"
        }
        PermissionRole::Viewer => {
            "px-2 py-0.5 rounded-full text-xs font-medium \
             bg-stone-100 text-stone-700 dark:bg-stone-800 dark:text-stone-300"
        }
    }
}

/// Turn a Cognito sub + optional email/name into a one-line display string.
fn display_name(sub: &str, user_map: &HashMap<String, (String, String)>) -> String {
    if let Some((email, name)) = user_map.get(sub) {
        if !name.is_empty() {
            format!("{name} <{email}>")
        } else if !email.is_empty() {
            email.clone()
        } else {
            sub[..sub.len().min(12)].to_string()
        }
    } else {
        // Truncate long sub strings for display
        let short = &sub[..sub.len().min(16)];
        format!("{short}…")
    }
}

// ── Component ─────────────────────────────────────────────────────────────────

#[component]
pub fn BulkPermissionsView() -> impl IntoView {
    // Refresh counter — bump after any mutation to reload the permissions list.
    let refresh = RwSignal::new(0u32);

    // ── Data fetching (parallel) ──────────────────────────────────────────────

    let perms_resource = LocalResource::new(move || {
        let _ = refresh.get();
        async move { crate::api::list_all_permissions().await.unwrap_or_default() }
    });

    let titles_resource =
        LocalResource::new(
            || async move { crate::api::fetch_node_titles().await.unwrap_or_default() },
        );

    // user_map: sub → (email, display_name)
    let users_resource = LocalResource::new(|| async move {
        crate::api::list_admin_users()
            .await
            .unwrap_or_default()
            .into_iter()
            .map(|u| {
                let email = u.email.clone().unwrap_or_default();
                let name = match (u.first_name.as_deref(), u.last_name.as_deref()) {
                    (Some(f), Some(l)) if !f.is_empty() || !l.is_empty() => {
                        format!("{f} {l}").trim().to_string()
                    }
                    _ => String::new(),
                };
                (u.id, (email, name))
            })
            .collect::<HashMap<String, (String, String)>>()
    });

    // ── Filter ────────────────────────────────────────────────────────────────

    let filter = RwSignal::new(String::new());

    // ── View ──────────────────────────────────────────────────────────────────

    view! {
        <div class="flex-1 flex flex-col min-h-0 p-4 md:p-6">

            // ── Header ─────────────────────────────────────────────────────
            <div class="flex items-center justify-between mb-6">
                <h1 class="text-xl font-semibold text-stone-900 dark:text-stone-100">
                    "Permission Management"
                </h1>
            </div>

            // ── Filter input ───────────────────────────────────────────────
            <div class="mb-4">
                <input
                    type="text"
                    class="w-full max-w-sm px-3 py-2 rounded-lg border border-stone-200
                           dark:border-stone-600 bg-white dark:bg-stone-800
                           text-sm text-stone-900 dark:text-stone-100
                           focus:outline-none focus:ring-2 focus:ring-amber-400"
                    placeholder="Filter by node title or user…"
                    prop:value=move || filter.get()
                    on:input=move |ev| filter.set(event_target_value(&ev))
                />
            </div>

            // ── Table ──────────────────────────────────────────────────────
            <div class="flex-1 overflow-y-auto min-h-0">
                <Transition fallback=move || view! {
                    <p class="text-sm text-stone-400 dark:text-stone-500 animate-pulse">
                        "Loading…"
                    </p>
                }>
                    {move || {
                        let perms = perms_resource.get().unwrap_or_default();
                        let titles = titles_resource.get().unwrap_or_default();
                        let user_map = users_resource.get().unwrap_or_default();

                        // Build node-id → title map.
                        let title_map: HashMap<NodeId, String> = titles
                            .iter()
                            .map(|t| (t.id, t.title.clone()))
                            .collect();

                        // Group permissions by node_id, preserving stable order.
                        let mut node_order: Vec<NodeId> = Vec::new();
                        let mut groups: HashMap<NodeId, Vec<Permission>> = HashMap::new();
                        for p in &perms {
                            if !groups.contains_key(&p.node_id) {
                                node_order.push(p.node_id);
                            }
                            groups.entry(p.node_id).or_default().push(p.clone());
                        }

                        // Sort groups by node title.
                        node_order.sort_by(|a, b| {
                            let ta = title_map.get(a).map(String::as_str).unwrap_or("");
                            let tb = title_map.get(b).map(String::as_str).unwrap_or("");
                            ta.cmp(tb)
                        });

                        let filter_str = filter.get().to_lowercase();

                        let nodes_view: Vec<_> = node_order.into_iter().filter_map(|nid| {
                            let title = title_map.get(&nid)
                                .cloned()
                                .unwrap_or_else(|| nid.0.to_string());
                            let rows = groups.remove(&nid).unwrap_or_default();

                            // Apply filter: keep group if title or any user matches.
                            if !filter_str.is_empty() {
                                let title_matches = title.to_lowercase().contains(&filter_str);
                                let user_matches = rows.iter().any(|p| {
                                    display_name(&p.subject_id, &user_map)
                                        .to_lowercase()
                                        .contains(&filter_str)
                                });
                                if !title_matches && !user_matches {
                                    return None;
                                }
                            }

                            Some(view! {
                                <NodeGroup
                                    node_id=nid
                                    title=title
                                    rows=rows
                                    user_map=user_map.clone()
                                    refresh=refresh
                                />
                            })
                        }).collect();

                        if nodes_view.is_empty() {
                            view! {
                                <p class="text-sm text-stone-400 dark:text-stone-500">
                                    {if filter.get().is_empty() {
                                        "No permissions found."
                                    } else {
                                        "No matches."
                                    }}
                                </p>
                            }.into_any()
                        } else {
                            view! {
                                <div class="space-y-4">{nodes_view}</div>
                            }.into_any()
                        }
                    }}
                </Transition>
            </div>
        </div>
    }
}

// ── NodeGroup sub-component ───────────────────────────────────────────────────

#[component]
fn NodeGroup(
    node_id: NodeId,
    title: String,
    rows: Vec<Permission>,
    user_map: HashMap<String, (String, String)>,
    refresh: RwSignal<u32>,
) -> impl IntoView {
    let navigate = use_navigate();
    view! {
        <div class="rounded-xl border border-stone-200 dark:border-stone-700
                    bg-white dark:bg-stone-900 overflow-hidden">
            // ── Node header ──────────────────────────────────────────────
            <div class="flex items-center justify-between px-4 py-3
                        bg-stone-50 dark:bg-stone-800
                        border-b border-stone-200 dark:border-stone-700">
                <span class="font-medium text-stone-900 dark:text-stone-100 truncate">
                    {title}
                </span>
                <button
                    class="flex items-center gap-1 text-xs text-stone-400 hover:text-amber-600
                           dark:hover:text-amber-400 transition-colors cursor-pointer flex-shrink-0 ml-2"
                    title="Go to node"
                    on:click=move |_| navigate(&format!("/nodes/{node_id}"), Default::default())
                >
                    <span class="material-symbols-outlined" style="font-size: 16px;">"open_in_new"</span>
                    "Open"
                </button>
            </div>
            // ── Permission rows ───────────────────────────────────────────
            <div class="divide-y divide-stone-100 dark:divide-stone-800">
                {rows.into_iter().map(|p| {
                    let um = user_map.clone();
                    view! {
                        <PermRow
                            perm=p
                            user_map=um
                            refresh=refresh
                        />
                    }
                }).collect_view()}
            </div>
        </div>
    }
}

// ── PermRow sub-component ─────────────────────────────────────────────────────

#[component]
fn PermRow(
    perm: Permission,
    user_map: HashMap<String, (String, String)>,
    refresh: RwSignal<u32>,
) -> impl IntoView {
    let is_owner = perm.role == PermissionRole::Owner;
    let perm_id = perm.id;
    let subject_display = display_name(&perm.subject_id, &user_map);
    let subject_display2 = subject_display.clone();
    let current_role = perm.role.clone();

    // Local optimistic role signal (so the select reflects changes instantly).
    let role_sig = RwSignal::new(match &current_role {
        PermissionRole::Owner => "owner",
        PermissionRole::Editor => "editor",
        PermissionRole::Viewer => "viewer",
    });

    let on_role_change = move |new_role_str: String| {
        let new_role = match new_role_str.as_str() {
            "editor" => PermissionRole::Editor,
            "viewer" => PermissionRole::Viewer,
            _ => return, // owner not selectable
        };
        let new_role_str_static = match new_role_str.as_str() {
            "editor" => "editor",
            "viewer" => "viewer",
            _ => return,
        };
        role_sig.set(new_role_str_static);
        let req = UpdatePermissionRequest { role: new_role };
        leptos::task::spawn_local(async move {
            match crate::api::update_permission(perm_id, &req).await {
                Ok(_) => push_toast(ToastLevel::Success, "Role updated."),
                Err(e) => {
                    push_toast(ToastLevel::Error, format!("Update failed: {e}"));
                    refresh.update(|n| *n += 1); // re-fetch to reset
                }
            }
        });
    };

    let on_revoke = move |_| {
        let id: PermissionId = perm_id;
        leptos::task::spawn_local(async move {
            // Use the standalone DELETE /permissions/{id} route.
            let resp = gloo_net::http::Request::delete(&crate::api::api_url(&format!(
                "/permissions/{}",
                id.0
            )))
            .send()
            .await;
            match resp {
                Ok(r) if r.ok() => {
                    push_toast(ToastLevel::Success, "Permission revoked.");
                    refresh.update(|n| *n += 1);
                }
                Ok(r) => {
                    let status = r.status();
                    push_toast(ToastLevel::Error, format!("Revoke failed ({status})."));
                }
                Err(e) => push_toast(ToastLevel::Error, format!("Revoke failed: {e}")),
            }
        });
    };

    view! {
        <div class=move || {
            let base = "flex items-center gap-3 px-4 py-2.5 text-sm";
            if is_owner {
                format!("{base} opacity-60")
            } else {
                base.to_string()
            }
        }>
            // Role badge
            <span class=role_badge_class(&perm.role)>
                {role_label(&perm.role)}
            </span>

            // User display name
            <span class="flex-1 truncate text-stone-700 dark:text-stone-300 text-xs"
                  title=subject_display>
                {subject_display2}
            </span>

            // Role selector (disabled for owner rows)
            {if is_owner {
                view! { <span /> }.into_any()
            } else {
                view! {
                    <select
                        class="text-xs px-2 py-1 rounded border border-stone-200 dark:border-stone-600
                               bg-white dark:bg-stone-800 text-stone-700 dark:text-stone-300
                               focus:outline-none focus:ring-1 focus:ring-amber-400 cursor-pointer"
                        on:change=move |ev| on_role_change(event_target_value(&ev))
                    >
                        <option value="editor" selected=move || role_sig.get() == "editor">"Editor"</option>
                        <option value="viewer" selected=move || role_sig.get() == "viewer">"Viewer"</option>
                    </select>
                }.into_any()
            }}

            // Revoke button (disabled for owner rows)
            {if is_owner {
                view! { <span class="w-7" /> }.into_any()
            } else {
                view! {
                    <button
                        class="p-1 rounded text-stone-400 hover:text-red-600 dark:hover:text-red-400
                               hover:bg-stone-100 dark:hover:bg-stone-800
                               transition-colors cursor-pointer"
                        title="Revoke"
                        on:click=on_revoke
                    >
                        <span class="material-symbols-outlined" style="font-size: 16px;">"person_remove"</span>
                    </button>
                }.into_any()
            }}
        </div>
    }
}
