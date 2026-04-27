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
    task::{CreateTaskRequest, Task, UpdateTaskRequest},
};
use leptos::prelude::*;

use crate::app::TaskRefresh;
use crate::components::task_common::{
    is_in_my_day, node_type_icon, parse_priority, parse_recurrence_opt, parse_status,
    priority_dot_color, priority_label, priority_value, recurrence_label, recurrence_value,
    status_done, status_value,
};
use crate::components::toast::{push_toast, ToastLevel};

// ── InboxView ─────────────────────────────────────────────────────────────────

#[component]
pub fn InboxView() -> impl IntoView {
    let task_refresh = use_context::<TaskRefresh>().expect("TaskRefresh context missing");
    let refresh = task_refresh.0;

    // If we got here via the iOS Web Share Target SW handler (which 303s to
    // /tasks/inbox?captured=1) or the home-screen "Quick capture" shortcut,
    // confirm the capture with a toast and strip the marker from the URL so
    // a refresh doesn't re-fire it.  Runs once per mount.
    Effect::new(move |run_count: Option<()>| {
        if run_count.is_some() { return; }
        let Some(win) = web_sys::window() else { return; };
        let Ok(href) = win.location().href() else { return; };
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

    // New-task form state
    let new_title    = RwSignal::new(String::new());
    let new_priority = RwSignal::new("medium".to_string());
    let new_due      = RwSignal::new(String::new());
    let adding       = RwSignal::new(false);

    let do_add = move || {
        let title = new_title.get_untracked().trim().to_string();
        if title.is_empty() { return; }
        let priority = parse_priority(&new_priority.get_untracked());
        let due_date: Option<NaiveDate> =
            new_due.get_untracked().trim().parse::<NaiveDate>().ok();
        adding.set(true);
        wasm_bindgen_futures::spawn_local(async move {
            let req = CreateTaskRequest {
                title,
                node_id: None,
                status: None,
                priority: Some(priority),
                focus_date: None,
                due_date,
                recurrence: None,
            };
            if crate::api::create_standalone_task(&req).await.is_ok() {
                new_title.set(String::new());
                new_priority.set("medium".to_string());
                new_due.set(String::new());
                refresh.update(|n| *n += 1);
            }
            adding.set(false);
        });
    };

    let on_key = move |ev: web_sys::KeyboardEvent| {
        if ev.key() == "Enter" { do_add(); }
    };

    let tasks_res = LocalResource::new(move || {
        let _ = refresh.get();
        async move { crate::api::list_inbox().await }
    });

    view! {
        <div class="flex flex-col h-full">
            // ── Header ────────────────────────────────────────────────────────
            <div class="flex-shrink-0 px-4 py-4 border-b border-stone-200 dark:border-stone-800">
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
            </div>

            // ── Scrollable content ────────────────────────────────────────────
            <div class="flex-1 overflow-auto px-4 py-4 space-y-4">

                // ── Add-task card ─────────────────────────────────────────────
                <div class="bg-white dark:bg-stone-900 rounded-xl border border-stone-200
                    dark:border-stone-700 p-4 shadow-sm space-y-3">
                    <p class="text-xs font-semibold text-stone-500 dark:text-stone-400
                        uppercase tracking-wider">
                        "New task"
                    </p>
                    // Title input — full width
                    <input
                        type="text"
                        placeholder="What needs to be done?"
                        class="w-full rounded-lg border border-stone-200 dark:border-stone-700
                            bg-stone-50 dark:bg-stone-800 px-3 py-2.5 text-sm
                            text-stone-900 dark:text-stone-100
                            focus:outline-none focus:ring-2 focus:ring-amber-400"
                        prop:value=move || new_title.get()
                        on:input=move |ev| new_title.set(event_target_value(&ev))
                        on:keydown=on_key
                    />
                    // Controls row — wraps gracefully on narrow screens
                    <div class="flex items-center gap-2 flex-wrap">
                        <select
                            class="rounded-lg border border-stone-200 dark:border-stone-700
                                bg-stone-50 dark:bg-stone-800 px-3 py-2 text-sm
                                text-stone-700 dark:text-stone-300
                                focus:outline-none focus:ring-1 focus:ring-amber-400"
                            prop:value=move || new_priority.get()
                            on:change=move |ev| new_priority.set(event_target_value(&ev))
                        >
                            <option value="high">"High"</option>
                            <option value="medium" selected=true>"Medium"</option>
                            <option value="low">"Low"</option>
                        </select>
                        <input
                            type="date"
                            class="flex-1 min-w-0 rounded-lg border border-stone-200
                                dark:border-stone-700 bg-stone-50 dark:bg-stone-800
                                px-3 py-2 text-sm text-stone-700 dark:text-stone-300
                                focus:outline-none focus:ring-1 focus:ring-amber-400"
                            prop:value=move || new_due.get()
                            on:input=move |ev| new_due.set(event_target_value(&ev))
                        />
                        <button
                            class="px-4 py-2 rounded-lg text-sm font-medium
                                bg-amber-500 hover:bg-amber-600 active:bg-amber-700
                                text-white transition-colors cursor-pointer
                                disabled:opacity-50 disabled:cursor-not-allowed"
                            on:click=move |_| do_add()
                            disabled=move || adding.get()
                        >
                            {move || if adding.get() { "Adding…" } else { "Add" }}
                        </button>
                    </div>
                </div>

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
                            <div class="space-y-2">
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
            </div>
        </div>
    }
}

// ── InboxTaskRow ──────────────────────────────────────────────────────────────

#[component]
fn InboxTaskRow(task: Task, refresh: RwSignal<u32>) -> impl IntoView {
    let task_id = task.id;
    let today   = crate::components::format_helpers::local_today();

    let status_val   = RwSignal::new(status_value(&task.status).to_string());
    let priority_val = RwSignal::new(priority_value(&task.priority).to_string());

    // Inline-edit state
    let editing         = RwSignal::new(false);
    let orig_title      = RwSignal::new(task.title.clone());
    let edit_title      = RwSignal::new(task.title.clone());
    let edit_priority   = RwSignal::new(priority_value(&task.priority).to_string());
    let edit_due        = RwSignal::new(
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
    let assigning      = RwSignal::new(false);
    let picker_query   = RwSignal::new(String::new());
    let picker_results = RwSignal::<Vec<SearchResult>>::new(vec![]);
    let pick_ver       = RwSignal::new(0u32);

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
            if pick_ver.get_untracked() != ver { return; }
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
            title: None, status: None, priority: None,
            focus_date: None, due_date: None, recurrence: None,
            node_id: Some(Some(node_id)),
        };
        wasm_bindgen_futures::spawn_local(async move {
            let _ = crate::api::update_task(task_id, &req).await;
            refresh.update(|n| *n += 1);
        });
    };

    let has_recurrence = task.recurrence.is_some();
    let recurrence_tip = task.recurrence.as_ref().map(|r| recurrence_label(r));
    let overdue        = task.due_date
        .map(|d| !status_done(&task.status) && d < today)
        .unwrap_or(false);
    let due = task.due_date;

    let do_save = move || {
        let new_title = edit_title.get_untracked().trim().to_string();
        if new_title.is_empty() { return; }
        let new_priority   = parse_priority(&edit_priority.get_untracked());
        let new_recurrence = parse_recurrence_opt(&edit_recurrence.get_untracked());
        let new_due: Option<Option<NaiveDate>> =
            Some(edit_due.get_untracked().trim().parse::<NaiveDate>().ok());
        editing.set(false);
        orig_title.set(new_title.clone());
        priority_val.set(priority_value(&new_priority).to_string());
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
            priority: None, focus_date: None, due_date: None,
            recurrence: None, node_id: None,
        };
        status_val.set(next.to_string());
        wasm_bindgen_futures::spawn_local(async move {
            let _ = crate::api::update_task(task_id, &req).await;
            refresh.update(|n| *n += 1);
        });
    };

    let on_toggle_my_day = move |_| {
        let currently_in = in_my_day.get_untracked();
        let new_focus = if currently_in { Some(None) } else { Some(Some(today)) };
        in_my_day.set(!currently_in);
        let req = UpdateTaskRequest {
            title: None, status: None, priority: None,
            focus_date: new_focus, due_date: None,
            recurrence: None, node_id: None,
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

    view! {
        <div class="bg-white dark:bg-stone-900 rounded-xl
            border border-stone-100 dark:border-stone-800
            shadow-sm overflow-hidden">

            // ── Main content area ─────────────────────────────────────────────
            <div class="px-3 pt-3 pb-2 space-y-2">

                // Row 1: checkbox + title (+ badges in read mode)
                <div class="flex items-start gap-2.5">
                    // Checkbox
                    <button
                        class="flex-shrink-0 mt-0.5 w-5 h-5 rounded border-2 border-stone-300
                            dark:border-stone-600 flex items-center justify-center
                            hover:border-amber-500 active:border-amber-600
                            transition-colors cursor-pointer"
                        on:click=on_toggle
                    >
                        {move || (status_val.get() == "done").then(|| view! {
                            <span class="material-symbols-outlined text-amber-500"
                                style="font-size: 14px; font-variation-settings: 'FILL' 1;">
                                "check"
                            </span>
                        })}
                    </button>

                    // Title area
                    <div class="flex-1 min-w-0">
                        {move || if editing.get() {
                            // ── Edit form ──────────────────────────────────────
                            view! {
                                <div class="space-y-2">
                                    <input
                                        type="text"
                                        class="w-full text-sm rounded-lg border border-amber-400
                                            bg-white dark:bg-stone-800 px-3 py-2
                                            text-stone-900 dark:text-stone-100
                                            focus:outline-none focus:ring-1 focus:ring-amber-400"
                                        prop:value=move || edit_title.get()
                                        on:input=move |ev| edit_title.set(event_target_value(&ev))
                                        on:keydown=move |ev: web_sys::KeyboardEvent| {
                                            if ev.key() == "Enter"  { do_save(); }
                                            if ev.key() == "Escape" {
                                                editing.set(false);
                                                edit_title.set(orig_title.get_untracked());
                                            }
                                        }
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
                                    <div class="flex items-center gap-2">
                                        <button
                                            class="flex items-center gap-1 px-3 py-1.5 rounded-lg
                                                text-xs font-medium bg-amber-500 hover:bg-amber-600
                                                active:bg-amber-700 text-white
                                                transition-colors cursor-pointer"
                                            on:click=move |_| do_save()
                                        >
                                            <span class="material-symbols-outlined"
                                                style="font-size: 14px;">"check"</span>
                                            "Save"
                                        </button>
                                        <button
                                            class="flex items-center gap-1 px-3 py-1.5 rounded-lg
                                                text-xs text-stone-500 hover:text-stone-700
                                                dark:hover:text-stone-300
                                                transition-colors cursor-pointer"
                                            on:click=move |_| {
                                                editing.set(false);
                                                edit_title.set(orig_title.get_untracked());
                                            }
                                        >
                                            <span class="material-symbols-outlined"
                                                style="font-size: 14px;">"close"</span>
                                            "Cancel"
                                        </button>
                                    </div>
                                </div>
                            }.into_any()
                        } else {
                            // ── Read mode ──────────────────────────────────────
                            view! {
                                <div class="space-y-1">
                                    // Title — wraps naturally; no truncation on mobile
                                    <p
                                        class="text-sm leading-snug text-stone-800
                                            dark:text-stone-200"
                                        style=move || {
                                            let mut s = String::new();
                                            if status_val.get() == "done" {
                                                s.push_str("text-decoration:line-through;\
                                                             opacity:0.45;");
                                            }
                                            if overdue { s.push_str("color:#ef4444;") }
                                            s
                                        }
                                    >
                                        {move || orig_title.get()}
                                    </p>
                                    // Badges row (due + recurrence)
                                    {(due.is_some() || has_recurrence).then(|| {
                                        let tip = recurrence_tip.unwrap_or("");
                                        view! {
                                            <div class="flex items-center gap-2 flex-wrap">
                                                {due.map(|d| {
                                                    let label = d.format("%b %-d").to_string();
                                                    let style = if overdue {
                                                        "color:#ef4444;"
                                                    } else {
                                                        "color:#6b7280;"
                                                    };
                                                    view! {
                                                        <span class="text-xs flex-shrink-0"
                                                            style=style>
                                                            {if overdue {
                                                                format!("⚠ {label}")
                                                            } else {
                                                                label
                                                            }}
                                                        </span>
                                                    }
                                                })}
                                                {has_recurrence.then(|| view! {
                                                    <span
                                                        class="flex items-center gap-0.5 text-xs
                                                            text-stone-400 dark:text-stone-500"
                                                        title=format!("Repeats: {tip}")
                                                    >
                                                        <span class="material-symbols-outlined"
                                                            style="font-size: 13px;">"repeat"</span>
                                                        <span>{tip}</span>
                                                    </span>
                                                })}
                                            </div>
                                        }
                                    })}
                                </div>
                            }.into_any()
                        }}
                    </div>
                </div>

                // ── Action bar — always visible, never hover-gated ────────────
                {move || (!editing.get()).then(|| view! {
                    <div class="flex items-center gap-0.5 pt-1
                        border-t border-stone-50 dark:border-stone-800">
                        // Priority pill (left side)
                        {move || {
                            let p = parse_priority(&priority_val.get());
                            view! {
                                <div class="flex items-center gap-1.5 flex-1">
                                    <div
                                        class="w-2 h-2 rounded-full flex-shrink-0"
                                        style=priority_dot_color(&p)
                                    />
                                    <span class="text-xs text-stone-400 dark:text-stone-500">
                                        {priority_label(&p)}
                                    </span>
                                </div>
                            }
                        }}

                        // Action buttons (right side)
                        <button
                            class="p-2 rounded-lg text-stone-400 hover:text-blue-500
                                dark:text-stone-500 dark:hover:text-blue-400
                                active:bg-stone-100 dark:active:bg-stone-800
                                transition-colors cursor-pointer"
                            title="Assign to node"
                            on:click=move |_| assigning.set(true)
                        >
                            <span class="material-symbols-outlined" style="font-size: 18px;">
                                "call_merge"
                            </span>
                        </button>
                        <button
                            class=move || if in_my_day.get() {
                                "p-2 rounded-lg text-amber-500 bg-amber-50 \
                                 dark:text-amber-400 dark:bg-amber-900/30 \
                                 hover:text-amber-600 dark:hover:text-amber-300 \
                                 transition-colors cursor-pointer"
                            } else {
                                "p-2 rounded-lg text-stone-400 hover:text-amber-500 \
                                 dark:text-stone-500 dark:hover:text-amber-400 \
                                 active:bg-stone-100 dark:active:bg-stone-800 \
                                 transition-colors cursor-pointer"
                            }
                            title=move || if in_my_day.get() { "Remove from My Day" } else { "Add to My Day" }
                            on:click=on_toggle_my_day
                        >
                            <span
                                class="material-symbols-outlined"
                                style=move || if in_my_day.get() {
                                    "font-size: 18px; font-variation-settings: 'FILL' 1;"
                                } else {
                                    "font-size: 18px;"
                                }
                            >
                                "wb_sunny"
                            </span>
                        </button>
                        <button
                            class="p-2 rounded-lg text-stone-400 hover:text-stone-600
                                dark:text-stone-500 dark:hover:text-stone-300
                                active:bg-stone-100 dark:active:bg-stone-800
                                transition-colors cursor-pointer"
                            title="Edit"
                            on:click=move |_| editing.set(true)
                        >
                            <span class="material-symbols-outlined" style="font-size: 18px;">
                                "edit"
                            </span>
                        </button>
                        <button
                            class="p-2 rounded-lg text-stone-400 hover:text-red-500
                                dark:text-stone-500 dark:hover:text-red-400
                                active:bg-stone-100 dark:active:bg-stone-800
                                transition-colors cursor-pointer"
                            title="Delete"
                            on:click=on_delete
                        >
                            <span class="material-symbols-outlined" style="font-size: 18px;">
                                "delete"
                            </span>
                        </button>
                    </div>
                })}
            </div>

            // ── Node picker — full-width expansion below the card ─────────────
            {move || assigning.get().then(|| view! {
                <div class="border-t border-stone-100 dark:border-stone-800 px-3 py-3 space-y-2
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
