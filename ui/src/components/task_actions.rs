//! Shared task mutations — the one implementation behind every task list.
//!
//! My Day (`task_row.rs` + the `Space` shortcut in `my_day_view.rs`), the
//! node task panel (`task_panel.rs`), and the Inbox (`inbox_view.rs`) used to
//! carry their own copy of the done-toggle. The copies drifted: two compared
//! against the string `"done"` (so a Cancelled task behaved three different
//! ways) and two had no in-flight guard (a double-click raced two PATCHes).
//! New task surfaces call [`toggle_done`] instead of hand-rolling the PATCH.

use std::sync::Arc;

use chrono::NaiveDate;
use common::id::TaskId;
use common::task::{TaskStatus, UpdateTaskRequest, done_toggle};
use leptos::prelude::*;

use crate::components::toast::{ToastLevel, ToastState, push_toast, push_undo_toast};

/// A rendered row's optimistic state. List-level callers that have no row
/// signals (keyboard shortcuts) pass `None` and rely on the refetch.
#[derive(Clone, Copy)]
pub struct RowSignals {
    /// Flipped immediately; rolled back if the PATCH fails.
    pub status: RwSignal<TaskStatus>,
    /// In-flight guard shared with the row's other mutations.
    pub busy: RwSignal<bool>,
}

/// The list-level context a toggle needs. All `Copy`, captured at component
/// setup: the Undo action runs after the row may have unmounted, outside any
/// reactive owner.
#[derive(Clone, Copy)]
pub struct ToggleCtx {
    /// The viewer's local date (`format_helpers::local_today`).
    pub today: NaiveDate,
    /// Bumped after every successful write so the list refetches.
    pub refresh: RwSignal<u32>,
    pub toast_state: Option<ToastState>,
}

/// Toggle a task between done and open ([`common::task::done_toggle`]).
///
/// `status` / `focus_date` are what the caller sees; `spark` is the click
/// that triggered it (the checkbox), used for the completion spark —
/// keyboard paths pass `None`. A completion offers an Undo toast that
/// reverts status (and a carried-over `focus_date`); the API retracts the
/// next occurrence of a recurring task on that revert.
pub fn toggle_done(
    task_id: TaskId,
    status: TaskStatus,
    focus_date: Option<NaiveDate>,
    row: Option<RowSignals>,
    ctx: ToggleCtx,
    spark: Option<&web_sys::MouseEvent>,
) {
    if row.is_some_and(|r| r.busy.get_untracked()) {
        return;
    }
    let toggle = done_toggle(&status, focus_date, ctx.today);
    let completes = toggle.completes();
    if completes && let Some(ev) = spark {
        crate::spark::strike_spark(ev);
    }
    if let Some(r) = row {
        r.status.set(toggle.next.clone());
        r.busy.set(true);
    }
    let revert = toggle.revert;
    wasm_bindgen_futures::spawn_local(async move {
        let result = crate::api::update_task(task_id, &toggle.request).await;
        if let Some(r) = row {
            r.busy.set(false);
        }
        match result {
            Ok(_) => {
                ctx.refresh.update(|n| *n += 1);
                if completes {
                    push_undo_toast("Task completed", undo_toggle(task_id, revert, ctx));
                }
            }
            Err(e) => {
                // Roll back the optimistic flip — the server still has the
                // old status, so the checkbox must not lie.
                if let Some(r) = row {
                    r.status.set(status);
                }
                push_toast(ToastLevel::Error, format!("Couldn't update: {e}"));
            }
        }
    });
}

/// The Undo action for a completion: PATCH the revert and refetch.
///
/// Owner-independent (see `ToastAction`): captures only the id, the revert
/// request, and `Copy` context, so it stays valid after the row unmounts.
fn undo_toggle(
    task_id: TaskId,
    revert: UpdateTaskRequest,
    ctx: ToggleCtx,
) -> Arc<dyn Fn() + Send + Sync> {
    Arc::new(move || {
        let req = revert.clone();
        wasm_bindgen_futures::spawn_local(async move {
            match crate::api::update_task(task_id, &req).await {
                Ok(_) => ctx.refresh.update(|n| *n += 1),
                Err(e) => {
                    if let Some(ts) = ctx.toast_state {
                        ts.push(ToastLevel::Error, format!("Undo failed: {e}"));
                    }
                }
            }
        });
    })
}
