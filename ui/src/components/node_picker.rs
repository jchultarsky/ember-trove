//! Shared type-ahead node picker: a combobox over the debounced node search
//! that writes the chosen `(NodeId, title)` into the `selected` signal and
//! shows a removable chip once picked. Used by the new-task form, the Notes
//! compose box and the Connections "add edge" form.

use common::{id::NodeId, search::SearchResult};
use leptos::prelude::*;

/// Debounce for type-ahead node search.
const NODE_SEARCH_DEBOUNCE_MS: u32 = 300;

/// Type-ahead node search behind every node picker (this component, the
/// palette, Inbox attach, triage attach): 300 ms debounce, and only the
/// latest query's response lands. Returns the results signal; callers may
/// clear it themselves.
///
/// The five hand-rolled copies drifted: three skipped the version bump on an
/// empty query, so clearing the box while a search was in flight let the old
/// results reappear, and the Links picker searched on every keystroke.
pub fn use_node_search(query: RwSignal<String>) -> RwSignal<Vec<SearchResult>> {
    use_node_search_answering(query).0
}

/// [`use_node_search`] plus the query the current results answer. During
/// the debounce the list still shows an *earlier* query's results; a picker
/// must not commit from those (Enter after fast typing would pick a match
/// for "Pro" while the user typed "Project X").
fn use_node_search_answering(
    query: RwSignal<String>,
) -> (RwSignal<Vec<SearchResult>>, RwSignal<String>) {
    let results = RwSignal::new(Vec::<SearchResult>::new());
    let answers = RwSignal::new(String::new());
    let version = RwSignal::new(0u32);
    Effect::new(move |_| {
        let q = query.get();
        version.update(|v| *v += 1);
        let mine = version.get_untracked();
        if q.trim().is_empty() {
            results.set(Vec::new());
            answers.set(q);
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
                answers.try_set(q);
            }
        });
    });
    (results, answers)
}

/// Unique DOM ids per picker instance (`aria-controls`, option ids).
static NEXT_PICKER_ID: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);

/// Shared node picker, a WAI-ARIA combobox (#126). Typed text only
/// *filters*: the best match is highlighted (`common::picker::best_index`)
/// but nothing is chosen until an option is committed — Enter on the active
/// option, or pressing one. The committed node renders as a chip. Enter
/// typed faster than the debounced search is remembered and, once results
/// land, commits only a *unique* exact title match (`unique_exact`) — never
/// a guess; likewise Enter won't commit an auto-highlighted option among
/// same-titled nodes (`is_ambiguous`) until the user picks one. ↑/↓ move the active option; Escape closes the list, then clears
/// the text, and is stopped whenever it does something so the surrounding
/// form and the window shortcuts never also act on it. Blur closes the list.
#[component]
pub fn NodePicker(
    /// The chosen node `(id, title)`, or `None`. Owned by the caller.
    selected: RwSignal<Option<(NodeId, String)>>,
    #[prop(optional, into)] placeholder: Option<String>,
    /// A node never to offer (e.g. the page's own node for an edge).
    #[prop(optional)]
    exclude: Option<NodeId>,
    /// Runs after a commit (e.g. move focus to the form's confirm button).
    #[prop(optional)]
    on_commit: Option<Callback<()>>,
) -> impl IntoView {
    let placeholder = placeholder.unwrap_or_else(|| "Link to a node (optional)…".to_string());
    let base = format!(
        "node-picker-{}",
        NEXT_PICKER_ID.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
    );
    let list_id = format!("{base}-list");
    let opt_id = StoredValue::new(move |i: usize| format!("{base}-opt-{i}"));

    let query = RwSignal::new(String::new());
    let (results, answers) = use_node_search_answering(query);
    let options = Signal::derive(move || {
        results
            .get()
            .into_iter()
            .filter(|r| Some(r.node_id) != exclude)
            .collect::<Vec<SearchResult>>()
    });
    let fresh = move || answers.get_untracked() == query.get_untracked();
    let open = RwSignal::new(false);
    let active = RwSignal::new(None::<usize>);
    // Chosen by the user (arrows, hover) rather than auto-highlighted.
    let chosen = RwSignal::new(false);
    let pending_enter = RwSignal::new(false);
    let picked_type = RwSignal::new(None::<String>);
    let input_ref: NodeRef<leptos::html::Input> = NodeRef::new();
    let refocus = RwSignal::new(false);

    let commit = move |i: usize| {
        let Some(r) = options.with_untracked(|o| o.get(i).cloned()) else {
            return;
        };
        selected.set(Some((r.node_id, r.title)));
        picked_type.set(Some(r.node_type));
        query.set(String::new());
        open.set(false);
        active.set(None);
        pending_enter.set(false);
        if let Some(cb) = on_commit {
            cb.run(());
        }
    };

    // Fresh results: highlight the best match; settle a pending Enter.
    Effect::new(move |_| {
        let opts = options.get();
        let answered = answers.get();
        if answered != query.get_untracked() {
            return;
        }
        let titles: Vec<&str> = opts.iter().map(|r| r.title.as_str()).collect();
        active.set(common::picker::best_index(&answered, &titles));
        chosen.set(false);
        if pending_enter.get_untracked() {
            pending_enter.set(false);
            if let Some(i) = common::picker::unique_exact(&answered, &titles) {
                commit(i);
            }
        }
    });

    // Keep the active option visible in the scrolled list.
    Effect::new(move |_| {
        if let Some(i) = active.get()
            && let Some(el) = web_sys::window()
                .and_then(|w| w.document())
                .and_then(|d| d.get_element_by_id(&opt_id.get_value()(i)))
        {
            let opts = web_sys::ScrollIntoViewOptions::new();
            opts.set_block(web_sys::ScrollLogicalPosition::Nearest);
            el.scroll_into_view_with_scroll_into_view_options(&opts);
        }
    });

    // Back to the input after the chip is cleared (it re-mounts).
    Effect::new(move |_| {
        if refocus.get()
            && let Some(el) = input_ref.get()
        {
            let _ = el.focus();
            refocus.set(false);
        }
    });

    let on_keydown = move |ev: leptos::ev::KeyboardEvent| {
        let len = options.with_untracked(|o| o.len());
        match ev.key().as_str() {
            "ArrowDown" | "ArrowUp" => {
                ev.prevent_default();
                open.set(true);
                active.set(common::picker::step(
                    active.get_untracked(),
                    len,
                    ev.key() == "ArrowDown",
                ));
                chosen.set(true);
            }
            "Enter" => {
                ev.prevent_default();
                ev.stop_propagation();
                if fresh() {
                    // Same-titled nodes: the highlight is arbitrary, so the
                    // user has to pick one before Enter commits it.
                    let ambiguous = options.with_untracked(|o| {
                        let titles: Vec<&str> = o.iter().map(|r| r.title.as_str()).collect();
                        common::picker::is_ambiguous(&query.get_untracked(), &titles)
                    });
                    if let Some(i) = active.get_untracked()
                        && (chosen.get_untracked() || !ambiguous)
                    {
                        commit(i);
                    }
                } else if !query.get_untracked().trim().is_empty() {
                    // Results for this text aren't in yet: decide when they are.
                    pending_enter.set(true);
                }
            }
            "Escape" => {
                if open.get_untracked() && len > 0 {
                    open.set(false);
                } else if !query.get_untracked().is_empty() {
                    query.set(String::new());
                } else {
                    return; // nothing to do: let the surrounding form have it
                }
                ev.prevent_default();
                ev.stop_propagation();
            }
            "Tab" => open.set(false),
            _ => {}
        }
    };

    let expanded = move || open.get() && !options.with(|o| o.is_empty());
    let list_id_attr = list_id.clone();

    view! {
        <div class="relative">
            {move || match selected.get() {
                Some((_, title)) => {
                    let icon = picked_type
                        .get()
                        .map(|t| crate::components::node_meta::type_icon(&t))
                        .unwrap_or("link");
                    view! {
                        <div
                            class="flex items-center gap-1.5 text-xs text-stone-600 dark:text-stone-300"
                            data-testid="node-picker-chip"
                        >
                            <span class="material-symbols-outlined text-stone-400" style="font-size: 14px;"
                                aria-hidden="true">{icon}</span>
                            <span class="truncate">{title}</span>
                            <button
                                type="button"
                                class="p-0.5 rounded text-stone-400 hover:text-red-500 dark:hover:text-red-400
                                    transition-colors cursor-pointer"
                                title="Unlink node"
                                aria-label="Unlink node"
                                on:click=move |_| {
                                    selected.set(None);
                                    picked_type.set(None);
                                    refocus.set(true);
                                }
                            >
                                <span class="material-symbols-outlined" style="font-size: 14px;"
                                    aria-hidden="true">"close"</span>
                            </button>
                        </div>
                    }.into_any()
                }
                None => view! {
                    <div class="flex items-center gap-1.5">
                        <span class="material-symbols-outlined text-stone-400 flex-shrink-0" style="font-size: 14px;"
                            aria-hidden="true">"link"</span>
                        <input
                            node_ref=input_ref
                            type="text"
                            role="combobox"
                            aria-autocomplete="list"
                            aria-controls=list_id_attr.clone()
                            aria-expanded=move || if expanded() { "true" } else { "false" }
                            aria-activedescendant=move || {
                                active.get().filter(|_| expanded()).map(|i| opt_id.get_value()(i))
                            }
                            placeholder=placeholder.clone()
                            aria-label=placeholder.clone()
                            class="flex-1 min-w-0 text-xs bg-stone-100 dark:bg-stone-700
                                text-stone-700 dark:text-stone-300 rounded-lg px-2 py-1 focus:outline-none"
                            prop:value=move || query.get()
                            on:input=move |ev| {
                                query.set(event_target_value(&ev));
                                open.set(true);
                                pending_enter.set(false);
                            }
                            on:focus=move |_| open.set(true)
                            on:blur=move |_| open.set(false)
                            on:keydown=on_keydown
                        />
                    </div>
                }.into_any(),
            }}
            {move || (selected.with(|s| s.is_none()) && expanded()).then(|| {
                let opts = options.get();
                view! {
                    <div
                        id=list_id.clone()
                        role="listbox"
                        class="absolute z-30 mt-1 w-full bg-white dark:bg-stone-900
                            border border-stone-200 dark:border-stone-700 rounded-lg shadow-md
                            overflow-hidden max-h-56 overflow-y-auto"
                    >
                        {opts.into_iter().enumerate().map(|(i, res)| {
                            let icon = crate::components::node_meta::type_icon(&res.node_type);
                            let type_label = crate::components::node_meta::type_label(&res.node_type);
                            view! {
                                <div
                                    id=opt_id.get_value()(i)
                                    role="option"
                                    aria-selected=move || if active.get() == Some(i) { "true" } else { "false" }
                                    class=move || format!(
                                        "w-full flex items-center gap-2 px-3 py-2 text-xs text-stone-700                                          dark:text-stone-300 border-b border-stone-50 dark:border-stone-800                                          last:border-b-0 cursor-pointer {}",
                                        if active.get() == Some(i) {
                                            "bg-amber-50 dark:bg-stone-800"
                                        } else {
                                            ""
                                        }
                                    )
                                    on:mouseenter=move |_| {
                                        active.set(Some(i));
                                        chosen.set(true);
                                    }
                                    // mousedown, not click: keep focus in the input so
                                    // its blur doesn't close the list mid-press.
                                    on:mousedown=move |ev: web_sys::MouseEvent| {
                                        ev.prevent_default();
                                        commit(i);
                                    }
                                >
                                    <span class="material-symbols-outlined text-stone-400 flex-shrink-0"
                                        style="font-size: 14px;" title=type_label
                                        aria-hidden="true">{icon}</span>
                                    <span class="truncate text-left">{res.title}</span>
                                    // Same-titled nodes: the type tells them apart.
                                    <span class="ml-auto text-[10px] text-stone-400 flex-shrink-0">{type_label}</span>
                                </div>
                            }
                        }).collect_view()}
                    </div>
                }
            })}
        </div>
    }
}
