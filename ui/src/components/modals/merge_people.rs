//! Merging two saved People into one (S8c, #363; ONTOLOGY §7.1).
//!
//! The same email address is the same person, and the registry holds each
//! person once. Where it holds one twice (no address, an address that
//! changed, a pair from before the rule), this dialog joins the two: one
//! is kept, the other's details, memberships and mentions move to them,
//! and the other is deleted.
//!
//! The dialog shows the plan the server made before anything is written.
//! Confirming sends the plan's token back; if either person changed
//! meanwhile the server refuses (412) and the dialog shows the new plan
//! instead of doing something its reader never saw. There is no undo.
//!
//! One dialog for the whole app, mounted in `Layout`, opened with
//! [`ask`]: from a person's page, from the notice that refuses an address
//! another saved person has, and from Data health.

use std::sync::Arc;

use common::form_draft::PersonForm;
use common::id::PersonId;
use common::person::Person;
use common::person_merge::{MergePlan, plan_lines};
use gloo_timers::callback::Timeout;
use leptos::html;
use leptos::prelude::*;

use super::ModalShell;
use super::cascade_confirm::current_path;
use crate::components::toast::{ToastLevel, push_toast};
use crate::error::UiError;

/// One question for the dialog.
#[derive(Clone)]
pub struct MergeAsk {
    /// The person kept, to start with. The dialog can swap the two.
    pub kept: PersonId,
    /// The other one, when the asker knows who. `None`: the dialog asks.
    pub other: Option<PersonId>,
    /// Who the other can be chosen from, by name (Data health: the people
    /// who share the address). Empty: the registry is searched.
    pub among: Vec<(PersonId, String)>,
    /// The page the question was asked on (`current_path` at the click).
    /// It is withdrawn when that page is left.
    pub asked_on: String,
    /// Run with the kept person, as saved, once the merge is written.
    pub on_merged: Arc<dyn Fn(Person) + Send + Sync>,
    /// Run when the question ends with nothing merged.
    pub on_back: Arc<dyn Fn() + Send + Sync>,
}

/// The app's one merge dialog, in context for `Layout`: the question on
/// screen, and whether its answer is being saved.
#[derive(Clone, Copy)]
pub struct MergePeople {
    request: RwSignal<Option<MergeAsk>>,
    busy: RwSignal<bool>,
}

thread_local! {
    /// The app's one [`MergePeople`]: a question can be asked where there
    /// is no reactive owner (after an `.await`), as for the cascade's.
    static GLOBAL: std::cell::Cell<Option<MergePeople>> =
        const { std::cell::Cell::new(None) };
}

impl MergePeople {
    /// Made once, at the app root.
    pub fn new() -> Self {
        let state = Self {
            request: RwSignal::new(None),
            busy: RwSignal::new(false),
        };
        GLOBAL.with(|g| g.set(Some(state)));
        state
    }

    /// Whether the dialog is on screen. The window's key handler asks: an
    /// open dialog owns the keyboard wherever the focus is.
    #[must_use]
    pub fn is_open(self) -> bool {
        self.request.try_with_untracked(Option::is_some) == Some(true)
    }

    /// Take the dialog off the screen with nothing merged, and tell its
    /// asker. Not while a merge is being saved.
    pub fn go_back(self) {
        if self.busy.try_get_untracked() != Some(false) {
            return;
        }
        self.withdraw();
    }

    fn withdraw(self) {
        if let Some(asked) = self.request.try_update(Option::take).flatten() {
            (asked.on_back)();
        }
    }
}

/// Open the dialog with `request`. Returns whether it was opened: not
/// when one is on screen already, when the page it was asked on has been
/// left, or when there is no dialog.
pub fn ask(request: MergeAsk) -> bool {
    let Some(dialog) = GLOBAL.with(std::cell::Cell::get) else {
        return false;
    };
    if dialog.request.try_with_untracked(Option::is_some) != Some(false)
        || request.asked_on != current_path()
    {
        return false;
    }
    dialog.busy.try_set(false);
    dialog.request.try_set(Some(request)).is_none()
}

/// What tells two people of the same name apart: position, company and
/// the first address, as far as they have them.
fn told_apart_by(person: &Person) -> String {
    [
        person.position.clone(),
        person.company.clone(),
        person.emails.first().map(|e| e.value.clone()),
        person.phones.first().map(|p| p.value.clone()),
    ]
    .into_iter()
    .flatten()
    .map(|s| s.trim().to_string())
    .filter(|s| !s.is_empty())
    .take(3)
    .collect::<Vec<_>>()
    .join(" \u{b7} ")
}

/// A failed merge, in words for the person. The API's own sentence where
/// it gave one; a missing person is said here, because the API's words
/// for it name neither of the two.
fn said(e: &UiError) -> String {
    match e {
        UiError::Api { status: 404, .. } => "One of these two is no longer in your registry \
             (deleted, or merged already). Go back and look again."
            .to_string(),
        e => e
            .explained()
            .unwrap_or_else(|| format!("Couldn\u{2019}t merge: {e}")),
    }
}

/// How many people the search lists.
const LISTED: usize = 20;

const LIST: &str = "list-disc pl-5 text-stone-700 dark:text-stone-300 space-y-1";
const CHOICE: &str = "w-full text-left rounded-lg px-3 py-2 text-sm cursor-pointer \
     hover:bg-stone-100 dark:hover:bg-stone-800 text-stone-800 dark:text-stone-200";
const QUIET: &str = "text-xs underline cursor-pointer text-stone-600 dark:text-stone-300";

#[component]
pub fn MergePeopleModal() -> impl IntoView {
    let dialog = expect_context::<MergePeople>();
    let MergePeople { request, busy } = dialog;
    let show = Signal::derive(move || request.with(Option::is_some));
    // Whose drafts to drop after a merge. Read here: there is no context
    // after an `.await`.
    let subject = use_context::<crate::auth::AuthState>().and_then(|a| match a.get_untracked() {
        crate::auth::AuthStatus::Authenticated(u) => Some(u.sub),
        _ => None,
    });
    let subject = StoredValue::new(subject);

    // The two people, as the dialog has them now: (kept, the other).
    let pair = RwSignal::new(None::<(PersonId, Option<PersonId>)>);
    let plan = RwSignal::new(None::<Result<MergePlan, String>>);
    // The two as the registry has them, to tell them apart on screen.
    let cards = RwSignal::new(Vec::<Person>::new());
    // The plan was replaced by a newer one after the server refused.
    let changed = RwSignal::new(false);
    let error = RwSignal::new(None::<String>);
    // Only the latest read is shown.
    let read_v = StoredValue::new(0u32);
    let back_ref: NodeRef<html::Button> = NodeRef::new();

    // Read the plan for the pair as it is now, and the two people.
    let load = move || {
        let Some((kept, Some(other))) = pair.try_get_untracked().flatten() else {
            return;
        };
        let v = read_v.try_get_value().unwrap_or(0).wrapping_add(1);
        read_v.try_set_value(v);
        plan.try_set(None);
        wasm_bindgen_futures::spawn_local(async move {
            let answered = crate::api::merge_preview(kept, other).await;
            let mut people = Vec::new();
            if answered.is_ok() {
                for id in [kept, other] {
                    if let Ok(p) = crate::api::fetch_person(id).await {
                        people.push(p);
                    }
                }
            }
            if read_v.try_get_value() != Some(v) {
                return;
            }
            cards.try_set(people);
            plan.try_set(Some(answered.map_err(|e| said(&e))));
        });
    };

    // A new question starts clean.
    Effect::new(move |was_open: Option<bool>| {
        let open = show.get();
        if open && was_open != Some(true) {
            busy.set(false);
            changed.set(false);
            error.set(None);
            cards.set(Vec::new());
            plan.set(None);
            let asked = request.get_untracked();
            pair.set(asked.map(|a| (a.kept, a.other)));
            load();
        }
        open
    });
    // The safe button has the keyboard once there is a plan to read.
    Effect::new(move |_| {
        if show.get()
            && plan.with(Option::is_some)
            && let Some(el) = back_ref.get()
        {
            let _ = el.focus();
        }
    });

    let go_back = move || dialog.go_back();

    // The question belongs to the page it was asked on.
    let path = leptos_router::hooks::use_location().pathname;
    Effect::new(move |seen: Option<String>| {
        let now = path.get();
        if seen.as_ref().is_some_and(|before| *before != now) {
            go_back();
        }
        now
    });

    let choose = move |other: PersonId| {
        if busy.get_untracked() {
            return;
        }
        pair.update(|p| {
            if let Some((_, o)) = p {
                *o = Some(other);
            }
        });
        changed.set(false);
        error.set(None);
        load();
    };
    let choose_again = move || {
        if busy.get_untracked() {
            return;
        }
        pair.update(|p| {
            if let Some((_, o)) = p {
                *o = None;
            }
        });
        plan.set(None);
        error.set(None);
    };
    let swap = move || {
        if busy.get_untracked() {
            return;
        }
        pair.update(|p| {
            if let Some((kept, Some(other))) = p {
                std::mem::swap(kept, other);
            }
        });
        changed.set(false);
        error.set(None);
        load();
    };

    let confirm = move || {
        if busy.get_untracked() {
            return;
        }
        let (Some(asked), Some((kept, Some(other))), Some(Ok(shown))) = (
            request.get_untracked(),
            pair.get_untracked(),
            plan.get_untracked(),
        ) else {
            return;
        };
        if !shown.refusals.is_empty() {
            return;
        }
        busy.set(true);
        error.set(None);
        wasm_bindgen_futures::spawn_local(async move {
            match crate::api::merge_people(kept, other, &shown.token).await {
                Ok(person) => {
                    // What this device kept of either is of a person who
                    // is gone, or of a card that no longer reads so.
                    if let Some(Some(sub)) = subject.try_get_value() {
                        for id in [kept, other] {
                            crate::draft::clear_form_draft(&PersonForm::Edit(id).key(&sub));
                        }
                    }
                    request.try_set(None);
                    busy.try_set(false);
                    push_toast(ToastLevel::Success, "Merged.");
                    (asked.on_merged)(person);
                }
                // One of them is no longer what was shown: show the plan
                // as it is now, and ask again.
                Err(UiError::Api { status: 412, .. }) => {
                    match crate::api::merge_preview(kept, other).await {
                        Ok(now) => {
                            plan.try_set(Some(Ok(now)));
                            changed.try_set(true);
                        }
                        Err(e) => {
                            error.try_set(Some(said(&e)));
                        }
                    }
                    busy.try_set(false);
                    if current_path() != asked.asked_on {
                        dialog.withdraw();
                    }
                }
                Err(e) => {
                    error.try_set(Some(said(&e)));
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
            label="Merge two people"
            panel_class="w-full max-w-lg p-6 flex flex-col gap-4 max-h-[85vh]"
            alert=true
        >
            <div class="flex items-center gap-3">
                <div class="flex-shrink-0 w-10 h-10 rounded-full bg-stone-100 dark:bg-stone-800
                            flex items-center justify-center">
                    <span class="material-symbols-outlined text-stone-600 dark:text-stone-300"
                          style="font-size: 20px;" aria-hidden="true">"merge"</span>
                </div>
                <h2 class="text-base font-semibold text-stone-900 dark:text-stone-100"
                    data-testid="merge-heading">
                    "Merge two people into one"
                </h2>
            </div>
            {move || match pair.get() {
                None => ().into_any(),
                Some((kept, None)) => {
                    let among = request.with(|r| r.as_ref().map(|a| a.among.clone())).unwrap_or_default();
                    view! { <ChooseOther kept=kept among=among on_choose=Callback::new(choose) /> }.into_any()
                }
                Some((kept, Some(other))) => {
                    let fixed = request.with(|r| r.as_ref().is_some_and(|a| a.other.is_some()));
                    view! {
                        <div class="flex flex-col gap-3 text-sm text-stone-600 dark:text-stone-400
                                    overflow-y-auto min-h-0 break-words"
                             data-testid="merge-plan">
                            {move || match plan.get() {
                                None => view! {
                                    <p role="status">"Working out what the merge would do\u{2026}"</p>
                                }.into_any(),
                                Some(Err(why)) => view! {
                                    <p class="text-red-600 dark:text-red-400" role="alert"
                                       data-testid="merge-load-error">{why}</p>
                                    <button type="button" class=QUIET on:click=move |_| load()>
                                        "Try again"
                                    </button>
                                }.into_any(),
                                Some(Ok(shown)) => {
                                    let about = move |id: PersonId, name: &str| {
                                        let apart = cards.with(|c| {
                                            c.iter().find(|p| p.id == id).map(told_apart_by)
                                        }).unwrap_or_default();
                                        (name.to_string(), apart)
                                    };
                                    let (kept_name, kept_apart) = about(kept, &shown.kept_name);
                                    let (gone_name, gone_apart) = about(other, &shown.gone_name);
                                    let lines = plan_lines(&shown);
                                    let refusals: Vec<String> =
                                        shown.refusals.iter().map(|r| r.says()).collect();
                                    view! {
                                        {move || changed.get().then(|| view! {
                                            <p class="text-stone-700 dark:text-stone-200 rounded-lg
                                                      bg-stone-100 dark:bg-stone-800 px-3 py-2"
                                               role="status" data-testid="merge-changed">
                                                "One of them was changed while this was open. \
                                                 This is what the merge would do now."
                                            </p>
                                        })}
                                        <dl class="grid grid-cols-1 gap-2">
                                            <div class="rounded-lg border border-stone-200 dark:border-stone-700 px-3 py-2"
                                                 data-testid="merge-kept">
                                                <dt class="text-xs text-stone-500 dark:text-stone-400">"Kept"</dt>
                                                <dd class="font-medium text-stone-900 dark:text-stone-100">{kept_name}</dd>
                                                {(!kept_apart.is_empty()).then(|| view! {
                                                    <dd class="text-xs">{kept_apart}</dd>
                                                })}
                                            </div>
                                            <div class="rounded-lg border border-stone-200 dark:border-stone-700 px-3 py-2"
                                                 data-testid="merge-gone">
                                                <dt class="text-xs text-stone-500 dark:text-stone-400">
                                                    "The other: merged in, then deleted"
                                                </dt>
                                                <dd class="font-medium text-stone-900 dark:text-stone-100">{gone_name}</dd>
                                                {(!gone_apart.is_empty()).then(|| view! {
                                                    <dd class="text-xs">{gone_apart}</dd>
                                                })}
                                            </div>
                                        </dl>
                                        <div class="flex flex-wrap gap-x-4 gap-y-1">
                                            <button type="button" class=QUIET data-testid="merge-swap"
                                                on:click=move |_| swap()>
                                                "Keep the other one instead"
                                            </button>
                                            {(!fixed).then(|| view! {
                                                <button type="button" class=QUIET data-testid="merge-choose-again"
                                                    on:click=move |_| choose_again()>
                                                    "Choose someone else"
                                                </button>
                                            })}
                                        </div>
                                        <ul class=LIST data-testid="merge-lines">
                                            {lines.into_iter().map(|line| view! { <li>{line}</li> }).collect_view()}
                                        </ul>
                                        {(!refusals.is_empty()).then(|| view! {
                                            <div role="alert" data-testid="merge-refusals"
                                                 class="rounded-lg border border-amber-300 dark:border-amber-700
                                                        bg-amber-50 dark:bg-amber-900/20 px-3 py-2
                                                        text-stone-700 dark:text-stone-200">
                                                <p>"These two can\u{2019}t be merged as they are:"</p>
                                                <ul class="list-disc pl-5">
                                                    {refusals.into_iter().map(|r| view! { <li>{r}</li> }).collect_view()}
                                                </ul>
                                            </div>
                                        })}
                                        <p class="text-xs text-stone-500 dark:text-stone-400">
                                            "The other record is deleted and its ID stands for the kept \
                                             person from then on. This can\u{2019}t be undone; a backup \
                                             made before the merge has both."
                                        </p>
                                    }.into_any()
                                }
                            }}
                        </div>
                    }.into_any()
                }
            }}

            {move || error.get().map(|e| view! {
                <p class="text-sm text-red-600 dark:text-red-400" role="alert"
                   data-testid="merge-error">{e}</p>
            })}

            <div class="flex flex-wrap justify-end gap-2 pt-1">
                <button
                    type="button"
                    node_ref=back_ref
                    class="px-4 py-2 text-sm rounded-lg
                           text-stone-600 dark:text-stone-400
                           hover:bg-stone-100 dark:hover:bg-stone-800
                           transition-colors"
                    data-testid="merge-back"
                    on:click=move |_| go_back()
                >
                    "Go back"
                </button>
                // Only with a plan that can be carried out. `aria-disabled`,
                // not `disabled`, while it saves: a disabled button drops
                // the focus to the page, and the dialog's keys go with it.
                {move || {
                    let ready = pair.get().is_some_and(|(_, other)| other.is_some())
                        && plan.with(|p| matches!(p, Some(Ok(shown)) if shown.refusals.is_empty()));
                    ready.then(|| view! {
                        <button
                            type="button"
                            class="inline-flex items-center justify-center gap-1.5 px-4 py-2 text-sm
                                   font-medium rounded-lg bg-ember hover:bg-ember-strong text-white
                                   transition-colors
                                   aria-disabled:opacity-50 aria-disabled:cursor-not-allowed"
                            aria-disabled=move || if busy.get() { "true" } else { "false" }
                            data-testid="merge-confirm"
                            on:click=move |_| confirm()
                        >
                            <span class="material-symbols-outlined" style="font-size: 16px;" aria-hidden="true">
                                "merge"
                            </span>
                            "Merge"
                        </button>
                    })
                }}
            </div>
        </ModalShell>
    }
}

/// The dialog's first step when its asker did not say who the other is:
/// one of a few named people, or a search of the registry.
#[component]
fn ChooseOther(
    kept: PersonId,
    among: Vec<(PersonId, String)>,
    on_choose: Callback<PersonId>,
) -> impl IntoView {
    if !among.is_empty() {
        // The keyboard goes to the first of them: the button that opened
        // the dialog is behind it.
        let list_ref: NodeRef<html::Div> = NodeRef::new();
        Effect::new(move |_| {
            use leptos::wasm_bindgen::JsCast;
            let first = list_ref
                .get()
                .and_then(|list| list.query_selector("button").ok().flatten())
                .and_then(|el| el.dyn_into::<web_sys::HtmlElement>().ok());
            if let Some(first) = first {
                let _ = first.focus();
            }
        });
        return view! {
            <div class="flex flex-col gap-2 text-sm min-h-0 overflow-y-auto" data-testid="merge-among"
                node_ref=list_ref>
                <p class="text-stone-600 dark:text-stone-400">"Merge with whom?"</p>
                <ul class="flex flex-col gap-1">
                    {among.into_iter().filter(|(id, _)| *id != kept).map(|(id, name)| view! {
                        <li>
                            <button type="button" class=CHOICE data-testid="merge-candidate"
                                on:click=move |_| on_choose.run(id)>
                                {name}
                            </button>
                        </li>
                    }).collect_view()}
                </ul>
            </div>
        }
        .into_any();
    }

    let typed = RwSignal::new(String::new());
    let query = RwSignal::new(String::new());
    let debounce_v = RwSignal::new(0u32);
    Effect::new(move |_| {
        let val = typed.get();
        let v = debounce_v.get_untracked() + 1;
        debounce_v.set(v);
        Timeout::new(300, move || {
            // A forgotten timer can fire after the dialog is gone.
            if debounce_v.try_get_untracked() == Some(v) {
                let _ = query.try_set(val.clone());
            }
        })
        .forget();
    });
    let retry = RwSignal::new(0u32);
    let found = LocalResource::new(move || {
        retry.track();
        let q = query.get();
        // One more than is listed: the person kept is among them.
        async move { crate::api::search_people(&q, LISTED as u32 + 1).await }
    });
    let input_ref: NodeRef<html::Input> = NodeRef::new();
    Effect::new(move |_| {
        if let Some(el) = input_ref.get() {
            let _ = el.focus();
        }
    });

    view! {
        <div class="flex flex-col gap-2 text-sm min-h-0" data-testid="merge-search">
            <label class="text-stone-600 dark:text-stone-400" for="merge-search-input">
                "Merge with whom? Search your people by name, company, email or phone."
            </label>
            <input id="merge-search-input" type="search" node_ref=input_ref autocomplete="off"
                class="w-full text-sm bg-stone-100 dark:bg-stone-800 text-stone-800 dark:text-stone-200
                       rounded-lg px-3 py-2 outline-none focus:ring-2 focus:ring-ember"
                placeholder="Search people\u{2026}"
                prop:value=move || typed.get()
                on:input=move |ev| typed.set(event_target_value(&ev)) />
            <div class="overflow-y-auto min-h-0 max-h-64">
                <Transition fallback=|| view! { <p class="text-stone-400">"Loading\u{2026}"</p> }>
                    {move || found.get().map(|r| match r {
                        Err(_) => view! {
                            <crate::components::load_error::LoadError
                                what="your people"
                                on_retry=Callback::new(move |()| retry.update(|n| *n += 1))
                            />
                        }.into_any(),
                        Ok(people) => {
                            let others: Vec<Person> = people
                                .into_iter()
                                .filter(|p| p.id != kept)
                                .take(LISTED)
                                .collect();
                            if others.is_empty() {
                                view! {
                                    <p class="text-stone-500 dark:text-stone-400" data-testid="merge-nobody">
                                        "No one else matches."
                                    </p>
                                }.into_any()
                            } else {
                                view! {
                                    <ul class="flex flex-col gap-1">
                                        {others.into_iter().map(|p| {
                                            let id = p.id;
                                            let apart = told_apart_by(&p);
                                            view! {
                                                <li>
                                                    <button type="button" class=CHOICE data-testid="merge-candidate"
                                                        on:click=move |_| on_choose.run(id)>
                                                        <span class="font-medium">{p.display_name.clone()}</span>
                                                        {(!apart.is_empty()).then(|| view! {
                                                            <span class="block text-xs text-stone-500 dark:text-stone-400">
                                                                {apart}
                                                            </span>
                                                        })}
                                                    </button>
                                                </li>
                                            }
                                        }).collect_view()}
                                    </ul>
                                }.into_any()
                            }
                        }
                    })}
                </Transition>
            </div>
        </div>
    }
    .into_any()
}

#[cfg(test)]
mod tests {
    use super::*;
    use common::person::ContactPoint;

    fn person(position: Option<&str>, company: Option<&str>, email: Option<&str>) -> Person {
        let now = chrono::Utc::now();
        Person {
            id: PersonId(uuid::Uuid::nil()),
            owner_id: "me".into(),
            display_name: "Ada".into(),
            company: company.map(str::to_string),
            position: position.map(str::to_string),
            emails: email
                .map(|value| ContactPoint {
                    label: String::new(),
                    value: value.to_string(),
                    preferred: true,
                })
                .into_iter()
                .collect(),
            phones: Vec::new(),
            addresses: Vec::new(),
            notes: None,
            created_at: now,
            updated_at: now,
        }
    }

    #[test]
    fn two_people_of_one_name_are_told_apart_by_what_they_have() {
        assert_eq!(
            told_apart_by(&person(
                Some("Engineer"),
                Some("Initech"),
                Some("ada@example.com")
            )),
            "Engineer \u{b7} Initech \u{b7} ada@example.com"
        );
        assert_eq!(
            told_apart_by(&person(None, Some(" "), Some("ada@example.com"))),
            "ada@example.com"
        );
        assert_eq!(told_apart_by(&person(None, None, None)), "");
    }

    #[test]
    fn a_failed_merge_is_said_in_words() {
        // The API's own sentence, where it gave one.
        assert_eq!(
            said(&UiError::api(
                409,
                "another change to these people is in progress: try again"
            )),
            "Another change to these people is in progress: try again"
        );
        // A missing person: which of the two is never said.
        let gone = said(&UiError::api(404, "person not found"));
        assert!(gone.starts_with("One of these two"), "{gone}");
        // Anything else is not passed on as if it were a sentence.
        let broke = said(&UiError::api(500, "boom"));
        assert!(broke.starts_with("Couldn\u{2019}t merge"), "{broke}");
    }
}
