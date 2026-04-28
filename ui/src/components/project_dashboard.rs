//! Project Dashboard — PARA grouping + pinning + activity recap (v2.9.0).
//!
//! Three sections, top to bottom:
//!
//! 1. **Activity recap** — "Today" and "Yesterday" headings listing
//!    recent activity entries across every node the user owns.  Pulled
//!    from `GET /api/dashboard/activity` (defaults to last 48h).
//!    Empty state hidden — if nothing changed in 48h the section
//!    just isn't rendered.
//! 2. **Project groups by Area** — projects collapsed under their
//!    parent Area name (PARA model).  Projects with no Area parent
//!    land in an "Ungrouped" bucket at the bottom of the list.
//!    Within each Area: pinned projects first (`★`), then by
//!    `last_activity_at` descending.
//! 3. *(was: per-card details)* — kept inline in `ProjectCard` so
//!    each card still surfaces its `## Status` markdown, top open
//!    tasks, and counts.  No layout change there.
//!
//! Pin/unpin: each card has a star button.  Click flips the bit via
//! `PUT /api/nodes/:id/pin`; on success the dashboard refreshes so
//! the card slides up/down to its new sort position.

use chrono::{DateTime, Duration, Utc};
use common::activity::RecentActivityEntry;
use common::id::NodeId;
use common::task::{ProjectDashboardEntry, TaskSummary};
use leptos::prelude::*;
use leptos_router::hooks::use_navigate;

use crate::app::TaskRefresh;
use crate::components::format_helpers::format_relative_short;
use crate::components::node_meta::{status_color, status_icon, status_label};
use crate::components::task_common::priority_color_hex;
use crate::components::toast::{push_toast, ToastLevel};
use crate::markdown::render_markdown_plain;

#[component]
pub fn ProjectDashboard() -> impl IntoView {
    let navigate = StoredValue::new(use_navigate());

    let task_refresh = use_context::<TaskRefresh>()
        .expect("TaskRefresh context must be provided")
        .0;

    // Refresh signal local to the dashboard so pin toggles refetch
    // both project list and the activity recap.
    let dashboard_refresh = RwSignal::new(0u32);

    let entries = LocalResource::new(move || {
        let _ = task_refresh.get();
        let _ = dashboard_refresh.get();
        async move { crate::api::fetch_project_dashboard().await }
    });
    let activity = LocalResource::new(move || {
        let _ = task_refresh.get();
        let _ = dashboard_refresh.get();
        // Fetch the last 48h so "Today" + "Yesterday" both have content
        // regardless of when the user opens the dashboard.
        let since = (Utc::now() - Duration::hours(48)).to_rfc3339();
        async move { crate::api::fetch_dashboard_activity(&since, 50).await }
    });

    view! {
        <div class="flex flex-col h-full">
            // Header
            <div class="flex items-center gap-3 px-6 py-4 border-b border-stone-200 dark:border-stone-800">
                <span class="material-symbols-outlined text-amber-500" style="font-size: 22px;">
                    {"rocket_launch"}
                </span>
                <h1 class="text-lg font-semibold text-stone-900 dark:text-stone-100">
                    "Project Dashboard"
                </h1>
            </div>

            <div class="flex-1 overflow-auto p-6 flex flex-col space-y-6">

                // ── Activity recap ─────────────────────────────────────
                <Suspense fallback=move || view! {
                    <crate::components::skeleton::SkeletonBar />
                }>
                    {move || {
                        let recap = activity.get().and_then(|r| r.ok()).unwrap_or_default();
                        view! { <ActivityRecap entries=recap /> }
                    }}
                </Suspense>

                // ── Project groups ─────────────────────────────────────
                <Suspense fallback=move || view! {
                    <crate::components::skeleton::SkeletonCards cards=3 />
                }>
                    {move || {
                        let data = entries.get().and_then(|r| r.ok()).unwrap_or_default();
                        if data.is_empty() {
                            return view! {
                                <div class="flex-1 flex flex-col items-center justify-center gap-3">
                                    <span class="material-symbols-outlined text-stone-300 dark:text-stone-700"
                                        style="font-size: 48px;">{"rocket_launch"}</span>
                                    <p class="text-stone-400 dark:text-stone-500 text-sm">
                                        "No projects yet. Create a Project node to get started."
                                    </p>
                                </div>
                            }.into_any();
                        }

                        // Group by area_title.  None → "Ungrouped".
                        // Preserve insertion order: walk the (already-
                        // sorted-by-pinned-then-recency) list, and bucket
                        // into a Vec to keep stable group ordering.
                        let mut groups: Vec<(Option<String>, Vec<ProjectDashboardEntry>)> = Vec::new();
                        for entry in data {
                            let key = entry.area_title.clone();
                            if let Some((_, bucket)) = groups
                                .iter_mut()
                                .find(|(k, _)| k == &key)
                            {
                                bucket.push(entry);
                            } else {
                                groups.push((key, vec![entry]));
                            }
                        }
                        // Sort: named Areas first (alphabetical), Ungrouped last.
                        groups.sort_by(|(a, _), (b, _)| match (a, b) {
                            (Some(x), Some(y)) => x.to_lowercase().cmp(&y.to_lowercase()),
                            (Some(_), None)    => std::cmp::Ordering::Less,
                            (None, Some(_))    => std::cmp::Ordering::Greater,
                            (None, None)       => std::cmp::Ordering::Equal,
                        });

                        view! {
                            <div class="space-y-6">
                                {groups.into_iter().map(|(area_title, projects)| {
                                    let header = area_title.clone().unwrap_or_else(|| "Ungrouped".to_string());
                                    let count = projects.len();
                                    view! {
                                        <section>
                                            <header class="flex items-center gap-2 mb-2">
                                                <span class="material-symbols-outlined text-amber-600 dark:text-amber-500"
                                                      style="font-size:14px;">
                                                    {if area_title.is_some() { "category" } else { "more_horiz" }}
                                                </span>
                                                <h2 class="text-xs font-semibold uppercase tracking-wider \
                                                           text-amber-700 dark:text-amber-400">
                                                    {header}
                                                </h2>
                                                <span class="text-xs text-stone-400 dark:text-stone-500">
                                                    {format!("({count})")}
                                                </span>
                                            </header>
                                            <div class="space-y-3">
                                                {projects.into_iter().map(|entry| {
                                                    let node_id = entry.node_id;
                                                    view! {
                                                        <ProjectCard
                                                            entry=entry
                                                            on_navigate=move || {
                                                                navigate.get_value()(
                                                                    &format!("/nodes/{node_id}"),
                                                                    Default::default(),
                                                                )
                                                            }
                                                            refresh=dashboard_refresh
                                                        />
                                                    }
                                                }).collect_view()}
                                            </div>
                                        </section>
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

// ── Activity recap ──────────────────────────────────────────────────────────

#[component]
fn ActivityRecap(entries: Vec<RecentActivityEntry>) -> impl IntoView {
    if entries.is_empty() {
        // Empty state hidden — nothing to recap = no section.
        return ().into_any();
    }
    let now = Utc::now();
    let today_start = now.date_naive().and_hms_opt(0, 0, 0).unwrap().and_utc();
    let yesterday_start = today_start - Duration::days(1);

    let (today_entries, older): (Vec<_>, Vec<_>) = entries
        .into_iter()
        .partition(|e| e.entry.created_at >= today_start);
    let yesterday_entries: Vec<_> = older
        .into_iter()
        .filter(|e| e.entry.created_at >= yesterday_start)
        .collect();

    if today_entries.is_empty() && yesterday_entries.is_empty() {
        return ().into_any();
    }

    view! {
        <section class="rounded-lg border border-stone-200 dark:border-stone-800 \
                        bg-stone-50/40 dark:bg-stone-900/30 p-4">
            <header class="flex items-center gap-2 mb-3">
                <span class="material-symbols-outlined text-amber-600 dark:text-amber-500"
                      style="font-size:16px;">"history"</span>
                <h2 class="text-xs font-semibold uppercase tracking-wider \
                           text-amber-700 dark:text-amber-400">
                    "Recent activity"
                </h2>
            </header>
            <div class="space-y-3">
                {(!today_entries.is_empty()).then(|| view! {
                    <ActivityGroup label="Today" entries=today_entries />
                })}
                {(!yesterday_entries.is_empty()).then(|| view! {
                    <ActivityGroup label="Yesterday" entries=yesterday_entries />
                })}
            </div>
        </section>
    }.into_any()
}

#[component]
fn ActivityGroup(label: &'static str, entries: Vec<RecentActivityEntry>) -> impl IntoView {
    view! {
        <div>
            <h3 class="text-[10px] font-semibold uppercase tracking-wider \
                       text-stone-500 dark:text-stone-400 mb-1">
                {label}
            </h3>
            <ul class="space-y-1 text-sm">
                {entries.into_iter().map(|e| {
                    let time = e.entry.created_at.format("%-I:%M %p").to_string();
                    let icon = e.entry.action.icon();
                    let action_label = e.entry.action.label();
                    let title = e.node_title.clone();
                    let node_id = e.entry.node_id;
                    view! {
                        <li class="flex items-center gap-2">
                            <span class="text-stone-400 dark:text-stone-500 font-mono text-xs \
                                         flex-shrink-0 w-16">
                                {time}
                            </span>
                            <span class="material-symbols-outlined text-stone-400 dark:text-stone-500 \
                                         flex-shrink-0" style="font-size:14px;">
                                {icon}
                            </span>
                            <span class="text-stone-500 dark:text-stone-400 text-xs flex-shrink-0">
                                {action_label}
                            </span>
                            <a
                                href=format!("/nodes/{node_id}")
                                class="text-stone-700 dark:text-stone-200 truncate \
                                       hover:text-amber-700 dark:hover:text-amber-400 \
                                       transition-colors"
                            >
                                {title}
                            </a>
                        </li>
                    }
                }).collect_view()}
            </ul>
        </div>
    }
}

// ── Project Card ────────────────────────────────────────────────────────────
//
// Refactored from v2.6.x.  Adds: pin button (left of title) + uses the new
// `dashboard_refresh` signal to re-fetch after a pin toggle.

#[component]
fn ProjectCard(
    entry: ProjectDashboardEntry,
    on_navigate: impl Fn() + 'static,
    refresh: RwSignal<u32>,
) -> impl IntoView {
    let counts = entry.task_counts.clone();
    let total = counts.open + counts.in_progress + counts.done + counts.cancelled;
    let done_pct = if total > 0 {
        (counts.done * 100 / total) as f32
    } else {
        0.0
    };

    let s_icon = status_icon(&entry.node_status);
    let s_label = status_label(&entry.node_status);
    let s_color = status_color(&entry.node_status);

    let status_html = entry.status_section.as_deref().map(render_markdown_plain);
    let open_tasks = entry.open_tasks.clone();
    let has_more = entry.has_more_tasks;
    let open_count = counts.open + counts.in_progress;
    let updated_label = format_relative_short(&entry.last_activity_at);

    let node_id = entry.node_id;
    let pinned_sig = RwSignal::new(entry.pinned);
    let pin_busy = RwSignal::new(false);

    let on_pin_click = move |ev: web_sys::MouseEvent| {
        ev.stop_propagation();
        if pin_busy.get_untracked() { return; }
        let next = !pinned_sig.get_untracked();
        pinned_sig.set(next);  // optimistic
        pin_busy.set(true);
        wasm_bindgen_futures::spawn_local(async move {
            match crate::api::set_node_pinned(node_id, next).await {
                Ok(_)  => {
                    push_toast(ToastLevel::Success,
                        if next { "Pinned" } else { "Unpinned" });
                    refresh.update(|n| *n += 1);
                }
                Err(e) => {
                    pinned_sig.set(!next);  // rollback
                    push_toast(ToastLevel::Error, format!("Pin failed: {e}"));
                }
            }
            pin_busy.set(false);
        });
    };

    view! {
        <div
            class="rounded-lg border border-stone-200 dark:border-stone-800
                bg-stone-50 dark:bg-stone-800/40
                hover:border-amber-300 dark:hover:border-amber-700
                transition-colors cursor-pointer group overflow-hidden"
            on:click=move |_| on_navigate()
        >
            <div class="flex flex-col sm:flex-row sm:items-center gap-2 sm:gap-4 px-4 py-3">
                // Title row — pin button + icon + title
                <div class="flex items-center gap-2 min-w-0 sm:flex-1">
                    <button
                        type="button"
                        class="flex-shrink-0 p-1 rounded transition-colors cursor-pointer"
                        style=move || if pinned_sig.get() {
                            "color:#f59e0b;"
                        } else {
                            "color:#9ca3af;"
                        }
                        title=move || if pinned_sig.get() {
                            "Unpin (currently pinned to top of group)"
                        } else {
                            "Pin to top of group"
                        }
                        on:click=on_pin_click
                    >
                        <span class="material-symbols-outlined" style="font-size:16px;font-variation-settings:'FILL' 1;">
                            {move || if pinned_sig.get() { "star" } else { "star_outline" }}
                        </span>
                    </button>
                    <span class="material-symbols-outlined text-amber-500 flex-shrink-0"
                        style="font-size: 16px;">{"rocket_launch"}</span>
                    <span class="font-medium text-stone-800 dark:text-stone-200 truncate
                        group-hover:text-amber-700 dark:group-hover:text-amber-400 transition-colors">
                        {entry.title.clone()}
                    </span>
                </div>

                <div class="flex items-center flex-wrap gap-x-3 gap-y-2 sm:gap-4 sm:flex-nowrap">
                    <div class="flex items-center gap-1 text-sm flex-shrink-0" style=s_color>
                        <span class="material-symbols-outlined" style="font-size: 15px;">{s_icon}</span>
                        <span class="text-xs">{s_label}</span>
                    </div>

                    <div class="hidden sm:flex items-center gap-1 text-xs
                        text-stone-500 dark:text-stone-400 flex-shrink-0"
                        title="Most recent activity across the project and its tasks">
                        <span class="material-symbols-outlined" style="font-size: 14px;">{"history"}</span>
                        <span>{updated_label}</span>
                    </div>

                    <div class="flex items-center gap-1 flex-shrink-0">
                        <CountBadge count=counts.open
                            color="bg-stone-200 dark:bg-stone-700 text-stone-700 dark:text-stone-300" />
                        <CountBadge count=counts.in_progress
                            color="bg-amber-100 dark:bg-amber-900/30 text-amber-700 dark:text-amber-400" />
                        <CountBadge count=counts.done
                            color="bg-green-100 dark:bg-green-900/30 text-green-700 dark:text-green-400" />
                        <CountBadge count=counts.cancelled
                            color="bg-stone-100 dark:bg-stone-800 text-stone-400 dark:text-stone-500" />
                    </div>

                    <div class="flex items-center gap-2 w-28 flex-shrink-0">
                        {if total == 0 {
                            view! {
                                <span class="text-xs text-stone-400 dark:text-stone-500 italic">"No tasks"</span>
                            }.into_any()
                        } else {
                            view! {
                                <div class="flex items-center gap-2 w-full">
                                    <div class="flex-1 h-1.5 bg-stone-200 dark:bg-stone-700 rounded-full overflow-hidden">
                                        <div
                                            class="h-full bg-green-500 rounded-full transition-all"
                                            style=format!("width: {done_pct:.0}%")
                                        />
                                    </div>
                                    <span class="text-xs text-stone-500 dark:text-stone-400 flex-shrink-0">
                                        {format!("{done_pct:.0}%")}
                                    </span>
                                </div>
                            }.into_any()
                        }}
                    </div>
                </div>
            </div>

            // Expandable detail section — unchanged from v2.6.x
            {(status_html.is_some() || !open_tasks.is_empty()).then(|| {
                view! {
                    <div class="border-t border-stone-200 dark:border-stone-700/60 px-4 py-3
                        grid grid-cols-1 md:grid-cols-2 gap-4">
                        {status_html.map(|html| view! {
                            <div>
                                <div class="text-xs font-semibold uppercase tracking-wider
                                    text-stone-400 dark:text-stone-500 mb-1.5">
                                    "Status"
                                </div>
                                <div
                                    class="prose prose-sm dark:prose-invert max-w-none
                                        text-stone-600 dark:text-stone-300
                                        [&_ul]:list-disc [&_ul]:pl-4 [&_ol]:list-decimal [&_ol]:pl-4
                                        [&_li]:my-0.5 [&_p]:my-1"
                                    inner_html=html
                                />
                            </div>
                        })}

                        {(!open_tasks.is_empty()).then(|| view! {
                            <div>
                                <div class="text-xs font-semibold uppercase tracking-wider
                                    text-stone-400 dark:text-stone-500 mb-1.5">
                                    {format!("Open Tasks ({})", open_count)}
                                </div>
                                <div class="space-y-1">
                                    {open_tasks.into_iter().map(|t| view! {
                                        <TaskChip task=t />
                                    }).collect_view()}
                                    {has_more.then(|| view! {
                                        <div class="text-xs text-stone-400 dark:text-stone-500 italic pl-5 pt-0.5">
                                            "\u{2026}and more"
                                        </div>
                                    })}
                                </div>
                            </div>
                        })}
                    </div>
                }
            })}
        </div>
    }
}

// ── Task chip (unchanged from v2.6.x) ───────────────────────────────────────

#[component]
fn TaskChip(task: TaskSummary) -> impl IntoView {
    let color = priority_color_hex(&task.priority);
    let due_label = task
        .due_date
        .map(|d| d.format("%b %-d").to_string())
        .unwrap_or_default();
    let has_due = task.due_date.is_some();
    let in_progress = matches!(task.status, common::task::TaskStatus::InProgress);

    view! {
        <div class="flex items-center gap-1.5 text-xs py-0.5 group/task">
            <span
                class="w-2 h-2 rounded-full flex-shrink-0"
                style=format!("background-color: {color};")
            />
            <span class="truncate text-stone-700 dark:text-stone-300 flex-1 min-w-0">
                {task.title}
            </span>
            {in_progress.then(|| view! {
                <span class="text-[10px] px-1.5 py-0.5 rounded
                    bg-amber-100 dark:bg-amber-900/30
                    text-amber-700 dark:text-amber-400 flex-shrink-0">
                    "In Progress"
                </span>
            })}
            {has_due.then(|| view! {
                <span class="text-stone-400 dark:text-stone-500 flex-shrink-0 whitespace-nowrap">
                    {due_label}
                </span>
            })}
        </div>
    }
}

// ── Count badge (unchanged from v2.6.x) ─────────────────────────────────────

#[component]
fn CountBadge(count: u32, color: &'static str) -> impl IntoView {
    view! {
        <span class=format!("inline-flex items-center justify-center w-7 h-5 rounded text-xs font-semibold {color}")>
            {count.to_string()}
        </span>
    }
}

// Suppress dead-code warning on unused import when the dashboard
// later becomes tag-aware.
#[allow(dead_code)]
fn _phantom(_: NodeId, _: DateTime<Utc>) {}
