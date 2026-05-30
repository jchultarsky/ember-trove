//! Shared type-ahead node picker. Debounced node search with a results
//! dropdown; writes the chosen `(NodeId, title)` into the `selected` signal and
//! shows a removable chip once picked. Used by the new-task form and the Notes
//! compose box (replacing the old full-list `<select>`, which doesn't scale as
//! the node count grows).

use common::{id::NodeId, search::SearchResult};
use leptos::prelude::*;

use crate::components::task_common::node_type_icon;

#[component]
pub fn NodePicker(
    /// The chosen node `(id, title)`, or `None`. Owned by the caller.
    selected: RwSignal<Option<(NodeId, String)>>,
    #[prop(optional, into)] placeholder: Option<String>,
) -> impl IntoView {
    let placeholder = placeholder.unwrap_or_else(|| "Link to a node (optional)…".to_string());
    let query = RwSignal::new(String::new());
    let results = RwSignal::<Vec<SearchResult>>::new(vec![]);
    let ver = RwSignal::new(0u32);

    // Debounced search (300ms), version-guarded against out-of-order responses.
    Effect::new(move |_| {
        let q = query.get();
        if q.trim().is_empty() {
            results.set(vec![]);
            return;
        }
        ver.update(|v| *v += 1);
        let myver = ver.get_untracked();
        wasm_bindgen_futures::spawn_local(async move {
            gloo_timers::future::TimeoutFuture::new(300).await;
            if ver.get_untracked() != myver {
                return;
            }
            if let Ok(r) = crate::api::node_picker_search(&q).await
                && ver.get_untracked() == myver
            {
                results.set(r);
            }
        });
    });

    view! {
        <div class="relative">
            {move || match selected.get() {
                Some((_, title)) => view! {
                    <div class="flex items-center gap-1.5 text-xs text-stone-600 dark:text-stone-300">
                        <span class="material-symbols-outlined text-stone-400" style="font-size: 14px;">"link"</span>
                        <span class="truncate">{title}</span>
                        <button
                            class="p-0.5 rounded text-stone-400 hover:text-red-500 dark:hover:text-red-400
                                transition-colors cursor-pointer"
                            title="Unlink node"
                            on:click=move |_| selected.set(None)
                        >
                            <span class="material-symbols-outlined" style="font-size: 14px;">"close"</span>
                        </button>
                    </div>
                }.into_any(),
                None => view! {
                    <div class="flex items-center gap-1.5">
                        <span class="material-symbols-outlined text-stone-400 flex-shrink-0" style="font-size: 14px;">"link"</span>
                        <input
                            type="text"
                            placeholder=placeholder.clone()
                            class="flex-1 min-w-0 text-xs bg-stone-100 dark:bg-stone-700
                                text-stone-700 dark:text-stone-300 rounded px-2 py-1 focus:outline-none"
                            prop:value=move || query.get()
                            on:input=move |ev| query.set(event_target_value(&ev))
                            on:keydown=move |ev: leptos::ev::KeyboardEvent| {
                                if ev.key() == "Escape" {
                                    query.set(String::new());
                                    results.set(vec![]);
                                }
                            }
                        />
                    </div>
                }.into_any(),
            }}
            {move || {
                let r = results.get();
                (selected.get().is_none() && !r.is_empty()).then(|| view! {
                    <div class="absolute z-10 mt-1 w-full bg-white dark:bg-stone-900
                        border border-stone-200 dark:border-stone-700 rounded-lg shadow-md
                        overflow-hidden max-h-56 overflow-y-auto">
                        {r.into_iter().map(|res| {
                            let nid = res.node_id;
                            let title_sel = res.title.clone();
                            let title_disp = res.title.clone();
                            let icon = node_type_icon(&res.node_type);
                            view! {
                                <button
                                    class="w-full flex items-center gap-2 px-3 py-2 text-xs
                                        text-stone-700 dark:text-stone-300 hover:bg-amber-50 dark:hover:bg-stone-800
                                        border-b border-stone-50 dark:border-stone-800 last:border-b-0
                                        transition-colors cursor-pointer"
                                    on:click=move |_| {
                                        selected.set(Some((nid, title_sel.clone())));
                                        query.set(String::new());
                                        results.set(vec![]);
                                    }
                                >
                                    <span class="material-symbols-outlined text-stone-400 flex-shrink-0"
                                        style="font-size: 14px;">{icon}</span>
                                    <span class="truncate text-left">{title_disp}</span>
                                </button>
                            }
                        }).collect_view()}
                    </div>
                })
            }}
        </div>
    }
}
