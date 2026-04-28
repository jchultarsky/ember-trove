//! `/plan` — Morning Planning Ritual.
//!
//! A once-per-day surface that turns "look at My Day" into "plan your
//! day": yesterday's recap, the carry-over backlog, the Inbox count, and
//! a peek at today's calendar all in one place.  Inspired by Sunsama's
//! daily-planner ritual but stripped down — no dragging, no time-blocking,
//! just the four things you actually need to decide before the day starts.
//!
//! Design notes:
//!   * Reuses existing `fetch_my_day` / `list_inbox` / `fetch_calendar_tasks`
//!     endpoints so this phase needs zero server work.  Yesterday's
//!     stats are derived client-side from `fetch_my_day(yesterday)`
//!     because the API already filters to "focus_date == d OR carryover".
//!   * Carry-over triage UI lives on `/tasks/my-day` (the Kanban
//!     backlog zone shows carryovers with a "carried from X" badge).
//!     Here we just surface the *count* as a nudge — same pattern as
//!     the Inbox section.
//!   * Confirming the plan stamps `et.plan.last_planned_at` in
//!     localStorage; the My Day banner reads that same key to decide
//!     whether to nudge.
//!
//! Out of scope for v2.5.0: time-blocking, AI suggestions, week view,
//! goal/objective alignment.  Those belong in later phases.

use chrono::{Datelike, Duration, NaiveDate};
use common::task::{MyDayTask, Task, TaskPriority, TaskStatus};
use leptos::prelude::*;
use leptos_router::hooks::use_navigate;

use crate::app::TaskRefresh;
use crate::components::format_helpers::local_today;

/// localStorage key used by both this view (writer) and `MyDayView`'s
/// banner (reader) to decide whether the user has already planned today.
pub const LAST_PLANNED_AT_KEY: &str = "et.plan.last_planned_at";

#[component]
pub fn PlanView() -> impl IntoView {
    let task_refresh = use_context::<TaskRefresh>()
        .expect("TaskRefresh context must be provided")
        .0;

    let today        = local_today();
    let yesterday    = today - Duration::days(1);
    let date_label   = today.format("%A, %B %-d").to_string();
    let yest_label   = yesterday.format("%A, %B %-d").to_string();

    // Resources — re-fetch when refresh bumps so triage actions reflect
    // immediately in the recap counts.
    let yesterday_tasks = LocalResource::new(move || {
        let _ = task_refresh.get();
        async move { crate::api::fetch_my_day(yesterday).await }
    });
    let today_tasks = LocalResource::new(move || {
        let _ = task_refresh.get();
        async move { crate::api::fetch_my_day(today).await }
    });
    let inbox_tasks = LocalResource::new(move || {
        let _ = task_refresh.get();
        async move { crate::api::list_inbox().await }
    });
    let cal_tasks = LocalResource::new(move || {
        let _ = task_refresh.get();
        let (y, m) = (today.year(), today.month());
        async move { crate::api::fetch_calendar_tasks(y, m).await }
    });

    let navigate = StoredValue::new(use_navigate());

    // "Confirm today's plan" — stamp localStorage so the banner stops
    // nagging, then jump to My Day where the user actually does the work.
    let on_confirm = move |_| {
        let _ = web_sys::window()
            .and_then(|w| w.local_storage().ok().flatten())
            .and_then(|s| s.set_item(LAST_PLANNED_AT_KEY, &today.to_string()).ok());
        navigate.get_value()("/tasks/my-day", Default::default());
    };

    view! {
        <div class="flex flex-col h-full overflow-auto">
            // ── Header ──────────────────────────────────────────────────
            <div class="px-4 md:px-6 py-4 border-b border-stone-200 dark:border-stone-800">
                <div class="flex items-center gap-3">
                    <span class="material-symbols-outlined text-amber-500" style="font-size:24px;">
                        "wb_twilight"
                    </span>
                    <div class="flex-1">
                        <h1 class="text-lg font-semibold text-stone-900 dark:text-stone-100">
                            "Plan your day"
                        </h1>
                        <p class="text-xs text-stone-500 dark:text-stone-400">{date_label}</p>
                    </div>
                </div>
            </div>

            <div class="flex-1 p-4 md:p-6 space-y-6 max-w-3xl w-full mx-auto">

                // ── Section 1: Yesterday recap ───────────────────────────
                <section>
                    <h2 class="text-xs font-semibold text-stone-500 dark:text-stone-400 \
                               uppercase tracking-wider mb-2">
                        "Yesterday — " {yest_label}
                    </h2>
                    <Suspense fallback=move || view! {
                        <crate::components::skeleton::SkeletonBar />
                    }>
                        {move || {
                            let yest = yesterday_tasks.get()
                                .and_then(|r| r.ok())
                                .unwrap_or_default();
                            // Filter to ones whose focus_date was actually yesterday
                            // (list_my_day for `yesterday` may include carryovers
                            // from earlier — those are already counted in the
                            // Carry Over section below).
                            let scoped: Vec<&MyDayTask> = yest.iter()
                                .filter(|t| t.task.focus_date == Some(yesterday))
                                .collect();
                            let total = scoped.len();
                            if total == 0 {
                                return view! {
                                    <p class="text-sm text-stone-500 dark:text-stone-400">
                                        "Nothing was planned for yesterday."
                                    </p>
                                }.into_any();
                            }
                            let done      = scoped.iter().filter(|t| matches!(t.task.status, TaskStatus::Done)).count();
                            let cancelled = scoped.iter().filter(|t| matches!(t.task.status, TaskStatus::Cancelled)).count();
                            let open      = total - done - cancelled;
                            view! {
                                <div class="flex items-center gap-4 text-sm">
                                    <Stat label="Done"      count=done      colour="text-green-600 dark:text-green-400" />
                                    <Stat label="Open"      count=open      colour="text-amber-600 dark:text-amber-400" />
                                    <Stat label="Cancelled" count=cancelled colour="text-stone-500 dark:text-stone-500" />
                                </div>
                            }.into_any()
                        }}
                    </Suspense>
                </section>

                // ── Section 2: Carry-over count ──────────────────────────
                // The actual triage UI lives on /tasks/my-day's Kanban
                // backlog (carryovers show up there with a "carried from
                // X" badge — drag/tap to bring them back to today).  Here
                // we just surface the count so the morning ritual nudges
                // the user to look.
                <section>
                    <h2 class="text-xs font-semibold text-stone-500 dark:text-stone-400 \
                               uppercase tracking-wider mb-2">
                        "Carry over"
                    </h2>
                    <Suspense fallback=move || view! {
                        <crate::components::skeleton::SkeletonBar />
                    }>
                        {move || {
                            let raw = today_tasks.get()
                                .and_then(|r| r.ok())
                                .unwrap_or_default();
                            let count = raw.iter()
                                .filter(|t| t.task.focus_date.is_some_and(|d| d < today))
                                .count();
                            if count == 0 {
                                return view! {
                                    <p class="text-sm text-stone-500 dark:text-stone-400">
                                        "Nothing carried over — clean slate."
                                    </p>
                                }.into_any();
                            }
                            let label = if count == 1 {
                                "1 task carried over from earlier days".to_string()
                            } else {
                                format!("{count} tasks carried over from earlier days")
                            };
                            view! {
                                <button
                                    class="flex items-center gap-2 text-sm text-stone-700 \
                                           dark:text-stone-200 hover:text-amber-600 \
                                           dark:hover:text-amber-400 transition-colors cursor-pointer"
                                    on:click=move |_| navigate.get_value()("/tasks/my-day", Default::default())
                                >
                                    <span class="material-symbols-outlined" style="font-size:16px;">"history"</span>
                                    <span>{label}</span>
                                    <span class="material-symbols-outlined" style="font-size:14px;">"arrow_forward"</span>
                                </button>
                            }.into_any()
                        }}
                    </Suspense>
                </section>

                // ── Section 3: Inbox ─────────────────────────────────────
                <section>
                    <h2 class="text-xs font-semibold text-stone-500 dark:text-stone-400 \
                               uppercase tracking-wider mb-2">
                        "Inbox"
                    </h2>
                    <Suspense fallback=move || view! {
                        <crate::components::skeleton::SkeletonBar />
                    }>
                        {move || {
                            // Server-side `list_inbox` returns ALL standalone
                            // tasks regardless of status — including done /
                            // cancelled.  "Items to triage" is only the open
                            // ones, so we filter on the client (server fix
                            // would need a coordinated change across the
                            // existing InboxView call site).
                            let inbox = inbox_tasks.get()
                                .and_then(|r| r.ok())
                                .unwrap_or_default();
                            let open_count = inbox.iter()
                                .filter(|t| !crate::components::task_common::status_done(&t.status))
                                .count();
                            if open_count == 0 {
                                return view! {
                                    <p class="text-sm text-stone-500 dark:text-stone-400">
                                        "Inbox is empty."
                                    </p>
                                }.into_any();
                            }
                            let label = if open_count == 1 { "1 item to triage".to_string() }
                                        else { format!("{open_count} items to triage") };
                            view! {
                                <button
                                    class="flex items-center gap-2 text-sm text-stone-700 \
                                           dark:text-stone-200 hover:text-amber-600 \
                                           dark:hover:text-amber-400 transition-colors cursor-pointer"
                                    on:click=move |_| navigate.get_value()("/tasks/inbox", Default::default())
                                >
                                    <span class="material-symbols-outlined" style="font-size:16px;">"inbox"</span>
                                    <span>{label}</span>
                                    <span class="material-symbols-outlined" style="font-size:14px;">"arrow_forward"</span>
                                </button>
                            }.into_any()
                        }}
                    </Suspense>
                </section>

                // ── Section 4: Today's calendar peek ─────────────────────
                <section>
                    <h2 class="text-xs font-semibold text-stone-500 dark:text-stone-400 \
                               uppercase tracking-wider mb-2">
                        "Due today"
                    </h2>
                    <Suspense fallback=move || view! {
                        <crate::components::skeleton::SkeletonBar />
                    }>
                        {move || {
                            let cal = cal_tasks.get()
                                .and_then(|r| r.ok())
                                .unwrap_or_default();
                            let due_today: Vec<&MyDayTask> = cal.iter()
                                .filter(|t| t.task.due_date == Some(today)
                                          && !matches!(t.task.status, TaskStatus::Done | TaskStatus::Cancelled))
                                .collect();
                            if due_today.is_empty() {
                                return view! {
                                    <p class="text-sm text-stone-500 dark:text-stone-400">
                                        "No deadlines today."
                                    </p>
                                }.into_any();
                            }
                            view! {
                                <ul class="space-y-1">
                                    {due_today.into_iter().map(|mdt| {
                                        view! {
                                            <CalRow task=mdt.task.clone() node_title=mdt.node_title.clone() />
                                        }
                                    }).collect_view()}
                                </ul>
                            }.into_any()
                        }}
                    </Suspense>
                </section>

                // ── Confirm CTA ──────────────────────────────────────────
                <div class="pt-4 border-t border-stone-200 dark:border-stone-800 \
                            flex items-center gap-3">
                    <button
                        class="px-4 py-2 rounded-lg bg-amber-600 text-white text-sm \
                               font-medium hover:bg-amber-700 cursor-pointer"
                        on:click=on_confirm
                    >
                        "Start my day"
                    </button>
                    <span class="text-xs text-stone-500 dark:text-stone-400">
                        "Marks today as planned and opens My Day."
                    </span>
                </div>
            </div>
        </div>
    }
}

#[component]
fn Stat(label: &'static str, count: usize, colour: &'static str) -> impl IntoView {
    view! {
        <span class=format!("font-semibold {colour}")>{count}</span>
        <span class="text-stone-500 dark:text-stone-400 -ml-3">{label}</span>
    }
}

#[component]
fn CalRow(task: Task, node_title: Option<String>) -> impl IntoView {
    let priority_dot = match task.priority {
        TaskPriority::High   => Some("color:#ef4444;"),
        TaskPriority::Medium => Some("color:#f59e0b;"),
        TaskPriority::Low    => None,
    };
    let parent_label = node_title.unwrap_or_else(|| "Inbox".to_string());
    view! {
        <li class="flex items-center gap-2 text-sm text-stone-700 dark:text-stone-200">
            {priority_dot.map(|s| view! {
                <span style=format!("{s}font-size:8px;line-height:1;")>"●"</span>
            })}
            <span class="flex-1 truncate">{task.title.clone()}</span>
            <span class="text-xs text-stone-500 dark:text-stone-400 truncate">{parent_label}</span>
        </li>
    }
}

/// Returns `true` when localStorage contains a `LAST_PLANNED_AT_KEY`
/// stamp equal to today's local date.  Used by `MyDayView` to decide
/// whether to surface the "Plan your day" banner.
#[must_use]
pub fn planned_today() -> bool {
    let today = local_today();
    web_sys::window()
        .and_then(|w| w.local_storage().ok().flatten())
        .and_then(|s| s.get_item(LAST_PLANNED_AT_KEY).ok().flatten())
        .and_then(|s| s.parse::<NaiveDate>().ok())
        .is_some_and(|d| d == today)
}
