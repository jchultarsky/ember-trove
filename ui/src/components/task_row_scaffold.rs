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
//! (drag/click/borders genuinely differ) and its own inline editor — the
//! scaffold is display mode only.

use common::task::TaskPriority;
use leptos::prelude::*;

/// Standard task checkbox classes (5×5, amber hover) — pair with an amber
/// fill + white `check` glyph when done.
pub const CHECKBOX_CLASS: &str = "flex-shrink-0 mt-0.5 w-5 h-5 rounded border-2 \
     border-stone-300 dark:border-stone-600 flex items-center \
     justify-center hover:border-amber-500 transition-colors cursor-pointer";

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
