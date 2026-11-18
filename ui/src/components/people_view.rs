//! People (#147): the registry (`/people`), a person's card
//! (`/people/:id`), and the pieces the node section reuses (the contact
//! actions and the person form).
//!
//! These are third parties' contact details. The API only ever returns the
//! caller's own people, so nothing here has to filter. Contact values render
//! as text (never HTML) and as mailto:/tel: links; addresses as plain text.

use common::form_draft::{Carried, FormRecovery, PersonForm, PersonFormDraft};
use common::id::PersonId;
use common::person::{
    ContactPoint, DuplicateMatch, Person, PersonField, PersonInput, PersonUsage, autofill_token,
    primary,
};
use common::person_carry::{CarriedField, CarryReport, TakenPerson, people_named};
use leptos::prelude::*;
use leptos_router::hooks::{use_navigate, use_params_map};

use crate::api::SavePerson;
use crate::components::empty_state::EmptyState;
use crate::components::format_helpers::copy_to_clipboard;
use crate::components::free_text_combo::FreeTextCombo;
use crate::components::icon_button::{IconButton, IconButtonVariant};
use crate::components::modals::{ModalPlacement, ModalShell};
use crate::components::node_people::{MembershipDraft, MembershipFields};
use crate::components::page_header::PageHeader;
use crate::components::toast::{ToastLevel, push_toast, push_undo_toast};

const INPUT_CLASS: &str = "w-full text-sm bg-stone-100 dark:bg-stone-800 text-stone-800 \
     dark:text-stone-200 rounded-lg px-2.5 py-1.5 focus:outline-none focus:ring-1 \
     focus:ring-amber-500";
const LABEL_CLASS: &str = "block text-xs font-medium text-stone-500 dark:text-stone-400 mb-1";
/// Helper text under a field, tied to it with `aria-describedby`.
pub(crate) const HINT_CLASS: &str = "block mt-1 text-xs text-stone-500 dark:text-stone-400";

/// `mailto:` / `tel:` href for a contact value. Phone links keep only what
/// a dialer reads. Addresses have none: map links are out of scope (#155).
fn contact_href(kind: ContactKind, value: &str) -> Option<String> {
    match kind {
        ContactKind::Email => Some(format!("mailto:{value}")),
        ContactKind::Phone => Some(format!(
            "tel:{}",
            value
                .chars()
                .filter(|c| c.is_ascii_digit() || *c == '+')
                .collect::<String>()
        )),
        ContactKind::Address => None,
    }
}

pub use common::person::ContactKind;

/// One email, phone or address as actions: an email or phone is a
/// mailto:/tel: link, an address is multi-line text, and a button copies it.
#[component]
pub fn ContactAction(kind: ContactKind, point: ContactPoint) -> impl IntoView {
    let href = contact_href(kind, &point.value);
    let value = point.value.clone();
    let icon = match kind {
        ContactKind::Email => "mail",
        ContactKind::Phone => "call",
        ContactKind::Address => "location_on",
    };
    let copy_label = match kind {
        ContactKind::Email => "Copy email",
        ContactKind::Phone => "Copy phone",
        ContactKind::Address => "Copy address",
    };
    let shown = match href {
        Some(href) => view! {
            <a href=href class="text-amber-700 dark:text-amber-400 hover:underline truncate"
                on:click=move |ev: web_sys::MouseEvent| ev.stop_propagation()>
                {point.value.clone()}
            </a>
        }
        .into_any(),
        None => view! {
            <span class="whitespace-pre-line text-stone-700 dark:text-stone-300">{point.value.clone()}</span>
        }
        .into_any(),
    };
    // A multi-line address keeps its icon and buttons on the first line.
    let row = if kind == ContactKind::Address {
        "inline-flex items-start gap-1 min-w-0 text-sm"
    } else {
        "inline-flex items-center gap-1 min-w-0 text-sm"
    };
    view! {
        <span class=row>
            <span class="material-symbols-outlined text-stone-400" style="font-size:14px;" aria-hidden="true">
                {icon}
            </span>
            {shown}
            {(!point.label.is_empty()).then(|| view! {
                <span class="text-xs text-stone-400">{point.label.clone()}</span>
            })}
            <button
                type="button"
                class="p-0.5 rounded text-stone-300 hover:text-stone-600 dark:hover:text-stone-300 cursor-pointer"
                title=copy_label
                aria-label=copy_label
                on:click=move |ev: web_sys::MouseEvent| {
                    ev.stop_propagation();
                    copy_to_clipboard(&value);
                    push_toast(ToastLevel::Success, "Copied.");
                }
            >
                <span class="material-symbols-outlined" style="font-size:14px;" aria-hidden="true">"content_copy"</span>
            </button>
        </span>
    }
}

/// The editable contact list (emails, phones or addresses). Typing writes through
/// untracked, so inputs never re-render (and lose focus) mid-word. Only
/// adding or removing a row redraws the list.
///
/// `flagged` marks rows (S8a): what the last save was refused for
/// ([`Flagged`]). A marked row is `aria-invalid`, and the mark is worked
/// out from what the rows hold now, so it goes when the row is put right.
#[component]
fn ContactListEditor(
    kind: ContactKind,
    points: RwSignal<Vec<ContactPoint>>,
    #[prop(optional)] flagged: Option<RwSignal<Flagged>>,
    /// A carry-over under review (S8b): the rows it added say "Added".
    /// Not an error, so not `flagged`: `aria-invalid` would say something
    /// is wrong, and the keyboard would be sent there.
    #[prop(optional)]
    carried: Option<RwSignal<Option<Carried>>>,
) -> impl IntoView {
    let redraw = RwSignal::new(0u32);
    // Whether row `i` is one the carry-over added, by what it holds now.
    let is_added = move |i: usize| {
        carried.is_some_and(|carried| {
            carried
                .get()
                .is_some_and(|c| points.with_untracked(|ps| c.report.marks(kind, ps, i)))
        })
    };
    let typed_in = move || {
        if let Some(flagged) = flagged {
            flagged.notify();
        }
        if let Some(carried) = carried {
            carried.notify();
        }
    };
    // Whether row `i` is marked, read when the flags change and when a
    // row is typed in (its input notifies the flags).
    let is_flagged = move |i: usize| {
        flagged.is_some_and(|flagged| {
            let flags = flagged.get();
            points.with_untracked(|ps| flags.marks(ps, i))
        })
    };
    // A text input with `inputmode`, not `type="email"`/`type="tel"`: those
    // types tell the browser this is the user's own address, and it offers
    // their details for someone else's card (#184).
    let (noun, heading, placeholder, input_mode) = match kind {
        ContactKind::Email => ("email", "Emails", "name@example.com", "email"),
        ContactKind::Phone => ("phone", "Phones", "+1 555 010 0000", "tel"),
        ContactKind::Address => ("address", "Addresses", "1 Main St\nSpringfield", "text"),
    };
    // Chrome ignores `off` on identity fields (#184): see `autofill_token`.
    let value_autocomplete = autofill_token(PersonField::Contact(kind));
    let add = Callback::new(move |()| {
        points.update_untracked(|ps| {
            ps.push(ContactPoint {
                label: String::new(),
                value: String::new(),
                preferred: ps.is_empty(),
            })
        });
        redraw.update(|n| *n += 1);
    });
    let label_placeholder = match kind {
        ContactKind::Email => "work",
        ContactKind::Phone => "mobile",
        ContactKind::Address => "home",
    };
    // Starter labels (#183): offered, never forced; the label saves as typed.
    let label_choices = Signal::stored(
        common::person::label_suggestions(kind)
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>(),
    );
    // The row's label as a signal for the combobox, written through to the
    // list untracked like the other fields, so the list never redraws.
    let row_label = move |i: usize, initial: String| {
        let label = RwSignal::new(initial);
        Effect::new(move |_| {
            let v = label.get();
            points.update_untracked(|ps| {
                if let Some(x) = ps.get_mut(i) {
                    x.label = v;
                }
            });
        });
        label
    };
    view! {
        <div class="space-y-1.5">
            // Add is a row action: icon-only, beside the list's heading (#156).
            <div class="flex items-center justify-between">
                <span class=LABEL_CLASS>{heading}</span>
                <IconButton icon="add" label=format!("Add {noun}") on_click=add />
            </div>
            {move || {
                redraw.track();
                points.get_untracked().into_iter().enumerate().map(|(i, p)| view! {
                    // Phones: the label gets its own line, then value +
                    // preferred + remove, instead of four controls on one line.
                    <div class="grid grid-cols-[1fr_auto_auto] sm:grid-cols-[7rem_1fr_auto_auto] gap-x-1.5 gap-y-1 items-center"
                        data-testid=format!("{noun}-row")>
                        <div class="col-span-3 sm:col-span-1">
                            <FreeTextCombo value=row_label(i, p.label.clone()) choices=label_choices
                                label=format!("{noun} label") list_label=format!("{noun} label suggestions")
                                placeholder=label_placeholder class=INPUT_CLASS />
                        </div>
                        {if kind == ContactKind::Address {
                            // Freeform and multi-line, like the old single field.
                            view! {
                                <textarea
                                    rows="2"
                                    autocomplete=value_autocomplete
                                    class=INPUT_CLASS
                                    placeholder=placeholder
                                    aria-label=noun
                                    aria-describedby=move || is_added(i).then(|| format!("{noun}-added-{i}"))
                                    prop:value=p.value.clone()
                                    on:input=move |ev| {
                                        let v = event_target_value(&ev);
                                        points.update_untracked(|ps| if let Some(x) = ps.get_mut(i) { x.value = v; });
                                        typed_in();
                                    }
                                />
                            }.into_any()
                        } else {
                            view! {
                                <input
                                    type="text"
                                    inputmode=input_mode
                                    autocomplete=value_autocomplete
                                    autocapitalize="off"
                                    spellcheck="false"
                                    class=format!(
                                        "{INPUT_CLASS} aria-invalid:ring-1 aria-invalid:ring-red-500"
                                    )
                                    placeholder=placeholder
                                    aria-label=noun
                                    aria-invalid=move || if is_flagged(i) { "true" } else { "false" }
                                    aria-describedby=move || is_added(i).then(|| format!("{noun}-added-{i}"))
                                    prop:value=p.value.clone()
                                    on:input=move |ev| {
                                        let v = event_target_value(&ev);
                                        points.update_untracked(|ps| if let Some(x) = ps.get_mut(i) { x.value = v; });
                                        // A mark is about the value: read it again.
                                        typed_in();
                                    }
                                />
                            }.into_any()
                        }}
                        <label class="flex items-center gap-1 text-xs text-stone-500 shrink-0" title="Preferred">
                            <input
                                type="radio"
                                name=format!("{noun}-preferred")
                                prop:checked=p.preferred
                                on:change=move |_| points.update_untracked(|ps| {
                                    for (j, x) in ps.iter_mut().enumerate() { x.preferred = j == i; }
                                })
                            />
                            "Preferred"
                        </label>
                        <IconButton icon="close" label=format!("Remove {noun}")
                            variant=IconButtonVariant::Danger
                            on_click=Callback::new(move |()| {
                                points.update_untracked(|ps| { if i < ps.len() { ps.remove(i); } });
                                redraw.update(|n| *n += 1);
                            }) />
                        {move || is_added(i).then(|| view! {
                            <span class="col-span-3 sm:col-span-4 text-xs text-stone-500 dark:text-stone-400"
                                id=format!("{noun}-added-{i}") data-testid="carried-mark">
                                {ADDED}
                            </span>
                        })}
                    </div>
                }).collect_view()
            }}
        </div>
    }
}

/// The editor's working copy of a person: a signal per field, owned by the
/// [`PersonEditor`], so Undo can put back exactly what was typed.
#[derive(Clone, Copy)]
struct PersonDraft {
    name: RwSignal<String>,
    company: RwSignal<String>,
    position: RwSignal<String>,
    emails: RwSignal<Vec<ContactPoint>>,
    phones: RwSignal<Vec<ContactPoint>>,
    addresses: RwSignal<Vec<ContactPoint>>,
    notes: RwSignal<String>,
}

impl PersonDraft {
    fn new() -> Self {
        Self {
            name: RwSignal::new(String::new()),
            company: RwSignal::new(String::new()),
            position: RwSignal::new(String::new()),
            emails: RwSignal::new(Vec::new()),
            phones: RwSignal::new(Vec::new()),
            addresses: RwSignal::new(Vec::new()),
            notes: RwSignal::new(String::new()),
        }
    }

    fn load(&self, p: PersonInput) {
        self.name.set(p.display_name);
        self.company.set(p.company.unwrap_or_default());
        self.position.set(p.position.unwrap_or_default());
        self.emails.set(p.emails);
        self.phones.set(p.phones);
        self.addresses.set(p.addresses);
        self.notes.set(p.notes.unwrap_or_default());
    }

    /// The current values. Untouched contact rows are left out; the server
    /// drops them anyway, and they mustn't make a draft look edited.
    fn read(&self) -> PersonInput {
        self.try_read().unwrap_or_default()
    }

    /// [`Self::read`], or `None` once the editor's signals are gone (the
    /// page is unmounting it, #170).
    fn try_read(&self) -> Option<PersonInput> {
        let opt = |s: String| (!s.trim().is_empty()).then_some(s);
        let rows = |v: Vec<ContactPoint>| {
            v.into_iter()
                .filter(|c| !(c.label.trim().is_empty() && c.value.trim().is_empty()))
                .collect()
        };
        Some(PersonInput {
            display_name: self.name.try_get_untracked()?,
            company: opt(self.company.try_get_untracked()?),
            position: opt(self.position.try_get_untracked()?),
            emails: rows(self.emails.try_get_untracked()?),
            phones: rows(self.phones.try_get_untracked()?),
            addresses: rows(self.addresses.try_get_untracked()?),
            notes: opt(self.notes.try_get_untracked()?),
        })
    }
}

/// What the [`PersonEditor`] saves: a new person, or an existing one.
#[derive(Clone, Copy, PartialEq, Eq)]
enum EditorTarget {
    New,
    Existing(PersonId),
}

/// The notice over a person dialog the app reopened from its draft (#170).
fn recovery_message(changed: bool) -> &'static str {
    if changed {
        "This device kept what you'd typed here. The person has been changed \
         since; saving replaces that change."
    } else {
        "This device kept what you'd typed here."
    }
}

/// The one person editor (create and edit look the same): a centred dialog
/// on desktop and a full-screen sheet on phones (`ModalPlacement::Sheet`),
/// with Save and Cancel pinned at the bottom. Only the name is required.
///
/// - Cancel closes and discards.
/// - Escape or a backdrop click with changes closes with "Changes
///   discarded · Undo", the task and note editors' pattern (#123).
/// - Ctrl/⌘+Enter saves.
/// - A refused save (an address that is someone else's, S8a) and the
///   server's validation messages show inside the dialog.
/// - What's typed is kept on this device while the dialog is open (#170):
///   a reload, a closed tab or a navigation away doesn't lose it. The next
///   time this dialog's page loads, it reopens with the input and a notice
///   offering Discard. Closing the dialog any other way drops the draft.
#[component]
pub fn PersonEditor(
    /// Open state, owned by the caller (it opens the editor).
    open: RwSignal<bool>,
    /// Which dialog this is; it names the draft and decides create or edit.
    form: PersonForm,
    /// The values to start from, read each time the editor opens: the
    /// person being edited, or a prefilled name.
    #[prop(into)]
    initial: Signal<PersonInput>,
    /// Runs after a successful save, with the saved person; the editor has
    /// already closed.
    #[prop(into)]
    on_saved: Callback<Person>,
    /// Creating from a node's People section (#162): the membership's Role
    /// and Note, under an "On this node" heading. The caller saves them
    /// with the attach; a blank block attaches as before.
    #[prop(optional)]
    membership: Option<MembershipDraft>,
    /// False while `initial` is still loading (an edit's person). A stored
    /// draft is only weighed against the loaded values.
    #[prop(optional, into)]
    ready: Option<Signal<bool>>,
    /// On a node whose owner may add people there: the refusal's notice
    /// can put the person who has the address on the node (S8b). `None`
    /// offers nothing of the kind.
    #[prop(optional_no_strip)]
    add_here: Option<AddHere>,
) -> impl IntoView {
    let target = match form {
        PersonForm::Edit(id) => EditorTarget::Existing(id),
        PersonForm::New | PersonForm::NewOnNode(_) => EditorTarget::New,
    };
    let draft = PersonDraft::new();
    let baseline = StoredValue::new(PersonInput::default());
    // Set by Undo, so reopening keeps the restored values.
    let restoring = StoredValue::new(false);
    let saving = RwSignal::new(false);
    // Some(changed) while the dialog shows input restored from a draft.
    let recovered = RwSignal::new(None::<bool>);
    // The draft's storage key: per user, so another account in this browser
    // is never offered someone else's contacts. None (never expected inside
    // AuthGate) turns drafts off rather than sharing them.
    let key_for = |f: PersonForm| {
        use_context::<crate::auth::AuthState>().and_then(|a| match a.get_untracked() {
            crate::auth::AuthStatus::Authenticated(u) => Some(f.key(&u.sub)),
            _ => None,
        })
    };
    let draft_key = StoredValue::new(key_for(form));
    let open_forms = use_context::<crate::draft::OpenForms>();
    // S8b: details typed for a new person, handed to this dialog when it
    // edits the person who already has the address. Taken here, at mount:
    // a page that never loads its person can't leave one waiting for a
    // later visit, and one meant for someone else is dropped.
    let hand_off = use_context::<crate::draft::PersonHandOff>();
    let incoming = StoredValue::new(match (form, hand_off) {
        (PersonForm::Edit(id), Some(h)) => {
            let waiting = h.0.get_untracked();
            if waiting.is_some() {
                h.0.set(None);
            }
            waiting.filter(|w| w.to == id)
        }
        _ => None,
    });
    // What this dialog is reviewing from such a hand-over, if anything.
    let carried = RwSignal::new(None::<Carried>);
    // The new-person dialog's own draft: dropped once a carry-over from
    // it is saved here.
    let source_key = StoredValue::new(key_for(PersonForm::New));
    let navigate = StoredValue::new(use_navigate());
    let duplicate = RwSignal::new(None::<Vec<DuplicateMatch>>);
    let error = RwSignal::new(None::<String>);
    // What the last save was refused for: its rows are marked (S8a).
    let flagged = RwSignal::new(Flagged::default());
    let form_id = match target {
        EditorTarget::New => "person-editor-form-new",
        EditorTarget::Existing(_) => "person-editor-form-edit",
    };
    // After a refusal the keyboard goes to the first marked address (the
    // alert says what is wrong, and every row is named "email"), as long
    // as that row is in view with the notice above it. In a short window,
    // or with the marked row far down, focusing the row scrolled the
    // notice away (seen in 3.30.0), and scrolling back would hide the
    // focused row instead. Then the keyboard goes to the notice itself: it
    // says why and what can be done, and Tab leads on from it.
    let focus_flagged = move || {
        set_timeout(
            move || {
                use wasm_bindgen::JsCast;
                let Some(form) = document().get_element_by_id(form_id) else {
                    return;
                };
                let find = |selector: &str| {
                    form.query_selector(selector)
                        .ok()
                        .flatten()
                        .and_then(|el| el.dyn_into::<web_sys::HtmlElement>().ok())
                };
                form.set_scroll_top(0);
                let marked = find("input[aria-invalid=\"true\"]");
                let in_view = marked.as_ref().is_some_and(|m| {
                    m.get_bounding_client_rect().bottom()
                        <= form.get_bounding_client_rect().bottom()
                });
                let target = if in_view {
                    marked
                } else {
                    find("[role=\"alert\"]").or(marked)
                };
                if let Some(target) = target {
                    let _ = target.focus();
                }
            },
            std::time::Duration::ZERO,
        );
    };
    // The keyboard was on a control that is gone (a notice's button whose
    // notice was replaced): put it on the form's alert, which says what
    // happened, instead of leaving it on the page behind the dialog.
    let focus_alert_if_lost = move || {
        set_timeout(
            move || {
                use wasm_bindgen::JsCast;
                let lost = document()
                    .active_element()
                    .is_none_or(|el| el.tag_name() == "BODY");
                if !lost {
                    return;
                }
                let alert = document()
                    .get_element_by_id(form_id)
                    .and_then(|form| form.query_selector("[role=\"alert\"]").ok().flatten())
                    .and_then(|el| el.dyn_into::<web_sys::HtmlElement>().ok());
                if let Some(alert) = alert {
                    let _ = alert.focus();
                }
            },
            std::time::Duration::ZERO,
        );
    };
    let position_hint_id = StoredValue::new(format!("{form_id}-position-hint"));
    let review_id = StoredValue::new(format!("{form_id}-carry-review"));
    // "Added" under a field the carry-over filled, while the field still
    // holds that (`now`): like a row's mark, it is about the value.
    let filled = move |field: CarriedField, now: String| {
        carried
            .get()
            .is_some_and(|c| c.report.fills(field, &now))
            .then(|| view! { <span class=HINT_CLASS data-testid="carried-mark">{ADDED}</span> })
    };
    let title = match target {
        EditorTarget::New => "New person",
        EditorTarget::Existing(_) => "Edit person",
    };

    // Keep the open dialog's input on this device, or drop the draft once
    // it holds nothing beyond what the dialog opened with. `try_` access:
    // this also runs while the page unmounts the editor. True when what
    // the dialog holds is safe here: stored, or nothing worth storing.
    let try_keep = move || -> bool {
        if open.try_get_untracked() != Some(true) {
            return false;
        }
        let Some(Some(key)) = draft_key.try_get_value() else {
            return false;
        };
        let (Some(person), Some(base)) = (draft.try_read(), baseline.try_get_value()) else {
            return false;
        };
        let typed = |f: fn(&MembershipDraft) -> RwSignal<String>| {
            membership
                .and_then(|m| f(&m).try_get_untracked())
                .unwrap_or_default()
        };
        let d = PersonFormDraft {
            person,
            role: typed(|m| m.role),
            note: typed(|m| m.note),
            base,
            saved_at: None,
            // Kept with the draft: a reload during the review brings the
            // notice and the marks back, and never merges again.
            carried: carried.try_get_untracked().flatten(),
        };
        if d.is_dirty() {
            crate::draft::write_person_draft(&key, &d)
        } else {
            crate::draft::clear_form_draft(&key);
            true
        }
    };
    let keep = move || {
        try_keep();
    };
    // The page going away: reload (the update banner's), tab close, or a
    // phone backgrounding it for good. Typing already wrote the draft; this
    // catches row removals and the radio, which fire no `input`.
    let unload = window_event_listener(leptos::ev::beforeunload, move |_| keep());
    let pagehide = window_event_listener(leptos::ev::pagehide, move |_| keep());
    // An in-app navigation unmounts the dialog with it.
    on_cleanup(move || {
        unload.remove();
        pagehide.remove();
        keep();
        if open.try_get_untracked() == Some(true)
            && let Some(f) = open_forms
        {
            f.0.try_update(|n| *n = n.saturating_sub(1));
        }
    });

    // A draft left by an earlier page: reopen with it, once, as soon as the
    // values it is weighed against have loaded.
    let checked = StoredValue::new(false);
    Effect::new(move |_| {
        if checked.get_value() || !ready.is_none_or(|r| r.get()) {
            return;
        }
        checked.set_value(true);
        let key = draft_key.get_value();
        let stored = key.as_deref().and_then(crate::draft::read_person_draft);
        let current = initial.get_untracked();
        let offered = stored.as_ref().map(|d| d.recovery(&current));
        // S8b: a hand-over is merged here, once, against the person as
        // just loaded, or against their own unsaved edit if this device
        // kept one. Never at the click: that copy could be stale by the
        // time the dialog shows.
        if let Some(h) = incoming.get_value() {
            incoming.set_value(None);
            let source_fp = common::person_carry::person_fingerprint(&h.typed);
            // An unsaved edit of them that this device kept, if any.
            let kept = match (&stored, offered) {
                (Some(d), Some(FormRecovery::Offer { changed })) => Some((d, changed)),
                _ => None,
            };
            // This very input is under review here already (Back, then the
            // same button again). The review is shown as it was left, by
            // the ordinary restore below: merging again would find nothing
            // to add and say so, over additions that are not saved yet.
            let reviewed_already = kept
                .is_some_and(|(d, _)| d.carried.as_ref().is_some_and(|c| c.source_fp == source_fp));
            if !reviewed_already {
                let (start, base, changed) = match kept {
                    Some((d, changed)) => (d.person.clone(), d.base.clone(), changed),
                    // `base` is the person as loaded, never the merged
                    // values: the dialog must count as changed, or closing
                    // it would discard what was added without an Undo.
                    None => (current.clone(), current.clone(), false),
                };
                let over = common::person_carry::carry_into(&start, &h.typed, &h.someone_elses);
                let nothing_to_say = over.report.added_nothing()
                    && over.report.not_used.is_empty()
                    && over.report.someone_elses.is_empty();
                if nothing_to_say && kept.is_none() {
                    // They have all of it already: nothing to review, and
                    // the new-person dialog has nothing left worth keeping.
                    if let (Some(key), Some(FormRecovery::Discard)) = (&key, offered) {
                        crate::draft::clear_form_draft(key);
                    }
                    if let Some(source) = source_key.get_value() {
                        crate::draft::clear_person_draft_if(&source, source_fp);
                    }
                    push_toast(ToastLevel::Info, "They already have everything you typed.");
                    return;
                }
                // On top of a kept edit, the review says so: Save writes
                // that edit too. On top of an earlier review, one report
                // accounts for both.
                let earlier = kept.and_then(|(d, _)| d.carried.clone());
                let on_kept_edit = earlier.as_ref().map_or(kept.is_some(), |c| c.on_kept_edit);
                let report = match earlier {
                    Some(c) => c.report.joined(over.report),
                    None => over.report,
                };
                draft.load(over.person);
                baseline.set_value(base);
                carried.set(Some(Carried {
                    source_fp,
                    report,
                    on_kept_edit,
                }));
                restoring.set_value(true);
                recovered.set(Some(changed));
                open.set(true);
                // On disk at once: a reload from here on restores this
                // review.
                keep();
                return;
            }
        }
        let (Some(key), Some(d), Some(offered)) = (key, stored, offered) else {
            return;
        };
        match offered {
            FormRecovery::Discard => crate::draft::clear_form_draft(&key),
            FormRecovery::Offer { changed } => {
                draft.load(d.person);
                baseline.set_value(d.base);
                if let Some(m) = membership {
                    m.role.set(d.role);
                    m.note.set(d.note);
                }
                carried.set(d.carried);
                restoring.set_value(true);
                recovered.set(Some(changed));
                open.set(true);
            }
        }
    });

    // Each opening starts from `initial`, except right after an Undo or a
    // draft restore. Every closing (Save, Cancel, Escape) drops the draft.
    Effect::new(move |was_open: Option<bool>| {
        let is_open = open.get();
        if is_open != was_open.unwrap_or(false)
            && let Some(f) = open_forms
        {
            f.0.update(|n| {
                *n = if is_open {
                    n.saturating_add(1)
                } else {
                    n.saturating_sub(1)
                }
            });
        }
        if !is_open && was_open == Some(true) {
            recovered.set(None);
            carried.set(None);
            if let Some(key) = draft_key.get_value() {
                crate::draft::clear_form_draft(&key);
            }
        }
        if is_open && was_open != Some(true) {
            duplicate.set(None);
            error.set(None);
            flagged.set(Flagged::default());
            if let Some(a) = add_here {
                // A new opening owes nothing to an add pressed in an
                // earlier one, still on its way or failed.
                a.failed.set(None);
                a.busy.set(false);
                a.opening.update(|n| *n += 1);
            }
            if restoring.get_value() {
                restoring.set_value(false);
            } else {
                draft.load(initial.get_untracked());
                baseline.set_value(draft.read());
                if let Some(m) = membership {
                    m.clear();
                }
            }
        }
        is_open
    });

    let save = move || {
        if saving.get_untracked() {
            return;
        }
        let input = draft.read();
        error.set(None);
        duplicate.set(None);
        flagged.set(Flagged::default());
        // The same address twice is refused by the API (422): say so
        // here, on the rows, without the round trip.
        if listed_twice(&input.emails) {
            error.set(Some("An email address is listed twice.".to_string()));
            flagged.set(Flagged {
                twice: true,
                ..Flagged::default()
            });
            focus_flagged();
            return;
        }
        saving.set(true);
        let was_carried = carried.get_untracked();
        wasm_bindgen_futures::spawn_local(async move {
            let result = match target {
                EditorTarget::New => crate::api::create_person(&input).await,
                EditorTarget::Existing(id) => crate::api::update_person(id, &input).await,
            };
            saving.try_set(false);
            match result {
                Ok(SavePerson::Saved(p)) => {
                    // S8b: what was typed for a new person is on this one
                    // now, so that dialog's draft goes: only now, and only
                    // if it still holds what was carried.
                    if let (Some(c), Some(Some(source))) =
                        (&was_carried, source_key.try_get_value())
                    {
                        crate::draft::clear_person_draft_if(&source, c.source_fp);
                    }
                    open.try_set(false);
                    on_saved.run(p);
                }
                // Refused: an address here is someone else's (§7.1).
                Ok(SavePerson::Collides(w)) => {
                    let taken = w
                        .matches
                        .iter()
                        .map(|m| common::person::email_key(&m.email))
                        .collect();
                    if flagged
                        .try_set(Flagged {
                            taken,
                            twice: false,
                        })
                        .is_none()
                    {
                        focus_flagged();
                    }
                    duplicate.try_set(Some(w.matches));
                }
                // A refusal the API explained (a list over its limit, a
                // value it won't take) reads as its own sentence.
                Err(e) => {
                    error.try_set(Some(e.explained().unwrap_or_else(|| e.to_string())));
                }
            }
        });
    };

    // The notice may be older than what the rows hold now: whether a row
    // still holds an address it named `person` for.
    let names_a_row = move |person: &TakenPerson| {
        draft.read().emails.iter().any(|e| {
            let key = typed_key(&e.value);
            person
                .emails
                .iter()
                .any(|theirs| common::person::email_key(theirs) == key)
        })
    };
    let stale_notice = move || {
        duplicate.set(None);
        flagged.set(Flagged::default());
        error.set(Some(
            "That address is no longer in this form. Save again.".to_string(),
        ));
        // The notice went, and the pressed button with it.
        focus_alert_if_lost();
    };
    // S8b, on a node: "add them here". The panel does the add, with the
    // Role and Note typed below; here only a stale notice is refused.
    let add_here = add_here.map(|a| AddHere {
        on_add: Callback::new(move |person: TakenPerson| {
            if names_a_row(&person) {
                a.on_add.run(person);
            } else {
                stale_notice();
            }
        }),
        ..a
    });
    // An answer can replace the pressed button ("Already here."): the
    // keyboard then goes to the notice.
    if let Some(a) = add_here {
        Effect::new(move |seen: Option<(Option<String>, Vec<PersonId>)>| {
            let now = (a.failed.get(), a.already.get());
            if seen.is_some_and(|was| was != now) && open.get_untracked() {
                focus_alert_if_lost();
            }
            now
        });
    }

    // S8b: "add what I typed to them". Synchronous: the typed input goes
    // into the hand-over and the page goes to that person, whose dialog
    // merges it against a fresh copy. Nothing is fetched or written here.
    let carry_to = Callback::new(move |person: TakenPerson| {
        let typed = draft.read();
        if !names_a_row(&person) {
            stale_notice();
            return;
        }
        // Only what the API would take as a person is carried.
        if let Err(e) = common::person::normalize(typed.clone()) {
            let said = crate::error::UiError::api(422, e.to_string());
            error.set(Some(said.explained().unwrap_or_else(|| e.to_string())));
            return;
        }
        let someone_elses = people_named(&duplicate.get_untracked().unwrap_or_default())
            .into_iter()
            .filter(|p| p.id != person.id)
            .flat_map(|p| p.emails)
            .collect();
        // The draft on disk is exactly what is handed over: its
        // fingerprint is how it is recognised once they are saved. If
        // this device can't keep it (storage full or blocked), nothing is
        // carried: backing out of the review must have this to go back to.
        if !try_keep() {
            error.set(Some(NOT_KEPT.to_string()));
            return;
        }
        if let Some(h) = hand_off {
            h.0.set(Some(crate::draft::HandOff {
                to: person.id,
                typed,
                someone_elses,
            }));
        }
        navigate.get_value()(&format!("/people/{}", person.id), Default::default());
    });

    // Escape and the backdrop: discard, with Undo when something changed.
    let dismiss = Callback::new(move |()| {
        let typed = draft.read();
        let was_carried = carried.get_untracked();
        let was_recovered = recovered.get_untracked();
        open.set(false);
        // Undo keeps the membership fields too: they aren't cleared until
        // the next fresh opening.
        let membership_typed = membership.is_some_and(|m| !m.is_blank());
        if typed != baseline.get_value() || membership_typed {
            push_undo_toast(
                "Changes discarded",
                std::sync::Arc::new(move || {
                    draft.load(typed.clone());
                    restoring.set_value(true);
                    // Closing dropped the review's notice and marks, and
                    // whether they were changed since, which Save replaces.
                    if was_carried.is_some() {
                        recovered.try_set(was_recovered);
                    }
                    carried.try_set(was_carried.clone());
                    open.try_set(true);
                }),
            );
        }
    });
    let on_key = Callback::new(move |ev: web_sys::KeyboardEvent| {
        if ev.key() == "Enter" && (ev.ctrl_key() || ev.meta_key()) {
            ev.prevent_default();
            save();
        }
    });
    let name_ref: NodeRef<leptos::html::Input> = NodeRef::new();
    Effect::new(move |_| {
        if open.get()
            && let Some(el) = name_ref.get()
        {
            let _ = el.focus();
        }
    });

    view! {
        <ModalShell
            show=open
            on_close=dismiss
            label=title
            placement=ModalPlacement::Sheet
            panel_class="h-full sm:h-auto sm:max-h-[90vh] sm:max-w-2xl flex flex-col"
            on_key=on_key
        >
            <div class="flex items-center justify-between gap-3 px-4 py-3 border-b border-stone-200 dark:border-stone-800"
                data-testid="person-editor">
                <h2 class="font-display text-lg text-stone-900 dark:text-stone-100">{title}</h2>
                <IconButton icon="close" label="Close" on_click=dismiss />
            </div>
            // Every field describes a third party, never the signed-in
            // user: browser autofill stays out of all of them (#184). Fields
            // the browser takes for identity fields carry an `autofill_token`,
            // because Chrome ignores `off` on them; the rest inherit `off`.
            <form
                id=form_id
                autocomplete="off"
                class="flex-1 overflow-y-auto px-4 py-4 space-y-4"
                data-testid="person-form"
                on:submit=move |ev| { ev.prevent_default(); save(); }
                // Every field's `input` and the radios' `change` bubble here.
                on:input=move |_| keep()
                on:change=move |_| keep()
            >
                {move || match (carried.get(), recovered.get()) {
                    // S8b: a carry-over under review says what it did.
                    (Some(c), changed) => view! {
                        <CarryReview report=c.report changed=changed.unwrap_or(false)
                            kept_edit=c.on_kept_edit
                            id=review_id.get_value()
                            on_discard=Callback::new(move |()| open.set(false)) />
                    }.into_any(),
                    (None, Some(changed)) => view! {
                        <div data-testid="form-draft-recovery" role="status"
                            class="flex flex-wrap items-center gap-3 rounded-lg px-3 py-2 text-sm
                                bg-amber-50 dark:bg-amber-900/20 text-amber-900 dark:text-amber-200">
                            <span>{recovery_message(changed)}</span>
                            // Discard ends the dialog, like Cancel: the app opened
                            // it only to give the input back.
                            <button type="button" class="text-xs underline cursor-pointer"
                                on:click=move |_| open.set(false)>
                                "Discard"
                            </button>
                        </div>
                    }.into_any(),
                    (None, None) => ().into_any(),
                }}
                {move || error.get().map(|e| view! {
                    // Focusable: a refusal whose marked row is out of view
                    // puts the keyboard here (`focus_flagged`).
                    <p class="text-sm text-red-600 dark:text-red-400" role="alert" tabindex="-1">{e}</p>
                })}
                {move || duplicate.get().map(|matches| view! {
                    // Opening the other person navigates away with the
                    // dialog still open, so the page's unmount keeps what
                    // was typed as a draft (#170). Closing it first would
                    // drop the draft: every closing does.
                    <EmailTakenNotice matches=matches
                        offer=match form {
                            PersonForm::New => TakenOffer::CarryOver(carry_to),
                            PersonForm::NewOnNode(_) => {
                                add_here.map_or(TakenOffer::OnNode, TakenOffer::AddHere)
                            }
                            PersonForm::Edit(_) => TakenOffer::OpenOnly,
                        } />
                })}
                <label class="block">
                    <span class=LABEL_CLASS>"Name"</span>
                    // The review is tied to the field that takes the
                    // keyboard on open: a status mounted with its text is
                    // often not announced on its own.
                    <input type="text" node_ref=name_ref class=INPUT_CLASS required=true
                        autocomplete=autofill_token(PersonField::Name)
                        aria-describedby=move || carried.get().is_some().then(|| review_id.get_value())
                        prop:value=move || draft.name.get()
                        on:input=move |ev| draft.name.set(event_target_value(&ev)) />
                </label>
                <div class="grid grid-cols-1 sm:grid-cols-2 gap-3">
                    <label class="block">
                        <span class=LABEL_CLASS>"Company"</span>
                        <input type="text" class=INPUT_CLASS
                            autocomplete=autofill_token(PersonField::Company)
                            prop:value=move || draft.company.get()
                            on:input=move |ev| draft.company.set(event_target_value(&ev)) />
                        {move || filled(CarriedField::Company, draft.company.get())}
                    </label>
                    // Position is the person's, Role the membership's (#147); the
                    // hint says so while typing, which a placeholder can't (#182).
                    <label class="block">
                        <span class=LABEL_CLASS>"Position"</span>
                        <input type="text" class=INPUT_CLASS autocomplete="off"
                            aria-describedby=position_hint_id.get_value()
                            prop:value=move || draft.position.get()
                            on:input=move |ev| draft.position.set(event_target_value(&ev)) />
                        <span id=position_hint_id.get_value() class=HINT_CLASS>
                            "Their job title \u{2014} stays with the person on every node."
                        </span>
                        {move || filled(CarriedField::Position, draft.position.get())}
                    </label>
                </div>
                <ContactListEditor kind=ContactKind::Email points=draft.emails flagged=flagged
                    carried=carried />
                <ContactListEditor kind=ContactKind::Phone points=draft.phones carried=carried />
                <ContactListEditor kind=ContactKind::Address points=draft.addresses carried=carried />
                <label class="block">
                    <span class=LABEL_CLASS>"Notes"</span>
                    <textarea rows="4" class=INPUT_CLASS autocomplete="off" prop:value=move || draft.notes.get()
                        on:input=move |ev| draft.notes.set(event_target_value(&ev)) />
                    {move || filled(CarriedField::Notes, draft.notes.get())}
                </label>
                // The membership's fields, set apart from the person's: the
                // heading says they belong to this node, not to the card.
                {membership.map(|m| view! {
                    <div role="group" aria-label="On this node"
                        class="space-y-1.5 pt-4 border-t border-stone-200 dark:border-stone-800"
                        data-testid="membership-fields">
                        <h3 class="text-sm font-medium text-stone-800 dark:text-stone-200">"On this node"</h3>
                        <p class="text-xs text-stone-500 dark:text-stone-400">
                            "Optional. Their role and a note here; Position stays on their card."
                        </p>
                        <MembershipFields draft=m />
                    </div>
                })}
            </form>
            // Pinned, so Save stays reachable on a phone however long the form.
            <div class="flex items-center justify-end gap-2 px-4 pt-3 border-t border-stone-200 dark:border-stone-800"
                style="padding-bottom: max(0.75rem, env(safe-area-inset-bottom));">
                <button type="button"
                    class="px-3 py-2 rounded-lg text-sm text-stone-600 dark:text-stone-300 hover:bg-stone-100 dark:hover:bg-stone-800 cursor-pointer"
                    on:click=move |_| open.set(false)>
                    "Cancel"
                </button>
                // A dialog's commit button keeps its word, with an icon, as in
                // the create-node dialog: a lone check is ambiguous there.
                <button type="submit" form=form_id
                    class="px-4 py-2 rounded-lg text-sm font-medium bg-ember hover:bg-ember-strong text-white
                        disabled:opacity-50 disabled:cursor-not-allowed cursor-pointer flex items-center gap-1.5"
                    disabled=move || saving.get() || draft.name.get().trim().is_empty()>
                    <span class="material-symbols-outlined" style="font-size: 16px;" aria-hidden="true">"check"</span>
                    "Save"
                </button>
            </div>
        </ModalShell>
    }
}

/// The key of an address as it is typed in a row. The API trims a value
/// before it keys it (`common::person::normalize`, every kind of white
/// space; `email_key` itself takes off spaces only), so the editor does
/// too: otherwise a pasted address with a tab or a no-break space after
/// it would be a different address here and the same one there.
fn typed_key(value: &str) -> String {
    common::person::email_key(value.trim())
}

/// Whether `emails` holds an address more than once (rows nobody typed
/// in aside). The API refuses such a list; the editor says so first.
fn listed_twice(emails: &[ContactPoint]) -> bool {
    let mut seen = std::collections::HashSet::new();
    emails
        .iter()
        .map(|e| typed_key(&e.value))
        .filter(|key| !key.is_empty())
        .any(|key| !seen.insert(key))
}

/// What a save was refused for, for marking rows (S8a).
#[derive(Debug, Clone, Default, PartialEq)]
struct Flagged {
    /// Keys of addresses that belong to someone else: a row is marked
    /// while it holds one.
    taken: Vec<String>,
    /// An address was listed twice: a row is marked while another row
    /// holds the same address. A relation between rows, so it is read
    /// from the rows as they are, not remembered as a key.
    twice: bool,
}

impl Flagged {
    /// Whether row `i` of `rows` is marked.
    fn marks(&self, rows: &[ContactPoint], i: usize) -> bool {
        let Some(key) = rows.get(i).map(|p| typed_key(&p.value)) else {
            return false;
        };
        if key.is_empty() {
            return false;
        }
        self.taken.contains(&key)
            || (self.twice && rows.iter().filter(|p| typed_key(&p.value) == key).count() > 1)
    }
}

/// The mark on a row or field a carry-over added (S8b).
const ADDED: &str = "Added from what you typed";

/// Said when a carry-over is refused because the typed input could not
/// be stored on this device.
const NOT_KEPT: &str = "This device can\u{2019}t keep what you typed right now, so it was not \
     carried over. It is still in this form.";

/// What a dialog reviewing a carry-over says (S8b): where the additions
/// came from, what they are, what was typed and not used, and that
/// nothing is saved yet. Discard closes the dialog and changes nothing.
#[component]
fn CarryReview(
    report: CarryReport,
    /// The person was saved with other values since this device kept an
    /// edit of them.
    changed: bool,
    /// The additions are on top of an unsaved edit of them that this
    /// device had kept.
    kept_edit: bool,
    id: String,
    #[prop(into)] on_discard: Callback<()>,
) -> impl IntoView {
    let from = if report.typed_name.is_empty() {
        "the person you were typing".to_string()
    } else {
        format!(
            "\u{201c}{}\u{201d}, the person you were typing",
            report.typed_name
        )
    };
    let lines = common::person_carry::review_lines(&report);
    view! {
        <div data-testid="carry-review" role="status" id=id
            class="rounded-lg px-3 py-2 text-sm space-y-1.5
                bg-amber-50 dark:bg-amber-900/20 text-amber-900 dark:text-amber-200">
            <p class="font-medium">
                {format!("From {from}. Nothing is saved until you press Save.")}
            </p>
            <ul class="list-disc pl-5 space-y-0.5">
                {lines.into_iter().map(|line| view! { <li>{line}</li> }).collect_view()}
            </ul>
            {kept_edit.then(|| view! {
                <p data-testid="carry-kept-edit">
                    "This is on top of an unsaved edit of them that this device had kept. \
                     Save writes that edit too; Discard drops it."
                </p>
            })}
            {changed.then(|| view! {
                <p>"The person has been changed since; saving replaces that change."</p>
            })}
            <button type="button" class="text-xs underline cursor-pointer"
                on:click=move |_| on_discard.run(())>
                "Discard"
            </button>
        </div>
    }
}

/// On a node: what the refusal's notice does with a person it names
/// (S8b). The panel that owns the dialog does the add, so the Role and
/// Note typed under "On this node" go with it.
#[derive(Clone, Copy)]
pub struct AddHere {
    /// Put this person on the node. The caller closes the dialog once the
    /// add has succeeded and not before: a failed add loses nothing typed.
    pub on_add: Callback<TakenPerson>,
    /// Who is on the node already: they are not offered again.
    pub already: Signal<Vec<PersonId>>,
    /// An add is on its way.
    pub busy: RwSignal<bool>,
    /// Why the last add failed, in words.
    pub failed: RwSignal<Option<String>>,
    /// Counts the dialog's openings; the editor bumps it. An answer that
    /// comes late acts on the dialog only if it is still the opening its
    /// button was pressed in.
    pub opening: RwSignal<u32>,
}

/// The look of a button the notice offers beside a person.
const OFFER_BUTTON: &str = "rounded-md px-2 py-0.5 text-xs text-stone-700 dark:text-stone-200 \
     border border-stone-300 dark:border-stone-600 hover:bg-stone-100 dark:hover:bg-stone-800 \
     cursor-pointer aria-disabled:opacity-50 aria-disabled:cursor-not-allowed";

/// What the refusal's notice offers for each person it names, besides
/// opening them (S8b).
#[derive(Clone, Copy)]
pub enum TakenOffer {
    /// Editing a saved person: merging two saved people is the merge
    /// tool's (S8c).
    OpenOnly,
    /// A new person being added on a node where nothing can be added by
    /// this reader now.
    OnNode,
    /// A new person being added on a node: put the person who has the
    /// address there instead, with the Role and Note typed.
    AddHere(AddHere),
    /// A new person from the registry: add what was typed to them, for
    /// review.
    CarryOver(Callback<TakenPerson>),
}

/// Why a save was refused: an email address here already belongs to
/// someone else in the registry (S8a; ONTOLOGY §7.1: the same address is
/// the same person). It names them, once each, with a way to open that
/// record and what else the dialog can do for them ([`TakenOffer`]). It
/// offers nothing that saves anyway. With no one left to name (they were
/// removed a moment later) it says to try again.
#[component]
pub fn EmailTakenNotice(matches: Vec<DuplicateMatch>, offer: TakenOffer) -> impl IntoView {
    let navigate = StoredValue::new(use_navigate());
    let people = people_named(&matches);
    let nobody = people.is_empty();
    view! {
        <div class="rounded-lg border border-amber-300 dark:border-amber-700 bg-amber-50 dark:bg-amber-900/20 p-3 text-sm space-y-2"
            role="alert" tabindex="-1" data-testid="email-taken">
            <p class="text-stone-700 dark:text-stone-200">
                {if nobody {
                    "An email address here was just given to someone else in your registry. \
                     Save again."
                } else {
                    "Not saved. An email address here already belongs to someone in your \
                     registry, and the same address means the same person:"
                }}
            </p>
            <ul class="space-y-1.5">
                {people.into_iter().map(|person| {
                    let id = person.id;
                    let name = person.display_name.clone();
                    let addresses = person.emails.join(", ");
                    let offered = match offer {
                        TakenOffer::CarryOver(carry_to) => view! {
                            <button type="button" class=OFFER_BUTTON
                                // The name starts with the words on the button.
                                aria-label=format!("Add what I typed to them: {name}")
                                data-testid="carry-over"
                                on:click=move |_| carry_to.run(person.clone())>
                                "Add what I typed to them"
                            </button>
                        }.into_any(),
                        TakenOffer::AddHere(a) => {
                            let label = format!("Add {name} here");
                            (move || if a.already.get().contains(&id) {
                                view! {
                                    <span class="text-xs text-stone-600 dark:text-stone-300"
                                        data-testid="already-here">
                                        "Already here."
                                    </span>
                                }.into_any()
                            } else {
                                let person = person.clone();
                                view! {
                                    <button type="button" class=OFFER_BUTTON data-testid="add-here"
                                        // Not `disabled`: that would drop the
                                        // keyboard from the pressed button. The
                                        // panel ignores a press while one is on
                                        // its way.
                                        aria-disabled=move || if a.busy.get() { "true" } else { "false" }
                                        on:click=move |_| a.on_add.run(person.clone())>
                                        {label.clone()}
                                    </button>
                                }.into_any()
                            }).into_any()
                        }
                        TakenOffer::OpenOnly | TakenOffer::OnNode => ().into_any(),
                    };
                    view! {
                        <li class="flex flex-wrap items-center gap-x-2 gap-y-1">
                            <span class="min-w-0">
                                <button type="button" class="text-amber-700 dark:text-amber-400 hover:underline cursor-pointer"
                                    on:click=move |_| {
                                        navigate.get_value()(&format!("/people/{id}"), Default::default());
                                    }>
                                    {name.clone()}
                                </button>
                                <span class="text-stone-500 dark:text-stone-400">{format!(" · {addresses}")}</span>
                            </span>
                            {offered}
                        </li>
                    }
                }).collect_view()}
            </ul>
            // Inside the alert, so it is read out when it appears.
            {match offer {
                TakenOffer::AddHere(a) => Some(move || a.failed.get().map(|why| view! {
                    <p class="text-sm text-red-600 dark:text-red-400" data-testid="add-here-failed">{why}</p>
                })),
                _ => None,
            }}
            {(!nobody).then(|| view! {
                <p class="text-xs text-stone-600 dark:text-stone-300">
                    {match offer {
                        TakenOffer::AddHere(_) => {
                            "Add them here: the role and note below go with them. The other \
                             details you typed are not added to them and are not kept. Or \
                             open them (what you typed is kept), or take the address off."
                        }
                        TakenOffer::CarryOver(_) => {
                            "Add what you typed to them (you review it before anything is \
                             saved), open them, or take the address off here. What you typed \
                             is kept."
                        }
                        TakenOffer::OnNode => {
                            "Open them (what you typed is kept), or take the address off. \
                             People can\u{2019}t be added to this node from here."
                        }
                        TakenOffer::OpenOnly => {
                            "Open them, or take the address off here. Merging two saved \
                             people isn\u{2019}t available yet. What you typed is kept."
                        }
                    }}
                </p>
            })}
        </div>
    }
}

// ── /people ────────────────────────────────────────────────────────────────────

/// How long a just-created row stays highlighted in the registry.
const NEW_ROW_HIGHLIGHT: std::time::Duration = std::time::Duration::from_millis(2500);

#[component]
pub fn PeopleView() -> impl IntoView {
    let query = RwSignal::new(String::new());
    // Bumped after a create, so the list refetches with the new row in it.
    let version = RwSignal::new(0u32);
    let people = LocalResource::new(move || {
        version.track();
        let q = query.get();
        async move { crate::api::fetch_people(Some(&q)).await }
    });
    let editor_open = RwSignal::new(false);
    // The row to highlight briefly: the person just created.
    let highlight = RwSignal::new(None::<PersonId>);
    // A modal flow returns to where it was launched from (#154): creating
    // from the registry stays on the registry, so adding several people in
    // a row needs no Back. The person's page is opened explicitly (a row).
    let on_created = Callback::new(move |p: Person| {
        // A search that doesn't match the new person would hide its row.
        query.set(String::new());
        version.update(|v| *v += 1);
        highlight.set(Some(p.id));
        push_toast(ToastLevel::Success, "Person added.");
        set_timeout(
            move || {
                // Only clear our own highlight, not a newer one.
                if highlight.try_get_untracked() == Some(Some(p.id)) {
                    highlight.try_set(None);
                }
            },
            NEW_ROW_HIGHLIGHT,
        );
    });

    view! {
        <div class="flex flex-col h-full">
            <PageHeader icon="group" title="People">
                // Icon-only, like the node list's "New node" (+).
                <IconButton icon="person_add" label="New person"
                    on_click=Callback::new(move |()| editor_open.set(true)) />
            </PageHeader>
            // Full width, like the node list: a registry is scanned (#156).
            <div class="flex-1 overflow-auto p-4 md:p-6 space-y-4">
                <PersonEditor open=editor_open form=PersonForm::New
                    initial=Signal::derive(PersonInput::default) on_saved=on_created />
                <input
                    type="search"
                    class=INPUT_CLASS
                    placeholder="Search people\u{2026}"
                    aria-label="Search people"
                    prop:value=move || query.get()
                    on:input=move |ev| query.set(event_target_value(&ev))
                />
                // Transition, not Suspense: a refetch (search, a create) keeps
                // the current list on screen instead of flashing "Loading…".
                <Transition fallback=|| view! { <p class="text-sm text-stone-400">"Loading\u{2026}"</p> }>
                    {move || people.get().map(|r| match r {
                        Ok(list) if list.is_empty() => view! {
                            <EmptyState icon="group" message="No people yet."
                                hint="People you add here can be placed on any node." />
                        }.into_any(),
                        Ok(list) => view! {
                            <ul class="divide-y divide-stone-100 dark:divide-stone-800 rounded-lg border border-stone-200 dark:border-stone-800">
                                {list.into_iter().map(|p| {
                                    let id = p.id;
                                    view! {
                                        <PersonRow person=p
                                            highlighted=Signal::derive(move || highlight.get() == Some(id)) />
                                    }
                                }).collect_view()}
                            </ul>
                        }.into_any(),
                        Err(e) => view! { <p class="text-sm text-red-500">{format!("Error: {e}")}</p> }.into_any(),
                    })}
                </Transition>
            </div>
        </div>
    }
}

#[component]
fn PersonRow(
    person: Person,
    /// Just created: tinted for a moment and scrolled into view.
    #[prop(into)]
    highlighted: Signal<bool>,
) -> impl IntoView {
    let navigate = use_navigate();
    let id = person.id;
    let subtitle = [person.position.clone(), person.company.clone()]
        .into_iter()
        .flatten()
        .collect::<Vec<_>>()
        .join(" \u{b7} ");
    let email = primary(&person.emails).cloned();
    let row_ref: NodeRef<leptos::html::Li> = NodeRef::new();
    Effect::new(move |_| {
        if highlighted.get()
            && let Some(el) = row_ref.get()
        {
            let opts = web_sys::ScrollIntoViewOptions::new();
            opts.set_block(web_sys::ScrollLogicalPosition::Nearest);
            el.scroll_into_view_with_scroll_into_view_options(&opts);
        }
    });
    view! {
        <li node_ref=row_ref
            class=move || if highlighted.get() {
                "px-4 py-3 cursor-pointer transition-colors bg-amber-50 dark:bg-amber-900/20"
            } else {
                "px-4 py-3 cursor-pointer transition-colors hover:bg-stone-50 dark:hover:bg-stone-900/60"
            }
            data-testid="person-row"
            data-new=move || highlighted.get().then_some("true")
            on:click=move |_| navigate(&format!("/people/{id}"), Default::default())>
            // A real button: the row was click-only (#281). Its click
            // bubbles to the row's handler.
            <button type="button"
                class="block text-left text-sm font-medium text-stone-900 dark:text-stone-100 cursor-pointer
                       focus-visible:outline-2 focus-visible:outline-amber-500 rounded">
                {person.display_name.clone()}
            </button>
            <div class="flex flex-wrap items-center gap-x-3 text-xs text-stone-500">
                {(!subtitle.is_empty()).then(|| view! { <span>{subtitle}</span> })}
                {email.map(|e| view! { <ContactAction kind=ContactKind::Email point=e /> })}
            </div>
        </li>
    }
}

// ── /people/:id ──────────────────────────────────────────────────────────────

#[component]
pub fn PersonRoute() -> impl IntoView {
    let params = use_params_map();
    move || match params.with(|p| p.get("id").and_then(|s| s.parse::<PersonId>().ok())) {
        Some(id) => view! { <PersonView id=id /> }.into_any(),
        None => view! { <p class="p-6 text-red-500">"Invalid person ID"</p> }.into_any(),
    }
}

#[component]
fn PersonView(id: PersonId) -> impl IntoView {
    let navigate = StoredValue::new(use_navigate());
    let version = RwSignal::new(0u32);
    let person = LocalResource::new(move || {
        version.track();
        crate::api::fetch_person(id)
    });
    let usages = LocalResource::new(move || {
        version.track();
        crate::api::fetch_person_usages(id)
    });
    let editor_open = RwSignal::new(false);
    let current = Signal::derive(move || {
        person
            .get()
            .and_then(Result::ok)
            .map(|p| PersonInput::from(&p))
            .unwrap_or_default()
    });
    let on_saved = Callback::new(move |_: Person| {
        version.update(|v| *v += 1);
        push_toast(ToastLevel::Success, "Saved.");
    });
    let delete = move || {
        let ok = web_sys::window()
            .and_then(|w| {
                w.confirm_with_message("Delete this person? They'll be removed from every node.")
                    .ok()
            })
            .unwrap_or(false);
        if !ok {
            return;
        }
        wasm_bindgen_futures::spawn_local(async move {
            match crate::api::delete_person(id).await {
                // The page may be gone by now: `try_` access (#281).
                Ok(()) => {
                    if let Some(go) = navigate.try_get_value() {
                        go("/people", Default::default());
                    }
                }
                Err(e) => push_toast(ToastLevel::Error, format!("Couldn't delete: {e}")),
            }
        });
    };

    view! {
        <div class="flex flex-col h-full">
            <PageHeader icon="person" title="Person">
                <button type="button" class="p-1.5 rounded-lg text-stone-400 hover:text-stone-600 hover:bg-stone-100 dark:hover:bg-stone-800 cursor-pointer"
                    title="Back to People" aria-label="Back to People"
                    on:click=move |_| navigate.get_value()("/people", Default::default())>
                    <span class="material-symbols-outlined" aria-hidden="true">"arrow_back"</span>
                </button>
            </PageHeader>
            <div class="flex-1 overflow-auto p-4 md:p-6 max-w-3xl space-y-6">
                <PersonEditor open=editor_open form=PersonForm::Edit(id)
                    initial=current on_saved=on_saved
                    ready=Signal::derive(move || person.get().is_some_and(|r| r.is_ok())) />
                <Suspense fallback=|| view! { <p class="text-sm text-stone-400">"Loading\u{2026}"</p> }>
                    {move || person.get().map(|r| match r {
                        Err(e) => view! { <p class="text-sm text-red-500">{format!("Error: {e}")}</p> }.into_any(),
                        Ok(p) => view! {
                            <PersonCard person=p
                                on_edit=Callback::new(move |()| editor_open.set(true))
                                on_delete=Callback::new(move |()| delete()) />
                        }.into_any(),
                    })}
                </Suspense>
                <section aria-labelledby="used-in-heading">
                    <h2 id="used-in-heading" class="text-sm font-semibold text-stone-700 dark:text-stone-300 mb-2">"Used in"</h2>
                    <Suspense fallback=|| ()>
                        {move || usages.get().map(|r| match r {
                            Ok(list) if list.is_empty() => view! {
                                <p class="text-sm text-stone-400">"Not on any node yet."</p>
                            }.into_any(),
                            Ok(list) => view! {
                                <ul class="space-y-1" data-testid="person-usages">
                                    {list.into_iter().map(|u| view! { <UsageRow usage=u /> }).collect_view()}
                                </ul>
                            }.into_any(),
                            // Not "Not on any node yet", and not nothing (#281, 1.7).
                            Err(_) => view! {
                                <crate::components::load_error::LoadError
                                    what="where this person appears"
                                    on_retry=Callback::new(move |()| version.update(|n| *n += 1))
                                />
                            }.into_any(),
                        })}
                    </Suspense>
                </section>
            </div>
        </div>
    }
}

#[component]
fn PersonCard(
    person: Person,
    #[prop(into)] on_edit: Callback<()>,
    #[prop(into)] on_delete: Callback<()>,
) -> impl IntoView {
    let subtitle = [person.position.clone(), person.company.clone()]
        .into_iter()
        .flatten()
        .collect::<Vec<_>>()
        .join(" \u{b7} ");
    let empty = person.emails.is_empty()
        && person.phones.is_empty()
        && person.addresses.is_empty()
        && person.notes.is_none();
    view! {
        <div class="space-y-4" data-testid="person-card">
            <div class="flex items-start justify-between gap-3">
                <div>
                    <h1 class="font-display text-2xl text-stone-900 dark:text-stone-100">{person.display_name.clone()}</h1>
                    {(!subtitle.is_empty()).then(|| view! { <p class="text-sm text-stone-500">{subtitle}</p> })}
                </div>
                // Icon-only, like the node page header's edit and delete.
                <div class="flex items-center gap-1">
                    <IconButton icon="edit" label="Edit person" on_click=on_edit />
                    <IconButton icon="delete" label="Delete person" variant=IconButtonVariant::Danger
                        on_click=on_delete />
                </div>
            </div>
            <div class="space-y-1.5">
                {person.emails.into_iter().map(|e| view! { <div><ContactAction kind=ContactKind::Email point=e /></div> }).collect_view()}
                {person.phones.into_iter().map(|p| view! { <div><ContactAction kind=ContactKind::Phone point=p /></div> }).collect_view()}
            </div>
            {(!person.addresses.is_empty()).then(|| view! {
                <div class="space-y-1.5" data-testid="person-addresses">
                    {person.addresses.into_iter().map(|a| view! { <div><ContactAction kind=ContactKind::Address point=a /></div> }).collect_view()}
                </div>
            })}
            {person.notes.map(|n| view! {
                <div class="text-sm">
                    <div class=LABEL_CLASS>"Notes"</div>
                    <p class="whitespace-pre-line text-stone-700 dark:text-stone-300">{n}</p>
                </div>
            })}
            {empty.then(|| view! {
                <p class="text-sm text-stone-400">"Just a name so far. Edit to add an email, phone, address or notes."</p>
            })}
        </div>
    }
}

#[component]
fn UsageRow(usage: PersonUsage) -> impl IntoView {
    let navigate = use_navigate();
    let node = usage.node_id;
    let icon = crate::components::node_meta::type_icon(usage.node_type.as_str());
    view! {
        <li>
            <button type="button" class="flex items-center gap-2 text-sm text-stone-700 dark:text-stone-200 hover:text-amber-700 cursor-pointer"
                on:click=move |_| navigate(&format!("/nodes/{node}"), Default::default())>
                <span class="material-symbols-outlined text-stone-400" style="font-size:16px;" aria-hidden="true">{icon}</span>
                {usage.node_title}
                {usage.role.map(|r| view! { <span class="text-xs text-stone-500">{format!("\u{b7} {r}")}</span> })}
            </button>
        </li>
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn email(value: &str) -> ContactPoint {
        ContactPoint {
            label: String::new(),
            value: value.into(),
            preferred: false,
        }
    }

    /// S8a: the editor finds an address listed twice as the API would,
    /// which trims a value before it keys it.
    #[test]
    fn an_address_listed_twice_is_found_as_the_api_finds_it() {
        assert!(!listed_twice(&[]));
        assert!(!listed_twice(&[
            email("ada@example.com"),
            email("bea@example.com")
        ]));
        assert!(listed_twice(&[
            email("ada@example.com"),
            email(" ADA@example.com")
        ]));
        // A tab or a no-break space after a pasted address: the API
        // trims it, so it is the same address here too.
        for pasted in [
            "ada@example.com\t",
            "ada@example.com\u{a0}",
            "\u{3000}ada@example.com",
        ] {
            let rows = [email("ada@example.com"), email(pasted)];
            assert!(listed_twice(&rows), "{pasted:?}");
            // And the editor's answer is the API's.
            let refused = common::person::normalize(common::person::PersonInput {
                display_name: "Ada".into(),
                emails: rows.to_vec(),
                ..Default::default()
            });
            assert_eq!(
                refused,
                Err(common::person::PersonError::DuplicateEmail),
                "{pasted:?}"
            );
        }
        // Rows nobody typed in are not "the same address".
        assert!(!listed_twice(&[email(""), email("  "), email("\t")]));
    }

    /// A mark is about what the rows hold now: it goes when the row is
    /// put right, and a "twice" mark needs a second row with the address.
    #[test]
    fn a_row_is_marked_only_while_it_is_what_the_save_was_refused_for() {
        let taken = Flagged {
            taken: vec!["ada@example.com".into()],
            twice: false,
        };
        let rows = [
            email("Ada@example.com\u{a0}"),
            email("bea@example.com"),
            email(""),
        ];
        assert!(taken.marks(&rows, 0), "the taken address, as typed");
        assert!(!taken.marks(&rows, 1));
        assert!(!taken.marks(&rows, 2));
        assert!(!taken.marks(&rows, 9), "no such row");

        let twice = Flagged {
            taken: Vec::new(),
            twice: true,
        };
        let rows = [
            email("a@example.com"),
            email("A@example.com"),
            email("b@example.com"),
            email("b@example.com "),
            email("c@example.com"),
        ];
        let marked: Vec<bool> = (0..rows.len()).map(|i| twice.marks(&rows, i)).collect();
        assert_eq!(marked, [true, true, true, true, false], "every pair");
        // One of a pair removed: the other is no longer marked.
        assert!(!twice.marks(&rows[1..3], 0));
        // Nothing is marked before a refusal.
        assert!(!Flagged::default().marks(&rows, 0));
    }
}
