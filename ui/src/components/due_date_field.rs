//! The one task due-date field — every task date input renders through it.
//!
//! A native `<input type="date">` (best picker on phones) plus a readable
//! echo of the chosen date ("Fri, Oct 30 · in 7 days") and a past-date
//! warning, so a stray keypress in a segment (Oct → Jan) is visible before
//! Save. The segments are browser-internal and can't carry `aria-valuetext`;
//! the echo and warning reach assistive tech through `aria-describedby`
//! instead. Logic lives in `common::task::{due_hint, warn_past_due}`.

use std::sync::atomic::{AtomicU32, Ordering};

use chrono::NaiveDate;
use common::task::{due_hint, warn_past_due};
use leptos::prelude::*;

use crate::components::format_helpers::local_today;

/// Compact input classes shared by the row edit forms and the add-task form.
pub const DUE_INPUT_CLASS: &str = "text-xs bg-stone-100 dark:bg-stone-700 \
     text-stone-700 dark:text-stone-300 rounded-lg px-2 py-0.5 \
     focus:outline-none focus:ring-1 focus:ring-amber-500";

/// Default wrapper: input, echo, and warning flow inline and wrap on phones.
const WRAPPER_CLASS: &str = "inline-flex flex-wrap items-center gap-x-2 gap-y-0.5";

/// Per-instance id suffix for `aria-describedby` (several fields can be on
/// screen at once — one per row being edited).
static NEXT_ID: AtomicU32 = AtomicU32::new(0);

#[component]
pub fn DueDateField(
    /// The input's `YYYY-MM-DD` string (`""` = no due date).
    value: RwSignal<String>,
    /// The task's saved due date (`None` for a new task). The past-date
    /// warning only fires when the date differs from this.
    #[prop(default = None)]
    original: Option<NaiveDate>,
    /// Input classes; defaults to [`DUE_INPUT_CLASS`].
    #[prop(optional)]
    input_class: Option<&'static str>,
    /// Wrapper classes (e.g. `flex-1` when the field shares a row).
    #[prop(optional)]
    wrapper_class: Option<&'static str>,
) -> impl IntoView {
    let id = NEXT_ID.fetch_add(1, Ordering::Relaxed);
    let hint_id = format!("due-hint-{id}");
    let warn_id = format!("due-warn-{id}");
    let described_by = format!("{hint_id} {warn_id}");
    let today = local_today();
    let parsed = move || value.get().trim().parse::<NaiveDate>().ok();

    view! {
        <span class=wrapper_class.unwrap_or(WRAPPER_CLASS) data-testid="due-date-field">
            <input
                type="date"
                class=input_class.unwrap_or(DUE_INPUT_CLASS)
                title="Due date (optional)"
                aria-describedby=described_by
                prop:value=move || value.get()
                on:input=move |ev| value.set(event_target_value(&ev))
            />
            <span id=hint_id class="text-xs text-stone-500 dark:text-stone-400" data-testid="due-hint">
                {move || parsed().map(|d| due_hint(d, today).label)}
            </span>
            <span
                id=warn_id
                class="text-xs text-heat-high"
                data-testid="due-warning"
                aria-live="polite"
            >
                {move || warn_past_due(parsed(), original, today).then_some("This date is in the past")}
            </span>
        </span>
    }
}
