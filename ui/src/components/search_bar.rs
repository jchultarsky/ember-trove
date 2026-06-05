/// Compact search bar in the sidebar header.
///
/// Writes to the shared `search_query` context signal so SearchView can read
/// the same value without a duplicate input. When already on the Search view
/// the live-results dropdown is suppressed — SearchView shows the full results.
/// Pressing Enter (or clicking "View all") navigates to the Search view.
use common::search::SearchResult;
use leptos::prelude::*;
use leptos_router::hooks::{use_location, use_navigate};

#[component]
pub fn SearchBar() -> impl IntoView {
    let navigate = use_navigate();
    let location = use_location();
    // Shared query — also read by SearchView.
    let search_query =
        use_context::<RwSignal<String>>().expect("search_query signal must be provided");

    let results: RwSignal<Vec<SearchResult>> = RwSignal::new(vec![]);
    let show_dropdown = RwSignal::new(false);
    let loading = RwSignal::new(false);
    let debounce_version = RwSignal::new(0u32);

    // Debounced search: fires 300 ms after last keystroke.
    // Suppressed when already on the Search view.
    let trigger_search = move || {
        debounce_version.update(|v| *v += 1);
        let version = debounce_version.get_untracked();
        let q = search_query.get_untracked().trim().to_string();

        if q.len() < 2 || location.pathname.get_untracked() == "/search" {
            results.set(vec![]);
            show_dropdown.set(false);
            loading.set(false);
            return;
        }

        loading.set(true);
        wasm_bindgen_futures::spawn_local(async move {
            gloo_timers::future::TimeoutFuture::new(300).await;
            if debounce_version.get_untracked() != version {
                return;
            }
            // Re-check: user might have navigated to Search view during debounce.
            if location.pathname.get_untracked() == "/search" {
                loading.set(false);
                show_dropdown.set(false);
                return;
            }
            match crate::api::search_nodes(&q, false, None, None, &[], "or", None, None, None, 1, 6)
                .await
            {
                Ok(resp) => {
                    if debounce_version.get_untracked() == version {
                        results.set(resp.results);
                        show_dropdown.set(true);
                    }
                }
                Err(_) => {
                    results.set(vec![]);
                }
            }
            loading.set(false);
        });
    };

    let on_input = move |ev: web_sys::Event| {
        search_query.set(event_target_value(&ev));
        trigger_search();
    };

    let on_keydown = {
        let navigate = navigate.clone();
        move |ev: web_sys::KeyboardEvent| {
            if ev.key() == "Enter" {
                ev.prevent_default();
                show_dropdown.set(false);
                navigate("/search", Default::default());
            } else if ev.key() == "Escape" {
                show_dropdown.set(false);
            }
        }
    };

    let on_blur = move |_| {
        wasm_bindgen_futures::spawn_local(async move {
            gloo_timers::future::TimeoutFuture::new(200).await;
            show_dropdown.set(false);
        });
    };

    let on_focus = move |_| {
        if location.pathname.get_untracked() != "/search" && !results.get_untracked().is_empty() {
            show_dropdown.set(true);
        }
    };

    view! {
        <div class="relative">
            <input
                type="search"
                class="w-full px-4 py-2 pl-10 text-sm bg-stone-100 dark:bg-stone-800
                    border border-transparent rounded-lg focus:outline-none
                    focus:ring-2 focus:ring-amber-500 dark:text-stone-100"
                placeholder="Search\u{2026} (Enter)"
                prop:value=move || search_query.get()
                on:input=on_input
                on:keydown=on_keydown
                on:blur=on_blur
                on:focus=on_focus
            />
            <span class="material-symbols-outlined absolute left-3 top-1/2 -translate-y-1/2
                text-stone-400 pointer-events-none text-lg">
                "search"
            </span>
            {move || loading.get().then_some(view! {
                <span class="absolute right-3 top-1/2 -translate-y-1/2
                    text-stone-400 text-xs animate-pulse">
                    "\u{2026}"
                </span>
            })}

            // Dropdown — only when NOT on Search view
            {move || {
                let visible = show_dropdown.get();
                let on_search_view = location.pathname.get() == "/search";
                let items = results.get();
                if !visible || on_search_view || items.is_empty() {
                    return None;
                }
                Some(view! {
                    <div class="absolute z-50 mt-1 w-full bg-white dark:bg-stone-900 border
                        border-stone-200 dark:border-stone-700 rounded-lg shadow-lg overflow-hidden">
                        {items.into_iter().map(|r| {
                            let node_id = r.node_id;
                            let navigate = navigate.clone();
                            view! {
                                <button
                                    class="w-full text-left px-3 py-2 text-sm hover:bg-stone-50
                                        dark:hover:bg-stone-800 transition-colors border-b
                                        border-stone-100 dark:border-stone-800 last:border-0"
                                    on:mousedown=move |ev| {
                                        ev.prevent_default();
                                        show_dropdown.set(false);
                                        navigate(&format!("/nodes/{node_id}"), Default::default());
                                    }
                                >
                                    <div class="font-medium text-stone-900 dark:text-stone-100 truncate">
                                        {r.title}
                                    </div>
                                    <div class="text-xs text-stone-400 truncate">{r.slug}</div>
                                </button>
                            }
                        }).collect::<Vec<_>>()}
                        <button
                            class="w-full text-center px-3 py-2 text-xs text-amber-600
                                dark:text-amber-400 hover:bg-stone-50 dark:hover:bg-stone-800
                                transition-colors font-medium"
                            on:mousedown={
                                let navigate = navigate.clone();
                                move |ev| {
                                    ev.prevent_default();
                                    show_dropdown.set(false);
                                    navigate("/search", Default::default());
                                }
                            }
                        >
                            "View all results \u{2192}"
                        </button>
                    </div>
                })
            }}
        </div>
    }
}
