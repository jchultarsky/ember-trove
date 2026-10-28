//! People (#147): the registry (`/people`), a person's card
//! (`/people/:id`), and the pieces the node section reuses (the contact
//! actions and the person form).
//!
//! These are third parties' contact details. The API only ever returns the
//! caller's own people, so nothing here has to filter. Contact values render
//! as text (never HTML) and as mailto:/tel: links; addresses as plain text.

use common::form_draft::{FormRecovery, PersonForm, PersonFormDraft};
use common::id::PersonId;
use common::person::{ContactPoint, DuplicateMatch, Person, PersonInput, PersonUsage, primary};
use leptos::prelude::*;
use leptos_router::hooks::{use_navigate, use_params_map};

use crate::api::SavePerson;
use crate::components::empty_state::EmptyState;
use crate::components::format_helpers::copy_to_clipboard;
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

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum ContactKind {
    Email,
    Phone,
    Address,
}

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
#[component]
fn ContactListEditor(kind: ContactKind, points: RwSignal<Vec<ContactPoint>>) -> impl IntoView {
    let redraw = RwSignal::new(0u32);
    // A text input with `inputmode`, not `type="email"`/`type="tel"`: those
    // types tell the browser this is the user's own address, and it offers
    // their details for someone else's card (#184).
    let (noun, heading, placeholder, input_mode) = match kind {
        ContactKind::Email => ("email", "Emails", "name@example.com", "email"),
        ContactKind::Phone => ("phone", "Phones", "+1 555 010 0000", "tel"),
        ContactKind::Address => ("address", "Addresses", "1 Main St\nSpringfield", "text"),
    };
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
        ContactKind::Phone => "cell",
        ContactKind::Address => "home",
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
                        <input
                            type="text"
                            autocomplete="off"
                            class=format!("{INPUT_CLASS} col-span-3 sm:col-span-1")
                            placeholder=label_placeholder
                            aria-label=format!("{noun} label")
                            prop:value=p.label.clone()
                            on:input=move |ev| {
                                let v = event_target_value(&ev);
                                points.update_untracked(|ps| if let Some(x) = ps.get_mut(i) { x.label = v; });
                            }
                        />
                        {if kind == ContactKind::Address {
                            // Freeform and multi-line, like the old single field.
                            view! {
                                <textarea
                                    rows="2"
                                    autocomplete="off"
                                    class=INPUT_CLASS
                                    placeholder=placeholder
                                    aria-label=noun
                                    prop:value=p.value.clone()
                                    on:input=move |ev| {
                                        let v = event_target_value(&ev);
                                        points.update_untracked(|ps| if let Some(x) = ps.get_mut(i) { x.value = v; });
                                    }
                                />
                            }.into_any()
                        } else {
                            view! {
                                <input
                                    type="text"
                                    inputmode=input_mode
                                    autocomplete="off"
                                    autocapitalize="off"
                                    spellcheck="false"
                                    class=INPUT_CLASS
                                    placeholder=placeholder
                                    aria-label=noun
                                    prop:value=p.value.clone()
                                    on:input=move |ev| {
                                        let v = event_target_value(&ev);
                                        points.update_untracked(|ps| if let Some(x) = ps.get_mut(i) { x.value = v; });
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
/// - The duplicate-email warning and the server's validation messages show
///   inside the dialog.
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
    let draft_key = StoredValue::new(use_context::<crate::auth::AuthState>().and_then(|a| {
        match a.get_untracked() {
            crate::auth::AuthStatus::Authenticated(u) => Some(form.key(&u.sub)),
            _ => None,
        }
    }));
    let open_forms = use_context::<crate::draft::OpenForms>();
    let duplicate = RwSignal::new(None::<Vec<DuplicateMatch>>);
    let error = RwSignal::new(None::<String>);
    let form_id = match target {
        EditorTarget::New => "person-editor-form-new",
        EditorTarget::Existing(_) => "person-editor-form-edit",
    };
    let position_hint_id = StoredValue::new(format!("{form_id}-position-hint"));
    let title = match target {
        EditorTarget::New => "New person",
        EditorTarget::Existing(_) => "Edit person",
    };

    // Keep the open dialog's input on this device, or drop the draft once
    // it holds nothing beyond what the dialog opened with. `try_` access:
    // this also runs while the page unmounts the editor.
    let keep = move || {
        if open.try_get_untracked() != Some(true) {
            return;
        }
        let Some(Some(key)) = draft_key.try_get_value() else {
            return;
        };
        let (Some(person), Some(base)) = (draft.try_read(), baseline.try_get_value()) else {
            return;
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
        };
        if d.is_dirty() {
            crate::draft::write_person_draft(&key, &d);
        } else {
            crate::draft::clear_form_draft(&key);
        }
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
        let Some(key) = draft_key.get_value() else {
            return;
        };
        let Some(d) = crate::draft::read_person_draft(&key) else {
            return;
        };
        match d.recovery(&initial.get_untracked()) {
            FormRecovery::Discard => crate::draft::clear_form_draft(&key),
            FormRecovery::Offer { changed } => {
                draft.load(d.person);
                baseline.set_value(d.base);
                if let Some(m) = membership {
                    m.role.set(d.role);
                    m.note.set(d.note);
                }
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
            if let Some(key) = draft_key.get_value() {
                crate::draft::clear_form_draft(&key);
            }
        }
        if is_open && was_open != Some(true) {
            duplicate.set(None);
            error.set(None);
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

    let save = move |confirm: bool| {
        if saving.get_untracked() {
            return;
        }
        let input = draft.read();
        saving.set(true);
        error.set(None);
        wasm_bindgen_futures::spawn_local(async move {
            let result = match target {
                EditorTarget::New => crate::api::create_person(&input, confirm).await,
                EditorTarget::Existing(id) => crate::api::update_person(id, &input, confirm).await,
            };
            saving.try_set(false);
            match result {
                Ok(SavePerson::Saved(p)) => {
                    open.try_set(false);
                    on_saved.run(p);
                }
                Ok(SavePerson::Duplicate(w)) => {
                    duplicate.try_set(Some(w.matches));
                }
                Err(e) => {
                    error.try_set(Some(e.to_string()));
                }
            }
        });
    };

    // Escape and the backdrop: discard, with Undo when something changed.
    let dismiss = Callback::new(move |()| {
        let typed = draft.read();
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
                    open.try_set(true);
                }),
            );
        }
    });
    let on_key = Callback::new(move |ev: web_sys::KeyboardEvent| {
        if ev.key() == "Enter" && (ev.ctrl_key() || ev.meta_key()) {
            ev.prevent_default();
            save(false);
        }
    });
    let field = move |label: &'static str, sig: RwSignal<String>| {
        view! {
            <label class="block">
                <span class=LABEL_CLASS>{label}</span>
                <input type="text" autocomplete="off" class=INPUT_CLASS prop:value=move || sig.get()
                    on:input=move |ev| sig.set(event_target_value(&ev)) />
            </label>
        }
    };
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
            // user: browser autofill stays out of all of them (#184).
            <form
                id=form_id
                autocomplete="off"
                class="flex-1 overflow-y-auto px-4 py-4 space-y-4"
                data-testid="person-form"
                on:submit=move |ev| { ev.prevent_default(); save(false); }
                // Every field's `input` and the radios' `change` bubble here.
                on:input=move |_| keep()
                on:change=move |_| keep()
            >
                {move || recovered.get().map(|changed| view! {
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
                })}
                {move || error.get().map(|e| view! {
                    <p class="text-sm text-red-600 dark:text-red-400" role="alert">{e}</p>
                })}
                {move || duplicate.get().map(|matches| view! {
                    <DuplicateNotice matches=matches proceed_label="Save anyway"
                        on_proceed=Callback::new(move |()| save(true)) />
                })}
                <label class="block">
                    <span class=LABEL_CLASS>"Name"</span>
                    <input type="text" node_ref=name_ref class=INPUT_CLASS required=true autocomplete="off"
                        prop:value=move || draft.name.get()
                        on:input=move |ev| draft.name.set(event_target_value(&ev)) />
                </label>
                <div class="grid grid-cols-1 sm:grid-cols-2 gap-3">
                    {field("Company", draft.company)}
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
                    </label>
                </div>
                <ContactListEditor kind=ContactKind::Email points=draft.emails />
                <ContactListEditor kind=ContactKind::Phone points=draft.phones />
                <ContactListEditor kind=ContactKind::Address points=draft.addresses />
                <label class="block">
                    <span class=LABEL_CLASS>"Notes"</span>
                    <textarea rows="4" class=INPUT_CLASS autocomplete="off" prop:value=move || draft.notes.get()
                        on:input=move |ev| draft.notes.set(event_target_value(&ev)) />
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

/// The "already in your registry" warning: links to the matches, and a way
/// to create anyway.
#[component]
pub fn DuplicateNotice(
    matches: Vec<DuplicateMatch>,
    /// "Create a new person anyway" / "Save anyway".
    proceed_label: &'static str,
    #[prop(into)] on_proceed: Callback<()>,
) -> impl IntoView {
    let navigate = StoredValue::new(use_navigate());
    view! {
        <div class="rounded-lg border border-amber-300 dark:border-amber-700 bg-amber-50 dark:bg-amber-900/20 p-3 text-sm space-y-2"
            role="alert" data-testid="duplicate-warning">
            <p class="text-stone-700 dark:text-stone-200">
                "Someone in your registry already has this email:"
            </p>
            <ul class="space-y-1">
                {matches.into_iter().map(|m| {
                    let id = m.id;
                    view! {
                        <li>
                            <button type="button" class="text-amber-700 dark:text-amber-400 hover:underline cursor-pointer"
                                on:click=move |_| navigate.get_value()(&format!("/people/{id}"), Default::default())>
                                {m.display_name}
                            </button>
                            <span class="text-stone-400">{format!(" · {}", m.email)}</span>
                        </li>
                    }
                }).collect_view()}
            </ul>
            <button type="button" class="text-xs text-stone-600 dark:text-stone-300 underline cursor-pointer"
                on:click=move |_| on_proceed.run(())>
                {proceed_label}
            </button>
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
            <div class="text-sm font-medium text-stone-900 dark:text-stone-100">{person.display_name.clone()}</div>
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
                Ok(()) => navigate.get_value()("/people", Default::default()),
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
                            Err(_) => ().into_any(),
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
