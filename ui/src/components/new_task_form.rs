//! The add-task form of a Project's `TaskPanel`: icon submit,
//! priority/due/recurrence controls and an inline error.
//!
//! It always creates the task on its node. The Inbox's variant with an
//! optional Project picker went with the Inbox (S4c, #240): a task needs its
//! Project (ONTOLOGY §9.3), and loose input is a capture.
//!
//! The ☀ toggle commits the new task to My Day in the same request
//! (`focus_date` = the viewer's local today), instead of a create-then-toggle
//! second step. Editing a task deliberately has no such field: each row's own
//! ☀ action already applies immediately, and a second, Save-gated control for
//! the same state would drift from it.

use chrono::NaiveDate;
use common::{id::NodeId, task::CreateTaskRequest};
use leptos::prelude::*;

use crate::components::due_date_field::DueDateField;
use crate::components::format_helpers::local_today;
use crate::components::task_common::parse_priority;
use crate::components::task_row_scaffold::{PriorityPicker, RecurrenceOptions};

/// Reusable "new task" form.
///
/// * `node_id` — the Project the task is created on.
/// * `refresh` — bumped after a successful create so the surrounding list reloads.
/// * `on_added` — optional callback invoked after a successful create (the node
///   panel uses it to collapse the form again).
#[component]
pub fn NewTaskForm(
    node_id: NodeId,
    refresh: RwSignal<u32>,
    #[prop(optional)] on_added: Option<Callback<()>>,
) -> impl IntoView {
    // Core form state
    let new_title = RwSignal::new(String::new());
    let new_priority = RwSignal::new("medium".to_string());
    let new_due = RwSignal::new(String::new());
    let new_recurrence = RwSignal::new(String::new());
    let new_my_day = RwSignal::new(false);
    let adding = RwSignal::new(false);
    let add_error = RwSignal::new(Option::<String>::None);

    let do_add = move || {
        // Enter bypassed the button's `disabled`: a fast double Enter created
        // the task twice.
        if adding.get_untracked() {
            return;
        }
        let title = new_title.get_untracked().trim().to_string();
        if title.is_empty() {
            add_error.set(Some("Title is required.".to_string()));
            return;
        }
        let priority = parse_priority(&new_priority.get_untracked());
        let due_date = new_due.get_untracked().trim().parse::<NaiveDate>().ok();
        let recurrence = common::task::RecurrenceRule::parse(&new_recurrence.get_untracked());
        let focus_date = new_my_day.get_untracked().then(local_today);
        adding.set(true);
        add_error.set(None);
        wasm_bindgen_futures::spawn_local(async move {
            let req = CreateTaskRequest {
                title,
                node_id: Some(node_id),
                status: None,
                priority: Some(priority),
                focus_date,
                due_date,
                recurrence,
            };
            match crate::api::create_task(node_id, &req).await {
                Ok(_) => {
                    new_title.set(String::new());
                    new_priority.set("medium".to_string());
                    new_due.set(String::new());
                    new_recurrence.set(String::new());
                    new_my_day.set(false);
                    refresh.update(|n| *n += 1);
                    if let Some(cb) = on_added {
                        cb.run(());
                    }
                }
                Err(e) => add_error.set(Some(format!("{e}"))),
            }
            adding.set(false);
        });
    };

    view! {
        <div
            class="p-3 rounded-lg bg-stone-50 dark:bg-stone-800/50
                border border-stone-200 dark:border-stone-700 space-y-2"
            data-testid="new-task-form"
        >
            <input
                type="text"
                placeholder="Task title…"
                class="w-full bg-transparent text-sm text-stone-900 dark:text-stone-100
                    focus:outline-none placeholder-stone-400"
                prop:value=move || new_title.get()
                on:input=move |ev| new_title.set(event_target_value(&ev))
                on:keydown=move |ev: leptos::ev::KeyboardEvent| {
                    if ev.key() == "Enter" { do_add(); }
                }
            />
            <div class="flex items-center gap-2 flex-wrap">
                <PriorityPicker value=new_priority />
                <DueDateField value=new_due />
                <select
                    class="text-xs bg-stone-100 dark:bg-stone-700 text-stone-700 dark:text-stone-300
                        rounded-lg px-2 py-1 focus:outline-none"
                    title="Recurrence"
                    prop:value=move || new_recurrence.get()
                    on:change=move |ev| new_recurrence.set(event_target_value(&ev))
                >
                    <RecurrenceOptions value=new_recurrence />
                </select>
                // My Day — same glyph as the row action; filled when on.
                <button
                    type="button"
                    class=move || if new_my_day.get() {
                        "p-1 rounded-lg text-amber-500 bg-amber-50 dark:text-amber-400 \
                         dark:bg-amber-900/30 transition-colors cursor-pointer"
                    } else {
                        "p-1 rounded-lg text-stone-400 dark:text-stone-500 \
                         hover:text-amber-500 dark:hover:text-amber-400 \
                         hover:bg-stone-100 dark:hover:bg-stone-700 transition-colors cursor-pointer"
                    }
                    title=move || if new_my_day.get() { "Will be added to My Day" } else { "Add to My Day" }
                    aria-label="Add to My Day"
                    aria-pressed=move || if new_my_day.get() { "true" } else { "false" }
                    on:click=move |_| new_my_day.update(|v| *v = !*v)
                >
                    <span
                        class="material-symbols-outlined"
                        style=move || if new_my_day.get() {
                            "font-size:18px; font-variation-settings: 'FILL' 1;"
                        } else {
                            "font-size:18px;"
                        }
                    >"wb_sunny"</span>
                </button>
                <span class="flex-1"/>
                <button
                    class="p-1.5 rounded-lg text-stone-400 hover:text-green-600 dark:hover:text-green-400
                        hover:bg-green-50 dark:hover:bg-green-900/30 transition-colors cursor-pointer
                        disabled:opacity-50 disabled:cursor-not-allowed"
                    title=move || if adding.get() { "Adding…" } else { "Add task" }
                    on:click=move |_| do_add()
                    disabled=move || adding.get()
                >
                    <span aria-hidden="true" class="material-symbols-outlined">
                        {move || if adding.get() { "hourglass_empty" } else { "add" }}
                    </span>
                </button>
            </div>

            {move || add_error.get().map(|msg| view! {
                <p class="text-xs text-red-500">{msg}</p>
            })}
        </div>
    }
}
