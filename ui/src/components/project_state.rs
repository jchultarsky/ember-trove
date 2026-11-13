//! A Project's declared state in the UI (S6a, #320; ONTOLOGY §4, §4.1):
//! active, parked, complete or cancelled.
//!
//! - [`ProjectStateChip`]: the control in a project's page header.
//! - [`ProjectStateBadge`]: the read-only pill (rows, cards).
//! - [`declare`]: the one write, shared with the dashboard's Park and
//!   Make active buttons.
//!
//! Active is the unmarked case, as Open is for a task: nothing says
//! "Active" in words (the status label beside it often does already), and
//! the chip is then just its icon. The rules live in
//! `common::project_state`.

use chrono::{DateTime, Utc};
use common::id::NodeId;
use common::node::Node;
use common::project_state::ProjectState;
use leptos::prelude::*;

use crate::components::task_common::WAITING_TONE;
use crate::components::toast::{ToastLevel, push_toast};

/// The Material Symbols glyph of a state.
#[must_use]
pub fn project_state_icon(state: ProjectState) -> &'static str {
    match state {
        ProjectState::Active => "play_circle",
        ProjectState::Parked => "pause_circle",
        ProjectState::Complete => "check_circle",
        ProjectState::Cancelled => "block",
    }
}

/// Pill colours. Cool for parked (the tone of a Waiting task: it asks
/// nothing of its person for now), muted stone for the rest. Warm colour
/// is kept for urgency, and a state is information.
fn tone(state: ProjectState) -> &'static str {
    match state {
        ProjectState::Parked => WAITING_TONE,
        ProjectState::Active | ProjectState::Complete | ProjectState::Cancelled => {
            "bg-stone-100 dark:bg-stone-800 text-stone-600 dark:text-stone-300"
        }
    }
}

/// "Parked since Nov 13": the state and, when a person declared it, the
/// day they did, on the reader's own calendar. A state nobody has
/// declared (read from the label once, by the migration) has no day.
#[must_use]
pub fn state_text(state: ProjectState, at: Option<DateTime<Utc>>) -> String {
    state_text_on(
        state,
        at.map(|at| crate::components::format_helpers::local_date(&at)),
    )
}

/// [`state_text`] for a day already on the reader's calendar.
fn state_text_on(state: ProjectState, day: Option<chrono::NaiveDate>) -> String {
    match day {
        Some(day) => format!("{} since {}", state.label(), day.format("%b %-d")),
        None => state.label().to_string(),
    }
}

const PILL: &str = "inline-flex items-center gap-1 px-2 py-0.5 rounded-full text-xs font-medium \
     whitespace-nowrap";

/// The state as a read-only pill: a parked, complete or cancelled
/// project says so. An active one shows nothing (see the module note).
#[component]
pub fn ProjectStateBadge(
    state: ProjectState,
    /// When it was declared, for "since". `None` when nobody has, and for
    /// rows that have no room for a date.
    at: Option<DateTime<Utc>>,
) -> impl IntoView {
    (state != ProjectState::Active).then(|| {
        let text = state_text(state, at);
        view! {
            <span class=format!("{PILL} {}", tone(state))
                title=state.description()
                data-testid="project-state-badge"
                data-state=state.as_str()>
                <span class="material-symbols-outlined" style="font-size: 14px;" aria-hidden="true">
                    {project_state_icon(state)}
                </span>
                {text}
            </span>
        }
    })
}

/// Declare `next` for `node_id` and hand the saved node to `on_saved`.
/// A failure is said in a toast and handed to `on_failed`, so the caller
/// can put back what it showed. Both run after an `.await`: they must use
/// `try_` access on their signals.
pub fn declare(
    node_id: NodeId,
    next: ProjectState,
    on_saved: impl FnOnce(Node) + 'static,
    on_failed: impl FnOnce() + 'static,
) {
    wasm_bindgen_futures::spawn_local(async move {
        match crate::api::declare_project_state(node_id, next).await {
            Ok(node) => on_saved(node),
            Err(e) => {
                on_failed();
                push_toast(
                    ToastLevel::Error,
                    format!("Couldn\u{2019}t save the state: {e}"),
                );
            }
        }
    });
}

/// The project header's state control. It shows the state (an icon alone
/// while active) and, for those who may change it, opens the four states
/// with what each one does. The choice is saved at once and put back if
/// the save fails.
///
/// `state` and `at` belong to the page (`NodeView`), not to this chip:
/// the Focus control and the task list follow the same signal, so parking
/// a project takes its Focus toggle away without a reload.
#[component]
pub fn ProjectStateChip(
    node_id: NodeId,
    state: RwSignal<Option<ProjectState>>,
    at: RwSignal<Option<DateTime<Utc>>>,
    editable: bool,
) -> impl IntoView {
    let open = RwSignal::new(false);
    let busy = RwSignal::new(false);

    let choose = move |next: ProjectState| {
        open.set(false);
        let (prev, prev_at) = (state.get_untracked(), at.get_untracked());
        // Already declared as this: nothing to say again.
        if prev == Some(next) && prev_at.is_some() {
            return;
        }
        state.set(Some(next));
        busy.set(true);
        declare(
            node_id,
            next,
            move |node| {
                state.try_set(node.project_state);
                at.try_set(node.project_state_at);
                busy.try_set(false);
            },
            move || {
                state.try_set(prev);
                at.try_set(prev_at);
                busy.try_set(false);
            },
        );
    };

    view! {
        <div class="relative" data-testid="project-state">
            {move || {
                let current = state.get()?;
                if !editable {
                    return Some(view! { <ProjectStateBadge state=current at=at.get() /> }.into_any());
                }
                let text = state_text(current, at.get());
                let name = format!("State: {text}. Change state");
                // Active is the unmarked case: the icon alone.
                let words = (current != ProjectState::Active).then(|| text.clone());
                let shape = if words.is_some() {
                    format!("{PILL} {} border border-transparent", tone(current))
                } else {
                    "inline-flex items-center p-1 rounded-full text-stone-400 \
                     dark:text-stone-500"
                        .to_string()
                };
                Some(view! {
                    <button
                        type="button"
                        class=format!("{shape} cursor-pointer hover:text-stone-700 \
                                       dark:hover:text-stone-200 disabled:opacity-60")
                        title=name.clone()
                        aria-label=name
                        aria-haspopup="true"
                        aria-expanded=move || if open.get() { "true" } else { "false" }
                        data-testid="project-state-chip"
                        data-state=current.as_str()
                        disabled=move || busy.get()
                        on:click=move |_| open.update(|v| *v = !*v)
                    >
                        <span class="material-symbols-outlined" style="font-size: 18px;" aria-hidden="true">
                            {project_state_icon(current)}
                        </span>
                        {words}
                    </button>
                }.into_any())
            }}
            {move || open.get().then(|| {
                let current = state.get_untracked();
                view! {
                    <div class="fixed inset-0 z-20" on:click=move |_| open.set(false) />
                    <div class="absolute left-0 top-full mt-1 z-30 w-72 max-w-[calc(100vw-2rem)] \
                                rounded-xl border border-stone-200 dark:border-stone-700 \
                                bg-white dark:bg-stone-900 shadow-lg p-1"
                        role="radiogroup"
                        aria-label="Project state"
                        data-testid="project-state-options"
                        on:keydown=move |ev: leptos::ev::KeyboardEvent| {
                            if ev.key() == "Escape" {
                                // Ours alone: the page's own Escape must
                                // not also run.
                                ev.stop_propagation();
                                open.set(false);
                            }
                        }>
                        {ProjectState::ALL.iter().copied().map(|option| {
                            let chosen = current == Some(option);
                            view! {
                                <button
                                    type="button"
                                    role="radio"
                                    aria-checked=if chosen { "true" } else { "false" }
                                    class="w-full flex items-start gap-2 px-2 py-1.5 rounded-lg text-left \
                                           hover:bg-stone-100 dark:hover:bg-stone-800 cursor-pointer"
                                    data-testid=format!("project-state-option-{}", option.as_str())
                                    on:click=move |_| choose(option)
                                >
                                    <span class="material-symbols-outlined mt-0.5 text-stone-500 \
                                                 dark:text-stone-400"
                                        style="font-size: 18px;" aria-hidden="true">
                                        {project_state_icon(option)}
                                    </span>
                                    <span class="min-w-0">
                                        <span class="block text-sm font-medium text-stone-800 \
                                                     dark:text-stone-100">
                                            {option.label()}
                                            {chosen.then_some(" (now)")}
                                        </span>
                                        <span class="block text-xs text-stone-500 dark:text-stone-400">
                                            {option.description()}
                                        </span>
                                    </span>
                                </button>
                            }
                        }).collect_view()}
                        <p class="px-2 py-1.5 text-xs text-stone-400 dark:text-stone-500 \
                                  border-t border-stone-100 dark:border-stone-800 mt-1">
                            "Complete and Cancelled change the project only. Its tasks and \
                             sub-projects stay as they are."
                        </p>
                    </div>
                }
            })}
        </div>
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_state_has_a_glyph_in_the_font() {
        // A name the subset lacks renders as its raw text ("pause_circle").
        let names = include_str!("../../icon-names.txt");
        for state in ProjectState::ALL {
            let icon = project_state_icon(*state);
            assert!(
                names.lines().any(|n| n == icon),
                "{icon} is not in ui/icon-names.txt: run scripts/refresh-icon-font.sh"
            );
        }
    }

    #[test]
    fn the_text_names_the_day_only_when_one_was_declared() {
        assert_eq!(state_text_on(ProjectState::Parked, None), "Parked");
        let day = chrono::NaiveDate::from_ymd_opt(2026, 11, 3).expect("a date");
        assert_eq!(
            state_text_on(ProjectState::Parked, Some(day)),
            "Parked since Nov 3"
        );
        assert_eq!(
            state_text_on(ProjectState::Complete, Some(day)),
            "Complete since Nov 3"
        );
    }
}
