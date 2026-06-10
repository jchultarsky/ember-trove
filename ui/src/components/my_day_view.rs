//! My Day — two-zone vertical Kanban (v2.6.0+) with keyboard triage (v2.7.0).
//!
//! Top zone:  tasks with `focus_date` set and on-or-before today — "what I'm
//!            doing today".  Sticky: once committed to My Day a task stays
//!            here until completed or removed; it never auto-drops at midnight.
//! Bottom:    every open task with no `focus_date` (or a future one) — the
//!            "everything else" backlog.
//!
//! ## Mouse + touch
//!
//! Tap the ☀ "Add to today" button (in backlog) or × "Remove from today"
//! button (in today) on any row.  Drag the row body across the divider
//! (desktop only — touch never fires HTML5 dragstart).  Both paths run
//! the same `PATCH /api/tasks/:id`.
//!
//! ## Keyboard (v2.7.0)
//!
//! Global keydown handler attached while this view is mounted.  Skipped
//! when an `<input>`, `<textarea>`, `<select>`, `[contenteditable]`, or
//! a `<button>` has focus, so typing in the inline edit form, or
//! tabbing to action buttons, never triggers a shortcut.
//!
//!   `j` / `↓`   focus next row (across both zones, in display order)
//!   `k` / `↑`   focus previous row
//!   `Enter`     open the focused task in its parent node (or Inbox)
//!   `Space`     toggle done on the focused task
//!   `t`         toggle the focused task between Today and Backlog
//!   `e`         open inline edit on the focused task
//!   `d`         delete the focused task
//!
//! `s` (snooze) is intentionally absent — `focus_date` is binary in this
//! model (today | None), so "snooze" is the same gesture as "remove from
//! today" (the `t` toggle from the Today zone).
//!
//! ## Carry-over
//!
//! Tasks committed to a previous day that aren't done stay in the Today
//! zone (sticky My Day) with a small "carried from May 2" badge (rendered
//! by `KanbanTaskRow`) so the user sees how long they've lingered.  They
//! leave My Day only when completed or tapped/dragged back to the backlog.

use chrono::NaiveDate;
use common::id::TaskId;
use common::task::{MyDayTask, TaskStatus, UpdateTaskRequest};
use leptos::wasm_bindgen::JsCast;
use leptos::{ev, prelude::*};
use leptos_router::hooks::use_navigate;

use crate::app::TaskRefresh;
use crate::components::task_common::{status_done, undo_restore_task};
use crate::components::task_row::{
    EditingTaskId, FocusedTaskId, KanbanTaskRow, KanbanZone, TaskEditorHeights,
};
use crate::components::toast::{ToastLevel, ToastState, push_toast, push_undo_toast};

#[component]
pub fn MyDayView() -> impl IntoView {
    let task_refresh = expect_context::<TaskRefresh>().0;
    // Captured at setup for the undo closure, which outlives the deleted row.
    let toast_state = use_context::<ToastState>();

    let today = crate::components::format_helpers::local_today();
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

    // ── Keyboard cursor + edit cursor (provided to all KanbanTaskRow's) ─
    let focused_id: RwSignal<Option<TaskId>> = RwSignal::new(None);
    let editing_id: RwSignal<Option<TaskId>> = RwSignal::new(None);
    provide_context(FocusedTaskId(focused_id));
    provide_context(EditingTaskId(editing_id));

    // Per-task saved inline-edit heights — fetched once, provided to all rows
    // so each opens its editor at the previously-resized size.
    let editor_heights =
        RwSignal::<std::collections::HashMap<uuid::Uuid, i32>>::new(Default::default());
    provide_context(TaskEditorHeights(editor_heights));
    wasm_bindgen_futures::spawn_local(async move {
        if let Ok(prefs) = crate::api::fetch_editor_prefs().await {
            editor_heights.set(
                prefs
                    .into_iter()
                    .filter(|p| p.entity_kind == "task")
                    .map(|p| (p.entity_id, p.height))
                    .collect(),
            );
        }
    });

    // ── Flat task list in display order — fed to the keyboard handler.
    // Today zone first, backlog second.  Updated whenever either
    // resource changes.  Stored in a separate signal (vs derived from
    // resources every keypress) so the keydown handler can read it
    // untracked without touching the reactive graph.
    let flat_tasks: RwSignal<Vec<MyDayTask>> = RwSignal::new(Vec::new());
    Effect::new(move |_| {
        let today_raw = today_tasks.get().and_then(|r| r.ok()).unwrap_or_default();
        let all_raw = all_open.get().and_then(|r| r.ok()).unwrap_or_default();
        let today_zone: Vec<MyDayTask> = today_raw
            .into_iter()
            .filter(|t| t.task.focus_date.is_some_and(|d| d <= today))
            .collect();
        let backlog_zone: Vec<MyDayTask> = all_raw
            .into_iter()
            .filter(|t| t.task.focus_date.is_none_or(|d| d > today))
            .collect();
        let mut flat = today_zone;
        flat.extend(backlog_zone);
        // Drop the focus cursor if it now points at a task that
        // disappeared (deleted, completed, etc.) — k/j start from the
        // top next time.
        if let Some(fid) = focused_id.get_untracked()
            && !flat.iter().any(|t| t.task.id == fid)
        {
            focused_id.set(None);
        }
        flat_tasks.set(flat);
    });

    let navigate = StoredValue::new(use_navigate());

    // ── Window keydown handler ─────────────────────────────────────────
    // Attached on mount (window_event_listener returns a Handle that
    // Leptos drops automatically when the view unmounts, removing the
    // listener — no manual cleanup needed).
    window_event_listener(ev::keydown, move |ev| {
        // Modifier keys are reserved for app-level shortcuts (Cmd-K
        // arrives in v2.8.0) — never consume them here.
        if ev.ctrl_key() || ev.meta_key() || ev.alt_key() {
            return;
        }

        // Skip when typing — input, textarea, select, button, or
        // anything contenteditable.  Buttons are excluded so Enter on
        // a focused tap-button doesn't trigger the row Enter shortcut.
        let editable = web_sys::window()
            .and_then(|w| w.document())
            .and_then(|d| d.active_element())
            .map(|el| {
                let tag = el.tag_name().to_uppercase();
                if matches!(tag.as_str(), "INPUT" | "TEXTAREA" | "SELECT" | "BUTTON") {
                    return true;
                }
                el.get_attribute("contenteditable")
                    .map(|v| v != "false")
                    .unwrap_or(false)
            })
            .unwrap_or(false);
        if editable {
            return;
        }

        let flat = flat_tasks.get_untracked();
        if flat.is_empty() {
            return;
        }
        let cur_idx = focused_id
            .get_untracked()
            .and_then(|id| flat.iter().position(|t| t.task.id == id));

        match ev.key().as_str() {
            "j" | "ArrowDown" => {
                ev.prevent_default();
                let next = cur_idx.map_or(0, |i| (i + 1).min(flat.len() - 1));
                let id = flat[next].task.id;
                focused_id.set(Some(id));
                scroll_focused_into_view(id);
            }
            "k" | "ArrowUp" => {
                ev.prevent_default();
                let next = cur_idx.map_or(0, |i| i.saturating_sub(1));
                let id = flat[next].task.id;
                focused_id.set(Some(id));
                scroll_focused_into_view(id);
            }
            "Enter" => {
                let Some(idx) = cur_idx else {
                    return;
                };
                ev.prevent_default();
                let mdt = &flat[idx];
                let target = match mdt.task.node_id {
                    Some(nid) => format!("/nodes/{nid}?task={}", mdt.task.id),
                    None => format!("/tasks/inbox?task={}", mdt.task.id),
                };
                navigate.get_value()(&target, Default::default());
            }
            " " => {
                let Some(idx) = cur_idx else {
                    return;
                };
                ev.prevent_default();
                let mdt = &flat[idx];
                let next_status = if status_done(&mdt.task.status) {
                    TaskStatus::Open
                } else {
                    TaskStatus::Done
                };
                patch_task(
                    mdt.task.id,
                    UpdateTaskRequest {
                        title: None,
                        status: Some(next_status),
                        priority: None,
                        focus_date: None,
                        due_date: None,
                        recurrence: None,
                        node_id: None,
                    },
                    "Toggled",
                    task_refresh,
                );
            }
            "t" => {
                let Some(idx) = cur_idx else {
                    return;
                };
                ev.prevent_default();
                let mdt = &flat[idx];
                let in_today = mdt.task.focus_date == Some(today);
                let new_focus = if in_today { None } else { Some(today) };
                let msg = if in_today {
                    "Removed from today"
                } else {
                    "Added to today"
                };
                patch_task(
                    mdt.task.id,
                    UpdateTaskRequest {
                        title: None,
                        status: None,
                        priority: None,
                        focus_date: Some(new_focus),
                        due_date: None,
                        recurrence: None,
                        node_id: None,
                    },
                    msg,
                    task_refresh,
                );
            }
            "e" => {
                let Some(idx) = cur_idx else {
                    return;
                };
                ev.prevent_default();
                editing_id.set(Some(flat[idx].task.id));
            }
            "d" => {
                let Some(idx) = cur_idx else {
                    return;
                };
                ev.prevent_default();
                let id = flat[idx].task.id;
                wasm_bindgen_futures::spawn_local(async move {
                    match crate::api::delete_task(id).await {
                        Ok(_) => {
                            task_refresh.update(|n| *n += 1);
                            push_undo_toast(
                                "Task deleted",
                                undo_restore_task(id, task_refresh, toast_state),
                            );
                        }
                        Err(e) => push_toast(ToastLevel::Error, format!("Delete failed: {e}")),
                    }
                });
            }
            _ => {}
        }
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
                            " · drag, tap ☀/×, or use j/k + Enter/Space/t/e/d (press ? for the full list)"
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
                        let scoped: Vec<MyDayTask> = raw.into_iter()
                            .filter(|t| t.task.focus_date.is_some_and(|d| d <= today))
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
                            .filter(|t| t.task.focus_date.is_none_or(|d| d > today))
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
    today: NaiveDate,
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
        let Some(dt) = ev.data_transfer() else {
            return;
        };
        let Ok(raw) = dt.get_data("text/plain") else {
            return;
        };
        let Ok(uuid) = raw.parse::<uuid::Uuid>() else {
            return;
        };
        let task_id = TaskId(uuid);
        let new_focus = match zone {
            KanbanZone::Today => Some(today),
            KanbanZone::Backlog => None,
        };
        let success_msg = match zone {
            KanbanZone::Today => "Added to today",
            KanbanZone::Backlog => "Removed from today",
        };
        let req = UpdateTaskRequest {
            title: None,
            status: None,
            priority: None,
            focus_date: Some(new_focus),
            due_date: None,
            recurrence: None,
            node_id: None,
        };
        wasm_bindgen_futures::spawn_local(async move {
            match crate::api::update_task(task_id, &req).await {
                Ok(_) => {
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

// ── helpers ─────────────────────────────────────────────────────────────────

/// Fire one PATCH and refresh on success.  Used by the keyboard
/// shortcut handler — the row's own buttons go through their local
/// handlers in `task_row.rs`.
fn patch_task(
    task_id: TaskId,
    req: UpdateTaskRequest,
    success_msg: &'static str,
    refresh: RwSignal<u32>,
) {
    wasm_bindgen_futures::spawn_local(async move {
        match crate::api::update_task(task_id, &req).await {
            Ok(_) => {
                push_toast(ToastLevel::Success, success_msg);
                refresh.update(|n| *n += 1);
            }
            Err(e) => push_toast(ToastLevel::Error, format!("Update failed: {e}")),
        }
    });
}

/// Scroll the row matching `task_id` into view (no flash — the focus
/// ring is the visual anchor).  Called after j/k navigation.
fn scroll_focused_into_view(task_id: TaskId) {
    let Some(win) = web_sys::window() else {
        return;
    };
    let Some(doc) = win.document() else {
        return;
    };
    let selector = format!("[data-task-id=\"{}\"]", task_id.0);
    let Ok(Some(el)) = doc.query_selector(&selector) else {
        return;
    };
    let Ok(html_el) = el.dyn_into::<web_sys::HtmlElement>() else {
        return;
    };
    let opts = web_sys::ScrollIntoViewOptions::new();
    opts.set_behavior(web_sys::ScrollBehavior::Smooth);
    opts.set_block(web_sys::ScrollLogicalPosition::Nearest);
    html_el.scroll_into_view_with_scroll_into_view_options(&opts);
}
