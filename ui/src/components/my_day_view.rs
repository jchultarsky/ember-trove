//! My Day — two-zone vertical Kanban (v2.6.0).
//!
//! Top zone:  tasks with `focus_date == today`        — "what I'm doing today"
//! Bottom:    every open task whose `focus_date` is NOT today  — "everything else"
//!
//! Two ways to swap a task between zones, both equivalent:
//!
//! * **Tap** — every row carries an explicit ☀ "Add to today" (in backlog)
//!   or × "Remove from today" (in today) button.  Always visible — no
//!   hover-to-reveal — so the touch path matches the desktop path.
//! * **Drag** — HTML5 native drag + drop on desktop.  `dataTransfer`
//!   carries the task id; the destination zone fires the same PATCH the
//!   tap button would.  Touch never fires `dragstart`, so iPhone users
//!   simply use the tap.
//!
//! `focus_date` is binary in this model: only `Some(today)` or `None`.
//! There is no "schedule for next Tuesday" affordance on My Day — the
//! task editor still lets you change `due_date` (the external deadline);
//! `focus_date` is purely the Kanban zone.
//!
//! Carry-over UI is gone from this view.  Tasks committed to a previous
//! day that aren't done show up in the backlog with a small
//! "carried from May 2" badge (rendered by `KanbanTaskRow`); the user
//! drags or taps them back to today the same as anything else.

use common::task::MyDayTask;
use leptos::prelude::*;

use crate::app::TaskRefresh;
use crate::components::task_common::status_done;
use crate::components::task_row::{KanbanTaskRow, KanbanZone};
use crate::components::toast::{push_toast, ToastLevel};

#[component]
pub fn MyDayView() -> impl IntoView {
    let task_refresh = use_context::<TaskRefresh>()
        .expect("TaskRefresh context must be provided")
        .0;

    let today      = crate::components::format_helpers::local_today();
    let date_label = today.format("%A, %B %-d").to_string();

    // Two parallel resources — server already filters status, so the
    // client just bins by focus_date.
    let today_tasks = LocalResource::new(move || {
        let _ = task_refresh.get();
        async move { crate::api::fetch_my_day(today).await }
    });
    let all_open = LocalResource::new(move || {
        let _ = task_refresh.get();
        async move { crate::api::list_open_tasks().await }
    });

    view! {
        <div class="flex flex-col h-full">

            // ── Header ──────────────────────────────────────────────────
            <div class="px-4 md:px-6 py-4 border-b border-stone-200 dark:border-stone-800">
                <div class="flex items-center gap-3">
                    <span class="material-symbols-outlined text-amber-500" style="font-size:22px;">
                        "wb_sunny"
                    </span>
                    <div class="flex-1 min-w-0">
                        <h1 class="text-lg font-semibold text-stone-900 dark:text-stone-100">
                            "My Day"
                        </h1>
                        <p class="text-xs text-stone-400 dark:text-stone-500">
                            {date_label}
                            " · drag or tap ☀ to add to today, × to send back to backlog"
                        </p>
                    </div>
                    // X / Y done counter for today
                    {move || {
                        let tasks = today_tasks.get()
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
            </div>

            // ── Two-zone scroll surface ─────────────────────────────────
            <div class="flex-1 overflow-auto p-4 md:p-6 space-y-6">

                // Top zone: focus_date == today
                <Suspense fallback=move || view! {
                    <crate::components::skeleton::SkeletonList rows=3 />
                }>
                    {move || {
                        let raw = today_tasks.get()
                            .and_then(|r| r.ok())
                            .unwrap_or_default();
                        // The server's list_my_day also includes
                        // carryovers (focus_date < today AND not done) —
                        // exclude them here so the today zone is strictly
                        // focus_date == today.  Carryovers naturally land
                        // in the backlog zone below.
                        let scoped: Vec<MyDayTask> = raw.into_iter()
                            .filter(|t| t.task.focus_date == Some(today))
                            .collect();
                        view! {
                            <KanbanZoneRow
                                title="Today"
                                subtitle="Tasks you're focused on today"
                                zone=KanbanZone::Today
                                empty_msg="Nothing on today's list — drag or tap ☀ on a backlog task below."
                                tasks=scoped
                                today=today
                                refresh=task_refresh
                                accent_class="bg-amber-50/30 dark:bg-amber-950/10 border-amber-200 dark:border-amber-900/40"
                            />
                        }
                    }}
                </Suspense>

                <div class="border-t border-dashed border-stone-300 dark:border-stone-700"></div>

                // Bottom zone: every open task NOT focused on today
                <Suspense fallback=move || view! {
                    <crate::components::skeleton::SkeletonList rows=8 />
                }>
                    {move || {
                        let raw = all_open.get()
                            .and_then(|r| r.ok())
                            .unwrap_or_default();
                        let scoped: Vec<MyDayTask> = raw.into_iter()
                            .filter(|t| t.task.focus_date != Some(today))
                            .collect();
                        let count = scoped.len();
                        let subtitle = if count == 0 {
                            "Your backlog is empty.".to_string()
                        } else {
                            format!("{count} open · sorted by deadline first, then priority")
                        };
                        view! {
                            <KanbanZoneRow
                                title="Backlog"
                                subtitle=subtitle
                                zone=KanbanZone::Backlog
                                empty_msg="No open tasks elsewhere — inbox zero across all projects."
                                tasks=scoped
                                today=today
                                refresh=task_refresh
                                accent_class="bg-stone-50/40 dark:bg-stone-900/30 border-stone-200 dark:border-stone-700"
                            />
                        }
                    }}
                </Suspense>
            </div>
        </div>
    }
}

// ── KanbanZoneRow ────────────────────────────────────────────────────────────
//
// A single zone (Today or Backlog) — header + task list + drop-target wiring.

#[component]
fn KanbanZoneRow(
    title: &'static str,
    #[prop(into)] subtitle: String,
    zone: KanbanZone,
    empty_msg: &'static str,
    tasks: Vec<MyDayTask>,
    today: chrono::NaiveDate,
    refresh: RwSignal<u32>,
    accent_class: &'static str,
) -> impl IntoView {
    let drag_over = RwSignal::new(false);

    // Drop handler — pulls the task id from dataTransfer and PATCHes
    // focus_date to match this zone.  Same code path as the per-row tap
    // buttons so behaviour is identical.
    let on_drop = move |ev: web_sys::DragEvent| {
        ev.prevent_default();
        drag_over.set(false);
        let Some(dt)  = ev.data_transfer() else { return; };
        let Ok(raw)   = dt.get_data("text/plain")    else { return; };
        let Ok(uuid)  = raw.parse::<uuid::Uuid>()    else { return; };
        let task_id   = common::id::TaskId(uuid);
        let new_focus = match zone {
            KanbanZone::Today   => Some(today),
            KanbanZone::Backlog => None,
        };
        let success_msg = match zone {
            KanbanZone::Today   => "Added to today",
            KanbanZone::Backlog => "Removed from today",
        };
        let req = common::task::UpdateTaskRequest {
            title:      None,
            status:     None,
            priority:   None,
            focus_date: Some(new_focus),
            due_date:   None,
            recurrence: None,
            node_id:    None,
        };
        wasm_bindgen_futures::spawn_local(async move {
            match crate::api::update_task(task_id, &req).await {
                Ok(_)  => {
                    push_toast(ToastLevel::Success, success_msg);
                    refresh.update(|n| *n += 1);
                }
                Err(e) => push_toast(ToastLevel::Error, format!("Drop failed: {e}")),
            }
        });
    };

    view! {
        <section
            class=move || format!(
                "rounded-lg border p-3 transition-colors {accent_class} {}",
                if drag_over.get() { "ring-2 ring-amber-400 ring-offset-1" } else { "" }
            )
            on:dragover=move |ev| {
                ev.prevent_default();
                if let Some(dt) = ev.data_transfer() {
                    dt.set_drop_effect("move");
                }
                drag_over.set(true);
            }
            on:dragleave=move |_| drag_over.set(false)
            on:drop=on_drop
        >
            <div class="flex items-center gap-2 mb-2">
                <span class="text-xs font-semibold text-stone-700 dark:text-stone-300 \
                             uppercase tracking-wider">
                    {title}
                </span>
                <span class="text-xs text-stone-500 dark:text-stone-400">
                    " · " {subtitle}
                </span>
            </div>
            {if tasks.is_empty() {
                view! {
                    <p class="text-sm text-stone-400 dark:text-stone-500 italic px-3 py-2">
                        {empty_msg}
                    </p>
                }.into_any()
            } else {
                view! {
                    <div class="divide-y divide-stone-100 dark:divide-stone-800">
                        {tasks.into_iter().map(|MyDayTask { task, node_title }| {
                            view! {
                                <KanbanTaskRow
                                    task=task
                                    node_title=node_title
                                    today=today
                                    zone=zone
                                    refresh=refresh
                                />
                            }
                        }).collect_view()}
                    </div>
                }.into_any()
            }}
        </section>
    }
}
