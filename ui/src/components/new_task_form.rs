//! Shared add-task form used by both the node-detail `TaskPanel` and the
//! `InboxView`.  Centralising the markup keeps the two surfaces visually and
//! behaviourally identical (icon submit, priority/due/recurrence controls,
//! inline error), so they can no longer drift apart.
//!
//! When `node_id` is `Some`, the form creates the task under that node and no
//! node picker is shown (node-detail usage).  When `node_id` is `None`, the
//! form is a standalone/inbox capture and shows an *optional* debounced
//! node-search picker so the user can link the task to a node — or leave it
//! unlinked if no suitable node exists yet.

use chrono::NaiveDate;
use common::{id::NodeId, search::SearchResult, task::CreateTaskRequest};
use leptos::prelude::*;

use crate::components::task_common::{node_type_icon, parse_priority, parse_recurrence_opt};

/// Reusable "new task" form.
///
/// * `node_id` — `Some(id)` fixes the parent node and hides the picker;
///   `None` shows the optional node-search picker and creates a standalone task.
/// * `refresh` — bumped after a successful create so the surrounding list reloads.
/// * `on_added` — optional callback invoked after a successful create (the node
///   panel uses it to collapse the form again).
#[component]
pub fn NewTaskForm(
    #[prop(optional)] node_id: Option<NodeId>,
    refresh: RwSignal<u32>,
    #[prop(optional)] on_added: Option<Callback<()>>,
) -> impl IntoView {
    let show_picker = node_id.is_none();

    // Core form state
    let new_title      = RwSignal::new(String::new());
    let new_priority   = RwSignal::new("medium".to_string());
    let new_due        = RwSignal::new(String::new());
    let new_recurrence = RwSignal::new(String::new());
    let adding         = RwSignal::new(false);
    let add_error      = RwSignal::new(Option::<String>::None);

    // Optional node-picker state (only meaningful when `show_picker`).
    let selected_node  = RwSignal::<Option<(NodeId, String)>>::new(None);
    let picker_query   = RwSignal::new(String::new());
    let picker_results = RwSignal::<Vec<SearchResult>>::new(vec![]);
    let pick_ver       = RwSignal::new(0u32);

    // Debounced node search — mirrors the inbox "assign to node" picker.
    if show_picker {
        Effect::new(move |_| {
            let q = picker_query.get();
            if q.trim().is_empty() {
                picker_results.set(vec![]);
                return;
            }
            pick_ver.update(|v| *v += 1);
            let ver = pick_ver.get_untracked();
            wasm_bindgen_futures::spawn_local(async move {
                gloo_timers::future::TimeoutFuture::new(300).await;
                if pick_ver.get_untracked() != ver { return; }
                if let Ok(results) = crate::api::node_picker_search(&q).await
                    && pick_ver.get_untracked() == ver
                {
                    picker_results.set(results);
                }
            });
        });
    }

    let do_add = move || {
        let title = new_title.get_untracked().trim().to_string();
        if title.is_empty() {
            add_error.set(Some("Title is required.".to_string()));
            return;
        }
        let priority = parse_priority(&new_priority.get_untracked());
        let due_date = new_due
            .get_untracked()
            .trim()
            .parse::<NaiveDate>()
            .ok();
        let recurrence = parse_recurrence_opt(&new_recurrence.get_untracked());
        // Fixed prop node wins; otherwise use the optionally-picked node.
        let chosen_node = node_id.or_else(|| selected_node.get_untracked().map(|(id, _)| id));
        adding.set(true);
        add_error.set(None);
        wasm_bindgen_futures::spawn_local(async move {
            let req = CreateTaskRequest {
                title,
                node_id: chosen_node,
                status: None,
                priority: Some(priority),
                focus_date: None,
                due_date,
                recurrence,
            };
            // A fixed node uses the node-scoped endpoint (path param);
            // standalone capture uses /tasks, which honours the body node_id.
            let result = match node_id {
                Some(nid) => crate::api::create_task(nid, &req).await,
                None => crate::api::create_standalone_task(&req).await,
            };
            match result {
                Ok(_) => {
                    new_title.set(String::new());
                    new_priority.set("medium".to_string());
                    new_due.set(String::new());
                    new_recurrence.set(String::new());
                    selected_node.set(None);
                    picker_query.set(String::new());
                    picker_results.set(vec![]);
                    refresh.update(|n| *n += 1);
                    if let Some(cb) = on_added {
                        cb.run(());
                    }
                }
                Err(e) => add_error.set(Some(format!("{e}"))),
            }
            adding.set(false);
        });
    };

    view! {
        <div class="p-3 rounded-lg bg-stone-50 dark:bg-stone-800/50
            border border-stone-200 dark:border-stone-700 space-y-2">
            <input
                type="text"
                placeholder="Task title…"
                class="w-full bg-transparent text-sm text-stone-900 dark:text-stone-100
                    focus:outline-none placeholder-stone-400"
                prop:value=move || new_title.get()
                on:input=move |ev| new_title.set(event_target_value(&ev))
                on:keydown=move |ev: leptos::ev::KeyboardEvent| {
                    if ev.key() == "Enter" { do_add(); }
                }
            />
            <div class="flex items-center gap-2">
                <select
                    class="text-xs bg-stone-100 dark:bg-stone-700 text-stone-700 dark:text-stone-300
                        rounded px-2 py-1 focus:outline-none"
                    prop:value=move || new_priority.get()
                    on:change=move |ev| new_priority.set(event_target_value(&ev))
                >
                    <option value="high">"High"</option>
                    <option value="medium">"Medium"</option>
                    <option value="low">"Low"</option>
                </select>
                <input
                    type="date"
                    class="text-xs bg-stone-100 dark:bg-stone-700 text-stone-700 dark:text-stone-300
                        rounded px-2 py-1 focus:outline-none"
                    title="Optional due date"
                    prop:value=move || new_due.get()
                    on:input=move |ev| new_due.set(event_target_value(&ev))
                />
                <select
                    class="text-xs bg-stone-100 dark:bg-stone-700 text-stone-700 dark:text-stone-300
                        rounded px-2 py-1 focus:outline-none"
                    title="Recurrence"
                    prop:value=move || new_recurrence.get()
                    on:change=move |ev| new_recurrence.set(event_target_value(&ev))
                >
                    <option value="">"No repeat"</option>
                    <option value="daily">"Daily"</option>
                    <option value="weekly">"Weekly"</option>
                    <option value="biweekly">"Every 2 weeks"</option>
                    <option value="monthly">"Monthly"</option>
                    <option value="yearly">"Yearly"</option>
                </select>
                <span class="flex-1"/>
                <button
                    class="p-1.5 rounded-lg text-stone-400 hover:text-green-600 dark:hover:text-green-400
                        hover:bg-green-50 dark:hover:bg-green-900/30 transition-colors cursor-pointer
                        disabled:opacity-50 disabled:cursor-not-allowed"
                    title=move || if adding.get() { "Adding…" } else { "Add task" }
                    on:click=move |_| do_add()
                    disabled=move || adding.get()
                >
                    <span class="material-symbols-outlined">
                        {move || if adding.get() { "hourglass_empty" } else { "add" }}
                    </span>
                </button>
            </div>

            // Optional node picker — only for standalone (inbox) capture.
            {show_picker.then(|| view! {
                <div class="relative">
                    {move || match selected_node.get() {
                        Some((_, title)) => view! {
                            <div class="flex items-center gap-1.5 text-xs
                                text-stone-600 dark:text-stone-300">
                                <span class="material-symbols-outlined text-stone-400"
                                    style="font-size: 14px;">"link"</span>
                                <span class="truncate">{title}</span>
                                <button
                                    class="p-0.5 rounded text-stone-400 hover:text-red-500
                                        dark:hover:text-red-400 transition-colors cursor-pointer"
                                    title="Unlink node"
                                    on:click=move |_| selected_node.set(None)
                                >
                                    <span class="material-symbols-outlined"
                                        style="font-size: 14px;">"close"</span>
                                </button>
                            </div>
                        }.into_any(),
                        None => view! {
                            <div class="flex items-center gap-1.5">
                                <span class="material-symbols-outlined text-stone-400 flex-shrink-0"
                                    style="font-size: 14px;">"link"</span>
                                <input
                                    type="text"
                                    placeholder="Link to a node (optional)…"
                                    class="flex-1 min-w-0 text-xs bg-stone-100 dark:bg-stone-700
                                        text-stone-700 dark:text-stone-300 rounded px-2 py-1
                                        focus:outline-none"
                                    prop:value=move || picker_query.get()
                                    on:input=move |ev| picker_query.set(event_target_value(&ev))
                                    on:keydown=move |ev: leptos::ev::KeyboardEvent| {
                                        if ev.key() == "Escape" {
                                            picker_query.set(String::new());
                                            picker_results.set(vec![]);
                                        }
                                    }
                                />
                            </div>
                        }.into_any(),
                    }}
                    // Results dropdown
                    {move || {
                        let results = picker_results.get();
                        (selected_node.get().is_none() && !results.is_empty()).then(|| view! {
                            <div class="absolute z-10 mt-1 w-full bg-white dark:bg-stone-900
                                border border-stone-200 dark:border-stone-700
                                rounded-lg shadow-md overflow-hidden max-h-56 overflow-y-auto">
                                {results.into_iter().map(|r| {
                                    let nid        = r.node_id;
                                    let title_sel  = r.title.clone();
                                    let title_disp = r.title.clone();
                                    let icon       = node_type_icon(&r.node_type);
                                    view! {
                                        <button
                                            class="w-full flex items-center gap-2 px-3 py-2 text-xs
                                                text-stone-700 dark:text-stone-300
                                                hover:bg-amber-50 dark:hover:bg-stone-800
                                                border-b border-stone-50 dark:border-stone-800
                                                last:border-b-0 transition-colors cursor-pointer"
                                            on:click=move |_| {
                                                selected_node.set(Some((nid, title_sel.clone())));
                                                picker_query.set(String::new());
                                                picker_results.set(vec![]);
                                            }
                                        >
                                            <span class="material-symbols-outlined text-stone-400
                                                flex-shrink-0" style="font-size: 14px;">{icon}</span>
                                            <span class="truncate text-left">{title_disp}</span>
                                        </button>
                                    }
                                }).collect_view()}
                            </div>
                        })
                    }}
                </div>
            })}

            {move || add_error.get().map(|msg| view! {
                <p class="text-xs text-red-500">{msg}</p>
            })}
        </div>
    }
}
