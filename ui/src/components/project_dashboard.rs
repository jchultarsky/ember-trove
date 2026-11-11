//! Project Dashboard — Focus + activity recap + PARA grouping + pinning.
//!
//! Top to bottom:
//!
//! 0. **Focus** (#282) — the projects the viewer put in their Focus, in
//!    their order, from `GET /api/focus`. Private to the viewer; it may
//!    hold projects shared with them, which the groups below never list.
//!    Each card says how long it has been there and why.
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

use chrono::{Duration, Utc};
use common::activity::{ActivityAction, RecentActivityEntry, status_change_summary};
use common::id::NodeId;
use common::project_focus::{FocusEntry, FocusItem, Step, in_focus_label, moved, over_soft_limit};
use common::task::{ProjectDashboardEntry, TaskSummary};
use leptos::prelude::*;
use leptos_router::hooks::use_navigate;

use crate::app::TaskRefresh;
use crate::components::empty_state::EmptyState;
use crate::components::format_helpers::{
    format_relative_short, format_timestamp, local_date, local_today,
};
use crate::components::icon_button::IconButton;
use crate::components::node_meta::{status_color, status_icon, status_label};
use crate::components::page_header::PageHeader;
use crate::components::project_focus::{FocusToggle, IN_FOCUS_ICON, WhyLine};
use crate::components::task_common::priority_color_hex;
use crate::components::toast::{ToastLevel, push_toast};
use crate::markdown::render_markdown_plain;

#[component]
pub fn ProjectDashboard() -> impl IntoView {
    let navigate = StoredValue::new(use_navigate());

    let task_refresh = expect_context::<TaskRefresh>().0;

    // Refresh signal local to the dashboard so pin toggles refetch
    // both project list and the activity recap.
    let dashboard_refresh = RwSignal::new(0u32);

    let entries = LocalResource::new(move || {
        let _ = task_refresh.get();
        let _ = dashboard_refresh.get();
        async move { crate::api::fetch_project_dashboard().await }
    });
    // The viewer's Focus (#282). Refetched with the rest, so a toggle on
    // any card moves the project in or out of the section.
    let focus = LocalResource::new(move || {
        let _ = task_refresh.get();
        let _ = dashboard_refresh.get();
        async move { crate::api::fetch_focus().await }
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
            <PageHeader icon="rocket_launch" title="Project Dashboard" />

            <div class="flex-1 overflow-auto p-6 flex flex-col space-y-6">

                // Transition, not Suspense (#123): a Focus or pin toggle
                // refetches, and a Suspense would swap the page for its
                // skeleton each time and lose the scroll position.

                // ── Focus (#282) ───────────────────────────────────────
                <Transition fallback=move || view! {
                    <crate::components::skeleton::SkeletonBar />
                }>
                    {move || match focus.get() {
                        None => ().into_any(),
                        // A failed load is said, not shown as an empty Focus.
                        Some(Err(_)) => view! {
                            <crate::components::load_error::LoadError
                                what="your Focus"
                                on_retry=Callback::new(move |()| dashboard_refresh.update(|n| *n += 1))
                            />
                        }.into_any(),
                        Some(Ok(list)) => {
                            // The hint for an empty Focus only makes sense
                            // to someone who has a project to put there.
                            let has_projects = entries
                                .get()
                                .and_then(Result::ok)
                                .is_some_and(|projects| !projects.is_empty());
                            view! {
                                <FocusSection
                                    entries=list
                                    show_empty=has_projects
                                    refresh=dashboard_refresh
                                />
                            }.into_any()
                        }
                    }}
                </Transition>

                // ── Activity recap ─────────────────────────────────────
                <Transition fallback=move || view! {
                    <crate::components::skeleton::SkeletonBar />
                }>
                    {move || {
                        let recap = activity.get().and_then(|r| r.ok()).unwrap_or_default();
                        view! { <ActivityRecap entries=recap /> }
                    }}
                </Transition>

                // ── Project groups ─────────────────────────────────────
                <Transition fallback=move || view! {
                    <crate::components::skeleton::SkeletonCards cards=3 />
                }>
                    {move || {
                        // Which of these are in the viewer's Focus. `None`
                        // when Focus didn't load: the cards then show no
                        // toggle rather than a wrong one.
                        let focused: Option<Vec<NodeId>> = focus
                            .get()
                            .and_then(Result::ok)
                            .map(|list| list.iter().map(|e| e.focus.node_id).collect());
                        // A failed load is not "No projects yet" (#281, 1.7).
                        let data = match entries.get() {
                            Some(Err(_)) => return view! {
                                <div class="flex-1 flex flex-col justify-center">
                                    <crate::components::load_error::LoadError
                                        what="your projects"
                                        on_retry=Callback::new(move |()| dashboard_refresh.update(|n| *n += 1))
                                    />
                                </div>
                            }.into_any(),
                            other => other.and_then(Result::ok).unwrap_or_default(),
                        };
                        if data.is_empty() {
                            return view! {
                                <div class="flex-1 flex flex-col justify-center">
                                    <EmptyState
                                        icon="rocket_launch"
                                        message="No projects yet."
                                        hint="Create a Project node to get started."
                                    />
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
                            <div class="space-y-6" data-testid="project-groups">
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
                                                    let in_focus = focused
                                                        .as_ref()
                                                        .map(|ids| ids.contains(&node_id));
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
                                                            focus=CardFocus::Toggle(in_focus)
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
                </Transition>
            </div>
        </div>
    }
}

// ── Focus (#282) ────────────────────────────────────────────────────────────

/// How a card relates to the viewer's Focus.
#[derive(Clone)]
enum CardFocus {
    /// A card in an Area group: the add/remove toggle, showing whether the
    /// project is in Focus. `None`: Focus didn't load, so no toggle.
    Toggle(Option<bool>),
    /// A card in the Focus section: its age and why-line, and the controls
    /// that move it or take it out.
    Listed {
        item: FocusItem,
        /// Whether the card is at that end of the order now. Signals: a
        /// move reorders the cards in place and doesn't rebuild them.
        first: Signal<bool>,
        last: Signal<bool>,
        on_move: Callback<(NodeId, Step)>,
    },
}

/// The dashboard's first section: the viewer's Focus, in their order.
#[component]
fn FocusSection(
    entries: Vec<FocusEntry>,
    /// Whether to say how to use Focus when it is empty.
    show_empty: bool,
    refresh: RwSignal<u32>,
) -> impl IntoView {
    let navigate = StoredValue::new(use_navigate());
    // A local copy, so a move shows at once.
    let list = RwSignal::new(entries);

    // Reorders go to the server one at a time, and only the latest order
    // waits: two moves in quick succession used to send two requests that
    // could land in either order, leaving the stored order behind the page.
    let waiting = StoredValue::new(None::<Vec<NodeId>>);
    let sending = StoredValue::new(false);

    let on_move = Callback::new(move |(node_id, step): (NodeId, Step)| {
        let ids: Vec<NodeId> = list.with_untracked(|l| l.iter().map(|e| e.focus.node_id).collect());
        let Some(next) = moved(&ids, node_id, step) else {
            return;
        };
        let place = {
            let next = next.clone();
            move |e: &FocusEntry| next.iter().position(|id| *id == e.focus.node_id)
        };
        list.update(|l| l.sort_by_key(|e| place(e)));
        waiting.set_value(Some(next));
        if sending.get_value() {
            return;
        }
        sending.set_value(true);
        wasm_bindgen_futures::spawn_local(async move {
            // `try_`: the section is rebuilt on a refetch, and may be gone.
            while let Some(order) = waiting.try_update_value(Option::take).flatten() {
                if let Err(e) = crate::api::set_focus_order(&order).await {
                    push_toast(ToastLevel::Error, format!("Couldn't reorder Focus: {e}"));
                    // Show the order the server still has.
                    let _ = refresh.try_update(|n| *n += 1);
                    break;
                }
            }
            let _ = sending.try_set_value(false);
        });
    });

    let header = move |count: usize| {
        view! {
            <header class="flex flex-wrap items-center gap-x-2 gap-y-1 mb-3">
                <span aria-hidden="true" class="material-symbols-outlined text-amber-600 dark:text-amber-500"
                      style="font-size:16px;">{IN_FOCUS_ICON}</span>
                <h2 class="text-xs font-semibold uppercase tracking-wider \
                           text-amber-700 dark:text-amber-400">
                    "Focus"
                </h2>
                {(count > 0).then(|| view! {
                    <span class="text-xs text-stone-400 dark:text-stone-500">
                        {format!("({count})")}
                    </span>
                })}
                // A hint, never a limit (ONTOLOGY §12).
                {over_soft_limit(count).then(|| view! {
                    <span class="text-xs text-stone-500 dark:text-stone-400" data-testid="focus-hint">
                        {format!("Focus works best small: {count} projects are in it.")}
                    </span>
                })}
            </header>
        }
    };

    // The count never changes while this section lives (an add or a remove
    // refetches and rebuilds it), so a move re-renders nothing here.
    let count = Memo::new(move |_| list.with(Vec::len));
    move || {
        let count = count.get();
        if count == 0 {
            return show_empty.then(|| view! {
                <section class="rounded-lg border border-stone-200 dark:border-stone-800 \
                                bg-stone-50/40 dark:bg-stone-900/30 p-4"
                         data-testid="focus-section">
                    {header(0)}
                    <p class="text-sm italic text-stone-500 dark:text-stone-400" data-testid="focus-empty">
                        "Nothing in Focus. Add a project with the Focus button on its card."
                    </p>
                </section>
            }).into_any();
        }
        view! {
            <section class="rounded-lg border border-amber-200 dark:border-amber-900/50 \
                            bg-amber-50/30 dark:bg-amber-900/10 p-4"
                     data-testid="focus-section">
                {header(count)}
                <div class="space-y-3">
                    // Keyed by project: a move reorders the cards in place.
                    // Rebuilding them from the fetched list showed a
                    // why-line saved a moment ago as it was before, and
                    // closed an editor that was open on another card.
                    <For
                        each=move || list.get()
                        key=|entry| entry.focus.node_id
                        children=move |entry| {
                            let node_id = entry.focus.node_id;
                            let at = move |end: fn(&[FocusEntry]) -> Option<&FocusEntry>| {
                                Signal::derive(move || {
                                    list.with(|l| end(l).is_some_and(|e| e.focus.node_id == node_id))
                                })
                            };
                            view! {
                                <ProjectCard
                                    entry=entry.project
                                    on_navigate=move || {
                                        navigate.get_value()(
                                            &format!("/nodes/{node_id}"),
                                            Default::default(),
                                        )
                                    }
                                    refresh=refresh
                                    focus=CardFocus::Listed {
                                        item: entry.focus,
                                        first: at(<[FocusEntry]>::first),
                                        last: at(<[FocusEntry]>::last),
                                        on_move,
                                    }
                                />
                            }
                        }
                    />
                </div>
            </section>
        }
        .into_any()
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
    // Midnight is infallible via `NaiveTime::MIN`; avoids an `.unwrap()` on `and_hms_opt`.
    let today_start = now.date_naive().and_time(chrono::NaiveTime::MIN).and_utc();
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
    }
    .into_any()
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
                    // A status change says what it became, not just that it
                    // changed (#208).
                    let detail = (e.entry.action == ActivityAction::StatusChanged).then(|| {
                        let label = |k: &str| e.entry.metadata.get(k).and_then(|v| v.as_str());
                        status_change_summary(label("from"), label("to"))
                    });
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
                            {detail.map(|d| view! {
                                <span class="text-stone-400 dark:text-stone-500 text-xs flex-shrink-0">
                                    {d}
                                </span>
                            })}
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
    focus: CardFocus,
) -> impl IntoView {
    let counts = entry.task_counts.clone();
    let total = counts.open + counts.in_progress + counts.done + counts.cancelled;
    let done_pct = (counts.done * 100)
        .checked_div(total)
        .map_or(0.0, |v| v as f32);

    let s_icon = status_icon(&entry.node_status);
    let s_label = status_label(&entry.node_status);
    let s_color = status_color(&entry.node_status);

    // The Brief is the project's narrative (#282). The body's scraped
    // `## Status` section is the fallback for a project that has none yet.
    let brief = entry.brief.clone();
    let status_html = brief
        .is_none()
        .then(|| entry.status_section.as_deref().map(render_markdown_plain))
        .flatten();
    let open_tasks = entry.open_tasks.clone();
    let has_more = entry.has_more_tasks;
    let open_count = counts.open + counts.in_progress;
    let updated_label = format_relative_short(&entry.last_activity_at);

    let node_id = entry.node_id;
    let pinned_sig = RwSignal::new(entry.pinned);
    let pin_busy = RwSignal::new(false);

    let on_pin_click = move |ev: web_sys::MouseEvent| {
        ev.stop_propagation();
        if pin_busy.get_untracked() {
            return;
        }
        let next = !pinned_sig.get_untracked();
        pinned_sig.set(next); // optimistic
        pin_busy.set(true);
        wasm_bindgen_futures::spawn_local(async move {
            match crate::api::set_node_pinned(node_id, next).await {
                Ok(_) => {
                    push_toast(
                        ToastLevel::Success,
                        if next { "Pinned" } else { "Unpinned" },
                    );
                    refresh.update(|n| *n += 1);
                }
                Err(e) => {
                    pinned_sig.set(!next); // rollback
                    push_toast(ToastLevel::Error, format!("Pin failed: {e}"));
                }
            }
            pin_busy.set(false);
        });
    };

    // In the Focus section the card leads with its Focus strip, and has
    // no pin star: pinning orders an Area group, and belongs to the
    // project's owner, who may not be the viewer here.
    let listed = matches!(focus, CardFocus::Listed { .. });
    let (strip, toggle) = match focus {
        CardFocus::Listed {
            item,
            first,
            last,
            on_move,
        } => {
            let removing = RwSignal::new(false);
            let remove = move || {
                if removing.get_untracked() {
                    return;
                }
                removing.set(true);
                wasm_bindgen_futures::spawn_local(async move {
                    let result = crate::api::remove_focus(node_id).await;
                    let _ = removing.try_set(false);
                    match result {
                        Ok(()) => {
                            push_toast(ToastLevel::Success, "Removed from Focus");
                            let _ = refresh.try_update(|n| *n += 1);
                        }
                        Err(e) => {
                            push_toast(ToastLevel::Error, format!("Couldn't change Focus: {e}"))
                        }
                    }
                });
            };
            // The reader's own clock and calendar, not UTC's.
            let since = format_timestamp(&item.added_at);
            let strip = view! {
                // Its own surface: a click here edits Focus, it doesn't
                // open the project.
                <div class="flex flex-wrap items-center gap-x-3 gap-y-1 px-4 py-2 cursor-default
                        border-b border-amber-200/70 dark:border-amber-900/40
                        bg-amber-50/70 dark:bg-amber-900/15"
                    data-testid="focus-strip"
                    on:click=move |ev| ev.stop_propagation()
                >
                    <span class="inline-flex items-center gap-1 flex-shrink-0 text-xs font-medium
                            text-amber-700 dark:text-amber-400"
                        data-testid="focus-age"
                        title=format!("In your Focus since {since}")>
                        <span aria-hidden="true" class="material-symbols-outlined" style="font-size:14px;">
                            {IN_FOCUS_ICON}
                        </span>
                        {in_focus_label(local_date(&item.added_at), local_today())}
                    </span>
                    // `basis-48`: at phone width the why-line takes its
                    // own row instead of squeezing to a few characters.
                    <div class="flex items-center min-w-0 flex-1 basis-48">
                        <WhyLine
                            node_id=node_id
                            initial=item.why_now
                            on_gone=Callback::new(move |()| {
                                let _ = refresh.try_update(|n| *n += 1);
                            })
                        />
                    </div>
                    <div class="flex items-center gap-0.5 flex-shrink-0 ml-auto">
                        <IconButton
                            icon="keyboard_arrow_up"
                            label="Move up"
                            disabled=first
                            on_click=Callback::new(move |()| on_move.run((node_id, Step::Up)))
                        />
                        <IconButton
                            icon="keyboard_arrow_down"
                            label="Move down"
                            disabled=last
                            on_click=Callback::new(move |()| on_move.run((node_id, Step::Down)))
                        />
                        <IconButton
                            icon="close"
                            label="Remove from Focus"
                            disabled=removing
                            on_click=Callback::new(move |()| remove())
                        />
                    </div>
                </div>
            };
            (Some(strip), None)
        }
        CardFocus::Toggle(in_focus) => {
            let state = RwSignal::new(in_focus);
            let toggle = view! {
                <FocusToggle
                    node_id=node_id
                    state=state
                    compact=true
                    on_changed=Callback::new(move |_| refresh.update(|n| *n += 1))
                />
            };
            (None, Some(toggle))
        }
    };

    view! {
        <div
            class="rounded-lg border border-stone-200 dark:border-stone-800
                bg-stone-50 dark:bg-stone-800/40
                hover:border-amber-300 dark:hover:border-amber-700
                transition-colors cursor-pointer group overflow-hidden"
            data-testid="project-card"
            on:click=move |_| on_navigate()
        >
            {strip}
            <div class="flex flex-col sm:flex-row sm:items-center gap-2 sm:gap-4 px-4 py-3">
                // Title row — pin button + Focus toggle + icon + title
                <div class="flex items-center gap-2 min-w-0 sm:flex-1">
                    {(!listed).then(|| view! {
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
                        <span aria-hidden="true" class="material-symbols-outlined" style="font-size:16px;font-variation-settings:'FILL' 1;">
                            {move || if pinned_sig.get() { "star" } else { "star_outline" }}
                        </span>
                    </button>
                    })}
                    {toggle}
                    <span class="material-symbols-outlined text-amber-500 flex-shrink-0"
                        style="font-size: 16px;">{"rocket_launch"}</span>
                    // A real button: the card was click-only (#281). Its
                    // click bubbles to the card's handler.
                    <button type="button" class="font-medium text-stone-800 dark:text-stone-200 truncate text-left
                        cursor-pointer focus-visible:outline-2 focus-visible:outline-amber-500 rounded
                        group-hover:text-amber-700 dark:group-hover:text-amber-400 transition-colors">
                        {entry.title.clone()}
                    </button>
                    {entry.project_status.clone().map(|s| view! {
                        <crate::components::project_status::ProjectStatusBadge status=s />
                    })}
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
            {(brief.is_some() || status_html.is_some() || !open_tasks.is_empty()).then(|| {
                view! {
                    <div class="border-t border-stone-200 dark:border-stone-700/60 px-4 py-3
                        grid grid-cols-1 md:grid-cols-2 gap-4">
                        {brief.map(|brief| view! {
                            <div class="min-w-0" data-testid="card-brief">
                                <div class="text-xs font-semibold uppercase tracking-wider
                                    text-stone-400 dark:text-stone-500 mb-1.5">
                                    "Brief"
                                </div>
                                <crate::components::node_brief::BriefBody brief=brief compact=true />
                            </div>
                        })}
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
