//! Reusable carry-over section.
//!
//! Originally introduced in v2.4.1 inside `my_day_view.rs`; lifted here
//! so the v2.5.0 morning planning ritual at `/plan` can reuse it without
//! pulling in MyDay-specific layout.
//!
//! Scope: render a list of `MyDayTask` entries with `focus_date < today`
//! and not done, each with one-tap **Today** / **Reschedule** / **× Drop**
//! actions.  Hidden when the list is empty so empty headers never appear.

use chrono::NaiveDate;
use common::task::{MyDayTask, Task, UpdateTaskRequest};
use leptos::prelude::*;

use crate::components::toast::{push_toast, ToastLevel};

/// Renders the full "Carry over (N)" panel.  Pass an already-filtered
/// list of tasks where `focus_date < today` AND not done.
#[component]
pub fn CarryoverSection(
    carryovers: Vec<MyDayTask>,
    refresh: RwSignal<u32>,
    today: NaiveDate,
) -> impl IntoView {
    if carryovers.is_empty() {
        return ().into_any();
    }
    let count = carryovers.len();
    view! {
        <div class="rounded-lg border border-amber-200 dark:border-amber-900/40 bg-amber-50/40 \
                    dark:bg-amber-950/20">
            <div class="flex items-center gap-2 px-3 py-2 border-b border-amber-200 \
                        dark:border-amber-900/40">
                <span class="material-symbols-outlined text-amber-600 dark:text-amber-500" style="font-size:16px;">
                    "history"
                </span>
                <span class="text-xs font-semibold text-amber-700 dark:text-amber-400 uppercase tracking-wider">
                    {format!("Carry over ({count})")}
                </span>
                <span class="text-xs text-stone-500 dark:text-stone-400 ml-1">
                    "— from earlier days, still open"
                </span>
            </div>
            <div class="divide-y divide-amber-100 dark:divide-amber-900/30">
                {carryovers.into_iter().map(|MyDayTask { task, node_title }| {
                    view! {
                        <CarryoverRow
                            task=task
                            node_title=node_title
                            today=today
                            refresh=refresh
                        />
                    }
                }).collect_view()}
            </div>
        </div>
    }.into_any()
}

#[component]
fn CarryoverRow(
    task: Task,
    node_title: Option<String>,
    today: NaiveDate,
    refresh: RwSignal<u32>,
) -> impl IntoView {
    let task_id      = task.id;
    let from_date    = task.focus_date.unwrap_or(today);
    let from_label   = from_date.format("%b %-d").to_string();
    let title        = task.title.clone();
    let parent_label = node_title.unwrap_or_else(|| "Inbox".to_string());

    let show_picker = RwSignal::new(false);
    let picked_date = RwSignal::new(today.format("%Y-%m-%d").to_string());
    let busy        = RwSignal::new(false);

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
                Err(e) => {
                    push_toast(ToastLevel::Error, format!("Couldn't update task: {e}"));
                }
            }
        });
    };

    let on_move_today = move |_| patch_focus(Some(today), "Moved to today");
    let on_drop       = move |_| patch_focus(None, "Dropped from My Day");
    let on_reschedule_apply = move |_| {
        let raw = picked_date.get_untracked();
        match raw.parse::<NaiveDate>() {
            Ok(d) if d == today => patch_focus(Some(d), "Moved to today"),
            Ok(d)                => patch_focus(Some(d), "Rescheduled"),
            Err(_) => push_toast(ToastLevel::Error, "Pick a valid date".to_string()),
        }
        show_picker.set(false);
    };

    view! {
        <div class="px-3 py-2">
            <div class="flex items-start gap-2">
                <span class="material-symbols-outlined text-amber-500 flex-shrink-0 mt-0.5"
                      style="font-size:14px;"
                      title=format!("Focused on {from_label}")>
                    "schedule"
                </span>
                <div class="flex-1 min-w-0">
                    <div class="text-sm text-stone-800 dark:text-stone-200 truncate">
                        {title}
                    </div>
                    <div class="text-xs text-stone-500 dark:text-stone-400 truncate">
                        {parent_label} " · from " {from_label.clone()}
                    </div>
                </div>
                <div class="flex items-center gap-1 flex-shrink-0">
                    <button
                        class="px-2 py-0.5 text-xs rounded bg-amber-600 text-white \
                               hover:bg-amber-700 disabled:opacity-50 cursor-pointer"
                        prop:disabled=move || busy.get()
                        on:click=on_move_today
                        title="Set focus date to today"
                    >
                        "Today"
                    </button>
                    <button
                        class="px-2 py-0.5 text-xs rounded border border-stone-300 \
                               dark:border-stone-600 text-stone-600 dark:text-stone-300 \
                               hover:bg-stone-100 dark:hover:bg-stone-800 \
                               disabled:opacity-50 cursor-pointer"
                        prop:disabled=move || busy.get()
                        on:click=move |_| show_picker.update(|v| *v = !*v)
                        title="Reschedule to a future date"
                    >
                        "Reschedule"
                    </button>
                    <button
                        class="p-1 rounded text-stone-400 hover:text-red-500 \
                               disabled:opacity-50 cursor-pointer"
                        prop:disabled=move || busy.get()
                        on:click=on_drop
                        title="Remove from My Day (back to Inbox / no focus date)"
                    >
                        <span class="material-symbols-outlined" style="font-size:16px;">"close"</span>
                    </button>
                </div>
            </div>
            <Show when=move || show_picker.get()>
                <div class="mt-2 flex items-center gap-2">
                    <input
                        type="date"
                        class="text-xs bg-stone-100 dark:bg-stone-700 \
                               text-stone-700 dark:text-stone-300 \
                               rounded px-2 py-0.5 focus:outline-none \
                               focus:ring-1 focus:ring-amber-500"
                        prop:value=move || picked_date.get()
                        on:input=move |ev| picked_date.set(event_target_value(&ev))
                    />
                    <button
                        class="px-2 py-0.5 text-xs rounded bg-amber-600 text-white \
                               hover:bg-amber-700 disabled:opacity-50 cursor-pointer"
                        prop:disabled=move || busy.get()
                        on:click=on_reschedule_apply
                    >
                        "Apply"
                    </button>
                    <button
                        class="px-2 py-0.5 text-xs rounded text-stone-500 \
                               hover:text-stone-800 dark:hover:text-stone-200 cursor-pointer"
                        on:click=move |_| show_picker.set(false)
                    >
                        "Cancel"
                    </button>
                </div>
            </Show>
        </div>
    }
}
