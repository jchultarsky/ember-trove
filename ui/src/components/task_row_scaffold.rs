//! Shared display-mode skeleton for task rows.
//!
//! My Day (`task_row.rs`), the Inbox (`inbox_view.rs`), and the node task
//! panel (`task_panel.rs`) are all "lists of tasks" and share one geometry
//! contract:
//!
//! ```text
//! [checkbox] [ title line  (dot + title)          ] [action cluster]
//!            [ meta line   (chips · due · badges) ]
//! ```
//!
//! The title leads; supporting metadata sits on a small second line that
//! wraps freely at phone widths. Each list keeps its own outer container
//! (drag/click/borders genuinely differ). Edit mode is shared too:
//! [`TaskEditor`] is the one inline editor (#132), and [`PriorityPicker`]
//! the one priority control.

use std::sync::Arc;

use chrono::NaiveDate;
use common::id::TaskId;
use common::task::{TaskPriority, UpdateTaskRequest};
use leptos::prelude::*;

use crate::components::due_date_field::{DUE_INPUT_CLASS, DueDateField};
use crate::components::icon_button::{IconButton, IconButtonVariant};
use crate::components::resizable_editor::ResizableEditor;
use crate::components::task_actions::TaskEditDraft;
use crate::components::task_common::parse_priority;
use crate::components::task_row::TaskEditorHeights;

/// Standard task checkbox classes (5×5, amber hover) — pair with an amber
/// fill + white `check` glyph when done.
pub const CHECKBOX_CLASS: &str = "flex-shrink-0 mt-0.5 w-5 h-5 rounded border-2 \
     border-stone-300 dark:border-stone-600 flex items-center \
     justify-center hover:border-amber-500 transition-colors cursor-pointer";

/// The task checkbox: ticked (amber fill + `check`) for terminal statuses.
/// Clicks never bubble to the row (rows navigate on click). Wire `on_toggle`
/// to [`crate::components::task_actions::toggle_done`], passing the event
/// for the completion spark.
#[component]
pub fn TaskCheckbox(
    /// Reactive "is this task ticked" (`status_done(&status.get())`).
    #[prop(into)]
    done: Signal<bool>,
    #[prop(into)] on_toggle: Callback<web_sys::MouseEvent>,
) -> impl IntoView {
    view! {
        <button
            type="button"
            class=CHECKBOX_CLASS
            style=move || if done.get() {
                "background:#d97706;border-color:#d97706;"
            } else { "" }
            on:click=move |ev: web_sys::MouseEvent| {
                ev.stop_propagation();
                on_toggle.run(ev);
            }
            title="Toggle done"
            aria-pressed=move || if done.get() { "true" } else { "false" }
        >
            {move || done.get().then(|| view! {
                <span aria-hidden="true" class="material-symbols-outlined text-white" style="font-size:13px;">"check"</span>
            })}
        </button>
    }
}

/// The row ☀ action (Add to / Remove from My Day): filled amber when the task
/// is in My Day. Wire `on_toggle` to
/// [`crate::components::task_actions::toggle_my_day`].
#[component]
pub fn MyDayButton(
    #[prop(into)] active: Signal<bool>,
    #[prop(into)] on_toggle: Callback<()>,
) -> impl IntoView {
    let label = move || {
        if active.get() {
            "Remove from My Day"
        } else {
            "Add to My Day"
        }
    };
    view! {
        <button
            type="button"
            class=move || if active.get() {
                "p-1.5 rounded text-amber-500 bg-amber-50 dark:text-amber-400 \
                 dark:bg-amber-900/30 hover:text-amber-600 dark:hover:text-amber-300 \
                 transition-colors cursor-pointer".to_string()
            } else {
                action_btn_class("hover:text-amber-500 dark:hover:text-amber-400")
            }
            title=label
            aria-label=label
            aria-pressed=move || if active.get() { "true" } else { "false" }
            on:click=move |ev: web_sys::MouseEvent| {
                ev.stop_propagation();
                on_toggle.run(());
            }
        >
            <span
                class="material-symbols-outlined"
                style=move || if active.get() {
                    "font-size:16px; font-variation-settings: 'FILL' 1;"
                } else {
                    "font-size:16px;"
                }
            >"wb_sunny"</span>
        </button>
    }
}

/// The `<option>`s of every recurrence `<select>`: "No repeat" plus each rule
/// with its one label (`task_common::recurrence_label`). The copies used to
/// disagree ("Biweekly" vs "Every 2 weeks"). Each option carries its own
/// `selected` binding: a `prop:value` on the `<select>` can be applied before
/// its options exist, which leaves a freshly opened editor on "No repeat".
#[component]
pub fn RecurrenceOptions(value: RwSignal<String>) -> impl IntoView {
    view! {
        <option value="" selected=move || value.get().is_empty()>"No repeat"</option>
        {common::task::RecurrenceRule::ALL
            .iter()
            .map(|r| {
                let v = r.as_str();
                view! {
                    <option value=v selected=move || value.get() == v>
                        {crate::components::task_common::recurrence_label(r)}
                    </option>
                }
            })
            .collect_view()}
    }
}

/// Text colour and selected tint per priority: the row dot's colours
/// (`priority_dot_view`), so the picker and the dot read as one cue.
fn priority_tint(p: &TaskPriority) -> (&'static str, &'static str) {
    match p {
        TaskPriority::High => ("#ef4444", "rgba(239,68,68,0.12)"),
        TaskPriority::Medium => ("#f59e0b", "rgba(245,158,11,0.14)"),
        TaskPriority::Low => ("#9ca3af", "rgba(156,163,175,0.18)"),
    }
}

/// The one priority control (#132): a labelled segmented radio group over
/// `TaskPriority::ALL`, used by every task editor and the add-task form.
/// Labels, not colour-only dots (WCAG 1.4.1); WAI-ARIA APG radio group —
/// one tab stop on the checked option, arrow keys move the selection.
#[component]
pub fn PriorityPicker(
    /// The priority's wire string (`"low"` / `"medium"` / `"high"`).
    value: RwSignal<String>,
) -> impl IntoView {
    let refs = StoredValue::new(
        TaskPriority::ALL
            .iter()
            .map(|_| NodeRef::<leptos::html::Button>::new())
            .collect::<Vec<_>>(),
    );
    let on_keydown = move |ev: leptos::ev::KeyboardEvent| {
        let down = match ev.key().as_str() {
            "ArrowRight" | "ArrowDown" => true,
            "ArrowLeft" | "ArrowUp" => false,
            _ => return,
        };
        ev.prevent_default();
        ev.stop_propagation();
        let current = parse_priority(&value.get_untracked());
        let active = TaskPriority::ALL.iter().position(|p| *p == current);
        let Some(i) = common::picker::step(active, TaskPriority::ALL.len(), down) else {
            return;
        };
        if let Some(p) = TaskPriority::ALL.get(i) {
            value.set(p.as_str().to_string());
        }
        // Focus follows the selection (roving tab stop).
        if let Some(btn) = refs.with_value(|r| r.get(i).and_then(|n| n.get_untracked())) {
            let _ = btn.focus();
        }
    };
    view! {
        <div
            role="radiogroup"
            aria-label="Priority"
            class="inline-flex items-center rounded-lg border border-stone-200 dark:border-stone-700 overflow-hidden"
            data-testid="priority-picker"
            on:keydown=on_keydown
        >
            {TaskPriority::ALL
                .iter()
                .enumerate()
                .map(|(i, p)| {
                    let v = p.as_str();
                    let (fg, tint) = priority_tint(p);
                    // An unknown value reads as Medium (`parse_priority`), so
                    // there is always exactly one checked option / tab stop.
                    let checked = move || parse_priority(&value.get()).as_str() == v;
                    let node_ref = refs.with_value(|r| r.get(i).copied()).unwrap_or_default();
                    view! {
                        <button
                            type="button"
                            role="radio"
                            node_ref=node_ref
                            aria-checked=move || if checked() { "true" } else { "false" }
                            tabindex=move || if checked() { "0" } else { "-1" }
                            class="text-xs px-2 py-0.5 transition-colors cursor-pointer \
                                   text-stone-500 dark:text-stone-400 \
                                   hover:bg-stone-100 dark:hover:bg-stone-800 \
                                   focus:outline-none focus-visible:ring-1 focus-visible:ring-amber-500"
                            style=move || if checked() {
                                format!("color:{fg};background-color:{tint};font-weight:600;")
                            } else {
                                String::new()
                            }
                            on:click=move |ev: web_sys::MouseEvent| {
                                ev.stop_propagation();
                                value.set(v.to_string());
                            }
                        >
                            {p.label()}
                        </button>
                    }
                })
                .collect_view()}
        </div>
    }
}

/// [`TaskEditor`] title-box density: `Compact` for list rows (My Day, the
/// node panel), `Comfortable` for the Inbox's roomier cards. Only the
/// textarea differs; the controls are identical everywhere.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum EditorDensity {
    #[default]
    Compact,
    Comfortable,
}

impl EditorDensity {
    fn textarea_class(self) -> &'static str {
        match self {
            EditorDensity::Compact => {
                "w-full bg-stone-100 dark:bg-stone-800 text-sm \
                 text-stone-900 dark:text-stone-100 rounded px-2 py-1 resize-y \
                 min-h-[40px] focus:outline-none focus:ring-1 focus:ring-amber-500"
            }
            EditorDensity::Comfortable => {
                "w-full text-sm rounded-lg border border-amber-400 \
                 bg-white dark:bg-stone-800 px-3 py-2 resize-y min-h-[44px] \
                 text-stone-900 dark:text-stone-100 \
                 focus:outline-none focus:ring-1 focus:ring-amber-400"
            }
        }
    }
}

/// The one inline task editor (#132): title, priority, due date, repeat,
/// Save / Cancel — the same controls and keys in My Day, the Inbox and the
/// node panel. The draft semantics live here too: Cancel resets every
/// field, Escape discards with "Edit discarded · Undo" (`TaskEditDraft`),
/// and Save hands the list the PATCH. Escape and Ctrl/⌘+Enter work from
/// any control, not just the title box. Each list keeps only its open
/// state (`close` / `reopen`) and its save side effects (`on_save`).
#[component]
pub fn TaskEditor(
    draft: TaskEditDraft,
    task_id: TaskId,
    /// The task's saved due date (the past-date warning's baseline).
    due: Option<NaiveDate>,
    /// Runs after the editor closed, with the PATCH to send (blank title =
    /// no call). Wire it to `task_actions::patch_task`.
    #[prop(into)]
    on_save: Callback<UpdateTaskRequest>,
    /// Close the editor (the list's open state).
    #[prop(into)]
    close: Callback<()>,
    /// Reopen it, from the Undo toast. Must tolerate a row that was
    /// refetched away meanwhile (`try_set`).
    reopen: Arc<dyn Fn() + Send + Sync>,
    #[prop(optional)] density: EditorDensity,
) -> impl IntoView {
    let save = Callback::new(move |()| {
        if let Some(req) = draft.take_request() {
            close.run(());
            on_save.run(req);
        }
    });
    let escape = Callback::new(move |()| {
        close.run(());
        let reopen = reopen.clone();
        draft.discard_with_undo(move || reopen());
    });
    let cancel = Callback::new(move |()| {
        draft.reset();
        close.run(());
    });
    // Keys from the other controls. The title box handles its own (and
    // `preventDefault`s them), so skip those here.
    let on_keydown = move |ev: leptos::ev::KeyboardEvent| {
        if ev.default_prevented() {
            return;
        }
        if ev.key() == "Escape" {
            ev.prevent_default();
            // Same reason as the title box: the window dispatcher would
            // otherwise read it as "leave the page" (#123).
            ev.stop_propagation();
            escape.run(());
        } else if ev.key() == "Enter" && (ev.ctrl_key() || ev.meta_key()) {
            ev.prevent_default();
            save.run(());
        }
    };
    view! {
        // Clicks stay in the editor (My Day rows navigate on click).
        <div
            class="space-y-2"
            data-testid="task-editor"
            on:click=move |ev: web_sys::MouseEvent| ev.stop_propagation()
            on:keydown=on_keydown
        >
            <ResizableEditor
                value=draft.title
                placeholder="Task title\u{2026}"
                initial_height=TaskEditorHeights::saved(task_id)
                on_submit=save
                on_escape=escape
                on_resize=TaskEditorHeights::on_resize(task_id)
                class=density.textarea_class().to_string()
            />
            <div class="flex items-center gap-2 flex-wrap">
                <PriorityPicker value=draft.priority />
                <DueDateField value=draft.due original=due />
                <select
                    class=DUE_INPUT_CLASS
                    title="Recurrence"
                    on:change=move |ev| draft.recurrence.set(event_target_value(&ev))
                >
                    <RecurrenceOptions value=draft.recurrence />
                </select>
                <span class="flex-1" />
                <IconButton
                    icon="check"
                    label="Save"
                    variant=IconButtonVariant::Save
                    stop_propagation=true
                    on_click=save
                />
                <IconButton icon="close" label="Cancel" stop_propagation=true on_click=cancel />
            </div>
        </div>
    }
}

/// Title classes: wraps below `sm` (phone real estate), truncates at sm+
/// (list density).
pub const TITLE_CLASS: &str =
    "text-sm text-stone-800 dark:text-stone-200 min-w-0 break-words sm:truncate";

/// Compact icon-action button classes; pass the hover colour utilities
/// (e.g. `"hover:text-red-500 dark:hover:text-red-400"`).
pub fn action_btn_class(hover: &'static str) -> String {
    format!(
        "p-1.5 rounded text-stone-400 dark:text-stone-500 {hover} \
         hover:bg-stone-100 dark:hover:bg-stone-800 \
         transition-colors cursor-pointer disabled:opacity-50"
    )
}

/// The colour-coded priority dot (High red, Medium amber, Low renders
/// nothing), accessible name on title/aria-label.
pub fn priority_dot_view(priority: &TaskPriority) -> Option<AnyView> {
    let (style, label) = match priority {
        TaskPriority::High => ("color:#ef4444;", "High priority"),
        TaskPriority::Medium => ("color:#f59e0b;", "Medium priority"),
        TaskPriority::Low => return None,
    };
    Some(
        view! {
            <span
                class="flex-shrink-0 mt-1.5 sm:mt-0"
                style=format!("{style}font-size:8px;line-height:1;")
                title=label
                aria-label=label
                role="img"
            >"●"</span>
        }
        .into_any(),
    )
}

/// Standard due-date badge for the meta line: `⚠ due <date>` red when
/// overdue, muted otherwise.
pub fn due_badge_view(due: chrono::NaiveDate, overdue: bool) -> AnyView {
    let style = if overdue {
        "color:#dc2626;font-weight:600;"
    } else {
        "color:#9ca3af;"
    };
    let label = if overdue {
        format!("⚠ due {}", due.format("%b %-d"))
    } else {
        format!("due {}", due.format("%b %-d"))
    };
    view! {
        <span style=style class="flex-shrink-0" title="External deadline">{label}</span>
    }
    .into_any()
}

/// Display-mode body: title line over a wrapping meta line. Render inside a
/// `flex-1 min-w-0` body slot of a `flex items-start gap-2` row container.
#[component]
pub fn TaskRowBody(title_line: AnyView, meta_line: AnyView) -> impl IntoView {
    view! {
        <div class="flex items-start sm:items-center gap-2">{title_line}</div>
        <div class="flex flex-wrap items-center gap-x-2 gap-y-0.5 mt-0.5 text-xs">
            {meta_line}
        </div>
    }
}
