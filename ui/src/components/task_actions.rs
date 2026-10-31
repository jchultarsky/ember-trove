//! Shared task mutations — the one implementation behind every task list.
//!
//! My Day (`task_row.rs` + the `Space` shortcut in `my_day_view.rs`), the
//! node task panel (`task_panel.rs`), and the Inbox (`inbox_view.rs`) used to
//! carry their own copy of the done-toggle. The copies drifted: two compared
//! against the string `"done"` (so a Cancelled task behaved three different
//! ways) and two had no in-flight guard (a double-click raced two PATCHes).
//! New task surfaces call [`toggle_done`] instead of hand-rolling the PATCH.
//!
//! The same goes for deleting ([`delete_with_undo`], five former copies, only
//! two of which guarded against a double-click) and the row ☀ action
//! ([`toggle_my_day`], two former copies).

use std::sync::Arc;

use chrono::NaiveDate;
use common::id::TaskId;
use common::task::{Task, TaskStatus, UpdateTaskRequest, done_toggle, task_edit_request};
use leptos::prelude::*;

use crate::components::task_common::undo_restore_task;
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

/// The list-level context every task action needs. All `Copy`, captured at
/// component setup: Undo actions run after the row may have unmounted,
/// outside any reactive owner.
#[derive(Clone, Copy)]
pub struct TaskCtx {
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
    ctx: TaskCtx,
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
    ctx: TaskCtx,
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

/// Calls `call` for each id, one at a time. Past the first `PACE_AFTER`
/// calls it waits `PACE_MS` between them: the API's write limiter allows a
/// burst of 100, then one call per 100 ms, so a long bulk action (a delete,
/// its Undo, Data health's "Convert all") slows down instead of failing
/// halfway with 429s. Returns the ids it succeeded on and how many failed.
pub(crate) async fn paced<I, F, Fut, T, E>(ids: Vec<I>, call: F) -> (Vec<I>, usize)
where
    I: Copy,
    F: Fn(I) -> Fut,
    Fut: std::future::Future<Output = Result<T, E>>,
{
    const PACE_AFTER: usize = 80;
    const PACE_MS: u32 = 110;
    let mut ok = Vec::with_capacity(ids.len());
    let mut failed = 0usize;
    for (i, id) in ids.into_iter().enumerate() {
        if i >= PACE_AFTER {
            gloo_timers::future::TimeoutFuture::new(PACE_MS).await;
        }
        match call(id).await {
            Ok(_) => ok.push(id),
            Err(_) => failed += 1,
        }
    }
    (ok, failed)
}

fn tasks_noun(n: usize) -> &'static str {
    if n == 1 { "task" } else { "tasks" }
}

/// Soft-delete many tasks and offer one Undo that restores them all: the
/// Tasks panel's "Delete all completed" and Data health's "Delete
/// finished". `adjective` names them in the toast ("completed",
/// "finished"). `refresh` is bumped after the deletes and after an Undo;
/// it's an app-root signal, so the Undo still works once the page is gone
/// (`ToastAction` must not hold page-owned state).
pub fn delete_tasks_with_undo(
    ids: Vec<TaskId>,
    adjective: &'static str,
    refresh: RwSignal<u32>,
    toast_state: Option<ToastState>,
) {
    wasm_bindgen_futures::spawn_local(async move {
        let (deleted, failed) = paced(ids, crate::api::delete_task).await;
        if failed > 0 {
            push_toast(
                ToastLevel::Error,
                format!("Couldn't delete {failed} {}", tasks_noun(failed)),
            );
        }
        // Refetch regardless: some deletes may have landed before a failure.
        refresh.try_update(|n| *n += 1);
        if deleted.is_empty() {
            return;
        }
        let n = deleted.len();
        let undo = Arc::new(move || {
            let deleted = deleted.clone();
            wasm_bindgen_futures::spawn_local(async move {
                let (_, restore_failed) = paced(deleted, crate::api::restore_task).await;
                refresh.try_update(|n| *n += 1);
                if restore_failed > 0
                    && let Some(ts) = toast_state
                {
                    ts.push(
                        ToastLevel::Error,
                        format!("Couldn't restore {restore_failed} of them"),
                    );
                }
            });
        });
        push_undo_toast(format!("Deleted {n} {adjective} {}", tasks_noun(n)), undo);
    });
}

/// Soft-delete a task and offer Undo (which restores it).
///
/// `busy` is the row's in-flight guard, if it has one (a second click while a
/// delete is in flight is ignored). `on_deleted` runs after a successful
/// delete, for caller bookkeeping — Inbox triage advances to the next card.
pub fn delete_with_undo(
    task_id: TaskId,
    busy: Option<RwSignal<bool>>,
    ctx: TaskCtx,
    on_deleted: impl FnOnce() + 'static,
) {
    if busy.is_some_and(|b| b.get_untracked()) {
        return;
    }
    if let Some(b) = busy {
        b.set(true);
    }
    wasm_bindgen_futures::spawn_local(async move {
        let result = crate::api::delete_task(task_id).await;
        if let Some(b) = busy {
            b.set(false);
        }
        match result {
            Ok(_) => {
                ctx.refresh.update(|n| *n += 1);
                push_undo_toast(
                    "Task deleted",
                    undo_restore_task(task_id, ctx.refresh, ctx.toast_state),
                );
                on_deleted();
            }
            Err(e) => push_toast(ToastLevel::Error, format!("Delete failed: {e}")),
        }
    });
}

/// The row ☀ action: commit the task to today's My Day, or take it out.
/// `in_my_day` flips immediately and rolls back if the PATCH fails.
pub fn toggle_my_day(task_id: TaskId, in_my_day: RwSignal<bool>, ctx: TaskCtx) {
    let currently = in_my_day.get_untracked();
    in_my_day.set(!currently);
    let req = UpdateTaskRequest {
        focus_date: Some((!currently).then_some(ctx.today)),
        ..Default::default()
    };
    patch_task(
        task_id,
        req,
        PatchOpts {
            refresh: Some(ctx.refresh),
            ..Default::default()
        },
        move |ok| {
            if !ok {
                in_my_day.set(currently);
            }
        },
    );
}

/// How a task PATCH reports back (see [`patch_task`]).
#[derive(Clone, Copy, Default)]
pub struct PatchOpts {
    /// The row's in-flight guard, if it has one: a second action while a
    /// PATCH is in flight is ignored.
    pub busy: Option<RwSignal<bool>>,
    /// Success toast; `None` = silent (the refetched list is the feedback).
    pub ok_toast: Option<&'static str>,
    /// Error toast prefix ("Save failed", "Couldn't attach to node");
    /// empty = "Couldn't update".
    pub err_prefix: &'static str,
    /// List to refetch on success; `None` when the caller manages its own
    /// view (Inbox triage advances its card stack instead).
    pub refresh: Option<RwSignal<u32>>,
}

/// One task PATCH with the shared guard / toast / refetch behaviour. Every
/// list's ad-hoc copy (eight of them) differed: only two had a busy guard,
/// and the error wording varied. `on_done(ok)` runs after the response, for
/// rollback (`false`) or caller bookkeeping (`true`).
pub fn patch_task(
    task_id: TaskId,
    req: UpdateTaskRequest,
    opts: PatchOpts,
    on_done: impl FnOnce(bool) + 'static,
) {
    if opts.busy.is_some_and(|b| b.get_untracked()) {
        return;
    }
    if let Some(b) = opts.busy {
        b.set(true);
    }
    wasm_bindgen_futures::spawn_local(async move {
        let result = crate::api::update_task(task_id, &req).await;
        if let Some(b) = opts.busy {
            b.set(false);
        }
        match result {
            Ok(_) => {
                if let Some(msg) = opts.ok_toast {
                    push_toast(ToastLevel::Success, msg);
                }
                if let Some(r) = opts.refresh {
                    r.update(|n| *n += 1);
                }
                on_done(true);
            }
            Err(e) => {
                let prefix = if opts.err_prefix.is_empty() {
                    "Couldn't update"
                } else {
                    opts.err_prefix
                };
                push_toast(ToastLevel::Error, format!("{prefix}: {e}"));
                on_done(false);
            }
        }
    });
}

/// An inline task editor's form state (title / priority / due / recurrence),
/// shared by every task list. Each list keeps its own markup (task-row
/// contract); this owns the values: seeding from the task, `reset` on Cancel
/// (it used to reset only the title, so abandoned priority/due/recurrence
/// edits came back on reopen) and building the PATCH
/// (`common::task::task_edit_request`).
#[derive(Clone, Copy)]
pub struct TaskEditDraft {
    pub title: RwSignal<String>,
    pub priority: RwSignal<String>,
    pub due: RwSignal<String>,
    pub recurrence: RwSignal<String>,
    saved: StoredValue<[String; 4]>,
}

impl TaskEditDraft {
    pub fn from_task(task: &Task) -> Self {
        let saved = [
            task.title.clone(),
            task.priority.as_str().to_string(),
            task.due_date
                .map(|d| d.format("%Y-%m-%d").to_string())
                .unwrap_or_default(),
            task.recurrence
                .as_ref()
                .map(|r| r.as_str().to_string())
                .unwrap_or_default(),
        ];
        Self {
            title: RwSignal::new(saved[0].clone()),
            priority: RwSignal::new(saved[1].clone()),
            due: RwSignal::new(saved[2].clone()),
            recurrence: RwSignal::new(saved[3].clone()),
            saved: StoredValue::new(saved),
        }
    }

    /// Discard edits: every field back to the last saved values.
    pub fn reset(&self) {
        let [t, p, d, r] = self.saved.get_value();
        self.title.set(t);
        self.priority.set(p);
        self.due.set(d);
        self.recurrence.set(r);
    }

    /// Escape: discard like Cancel, but if anything was changed offer
    /// "Edit discarded · Undo", which puts the typed values back and calls
    /// `reopen` to show the editor again (#123: a stray Escape used to throw
    /// the edit away silently). The explicit Cancel button stays `reset`.
    pub fn discard_with_undo(self, reopen: impl Fn() + Send + Sync + 'static) {
        let typed = [
            self.title.get_untracked(),
            self.priority.get_untracked(),
            self.due.get_untracked(),
            self.recurrence.get_untracked(),
        ];
        let dirty = typed != self.saved.get_value();
        self.reset();
        if !dirty {
            return;
        }
        push_undo_toast(
            "Edit discarded",
            Arc::new(move || {
                let [t, p, d, r] = typed.clone();
                // `try_`: the row may have been refetched away meanwhile.
                self.title.try_set(t);
                self.priority.try_set(p);
                self.due.try_set(d);
                self.recurrence.try_set(r);
                reopen();
            }),
        );
    }

    /// The PATCH for the current values (`None` = blank title, don't save),
    /// and remember them as saved so a later Cancel keeps them.
    pub fn take_request(&self) -> Option<UpdateTaskRequest> {
        let vals = [
            self.title.get_untracked(),
            self.priority.get_untracked(),
            self.due.get_untracked(),
            self.recurrence.get_untracked(),
        ];
        let req = task_edit_request(&vals[0], &vals[1], &vals[2], &vals[3])?;
        self.saved.set_value(vals);
        Some(req)
    }
}
