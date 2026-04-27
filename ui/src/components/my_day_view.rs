use chrono::NaiveDate;
use common::{
    id::TaskId,
    task::{MyDayTask, Task, UpdateTaskRequest},
};
use leptos::prelude::*;

use crate::app::TaskRefresh;
use crate::components::task_common::{
    parse_priority, parse_recurrence_opt, parse_status, priority_value,
    recurrence_label, recurrence_value, sort_tasks_full, status_done, status_value,
};
use crate::components::toast::{push_toast, ToastLevel};
use leptos_router::hooks::use_navigate;

// ── MyDayView ─────────────────────────────────────────────────────────────────

#[component]
pub fn MyDayView() -> impl IntoView {
    let navigate = StoredValue::new(use_navigate());
    let refresh = use_context::<TaskRefresh>()
        .expect("TaskRefresh context must be provided")
        .0;

    let today      = crate::components::format_helpers::local_today();
    let date_label = today.format("%A, %B %-d").to_string();

    let tasks_resource = LocalResource::new(move || {
        let _ = refresh.get();
        async move { crate::api::fetch_my_day(today).await }
    });

    view! {
        <div class="flex flex-col h-full">

            // ── Header ────────────────────────────────────────────────────────
            <div class="px-4 md:px-6 py-4 border-b border-stone-200 dark:border-stone-800">
                <div class="flex items-center gap-3 mb-2">
                    <span class="material-symbols-outlined text-amber-500" style="font-size: 22px;">
                        "wb_sunny"
                    </span>
                    <div class="flex-1 min-w-0">
                        <div class="flex items-baseline justify-between gap-2">
                            <h1 class="text-lg font-semibold text-stone-900 dark:text-stone-100">
                                "My Day"
                            </h1>
                            // X / Y done counter
                            {move || {
                                let tasks = tasks_resource.get()
                                    .and_then(|r| r.ok())
                                    .unwrap_or_default();
                                let total = tasks.len();
                                if total == 0 { return None; }
                                let done = tasks.iter()
                                    .filter(|t| status_done(&t.task.status))
                                    .count();
                                Some(view! {
                                    <span class="text-xs text-stone-400 dark:text-stone-500 flex-shrink-0">
                                        {format!("{done} / {total} done")}
                                    </span>
                                })
                            }}
                        </div>
                        <p class="text-xs text-stone-400 dark:text-stone-500">{date_label}</p>
                    </div>
                </div>

                // ── Progress bar ──────────────────────────────────────────────
                {move || {
                    let tasks = tasks_resource.get()
                        .and_then(|r| r.ok())
                        .unwrap_or_default();
                    let total = tasks.len();
                    if total == 0 { return None; }
                    let done = tasks.iter()
                        .filter(|t| status_done(&t.task.status))
                        .count();
                    let pct = (done * 100) / total;
                    Some(view! {
                        <div class="h-1 bg-stone-100 dark:bg-stone-700 rounded-full overflow-hidden">
                            <div
                                class="h-full bg-amber-400 rounded-full transition-all duration-500"
                                style=format!("width: {pct}%")
                            />
                        </div>
                    })
                }}
            </div>

            // ── Content ───────────────────────────────────────────────────────
            <div class="flex-1 overflow-auto p-4 md:p-6 flex flex-col">
                <Suspense fallback=move || view! {
                    <crate::components::skeleton::SkeletonList rows=6 />
                }>
                    {move || {
                        let raw_tasks = tasks_resource.get()
                            .and_then(|r| r.ok())
                            .unwrap_or_default();

                        if raw_tasks.is_empty() {
                            return view! {
                                <div class="flex-1 flex flex-col items-center justify-center gap-3 py-16">
                                    <span
                                        class="material-symbols-outlined text-stone-300 dark:text-stone-700"
                                        style="font-size: 48px;"
                                    >
                                        "wb_sunny"
                                    </span>
                                    <p class="text-stone-400 dark:text-stone-500 text-sm text-center">
                                        "No tasks for today."
                                    </p>
                                    <p class="text-stone-400 dark:text-stone-500 text-sm text-center max-w-xs">
                                        "Open a project and click the ☀ icon on a task to add it here."
                                    </p>
                                </div>
                            }.into_any();
                        }

                        // Partition: carryovers (focus_date < today AND not done) vs today.
                        // The server already excludes "done before today" from list_my_day,
                        // so anything with focus_date < today is by definition unfinished
                        // — the user must decide what to do with it.
                        let (carryovers, today_tasks): (Vec<MyDayTask>, Vec<MyDayTask>) =
                            raw_tasks.into_iter().partition(|mdt| {
                                mdt.task.focus_date.is_some_and(|d| d < today)
                            });

                        // Group today's tasks by node_id, preserving sort_order.
                        let mut grouped: Vec<(String, Option<String>, Vec<Task>)> = vec![];
                        for MyDayTask { task, node_title } in today_tasks {
                            let node_id_str = task.node_id
                                .map_or_else(|| "inbox".to_string(), |n| n.to_string());
                            if let Some(g) = grouped.iter_mut().find(|(id, _, _)| id == &node_id_str) {
                                g.2.push(task);
                            } else {
                                grouped.push((node_id_str, node_title, vec![task]));
                            }
                        }
                        for (_, _, tasks) in grouped.iter_mut() {
                            sort_tasks_full(tasks);
                        }

                        view! {
                            <div class="space-y-8 w-full">
                                <CarryoverSection carryovers=carryovers refresh=refresh today=today />
                                {grouped.into_iter().map(|(_, node_title, tasks)| {
                                    let node_id = tasks[0].node_id;
                                    view! {
                                        <MyDayGroup
                                            node_id=node_id
                                            node_title=node_title
                                            tasks=tasks
                                            refresh=refresh
                                            on_navigate=move || {
                                                if let Some(nid) = node_id {
                                                    navigate.get_value()(&format!("/nodes/{nid}"), Default::default());
                                                }
                                            }
                                        />
                                    }
                                }).collect_view()}
                            </div>
                        }.into_any()
                    }}
                </Suspense>
            </div>
        </div>
    }
}

// ── MyDayGroup ────────────────────────────────────────────────────────────────

#[component]
fn MyDayGroup(
    node_id: Option<common::id::NodeId>,
    node_title: Option<String>,
    tasks: Vec<Task>,
    refresh: RwSignal<u32>,
    on_navigate: impl Fn() + Copy + 'static,
) -> impl IntoView {
    let _ = node_id;
    let show_done = RwSignal::new(false);

    // Split pre-sorted tasks into active and done buckets.
    let (active, done): (Vec<Task>, Vec<Task>) =
        tasks.into_iter().partition(|t| !status_done(&t.status));
    let done_count  = done.len();
    let done_stored = StoredValue::new(done);

    // Reactive ordering for drag-to-reorder (active tasks only).
    let task_list: RwSignal<Vec<Task>> = RwSignal::new(active);
    let drag_src: RwSignal<Option<usize>> = RwSignal::new(None);
    let drag_over: RwSignal<Option<usize>> = RwSignal::new(None);

    view! {
        <div>
            // Clickable project header (or "Inbox" label for standalone tasks)
            <button
                class="flex items-center gap-2 mb-2 text-xs font-semibold
                    text-stone-500 dark:text-stone-400 uppercase tracking-wider
                    hover:text-amber-600 dark:hover:text-amber-400 transition-colors"
                on:click=move |_| on_navigate()
            >
                <span class="material-symbols-outlined" style="font-size: 14px;">
                    {if node_id.is_some() { "rocket_launch" } else { "inbox" }}
                </span>
                {node_title.unwrap_or_else(|| "Inbox".to_string())}
                {node_id.is_some().then(|| view! {
                    <span class="material-symbols-outlined" style="font-size: 12px;">"open_in_new"</span>
                })}
            </button>

            <div class="space-y-0.5 pl-2 border-l-2 border-stone-200 dark:border-stone-700">
                // Active tasks — draggable for reordering
                {move || task_list.get().into_iter().enumerate().map(|(idx, task)| {
                    view! {
                        <div
                            draggable="true"
                            style=move || if drag_over.get() == Some(idx) {
                                "border-top: 2px solid #f59e0b; margin-top: -2px;"
                            } else { "" }
                            on:dragstart=move |_| drag_src.set(Some(idx))
                            on:dragover=move |ev| {
                                ev.prevent_default();
                                drag_over.set(Some(idx));
                            }
                            on:dragleave=move |_| {
                                if drag_over.get_untracked() == Some(idx) {
                                    drag_over.set(None);
                                }
                            }
                            on:drop=move |ev| {
                                ev.prevent_default();
                                if let Some(src) = drag_src.get_untracked()
                                    && src != idx
                                {
                                    task_list.update(|tasks| {
                                        let t = tasks.remove(src);
                                        let dst = if src < idx { idx - 1 } else { idx };
                                        tasks.insert(dst.min(tasks.len()), t);
                                    });
                                    let updates: Vec<(TaskId, i32)> = task_list
                                        .get_untracked()
                                        .iter()
                                        .enumerate()
                                        .map(|(i, t)| (t.id, (i as i32) * 10))
                                        .collect();
                                    wasm_bindgen_futures::spawn_local(async move {
                                        let _ = crate::api::reorder_tasks(&updates).await;
                                    });
                                }
                                drag_src.set(None);
                                drag_over.set(None);
                            }
                        >
                            <MyDayTaskRow task=task refresh=refresh />
                        </div>
                    }
                }).collect_view()}

                // Completed tasks — collapsed by default
                {(done_count > 0).then(|| view! {
                    <div class="mt-1">
                        <button
                            class="flex items-center gap-1 text-xs text-stone-400
                                dark:text-stone-500 hover:text-stone-600 dark:hover:text-stone-300
                                transition-colors py-1 px-3"
                            on:click=move |_| show_done.update(|v| *v = !*v)
                        >
                            <span class="material-symbols-outlined" style="font-size: 13px;">
                                {move || if show_done.get() { "expand_more" } else { "chevron_right" }}
                            </span>
                            {move || if show_done.get() {
                                format!("Hide {done_count} completed")
                            } else {
                                format!("{done_count} completed")
                            }}
                        </button>
                        {move || show_done.get().then(|| {
                            done_stored.get_value().into_iter().map(|task| {
                                view! { <MyDayTaskRow task=task refresh=refresh /> }
                            }).collect_view()
                        })}
                    </div>
                })}
            </div>
        </div>
    }
}

// ── CarryoverSection ──────────────────────────────────────────────────────────
//
// Renders above today's groups. Tasks here have `focus_date < today` and were
// not finished — they need an explicit decision from the user. Three single-
// click actions per task:
//   * **Move to Today** — sets `focus_date = today`. Stays in My Day.
//   * **Drop**          — clears `focus_date`. Falls back to the Inbox.
//   * **Reschedule**    — toggles a tiny date input; the picked date becomes
//                         the new `focus_date`. Hidden until clicked so it
//                         doesn't add visual noise to every row.
//
// Hidden when there are no carryovers — first-time users never see an empty
// "Carry Over (0)" header.

#[component]
fn CarryoverSection(
    carryovers: Vec<MyDayTask>,
    refresh: RwSignal<u32>,
    today: NaiveDate,
) -> impl IntoView {
    if carryovers.is_empty() {
        return ().into_any();
    }
    let count = carryovers.len();
    view! {
        <div class="rounded-lg border border-amber-200 dark:border-amber-900/40 bg-amber-50/40 \
                    dark:bg-amber-950/20">
            <div class="flex items-center gap-2 px-3 py-2 border-b border-amber-200 \
                        dark:border-amber-900/40">
                <span class="material-symbols-outlined text-amber-600 dark:text-amber-500" style="font-size:16px;">
                    "history"
                </span>
                <span class="text-xs font-semibold text-amber-700 dark:text-amber-400 uppercase tracking-wider">
                    {format!("Carry over ({count})")}
                </span>
                <span class="text-xs text-stone-500 dark:text-stone-400 ml-1">
                    "— from earlier days, still open"
                </span>
            </div>
            <div class="divide-y divide-amber-100 dark:divide-amber-900/30">
                {carryovers.into_iter().map(|MyDayTask { task, node_title }| {
                    view! {
                        <CarryoverRow
                            task=task
                            node_title=node_title
                            today=today
                            refresh=refresh
                        />
                    }
                }).collect_view()}
            </div>
        </div>
    }.into_any()
}

#[component]
fn CarryoverRow(
    task: Task,
    node_title: Option<String>,
    today: NaiveDate,
    refresh: RwSignal<u32>,
) -> impl IntoView {
    let task_id        = task.id;
    let from_date      = task.focus_date.unwrap_or(today);
    let from_label     = from_date.format("%b %-d").to_string();
    let title          = task.title.clone();
    let parent_label   = node_title.unwrap_or_else(|| "Inbox".to_string());

    let show_picker    = RwSignal::new(false);
    let picked_date    = RwSignal::new(today.format("%Y-%m-%d").to_string());
    let busy           = RwSignal::new(false);

    // Fire one PATCH /tasks/:id, refresh My Day, surface a brief toast.
    let patch_focus = move |new_focus: Option<NaiveDate>, success_msg: &'static str| {
        if busy.get_untracked() { return; }
        busy.set(true);
        let req = UpdateTaskRequest {
            title:      None,
            status:     None,
            priority:   None,
            focus_date: Some(new_focus),
            due_date:   None,
            recurrence: None,
            node_id:    None,
        };
        wasm_bindgen_futures::spawn_local(async move {
            let result = crate::api::update_task(task_id, &req).await;
            busy.set(false);
            match result {
                Ok(_) => {
                    push_toast(ToastLevel::Success, success_msg);
                    refresh.update(|n| *n += 1);
                }
                Err(e) => {
                    push_toast(ToastLevel::Error, format!("Couldn't update task: {e}"));
                }
            }
        });
    };

    let on_move_today = move |_| patch_focus(Some(today), "Moved to today");
    let on_drop       = move |_| patch_focus(None, "Dropped from My Day");
    let on_reschedule_apply = move |_| {
        let raw = picked_date.get_untracked();
        match raw.parse::<NaiveDate>() {
            Ok(d) if d == today => patch_focus(Some(d), "Moved to today"),
            Ok(d)                => patch_focus(Some(d), "Rescheduled"),
            Err(_) => push_toast(ToastLevel::Error, "Pick a valid date".to_string()),
        }
        show_picker.set(false);
    };

    view! {
        <div class="px-3 py-2">
            <div class="flex items-start gap-2">
                <span class="material-symbols-outlined text-amber-500 flex-shrink-0 mt-0.5"
                      style="font-size:14px;"
                      title=format!("Focused on {from_label}")>
                    "schedule"
                </span>
                <div class="flex-1 min-w-0">
                    <div class="text-sm text-stone-800 dark:text-stone-200 truncate">
                        {title}
                    </div>
                    <div class="text-xs text-stone-500 dark:text-stone-400 truncate">
                        {parent_label} " · from " {from_label.clone()}
                    </div>
                </div>
                <div class="flex items-center gap-1 flex-shrink-0">
                    <button
                        class="px-2 py-0.5 text-xs rounded bg-amber-600 text-white \
                               hover:bg-amber-700 disabled:opacity-50 cursor-pointer"
                        prop:disabled=move || busy.get()
                        on:click=on_move_today
                        title="Set focus date to today"
                    >
                        "Today"
                    </button>
                    <button
                        class="px-2 py-0.5 text-xs rounded border border-stone-300 \
                               dark:border-stone-600 text-stone-600 dark:text-stone-300 \
                               hover:bg-stone-100 dark:hover:bg-stone-800 \
                               disabled:opacity-50 cursor-pointer"
                        prop:disabled=move || busy.get()
                        on:click=move |_| show_picker.update(|v| *v = !*v)
                        title="Reschedule to a future date"
                    >
                        "Reschedule"
                    </button>
                    <button
                        class="p-1 rounded text-stone-400 hover:text-red-500 \
                               disabled:opacity-50 cursor-pointer"
                        prop:disabled=move || busy.get()
                        on:click=on_drop
                        title="Remove from My Day (back to Inbox / no focus date)"
                    >
                        <span class="material-symbols-outlined" style="font-size:16px;">"close"</span>
                    </button>
                </div>
            </div>
            <Show when=move || show_picker.get()>
                <div class="mt-2 flex items-center gap-2">
                    <input
                        type="date"
                        class="text-xs bg-stone-100 dark:bg-stone-700 \
                               text-stone-700 dark:text-stone-300 \
                               rounded px-2 py-0.5 focus:outline-none \
                               focus:ring-1 focus:ring-amber-500"
                        prop:value=move || picked_date.get()
                        on:input=move |ev| picked_date.set(event_target_value(&ev))
                    />
                    <button
                        class="px-2 py-0.5 text-xs rounded bg-amber-600 text-white \
                               hover:bg-amber-700 disabled:opacity-50 cursor-pointer"
                        prop:disabled=move || busy.get()
                        on:click=on_reschedule_apply
                    >
                        "Apply"
                    </button>
                    <button
                        class="px-2 py-0.5 text-xs rounded text-stone-500 \
                               hover:text-stone-800 dark:hover:text-stone-200 cursor-pointer"
                        on:click=move |_| show_picker.set(false)
                    >
                        "Cancel"
                    </button>
                </div>
            </Show>
        </div>
    }
}

// ── MyDayTaskRow ──────────────────────────────────────────────────────────────

#[component]
fn MyDayTaskRow(task: Task, refresh: RwSignal<u32>) -> impl IntoView {
    let task_id  = task.id;
    let node_id  = task.node_id;
    let today    = crate::components::format_helpers::local_today();

    let status_val   = RwSignal::new(status_value(&task.status).to_string());
    let priority_val = RwSignal::new(priority_value(&task.priority).to_string());

    let navigate = StoredValue::new(use_navigate());

    // Edit form state
    let editing         = RwSignal::new(false);
    let edit_title      = RwSignal::new(task.title.clone());
    let orig_title      = RwSignal::new(task.title.clone());
    let edit_due        = RwSignal::new(
        task.due_date
            .map(|d| d.format("%Y-%m-%d").to_string())
            .unwrap_or_default(),
    );
    let edit_priority   = RwSignal::new(priority_value(&task.priority).to_string());
    let edit_recurrence = RwSignal::new(
        task.recurrence
            .as_ref()
            .map(|r| recurrence_value(r).to_string())
            .unwrap_or_default(),
    );

    let do_save = move || {
        let new_title = edit_title.get_untracked().trim().to_string();
        if new_title.is_empty() { return; }
        let new_priority  = parse_priority(&edit_priority.get_untracked());
        let new_recurrence_str = edit_recurrence.get_untracked();
        let new_recurrence = parse_recurrence_opt(&new_recurrence_str);
        editing.set(false);
        orig_title.set(new_title.clone());
        priority_val.set(priority_value(&new_priority).to_string());
        let new_due: Option<Option<NaiveDate>> =
            Some(edit_due.get_untracked().trim().parse::<NaiveDate>().ok());
        let req = UpdateTaskRequest {
            title:      Some(new_title),
            status:     None,
            priority:   Some(new_priority),
            focus_date: None,
            due_date:   new_due,
            recurrence: Some(new_recurrence),
            node_id:    None,
        };
        wasm_bindgen_futures::spawn_local(async move {
            let _ = crate::api::update_task(task_id, &req).await;
            refresh.update(|n| *n += 1);
        });
    };

    let on_toggle = move |_| {
        let current = status_val.get_untracked();
        let next    = if current == "done" { "open" } else { "done" };
        let req = UpdateTaskRequest {
            title: None, status: Some(parse_status(next)),
            priority: None, focus_date: None, due_date: None, recurrence: None, node_id: None,
        };
        status_val.set(next.to_string());
        wasm_bindgen_futures::spawn_local(async move {
            let _ = crate::api::update_task(task_id, &req).await;
            refresh.update(|n| *n += 1);
        });
    };

    let on_remove = move |_| {
        let req = UpdateTaskRequest {
            title: None, status: None, priority: None,
            focus_date: Some(None), due_date: None, recurrence: None, node_id: None,
        };
        wasm_bindgen_futures::spawn_local(async move {
            let _ = crate::api::update_task(task_id, &req).await;
            refresh.update(|n| *n += 1);
        });
    };

    let on_delete = move |_| {
        wasm_bindgen_futures::spawn_local(async move {
            let _ = crate::api::delete_task(task_id).await;
            refresh.update(|n| *n += 1);
        });
    };

    let overdue      = task.due_date
        .map(|d| !status_done(&task.status) && d < today)
        .unwrap_or(false);
    let due          = task.due_date;
    let carried_from = task.focus_date.filter(|&d| d < today);
    let has_recurrence = task.recurrence.is_some();
    let recurrence_tip = task.recurrence.as_ref().map(|r| recurrence_label(r));

    view! {
        <div class="group flex items-start gap-2 py-2 px-3 rounded-lg
            hover:bg-stone-50 dark:hover:bg-stone-800/50 transition-colors cursor-grab active:cursor-grabbing">

            // Checkbox
            <button
                class="flex-shrink-0 mt-0.5 w-5 h-5 rounded border-2 border-stone-300
                    dark:border-stone-600 flex items-center justify-center
                    hover:border-amber-500 transition-colors cursor-pointer"
                style=move || if status_done(&parse_status(&status_val.get())) {
                    "background:#d97706;border-color:#d97706;"
                } else { "" }
                on:click=on_toggle
                title="Toggle done"
            >
                {move || status_done(&parse_status(&status_val.get())).then(|| view! {
                    <span class="material-symbols-outlined text-white" style="font-size:13px;">"check"</span>
                })}
            </button>

            // Priority dot — High=red, Medium=amber, Low=hidden
            {move || match parse_priority(&priority_val.get()) {
                common::task::TaskPriority::High => Some(view! {
                    <span
                        class="flex-shrink-0 mt-1.5"
                        style="color:#ef4444;font-size:8px;line-height:1;"
                        title="High priority"
                    >{"●"}</span>
                }),
                common::task::TaskPriority::Medium => Some(view! {
                    <span
                        class="flex-shrink-0 mt-1.5"
                        style="color:#f59e0b;font-size:8px;line-height:1;"
                        title="Medium priority"
                    >{"●"}</span>
                }),
                common::task::TaskPriority::Low => None,
            }}

            // Body
            <div class="flex-1 min-w-0">
                {move || if editing.get() {
                    // ── Edit form ─────────────────────────────────────────────
                    view! {
                        <div class="space-y-2 pb-1">
                            // Title input
                            <input
                                type="text"
                                class="w-full bg-stone-100 dark:bg-stone-800 text-sm
                                    text-stone-900 dark:text-stone-100 rounded px-2 py-1
                                    focus:outline-none focus:ring-1 focus:ring-amber-500"
                                prop:value=move || edit_title.get()
                                on:input=move |ev| edit_title.set(event_target_value(&ev))
                                on:keydown=move |ev: leptos::ev::KeyboardEvent| {
                                    match ev.key().as_str() {
                                        "Enter"  => do_save(),
                                        "Escape" => {
                                            editing.set(false);
                                            edit_title.set(orig_title.get_untracked());
                                        }
                                        _ => {}
                                    }
                                }
                            />
                            // Priority selector
                            <div class="flex items-center gap-1.5 flex-wrap">
                                <span class="text-xs text-stone-400 dark:text-stone-500">"Priority"</span>
                                {["low", "medium", "high"].iter().map(|&p| {
                                    let (label, sel_style) = match p {
                                        "high"   => ("High",   "color:#ef4444;border-color:#ef4444;"),
                                        "medium" => ("Medium", "color:#f59e0b;border-color:#f59e0b;"),
                                        _        => ("Low",    "color:#9ca3af;border-color:#9ca3af;"),
                                    };
                                    view! {
                                        <button
                                            class="text-xs px-2 py-0.5 rounded border transition-colors cursor-pointer"
                                            style=move || if edit_priority.get() == p {
                                                format!("{sel_style}font-weight:600;")
                                            } else {
                                                "color:#9ca3af;border-color:#d1d5db;".to_string()
                                            }
                                            on:click=move |_| edit_priority.set(p.to_string())
                                        >
                                            {label}
                                        </button>
                                    }
                                }).collect_view()}
                            </div>
                            // Due date + Recurrence row
                            <div class="flex items-center gap-2 flex-wrap">
                                <input
                                    type="date"
                                    class="text-xs bg-stone-100 dark:bg-stone-700
                                        text-stone-700 dark:text-stone-300
                                        rounded px-2 py-0.5 focus:outline-none
                                        focus:ring-1 focus:ring-amber-500"
                                    title="Due date (optional)"
                                    prop:value=move || edit_due.get()
                                    on:input=move |ev| edit_due.set(event_target_value(&ev))
                                />
                                // Recurrence selector
                                <select
                                    class="text-xs bg-stone-100 dark:bg-stone-700
                                        text-stone-700 dark:text-stone-300
                                        rounded px-2 py-0.5 focus:outline-none
                                        focus:ring-1 focus:ring-amber-500"
                                    title="Recurrence"
                                    on:change=move |ev| edit_recurrence.set(event_target_value(&ev))
                                >
                                    <option value="" selected=move || edit_recurrence.get().is_empty()>
                                        "No repeat"
                                    </option>
                                    {[
                                        ("daily",    "Daily"),
                                        ("weekly",   "Weekly"),
                                        ("biweekly", "Every 2 weeks"),
                                        ("monthly",  "Monthly"),
                                        ("yearly",   "Yearly"),
                                    ].iter().map(|&(val, label)| {
                                        view! {
                                            <option value=val selected=move || edit_recurrence.get() == val>
                                                {label}
                                            </option>
                                        }
                                    }).collect_view()}
                                </select>
                                <span class="flex-1"/>
                                <button
                                    class="p-1.5 rounded-lg text-stone-400 hover:text-green-600 dark:hover:text-green-400
                                        hover:bg-green-50 dark:hover:bg-green-900/30 transition-colors cursor-pointer"
                                    title="Save"
                                    on:click=move |_| do_save()
                                >
                                    <span class="material-symbols-outlined">"check"</span>
                                </button>
                                <button
                                    class="p-1.5 rounded-lg text-stone-400 hover:text-stone-600 dark:hover:text-stone-300
                                        hover:bg-stone-100 dark:hover:bg-stone-800 transition-colors cursor-pointer"
                                    title="Cancel"
                                    on:click=move |_| {
                                        editing.set(false);
                                        edit_title.set(orig_title.get_untracked());
                                    }
                                >
                                    <span class="material-symbols-outlined">"close"</span>
                                </button>
                            </div>
                        </div>
                    }.into_any()
                } else {
                    // ── Display row ───────────────────────────────────────────
                    view! {
                        <div class="flex items-center gap-2 flex-wrap">
                            // Title — click navigates to the parent node
                            <span
                                class="flex-1 min-w-0 text-sm text-stone-800 dark:text-stone-200
                                    cursor-pointer hover:text-amber-600 dark:hover:text-amber-400
                                    transition-colors truncate"
                                style=move || if status_done(&parse_status(&status_val.get())) {
                                    "text-decoration:line-through;opacity:0.45;"
                                } else { "" }
                                on:click=move |_| {
                                    if let Some(nid) = node_id {
                                        navigate.get_value()(&format!("/nodes/{nid}"), Default::default());
                                    }
                                }
                                title="Open parent node"
                            >
                                {move || orig_title.get()}
                            </span>

                            // Recurrence badge
                            {has_recurrence.then(|| {
                                let tip = recurrence_tip.unwrap_or("");
                                view! {
                                    <span
                                        class="flex-shrink-0 text-stone-400 dark:text-stone-500"
                                        title=format!("Repeats: {tip}")
                                    >
                                        <span class="material-symbols-outlined" style="font-size:13px;">"repeat"</span>
                                    </span>
                                }
                            })}

                            // Carried-over badge
                            {carried_from.map(|d| view! {
                                <span
                                    class="flex items-center gap-0.5 text-stone-400
                                        dark:text-stone-500 flex-shrink-0"
                                    style="font-size:11px;"
                                    title=format!("Carried over from {}", d.format("%b %-d"))
                                >
                                    <span class="material-symbols-outlined" style="font-size:12px;">"history"</span>
                                    {d.format("%b %-d").to_string()}
                                </span>
                            })}

                            // Due date
                            {due.map(|d| {
                                let style = if overdue {
                                    "color:#dc2626;font-size:11px;font-weight:600;"
                                } else {
                                    "color:#9ca3af;font-size:11px;"
                                };
                                let label = if overdue {
                                    format!("⚠ {}", d.format("%b %-d"))
                                } else {
                                    d.format("%b %-d").to_string()
                                };
                                view! {
                                    <span style=style class="flex-shrink-0">{label}</span>
                                }
                            })}

                            // Actions — always visible
                            <div class="flex items-center gap-0.5 flex-shrink-0">
                                <button
                                    class="p-1 rounded text-stone-300 dark:text-stone-600
                                        hover:text-stone-600 dark:hover:text-stone-300
                                        transition-colors cursor-pointer"
                                    title="Edit task"
                                    on:click=move |_| editing.set(true)
                                >
                                    <span class="material-symbols-outlined" style="font-size:15px;">"edit"</span>
                                </button>
                                <button
                                    class="p-1 rounded text-amber-300 dark:text-amber-700
                                        hover:text-amber-500 transition-colors cursor-pointer"
                                    title="Remove from My Day"
                                    on:click=on_remove
                                >
                                    <span class="material-symbols-outlined" style="font-size:15px;">"wb_sunny"</span>
                                </button>
                                <button
                                    class="p-1 rounded text-stone-300 dark:text-stone-600
                                        hover:text-red-500 transition-colors cursor-pointer"
                                    title="Delete task"
                                    on:click=on_delete
                                >
                                    <span class="material-symbols-outlined" style="font-size:15px;">"delete"</span>
                                </button>
                            </div>
                        </div>
                    }.into_any()
                }}
            </div>
        </div>
    }
}
