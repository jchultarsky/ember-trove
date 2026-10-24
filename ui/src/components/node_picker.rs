//! Shared type-ahead node picker. Debounced node search with a results
//! dropdown; writes the chosen `(NodeId, title)` into the `selected` signal and
//! shows a removable chip once picked. Used by the new-task form and the Notes
//! compose box (replacing the old full-list `<select>`, which doesn't scale as
//! the node count grows).

use common::{id::NodeId, search::SearchResult};
use leptos::prelude::*;

/// Debounce for type-ahead node search.
const NODE_SEARCH_DEBOUNCE_MS: u32 = 300;

/// Type-ahead node search behind every node picker (this component, the
/// palette, Inbox attach, triage attach, the Links "add" picker): 300 ms
/// debounce, and only the latest query's response lands. Returns the results
/// signal; callers may clear it themselves.
///
/// The five hand-rolled copies drifted: three skipped the version bump on an
/// empty query, so clearing the box while a search was in flight let the old
/// results reappear, and the Links picker searched on every keystroke.
pub fn use_node_search(query: RwSignal<String>) -> RwSignal<Vec<SearchResult>> {
    let results = RwSignal::new(Vec::<SearchResult>::new());
    let version = RwSignal::new(0u32);
    Effect::new(move |_| {
        let q = query.get();
        version.update(|v| *v += 1);
        let mine = version.get_untracked();
        if q.trim().is_empty() {
            results.set(Vec::new());
            return;
        }
        wasm_bindgen_futures::spawn_local(async move {
            gloo_timers::future::TimeoutFuture::new(NODE_SEARCH_DEBOUNCE_MS).await;
            // `try_`: the picker may have unmounted during the wait.
            if version.try_get_untracked() != Some(mine) {
                return;
            }
            if let Ok(r) = crate::api::node_picker_search(&q).await
                && version.try_get_untracked() == Some(mine)
            {
                results.try_set(r);
            }
        });
    });
    results
}

#[component]
pub fn NodePicker(
    /// The chosen node `(id, title)`, or `None`. Owned by the caller.
    selected: RwSignal<Option<(NodeId, String)>>,
    #[prop(optional, into)] placeholder: Option<String>,
) -> impl IntoView {
    let placeholder = placeholder.unwrap_or_else(|| "Link to a node (optional)…".to_string());
    let query = RwSignal::new(String::new());
    let results = use_node_search(query);

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
                                text-stone-700 dark:text-stone-300 rounded-lg px-2 py-1 focus:outline-none"
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
                            let icon = crate::components::node_meta::type_icon(&res.node_type);
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
