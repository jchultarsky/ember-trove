//! Shared task mutations — the one implementation behind every task list.
//!
//! My Day (`task_row.rs` + the `Space` shortcut in `my_day_view.rs`), the
//! node task panel (`task_panel.rs`), and the Inbox (`inbox_view.rs`) used to
//! carry their own copy of the done-toggle. The copies drifted: two compared
//! against the string `"done"` (so a Cancelled task behaved three different
//! ways) and two had no in-flight guard (a double-click raced two PATCHes).
//! New task surfaces call [`toggle_done`] instead of hand-rolling the PATCH.

use common::id::TaskId;
use common::task::{TaskStatus, UpdateTaskRequest};
use leptos::prelude::*;

use crate::components::toast::{ToastLevel, push_toast};

/// A rendered row's optimistic state. List-level callers that have no row
/// signals (keyboard shortcuts) pass `None` and rely on the refetch.
#[derive(Clone, Copy)]
pub struct RowSignals {
    /// Flipped immediately; rolled back if the PATCH fails.
    pub status: RwSignal<TaskStatus>,
    /// In-flight guard shared with the row's other mutations.
    pub busy: RwSignal<bool>,
}

/// Toggle a task between done and open ([`TaskStatus::toggled`]).
///
/// `current` is the status the caller sees; `spark` is the click that
/// triggered it (the checkbox), used for the completion spark — keyboard
/// paths pass `None`.
pub fn toggle_done(
    task_id: TaskId,
    current: TaskStatus,
    row: Option<RowSignals>,
    refresh: RwSignal<u32>,
    spark: Option<&web_sys::MouseEvent>,
) {
    if row.is_some_and(|r| r.busy.get_untracked()) {
        return;
    }
    let next = current.toggled();
    if next == TaskStatus::Done
        && let Some(ev) = spark
    {
        crate::spark::strike_spark(ev);
    }
    if let Some(r) = row {
        r.status.set(next.clone());
        r.busy.set(true);
    }
    let req = UpdateTaskRequest {
        status: Some(next),
        ..Default::default()
    };
    wasm_bindgen_futures::spawn_local(async move {
        let result = crate::api::update_task(task_id, &req).await;
        if let Some(r) = row {
            r.busy.set(false);
        }
        match result {
            Ok(_) => refresh.update(|n| *n += 1),
            Err(e) => {
                // Roll back the optimistic flip — the server still has the
                // old status, so the checkbox must not lie.
                if let Some(r) = row {
                    r.status.set(current);
                }
                push_toast(ToastLevel::Error, format!("Couldn't update: {e}"));
            }
        }
    });
}
