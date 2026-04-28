use chrono::NaiveDate;
use common::{
    id::{NodeId, TaskId},
    task::{CreateTaskRequest, Task, TaskStatus, UpdateTaskRequest},
};
use leptos::prelude::*;

use crate::app::TaskRefresh;
use crate::components::task_common::{
    is_in_my_day, parse_priority, parse_recurrence_opt, parse_status, priority_color,
    priority_icon, priority_label, priority_value, sort_tasks_by_order, status_done, status_label,
    status_value,
};

// ── TaskPanel ─────────────────────────────────────────────────────────────────

#[component]
pub fn TaskPanel(node_id: NodeId) -> impl IntoView {
    let task_refresh = use_context::<TaskRefresh>()
        .expect("TaskRefresh context must be provided")
        .0;

    let tasks_resource = LocalResource::new(move || {
        let _ = task_refresh.get();
        async move { crate::api::fetch_tasks(node_id).await }
    });

    // Filter: hide done/cancelled tasks by default; toggled by the user.
    let show_completed = RwSignal::new(false);

    // Drag-to-reorder state — open tasks are kept in a local signal so
    // optimistic reorders survive the next resource refetch.
    let open_tasks: RwSignal<Vec<Task>> = RwSignal::new(Vec::new());
    let done_tasks: RwSignal<Vec<Task>> = RwSignal::new(Vec::new());
    let drag_src: RwSignal<Option<usize>> = RwSignal::new(None);
    let drag_over: RwSignal<Option<usize>> = RwSignal::new(None);

    // Sync open/done buckets whenever the resource (re-)loads.
    Effect::new(move |_| {
        let all = tasks_resource.get()
            .and_then(|r| r.ok())
            .unwrap_or_default();
        let (mut open, done): (Vec<_>, Vec<_>) =
            all.into_iter().partition(|t| !status_done(&t.status));
        sort_tasks_by_order(&mut open);
        open_tasks.set(open);
        done_tasks.set(done);
    });

    // New task form state
    let new_title      = RwSignal::new(String::new());
    let new_priority   = RwSignal::new("medium".to_string());
    let new_due        = RwSignal::new(String::new());
    let new_recurrence = RwSignal::new(String::new());
    let adding         = RwSignal::new(false);
    let show_form      = RwSignal::new(false);
    let add_error      = RwSignal::new(Option::<String>::None);

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
        adding.set(true);
        add_error.set(None);
        wasm_bindgen_futures::spawn_local(async move {
            let req = CreateTaskRequest {
                title,
                node_id: None,
                status: None,
                priority: Some(priority),
                focus_date: None,
                due_date,
                recurrence,
            };
            match crate::api::create_task(node_id, &req).await {
                Ok(_) => {
                    new_title.set(String::new());
                    new_priority.set("medium".to_string());
                    new_due.set(String::new());
                    new_recurrence.set(String::new());
                    show_form.set(false);
                    task_refresh.update(|n| *n += 1);
                }
                Err(e) => {
                    add_error.set(Some(format!("{e}")));
                }
            }
            adding.set(false);
        });
    };
    let on_add = move |_| do_add();

    view! {
        <div class="mt-8 border-t border-stone-200 dark:border-stone-700 pt-6">
            // Section header
            <div class="flex items-center justify-between mb-4">
                <h3 class="text-sm font-semibold text-stone-500 dark:text-stone-400 uppercase tracking-wider flex items-center gap-2">
                    <span class="material-symbols-outlined text-base">{"check_box"}</span>
                    "Tasks"
                </h3>
                <button
                    class="p-1.5 rounded-lg text-stone-400 hover:text-stone-600 dark:hover:text-stone-300
                        hover:bg-stone-100 dark:hover:bg-stone-800 transition-colors cursor-pointer"
                    title=move || if show_form.get() { "Cancel" } else { "Add task" }
                    on:click=move |_| show_form.update(|v| *v = !*v)
                >
                    <span class="material-symbols-outlined" style="font-size: 18px;">
                        {move || if show_form.get() { "close" } else { "add" }}
                    </span>
                </button>
            </div>

            // Add-task form
            {move || show_form.get().then(|| view! {
                <div class="mb-4 p-3 rounded-lg bg-stone-50 dark:bg-stone-800/50
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
                            <option value="medium" selected>"Medium"</option>
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
                            on:change=move |ev| new_recurrence.set(event_target_value(&ev))
                        >
                            <option value="" selected>"No repeat"</option>
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
                            on:click=on_add
                            disabled=move || adding.get()
                        >
                            <span class="material-symbols-outlined">
                                {move || if adding.get() { "hourglass_empty" } else { "add" }}
                            </span>
                        </button>
                    </div>
                    {move || add_error.get().map(|msg| view! {
                        <p class="text-xs text-red-500">{msg}</p>
                    })}
                </div>
            })}

            // Task list ──────────────────────────────────────────────────────
            // Empty state
            {move || (open_tasks.get().is_empty() && done_tasks.get().is_empty()).then(|| view! {
                <p class="text-sm text-stone-400 dark:text-stone-500 italic">
                    "No tasks yet."
                </p>
            })}

            // Open tasks — draggable for reordering
            {move || open_tasks.get().into_iter().enumerate().map(|(idx, task)| view! {
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
                            open_tasks.update(|list| {
                                let moved = list.remove(src);
                                let dst = if src < idx { idx - 1 } else { idx };
                                list.insert(dst.min(list.len()), moved);
                            });
                            let updates: Vec<(TaskId, i32)> = open_tasks
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
                    <TaskRow task=task task_refresh=task_refresh />
                </div>
            }).collect_view()}

            // Completed tasks (revealed on demand) + disclosure / clear row
            {move || {
                let done = done_tasks.get();
                let done_count = done.len();
                if done_count == 0 { return ().into_any(); }

                let showing = show_completed.get();
                let done_ids: Vec<TaskId> = done.iter().map(|t| t.id).collect();
                let label = if showing {
                    "Hide completed".to_string()
                } else {
                    format!("{done_count} completed · show")
                };
                let icon = if showing { "expand_less" } else { "expand_more" };
                let ids_for_clear = done_ids.clone();

                view! {
                    // Completed rows
                    {showing.then(|| done.into_iter().map(|task| view! {
                        <TaskRow task=task task_refresh=task_refresh />
                    }).collect_view())}

                    // Disclosure + clear-all bar
                    <div class="mt-2 flex items-center gap-3">
                        <button
                            class="flex items-center gap-1 text-xs
                                   text-stone-400 dark:text-stone-500
                                   hover:text-stone-600 dark:hover:text-stone-300
                                   transition-colors cursor-pointer"
                            on:click=move |_| show_completed.update(|v| *v = !*v)
                        >
                            <span class="material-symbols-outlined"
                                  style="font-size: 14px;">{icon}</span>
                            {label}
                        </button>
                        <button
                            class="text-xs text-stone-300 dark:text-stone-600
                                   hover:text-red-500 dark:hover:text-red-400
                                   transition-colors cursor-pointer"
                            title="Delete all completed tasks"
                            on:click=move |_| {
                                let ids = ids_for_clear.clone();
                                wasm_bindgen_futures::spawn_local(async move {
                                    for id in ids {
                                        let _ = crate::api::delete_task(id).await;
                                    }
                                    task_refresh.update(|n| *n += 1);
                                });
                            }
                        >
                            "Clear all"
                        </button>
                    </div>
                }.into_any()
            }}
        </div>
    }
}

// ── TaskRow ───────────────────────────────────────────────────────────────────

#[component]
fn TaskRow(task: Task, task_refresh: RwSignal<u32>) -> impl IntoView {
    let task_id = task.id;
    let is_done = status_done(&task.status);
    let status_val = status_value(&task.status).to_string();
    let priority = task.priority.clone();
    let p_icon = priority_icon(&task.priority);
    let p_color = priority_color(&task.priority);
    let p_label = priority_label(&task.priority);
    let due = task.due_date;

    let today = crate::components::format_helpers::local_today();
    let overdue = due.map(|d| !is_done && d < today).unwrap_or(false);
    let in_my_day = RwSignal::new(is_in_my_day(&task, today));

    let status_sig = RwSignal::new(status_val.clone());

    // Inline editing — title, priority, due date.
    let editing_title  = RwSignal::new(false);
    let edit_title     = RwSignal::new(task.title.clone());
    let orig_title     = RwSignal::new(task.title.clone());
    let edit_priority  = RwSignal::new(priority_value(&priority).to_string());
    let edit_due       = RwSignal::new(
        due.map(|d| d.format("%Y-%m-%d").to_string())
            .unwrap_or_default(),
    );

    // Save all editable fields at once.
    let do_save_edit = move || {
        let new_title = edit_title.get_untracked().trim().to_string();
        if new_title.is_empty() { return; }
        editing_title.set(false);
        let new_priority = Some(parse_priority(&edit_priority.get_untracked()));
        let new_due: Option<Option<NaiveDate>> = Some(
            edit_due.get_untracked().trim().parse::<NaiveDate>().ok()
        );
        let req = UpdateTaskRequest {
            title: Some(new_title),
            status: None,
            priority: new_priority,
            focus_date: None,
            due_date: new_due,
            recurrence: None,
            node_id: None,
        };
        wasm_bindgen_futures::spawn_local(async move {
            let _ = crate::api::update_task(task_id, &req).await;
            task_refresh.update(|n| *n += 1);
        });
    };

    // Toggle done ↔ open on checkbox click
    let on_toggle = move |_| {
        let current = status_sig.get_untracked();
        let next = if current == "done" { "open" } else { "done" };
        let next_status = parse_status(next);
        let req = UpdateTaskRequest {
            title: None,
            status: Some(next_status),
            priority: None,
            focus_date: None,
            due_date: None,
            recurrence: None,
            node_id: None,
        };
        status_sig.set(next.to_string());
        wasm_bindgen_futures::spawn_local(async move {
            let _ = crate::api::update_task(task_id, &req).await;
            task_refresh.update(|n| *n += 1);
        });
    };

    // Toggle My Day
    let on_toggle_focus = move |_| {
        let currently = in_my_day.get_untracked();
        let new_focus = if currently { None } else { Some(today) };
        in_my_day.set(!currently); // optimistic UI update
        let req = UpdateTaskRequest {
            title: None,
            status: None,
            priority: None,
            focus_date: Some(new_focus),
            due_date: None,
            recurrence: None,
            node_id: None,
        };
        wasm_bindgen_futures::spawn_local(async move {
            let _ = crate::api::update_task(task_id, &req).await;
            task_refresh.update(|n| *n += 1);
        });
    };

    // Delete
    let on_delete = move |_| {
        wasm_bindgen_futures::spawn_local(async move {
            let _ = crate::api::delete_task(task_id).await;
            task_refresh.update(|n| *n += 1);
        });
    };

    let title_display  = task.title.clone();
    let has_recurrence = task.recurrence.is_some();

    view! {
        <div
            class="group flex items-start gap-2 py-2 border-b border-stone-100 dark:border-stone-800 last:border-0"
            data-task-id=task_id.0.to_string()
        >
            // Checkbox
            <button
                class="mt-0.5 flex-shrink-0 w-4 h-4 rounded border border-stone-400 dark:border-stone-500
                    flex items-center justify-center transition-colors
                    hover:border-amber-500"
                style=move || if status_done(&parse_status(&status_sig.get())) {
                    "background: #d97706; border-color: #d97706;"
                } else { "" }
                on:click=on_toggle
                title="Toggle done"
            >
                {move || status_done(&parse_status(&status_sig.get())).then(|| view! {
                    <span class="material-symbols-outlined text-white"
                        style="font-size: 12px;">{"check"}</span>
                })}
            </button>

            // Title + meta
            <div class="flex-1 min-w-0">
                // Title — display or inline edit form (title + priority + due date)
                {move || if editing_title.get() {
                    view! {
                        <div class="space-y-1.5 pb-1">
                            <input
                                type="text"
                                class="w-full bg-stone-100 dark:bg-stone-800 text-sm
                                    text-stone-900 dark:text-stone-100 rounded px-1.5 py-0.5
                                    focus:outline-none focus:ring-1 focus:ring-amber-500"
                                prop:value=move || edit_title.get()
                                on:input=move |ev| edit_title.set(event_target_value(&ev))
                                on:keydown=move |ev: leptos::ev::KeyboardEvent| {
                                    match ev.key().as_str() {
                                        "Enter" => do_save_edit(),
                                        "Escape" => {
                                            editing_title.set(false);
                                            edit_title.set(orig_title.get_untracked());
                                        }
                                        _ => {}
                                    }
                                }
                            />
                            <div class="flex items-center gap-2">
                                <select
                                    class="text-xs bg-stone-100 dark:bg-stone-700
                                        text-stone-700 dark:text-stone-300
                                        rounded px-2 py-0.5 focus:outline-none"
                                    prop:value=move || edit_priority.get()
                                    on:change=move |ev| edit_priority.set(event_target_value(&ev))
                                >
                                    <option value="high">"High"</option>
                                    <option value="medium">"Medium"</option>
                                    <option value="low">"Low"</option>
                                </select>
                                <input
                                    type="date"
                                    class="text-xs bg-stone-100 dark:bg-stone-700
                                        text-stone-700 dark:text-stone-300
                                        rounded px-2 py-0.5 focus:outline-none"
                                    title="Due date (optional)"
                                    prop:value=move || edit_due.get()
                                    on:input=move |ev| edit_due.set(event_target_value(&ev))
                                />
                                <span class="flex-1"/>
                                <button
                                    class="p-1.5 rounded-lg text-stone-400 hover:text-green-600 dark:hover:text-green-400
                                        hover:bg-green-50 dark:hover:bg-green-900/30 transition-colors cursor-pointer"
                                    title="Save"
                                    on:click=move |_| do_save_edit()
                                >
                                    <span class="material-symbols-outlined">"check"</span>
                                </button>
                                <button
                                    class="p-1.5 rounded-lg text-stone-400 hover:text-stone-600 dark:hover:text-stone-300
                                        hover:bg-stone-100 dark:hover:bg-stone-800 transition-colors cursor-pointer"
                                    title="Cancel"
                                    on:click=move |_| {
                                        editing_title.set(false);
                                        edit_title.set(orig_title.get_untracked());
                                    }
                                >
                                    <span class="material-symbols-outlined">"close"</span>
                                </button>
                            </div>
                        </div>
                    }.into_any()
                } else {
                    let td = title_display.clone();
                    view! {
                        <p
                            class="text-sm text-stone-800 dark:text-stone-200 leading-snug"
                            style=move || if status_done(&parse_status(&status_sig.get())) {
                                "text-decoration: line-through; opacity: 0.5;"
                            } else { "" }
                        >
                            {td}
                        </p>
                    }.into_any()
                }}
                <div class="flex items-center gap-2 mt-0.5">
                    // Priority badge
                    <span class="flex items-center gap-0.5 text-xs" style=p_color
                        title=p_label>
                        <span class="material-symbols-outlined" style="font-size:12px;">{p_icon}</span>
                        {p_label}
                    </span>
                    // Due date
                    {due.map(|d| {
                        let due_style = if overdue {
                            "color: #dc2626; font-weight: 600;"
                        } else {
                            "color: #6b7280;"
                        };
                        view! {
                            <span class="flex items-center gap-0.5 text-xs" style=due_style>
                                <span class="material-symbols-outlined" style="font-size:12px;">{"event"}</span>
                                {d.format("%b %-d").to_string()}
                                {overdue.then_some(" · overdue")}
                            </span>
                        }
                    })}
                    // Recurrence badge
                    {has_recurrence.then(|| view! {
                        <span class="flex items-center gap-0.5 text-xs text-stone-400 dark:text-stone-500"
                            title="Recurring task">
                            <span class="material-symbols-outlined" style="font-size:12px;">"repeat"</span>
                        </span>
                    })}
                    // Status badge (if not open/done)
                    {(!matches!(parse_status(&status_val), TaskStatus::Open | TaskStatus::Done)).then(|| {
                        let lbl = status_label(&parse_status(&status_val));
                        view! {
                            <span class="text-xs text-stone-400 dark:text-stone-500">{lbl}</span>
                        }
                    })}
                </div>
            </div>

            // Actions — always visible
            <div class="flex items-center gap-1 flex-shrink-0">
                // Edit task
                <button
                    class="p-1 rounded text-stone-300 dark:text-stone-600 hover:text-amber-500 dark:hover:text-amber-400 transition-colors"
                    title="Edit task"
                    on:click=move |_| editing_title.set(true)
                >
                    <span class="material-symbols-outlined" style="font-size:16px;">{"edit"}</span>
                </button>
                // My Day toggle
                <button
                    class="p-1 rounded transition-colors"
                    style=move || if in_my_day.get() { "color: #d97706;" } else { "color: #d6d3d1;" }
                    title=move || if in_my_day.get() { "Remove from My Day" } else { "Add to My Day" }
                    on:click=on_toggle_focus
                >
                    <span class="material-symbols-outlined" style="font-size:16px;">{"wb_sunny"}</span>
                </button>
                // Delete
                <button
                    class="p-1 rounded text-stone-300 dark:text-stone-600 hover:text-red-500 dark:hover:text-red-400 transition-colors"
                    title="Delete task"
                    on:click=on_delete
                >
                    <span class="material-symbols-outlined" style="font-size:16px;">{"delete"}</span>
                </button>
            </div>
        </div>
    }
}
