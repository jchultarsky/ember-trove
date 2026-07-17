//! Webhooks management — `/webhooks`.
//!
//! Surfaces the (previously headless) webhooks backend: list, create, edit,
//! toggle, and delete outgoing webhooks. The server masks stored secrets, so
//! the edit form uses PATCH semantics for the secret field: blank = keep,
//! "clear" checkbox = remove, new value = replace (see
//! `common::webhook::UpdateWebhookRequest`).

use common::webhook::{CreateWebhookRequest, UpdateWebhookRequest, Webhook, available_events};
use leptos::prelude::*;
use wasm_bindgen_futures::spawn_local;

use crate::components::modals::delete_confirm::DeleteConfirmModal;
use crate::components::toast::{ToastLevel, push_toast};

/// Short human label for a canonical event name ("node.created" → "Node created").
fn event_label(event: &str) -> String {
    let mut s = event.replace('.', " ");
    if let Some(first) = s.get_mut(0..1) {
        first.make_ascii_uppercase();
    }
    s
}

#[component]
pub fn WebhooksView() -> impl IntoView {
    let refresh = RwSignal::new(0u32);
    let webhooks = LocalResource::new(move || {
        let _ = refresh.get();
        async move { crate::api::list_webhooks().await.unwrap_or_default() }
    });

    // editing: None = list only; Some(None) = create form; Some(Some(hook)) = edit form.
    let editing: RwSignal<Option<Option<Webhook>>> = RwSignal::new(None);
    let form_url = RwSignal::new(String::new());
    let form_secret = RwSignal::new(String::new());
    let form_clear_secret = RwSignal::new(false);
    let form_events: RwSignal<Vec<String>> = RwSignal::new(vec![]);
    let saving = RwSignal::new(false);
    let delete_confirm: RwSignal<Option<Webhook>> = RwSignal::new(None);

    let open_create = move |_| {
        form_url.set(String::new());
        form_secret.set(String::new());
        form_clear_secret.set(false);
        form_events.set(
            available_events()
                .iter()
                .map(|e| (*e).to_string())
                .collect(),
        );
        editing.set(Some(None));
    };

    let submit = move || {
        let url = form_url.get_untracked().trim().to_string();
        if url.is_empty() {
            push_toast(ToastLevel::Error, "Webhook URL is required.");
            return;
        }
        let events = form_events.get_untracked();
        if events.is_empty() {
            push_toast(ToastLevel::Error, "Select at least one event.");
            return;
        }
        let secret_input = form_secret.get_untracked().trim().to_string();
        saving.set(true);
        match editing.get_untracked() {
            Some(Some(hook)) => {
                let req = UpdateWebhookRequest {
                    url,
                    // Blank field keeps the stored secret; the explicit
                    // checkbox clears it; a new value replaces it.
                    secret: if form_clear_secret.get_untracked() {
                        Some(None)
                    } else if secret_input.is_empty() {
                        None
                    } else {
                        Some(Some(secret_input))
                    },
                    events,
                    is_active: hook.is_active,
                };
                spawn_local(async move {
                    match crate::api::update_webhook(hook.id.0, &req).await {
                        Ok(_) => {
                            push_toast(ToastLevel::Success, "Webhook updated.");
                            editing.set(None);
                            refresh.update(|n| *n += 1);
                        }
                        Err(e) => push_toast(ToastLevel::Error, format!("Update failed: {e}")),
                    }
                    saving.set(false);
                });
            }
            Some(None) => {
                let req = CreateWebhookRequest {
                    url,
                    secret: (!secret_input.is_empty()).then_some(secret_input),
                    events,
                };
                spawn_local(async move {
                    match crate::api::create_webhook(&req).await {
                        Ok(_) => {
                            push_toast(ToastLevel::Success, "Webhook created.");
                            editing.set(None);
                            refresh.update(|n| *n += 1);
                        }
                        Err(e) => push_toast(ToastLevel::Error, format!("Create failed: {e}")),
                    }
                    saving.set(false);
                });
            }
            None => saving.set(false),
        }
    };

    view! {
        <div class="max-w-3xl mx-auto p-6">
            <div class="flex items-center justify-between mb-1">
                <h1 class="text-2xl font-bold text-stone-900 dark:text-stone-100">"Webhooks"</h1>
                <button
                    class="px-3 py-1.5 text-sm font-medium rounded-lg bg-amber-600
                           hover:bg-amber-700 text-white transition-colors cursor-pointer"
                    on:click=open_create
                >
                    "New Webhook"
                </button>
            </div>
            <p class="text-sm text-stone-500 dark:text-stone-400 mb-6">
                "POST notifications to your endpoints when nodes or tasks change. \
                 Payloads are HMAC-signed when a secret is set (X-Webhook-Signature)."
            </p>

            // ── Create / edit form ───────────────────────────────────────────
            {move || editing.get().map(|hook| {
                let is_edit = hook.is_some();
                let has_secret = hook.as_ref().is_some_and(|h| h.secret.is_some());
                view! {
                    <div class="mb-6 p-4 rounded-xl border border-stone-200 dark:border-stone-700
                                bg-white dark:bg-stone-900 flex flex-col gap-3"
                         data-testid="webhook-form">
                        <h2 class="text-sm font-semibold text-stone-900 dark:text-stone-100">
                            {if is_edit { "Edit Webhook" } else { "New Webhook" }}
                        </h2>
                        <div class="flex flex-col gap-1">
                            <label class="text-xs uppercase tracking-wide font-medium
                                          text-stone-500 dark:text-stone-400" for="webhook-url">
                                "Endpoint URL (HTTPS)"
                            </label>
                            <input
                                id="webhook-url"
                                type="url"
                                placeholder="https://example.com/hooks/ember-trove"
                                class="w-full px-3 py-1.5 rounded-lg text-sm
                                       bg-stone-50 dark:bg-stone-800
                                       border border-stone-200 dark:border-stone-700
                                       text-stone-900 dark:text-stone-100 placeholder-stone-400
                                       focus:outline-none focus:ring-2 focus:ring-amber-500"
                                prop:value=move || form_url.get()
                                on:input=move |ev| form_url.set(event_target_value(&ev))
                            />
                        </div>
                        <div class="flex flex-col gap-1">
                            <span class="text-xs uppercase tracking-wide font-medium
                                         text-stone-500 dark:text-stone-400">"Events"</span>
                            <div class="flex flex-wrap gap-2">
                                {available_events().iter().map(|ev_name| {
                                    let name = (*ev_name).to_string();
                                    let toggle_name = name.clone();
                                    let checked = move || form_events.get().contains(&name);
                                    view! {
                                        <label class="flex items-center gap-1.5 text-sm px-2 py-1 rounded-lg
                                                      border border-stone-200 dark:border-stone-700
                                                      text-stone-700 dark:text-stone-300 cursor-pointer
                                                      hover:bg-stone-50 dark:hover:bg-stone-800">
                                            <input
                                                type="checkbox"
                                                prop:checked=checked
                                                on:change=move |_| {
                                                    let n = toggle_name.clone();
                                                    form_events.update(|evs| {
                                                        if evs.contains(&n) {
                                                            evs.retain(|e| e != &n);
                                                        } else {
                                                            evs.push(n);
                                                        }
                                                    });
                                                }
                                            />
                                            {event_label(ev_name)}
                                        </label>
                                    }
                                }).collect_view()}
                            </div>
                        </div>
                        <div class="flex flex-col gap-1">
                            <label class="text-xs uppercase tracking-wide font-medium
                                          text-stone-500 dark:text-stone-400" for="webhook-secret">
                                {if is_edit { "Secret (blank = keep current)" } else { "Secret (optional)" }}
                            </label>
                            <input
                                id="webhook-secret"
                                type="password"
                                autocomplete="off"
                                placeholder={if is_edit { "unchanged" } else { "used to HMAC-sign payloads" }}
                                class="w-full px-3 py-1.5 rounded-lg text-sm
                                       bg-stone-50 dark:bg-stone-800
                                       border border-stone-200 dark:border-stone-700
                                       text-stone-900 dark:text-stone-100 placeholder-stone-400
                                       focus:outline-none focus:ring-2 focus:ring-amber-500"
                                prop:value=move || form_secret.get()
                                on:input=move |ev| form_secret.set(event_target_value(&ev))
                            />
                            {(is_edit && has_secret).then(|| view! {
                                <label class="flex items-center gap-1.5 text-xs
                                              text-stone-500 dark:text-stone-400 cursor-pointer">
                                    <input
                                        type="checkbox"
                                        prop:checked=move || form_clear_secret.get()
                                        on:change=move |_| form_clear_secret.update(|v| *v = !*v)
                                    />
                                    "Remove the stored secret (deliveries become unsigned)"
                                </label>
                            })}
                        </div>
                        <div class="flex gap-2 justify-end pt-1">
                            <button
                                class="px-3 py-1.5 text-sm rounded-lg text-stone-600 dark:text-stone-400
                                       hover:bg-stone-100 dark:hover:bg-stone-800 transition-colors cursor-pointer"
                                on:click=move |_| editing.set(None)
                            >
                                "Cancel"
                            </button>
                            <button
                                class="px-3 py-1.5 text-sm font-medium rounded-lg bg-amber-600
                                       hover:bg-amber-700 text-white transition-colors cursor-pointer
                                       disabled:opacity-50 disabled:cursor-not-allowed"
                                disabled=move || saving.get()
                                on:click=move |_| submit()
                            >
                                {move || if saving.get() { "Saving…" } else { "Save" }}
                            </button>
                        </div>
                    </div>
                }
            })}

            // ── Webhook list ─────────────────────────────────────────────────
            <Suspense fallback=|| view! { <p class="text-sm text-stone-400">"Loading…"</p> }>
                {move || webhooks.get().map(|hooks| {
                    if hooks.is_empty() && editing.get().is_none() {
                        view! {
                            <p class="text-sm text-stone-500 dark:text-stone-400"
                               data-testid="webhooks-empty">
                                "No webhooks yet. Create one to get notified when your nodes and tasks change."
                            </p>
                        }.into_any()
                    } else {
                        view! {
                            <ul class="flex flex-col gap-2">
                                {hooks.into_iter().map(|hook| {
                                    let edit_hook = hook.clone();
                                    let toggle_hook = hook.clone();
                                    let confirm_hook = hook.clone();
                                    let signed = hook.secret.is_some();
                                    view! {
                                        <li class="p-3 rounded-xl border border-stone-200 dark:border-stone-700
                                                   bg-white dark:bg-stone-900 flex items-center gap-3"
                                            data-webhook-id=hook.id.0.to_string()>
                                            <div class="flex-1 min-w-0">
                                                <p class="text-sm font-medium text-stone-900 dark:text-stone-100 truncate">
                                                    {hook.url.clone()}
                                                </p>
                                                <p class="text-xs text-stone-500 dark:text-stone-400 truncate">
                                                    {hook.events.iter().map(|e| event_label(e))
                                                        .collect::<Vec<_>>().join(" · ")}
                                                    {signed.then_some(" · signed")}
                                                </p>
                                            </div>
                                            <button
                                                class=move || if toggle_hook.is_active {
                                                    "px-2 py-0.5 text-xs font-medium rounded-full cursor-pointer \
                                                     bg-green-100 text-green-700 dark:bg-green-900/40 dark:text-green-300"
                                                } else {
                                                    "px-2 py-0.5 text-xs font-medium rounded-full cursor-pointer \
                                                     bg-stone-100 text-stone-500 dark:bg-stone-800 dark:text-stone-400"
                                                }
                                                title="Toggle active"
                                                on:click={
                                                    let h = hook.clone();
                                                    move |_| {
                                                        let req = UpdateWebhookRequest {
                                                            url: h.url.clone(),
                                                            secret: None, // absent → keep stored secret
                                                            events: h.events.clone(),
                                                            is_active: !h.is_active,
                                                        };
                                                        let id = h.id.0;
                                                        spawn_local(async move {
                                                            match crate::api::update_webhook(id, &req).await {
                                                                Ok(_) => refresh.update(|n| *n += 1),
                                                                Err(e) => push_toast(
                                                                    ToastLevel::Error,
                                                                    format!("Toggle failed: {e}"),
                                                                ),
                                                            }
                                                        });
                                                    }
                                                }
                                            >
                                                {if hook.is_active { "Active" } else { "Paused" }}
                                            </button>
                                            <button
                                                class="text-sm text-stone-500 dark:text-stone-400 cursor-pointer
                                                       hover:text-stone-800 dark:hover:text-stone-200"
                                                on:click=move |_| {
                                                    let h = edit_hook.clone();
                                                    form_url.set(h.url.clone());
                                                    form_secret.set(String::new());
                                                    form_clear_secret.set(false);
                                                    form_events.set(h.events.clone());
                                                    editing.set(Some(Some(h)));
                                                }
                                            >
                                                "Edit"
                                            </button>
                                            <button
                                                class="text-sm text-red-500 cursor-pointer hover:text-red-700"
                                                on:click=move |_| delete_confirm.set(Some(confirm_hook.clone()))
                                            >
                                                "Delete"
                                            </button>
                                        </li>
                                    }
                                }).collect_view()}
                            </ul>
                        }.into_any()
                    }
                })}
            </Suspense>
        </div>

        <DeleteConfirmModal
            show=Signal::derive(move || delete_confirm.get().is_some())
            item_name=Signal::derive(move || {
                delete_confirm.get().map(|h| h.url).unwrap_or_default()
            })
            on_confirm=Callback::new(move |_| {
                let Some(hook) = delete_confirm.get_untracked() else { return };
                delete_confirm.set(None);
                spawn_local(async move {
                    match crate::api::delete_webhook(hook.id.0).await {
                        Ok(_) => {
                            push_toast(ToastLevel::Success, "Webhook deleted.");
                            refresh.update(|n| *n += 1);
                        }
                        Err(e) => push_toast(ToastLevel::Error, format!("Delete failed: {e}")),
                    }
                });
            })
            on_cancel=Callback::new(move |_| delete_confirm.set(None))
        />
    }
}
