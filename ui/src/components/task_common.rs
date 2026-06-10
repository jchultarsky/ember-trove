//! Shared task helpers — pure functions used by `task_panel`, `inbox_view`,
//! `my_day_view`, and the reusable `TaskRowInner` component.
//!
//! Centralises status / priority / recurrence parsing, formatting, and
//! comparison so that every task-related view speaks the same language.

use std::sync::Arc;

use common::id::TaskId;
use common::task::{RecurrenceRule, Task, TaskPriority, TaskStatus};
use leptos::prelude::*;

use crate::components::toast::{ToastLevel, ToastState};

// ── Undo (soft delete) ──────────────────────────────────────────────────────

/// Build the "Undo" action for a deleted task: restores it server-side and
/// bumps `refresh` so the list refetches.
///
/// Owner-independent by design (see `ToastAction`): captures only Copy values
/// (id, app-root signals, `ToastState`), so it stays valid after the deleting
/// row has unmounted.
pub fn undo_restore_task(
    task_id: TaskId,
    refresh: RwSignal<u32>,
    toast_state: Option<ToastState>,
) -> Arc<dyn Fn() + Send + Sync> {
    Arc::new(move || {
        wasm_bindgen_futures::spawn_local(async move {
            match crate::api::restore_task(task_id).await {
                Ok(_) => refresh.update(|n| *n += 1),
                Err(e) => {
                    if let Some(ts) = toast_state {
                        ts.push(ToastLevel::Error, format!("Undo failed: {e}"));
                    }
                }
            }
        });
    })
}

// ── Status ──────────────────────────────────────────────────────────────────

/// Returns `true` for terminal statuses (Done, Cancelled).
pub fn status_done(s: &TaskStatus) -> bool {
    matches!(s, TaskStatus::Done | TaskStatus::Cancelled)
}

pub fn parse_status(s: &str) -> TaskStatus {
    match s {
        "in_progress" => TaskStatus::InProgress,
        "done" => TaskStatus::Done,
        "cancelled" => TaskStatus::Cancelled,
        _ => TaskStatus::Open,
    }
}

pub fn status_value(s: &TaskStatus) -> &'static str {
    match s {
        TaskStatus::Open => "open",
        TaskStatus::InProgress => "in_progress",
        TaskStatus::Done => "done",
        TaskStatus::Cancelled => "cancelled",
    }
}

pub fn status_label(s: &TaskStatus) -> &'static str {
    match s {
        TaskStatus::Open => "Open",
        TaskStatus::InProgress => "In Progress",
        TaskStatus::Done => "Done",
        TaskStatus::Cancelled => "Cancelled",
    }
}

// ── Priority ────────────────────────────────────────────────────────────────

pub fn parse_priority(s: &str) -> TaskPriority {
    match s {
        "high" => TaskPriority::High,
        "low" => TaskPriority::Low,
        _ => TaskPriority::Medium,
    }
}

pub fn priority_value(p: &TaskPriority) -> &'static str {
    match p {
        TaskPriority::High => "high",
        TaskPriority::Medium => "medium",
        TaskPriority::Low => "low",
    }
}

pub fn priority_label(p: &TaskPriority) -> &'static str {
    match p {
        TaskPriority::High => "High",
        TaskPriority::Medium => "Medium",
        TaskPriority::Low => "Low",
    }
}

/// Raw hex colour string for a priority level (e.g. `"#dc2626"`).
pub fn priority_color_hex(p: &TaskPriority) -> &'static str {
    match p {
        TaskPriority::High => "#dc2626",
        TaskPriority::Medium => "#d97706",
        TaskPriority::Low => "#6b7280",
    }
}

/// Inline CSS for a small filled circle used as a priority indicator.
pub fn priority_dot_color(p: &TaskPriority) -> &'static str {
    match p {
        TaskPriority::High => "background:#dc2626;",
        TaskPriority::Medium => "background:#d97706;",
        TaskPriority::Low => "background:#6b7280;",
    }
}

/// Material Symbols icon name for the priority level.
pub fn priority_icon(p: &TaskPriority) -> &'static str {
    match p {
        TaskPriority::High => "keyboard_double_arrow_up",
        TaskPriority::Medium => "drag_handle",
        TaskPriority::Low => "keyboard_double_arrow_down",
    }
}

/// Inline CSS colour for the priority icon / label text.
pub fn priority_color(p: &TaskPriority) -> &'static str {
    match p {
        TaskPriority::High => "color: #dc2626;",
        TaskPriority::Medium => "color: #d97706;",
        TaskPriority::Low => "color: #6b7280;",
    }
}

// ── Recurrence ──────────────────────────────────────────────────────────────

pub fn parse_recurrence_opt(s: &str) -> Option<RecurrenceRule> {
    match s {
        "daily" => Some(RecurrenceRule::Daily),
        "weekly" => Some(RecurrenceRule::Weekly),
        "biweekly" => Some(RecurrenceRule::Biweekly),
        "monthly" => Some(RecurrenceRule::Monthly),
        "yearly" => Some(RecurrenceRule::Yearly),
        _ => None,
    }
}

pub fn recurrence_value(r: &RecurrenceRule) -> &'static str {
    match r {
        RecurrenceRule::Daily => "daily",
        RecurrenceRule::Weekly => "weekly",
        RecurrenceRule::Biweekly => "biweekly",
        RecurrenceRule::Monthly => "monthly",
        RecurrenceRule::Yearly => "yearly",
    }
}

pub fn recurrence_label(r: &RecurrenceRule) -> &'static str {
    match r {
        RecurrenceRule::Daily => "Daily",
        RecurrenceRule::Weekly => "Weekly",
        RecurrenceRule::Biweekly => "Every 2 weeks",
        RecurrenceRule::Monthly => "Monthly",
        RecurrenceRule::Yearly => "Yearly",
    }
}

// ── Node type ───────────────────────────────────────────────────────────────

/// Material Symbols icon name for a node type string (used in the inbox
/// node-picker and elsewhere).
pub fn node_type_icon(node_type: &str) -> &'static str {
    match node_type {
        "article" => "description",
        "project" => "rocket_launch",
        "area" => "category",
        "resource" => "bookmarks",
        "reference" => "menu_book",
        _ => "article",
    }
}

// ── My Day predicate ────────────────────────────────────────────────────────

/// Returns `true` when the task is currently visible in today's My Day list.
///
/// Mirrors the server's carry-forward rule in `list_my_day`: a task shows up
/// in My Day when its `focus_date` is on or before `today`, and is either
/// focused on `today` exactly or still in a non-terminal state. This keeps
/// the node-detail lightbulb in sync with what actually appears in My Day.
pub fn is_in_my_day(task: &Task, today: chrono::NaiveDate) -> bool {
    match task.focus_date {
        Some(d) if d == today => true,
        Some(d) if d < today => !status_done(&task.status),
        _ => false,
    }
}

// ── Sorting ─────────────────────────────────────────────────────────────────

/// Sort tasks by `sort_order` first, then `created_at` as a tiebreak.
/// Used by TaskPanel (node-scoped task list).
pub fn sort_tasks_by_order(tasks: &mut [Task]) {
    tasks.sort_by(|a, b| {
        a.sort_order
            .cmp(&b.sort_order)
            .then_with(|| a.created_at.cmp(&b.created_at))
    });
}
