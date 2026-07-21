//! Inbox — standalone tasks not yet associated with any node.
//!
//! Shows all tasks where `node_id IS NULL`, lets the user create new ones
//! inline, and provides the standard toggle/edit/delete/My-Day actions.
//!
//! Layout is mobile-first: each task renders as a self-contained card with
//! an always-visible action bar so controls are reachable without hover.

use chrono::NaiveDate;
use common::{
    id::NodeId,
    search::SearchResult,
    task::{Task, UpdateTaskRequest},
};
use leptos::prelude::*;

use crate::app::TaskRefresh;
use crate::components::icon_button::{IconButton, IconButtonVariant};
use crate::components::new_task_form::NewTaskForm;
use crate::components::task_common::{
    is_in_my_day, node_type_icon, parse_priority, parse_recurrence_opt, parse_status,
    priority_value, recurrence_label, recurrence_value, status_done, status_value,
    undo_restore_task,
};
use crate::components::task_row_scaffold::{
    CHECKBOX_CLASS, TITLE_CLASS, TaskRowBody, action_btn_class, due_badge_view, priority_dot_view,
};
use crate::components::toast::{ToastLevel, ToastState, push_toast, push_undo_toast};
use crate::focus_task::schedule_focus_task;

// ── InboxView ─────────────────────────────────────────────────────────────────

#[component]
pub fn InboxView() -> impl IntoView {
    let task_refresh = expect_context::<TaskRefresh>();
    let refresh = task_refresh.0;

    // If we got here via the iOS Web Share Target SW handler (which 303s to
    // /tasks/inbox?captured=1) or the home-screen "Quick capture" shortcut,
    // confirm the capture with a toast and strip the marker from the URL so
    // a refresh doesn't re-fire it.  Runs once per mount.
    Effect::new(move |run_count: Option<()>| {
        if run_count.is_some() {
            return;
        }
        let Some(win) = web_sys::window() else {
            return;
        };
        let Ok(href) = win.location().href() else {
            return;
        };
        if let Ok(url) = web_sys::Url::new(&href) {
            let params = url.search_params();
            if params.get("captured").as_deref() == Some("1") {
                push_toast(ToastLevel::Success, "Captured to Inbox");
                refresh.update(|n| *n += 1);
                params.delete("captured");
                url.set_search(&params.to_string().as_string().unwrap_or_default());
                let new_href = format!(
                    "{}{}{}",
                    url.pathname(),
                    if url.search().is_empty() { "" } else { "?" },
                    url.search().trim_start_matches('?')
                );
                if let Ok(history) = win.history() {
                    let _ = history.replace_state_with_url(
                        &leptos::wasm_bindgen::JsValue::NULL,
                        "",
                        Some(&new_href),
                    );
                }
            }
        }
    });

    // v2.6.2: when navigation arrived via the Kanban row click on a
    // standalone (Inbox) task, scroll to and briefly highlight the
    // matching row.  See `crate::focus_task`.
    schedule_focus_task();

    let tasks_res = LocalResource::new(move || {
        let _ = refresh.get();
        async move { crate::api::list_inbox().await }
    });

    // Per-task saved inline-edit heights, provided to the InboxTaskRows.
    let editor_heights =
        RwSignal::<std::collections::HashMap<uuid::Uuid, i32>>::new(Default::default());
    provide_context(crate::components::task_row::TaskEditorHeights(
        editor_heights,
    ));
    wasm_bindgen_futures::spawn_local(async move {
        if let Ok(prefs) = crate::api::fetch_editor_prefs().await {
            editor_heights.set(
                prefs
                    .into_iter()
                    .filter(|p| p.entity_kind == "task")
                    .map(|p| (p.entity_id, p.height))
                    .collect(),
            );
        }
    });

    // Triage ("Process") mode — one task at a time, keyboard-driven.
    let triage = RwSignal::new(false);

    view! {
        <div class="flex flex-col h-full">
            // ── Header ────────────────────────────────────────────────────────
            <div class="flex-shrink-0 px-4 py-4 border-b border-stone-200 dark:border-stone-800">
                <div class="flex items-center justify-between gap-3">
                    <div class="flex items-center gap-3">
                        <span class="material-symbols-outlined text-amber-500" style="font-size: 26px;">
                            "inbox"
                        </span>
                        <div>
                            <h1 class="text-xl font-semibold text-stone-900 dark:text-stone-100">
                                "Inbox"
                            </h1>
                            <p class="text-xs text-stone-500 dark:text-stone-400">
                                "Capture tasks — link to a node when ready"
                            </p>
                        </div>
                    </div>
                    {move || (!triage.get()).then(|| view! {
                        <button
                            class="flex items-center gap-1.5 text-sm px-3 py-1.5 rounded-lg
                                   border border-stone-200 dark:border-stone-700
                                   text-stone-600 dark:text-stone-300
                                   hover:border-amber-400 hover:text-amber-600 dark:hover:text-amber-400
                                   transition-colors cursor-pointer"
                            title="Process the inbox one task at a time (keyboard-driven)"
                            on:click=move |_| triage.set(true)
                        >
                            <span class="material-symbols-outlined" style="font-size:16px;">"playlist_add_check"</span>
                            "Process"
                        </button>
                    })}
                </div>
            </div>

            // ── Scrollable content ────────────────────────────────────────────
            <div class="flex-1 overflow-auto px-4 py-4 space-y-4">

                {move || triage.get().then(|| view! {
                    <crate::components::inbox_triage::InboxTriage
                        refresh=refresh
                        on_exit=Callback::new(move |()| triage.set(false))
                    />
                })}

                <div class=move || if triage.get() { "hidden" } else { "contents" }>
                // ── Add-task form — shared with the node-detail TaskPanel ─────
                // node_id omitted ⇒ standalone capture with the optional picker.
                <NewTaskForm refresh=refresh />

                // ── Task list ─────────────────────────────────────────────────
                <Suspense fallback=|| view! {
                    <div class="px-4">
                        <crate::components::skeleton::SkeletonList rows=6 />
                    </div>
                }>
                    {move || {
                        let tasks = tasks_res.get()
                            .and_then(|r| r.ok())
                            .unwrap_or_default();
                        if tasks.is_empty() {
                            return view! {
                                <div class="text-center py-16 space-y-2">
                                    <span class="material-symbols-outlined text-stone-300
                                        dark:text-stone-600" style="font-size: 48px;">
                                        "check_circle"
                                    </span>
                                    <p class="text-stone-400 dark:text-stone-500 text-sm">
                                        "Inbox zero!"
                                    </p>
                                </div>
                            }.into_any();
                        }
                        let (active, done): (Vec<Task>, Vec<Task>) =
                            tasks.into_iter().partition(|t| !status_done(&t.status));
                        let done_count  = done.len();
                        let show_done   = RwSignal::new(false);
                        let done_stored = StoredValue::new(done);
                        view! {
                            // One bordered container of flat rows (divide-y),
                            // matching the My Day zone-box look.
                            <div class="rounded-xl border border-stone-100 dark:border-stone-800 \
                                        bg-white dark:bg-stone-900 \
                                        divide-y divide-stone-100 dark:divide-stone-800 \
                                        overflow-hidden">
                                {active.into_iter().map(|task| view! {
                                    <InboxTaskRow task=task refresh=refresh />
                                }).collect_view()}

                                // Completed section toggle
                                {(done_count > 0).then(|| view! {
                                    <button
                                        class="w-full flex items-center gap-1.5 px-2 py-2 mt-1
                                            text-xs text-stone-400 hover:text-stone-600
                                            dark:hover:text-stone-300 transition-colors cursor-pointer"
                                        on:click=move |_| show_done.update(|v| *v = !*v)
                                    >
                                        <span class="material-symbols-outlined" style="font-size: 14px;">
                                            {move || if show_done.get() { "expand_more" } else { "chevron_right" }}
                                        </span>
                                        {move || if show_done.get() {
                                            format!("Hide {done_count} completed")
                                        } else {
                                            format!("{done_count} completed")
                                        }}
                                    </button>
                                    {move || show_done.get().then(|| {
                                        done_stored.get_value().into_iter().map(|task| view! {
                                            <InboxTaskRow task=task refresh=refresh />
                                        }).collect_view()
                                    })}
                                })}
                            </div>
                        }.into_any()
                    }}
                </Suspense>
                </div>  // close triage-hidden wrapper
            </div>
        </div>
    }
}

// ── InboxTaskRow ──────────────────────────────────────────────────────────────

#[component]
fn InboxTaskRow(task: Task, refresh: RwSignal<u32>) -> impl IntoView {
    // Captured at setup for the undo closure, which outlives this row.
    let toast_state = use_context::<ToastState>();
    let task_id = task.id;
    let today = crate::components::format_helpers::local_today();

    let status_val = RwSignal::new(status_value(&task.status).to_string());
    let priority_val = RwSignal::new(priority_value(&task.priority).to_string());

    // Inline-edit state
    let editing = RwSignal::new(false);
    let orig_title = RwSignal::new(task.title.clone());
    let edit_title = RwSignal::new(task.title.clone());
    let edit_priority = RwSignal::new(priority_value(&task.priority).to_string());
    let edit_due = RwSignal::new(
        task.due_date
            .map(|d| d.format("%Y-%m-%d").to_string())
            .unwrap_or_default(),
    );
    let edit_recurrence = RwSignal::new(
        task.recurrence
            .as_ref()
            .map(|r| recurrence_value(r).to_string())
            .unwrap_or_default(),
    );

    // My Day toggle — mirrors server carry-forward so tasks set on a
    // previous day but still open still read as "in My Day" today.
    let in_my_day = RwSignal::new(is_in_my_day(&task, today));

    // Node-picker state
    let assigning = RwSignal::new(false);
    let picker_query = RwSignal::new(String::new());
    let picker_results = RwSignal::<Vec<SearchResult>>::new(vec![]);
    let pick_ver = RwSignal::new(0u32);

    // Debounced search
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
            if pick_ver.get_untracked() != ver {
                return;
            }
            if let Ok(results) = crate::api::node_picker_search(&q).await
                && pick_ver.get_untracked() == ver
            {
                picker_results.set(results);
            }
        });
    });

    let do_assign = move |node_id: NodeId| {
        assigning.set(false);
        picker_query.set(String::new());
        picker_results.set(vec![]);
        let req = UpdateTaskRequest {
            title: None,
            status: None,
            priority: None,
            focus_date: None,
            due_date: None,
            recurrence: None,
            node_id: Some(Some(node_id)),
        };
        wasm_bindgen_futures::spawn_local(async move {
            match crate::api::update_task(task_id, &req).await {
                Ok(_) => refresh.update(|n| *n += 1),
                Err(e) => push_toast(ToastLevel::Error, format!("Couldn't attach to node: {e}")),
            }
        });
    };

    let has_recurrence = task.recurrence.is_some();
    let recurrence_tip = task.recurrence.as_ref().map(|r| recurrence_label(r));
    let overdue = task
        .due_date
        .map(|d| !status_done(&task.status) && d < today)
        .unwrap_or(false);
    let due = task.due_date;

    let do_save = move || {
        let new_title = edit_title.get_untracked().trim().to_string();
        if new_title.is_empty() {
            return;
        }
        let new_priority = parse_priority(&edit_priority.get_untracked());
        let new_recurrence = parse_recurrence_opt(&edit_recurrence.get_untracked());
        let new_due: Option<Option<NaiveDate>> =
            Some(edit_due.get_untracked().trim().parse::<NaiveDate>().ok());
        editing.set(false);
        let prev_title = orig_title.get_untracked();
        let prev_priority = priority_val.get_untracked();
        orig_title.set(new_title.clone());
        priority_val.set(priority_value(&new_priority).to_string());
        let req = UpdateTaskRequest {
            title: Some(new_title),
            status: None,
            priority: Some(new_priority),
            focus_date: None,
            due_date: new_due,
            recurrence: Some(new_recurrence),
            node_id: None,
        };
        wasm_bindgen_futures::spawn_local(async move {
            match crate::api::update_task(task_id, &req).await {
                Ok(_) => refresh.update(|n| *n += 1),
                Err(e) => {
                    // Roll back the optimistic title/priority display.
                    orig_title.set(prev_title);
                    priority_val.set(prev_priority);
                    push_toast(ToastLevel::Error, format!("Save failed: {e}"));
                }
            }
        });
    };

    let on_toggle = move |_| {
        let current = status_val.get_untracked();
        let next = if current == "done" { "open" } else { "done" };
        let req = UpdateTaskRequest {
            title: None,
            status: Some(parse_status(next)),
            priority: None,
            focus_date: None,
            due_date: None,
            recurrence: None,
            node_id: None,
        };
        status_val.set(next.to_string());
        wasm_bindgen_futures::spawn_local(async move {
            match crate::api::update_task(task_id, &req).await {
                Ok(_) => refresh.update(|n| *n += 1),
                Err(e) => {
                    // Roll back the optimistic flip.
                    status_val.set(current);
                    push_toast(ToastLevel::Error, format!("Couldn't update: {e}"));
                }
            }
        });
    };

    let on_toggle_my_day = move |_| {
        let currently_in = in_my_day.get_untracked();
        let new_focus = if currently_in {
            Some(None)
        } else {
            Some(Some(today))
        };
        in_my_day.set(!currently_in);
        let req = UpdateTaskRequest {
            title: None,
            status: None,
            priority: None,
            focus_date: new_focus,
            due_date: None,
            recurrence: None,
            node_id: None,
        };
        wasm_bindgen_futures::spawn_local(async move {
            match crate::api::update_task(task_id, &req).await {
                Ok(_) => refresh.update(|n| *n += 1),
                Err(e) => {
                    // Roll back the optimistic flip.
                    in_my_day.set(currently_in);
                    push_toast(ToastLevel::Error, format!("Couldn't update: {e}"));
                }
            }
        });
    };

    let on_delete = move |_| {
        wasm_bindgen_futures::spawn_local(async move {
            match crate::api::delete_task(task_id).await {
                Ok(_) => {
                    refresh.update(|n| *n += 1);
                    push_undo_toast(
                        "Task deleted",
                        undo_restore_task(task_id, refresh, toast_state),
                    );
                }
                Err(e) => push_toast(ToastLevel::Error, format!("Delete failed: {e}")),
            }
        });
    };

    view! {
        <div class="px-3 py-2" data-task-id=task_id.0.to_string()>
            // Row: checkbox | body | actions — geometry per task_row_scaffold.
            <div class="flex items-start gap-2">
                    <button
                        class=CHECKBOX_CLASS
                        style=move || if status_val.get() == "done" {
                            "background:#d97706;border-color:#d97706;"
                        } else { "" }
                        on:click=on_toggle
                        title="Toggle done"
                    >
                        {move || (status_val.get() == "done").then(|| view! {
                            <span class="material-symbols-outlined text-white"
                                style="font-size:13px;">"check"</span>
                        })}
                    </button>

                    // Title area
                    <div class="flex-1 min-w-0">
                        {move || if editing.get() {
                            // ── Edit form ──────────────────────────────────────
                            let saved_height = use_context::<crate::components::task_row::TaskEditorHeights>()
                                .and_then(|c| c.0.get_untracked().get(&task_id.0).copied());
                            view! {
                                <div class="space-y-2">
                                    <crate::components::resizable_editor::ResizableEditor
                                        value=edit_title
                                        placeholder="Task title…"
                                        initial_height=saved_height
                                        on_submit=Callback::new(move |()| do_save())
                                        on_escape=Callback::new(move |()| {
                                            editing.set(false);
                                            edit_title.set(orig_title.get_untracked());
                                        })
                                        on_resize=Callback::new(move |h: i32| {
                                            wasm_bindgen_futures::spawn_local(async move {
                                                // Best-effort: losing a height pref is cosmetic.
                                                let _ = crate::api::set_editor_pref("task", task_id.0, h).await;
                                            });
                                        })
                                        class="w-full text-sm rounded-lg border border-amber-400 \
                                            bg-white dark:bg-stone-800 px-3 py-2 resize-y min-h-[44px] \
                                            text-stone-900 dark:text-stone-100 \
                                            focus:outline-none focus:ring-1 focus:ring-amber-400".to_string()
                                    />
                                    // Edit controls — wrap on mobile
                                    <div class="flex items-center gap-2 flex-wrap">
                                        <select
                                            class="rounded-lg border border-stone-200
                                                dark:border-stone-700 bg-stone-50
                                                dark:bg-stone-800 px-2 py-1.5 text-xs
                                                text-stone-700 dark:text-stone-300
                                                focus:outline-none focus:ring-1 focus:ring-amber-400"
                                            prop:value=move || edit_priority.get()
                                            on:change=move |ev| edit_priority.set(event_target_value(&ev))
                                        >
                                            <option value="high">"High"</option>
                                            <option value="medium">"Medium"</option>
                                            <option value="low">"Low"</option>
                                        </select>
                                        <input
                                            type="date"
                                            class="flex-1 min-w-0 rounded-lg border
                                                border-stone-200 dark:border-stone-700
                                                bg-stone-50 dark:bg-stone-800
                                                px-2 py-1.5 text-xs
                                                text-stone-700 dark:text-stone-300
                                                focus:outline-none focus:ring-1 focus:ring-amber-400"
                                            prop:value=move || edit_due.get()
                                            on:input=move |ev| edit_due.set(event_target_value(&ev))
                                        />
                                        <select
                                            class="rounded-lg border border-stone-200
                                                dark:border-stone-700 bg-stone-50
                                                dark:bg-stone-800 px-2 py-1.5 text-xs
                                                text-stone-700 dark:text-stone-300
                                                focus:outline-none focus:ring-1 focus:ring-amber-400"
                                            prop:value=move || edit_recurrence.get()
                                            on:change=move |ev| edit_recurrence.set(event_target_value(&ev))
                                        >
                                            <option value="">"No repeat"</option>
                                            <option value="daily">"Daily"</option>
                                            <option value="weekly">"Weekly"</option>
                                            <option value="biweekly">"Biweekly"</option>
                                            <option value="monthly">"Monthly"</option>
                                            <option value="yearly">"Yearly"</option>
                                        </select>
                                    </div>
                                    // Save / cancel
                                    <div class="flex items-center gap-1">
                                        <IconButton
                                            icon="check"
                                            label="Save"
                                            variant=IconButtonVariant::Save
                                            on_click=Callback::new(move |()| do_save())
                                        />
                                        <IconButton
                                            icon="close"
                                            label="Cancel"
                                            on_click=Callback::new(move |()| {
                                                editing.set(false);
                                                edit_title.set(orig_title.get_untracked());
                                            })
                                        />
                                    </div>
                                </div>
                            }.into_any()
                        } else {
                            // ── Read mode: shared scaffold (title, then meta).
                            // Overdue now colours the due badge, not the title.
                            let title_line = view! {
                                {move || priority_dot_view(&parse_priority(&priority_val.get()))}
                                <span class=TITLE_CLASS
                                      style=move || if status_val.get() == "done" {
                                          "text-decoration:line-through;"
                                      } else { "" }
                                      inner_html=move || crate::markdown::render_markdown_inline(&orig_title.get())>
                                </span>
                            }
                            .into_any();
                            let tip = recurrence_tip.unwrap_or("");
                            let meta_line = view! {
                                {due.map(|d| due_badge_view(d, overdue))}
                                {has_recurrence.then(|| view! {
                                    <span
                                        class="flex items-center gap-0.5 text-stone-400 dark:text-stone-500"
                                        title=format!("Repeats: {tip}")
                                    >
                                        <span class="material-symbols-outlined"
                                            style="font-size: 13px;">"repeat"</span>
                                        <span>{tip}</span>
                                    </span>
                                })}
                            }
                            .into_any();
                            view! { <TaskRowBody title_line=title_line meta_line=meta_line/> }
                                .into_any()
                        }}
                    </div>

                    // Actions — right cluster, hidden while editing.
                    // Order matches My Day: context actions, edit, delete.
                    {move || (!editing.get()).then(|| view! {
                        <div class="flex items-center gap-0.5 flex-shrink-0">
                            <button
                                class=action_btn_class("hover:text-blue-500 dark:hover:text-blue-400")
                                title="Assign to node"
                                on:click=move |_| assigning.set(true)
                            >
                                <span class="material-symbols-outlined" style="font-size:16px;">
                                    "call_merge"
                                </span>
                            </button>
                            <button
                                class=move || if in_my_day.get() {
                                    "p-1.5 rounded text-amber-500 bg-amber-50 \
                                     dark:text-amber-400 dark:bg-amber-900/30 \
                                     hover:text-amber-600 dark:hover:text-amber-300 \
                                     transition-colors cursor-pointer".to_string()
                                } else {
                                    action_btn_class("hover:text-amber-500 dark:hover:text-amber-400")
                                }
                                title=move || if in_my_day.get() { "Remove from My Day" } else { "Add to My Day" }
                                on:click=on_toggle_my_day
                            >
                                <span
                                    class="material-symbols-outlined"
                                    style=move || if in_my_day.get() {
                                        "font-size:16px; font-variation-settings: 'FILL' 1;"
                                    } else {
                                        "font-size:16px;"
                                    }
                                >
                                    "wb_sunny"
                                </span>
                            </button>
                            <button
                                class=action_btn_class("hover:text-stone-600 dark:hover:text-stone-300")
                                title="Edit"
                                on:click=move |_| editing.set(true)
                            >
                                <span class="material-symbols-outlined" style="font-size:16px;">
                                    "edit"
                                </span>
                            </button>
                            <button
                                class=action_btn_class("hover:text-red-500 dark:hover:text-red-400")
                                title="Delete"
                                on:click=on_delete
                            >
                                <span class="material-symbols-outlined" style="font-size:16px;">
                                    "delete"
                                </span>
                            </button>
                        </div>
                    })}
            </div>

            // ── Node picker — full-width expansion below the row ──────────────
            {move || assigning.get().then(|| view! {
                <div class="mt-2 rounded-lg px-3 py-3 space-y-2
                    bg-stone-50 dark:bg-stone-800/50">
                    // Search input row
                    <div class="flex items-center gap-2">
                        <span class="material-symbols-outlined text-stone-400 flex-shrink-0"
                            style="font-size: 16px;">"link"</span>
                        <input
                            type="text"
                            placeholder="Search nodes…"
                            autofocus=true
                            class="flex-1 min-w-0 text-sm rounded-lg border border-amber-400
                                bg-white dark:bg-stone-900 px-3 py-2
                                text-stone-900 dark:text-stone-100
                                focus:outline-none focus:ring-1 focus:ring-amber-400"
                            prop:value=move || picker_query.get()
                            on:input=move |ev| picker_query.set(event_target_value(&ev))
                            on:keydown=move |ev: web_sys::KeyboardEvent| {
                                if ev.key() == "Escape" {
                                    assigning.set(false);
                                    picker_query.set(String::new());
                                    picker_results.set(vec![]);
                                }
                            }
                        />
                        <button
                            class="p-2 rounded-lg text-stone-400 hover:text-stone-600
                                dark:hover:text-stone-300 flex-shrink-0
                                transition-colors cursor-pointer"
                            title="Cancel"
                            on:click=move |_| {
                                assigning.set(false);
                                picker_query.set(String::new());
                                picker_results.set(vec![]);
                            }
                        >
                            <span class="material-symbols-outlined" style="font-size: 18px;">
                                "close"
                            </span>
                        </button>
                    </div>
                    // Results list
                    {move || {
                        let results = picker_results.get();
                        (!results.is_empty()).then(|| view! {
                            <div class="bg-white dark:bg-stone-900
                                border border-stone-200 dark:border-stone-700
                                rounded-xl shadow-md overflow-hidden">
                                {results.into_iter().map(|r| {
                                    let node_id = r.node_id;
                                    let title   = r.title.clone();
                                    let icon    = node_type_icon(&r.node_type);
                                    view! {
                                        <button
                                            class="w-full flex items-center gap-2.5 px-3 py-2.5
                                                text-sm text-stone-700 dark:text-stone-300
                                                hover:bg-amber-50 dark:hover:bg-stone-800
                                                active:bg-amber-100 dark:active:bg-stone-700
                                                border-b border-stone-50 dark:border-stone-800
                                                last:border-b-0 transition-colors cursor-pointer"
                                            on:click=move |_| do_assign(node_id)
                                        >
                                            <span class="material-symbols-outlined text-stone-400
                                                flex-shrink-0" style="font-size: 16px;">
                                                {icon}
                                            </span>
                                            <span class="truncate text-left">{title}</span>
                                        </button>
                                    }
                                }).collect_view()}
                            </div>
                        })
                    }}
                </div>
            })}
        </div>
    }
}
