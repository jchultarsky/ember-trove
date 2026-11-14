//! The confirmation before a cascade (S6b, #326; ONTOLOGY §4.1,
//! "confirmation first").
//!
//! Declaring a Project complete or cancelled closes its sub-Projects and
//! cancels the tasks still to do under it. Before that is written, this
//! dialog shows the plan the server made: what closes, whose tasks are
//! among it, and what stays open because the reader may not edit it.
//! Confirming sends the plan's token back; if what is under the project
//! changed meanwhile, the server refuses (412) and the dialog shows the
//! new plan instead of closing something its reader never saw.
//!
//! One dialog for the whole app, mounted in `Layout`. Anything that
//! closes a project opens it with [`ask`]: the state chip, Data health's
//! fixes, and the "Close them" notice on a closed project's page.

use std::sync::Arc;

use common::cascade::CascadePlan;
use common::id::NodeId;
use common::node::Node;
use common::project_state::ProjectState;
use leptos::html;
use leptos::prelude::*;

use super::ModalShell;
use crate::components::project_state::project_state_icon;
use crate::error::UiError;

/// Why a question ended with nothing closed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NotClosed {
    /// The person went back, or left the page the question was asked on.
    WentBack,
    /// There was nothing to close any more (closed elsewhere meanwhile),
    /// or the project is no longer in the state the offer was made for.
    /// What made the offer is out of date and should read again.
    NothingLeft,
    /// The plan could not be loaded, or there was no dialog to ask with.
    /// Already said in a toast where there is something to say.
    Failed,
}

/// One question for the dialog.
#[derive(Clone)]
pub struct CascadeRequest {
    pub node_id: NodeId,
    /// The terminal state being declared.
    pub state: ProjectState,
    pub plan: CascadePlan,
    /// The page the question was asked on (`current_path` at the click).
    /// It is withdrawn when that page is left.
    pub asked_on: String,
    /// Run with the saved node once the cascade is written.
    pub on_saved: Arc<dyn Fn(Node) + Send + Sync>,
    /// Run when the question ends with nothing closed.
    pub on_back: Arc<dyn Fn(NotClosed) + Send + Sync>,
}

/// The app's one confirmation, in context for `Layout`: the question on
/// screen, and whether its answer is being saved.
#[derive(Clone, Copy)]
pub struct CascadeConfirm {
    request: RwSignal<Option<CascadeRequest>>,
    busy: RwSignal<bool>,
}

thread_local! {
    /// The app's one [`CascadeConfirm`]. A question is asked after an
    /// `.await` (the plan is fetched first), where there is no reactive
    /// owner and so no context: the same reason the toasts keep a handle.
    static GLOBAL: std::cell::Cell<Option<CascadeConfirm>> =
        const { std::cell::Cell::new(None) };
}

/// The page's path now. Read at the click that asks, and again whenever
/// the answer could land on another page.
#[must_use]
pub fn current_path() -> String {
    web_sys::window()
        .and_then(|w| w.location().pathname().ok())
        .unwrap_or_default()
}

impl CascadeConfirm {
    /// Made once, at the app root.
    pub fn new() -> Self {
        let state = Self {
            request: RwSignal::new(None),
            busy: RwSignal::new(false),
        };
        GLOBAL.with(|g| g.set(Some(state)));
        state
    }

    /// Whether a question is on screen. The window's key handler asks: an
    /// open confirmation owns the keyboard wherever the focus is.
    #[must_use]
    pub fn is_open(self) -> bool {
        self.request.try_with_untracked(Option::is_some) == Some(true)
    }

    /// Take the question off the screen with nothing closed, and tell its
    /// asker. Not while its answer is being saved.
    pub fn go_back(self) {
        if self.busy.try_get_untracked() != Some(false) {
            return;
        }
        self.withdraw();
    }

    /// [`Self::go_back`] without the check: also for a save that came
    /// back refused after the page was left.
    fn withdraw(self) {
        let asked = self.request.try_update(Option::take).flatten();
        if let Some(asked) = asked {
            let why = if asked.plan.is_catch_up() && !asked.plan.closes_something() {
                NotClosed::NothingLeft
            } else {
                NotClosed::WentBack
            };
            (asked.on_back)(why);
        }
    }
}

/// Open the dialog with `request`. Returns whether it was opened. It is
/// not when another question is already on screen (two saves answered at
/// once: that one stays), when the page it was asked on has been left
/// meanwhile, or when there is no dialog. The caller then treats the
/// question as unanswered.
pub fn ask(request: CascadeRequest) -> bool {
    let Some(confirm) = GLOBAL.with(std::cell::Cell::get) else {
        return false;
    };
    if confirm.request.try_with_untracked(Option::is_some) != Some(false)
        || request.asked_on != current_path()
    {
        return false;
    }
    confirm.busy.try_set(false);
    confirm.request.try_set(Some(request)).is_none()
}

/// The dialog's heading.
fn heading(state: ProjectState, catch_up: bool, title: &str) -> String {
    let named = if title.is_empty() {
        "this project".to_string()
    } else {
        format!("\u{201c}{title}\u{201d}")
    };
    match (catch_up, state) {
        (true, _) => format!("Close what is still open under {named}?"),
        (false, ProjectState::Cancelled) => format!("Cancel {named}?"),
        (false, _) => format!("Complete {named}?"),
    }
}

/// The words on the button that closes, or none when there is nothing
/// for it to do: a catch-up with nothing left to close.
fn commit_label(state: ProjectState, catch_up: bool, nothing: bool) -> Option<&'static str> {
    match (catch_up, nothing, state) {
        (true, true, _) => None,
        (true, false, _) => Some("Close these"),
        (false, true, ProjectState::Cancelled) => Some("Cancel the project"),
        (false, true, _) => Some("Complete the project"),
        (false, false, ProjectState::Cancelled) => Some("Cancel and close these"),
        (false, false, _) => Some("Complete and close these"),
    }
}

/// How many of a list are shown before "+n more".
const SHOWN_PROJECTS: usize = 8;
const SHOWN_TASKS_PER_PROJECT: usize = 5;

/// The first `cap` of `items`, and how many are left out.
fn first<T>(items: &[T], cap: usize) -> (&[T], usize) {
    let shown = items.len().min(cap);
    (&items[..shown], items.len() - shown)
}

/// "Tasks that are cancelled (5, 2 of them other people's)".
fn tasks_line(plan: &CascadePlan) -> String {
    let (all, others) = (plan.task_count(), plan.others_task_count());
    match others {
        0 => format!("Tasks that are cancelled ({all})"),
        n if n == all && all == 1 => {
            "Tasks that are cancelled (1, someone else\u{2019}s)".to_string()
        }
        n if n == all => format!("Tasks that are cancelled ({all}, all other people\u{2019}s)"),
        n => format!("Tasks that are cancelled ({all}, {n} of them other people\u{2019}s)"),
    }
}

const LIST: &str = "list-disc pl-5 text-stone-700 dark:text-stone-300";
const MORE: &str = "list-none text-stone-500 dark:text-stone-400";

#[component]
pub fn CascadeConfirmModal() -> impl IntoView {
    let dialog = expect_context::<CascadeConfirm>();
    let CascadeConfirm { request, busy } = dialog;
    let show = Signal::derive(move || request.with(Option::is_some));
    // The plan was replaced by a newer one after the server refused.
    let changed = RwSignal::new(false);
    let error = RwSignal::new(None::<String>);
    let back_ref: NodeRef<html::Button> = NodeRef::new();

    // A new question starts clean, and the safe button has the keyboard.
    Effect::new(move |was_open: Option<bool>| {
        let open = show.get();
        if open && was_open != Some(true) {
            busy.set(false);
            changed.set(false);
            error.set(None);
        }
        open
    });
    Effect::new(move |_| {
        if show.get()
            && let Some(el) = back_ref.get()
        {
            let _ = el.focus();
        }
    });

    let go_back = move || dialog.go_back();

    // The question belongs to the page it was asked on. If that page is
    // left underneath it (the browser's Back), it is withdrawn: nothing
    // is closed from a dialog whose project is no longer on screen. While
    // an answer is being saved it stays; the save's own branches below
    // withdraw it if it comes back refused on another page.
    let path = leptos_router::hooks::use_location().pathname;
    Effect::new(move |seen: Option<String>| {
        let now = path.get();
        if seen.as_ref().is_some_and(|before| *before != now) {
            go_back();
        }
        now
    });

    let confirm = move || {
        if busy.get_untracked() {
            return;
        }
        let Some(asked) = request.get_untracked() else {
            return;
        };
        busy.set(true);
        error.set(None);
        wasm_bindgen_futures::spawn_local(async move {
            let saved = crate::api::declare_project_state(
                asked.node_id,
                asked.state,
                Some(&asked.plan.token),
            )
            .await;
            match saved {
                Ok(node) => {
                    request.try_set(None);
                    busy.try_set(false);
                    (asked.on_saved)(node);
                }
                // What is under the project is no longer what was shown:
                // show it as it is now, and ask again.
                Err(UiError::Api { status: 412, .. }) => {
                    match crate::api::cascade_plan(asked.node_id, asked.state).await {
                        Ok(plan) => {
                            request.try_update(|r| {
                                if let Some(r) = r {
                                    r.plan = plan;
                                }
                            });
                            changed.try_set(true);
                        }
                        Err(e) => {
                            error.try_set(Some(format!(
                                "What is under this project changed, and it couldn\u{2019}t be \
                                 loaded again: {e}"
                            )));
                        }
                    }
                    busy.try_set(false);
                    // Refused, and the page was left meanwhile: there is
                    // no one here to ask again.
                    if current_path() != asked.asked_on {
                        dialog.withdraw();
                    }
                }
                Err(e) => {
                    error.try_set(Some(format!("Couldn\u{2019}t save: {e}")));
                    busy.try_set(false);
                    if current_path() != asked.asked_on {
                        dialog.withdraw();
                    }
                }
            }
        });
    };

    view! {
        <ModalShell
            show=show
            on_close=Callback::new(move |()| go_back())
            label="Confirm closing a project"
            panel_class="w-full max-w-md p-6 flex flex-col gap-4 max-h-[85vh]"
            alert=true
        >
            {move || request.get().map(|asked| {
                let plan = asked.plan.clone();
                let closing = asked.state.label().to_lowercase();
                let (projects, more_projects) = first(&plan.projects, SHOWN_PROJECTS);
                let projects = projects.to_vec();
                let nothing = !plan.closes_something();
                let catch_up = plan.is_catch_up();
                let commit = commit_label(asked.state, catch_up, nothing);
                view! {
                    <div class="flex items-center gap-3">
                        <div class="flex-shrink-0 w-10 h-10 rounded-full bg-stone-100 dark:bg-stone-800
                                    flex items-center justify-center">
                            <span class="material-symbols-outlined text-stone-600 dark:text-stone-300"
                                  style="font-size: 20px;" aria-hidden="true">
                                {project_state_icon(asked.state)}
                            </span>
                        </div>
                        <h2 class="text-base font-semibold text-stone-900 dark:text-stone-100 min-w-0 break-words"
                            data-testid="cascade-heading">
                            {heading(asked.state, catch_up, &plan.master_title)}
                        </h2>
                    </div>

                    {move || changed.get().then(|| view! {
                        <p class="text-sm text-stone-700 dark:text-stone-200 rounded-lg
                                  bg-stone-100 dark:bg-stone-800 px-3 py-2"
                           role="status" data-testid="cascade-changed">
                            "What is under this project changed while this was open. \
                             This is how it is now."
                        </p>
                    })}

                    // `break-words`: a title with no spaces (a URL) wraps
                    // instead of scrolling the list sideways.
                    <div class="flex flex-col gap-3 text-sm text-stone-600 dark:text-stone-400
                                overflow-y-auto min-h-0 break-words"
                         data-testid="cascade-plan">
                        {nothing.then(|| view! {
                            <p data-testid="cascade-nothing">
                                "Nothing under this project is left to close."
                            </p>
                        })}
                        {(!plan.projects.is_empty()).then(|| view! {
                            <div data-testid="cascade-projects">
                                <p>{format!(
                                    "Sub-projects that become {closing} ({})",
                                    plan.projects.len()
                                )}</p>
                                <ul class=LIST>
                                    {projects.iter().map(|p| {
                                        let was = (p.from != ProjectState::Active)
                                            .then(|| format!(" ({})", p.from.label().to_lowercase()));
                                        view! { <li>{p.title.clone()}{was}</li> }
                                    }).collect_view()}
                                    {(more_projects > 0).then(|| view! {
                                        <li class=MORE>{format!("+{more_projects} more")}</li>
                                    })}
                                </ul>
                            </div>
                        })}
                        {(!plan.tasks.is_empty()).then(|| view! {
                            <div data-testid="cascade-tasks">
                                <p>{tasks_line(&plan)}</p>
                                {plan.tasks.iter().map(|group| {
                                    let (tasks, more) = first(&group.tasks, SHOWN_TASKS_PER_PROJECT);
                                    view! {
                                        <p class="mt-1 font-medium text-stone-700 dark:text-stone-300">
                                            {format!("On {}", group.node_title)}
                                        </p>
                                        <ul class=LIST>
                                            {tasks.iter().map(|t| view! {
                                                <li>{t.title.clone()}</li>
                                            }).collect_view()}
                                            {(more > 0).then(|| view! {
                                                <li class=MORE>{format!("+{more} more")}</li>
                                            })}
                                        </ul>
                                    }
                                }).collect_view()}
                            </div>
                        })}
                        {(!plan.skipped.is_empty()).then(|| view! {
                            <div data-testid="cascade-skipped">
                                <p>"Stays open, with what is under it, because you can\u{2019}t edit it:"</p>
                                <ul class=LIST>
                                    {plan.skipped.iter().map(|s| view! {
                                        <li>{s.title.clone()}</li>
                                    }).collect_view()}
                                </ul>
                            </div>
                        })}
                        {(!plan.own_grant).then(|| view! {
                            <p data-testid="cascade-no-grant">
                                "You have no share of your own on this project, so only what \
                                 you do have one on is closed."
                            </p>
                        })}
                        <p class="text-xs text-stone-500 dark:text-stone-400">
                            "Nothing is deleted. A cancelled task gets no completion date. \
                             Making the project active again later does not re-open these."
                        </p>
                    </div>

                    {move || error.get().map(|e| view! {
                        <p class="text-sm text-red-600 dark:text-red-400" role="alert"
                           data-testid="cascade-error">{e}</p>
                    })}

                    <div class="flex flex-wrap justify-end gap-2 pt-1">
                        <button
                            type="button"
                            node_ref=back_ref
                            class="px-4 py-2 text-sm rounded-lg
                                   text-stone-600 dark:text-stone-400
                                   hover:bg-stone-100 dark:hover:bg-stone-800
                                   transition-colors"
                            data-testid="cascade-back"
                            on:click=move |_| go_back()
                        >
                            {if commit.is_some() { "Go back" } else { "Close" }}
                        </button>
                        // `aria-disabled`, not `disabled`, while it saves:
                        // a disabled button drops the focus to the page,
                        // and the dialog's keys go with it.
                        {commit.map(|label| view! {
                            <button
                                type="button"
                                class="inline-flex items-center justify-center gap-1.5 px-4 py-2 text-sm
                                       font-medium rounded-lg bg-ember hover:bg-ember-strong text-white
                                       transition-colors
                                       aria-disabled:opacity-50 aria-disabled:cursor-not-allowed"
                                aria-disabled=move || if busy.get() { "true" } else { "false" }
                                data-testid="cascade-confirm"
                                on:click=move |_| confirm()
                            >
                                <span class="material-symbols-outlined" style="font-size: 16px;" aria-hidden="true">
                                    {project_state_icon(asked.state)}
                                </span>
                                {label}
                            </button>
                        })}
                    </div>
                }
            })}
        </ModalShell>
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use common::cascade::{CascadeTask, CascadeTasks};
    use common::id::TaskId;

    fn plan_with(tasks: &[bool]) -> CascadePlan {
        CascadePlan {
            master_title: "m".into(),
            master_state: Some(ProjectState::Active),
            state: ProjectState::Complete,
            own_grant: true,
            projects: Vec::new(),
            tasks: vec![CascadeTasks {
                node_id: NodeId(uuid::Uuid::nil()),
                node_title: "m".into(),
                tasks: tasks
                    .iter()
                    .map(|others| CascadeTask {
                        id: TaskId(uuid::Uuid::nil()),
                        title: "t".into(),
                        others: *others,
                    })
                    .collect(),
            }],
            skipped: Vec::new(),
            token: String::new(),
        }
    }

    #[test]
    fn the_words_say_what_the_button_does() {
        use ProjectState::{Cancelled, Complete};
        assert_eq!(
            heading(Complete, false, "Kitchen"),
            "Complete \u{201c}Kitchen\u{201d}?"
        );
        assert_eq!(heading(Cancelled, false, ""), "Cancel this project?");
        assert_eq!(
            heading(Complete, true, "Kitchen"),
            "Close what is still open under \u{201c}Kitchen\u{201d}?"
        );
        assert_eq!(
            commit_label(Complete, false, false),
            Some("Complete and close these")
        );
        assert_eq!(
            commit_label(Cancelled, false, false),
            Some("Cancel and close these")
        );
        // A catch-up changes nothing about the project itself.
        assert_eq!(commit_label(Cancelled, true, false), Some("Close these"));
        // Nothing under it any more: the button says only what it still
        // does, and a catch-up has nothing left to do at all.
        assert_eq!(
            commit_label(Complete, false, true),
            Some("Complete the project")
        );
        assert_eq!(
            commit_label(Cancelled, false, true),
            Some("Cancel the project")
        );
        assert_eq!(commit_label(Complete, true, true), None);
    }

    #[test]
    fn other_peoples_tasks_are_counted_in_the_line() {
        assert_eq!(
            tasks_line(&plan_with(&[false, false])),
            "Tasks that are cancelled (2)"
        );
        assert_eq!(
            tasks_line(&plan_with(&[false, true, true])),
            "Tasks that are cancelled (3, 2 of them other people\u{2019}s)"
        );
        assert_eq!(
            tasks_line(&plan_with(&[true, true])),
            "Tasks that are cancelled (2, all other people\u{2019}s)"
        );
        assert_eq!(
            tasks_line(&plan_with(&[true])),
            "Tasks that are cancelled (1, someone else\u{2019}s)"
        );
    }

    #[test]
    fn a_long_list_is_cut_and_says_how_many_more() {
        let items = [1, 2, 3, 4, 5, 6, 7];
        assert_eq!(first(&items, 5), (&items[..5], 2));
        assert_eq!(first(&items, 10), (&items[..], 0));
        assert_eq!(first::<i32>(&[], 5), (&[][..], 0));
    }
}
