//! Shared task-row component for the My Day Kanban (v2.6.0).
//!
//! Renders one task as a row with consistent visual hierarchy:
//! checkbox, project chip, title, priority dot, due-date label,
//! actions.  Drives both zones of the My Day Kanban (today + backlog)
//! via a single `KanbanZone` enum that swaps which "zone-swap" button
//! is shown.
//!
//! `focus_date` is binary in this model — "today" or "not today".  All
//! mutations on this row go through `PATCH /api/tasks/:id` setting
//! `focus_date` to `Some(today)` or `Some(None)` (= clear).
//!
//! Drag-and-drop is wired via HTML5 native events.  `dataTransfer` carries
//! the task id as `text/plain`; the parent zone listens for `drop` events
//! and runs the same code path as the tap-button handler — so the drag
//! gesture is purely a desktop convenience, never the only mechanism.
//!
//! Keyboard triage (Phase 5 / v2.7.0) will plug in here without restructuring.

use chrono::NaiveDate;
use common::task::{Task, TaskPriority, TaskStatus, UpdateTaskRequest};
use leptos::prelude::*;

use crate::components::task_common::status_done;
use crate::components::toast::{push_toast, ToastLevel};

/// Which zone of the Kanban this row currently lives in.  Determines which
/// zone-swap button (× Remove vs ☀ Add) is shown and the colour of the
/// row's left border accent.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum KanbanZone {
    /// `focus_date == today` — row in the upper "today" zone.
    Today,
    /// `focus_date != today` — row in the lower backlog zone.
    Backlog,
}

#[component]
pub fn KanbanTaskRow(
    task: Task,
    /// Pre-resolved parent node title; `None` for standalone (Inbox) tasks.
    /// The row renders an "Inbox" chip when None, or `rocket_launch` + node
    /// name when Some.
    node_title: Option<String>,
    today: NaiveDate,
    zone: KanbanZone,
    refresh: RwSignal<u32>,
) -> impl IntoView {
    let task_id   = task.id;
    let title     = task.title.clone();
    let priority  = task.priority.clone();
    let due       = task.due_date;
    let focus     = task.focus_date;
    let parent    = node_title.unwrap_or_else(|| "Inbox".to_string());
    let node_icon = if task.node_id.is_some() { "rocket_launch" } else { "inbox" };

    // Status flips locally on toggle; PATCH happens in the background.
    let status_sig = RwSignal::new(task.status.clone());
    let busy       = RwSignal::new(false);

    // Carry-over context: a task in the backlog zone whose focus_date is
    // strictly before today was committed to a previous day and never
    // finished.  Surfaces as a small "from May 2" hint so the user knows
    // it's been sitting.  Today-zone rows never show this.
    let carryover_from: Option<NaiveDate> = match zone {
        KanbanZone::Backlog => focus.filter(|&d| d < today),
        KanbanZone::Today   => None,
    };

    // ── Mutations ─────────────────────────────────────────────────────

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
                Err(e) => push_toast(ToastLevel::Error, format!("Couldn't update: {e}")),
            }
        });
    };

    let on_add_today    = move |_| patch_focus(Some(today), "Added to today");
    let on_remove_today = move |_| patch_focus(None,         "Removed from today");

    let on_toggle_done = move |_| {
        if busy.get_untracked() { return; }
        let next = if status_done(&status_sig.get_untracked()) {
            TaskStatus::Open
        } else {
            TaskStatus::Done
        };
        status_sig.set(next.clone());
        busy.set(true);
        let req = UpdateTaskRequest {
            title: None, status: Some(next), priority: None,
            focus_date: None, due_date: None, recurrence: None, node_id: None,
        };
        wasm_bindgen_futures::spawn_local(async move {
            let _ = crate::api::update_task(task_id, &req).await;
            busy.set(false);
            refresh.update(|n| *n += 1);
        });
    };

    let on_delete = move |_| {
        if busy.get_untracked() { return; }
        busy.set(true);
        wasm_bindgen_futures::spawn_local(async move {
            let _ = crate::api::delete_task(task_id).await;
            busy.set(false);
            refresh.update(|n| *n += 1);
        });
    };

    // ── Drag (desktop only — touch never fires HTML5 dragstart) ──────
    let on_dragstart = move |ev: web_sys::DragEvent| {
        if let Some(dt) = ev.data_transfer() {
            // Move semantics: the row visually leaves its source zone on drop.
            dt.set_effect_allowed("move");
            let _ = dt.set_data("text/plain", &task_id.0.to_string());
        }
    };

    // ── Render ────────────────────────────────────────────────────────

    let priority_dot = match priority {
        TaskPriority::High   => Some("color:#ef4444;"),
        TaskPriority::Medium => Some("color:#f59e0b;"),
        TaskPriority::Low    => None,
    };

    let zone_accent = match zone {
        KanbanZone::Today   => "border-l-2 border-amber-400",
        KanbanZone::Backlog => "border-l-2 border-stone-200 dark:border-stone-700",
    };

    view! {
        <div
            draggable="true"
            on:dragstart=on_dragstart
            class=move || format!(
                "group flex items-start gap-2 py-2 px-3 rounded-r-lg \
                 hover:bg-stone-50 dark:hover:bg-stone-800/50 \
                 transition-colors cursor-grab active:cursor-grabbing \
                 {zone_accent} {}",
                if status_done(&status_sig.get()) { "opacity-50" } else { "" }
            )
            data-task-id=task_id.0.to_string()
        >
            // Checkbox — toggles Open ↔ Done
            <button
                type="button"
                class="flex-shrink-0 mt-0.5 w-5 h-5 rounded border-2 \
                       border-stone-300 dark:border-stone-600 flex items-center \
                       justify-center hover:border-amber-500 transition-colors \
                       cursor-pointer"
                style=move || if status_done(&status_sig.get()) {
                    "background:#d97706;border-color:#d97706;"
                } else { "" }
                on:click=on_toggle_done
                title="Toggle done"
            >
                {move || status_done(&status_sig.get()).then(|| view! {
                    <span class="material-symbols-outlined text-white" style="font-size:13px;">"check"</span>
                })}
            </button>

            // Body — project chip on its own line, then title + meta inline
            <div class="flex-1 min-w-0">
                <div class="flex items-center gap-1.5">
                    <span class="material-symbols-outlined text-stone-400 dark:text-stone-500"
                          style="font-size:13px;">{node_icon}</span>
                    <span class="text-xs font-medium text-stone-600 dark:text-stone-300 truncate">
                        {parent}
                    </span>
                    {carryover_from.map(|d| {
                        let label = d.format("%b %-d").to_string();
                        let title_attr = format!("Was focused on {label}");
                        view! {
                            <span class="text-xs text-amber-700 dark:text-amber-400 flex-shrink-0"
                                  title=title_attr>
                                " · carried from " {label}
                            </span>
                        }
                    })}
                </div>
                <div class="flex items-center gap-2 mt-0.5">
                    {priority_dot.map(|s| view! {
                        <span style=format!("{s}font-size:8px;line-height:1;")>"●"</span>
                    })}
                    <span class="text-sm text-stone-800 dark:text-stone-200 truncate"
                          style=move || if status_done(&status_sig.get()) {
                              "text-decoration:line-through;"
                          } else { "" }>
                        {title}
                    </span>
                    {due.map(|d| {
                        let overdue = d < today && !matches!(status_sig.get_untracked(), TaskStatus::Done | TaskStatus::Cancelled);
                        let style = if overdue {
                            "color:#dc2626;font-size:11px;font-weight:600;"
                        } else {
                            "color:#9ca3af;font-size:11px;"
                        };
                        let label = if overdue {
                            format!("⚠ due {}", d.format("%b %-d"))
                        } else {
                            format!("due {}", d.format("%b %-d"))
                        };
                        view! {
                            <span style=style class="flex-shrink-0" title="External deadline">{label}</span>
                        }
                    })}
                </div>
            </div>

            // Actions — always visible (no hover-to-reveal so touch users
            // don't have to discover them)
            <div class="flex items-center gap-0.5 flex-shrink-0">
                {match zone {
                    KanbanZone::Today => view! {
                        <button
                            type="button"
                            class="p-1 rounded text-amber-500 hover:text-amber-700 \
                                   hover:bg-amber-50 dark:hover:bg-amber-950/40 \
                                   transition-colors cursor-pointer disabled:opacity-50"
                            prop:disabled=move || busy.get()
                            on:click=on_remove_today
                            title="Remove from today (back to backlog)"
                        >
                            <span class="material-symbols-outlined" style="font-size:16px;">"close"</span>
                        </button>
                    }.into_any(),
                    KanbanZone::Backlog => view! {
                        <button
                            type="button"
                            class="p-1 rounded text-stone-400 hover:text-amber-600 \
                                   hover:bg-amber-50 dark:hover:bg-amber-950/40 \
                                   transition-colors cursor-pointer disabled:opacity-50"
                            prop:disabled=move || busy.get()
                            on:click=on_add_today
                            title="Add to today"
                        >
                            <span class="material-symbols-outlined" style="font-size:16px;">"wb_sunny"</span>
                        </button>
                    }.into_any(),
                }}
                <button
                    type="button"
                    class="p-1 rounded text-stone-300 dark:text-stone-600 \
                           hover:text-red-500 transition-colors cursor-pointer \
                           disabled:opacity-50"
                    prop:disabled=move || busy.get()
                    on:click=on_delete
                    title="Delete task"
                >
                    <span class="material-symbols-outlined" style="font-size:16px;">"delete"</span>
                </button>
            </div>
        </div>
    }
}
