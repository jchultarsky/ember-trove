use common::{favorite::CreateFavoriteRequest, id::NodeId};
use leptos::prelude::*;

use wasm_bindgen_futures::spawn_local;

use super::ModalShell;
use crate::{
    api::create_favorite,
    components::toast::{ToastLevel, push_toast},
};

/// Modal for adding a new sidebar favorite.
/// Supports two modes: pinning an internal node (search-picker) or bookmarking an external URL.
#[component]
pub fn AddFavoriteModal(
    #[prop(into)] show: Signal<bool>,
    on_close: Callback<()>,
    on_added: Callback<common::favorite::Favorite>,
) -> impl IntoView {
    // "node" | "url"
    let mode = RwSignal::new("node".to_string());
    // Node mode: the shared picker (#130) — it used to download every title
    // and filter them here, with its own list and keyboard behaviour.
    let selected_node: RwSignal<Option<(NodeId, String)>> = RwSignal::new(None);
    // URL mode
    let url_input = RwSignal::new(String::new());
    let label_input = RwSignal::new(String::new());
    // State
    let loading = RwSignal::new(false);
    let error: RwSignal<Option<String>> = RwSignal::new(None);

    // Reset fields on open.
    Effect::new(move |_| {
        if show.get() {
            mode.set("node".to_string());
            selected_node.set(None);
            url_input.set(String::new());
            label_input.set(String::new());
            loading.set(false);
            error.set(None);
        }
    });

    let handle_submit = move || {
        let m = mode.get_untracked();
        let req = if m == "node" {
            let Some((node_id, title)) = selected_node.get_untracked() else {
                error.set(Some("Select a node first.".to_string()));
                return;
            };
            CreateFavoriteRequest {
                node_id: Some(node_id.0),
                url: None,
                label: title,
            }
        } else {
            let u = url_input.get_untracked();
            let l = label_input.get_untracked();
            if u.trim().is_empty() {
                error.set(Some("URL is required.".to_string()));
                return;
            }
            if l.trim().is_empty() {
                error.set(Some("Label is required.".to_string()));
                return;
            }
            CreateFavoriteRequest {
                node_id: None,
                url: Some(u.trim().to_string()),
                label: l.trim().to_string(),
            }
        };

        loading.set(true);
        error.set(None);
        spawn_local(async move {
            match create_favorite(&req).await {
                Ok(fav) => {
                    loading.set(false);
                    push_toast(
                        ToastLevel::Success,
                        format!("\"{}\" added to Favorites.", fav.label),
                    );
                    on_added.run(fav);
                    on_close.run(());
                }
                Err(e) => {
                    loading.set(false);
                    error.set(Some(e.to_string()));
                }
            }
        });
    };

    let handle_keydown = move |ev: web_sys::KeyboardEvent| {
        if ev.key() == "Enter" && (ev.ctrl_key() || ev.meta_key()) {
            ev.prevent_default();
            handle_submit();
        }
    };

    view! {
        <ModalShell
            show=show
            on_close=on_close
            label="Add to favorites"
            panel_class="w-full max-w-md mx-4 p-6"
            on_key=Callback::new(handle_keydown)
        >
            // Header
            <div class="flex items-center justify-between mb-4">
                <h2 class="text-base font-semibold text-stone-800 dark:text-stone-100">
                    "Add to Favorites"
                </h2>
                <button
                    class="text-stone-400 hover:text-stone-600 dark:hover:text-stone-300 cursor-pointer"
                    on:click=move |_| on_close.run(())
                >
                    <span class="material-symbols-outlined">"close"</span>
                </button>
            </div>

            // Mode toggle
            <div class="flex gap-2 mb-4 p-1 bg-stone-100 dark:bg-stone-800 rounded-lg">
                <button
                    class=move || {
                        let active = mode.get() == "node";
                        let base = "flex-1 py-1.5 rounded-md text-sm font-medium transition-colors cursor-pointer";
                        if active {
                            format!("{base} bg-white dark:bg-stone-700 text-stone-800 dark:text-stone-100 shadow-sm")
                        } else {
                            format!("{base} text-stone-500 dark:text-stone-400 hover:text-stone-700 dark:hover:text-stone-300")
                        }
                    }
                    on:click=move |_| { mode.set("node".to_string()); selected_node.set(None); }
                >
                    "Internal Node"
                </button>
                <button
                    class=move || {
                        let active = mode.get() == "url";
                        let base = "flex-1 py-1.5 rounded-md text-sm font-medium transition-colors cursor-pointer";
                        if active {
                            format!("{base} bg-white dark:bg-stone-700 text-stone-800 dark:text-stone-100 shadow-sm")
                        } else {
                            format!("{base} text-stone-500 dark:text-stone-400 hover:text-stone-700 dark:hover:text-stone-300")
                        }
                    }
                    on:click=move |_| mode.set("url".to_string())
                >
                    "External URL"
                </button>
            </div>

            // Node mode
            {move || {
                if mode.get() != "node" { return view! { <div /> }.into_any(); }
                view! {
                    <div class="space-y-3">
                        <crate::components::node_picker::NodePicker
                            selected=selected_node
                            placeholder="Search nodes…"
                            autofocus=true
                        />
                    </div>
                }.into_any()
            }}

            // URL mode
            {move || {
                if mode.get() != "url" { return view! { <div /> }.into_any(); }
                view! {
                    <div class="space-y-3">
                        <div>
                            <label class="block text-xs font-medium text-stone-600 dark:text-stone-400 mb-1">
                                "URL"
                            </label>
                            <input
                                type="url"
                                class="w-full px-3 py-2 text-sm border border-stone-300 dark:border-stone-600
                                       rounded-lg bg-white dark:bg-stone-800 text-stone-800 dark:text-stone-100
                                       placeholder-stone-400 focus:outline-none focus:ring-2 focus:ring-amber-400"
                                placeholder="https://…"
                                prop:value=move || url_input.get()
                                on:input=move |ev| url_input.set(event_target_value(&ev))
                            />
                        </div>
                        <div>
                            <label class="block text-xs font-medium text-stone-600 dark:text-stone-400 mb-1">
                                "Label"
                            </label>
                            <input
                                type="text"
                                class="w-full px-3 py-2 text-sm border border-stone-300 dark:border-stone-600
                                       rounded-lg bg-white dark:bg-stone-800 text-stone-800 dark:text-stone-100
                                       placeholder-stone-400 focus:outline-none focus:ring-2 focus:ring-amber-400"
                                placeholder="My Mailbox"
                                prop:value=move || label_input.get()
                                on:input=move |ev| label_input.set(event_target_value(&ev))
                            />
                        </div>
                    </div>
                }.into_any()
            }}

            // Error banner
            {move || error.get().map(|e| view! {
                <p class="mt-3 text-xs text-red-500 dark:text-red-400">{e}</p>
            })}

            // Actions
            <div class="flex justify-end gap-3 mt-5">
                <button
                    class="px-4 py-2 text-sm text-stone-600 dark:text-stone-400
                           hover:text-stone-800 dark:hover:text-stone-200 cursor-pointer"
                    on:click=move |_| on_close.run(())
                >
                    "Cancel"
                </button>
                <button
                    class="px-4 py-2 text-sm font-medium bg-ember hover:bg-ember-strong
                           text-white rounded-lg disabled:opacity-50 cursor-pointer"
                    disabled=move || {
                        loading.get()
                            || (mode.get() == "node" && selected_node.with(|n| n.is_none()))
                    }
                    on:click=move |_| handle_submit()
                >
                    {move || if loading.get() { "Adding…" } else { "Add" }}
                </button>
            </div>
        </ModalShell>
    }
}
