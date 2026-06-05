use common::{
    search::{CreateSearchPresetRequest, SearchPreset, SearchResponse},
    tag::Tag,
};
use leptos::prelude::*;

use crate::app::{storage_get, storage_set};
use crate::components::node_meta::{
    status_color, status_icon, status_label, type_icon, type_label,
};
use leptos_router::hooks::use_navigate;

/// Sanitize `ts_headline` output to allow only `<b>` tags (search highlights).
/// Prevents XSS from user-controlled node titles/bodies injected via `inner_html`.
fn sanitize_highlight(html: &str) -> String {
    let mut tags = std::collections::HashSet::new();
    tags.insert("b");
    ammonia::Builder::new().tags(tags).clean(html).to_string()
}

// ── Search history (localStorage) ────────────────────────────────────────────

const HISTORY_KEY: &str = "ember_trove_search_history";
const HISTORY_MAX: usize = 20;

fn load_history() -> Vec<String> {
    storage_get(HISTORY_KEY)
        .map(|raw| {
            raw.split('\n')
                .filter(|s| !s.is_empty())
                .map(String::from)
                .collect()
        })
        .unwrap_or_default()
}

fn save_history(items: &[String]) {
    storage_set(HISTORY_KEY, &items.join("\n"));
}

fn push_history(query: &str) {
    let q = query.trim().to_string();
    if q.is_empty() {
        return;
    }
    let mut items = load_history();
    items.retain(|s| s != &q);
    items.insert(0, q);
    items.truncate(HISTORY_MAX);
    save_history(&items);
}

fn remove_history(query: &str) {
    let mut items = load_history();
    items.retain(|s| s != query);
    save_history(&items);
}

fn clear_history() {
    save_history(&[]);
}

/// Full-page search results view.
///
/// Tag filtering is managed locally: `tag_filters` holds zero or more `Tag`
/// values; `tag_op_and` toggles between OR (default) and AND semantics.
/// The AND/OR toggle only appears once two or more tags are selected.
///
/// The query comes from the shared `search_query` context written by the
/// sidebar `SearchBar`.
#[component]
pub fn SearchView() -> impl IntoView {
    let navigate = StoredValue::new(use_navigate());
    let search_query =
        use_context::<RwSignal<String>>().expect("search_query signal must be provided");
    // Initialise from global single-tag context (set by NodeList chip clicks),
    // then manage locally — no reactive subscription to the global signal.
    let global_tag_filter =
        use_context::<RwSignal<Option<Tag>>>().expect("tag_filter signal must be provided");
    let init_tags: Vec<Tag> = global_tag_filter.get_untracked().into_iter().collect();

    let tag_filters: RwSignal<Vec<Tag>> = RwSignal::new(init_tags);
    // false = OR (default), true = AND
    let tag_op_and = RwSignal::new(false);

    let fuzzy = RwSignal::new(false);
    let published_only = RwSignal::new(false);
    let node_type_filter: RwSignal<Option<String>> = RwSignal::new(None);
    let sort = RwSignal::new("relevance".to_string());
    let updated_after = RwSignal::new(String::new());
    let updated_before = RwSignal::new(String::new());
    let page = RwSignal::new(1u32);
    let error_msg = RwSignal::new(Option::<String>::None);
    let results: RwSignal<Option<SearchResponse>> = RwSignal::new(None);
    let loading = RwSignal::new(false);
    let search_version = RwSignal::new(0u32);
    let history: RwSignal<Vec<String>> = RwSignal::new(load_history());

    // Fetch all tags once for the picker.
    let all_tags: LocalResource<Vec<Tag>> =
        LocalResource::new(|| async { crate::api::fetch_tags().await.unwrap_or_default() });

    // ── Search presets ────────────────────────────────────────────────────────
    let preset_refresh = RwSignal::new(0u32);
    let presets: LocalResource<Vec<SearchPreset>> = LocalResource::new(move || {
        let _ = preset_refresh.get();
        async move { crate::api::fetch_search_presets().await.unwrap_or_default() }
    });
    let show_preset_menu = RwSignal::new(false);
    let show_save_form = RwSignal::new(false);
    let preset_name: RwSignal<String> = RwSignal::new(String::new());

    let do_search = move || {
        let q = search_query.get_untracked().trim().to_string();
        loading.set(true);
        error_msg.set(None);
        let is_fuzzy = fuzzy.get_untracked();
        let status = if published_only.get_untracked() {
            Some("published")
        } else {
            None
        };
        let nt = node_type_filter.get_untracked();
        let sort_val = sort.get_untracked();
        let after_val = updated_after.get_untracked();
        let before_val = updated_before.get_untracked();
        let tag_ids: Vec<uuid::Uuid> = tag_filters.get_untracked().iter().map(|t| t.id.0).collect();
        let tag_op = if tag_op_and.get_untracked() {
            "and"
        } else {
            "or"
        };
        let current_page = page.get_untracked();
        search_version.update(|v| *v += 1);
        let version = search_version.get_untracked();

        wasm_bindgen_futures::spawn_local(async move {
            gloo_timers::future::TimeoutFuture::new(300).await;
            if search_version.get_untracked() != version {
                return;
            }
            let sort_str = if sort_val == "relevance" {
                None
            } else {
                Some(sort_val.as_str())
            };
            let after_str = if after_val.is_empty() {
                None
            } else {
                Some(after_val.as_str())
            };
            let before_str = if before_val.is_empty() {
                None
            } else {
                Some(before_val.as_str())
            };
            match crate::api::search_nodes(
                &q,
                is_fuzzy,
                status,
                nt.as_deref(),
                &tag_ids,
                tag_op,
                sort_str,
                after_str,
                before_str,
                current_page,
                20,
            )
            .await
            {
                Ok(resp) => {
                    if search_version.get_untracked() == version {
                        if resp.total > 0 {
                            push_history(&q);
                            history.set(load_history());
                        }
                        results.set(Some(resp));
                    }
                }
                Err(e) => {
                    if search_version.get_untracked() == version {
                        error_msg.set(Some(format!("{e}")));
                    }
                }
            }
            if search_version.get_untracked() == version {
                loading.set(false);
            }
        });
    };

    // Auto-search when query, options, or tag filters change.
    Effect::new(move |_| {
        let _q = search_query.get();
        let _f = fuzzy.get();
        let _p = published_only.get();
        let _nt = node_type_filter.get();
        let _s = sort.get();
        let _af = updated_after.get();
        let _bf = updated_before.get();
        let _t = tag_filters.get();
        let _o = tag_op_and.get();
        page.set(1);
        do_search();
    });

    let on_page_change = move |new_page: u32| {
        page.set(new_page);
        do_search();
    };

    view! {
        <div class="flex flex-col h-full">
            // Filter bar
            <div class="flex items-center gap-2 px-6 py-3 border-b border-stone-200 dark:border-stone-800
                bg-white dark:bg-stone-900 flex-wrap">
                <h1 class="text-lg font-semibold text-stone-900 dark:text-stone-100 shrink-0">"Search"</h1>

                // AND/OR toggle — only when 2+ tags selected
                {move || {
                    if tag_filters.get().len() >= 2 {
                        let label = if tag_op_and.get() { "AND" } else { "OR" };
                        Some(view! {
                            <button
                                class="px-2 py-0.5 text-xs font-semibold rounded border
                                    border-amber-400 text-amber-500 dark:text-amber-400
                                    hover:bg-amber-50 dark:hover:bg-amber-900/20 transition-colors shrink-0"
                                title="Toggle AND / OR between tags"
                                on:click=move |_| tag_op_and.update(|v| *v = !*v)
                            >
                                {label}
                            </button>
                        })
                    } else {
                        None
                    }
                }}

                // Active tag chips
                <Suspense fallback=|| ()>
                    {move || all_tags.get().map(|_tags| {
                        let chips = tag_filters.get();
                        chips.into_iter().map(|tag| {
                            let name = tag.name.clone();
                            let color = tag.color.clone();
                            let tag_id = tag.id;
                            view! {
                                <button
                                    class="flex items-center gap-1 px-2 py-0.5 text-xs rounded-full
                                        text-white hover:opacity-80 transition-opacity shrink-0"
                                    style=format!("background-color: {color}")
                                    on:click=move |_| {
                                        tag_filters.update(|v| v.retain(|t| t.id != tag_id));
                                    }
                                    title="Remove tag filter"
                                >
                                    {name}
                                    " \u{00d7}"
                                </button>
                            }
                        }).collect::<Vec<_>>()
                    })}
                </Suspense>

                // Custom tag picker — replaces native <select> which is unreliable in CSR/dark mode
                <TagPicker all_tags=all_tags tag_filters=tag_filters />

                // ── Saved presets picker ──────────────────────────────────────
                <div class="relative">
                    <button
                        class="flex items-center gap-1 px-2 py-0.5 text-xs rounded-md
                               border border-stone-300 dark:border-stone-600
                               bg-white dark:bg-stone-800
                               text-stone-500 dark:text-stone-400
                               hover:border-amber-400 dark:hover:border-amber-500
                               hover:text-stone-700 dark:hover:text-stone-200
                               transition-colors"
                        title="Saved search presets"
                        on:click=move |_| show_preset_menu.update(|v| *v = !*v)
                    >
                        <span class="material-symbols-outlined" style="font-size: 13px;">"bookmarks"</span>
                        "Presets"
                        <span class="material-symbols-outlined" style="font-size: 13px;">
                            {move || if show_preset_menu.get() { "expand_less" } else { "expand_more" }}
                        </span>
                    </button>

                    {move || show_preset_menu.get().then(|| {
                        view! {
                            // Click-away backdrop
                            <div
                                class="fixed inset-0 z-10"
                                on:click=move |_| {
                                    show_preset_menu.set(false);
                                    show_save_form.set(false);
                                }
                            />
                            // Dropdown panel
                            <div class="absolute left-0 top-full mt-1 z-20 w-64
                                bg-white dark:bg-stone-900 rounded-xl shadow-xl
                                border border-stone-200 dark:border-stone-700 overflow-hidden">

                                // ── Preset list ────────────────────────────
                                {move || {
                                    let ps = presets.get().unwrap_or_default();
                                    if ps.is_empty() {
                                        view! {
                                            <p class="px-3 py-2.5 text-xs
                                                text-stone-400 dark:text-stone-500 italic">
                                                "No presets saved"
                                            </p>
                                        }.into_any()
                                    } else {
                                        view! {
                                            <div>
                                                {ps.into_iter().map(|preset| {
                                                    let preset_id = preset.id;
                                                    let name = preset.name.clone();
                                                    let q = preset.query.clone();
                                                    let pf = preset.fuzzy;
                                                    let pp = preset.published_only;
                                                    let ptag_ids = preset.tag_ids.clone();
                                                    let ptag_op = preset.tag_op.clone();
                                                    view! {
                                                        <div class="flex items-center
                                                            hover:bg-stone-50 dark:hover:bg-stone-800">
                                                            // Load preset button (entire row minus × btn)
                                                            <button
                                                                class="flex-1 text-left px-3 py-2 text-xs
                                                                    text-stone-700 dark:text-stone-300"
                                                                on:click=move |_| {
                                                                    search_query.set(q.clone());
                                                                    fuzzy.set(pf);
                                                                    published_only.set(pp);
                                                                    tag_op_and.set(ptag_op == "and");
                                                                    let ids: Vec<String> = ptag_ids
                                                                        .split(',')
                                                                        .filter(|s| !s.is_empty())
                                                                        .map(String::from)
                                                                        .collect();
                                                                    let resolved: Vec<Tag> = all_tags
                                                                        .get()
                                                                        .unwrap_or_default()
                                                                        .into_iter()
                                                                        .filter(|t| ids.contains(&t.id.0.to_string()))
                                                                        .collect();
                                                                    tag_filters.set(resolved);
                                                                    show_preset_menu.set(false);
                                                                }
                                                            >
                                                                {name}
                                                            </button>
                                                            // Delete × button
                                                            <button
                                                                class="px-2.5 py-2 text-stone-300
                                                                    dark:text-stone-600
                                                                    hover:text-red-500 dark:hover:text-red-400
                                                                    transition-colors"
                                                                title="Delete preset"
                                                                on:click=move |_| {
                                                                    let pr = preset_refresh;
                                                                    wasm_bindgen_futures::spawn_local(async move {
                                                                        if crate::api::delete_search_preset(preset_id)
                                                                            .await
                                                                            .is_ok()
                                                                        {
                                                                            pr.update(|n| *n += 1);
                                                                        }
                                                                    });
                                                                }
                                                            >
                                                                <span class="material-symbols-outlined"
                                                                    style="font-size: 13px;">"close"</span>
                                                            </button>
                                                        </div>
                                                    }
                                                }).collect::<Vec<_>>()}
                                            </div>
                                        }.into_any()
                                    }
                                }}

                                // ── Save current search ─────────────────────
                                <div class="border-t border-stone-100 dark:border-stone-800">
                                    {move || {
                                        if show_save_form.get() {
                                            view! {
                                                <div class="flex items-center gap-1.5 px-3 py-2">
                                                    <input
                                                        type="text"
                                                        class="flex-1 text-xs px-2 py-1 rounded
                                                            border border-stone-300 dark:border-stone-600
                                                            bg-white dark:bg-stone-800
                                                            text-stone-800 dark:text-stone-200
                                                            focus:outline-none focus:border-amber-400
                                                            dark:focus:border-amber-500"
                                                        placeholder="Preset name\u{2026}"
                                                        prop:value=move || preset_name.get()
                                                        on:input=move |ev| preset_name.set(event_target_value(&ev))
                                                    />
                                                    <button
                                                        class="text-xs px-2 py-1 rounded bg-amber-500 text-white
                                                            hover:bg-amber-600 transition-colors"
                                                        on:click=move |_| {
                                                            let name = preset_name.get_untracked()
                                                                .trim()
                                                                .to_string();
                                                            if name.is_empty() { return; }
                                                            let q  = search_query.get_untracked();
                                                            let f  = fuzzy.get_untracked();
                                                            let p  = published_only.get_untracked();
                                                            let tids = tag_filters
                                                                .get_untracked()
                                                                .iter()
                                                                .map(|t| t.id.0.to_string())
                                                                .collect::<Vec<_>>()
                                                                .join(",");
                                                            let top = if tag_op_and.get_untracked() {
                                                                "and"
                                                            } else {
                                                                "or"
                                                            };
                                                            let pr = preset_refresh;
                                                            let sn = preset_name;
                                                            let sf = show_save_form;
                                                            wasm_bindgen_futures::spawn_local(async move {
                                                                let req = CreateSearchPresetRequest {
                                                                    name,
                                                                    query:          q,
                                                                    fuzzy:          f,
                                                                    published_only: p,
                                                                    tag_ids:        tids,
                                                                    tag_op:         top.to_string(),
                                                                };
                                                                if crate::api::create_search_preset(&req)
                                                                    .await
                                                                    .is_ok()
                                                                {
                                                                    pr.update(|n| *n += 1);
                                                                    sn.set(String::new());
                                                                    sf.set(false);
                                                                }
                                                            });
                                                        }
                                                    >
                                                        "Save"
                                                    </button>
                                                    <button
                                                        class="text-stone-400 hover:text-stone-600
                                                            dark:hover:text-stone-300 transition-colors"
                                                        on:click=move |_| {
                                                            show_save_form.set(false);
                                                            preset_name.set(String::new());
                                                        }
                                                    >
                                                        <span class="material-symbols-outlined"
                                                            style="font-size: 14px;">"close"</span>
                                                    </button>
                                                </div>
                                            }.into_any()
                                        } else {
                                            view! {
                                                <button
                                                    class="w-full text-left flex items-center gap-2
                                                        px-3 py-2 text-xs
                                                        text-stone-500 dark:text-stone-400
                                                        hover:bg-stone-50 dark:hover:bg-stone-800
                                                        transition-colors"
                                                    on:click=move |_| show_save_form.set(true)
                                                >
                                                    <span class="material-symbols-outlined"
                                                        style="font-size: 13px;">"bookmark_add"</span>
                                                    "Save current search"
                                                </button>
                                            }.into_any()
                                        }
                                    }}
                                </div>
                            </div>
                        }
                    })}
                </div>

                <div class="flex items-center gap-3 ml-auto">
                    {move || loading.get().then_some(view! {
                        <span class="text-xs text-stone-400 dark:text-stone-500 animate-pulse">"Searching\u{2026}"</span>
                    })}
                    // Date range
                    <div class="flex items-center gap-1.5">
                        <span class="text-xs text-stone-400 dark:text-stone-500 shrink-0">"From"</span>
                        <input
                            type="date"
                            class="text-xs rounded border border-stone-300 dark:border-stone-600
                                bg-white dark:bg-stone-800 text-stone-700 dark:text-stone-300
                                px-1.5 py-0.5 focus:outline-none focus:ring-1 focus:ring-amber-400"
                            prop:value=move || updated_after.get()
                            on:change=move |ev| updated_after.set(event_target_value(&ev))
                        />
                        <span class="text-xs text-stone-400 dark:text-stone-500 shrink-0">"To"</span>
                        <input
                            type="date"
                            class="text-xs rounded border border-stone-300 dark:border-stone-600
                                bg-white dark:bg-stone-800 text-stone-700 dark:text-stone-300
                                px-1.5 py-0.5 focus:outline-none focus:ring-1 focus:ring-amber-400"
                            prop:value=move || updated_before.get()
                            on:change=move |ev| updated_before.set(event_target_value(&ev))
                        />
                    </div>
                    // Sort
                    <select
                        class="text-xs rounded border border-stone-300 dark:border-stone-600
                            bg-white dark:bg-stone-800 text-stone-600 dark:text-stone-300
                            px-1.5 py-0.5 focus:outline-none focus:ring-1 focus:ring-amber-400"
                        prop:value=move || sort.get()
                        on:change=move |ev| sort.set(event_target_value(&ev))
                    >
                        <option value="relevance">"Relevance"</option>
                        <option value="updated_desc">"Updated \u{2193}"</option>
                        <option value="updated_asc">"Updated \u{2191}"</option>
                        <option value="title_asc">"Title A\u{2013}Z"</option>
                        <option value="title_desc">"Title Z\u{2013}A"</option>
                    </select>
                    <label class="flex items-center gap-1.5 text-sm text-stone-600 dark:text-stone-400 cursor-pointer select-none">
                        <input
                            type="checkbox"
                            class="cursor-pointer"
                            prop:checked=move || fuzzy.get()
                            on:change=move |_| fuzzy.update(|f| *f = !*f)
                        />
                        "Fuzzy"
                    </label>
                    <label class="flex items-center gap-1.5 text-sm text-stone-600 dark:text-stone-400 cursor-pointer select-none">
                        <input
                            type="checkbox"
                            class="cursor-pointer"
                            prop:checked=move || published_only.get()
                            on:change=move |_| published_only.update(|v| *v = !*v)
                        />
                        "Published"
                    </label>
                </div>

                // Node type chips — second row
                <div class="w-full flex items-center gap-1 pt-2 border-t border-stone-100 dark:border-stone-800">
                    {[
                        (None,                        "All"),
                        (Some("article"),             "Article"),
                        (Some("project"),             "Project"),
                        (Some("area"),                "Area"),
                        (Some("resource"),            "Resource"),
                        (Some("reference"),           "Reference"),
                    ].into_iter().map(|(value, label)| {
                        let value_owned: Option<String> = value.map(String::from);
                        let value_for_click = value_owned.clone();
                        view! {
                            <button
                                class=move || {
                                    let active = node_type_filter.get() == value_owned;
                                    if active {
                                        "px-2.5 py-0.5 text-xs rounded-full font-medium \
                                         bg-amber-500 text-white transition-colors"
                                    } else {
                                        "px-2.5 py-0.5 text-xs rounded-full font-medium \
                                         text-stone-500 dark:text-stone-400 \
                                         hover:text-stone-700 dark:hover:text-stone-200 \
                                         hover:bg-stone-100 dark:hover:bg-stone-800 transition-colors"
                                    }
                                }
                                on:click=move |_| node_type_filter.set(value_for_click.clone())
                            >
                                {label}
                            </button>
                        }
                    }).collect::<Vec<_>>()}
                </div>
            </div>

            // Results area
            <div class="flex-1 overflow-y-auto px-6 py-4">
                {move || error_msg.get().map(|msg| view! {
                    <div class="mb-4 px-4 py-3 rounded-lg bg-red-50 dark:bg-red-900/20 text-red-600 dark:text-red-400 text-sm">
                        {msg}
                    </div>
                })}

                {move || {
                    results.get().map(|resp| {
                        let total = resp.total;
                        let current_page = resp.page;
                        let per_page = resp.per_page;
                        let total_pages = ((total as f64) / (per_page as f64)).ceil() as u32;

                        view! {
                            <div class="mb-3 text-sm text-stone-500 dark:text-stone-400">
                                {format!("{total} result{}", if total == 1 { "" } else { "s" })}
                                {if total_pages > 1 {
                                    format!(" \u{00b7} page {current_page} of {total_pages}")
                                } else {
                                    String::new()
                                }}
                            </div>

                            {if resp.results.is_empty() && total == 0 {
                                view! {
                                    <div class="text-center py-12 text-stone-400 dark:text-stone-600">
                                        <span class="material-symbols-outlined text-4xl mb-2 block">"search_off"</span>
                                        <p>"No results found. Try different keywords, tags, or enable fuzzy search."</p>
                                    </div>
                                }.into_any()
                            } else {
                                view! {
                                    <div class="space-y-3">
                                        {resp.results.into_iter().map(|result| {
                                            let node_id = result.node_id;
                                            // 3-tier relevance: ≥0.3 → high, ≥0.08 → medium, >0 → low
                                            let rank_tier: u8 = if result.rank >= 0.3 { 3 }
                                                else if result.rank >= 0.08 { 2 }
                                                else if result.rank > 0.0 { 1 }
                                                else { 0 };
                                            let rank_title = format!("Relevance: {:.0}%", result.rank * 100.0);
                                            let nt = result.node_type.clone();
                                            let st = result.status.clone();
                                            let t_icon  = type_icon(&nt);
                                            let t_label = type_label(&nt);
                                            let s_icon  = status_icon(&st);
                                            let s_label = status_label(&st);
                                            let s_color = status_color(&st);
                                            let match_src = result.match_source.clone();
                                            let snippet = result.snippet.as_deref().map(sanitize_highlight);
                                            let plain_title = result.title.clone();
                                            let hl_title = result.highlighted_title.as_deref().map(sanitize_highlight);

                                            view! {
                                                <button
                                                    class="w-full text-left block p-4 rounded-lg border border-stone-200
                                                        dark:border-stone-700 hover:border-amber-300 dark:hover:border-amber-700
                                                        bg-white dark:bg-stone-900 transition-colors"
                                                    on:click=move |_| {
                                                        navigate.get_value()(&format!("/nodes/{node_id}"), Default::default());
                                                    }
                                                >
                                                    // Row 1: type icon · title · status icon · rank
                                                    <div class="flex items-center gap-2 mb-1">
                                                        <span
                                                            class="material-symbols-outlined text-stone-400
                                                                   dark:text-stone-500 flex-shrink-0"
                                                            style="font-size: 15px;"
                                                            title=t_label
                                                        >
                                                            {t_icon}
                                                        </span>
                                                        <h3 class="text-sm font-medium text-stone-900 dark:text-stone-100 truncate flex-1">
                                                            {match hl_title {
                                                                Some(ht) => view! {
                                                                    <span inner_html=ht />
                                                                }.into_any(),
                                                                None => view! { {plain_title} }.into_any(),
                                                            }}
                                                        </h3>
                                                        <span
                                                            class="material-symbols-outlined flex-shrink-0"
                                                            style=format!("font-size: 14px; {s_color}")
                                                            title=s_label
                                                        >
                                                            {s_icon}
                                                        </span>
                                                        // 3-bar relevance indicator
                                                        <div class="flex gap-px items-end shrink-0"
                                                            title=rank_title>
                                                            <div class=format!("w-1 rounded-sm {}",
                                                                if rank_tier >= 1 { "bg-amber-400 dark:bg-amber-500" }
                                                                else { "bg-stone-200 dark:bg-stone-700" })
                                                                style="height: 6px;" />
                                                            <div class=format!("w-1 rounded-sm {}",
                                                                if rank_tier >= 2 { "bg-amber-400 dark:bg-amber-500" }
                                                                else { "bg-stone-200 dark:bg-stone-700" })
                                                                style="height: 9px;" />
                                                            <div class=format!("w-1 rounded-sm {}",
                                                                if rank_tier >= 3 { "bg-amber-400 dark:bg-amber-500" }
                                                                else { "bg-stone-200 dark:bg-stone-700" })
                                                                style="height: 12px;" />
                                                        </div>
                                                    </div>
                                                    // Row 2: slug
                                                    <p class="text-xs text-stone-500 dark:text-stone-400 mb-1 font-mono">
                                                        {result.slug}
                                                    </p>
                                                    // Row 2.5: match source badge (notes / tasks only)
                                                    {match match_src.as_deref() {
                                                        Some("note") => Some(view! {
                                                            <span class="inline-flex items-center gap-1 mb-1 px-1.5 py-0.5
                                                                text-xs rounded
                                                                bg-blue-50 dark:bg-blue-900/20
                                                                text-blue-600 dark:text-blue-400
                                                                border border-blue-200 dark:border-blue-800">
                                                                <span class="material-symbols-outlined"
                                                                    style="font-size: 11px;">"sticky_note_2"</span>
                                                                "matched in note"
                                                            </span>
                                                        }.into_any()),
                                                        Some("task") => Some(view! {
                                                            <span class="inline-flex items-center gap-1 mb-1 px-1.5 py-0.5
                                                                text-xs rounded
                                                                bg-amber-50 dark:bg-amber-900/20
                                                                text-amber-700 dark:text-amber-400
                                                                border border-amber-200 dark:border-amber-800">
                                                                <span class="material-symbols-outlined"
                                                                    style="font-size: 11px;">"task_alt"</span>
                                                                "matched in task"
                                                            </span>
                                                        }.into_any()),
                                                        _ => None,
                                                    }}
                                                    // Row 3: snippet (optional)
                                                    {snippet.map(|s| view! {
                                                        <p
                                                            class="text-xs text-stone-600 dark:text-stone-400 line-clamp-2"
                                                            inner_html=s
                                                        />
                                                    })}
                                                </button>
                                            }
                                        }).collect::<Vec<_>>()}
                                    </div>

                                    {if total_pages > 1 {
                                        Some(view! {
                                            <Pagination
                                                current_page=current_page
                                                total_pages=total_pages
                                                on_page=on_page_change
                                            />
                                        })
                                    } else {
                                        None
                                    }}
                                }.into_any()
                            }}
                        }
                    })
                }}

                // ── Recent search history ─────────────────────────────────
                {move || {
                    let q = search_query.get();
                    let items = history.get();
                    if !q.trim().is_empty() || items.is_empty() { return None; }
                    Some(view! {
                        <div class="mb-6">
                            <div class="flex items-center justify-between mb-3">
                                <h2 class="text-sm font-medium text-stone-500 dark:text-stone-400">
                                    "Recent searches"
                                </h2>
                                <button
                                    class="text-xs text-stone-400 dark:text-stone-500
                                           hover:text-stone-600 dark:hover:text-stone-300
                                           transition-colors cursor-pointer"
                                    on:click=move |_| {
                                        clear_history();
                                        history.set(Vec::new());
                                    }
                                >
                                    "Clear all"
                                </button>
                            </div>
                            <div class="space-y-1">
                                {items.into_iter().map(|item| {
                                    let item_click = item.clone();
                                    let item_remove = item.clone();
                                    let item_display = item.clone();
                                    view! {
                                        <div class="flex items-center gap-2 px-3 py-2 rounded-lg
                                                    hover:bg-stone-100 dark:hover:bg-stone-800
                                                    transition-colors group">
                                            <span class="material-symbols-outlined text-stone-400
                                                         dark:text-stone-500"
                                                  style="font-size: 16px;">
                                                "history"
                                            </span>
                                            <button
                                                class="flex-1 text-left text-sm text-stone-700
                                                       dark:text-stone-300 truncate cursor-pointer"
                                                on:click=move |_| {
                                                    search_query.set(item_click.clone());
                                                }
                                            >
                                                {item_display}
                                            </button>
                                            <button
                                                class="p-0.5 rounded text-stone-300 dark:text-stone-600
                                                       hover:text-stone-500 dark:hover:text-stone-400
                                                       opacity-0 group-hover:opacity-100
                                                       transition-all cursor-pointer"
                                                title="Remove"
                                                on:click=move |_| {
                                                    remove_history(&item_remove);
                                                    history.set(load_history());
                                                }
                                            >
                                                <span class="material-symbols-outlined"
                                                      style="font-size: 14px;">"close"</span>
                                            </button>
                                        </div>
                                    }
                                }).collect::<Vec<_>>()}
                            </div>
                        </div>
                    })
                }}

                // Loading state on first render
                {move || {
                    if results.get().is_none() && loading.get() {
                        Some(view! {
                            <div class="text-center py-16 text-stone-400 dark:text-stone-600">
                                <span class="text-sm animate-pulse">"Loading\u{2026}"</span>
                            </div>
                        }.into_any())
                    } else {
                        None
                    }
                }}
            </div>
        </div>
    }
}

// ── Custom tag picker ─────────────────────────────────────────────────────────

/// Replaces the native `<select>` for adding tag filters.
///
/// Renders a button that opens a custom dropdown listing unselected tags.
/// A click-away backdrop closes the menu without selecting anything.
#[component]
fn TagPicker(all_tags: LocalResource<Vec<Tag>>, tag_filters: RwSignal<Vec<Tag>>) -> impl IntoView {
    let open = RwSignal::new(false);

    view! {
        <div class="relative">
            <button
                class="flex items-center gap-1 px-2 py-0.5 text-xs rounded-md
                       border border-stone-300 dark:border-stone-600
                       bg-white dark:bg-stone-800
                       text-stone-500 dark:text-stone-400
                       hover:border-amber-400 dark:hover:border-amber-500
                       hover:text-stone-700 dark:hover:text-stone-200
                       transition-colors"
                on:click=move |_| open.update(|v| *v = !*v)
            >
                <span class="material-symbols-outlined" style="font-size: 13px;">"label"</span>
                "+ Add tag filter"
                <span class="material-symbols-outlined" style="font-size: 13px;">
                    {move || if open.get() { "expand_less" } else { "expand_more" }}
                </span>
            </button>

            {move || open.get().then(|| {
                let tags_now = all_tags.get().unwrap_or_default();
                let active_ids: Vec<_> = tag_filters.get().iter().map(|t| t.id).collect();
                let available: Vec<Tag> = tags_now
                    .into_iter()
                    .filter(|t| !active_ids.contains(&t.id))
                    .collect();

                view! {
                    // Click-away backdrop
                    <div
                        class="fixed inset-0 z-10"
                        on:click=move |_| open.set(false)
                    />
                    <div class="absolute left-0 top-full mt-1 z-20 min-w-36
                                bg-white dark:bg-stone-900 rounded-xl shadow-xl
                                border border-stone-200 dark:border-stone-700 overflow-hidden">
                        {if available.is_empty() {
                            view! {
                                <p class="px-3 py-2 text-xs text-stone-400 dark:text-stone-500 italic">
                                    "All tags selected"
                                </p>
                            }.into_any()
                        } else {
                            view! {
                                <div>
                                    {available.into_iter().map(|tag| {
                                        let color = tag.color.clone();
                                        let name = tag.name.clone();
                                        view! {
                                            <button
                                                class="w-full text-left flex items-center gap-2 px-3 py-2 text-xs
                                                       text-stone-700 dark:text-stone-300
                                                       hover:bg-stone-50 dark:hover:bg-stone-800
                                                       transition-colors"
                                                on:click=move |_| {
                                                    let t = tag.clone();
                                                    tag_filters.update(|v| {
                                                        if !v.iter().any(|x| x.id == t.id) {
                                                            v.push(t);
                                                        }
                                                    });
                                                    open.set(false);
                                                }
                                            >
                                                <span
                                                    class="w-2.5 h-2.5 rounded-full flex-shrink-0"
                                                    style=format!("background-color: {color}")
                                                />
                                                {name}
                                            </button>
                                        }
                                    }).collect::<Vec<_>>()}
                                </div>
                            }.into_any()
                        }}
                    </div>
                }
            })}
        </div>
    }
}

#[component]
fn Pagination(
    current_page: u32,
    total_pages: u32,
    #[prop(into)] on_page: Callback<u32>,
) -> impl IntoView {
    let pages: Vec<u32> = {
        let mut p = Vec::new();
        let start = current_page.saturating_sub(2).max(1);
        let end = (start + 4).min(total_pages);
        let start = end.saturating_sub(4).max(1);
        for i in start..=end {
            p.push(i);
        }
        p
    };

    let prev_disabled = current_page <= 1;
    let next_disabled = current_page >= total_pages;

    let on_prev = move |_: web_sys::MouseEvent| {
        on_page.run(current_page - 1);
    };

    let on_next = move |_: web_sys::MouseEvent| {
        on_page.run(current_page + 1);
    };

    view! {
        <div class="flex items-center justify-center gap-1 mt-6">
            <button
                class="px-3 py-1.5 text-xs rounded-md border border-stone-300 dark:border-stone-600
                    text-stone-600 dark:text-stone-400 hover:bg-stone-100 dark:hover:bg-stone-800
                    disabled:opacity-30 disabled:cursor-not-allowed transition-colors"
                prop:disabled=prev_disabled
                on:click=on_prev
            >
                "\u{2190} Prev"
            </button>

            {pages.into_iter().map(|p| {
                let is_current = p == current_page;
                view! {
                    <button
                        class=move || format!(
                            "px-3 py-1.5 text-xs rounded-md transition-colors {}",
                            if is_current {
                                "bg-amber-600 text-white font-medium"
                            } else {
                                "border border-stone-300 dark:border-stone-600 text-stone-600 dark:text-stone-400 hover:bg-stone-100 dark:hover:bg-stone-800"
                            }
                        )
                        on:click=move |_| {
                            if !is_current {
                                on_page.run(p);
                            }
                        }
                    >
                        {p.to_string()}
                    </button>
                }
            }).collect::<Vec<_>>()}

            <button
                class="px-3 py-1.5 text-xs rounded-md border border-stone-300 dark:border-stone-600
                    text-stone-600 dark:text-stone-400 hover:bg-stone-100 dark:hover:bg-stone-800
                    disabled:opacity-30 disabled:cursor-not-allowed transition-colors"
                prop:disabled=next_disabled
                on:click=on_next
            >
                "Next \u{2192}"
            </button>
        </div>
    }
}
