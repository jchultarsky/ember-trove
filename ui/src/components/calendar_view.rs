use chrono::{Datelike, NaiveDate, Weekday};
use common::task::MyDayTask;
use leptos::prelude::*;

use crate::app::TaskRefresh;
use crate::components::task_common::{priority_color_hex, status_done};
use leptos_router::hooks::use_navigate;

fn next_month(year: i32, month: u32) -> (i32, u32) {
    if month == 12 {
        (year + 1, 1)
    } else {
        (year, month + 1)
    }
}

fn prev_month(year: i32, month: u32) -> (i32, u32) {
    if month == 1 {
        (year - 1, 12)
    } else {
        (year, month - 1)
    }
}

fn days_in_month(year: i32, month: u32) -> u32 {
    let (ny, nm) = next_month(year, month);
    NaiveDate::from_ymd_opt(ny, nm, 1)
        .and_then(|d| d.pred_opt())
        .map(|d| d.day())
        .unwrap_or(30)
}

fn first_weekday(year: i32, month: u32) -> usize {
    NaiveDate::from_ymd_opt(year, month, 1)
        .map(|d| match d.weekday() {
            Weekday::Mon => 0,
            Weekday::Tue => 1,
            Weekday::Wed => 2,
            Weekday::Thu => 3,
            Weekday::Fri => 4,
            Weekday::Sat => 5,
            Weekday::Sun => 6,
        })
        .unwrap_or(0)
}

const MONTH_NAMES: [&str; 12] = [
    "January",
    "February",
    "March",
    "April",
    "May",
    "June",
    "July",
    "August",
    "September",
    "October",
    "November",
    "December",
];

fn month_name(month: u32) -> &'static str {
    MONTH_NAMES[(month as usize).saturating_sub(1).min(11)]
}

/// Format the header label spanning two months.
/// Same year: "April \u{2013} May 2026". Different years: "December 2025 \u{2013} January 2026".
fn two_month_label(y1: i32, m1: u32, y2: i32, m2: u32) -> String {
    if y1 == y2 {
        format!("{} \u{2013} {} {}", month_name(m1), month_name(m2), y1)
    } else {
        format!(
            "{} {} \u{2013} {} {}",
            month_name(m1),
            y1,
            month_name(m2),
            y2
        )
    }
}

// ── MonthGrid (inner component) ──────────────────────────────────────────────

#[component]
fn MonthGrid(year: i32, month: u32, tasks: Vec<MyDayTask>, today: NaiveDate) -> impl IntoView {
    let navigate = StoredValue::new(use_navigate());
    let days = days_in_month(year, month);
    let offset = first_weekday(year, month);

    view! {
        <div>
            <div class="text-sm font-semibold text-stone-600 dark:text-stone-300 mb-2 text-center">
                {format!("{} {}", month_name(month), year)}
            </div>

            // Day-of-week headers
            <div class="grid grid-cols-7 mb-1">
                {["Mon","Tue","Wed","Thu","Fri","Sat","Sun"].into_iter().map(|d| view! {
                    <div class="text-center text-xs font-semibold text-stone-400 \
                        dark:text-stone-500 py-1">
                        {d}
                    </div>
                }).collect_view()}
            </div>

            // Day cells
            <div class="grid grid-cols-7 gap-1">
                // Leading blank cells
                {(0..offset).map(|_| view! {
                    <div class="min-h-[80px] rounded-lg bg-stone-50 \
                        dark:bg-stone-900/30 opacity-30"/>
                }).collect_view()}

                // Day cells with tasks
                {(1..=days).map(|day| {
                    let date = NaiveDate::from_ymd_opt(year, month, day)
                        .unwrap_or(today);
                    let is_today = date == today;
                    let day_tasks: Vec<MyDayTask> = tasks.iter()
                        .filter(|t| t.task.due_date == Some(date))
                        .cloned()
                        .collect();

                    let cell_class = if is_today {
                        "min-h-[80px] rounded-lg p-1.5 flex flex-col gap-0.5 \
                            bg-amber-50 dark:bg-amber-900/20 ring-1 ring-amber-400"
                    } else {
                        "min-h-[80px] rounded-lg p-1.5 flex flex-col gap-0.5 \
                            bg-stone-50 dark:bg-stone-900/30 \
                            hover:bg-stone-100 dark:hover:bg-stone-800/50"
                    };

                    let day_label_class = if is_today {
                        "text-xs font-bold text-amber-600 dark:text-amber-400"
                    } else {
                        "text-xs font-medium text-stone-500 dark:text-stone-400"
                    };

                    view! {
                        <div class=cell_class>
                            // Day number
                            <span class=day_label_class>{day.to_string()}</span>

                            // Task chips
                            {day_tasks.into_iter().map(|mt| {
                                let task = mt.task;
                                let done = status_done(&task.status);
                                let color = priority_color_hex(&task.priority);
                                let title = task.title.clone();
                                let node_id = task.node_id;

                                view! {
                                    <button
                                        class="w-full text-left text-xs rounded px-1.5 py-0.5 \
                                            truncate transition-colors hover:opacity-80 cursor-pointer"
                                        style=move || format!(
                                            "background: {}22; color: {}; {}",
                                            color,
                                            color,
                                            if done {
                                                "text-decoration: line-through; opacity: 0.5;"
                                            } else {
                                                ""
                                            }
                                        )
                                        title=title.clone()
                                        on:click=move |_| {
                                            if let Some(nid) = node_id {
                                                navigate.get_value()(&format!("/nodes/{nid}"), Default::default());
                                            }
                                        }
                                        inner_html=crate::markdown::render_markdown_inline(&title)
                                    ></button>
                                }
                            }).collect_view()}
                        </div>
                    }
                }).collect_view()}
            </div>
        </div>
    }
}

// ── CalendarView ─────────────────────────────────────────────────────────────

#[component]
pub fn CalendarView() -> impl IntoView {
    let task_refresh = expect_context::<TaskRefresh>().0;

    let today = crate::components::format_helpers::local_today();
    let year_sig = RwSignal::new(today.year());
    let month_sig = RwSignal::new(today.month());

    // Derived signals for the second month
    let year2 = Memo::new(move |_| {
        let (y, _) = next_month(year_sig.get(), month_sig.get());
        y
    });
    let month2 = Memo::new(move |_| {
        let (_, m) = next_month(year_sig.get(), month_sig.get());
        m
    });

    // Fetch tasks for both months
    let tasks_resource = LocalResource::new(move || {
        let _ = task_refresh.get();
        let year = year_sig.get();
        let month = month_sig.get();
        async move { crate::api::fetch_calendar_tasks(year, month).await }
    });

    let tasks_resource_2 = LocalResource::new(move || {
        let _ = task_refresh.get();
        let y2 = year2.get();
        let m2 = month2.get();
        async move { crate::api::fetch_calendar_tasks(y2, m2).await }
    });

    view! {
        <div class="flex flex-col h-full">
            // Header
            <div class="flex items-center gap-3 px-6 py-4 border-b border-stone-200 dark:border-stone-800">
                <span class="material-symbols-outlined text-amber-500" style="font-size: 22px;">
                    {"calendar_month"}
                </span>
                <div class="flex-1">
                    <h1 class="text-lg font-semibold text-stone-900 dark:text-stone-100">"Calendar"</h1>
                    <p class="text-xs text-stone-400 dark:text-stone-500">"Tasks by due date"</p>
                </div>
                // Month navigation
                <div class="flex items-center gap-2">
                    <button
                        class="p-1.5 rounded-lg text-stone-400 hover:text-stone-600 dark:hover:text-stone-300 \
                            hover:bg-stone-100 dark:hover:bg-stone-800 transition-colors cursor-pointer"
                        title="Previous month"
                        on:click=move |_| {
                            let (y, m) = prev_month(year_sig.get_untracked(), month_sig.get_untracked());
                            year_sig.set(y);
                            month_sig.set(m);
                        }
                    >
                        <span class="material-symbols-outlined">"chevron_left"</span>
                    </button>
                    <span class="text-sm font-semibold text-stone-700 dark:text-stone-300 min-w-[200px] text-center">
                        {move || two_month_label(year_sig.get(), month_sig.get(), year2.get(), month2.get())}
                    </span>
                    <button
                        class="p-1.5 rounded-lg text-stone-400 hover:text-stone-600 dark:hover:text-stone-300 \
                            hover:bg-stone-100 dark:hover:bg-stone-800 transition-colors cursor-pointer"
                        title="Next month"
                        on:click=move |_| {
                            let (y, m) = next_month(year_sig.get_untracked(), month_sig.get_untracked());
                            year_sig.set(y);
                            month_sig.set(m);
                        }
                    >
                        <span class="material-symbols-outlined">"chevron_right"</span>
                    </button>
                    <button
                        class="ml-2 px-3 py-1 text-xs rounded-lg bg-stone-100 dark:bg-stone-800 \
                            text-stone-600 dark:text-stone-400 hover:bg-stone-200 dark:hover:bg-stone-700 \
                            transition-colors cursor-pointer"
                        title="Jump to today"
                        on:click=move |_| {
                            year_sig.set(today.year());
                            month_sig.set(today.month());
                        }
                    >
                        "Today"
                    </button>
                </div>
            </div>

            // Two-month calendar grid
            <div class="flex-1 overflow-auto p-4">
                <Suspense fallback=move || view! {
                    <div class="flex items-center justify-center h-32 text-stone-400 text-sm">
                        "Loading\u{2026}"
                    </div>
                }>
                {move || {
                    let tasks_1: Vec<MyDayTask> = tasks_resource.get()
                        .and_then(|r| r.ok())
                        .unwrap_or_default();
                    let tasks_2: Vec<MyDayTask> = tasks_resource_2.get()
                        .and_then(|r| r.ok())
                        .unwrap_or_default();
                    let y1 = year_sig.get();
                    let m1 = month_sig.get();
                    let y2 = year2.get();
                    let m2 = month2.get();

                    view! {
                        <div class="grid grid-cols-1 gap-8">
                            <MonthGrid year=y1 month=m1 tasks=tasks_1 today=today />
                            <MonthGrid year=y2 month=m2 tasks=tasks_2 today=today />
                        </div>
                    }
                }}
                </Suspense>
            </div>
        </div>
    }
}
