/// Permission panel — list, invite, and revoke per-node access.
///
/// The "Invite" form accepts an email address and calls `POST /nodes/{id}/invite`.
/// The backend looks up the email in Cognito; if the user does not exist yet it
/// creates a Cognito account (which triggers a welcome / temporary-password email)
/// and then grants the permission.  If the user already exists the permission is
/// granted directly without sending an email.
use common::id::NodeId;
use common::permission::{InviteRequest, PermissionRole, UpdatePermissionRequest};
use leptos::prelude::*;

use crate::api;

#[component]
pub fn PermissionPanel(node_id: NodeId, is_owner: bool) -> impl IntoView {
    let refresh = RwSignal::new(0u32);
    let show_invite = RwSignal::new(false);
    let email_input = RwSignal::new(String::new());
    let role_input = RwSignal::new("viewer".to_string());
    let error_msg = RwSignal::new(Option::<String>::None);
    let saving = RwSignal::new(false);

    // Admin users — loaded once when the panel is first opened, used only to
    // resolve display names in the permission list.  The invite form no longer
    // requires it.
    let admin_users: RwSignal<Option<Vec<common::admin::AdminUser>>> = RwSignal::new(None);

    // open must be declared before permissions so the resource closure can capture it.
    let open = RwSignal::new(false);

    let permissions = LocalResource::new(move || {
        let _ = refresh.get();
        let is_open = open.get();
        let node_id = node_id;
        async move {
            if !is_open {
                return Ok(vec![]);
            }
            api::list_permissions(node_id).await
        }
    });
    let on_toggle_open = move |_| {
        let opening = !open.get_untracked();
        open.update(|v| *v = !*v);
        if opening && admin_users.get_untracked().is_none() {
            wasm_bindgen_futures::spawn_local(async move {
                match api::list_admin_users().await {
                    Ok(users) => admin_users.set(Some(users)),
                    Err(_) => admin_users.set(Some(vec![])),
                }
            });
        }
    };

    let on_invite = move |_| {
        let email = email_input.get_untracked().trim().to_string();
        if email.is_empty() {
            error_msg.set(Some("Please enter an email address.".to_string()));
            return;
        }
        let role = match role_input.get_untracked().as_str() {
            "owner" => PermissionRole::Owner,
            "editor" => PermissionRole::Editor,
            _ => PermissionRole::Viewer,
        };
        error_msg.set(None);
        saving.set(true);
        let req = InviteRequest { email, role };
        wasm_bindgen_futures::spawn_local(async move {
            match api::invite_to_node(node_id, &req).await {
                Ok(_) => {
                    show_invite.set(false);
                    email_input.set(String::new());
                    role_input.set("viewer".to_string());
                    refresh.update(|n| *n += 1);
                }
                Err(e) => error_msg.set(Some(format!("Invite failed: {e}"))),
            }
            saving.set(false);
        });
    };

    view! {
        <div class="mt-8 border-t border-stone-200 dark:border-stone-700 pt-6">
            <div class="flex items-center justify-between">
                <button
                    class="flex items-center gap-1 text-left cursor-pointer"
                    on:click=on_toggle_open
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
                        "group"
                    </span>
                    <h2 class="text-sm font-semibold text-stone-700 dark:text-stone-300">
                        "Sharing"
                    </h2>
                    <Suspense fallback=|| ()>
                    {move || {
                        permissions.with(|r| r.as_ref().and_then(|res| match res {
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
                {move || (open.get() && is_owner).then(|| view! {
                    <button
                        class="p-1.5 rounded-lg text-stone-400 hover:text-stone-600
                            dark:hover:text-stone-300 hover:bg-stone-100
                            dark:hover:bg-stone-800 transition-colors"
                        on:click=move |_| show_invite.update(|v| *v = !*v)
                        title=move || if show_invite.get() { "Cancel" } else { "Invite someone" }
                    >
                        <span class="material-symbols-outlined" style="font-size: 16px;">
                            {move || if show_invite.get() { "close" } else { "person_add" }}
                        </span>
                    </button>
                })}
            </div>

            {move || open.get().then(|| view! {
            <div class="mt-4">
            // Invite form
            {move || show_invite.get().then(|| view! {
                <div class="mb-4 p-3 bg-stone-50 dark:bg-stone-900 rounded-lg space-y-2">
                    <div class="flex gap-2">
                        <input
                            type="email"
                            class="flex-1 px-2 py-1 text-xs rounded border border-stone-300 dark:border-stone-600
                                bg-transparent text-stone-900 dark:text-stone-100 focus:outline-none
                                focus:ring-1 focus:ring-amber-500"
                            placeholder="Email address\u{2026}"
                            prop:value=move || email_input.get()
                            on:input=move |ev| email_input.set(event_target_value(&ev))
                        />
                        <select
                            class="px-2 py-1 text-xs rounded border border-stone-300 dark:border-stone-600
                                bg-stone-50 dark:bg-stone-800 text-stone-700 dark:text-stone-300
                                focus:outline-none"
                            prop:value=move || role_input.get()
                            on:change=move |ev| role_input.set(event_target_value(&ev))
                        >
                            <option value="viewer">"Viewer"</option>
                            <option value="editor">"Editor"</option>
                            <option value="owner">"Owner"</option>
                        </select>
                    </div>
                    <div class="flex items-center gap-2">
                        <button
                            class="p-1.5 rounded-lg text-stone-400 hover:text-stone-600
                                dark:hover:text-stone-300 hover:bg-stone-100
                                dark:hover:bg-stone-800 transition-colors disabled:opacity-30"
                            on:click=on_invite
                            disabled=move || saving.get()
                            title=move || if saving.get() { "Sending\u{2026}" } else { "Send invite" }
                        >
                            <span class="material-symbols-outlined" style="font-size: 16px;">
                                {move || if saving.get() { "hourglass_empty" } else { "send" }}
                            </span>
                        </button>
                        <span class="text-[10px] text-stone-400 dark:text-stone-500">
                            "New users receive a Cognito welcome email."
                        </span>
                    </div>
                    {move || error_msg.get().map(|msg| view! {
                        <div class="text-xs text-red-500">{msg}</div>
                    })}
                </div>
            })}

            // Permission list
            <Suspense fallback=|| view! {
                <div class="text-xs text-stone-400">"Loading..."</div>
            }>
                {move || {
                    permissions.get().map(|result| {
                        match result {
                            Ok(list) if list.is_empty() => view! {
                                <div class="flex flex-col items-center gap-2 py-6">
                                    <span
                                        class="material-symbols-outlined text-stone-300 dark:text-stone-700"
                                        style="font-size: 32px;"
                                    >
                                        "lock"
                                    </span>
                                    <p class="text-xs text-stone-400 dark:text-stone-600">
                                        "Only you have access."
                                    </p>
                                </div>
                            }.into_any(),
                            Ok(list) => view! {
                                <div class="space-y-1">
                                    {list.into_iter().map(|perm| {
                                        let perm_id = perm.id;
                                        let subject = perm.subject_id.clone();
                                        let current_role = RwSignal::new(match perm.role {
                                            PermissionRole::Owner  => "owner",
                                            PermissionRole::Editor => "editor",
                                            PermissionRole::Viewer => "viewer",
                                        }.to_string());
                                        let updating = RwSignal::new(false);

                                        // Resolve display name from cached users list.
                                        let display_subject = {
                                            let sub = subject.clone();
                                            move || {
                                                if let Some(users) = admin_users.get()
                                                    && let Some(u) = users.iter().find(|u| u.id == sub) {
                                                        let name = u.display_name();
                                                        let email = u.email.as_deref().unwrap_or("");
                                                        if email.is_empty() {
                                                            return name;
                                                        }
                                                        return format!("{name} <{email}>");
                                                }
                                                sub.clone()
                                            }
                                        };

                                        let role_color = move || match current_role.get().as_str() {
                                            "owner"  => "bg-purple-100 text-purple-700 dark:bg-purple-900/40 dark:text-purple-300",
                                            "editor" => "bg-amber-100 text-amber-700 dark:bg-amber-900/40 dark:text-amber-300",
                                            _        => "bg-stone-100 text-stone-600 dark:bg-stone-800 dark:text-stone-400",
                                        };

                                        let on_role_change = move |ev: leptos::ev::Event| {
                                            let new_role_str = event_target_value(&ev);
                                            let new_role = match new_role_str.as_str() {
                                                "owner"  => PermissionRole::Owner,
                                                "editor" => PermissionRole::Editor,
                                                _        => PermissionRole::Viewer,
                                            };
                                            current_role.set(new_role_str);
                                            updating.set(true);
                                            let req = UpdatePermissionRequest { role: new_role };
                                            wasm_bindgen_futures::spawn_local(async move {
                                                let _ = api::update_permission(perm_id, &req).await;
                                                updating.set(false);
                                                refresh.update(|n| *n += 1);
                                            });
                                        };

                                        view! {
                                            <div class="flex items-center justify-between py-1.5 px-2 rounded
                                                hover:bg-stone-50 dark:hover:bg-stone-800/50 group">
                                                <div class="flex items-center gap-2 min-w-0">
                                                    <span class="material-symbols-outlined text-stone-400 dark:text-stone-600 text-[16px] shrink-0">
                                                        "person"
                                                    </span>
                                                    <span class="text-xs text-stone-700 dark:text-stone-300 truncate max-w-[160px]"
                                                        title={subject.clone()}>
                                                        {display_subject}
                                                    </span>
                                                </div>
                                                <div class="flex items-center gap-1.5 shrink-0">
                                                    {move || if is_owner && updating.get() {
                                                        view! {
                                                            <span class={format!("px-1.5 py-0.5 text-[10px] rounded-full font-medium {}", role_color())}>
                                                                "saving\u{2026}"
                                                            </span>
                                                        }.into_any()
                                                    } else if is_owner {
                                                        view! {
                                                            <select
                                                                class=move || format!(
                                                                    "text-[10px] rounded-full font-medium px-1.5 py-0.5 border-0 \
                                                                     focus:outline-none focus:ring-1 focus:ring-amber-400 \
                                                                     cursor-pointer transition-colors {}",
                                                                    role_color()
                                                                )
                                                                prop:value=move || current_role.get()
                                                                on:change=on_role_change
                                                            >
                                                                <option value="viewer">"viewer"</option>
                                                                <option value="editor">"editor"</option>
                                                                <option value="owner">"owner"</option>
                                                            </select>
                                                        }.into_any()
                                                    } else {
                                                        // Read-only role badge for non-owners.
                                                        view! {
                                                            <span class={format!("px-1.5 py-0.5 text-[10px] rounded-full font-medium {}", role_color())}>
                                                                {current_role.get_untracked()}
                                                            </span>
                                                        }.into_any()
                                                    }}
                                                    // Revoke button — always visible with muted colour (Tailwind v4
                                                    // group-hover opacity is unreliable). Hidden for non-owners.
                                                    {is_owner.then(|| view! {
                                                        <button
                                                            class="text-stone-300 hover:text-red-500
                                                                dark:text-stone-600 dark:hover:text-red-400
                                                                text-xs transition-colors shrink-0 px-1"
                                                            title="Revoke access"
                                                            on:click=move |_| {
                                                                wasm_bindgen_futures::spawn_local(async move {
                                                                    let _ = api::revoke_permission(node_id, perm_id).await;
                                                                    refresh.update(|n| *n += 1);
                                                                });
                                                            }
                                                        >
                                                            "\u{00d7}"
                                                        </button>
                                                    })}
                                                </div>
                                            </div>
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
