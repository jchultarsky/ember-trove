//! Share-link panel — create, copy, and revoke public read-only links for a node.
use common::id::{NodeId, ShareTokenId};
use common::share_token::CreateShareTokenRequest;
use leptos::prelude::*;

use crate::api;

/// Derives the public share URL from the current window origin + the token UUID.
fn share_url(token: &uuid::Uuid) -> String {
    let origin = web_sys::window()
        .and_then(|w| w.location().origin().ok())
        .unwrap_or_else(|| "https://trove.chultarsky.me".to_string());
    format!("{origin}/share/{token}")
}

/// Copy `text` to the clipboard via the JS Clipboard API.
/// Uses `js_sys::eval` to avoid depending on additional web-sys features.
fn copy_to_clipboard(text: String) {
    let escaped = text.replace('\\', "\\\\").replace('\'', "\\'");
    let _ = js_sys::eval(&format!("navigator.clipboard.writeText('{escaped}')"));
}

#[component]
pub fn SharePanel(node_id: NodeId, is_owner: bool) -> impl IntoView {
    let refresh = RwSignal::new(0u32);
    let open = RwSignal::new(false);
    let creating = RwSignal::new(false);
    let error_msg = RwSignal::new(Option::<String>::None);
    let copied_id: RwSignal<Option<ShareTokenId>> = RwSignal::new(None);

    let tokens = LocalResource::new(move || {
        let _ = refresh.get();
        let is_open = open.get();
        async move {
            if !is_open {
                return Ok(vec![]);
            }
            api::list_share_tokens(node_id).await
        }
    });

    let on_create = move |_| {
        creating.set(true);
        error_msg.set(None);
        let req = CreateShareTokenRequest { expires_at: None };
        wasm_bindgen_futures::spawn_local(async move {
            match api::create_share_token(node_id, &req).await {
                Ok(_) => refresh.update(|n| *n += 1),
                Err(e) => error_msg.set(Some(format!("Failed to create link: {e}"))),
            }
            creating.set(false);
        });
    };

    view! {
        <div class="mt-4 border-t border-stone-200 dark:border-stone-700 pt-6">
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
                        "public"
                    </span>
                    <h2 class="text-sm font-semibold text-stone-700 dark:text-stone-300">
                        "Public Links"
                    </h2>
                    <Suspense fallback=|| ()>
                    {move || {
                        tokens.with(|r| r.as_ref().and_then(|res| match res {
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
                            dark:hover:bg-stone-800 transition-colors disabled:opacity-30"
                        on:click=on_create
                        disabled=move || creating.get()
                        title=move || if creating.get() { "Creating\u{2026}" } else { "New public link" }
                    >
                        <span class="material-symbols-outlined" style="font-size: 16px;">
                            {move || if creating.get() { "hourglass_empty" } else { "add_link" }}
                        </span>
                    </button>
                })}
            </div>

            {move || open.get().then(|| view! {
                <div class="mt-3 space-y-1">
                    {move || error_msg.get().map(|msg| view! {
                        <div class="text-xs text-red-500 mb-2">{msg}</div>
                    })}
                    <Suspense fallback=|| view! {
                        <div class="text-xs text-stone-400">"Loading\u{2026}"</div>
                    }>
                        {move || tokens.get().map(|result| match result {
                            Ok(list) if list.is_empty() => view! {
                                <div class="flex flex-col items-center gap-2 py-4">
                                    <span
                                        class="material-symbols-outlined text-stone-300 dark:text-stone-700"
                                        style="font-size: 28px;"
                                    >
                                        "link_off"
                                    </span>
                                    <p class="text-xs text-stone-400 dark:text-stone-600">
                                        "No public links yet."
                                    </p>
                                </div>
                            }.into_any(),
                            Ok(list) => view! {
                                <div class="space-y-1.5">
                                    {list.into_iter().map(|st| {
                                        let token_id = st.id;
                                        let url = share_url(&st.token);
                                        let url_display = {
                                            let u = url.clone();
                                            // Show only the path + token for brevity.
                                            u.splitn(3, '/').last()
                                                .map(|s| format!("\u{2026}/{s}"))
                                                .unwrap_or(u)
                                        };
                                        let url_copy = url.clone();
                                        view! {
                                            <div class="flex items-center gap-2 py-1.5 px-2 rounded
                                                hover:bg-stone-50 dark:hover:bg-stone-800/50 group">
                                                <span class="material-symbols-outlined text-stone-400 dark:text-stone-500"
                                                    style="font-size: 14px; flex-shrink: 0;">
                                                    "link"
                                                </span>
                                                <span class="text-[11px] text-stone-500 dark:text-stone-400
                                                    font-mono truncate flex-1 select-all"
                                                    title=url.clone()>
                                                    {url_display}
                                                </span>
                                                // Copy button
                                                <button
                                                    class=move || {
                                                        let active = copied_id.get() == Some(token_id);
                                                        if active {
                                                            "text-green-500 shrink-0 transition-colors"
                                                        } else {
                                                            "text-stone-300 hover:text-amber-500 dark:text-stone-600 \
                                                             dark:hover:text-amber-400 shrink-0 transition-colors"
                                                        }
                                                    }
                                                    title="Copy link"
                                                    on:click=move |_| {
                                                        copy_to_clipboard(url_copy.clone());
                                                        copied_id.set(Some(token_id));
                                                        // Clear the "copied" indicator after 2 s.
                                                        wasm_bindgen_futures::spawn_local(async move {
                                                            gloo_timers::future::TimeoutFuture::new(2000).await;
                                                            if copied_id.get_untracked() == Some(token_id) {
                                                                copied_id.set(None);
                                                            }
                                                        });
                                                    }
                                                >
                                                    <span class="material-symbols-outlined" style="font-size: 14px;">
                                                        {move || if copied_id.get() == Some(token_id) { "check" } else { "content_copy" }}
                                                    </span>
                                                </button>
                                                // Revoke button (owner only)
                                                {is_owner.then(|| view! {
                                                    <button
                                                        class="text-stone-300 hover:text-red-500
                                                            dark:text-stone-600 dark:hover:text-red-400
                                                            shrink-0 transition-colors text-xs px-0.5"
                                                        title="Revoke link"
                                                        on:click=move |_| {
                                                            wasm_bindgen_futures::spawn_local(async move {
                                                                let _ = api::revoke_share_token(node_id, token_id).await;
                                                                refresh.update(|n| *n += 1);
                                                            });
                                                        }
                                                    >
                                                        "\u{00d7}"
                                                    </button>
                                                })}
                                            </div>
                                        }
                                    }).collect::<Vec<_>>()}
                                </div>
                            }.into_any(),
                            Err(e) => view! {
                                <div class="text-xs text-red-500">{format!("Error: {e}")}</div>
                            }.into_any(),
                        })}
                    </Suspense>
                </div>
            })}
        </div>
    }
}
