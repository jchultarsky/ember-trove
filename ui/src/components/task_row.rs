//! Shared task-row component for the My Day Kanban (v2.6.0+).
//!
//! Renders one task as a row with consistent visual hierarchy:
//! checkbox · project chip · title · priority dot · due-date label · actions.
//! Drives both zones of the My Day Kanban (today + backlog) via a single
//! `KanbanZone` enum that swaps which "zone-swap" button is shown.
//!
//! `focus_date` is binary at the *write* boundary — committing a task to My
//! Day stamps it with today's date; removing clears it.  All focus mutations
//! on this row go through `PATCH /api/tasks/:id` setting `focus_date` to
//! `Some(today)` or `Some(None)` (= clear).  At the *read* boundary the Today
//! zone is sticky: any task whose `focus_date` is on-or-before today stays in
//! My Day until it is completed or removed — it never auto-drops at midnight.
//!
//! ## Click + drag (v2.6.2)
//!
//! Two ways to interact with the row body:
//! * **Click** the row body (anywhere outside the action buttons or the
//!   inline edit form) → navigate to the parent node, scrolled and
//!   briefly highlighted on the task.  Standalone tasks navigate to the
//!   Inbox.  Action buttons call `stopPropagation()` so they never
//!   trigger the row-click.
//! * **Drag** the row body → HTML5 native drag.  `dataTransfer` carries
//!   the task id; the destination zone fires the same PATCH the tap
//!   button would.  Mousedown-then-move triggers drag; mousedown-then-up
//!   triggers click — they don't conflict in practice.
//!
//! ## Inline edit (v2.6.2)
//!
//! The pencil button toggles an inline edit form (title, priority,
//! due_date, recurrence).  No `focus_date` field — that's binary, owned
//! by the zone-swap button.  Save persists via PATCH; Esc cancels.
//!
//! Keyboard triage (Phase 5 / v2.7.0) will plug in here without
//! restructuring.

use chrono::NaiveDate;
use common::id::TaskId;
use common::task::{Task, TaskPriority, TaskStatus, UpdateTaskRequest};
use leptos::prelude::*;
use leptos_router::hooks::use_navigate;

use crate::components::icon_button::{IconButton, IconButtonVariant};
use crate::components::task_common::{
    parse_priority, parse_recurrence_opt, priority_value, recurrence_value, status_done,
    undo_restore_task,
};
use crate::components::toast::{ToastLevel, ToastState, push_toast, push_undo_toast};

/// Which zone of the Kanban this row currently lives in.  Determines which
/// zone-swap button (× Remove vs ☀ Add) is shown and the colour of the
/// row's left border accent.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum KanbanZone {
    /// `focus_date` is set and on-or-before today — row in the upper "today"
    /// zone.  Sticky: a task committed to My Day stays here across days until
    /// it is completed or removed (focus_date cleared).
    Today,
    /// No `focus_date`, or a `focus_date` in the future — row in the lower
    /// backlog zone.
    Backlog,
}

/// Newtype for the keyboard cursor: which task row is currently focused
/// for keyboard shortcuts.  Provided as a context by `MyDayView`; rows
/// read it to render a focus ring.  Also written by the row's own click
/// handler so mouse and keyboard share one source of truth.
#[derive(Clone, Copy)]
pub struct FocusedTaskId(pub RwSignal<Option<TaskId>>);

/// Newtype for the inline-edit cursor: which task row is currently in
/// edit mode.  Provided as a context by `MyDayView`; rows watch it to
/// open / close their inline edit form.  Letting an external signal
/// drive editing means the keyboard `e` shortcut and the pencil button
/// share one mechanism.
#[derive(Clone, Copy)]
pub struct EditingTaskId(pub RwSignal<Option<TaskId>>);

/// Saved per-task inline-edit heights (px), keyed by task id. Provided as a
/// context by the surrounding view so each row can open its editor at the
/// previously-saved size.
#[derive(Clone, Copy)]
pub struct TaskEditorHeights(pub RwSignal<std::collections::HashMap<uuid::Uuid, i32>>);

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
    let task_id = task.id;
    let node_id = task.node_id;
    let priority = task.priority.clone();
    let due = task.due_date;
    let focus = task.focus_date;
    let parent = node_title.unwrap_or_else(|| "Inbox".to_string());
    let node_icon = if task.node_id.is_some() {
        "rocket_launch"
    } else {
        "inbox"
    };

    // Display state — title shows the latest known value (may be edited).
    let title_sig = RwSignal::new(task.title.clone());

    // Status flips locally on toggle; PATCH happens in the background.
    let status_sig = RwSignal::new(task.status.clone());

    // Keyboard-focus + edit cursors come from MyDayView via context.
    // Optional because KanbanTaskRow could in principle be used outside
    // a Kanban (no consumer today, but the type is reusable).  When the
    // contexts aren't provided we fall back to local-only signals so
    // mouse-driven UX still works.
    let focused_ctx: Option<FocusedTaskId> = use_context();
    let editing_ctx: Option<EditingTaskId> = use_context();
    let editing_local: RwSignal<Option<TaskId>> = RwSignal::new(None);
    let editing_id_sig: RwSignal<Option<TaskId>> =
        editing_ctx.map(|e| e.0).unwrap_or(editing_local);
    // Derived: is *this* row currently focused / editing?
    let is_focused = move || {
        focused_ctx
            .map(|f| f.0.get() == Some(task_id))
            .unwrap_or(false)
    };
    let is_editing = move || editing_id_sig.get() == Some(task_id);

    // Inline edit form state.  Mirrors the original MyDayTaskRow layout
    // minus the focus_date field (focus is binary, owned by zone-swap).
    let edit_title = RwSignal::new(task.title.clone());
    let edit_priority = RwSignal::new(priority_value(&priority).to_string());
    let edit_due = RwSignal::new(
        due.map(|d| d.format("%Y-%m-%d").to_string())
            .unwrap_or_default(),
    );
    let edit_recurrence = RwSignal::new(
        task.recurrence
            .as_ref()
            .map(|r| recurrence_value(r).to_string())
            .unwrap_or_default(),
    );

    let busy = RwSignal::new(false);
    let navigate = StoredValue::new(use_navigate());
    // Captured at setup for the undo closure, which outlives this row.
    let toast_state = use_context::<ToastState>();

    // Carry-over context: a row whose focus_date is strictly before today was
    // committed to My Day on a previous day and never finished.  Such tasks
    // now stay in the Today zone (sticky My Day) rather than dropping to the
    // backlog, so surface the "carried from May 2" hint in either zone.
    let carryover_from: Option<NaiveDate> = focus.filter(|&d| d < today);

    // ── Mutations ─────────────────────────────────────────────────────

    let patch_focus = move |new_focus: Option<NaiveDate>, success_msg: &'static str| {
        if busy.get_untracked() {
            return;
        }
        busy.set(true);
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

    let on_toggle_done = move |ev: web_sys::MouseEvent| {
        ev.stop_propagation();
        if busy.get_untracked() {
            return;
        }
        let prev = status_sig.get_untracked();
        let next = if status_done(&prev) {
            TaskStatus::Open
        } else {
            TaskStatus::Done
        };
        status_sig.set(next.clone());
        busy.set(true);
        let req = UpdateTaskRequest {
            title: None,
            status: Some(next),
            priority: None,
            focus_date: None,
            due_date: None,
            recurrence: None,
            node_id: None,
        };
        wasm_bindgen_futures::spawn_local(async move {
            let result = crate::api::update_task(task_id, &req).await;
            busy.set(false);
            match result {
                Ok(_) => refresh.update(|n| *n += 1),
                Err(e) => {
                    // Roll back the optimistic flip — the server still has
                    // the old status, so the checkbox must not lie.
                    status_sig.set(prev);
                    push_toast(ToastLevel::Error, format!("Couldn't update: {e}"));
                }
            }
        });
    };

    let on_delete = move |ev: web_sys::MouseEvent| {
        ev.stop_propagation();
        if busy.get_untracked() {
            return;
        }
        busy.set(true);
        wasm_bindgen_futures::spawn_local(async move {
            let result = crate::api::delete_task(task_id).await;
            busy.set(false);
            match result {
                Ok(_) => {
                    refresh.update(|n| *n += 1);
                    push_undo_toast(
                        "Task deleted",
                        undo_restore_task(task_id, refresh, toast_state),
                    );
                }
                Err(e) => push_toast(ToastLevel::Error, format!("Delete failed: {e}")),
            }
        });
    };

    let on_pencil = move |ev: web_sys::MouseEvent| {
        ev.stop_propagation();
        // Reset edit fields to the latest known values, then open.
        edit_title.set(title_sig.get_untracked());
        editing_id_sig.set(Some(task_id));
    };

    let do_save = move || {
        let new_title = edit_title.get_untracked().trim().to_string();
        if new_title.is_empty() {
            return;
        }
        let new_priority = parse_priority(&edit_priority.get_untracked());
        let new_recurrence = parse_recurrence_opt(&edit_recurrence.get_untracked());
        let new_due: Option<Option<NaiveDate>> =
            Some(edit_due.get_untracked().trim().parse::<NaiveDate>().ok());
        let req = UpdateTaskRequest {
            title: Some(new_title.clone()),
            status: None,
            priority: Some(new_priority),
            focus_date: None,
            due_date: new_due,
            recurrence: Some(new_recurrence),
            node_id: None,
        };
        title_sig.set(new_title);
        editing_id_sig.set(None);
        wasm_bindgen_futures::spawn_local(async move {
            match crate::api::update_task(task_id, &req).await {
                Ok(_) => {
                    push_toast(ToastLevel::Success, "Saved");
                    refresh.update(|n| *n += 1);
                }
                Err(e) => push_toast(ToastLevel::Error, format!("Save failed: {e}")),
            }
        });
    };
    // ── Row click → focus + navigate to parent (or Inbox) ────────────
    let on_row_click = move |_ev: web_sys::MouseEvent| {
        // Don't navigate while editing — clicks inside the edit form
        // would otherwise jump the user away mid-typing.
        if editing_id_sig.get_untracked() == Some(task_id) {
            return;
        }
        // Mouse and keyboard share one focus cursor — clicking a row
        // moves the keyboard focus to it as well.
        if let Some(f) = focused_ctx {
            f.0.set(Some(task_id));
        }
        let target = match node_id {
            Some(nid) => format!("/nodes/{nid}?task={task_id}"),
            None => format!("/tasks/inbox?task={task_id}"),
        };
        navigate.get_value()(&target, Default::default());
    };

    // ── Drag (desktop only — touch never fires HTML5 dragstart) ──────
    let on_dragstart = move |ev: web_sys::DragEvent| {
        if let Some(dt) = ev.data_transfer() {
            dt.set_effect_allowed("move");
            let _ = dt.set_data("text/plain", &task_id.0.to_string());
        }
    };

    // ── Render ────────────────────────────────────────────────────────

    // Style + accessible name: the dot is colour-coded for sighted users,
    // with title/aria-label carrying the same information non-visually.
    let priority_dot = match priority {
        TaskPriority::High => Some(("color:#ef4444;", "High priority")),
        TaskPriority::Medium => Some(("color:#f59e0b;", "Medium priority")),
        TaskPriority::Low => None,
    };

    let zone_accent = match zone {
        KanbanZone::Today => "border-l-2 border-amber-400",
        KanbanZone::Backlog => "border-l-2 border-stone-200 dark:border-stone-700",
    };

    // Static class string (no move closure) — keeps Tailwind from
    // re-evaluating the class list on every status change, which was
    // leaving transient hover/border artifacts when the cursor swept the
    // list quickly.  Dynamic state lives in the inline `style=move ||`
    // (opacity for done tasks, focus ring for keyboard cursor) where
    // Leptos diffs cleanly.
    let row_class = format!(
        "group flex items-start gap-2 py-2 px-3 rounded-r-lg \
         hover:bg-stone-50 dark:hover:bg-stone-800/50 \
         cursor-pointer {zone_accent}"
    );
    view! {
        <div
            draggable="true"
            on:dragstart=on_dragstart
            on:click=on_row_click
            class=row_class
            style=move || {
                let mut s = String::new();
                if status_done(&status_sig.get()) { s.push_str("opacity:0.5;"); }
                if is_focused() {
                    // Inner amber ring; sits inside the row so it
                    // doesn't add layout (no offset) and reads as
                    // "the keyboard is here" without competing with
                    // the zone's left-border accent.
                    s.push_str("box-shadow:inset 0 0 0 2px #f59e0b;background-color:rgba(245,158,11,0.04);");
                }
                s
            }
            data-task-id=task_id.0.to_string()
            title="Click to open the task in its parent node"
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

            // Body — project chip + title + meta, OR inline edit form
            <div class="flex-1 min-w-0">
                // Parent-node chip — amber so it pops against the title
                // (the app's accent colour; matches the Today-zone left
                // border, the priority dots, and the focused-task flash).
                <div class="flex items-center gap-1.5">
                    <span class="material-symbols-outlined text-amber-600 dark:text-amber-500"
                          style="font-size:13px;">{node_icon}</span>
                    <span class="text-xs font-semibold uppercase tracking-wide \
                                 text-amber-700 dark:text-amber-400 truncate">
                        {parent}
                    </span>
                    {carryover_from.map(|d| {
                        let label = d.format("%b %-d").to_string();
                        let title_attr = format!("Was focused on {label}");
                        view! {
                            <span class="text-xs text-stone-500 dark:text-stone-400 flex-shrink-0"
                                  title=title_attr>
                                " · carried from " {label}
                            </span>
                        }
                    })}
                </div>

                {move || if is_editing() {
                    // ── Inline edit form ─────────────────────────────
                    let saved_height = use_context::<TaskEditorHeights>()
                        .and_then(|c| c.0.get_untracked().get(&task_id.0).copied());
                    view! {
                        <div class="mt-1 space-y-2"
                             on:click=move |ev: web_sys::MouseEvent| ev.stop_propagation()>
                            <crate::components::resizable_editor::ResizableEditor
                                value=edit_title
                                placeholder="Task title…"
                                initial_height=saved_height
                                on_submit=Callback::new(move |()| do_save())
                                on_escape=Callback::new(move |()| editing_id_sig.set(None))
                                on_resize=Callback::new(move |h: i32| {
                                    wasm_bindgen_futures::spawn_local(async move {
                                        // Best-effort: losing a height pref is cosmetic.
                                        let _ = crate::api::set_editor_pref("task", task_id.0, h).await;
                                    });
                                })
                                class="w-full bg-stone-100 dark:bg-stone-800 text-sm \
                                       text-stone-900 dark:text-stone-100 rounded px-2 py-1 resize-y \
                                       min-h-[40px] focus:outline-none focus:ring-1 focus:ring-amber-500".to_string()
                            />
                            <div class="flex items-center gap-2 flex-wrap">
                                <span class="text-xs text-stone-400 dark:text-stone-500">"Priority"</span>
                                {["low", "medium", "high"].iter().map(|&p| {
                                    let (label, sel_style) = match p {
                                        "high"   => ("High",   "color:#ef4444;border-color:#ef4444;"),
                                        "medium" => ("Medium", "color:#f59e0b;border-color:#f59e0b;"),
                                        _        => ("Low",    "color:#9ca3af;border-color:#9ca3af;"),
                                    };
                                    view! {
                                        <button
                                            type="button"
                                            class="text-xs px-2 py-0.5 rounded border transition-colors cursor-pointer"
                                            style=move || if edit_priority.get() == p {
                                                format!("{sel_style}font-weight:600;")
                                            } else {
                                                "color:#9ca3af;border-color:#d1d5db;".to_string()
                                            }
                                            on:click=move |ev: web_sys::MouseEvent| {
                                                ev.stop_propagation();
                                                edit_priority.set(p.to_string());
                                            }
                                        >
                                            {label}
                                        </button>
                                    }
                                }).collect_view()}
                            </div>
                            <div class="flex items-center gap-2 flex-wrap">
                                <input
                                    type="date"
                                    class="text-xs bg-stone-100 dark:bg-stone-700 \
                                           text-stone-700 dark:text-stone-300 \
                                           rounded px-2 py-0.5 focus:outline-none \
                                           focus:ring-1 focus:ring-amber-500"
                                    title="Due date (optional)"
                                    prop:value=move || edit_due.get()
                                    on:input=move |ev| edit_due.set(event_target_value(&ev))
                                />
                                <select
                                    class="text-xs bg-stone-100 dark:bg-stone-700 \
                                           text-stone-700 dark:text-stone-300 \
                                           rounded px-2 py-0.5 focus:outline-none \
                                           focus:ring-1 focus:ring-amber-500"
                                    title="Recurrence"
                                    on:change=move |ev| edit_recurrence.set(event_target_value(&ev))
                                >
                                    <option value="" selected=move || edit_recurrence.get().is_empty()>
                                        "No repeat"
                                    </option>
                                    {[
                                        ("daily",    "Daily"),
                                        ("weekly",   "Weekly"),
                                        ("biweekly", "Every 2 weeks"),
                                        ("monthly",  "Monthly"),
                                        ("yearly",   "Yearly"),
                                    ].iter().map(|&(val, label)| {
                                        view! {
                                            <option value=val selected=move || edit_recurrence.get() == val>
                                                {label}
                                            </option>
                                        }
                                    }).collect_view()}
                                </select>
                                <span class="flex-1"/>
                                <IconButton
                                    icon="check"
                                    label="Save"
                                    variant=IconButtonVariant::Save
                                    stop_propagation=true
                                    on_click=Callback::new(move |()| do_save())
                                />
                                <IconButton
                                    icon="close"
                                    label="Cancel"
                                    stop_propagation=true
                                    on_click=Callback::new(move |()| editing_id_sig.set(None))
                                />
                            </div>
                        </div>
                    }.into_any()
                } else {
                    // ── Display row ──────────────────────────────────
                    view! {
                        <div class="flex items-center gap-2 mt-0.5">
                            {priority_dot.map(|(s, label)| view! {
                                <span
                                    style=format!("{s}font-size:8px;line-height:1;")
                                    title=label
                                    aria-label=label
                                    role="img"
                                >"●"</span>
                            })}
                            <span class="text-sm text-stone-800 dark:text-stone-200 truncate"
                                  style=move || if status_done(&status_sig.get()) {
                                      "text-decoration:line-through;"
                                  } else { "" }
                                  inner_html=move || crate::markdown::render_markdown_inline(&title_sig.get())>
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
                    }.into_any()
                }}
            </div>

            // Actions — always visible, never trigger row navigation
            <div class="flex items-center gap-0.5 flex-shrink-0">
                {match zone {
                    KanbanZone::Today => view! {
                        <button
                            type="button"
                            class="p-1 rounded text-amber-500 hover:text-amber-700 \
                                   hover:bg-amber-50 dark:hover:bg-amber-950/40 \
                                   transition-colors cursor-pointer disabled:opacity-50"
                            prop:disabled=move || busy.get()
                            on:click=move |ev: web_sys::MouseEvent| {
                                ev.stop_propagation();
                                patch_focus(None, "Removed from today");
                            }
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
                            on:click=move |ev: web_sys::MouseEvent| {
                                ev.stop_propagation();
                                patch_focus(Some(today), "Added to today");
                            }
                            title="Add to today"
                        >
                            <span class="material-symbols-outlined" style="font-size:16px;">"wb_sunny"</span>
                        </button>
                    }.into_any(),
                }}
                <button
                    type="button"
                    class="p-1 rounded text-stone-300 dark:text-stone-600 \
                           hover:text-stone-700 dark:hover:text-stone-200 \
                           transition-colors cursor-pointer"
                    on:click=on_pencil
                    title="Edit this task in place"
                >
                    <span class="material-symbols-outlined" style="font-size:16px;">"edit"</span>
                </button>
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
