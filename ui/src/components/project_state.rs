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

use crate::components::modals::cascade_confirm::{self, CascadeRequest, NotClosed};
use crate::components::task_common::WAITING_TONE;
use crate::components::toast::{ToastLevel, push_toast};
use crate::error::UiError;

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
///
/// Closing a project that has something to close under it is confirmed
/// first (S6b): the server refuses the plain request with 412, the plan
/// is fetched, and the app's confirmation dialog asks. Nothing under the
/// project means no dialog. So does a state the project already has: the
/// server treats that as a no-op whatever is under it (Data health's
/// "Keep complete" relies on it).
///
/// `on_failed` runs when nothing was saved: the save failed (said in a
/// toast) or the person went back from the dialog (said by nobody). The
/// caller puts back what it showed. Both callbacks run after an `.await`
/// and must use `try_` access on their signals.
pub fn declare(
    node_id: NodeId,
    next: ProjectState,
    on_saved: impl Fn(Node) + Send + Sync + 'static,
    on_failed: impl Fn() + Send + Sync + 'static,
) {
    // Where the person is now, read before anything is awaited: an
    // answer that lands after they have left asks nothing there.
    let asked_on = cascade_confirm::current_path();
    wasm_bindgen_futures::spawn_local(async move {
        match crate::api::declare_project_state(node_id, next, None).await {
            Ok(node) => on_saved(node),
            Err(UiError::Api { status: 412, .. }) if next.is_terminal() => {
                ask_with_plan(node_id, next, false, asked_on, on_saved, move |_| {
                    on_failed()
                })
                .await;
            }
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

/// Offer to close what is still open under a project that is already
/// `state` (the catch-up; owner decision, 2026-11-14: offered, never
/// automatic). The same confirmation as closing it.
///
/// It never changes the project's own state: if the project is no longer
/// `state` (re-opened in another tab since this page loaded), nothing is
/// asked and `on_back` is told the offer is out of date. `on_back` runs
/// whenever nothing was closed, with why.
pub fn ask_to_close(
    node_id: NodeId,
    state: ProjectState,
    on_saved: impl Fn(Node) + Send + Sync + 'static,
    on_back: impl Fn(NotClosed) + Send + Sync + 'static,
) {
    let asked_on = cascade_confirm::current_path();
    wasm_bindgen_futures::spawn_local(ask_with_plan(
        node_id, state, true, asked_on, on_saved, on_back,
    ));
}

/// Fetch the plan and open the confirmation with it. `catch_up_only`: the
/// caller offered to close what is under a project it believes is already
/// `state`, and must not end up changing the project itself.
async fn ask_with_plan(
    node_id: NodeId,
    state: ProjectState,
    catch_up_only: bool,
    asked_on: String,
    on_saved: impl Fn(Node) + Send + Sync + 'static,
    on_back: impl Fn(NotClosed) + Send + Sync + 'static,
) {
    let plan = match crate::api::cascade_plan(node_id, state).await {
        Ok(plan) => plan,
        Err(e) => {
            on_back(NotClosed::Failed);
            push_toast(
                ToastLevel::Error,
                format!("Couldn\u{2019}t load what closing this would close: {e}"),
            );
            return;
        }
    };
    if catch_up_only {
        let stale = if !plan.is_catch_up() {
            Some(format!(
                "This project is not {} any more. Nothing was closed.",
                state.label().to_lowercase()
            ))
        } else if !plan.closes_something() {
            Some("Nothing is left to close under it.".to_string())
        } else {
            None
        };
        if let Some(said) = stale {
            on_back(NotClosed::NothingLeft);
            push_toast(ToastLevel::Info, said);
            return;
        }
    }
    let on_back = std::sync::Arc::new(on_back);
    let unanswered = on_back.clone();
    let asked = cascade_confirm::ask(CascadeRequest {
        node_id,
        state,
        plan,
        asked_on,
        on_saved: std::sync::Arc::new(on_saved),
        on_back,
    });
    if !asked {
        // Another question is on screen, or the page was left meanwhile.
        unanswered(NotClosed::WentBack);
    }
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
    /// Run when a choice has settled: the save answered, or it failed and
    /// the old state is back. `state` changes as soon as a choice is made,
    /// before the server has it, so anything the page must read again
    /// from the server (its Focus entry) waits for this.
    #[prop(optional, into)]
    on_settled: Option<Callback<()>>,
) -> impl IntoView {
    let open = RwSignal::new(false);
    let busy = RwSignal::new(false);
    let wrapper: NodeRef<leptos::html::Div> = NodeRef::new();
    let panel: NodeRef<leptos::html::Div> = NodeRef::new();
    let focus_in = move |within: Option<web_sys::Element>, selector: &str| {
        use wasm_bindgen::JsCast;
        if let Some(el) = within
            .and_then(|w| w.query_selector(selector).ok().flatten())
            .and_then(|el| el.dyn_into::<web_sys::HtmlElement>().ok())
        {
            let _ = el.focus();
        }
    };
    // The choices take the keyboard when they open, on the state the
    // project has. Tracking the NodeRef (not a timer) runs this once the
    // panel is in the page.
    Effect::new(move |_| {
        if let Some(el) = panel.get() {
            focus_in(Some(el.into()), "[aria-checked='true']");
        }
    });
    // A radio group's own keys (as `SegmentedRadio` has them): one tab
    // stop, and the arrows, Home and End move between the states. Moving
    // does not choose: a state is declared with Enter or Space, since
    // each choice is saved at once.
    let step = move |ev: leptos::ev::KeyboardEvent| {
        let count = ProjectState::ALL.len();
        let at = ev
            .target()
            .and_then(|t| {
                use wasm_bindgen::JsCast;
                t.dyn_into::<web_sys::Element>().ok()
            })
            .and_then(|el| el.get_attribute("data-index"))
            .and_then(|i| i.parse::<usize>().ok());
        let Some(at) = at else { return };
        let next = match ev.key().as_str() {
            "ArrowDown" | "ArrowRight" => (at + 1) % count,
            "ArrowUp" | "ArrowLeft" => (at + count - 1) % count,
            "Home" => 0,
            "End" => count - 1,
            _ => return,
        };
        ev.prevent_default();
        focus_in(
            panel.get_untracked().map(Into::into),
            &format!("[data-index='{next}']"),
        );
    };
    // Close, and hand the keyboard back to the chip: the choices are
    // about to leave the page, and focus must not be lost with them.
    let close = move || {
        open.set(false);
        focus_in(
            wrapper.get_untracked().map(Into::into),
            "[data-testid='project-state-chip']",
        );
    };

    let choose = move |next: ProjectState| {
        // Back to the chip: the choices are leaving the page.
        close();
        if busy.get_untracked() {
            return;
        }
        let (prev, prev_at) = (state.get_untracked(), at.get_untracked());
        // Already declared as this: nothing to say again.
        if prev == Some(next) && prev_at.is_some() {
            return;
        }
        // Parking and re-opening show at once. Closing waits for the
        // server: it may first ask what to do about what is under the
        // project, and the chip must not say "Complete" while it asks.
        if !next.is_terminal() {
            state.set(Some(next));
            if prev != Some(next) {
                // The old state's date is not this one's: show none until
                // the save says when it was declared.
                at.set(None);
            }
        }
        busy.set(true);
        declare(
            node_id,
            next,
            move |node| {
                state.try_set(node.project_state);
                at.try_set(node.project_state_at);
                busy.try_set(false);
                if let Some(settled) = on_settled {
                    settled.try_run(());
                }
            },
            move || {
                state.try_set(prev);
                at.try_set(prev_at);
                busy.try_set(false);
                if let Some(settled) = on_settled {
                    settled.try_run(());
                }
            },
        );
    };

    // Only a Project has the chip, and a Project always has a state.
    let current = move || state.get().unwrap_or(ProjectState::Active);
    if !editable {
        return view! {
            <div data-testid="project-state">
                {move || state.get().map(|s| view! { <ProjectStateBadge state=s at=at.get() /> })}
            </div>
        }
        .into_any();
    }
    let name = move || format!("State: {}. Change state", state_text(current(), at.get()));

    view! {
        // Escape is handled here, on the element that holds both the chip
        // and its choices, so it works wherever the keyboard is in them,
        // and is stopped: the page's own Escape must not also run. And
        // when the keyboard leaves them for something else (Tab), the
        // choices close: left open, a later Escape would be the page's.
        <div class="relative" data-testid="project-state" node_ref=wrapper
            on:keydown=move |ev: leptos::ev::KeyboardEvent| {
                if ev.key() == "Escape" && open.get_untracked() {
                    ev.stop_propagation();
                    close();
                }
            }
            on:focusout=move |ev: leptos::ev::FocusEvent| {
                use wasm_bindgen::JsCast;
                // Only when focus went to another element. A click on a
                // choice gives it no focus in WebKit (no related target),
                // and closing then would lose the click.
                let Some(to) = ev
                    .related_target()
                    .and_then(|t| t.dyn_into::<web_sys::Node>().ok())
                else {
                    return;
                };
                let inside = wrapper
                    .get_untracked()
                    .is_some_and(|w| w.contains(Some(&to)));
                if !inside {
                    open.set(false);
                }
            }>
            // One button for the life of the page: its words and icon
            // change in place, so it keeps the keyboard through a choice.
            // While a save is in flight it stays focusable and ignores
            // clicks (a disabled button would drop the focus).
            <button
                type="button"
                class=move || {
                    // Active is the unmarked case: the icon alone.
                    let shape = if current() == ProjectState::Active {
                        "inline-flex items-center p-1 rounded-full text-stone-400 \
                         dark:text-stone-500"
                            .to_string()
                    } else {
                        format!("{PILL} {} border border-transparent", tone(current()))
                    };
                    let waiting = if busy.get() { " opacity-60" } else { "" };
                    format!(
                        "{shape} cursor-pointer hover:text-stone-700 \
                         dark:hover:text-stone-200{waiting}"
                    )
                }
                title=name
                aria-label=name
                aria-expanded=move || if open.get() { "true" } else { "false" }
                aria-disabled=move || if busy.get() { "true" } else { "false" }
                data-testid="project-state-chip"
                data-state=move || current().as_str()
                on:click=move |_| {
                    if !busy.get_untracked() {
                        open.update(|v| *v = !*v);
                    }
                }
            >
                <span class="material-symbols-outlined" style="font-size: 18px;" aria-hidden="true">
                    {move || project_state_icon(current())}
                </span>
                {move || (current() != ProjectState::Active).then(|| state_text(current(), at.get()))}
            </button>
            {move || open.get().then(|| {
                let current = state.get_untracked();
                view! {
                    <div class="fixed inset-0 z-20" on:click=move |_| open.set(false) />
                    // Under the chip. At phone width the chip can sit
                    // anywhere along its row, so the choices span the
                    // screen instead of starting at the chip's left edge
                    // (`top-auto` keeps them just below it).
                    <div class="absolute left-0 top-full mt-1 z-30 w-72 \
                                max-sm:fixed max-sm:left-4 max-sm:right-4 max-sm:top-auto max-sm:w-auto \
                                rounded-xl border border-stone-200 dark:border-stone-700 \
                                bg-white dark:bg-stone-900 shadow-lg p-1"
                        role="radiogroup"
                        aria-label="Project state"
                        data-testid="project-state-options"
                        node_ref=panel
                        on:keydown=step>
                        {ProjectState::ALL.iter().copied().enumerate().map(|(index, option)| {
                            let chosen = current == Some(option);
                            view! {
                                <button
                                    type="button"
                                    role="radio"
                                    aria-checked=if chosen { "true" } else { "false" }
                                    // One tab stop: the state it has.
                                    tabindex=if chosen { "0" } else { "-1" }
                                    data-index=index
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
                            "Complete and Cancelled also close its sub-projects and cancel its \
                             open tasks. You are shown them first."
                        </p>
                    </div>
                }
            })}
        </div>
    }
    .into_any()
}

/// The notice on a closed project's page when something is still open
/// under it (S6b): tasks added since, a sub-Project moved in, or a
/// project closed before closing reached below it. It offers the
/// catch-up, which shows the same confirmation as closing does; nothing
/// is closed until that is confirmed (owner decision, 2026-11-14).
///
/// Shown to those the page gives the state chip (its owner, an admin).
/// What it counts is the server's plan for this reader, so it never
/// mentions what they may not edit, and it is gone when the plan closes
/// nothing. A plan that fails to load shows no notice: the page's own
/// content does not depend on it.
#[component]
pub fn StillOpenNotice(
    node_id: NodeId,
    #[prop(into)] state: Signal<Option<ProjectState>>,
    /// Bumped when the tasks or the state may have changed.
    #[prop(into)]
    refresh: Signal<u32>,
    /// Run with the saved node once the catch-up is written.
    on_closed: Callback<Node>,
) -> impl IntoView {
    // Bumped when the offer turned out to be out of date.
    let reread = RwSignal::new(0u32);
    let plan = LocalResource::new(move || {
        let state = state.get();
        refresh.track();
        reread.track();
        async move {
            match state {
                Some(state) if state.is_terminal() => {
                    crate::api::cascade_plan(node_id, state).await.ok()
                }
                _ => None,
            }
        }
    });
    let asking = RwSignal::new(false);
    view! {
        {move || {
            let plan = plan.get().flatten()?;
            let closes = plan.closes_text()?;
            let state = plan.state;
            let ask = move |_| {
                if asking.get_untracked() {
                    return;
                }
                asking.set(true);
                ask_to_close(
                    node_id,
                    state,
                    move |node| {
                        asking.try_set(false);
                        on_closed.try_run(node);
                    },
                    move |why| {
                        asking.try_set(false);
                        if why == NotClosed::NothingLeft {
                            reread.try_update(|n| *n += 1);
                        }
                    },
                );
            };
            Some(view! {
                <div class="mb-4 flex flex-wrap items-center gap-x-3 gap-y-1 rounded-lg border \
                            border-stone-200 dark:border-stone-700 bg-stone-50 dark:bg-stone-800/40 \
                            px-3 py-2 text-sm text-stone-700 dark:text-stone-300"
                    role="status"
                    data-testid="still-open-notice">
                    <span class="material-symbols-outlined text-stone-500 dark:text-stone-400"
                        style="font-size: 18px;" aria-hidden="true">"info"</span>
                    <span class="min-w-[12rem] flex-1">
                        {format!(
                            "This project is {}, and {closes} under it {} still open.",
                            state.label().to_lowercase(),
                            if closes.starts_with("1 ") && !closes.contains(" and ") { "is" } else { "are" },
                        )}
                    </span>
                    <button type="button"
                        class="inline-flex items-center gap-1 rounded-md border border-stone-300 \
                               dark:border-stone-600 px-2 py-0.5 text-xs font-medium \
                               hover:bg-stone-100 dark:hover:bg-stone-800 aria-disabled:opacity-40"
                        // Not `disabled`: the dialog gives the keyboard
                        // back to this button when it closes.
                        aria-disabled=move || if asking.get() { "true" } else { "false" }
                        data-testid="still-open-close"
                        on:click=ask>
                        <span class="material-symbols-outlined" style="font-size: 14px;" aria-hidden="true">
                            {project_state_icon(state)}
                        </span>
                        "Close them\u{2026}"
                    </button>
                </div>
            })
        }}
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
