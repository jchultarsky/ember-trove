//! The People section on a node page (#147): who is involved here, and in
//! what role.
//!
//! What you see depends on whose person it is (the API redacts): the
//! person's owner gets the card (company, position, preferred email/phone
//! as actions) and the context note; a sharee gets name and role only.
//! Only the node's own owner can add, edit or remove, and the picker offers
//! only their own registry.

use common::form_draft::PersonForm;
use common::id::{NodeId, PersonId};
use common::person::{
    CARD_CONTACTS, MembershipInput, MembershipUpdate, MembershipView, Person, PersonInput,
    card_contacts, matches,
};
use leptos::prelude::*;
use leptos_router::hooks::use_navigate;

use crate::components::icon_button::{IconButton, IconButtonVariant, icon_button_class};
use crate::components::node_section::{NodeSection, SectionShell};
use crate::components::people_view::{ContactAction, ContactKind, HINT_CLASS, PersonEditor};
use crate::components::toast::{ToastLevel, push_toast};

const INPUT_CLASS: &str = "w-full text-xs bg-stone-100 dark:bg-stone-800 text-stone-800 \
     dark:text-stone-200 rounded-lg px-2 py-1.5 focus:outline-none focus:ring-1 focus:ring-amber-500";

/// What is typed for a membership (its role and note), as opposed to the
/// person's own card. Shared by the row editor and both attach paths
/// (#162), so the role is captured when it's known: at attach time.
#[derive(Clone, Copy)]
pub struct MembershipDraft {
    pub role: RwSignal<String>,
    pub note: RwSignal<String>,
    /// The owner's roles, most used first, for suggestions.
    pub roles: Signal<Vec<String>>,
}

impl MembershipDraft {
    pub fn new(roles: Signal<Vec<String>>) -> Self {
        Self {
            role: RwSignal::new(String::new()),
            note: RwSignal::new(String::new()),
            roles,
        }
    }

    pub fn clear(&self) {
        self.role.set(String::new());
        self.note.set(String::new());
    }

    pub fn is_blank(&self) -> bool {
        self.role.with_untracked(|r| r.trim().is_empty())
            && self.note.with_untracked(|n| n.trim().is_empty())
    }

    pub fn input(&self, person_id: PersonId) -> MembershipInput {
        MembershipInput::from_typed(
            person_id,
            &self.role.get_untracked(),
            &self.note.get_untracked(),
        )
    }
}

/// The Role (with suggestions) and Note fields of a membership.
///
/// The Role is an APG combobox of our own, not an `<input list>` (#171):
/// the browser's datalist could commit a suggestion's spelling ("abc") over
/// what was typed ("ABC") and swallowed select-all. Here the typed text is
/// what's saved; a suggestion is committed only when it's chosen, by click
/// or by ↑/↓ then Enter, and it then shows in the field before any save.
/// Every other key is left to the input.
#[component]
pub fn MembershipFields(
    draft: MembershipDraft,
    /// Focus the Role field on mount.
    #[prop(optional)]
    autofocus: bool,
) -> impl IntoView {
    let role_ref: NodeRef<leptos::html::Input> = NodeRef::new();
    Effect::new(move |_| {
        if autofocus && let Some(el) = role_ref.get() {
            let _ = el.focus();
        }
    });
    let MembershipDraft { role, note, roles } = draft;
    let base = format!(
        "role-combo-{}",
        NEXT_ID.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
    );
    let list_id = StoredValue::new(format!("{base}-list"));
    let hint_id = StoredValue::new(format!("{base}-hint"));
    let opt_id = StoredValue::new(move |i: usize| format!("{base}-opt-{i}"));
    // Shown while typing; closed by a choice, Escape or leaving the field.
    let listing = RwSignal::new(false);
    // Only the arrow keys set this, never typing or hover: Enter commits an
    // option only when the user moved onto it.
    let active = RwSignal::new(None::<usize>);
    let options = Memo::new(move |_| {
        roles.with(|r| common::picker::free_text_suggestions(&role.get(), r, 8))
    });
    let expanded = move || listing.get() && options.with(|o| !o.is_empty());
    let close = move || {
        listing.set(false);
        active.set(None);
    };
    let choose = move |i: usize| {
        let picked = options.with_untracked(|o| {
            common::picker::free_text_commit(&role.get_untracked(), Some(i), o).to_string()
        });
        role.set(picked);
        close();
    };
    let on_keydown = move |ev: leptos::ev::KeyboardEvent| match ev.key().as_str() {
        "ArrowDown" | "ArrowUp" => {
            let len = options.with_untracked(Vec::len);
            if len > 0 {
                ev.prevent_default();
                listing.set(true);
                active.set(common::picker::step(
                    active.get_untracked(),
                    len,
                    ev.key() == "ArrowDown",
                ));
            }
        }
        // Plain Enter on a chosen option picks it; otherwise it falls through
        // to the form (attach / save) with the typed text.
        "Enter" if !(ev.ctrl_key() || ev.meta_key()) => {
            if listing.get_untracked()
                && let Some(i) = active.get_untracked()
            {
                ev.prevent_default();
                ev.stop_propagation();
                choose(i);
            }
        }
        // Closing the list is this Escape's whole job: stopped, so the
        // form, dialog or page doesn't also act on it.
        "Escape" if listing.get_untracked() && options.with_untracked(|o| !o.is_empty()) => {
            ev.prevent_default();
            ev.stop_propagation();
            close();
        }
        _ => {}
    };
    view! {
        <div>
            <input type="text" node_ref=role_ref class=INPUT_CLASS
                placeholder="Role here (sponsor, approver\u{2026})"
                role="combobox" aria-label="Role" aria-autocomplete="list"
                aria-controls=list_id.get_value()
                aria-describedby=hint_id.get_value()
                aria-expanded=move || if expanded() { "true" } else { "false" }
                aria-activedescendant=move || active.get().map(|i| opt_id.get_value()(i))
                autocomplete="off"
                prop:value=move || role.get()
                on:input=move |ev| {
                    role.set(event_target_value(&ev));
                    active.set(None);
                    listing.set(true);
                }
                on:click=move |_| listing.set(true)
                on:blur=move |_| close()
                on:keydown=on_keydown />
            // In the flow, not floating: an open list never covers the
            // fields below it.
            <div id=list_id.get_value() role="listbox" aria-label="Role suggestions"
                class="mt-1 max-h-40 overflow-y-auto"
                class:hidden=move || !expanded()>
                {move || expanded().then(|| options.get().into_iter().enumerate().map(|(i, o)| view! {
                    <div
                        id=opt_id.get_value()(i)
                        role="option"
                        aria-selected=move || if active.get() == Some(i) { "true" } else { "false" }
                        class=move || format!(
                            "px-2 py-1 rounded-lg text-xs cursor-pointer text-stone-700 \
                             dark:text-stone-300 hover:bg-stone-100 dark:hover:bg-stone-800 {}",
                            if active.get() == Some(i) { "bg-amber-50 dark:bg-stone-800" } else { "" }
                        )
                        // mousedown, not click: it lands before the input's blur.
                        on:mousedown=move |ev: web_sys::MouseEvent| {
                            ev.prevent_default();
                            if common::picker::is_pointer_press(ev.detail()) { choose(i); }
                        }
                        // An accessibility press is a bare click (#174).
                        on:click=move |ev: web_sys::MouseEvent| {
                            if !common::picker::is_pointer_press(ev.detail()) { choose(i); }
                        }
                    >
                        {o}
                    </div>
                }).collect_view())}
            </div>
            // The membership's half of Position vs Role (#147, #182).
            <span id=hint_id.get_value() class=HINT_CLASS>
                "Their part in this node only."
            </span>
        </div>
        <textarea rows="2" class=INPUT_CLASS placeholder="Note about them on this node\u{2026}"
            aria-label="Note"
            prop:value=move || note.get() on:input=move |ev| note.set(event_target_value(&ev)) />
    }
}

#[component]
pub fn NodePeoplePanel(
    node_id: NodeId,
    /// The node's canonical owner (not an editor, not the admin): the only
    /// one the API lets change its people.
    can_edit: bool,
) -> impl IntoView {
    let open = RwSignal::new(false);
    let refresh = RwSignal::new(0u32);
    let adding = RwSignal::new(false);
    let members = LocalResource::new(move || {
        refresh.track();
        crate::api::fetch_node_people(node_id)
    });
    // Role suggestions: the owner's roles, most used first.
    let roles = LocalResource::new(move || async move {
        if can_edit {
            crate::api::fetch_role_suggestions()
                .await
                .unwrap_or_default()
        } else {
            Vec::new()
        }
    });
    let role_names = Signal::derive(move || {
        roles
            .get()
            .unwrap_or_default()
            .into_iter()
            .map(|r| r.role)
            .collect::<Vec<_>>()
    });

    let count = Signal::derive(move || members.with(|r| r.as_ref()?.as_ref().ok().map(Vec::len)));
    let actions: ChildrenFn = std::sync::Arc::new(move || {
        view! {
            {move || (open.get() && can_edit).then(|| view! {
                <button
                    type="button"
                    class=icon_button_class(IconButtonVariant::Neutral)
                    title=move || if adding.get() { "Cancel" } else { "Add person" }
                    aria-label=move || if adding.get() { "Cancel" } else { "Add person" }
                    on:click=move |_| adding.update(|v| *v = !*v)
                >
                    <span class="material-symbols-outlined" style="font-size: 18px;" aria-hidden="true">
                        {move || if adding.get() { "close" } else { "person_add" }}
                    </span>
                </button>
            })}
        }
        .into_any()
    });

    let on_added = Callback::new(move |()| {
        adding.set(false);
        refresh.update(|n| *n += 1);
    });
    // "Create “…”" opens the shared editor with the typed name; saving the
    // new person also adds them here.
    let editor_open = RwSignal::new(false);
    let prefill = RwSignal::new(String::new());
    // The editor's "On this node" block: the membership, saved in the same
    // motion as the person (#162).
    let new_membership = MembershipDraft::new(role_names);
    let on_create = Callback::new(move |name: String| {
        prefill.set(name);
        editor_open.set(true);
    });
    let on_created = Callback::new(move |p: Person| {
        let input = new_membership.input(p.id);
        wasm_bindgen_futures::spawn_local(async move {
            match crate::api::add_node_person(node_id, &input).await {
                Ok(_) => on_added.run(()),
                Err(e) => push_toast(ToastLevel::Error, format!("Couldn't add: {e}")),
            }
        });
    });

    view! {
        <SectionShell section=NodeSection::People open=open count=count actions=actions>
            <PersonEditor open=editor_open form=PersonForm::NewOnNode(node_id)
                initial=Signal::derive(move || PersonInput { display_name: prefill.get(), ..Default::default() })
                membership=new_membership on_saved=on_created />
            {move || adding.get().then(|| {
                let taken: Vec<_> = members
                    .get()
                    .and_then(Result::ok)
                    .unwrap_or_default()
                    .into_iter()
                    .map(|m| m.person_id)
                    .collect();
                view! { <PersonPicker node_id=node_id taken=taken roles=role_names
                    on_added=on_added on_create=on_create /> }
            })}
            <Suspense fallback=|| view! { <p class="text-xs text-stone-400">"Loading\u{2026}"</p> }>
                {move || {
                    members.get().map(move |r| match r {
                        Ok(list) if list.is_empty() => view! {
                            <p class="text-xs text-stone-400 dark:text-stone-600">
                                {if can_edit { "No one yet. Add people from your registry." } else { "No one yet." }}
                            </p>
                        }.into_any(),
                        Ok(list) => view! {
                            <ul class="divide-y divide-stone-100 dark:divide-stone-800" data-testid="node-people-list">
                                {list.into_iter().map(|m| view! {
                                    <MemberRow node_id=node_id member=m can_edit=can_edit
                                        roles=role_names
                                        on_change=Callback::new(move |()| refresh.update(|n| *n += 1)) />
                                }).collect_view()}
                            </ul>
                        }.into_any(),
                        Err(e) => view! { <p class="text-xs text-red-500">{format!("Error: {e}")}</p> }.into_any(),
                    })
                }}
            </Suspense>
        </SectionShell>
    }
}

#[component]
fn MemberRow(
    node_id: NodeId,
    member: MembershipView,
    can_edit: bool,
    roles: Signal<Vec<String>>,
    #[prop(into)] on_change: Callback<()>,
) -> impl IntoView {
    let navigate = StoredValue::new(use_navigate());
    let id = member.id;
    let editing = RwSignal::new(false);
    let draft = MembershipDraft {
        role: RwSignal::new(member.role.clone().unwrap_or_default()),
        note: RwSignal::new(member.note.clone().unwrap_or_default()),
        roles,
    };
    let MembershipDraft { role, note, .. } = draft;
    let person_id = member.person_id;
    let card = member.person.clone();
    let subtitle = card
        .as_ref()
        .map(|p| {
            [p.position.clone(), p.company.clone()]
                .into_iter()
                .flatten()
                .collect::<Vec<_>>()
                .join(" \u{b7} ")
        })
        .unwrap_or_default();
    // Every email and phone, preferred first, up to CARD_CONTACTS each; the
    // rest are counted in a "+n more" link to the person page (#172).
    let (emails, more_emails) = card
        .as_ref()
        .map(|p| card_contacts(&p.emails, CARD_CONTACTS))
        .unwrap_or_default();
    let (phones, more_phones) = card
        .as_ref()
        .map(|p| card_contacts(&p.phones, CARD_CONTACTS))
        .unwrap_or_default();
    let more = more_emails + more_phones;
    let owns_card = card.is_some();

    let save = move || {
        let req = MembershipUpdate {
            role: Some(role.get_untracked()),
            note: Some(note.get_untracked()),
        };
        wasm_bindgen_futures::spawn_local(async move {
            match crate::api::update_node_person(node_id, id, &req).await {
                Ok(_) => {
                    editing.set(false);
                    on_change.run(());
                }
                Err(e) => push_toast(ToastLevel::Error, format!("Couldn't save: {e}")),
            }
        });
    };
    let remove = move || {
        wasm_bindgen_futures::spawn_local(async move {
            match crate::api::remove_node_person(node_id, id).await {
                Ok(()) => on_change.run(()),
                Err(e) => push_toast(ToastLevel::Error, format!("Couldn't remove: {e}")),
            }
        });
    };

    view! {
        <li class="py-2 flex items-start gap-3" data-testid="node-person">
            <span class="material-symbols-outlined text-stone-300 dark:text-stone-600 mt-0.5" style="font-size:18px;" aria-hidden="true">"person"</span>
            <div class="flex-1 min-w-0 space-y-0.5">
                <div class="flex flex-wrap items-baseline gap-x-2">
                    {if owns_card {
                        view! {
                            <button type="button" class="text-sm font-medium text-stone-900 dark:text-stone-100 hover:text-amber-700 cursor-pointer"
                                on:click=move |_| navigate.get_value()(&format!("/people/{person_id}"), Default::default())>
                                {member.display_name.clone()}
                            </button>
                        }.into_any()
                    } else {
                        view! { <span class="text-sm font-medium text-stone-900 dark:text-stone-100">{member.display_name.clone()}</span> }.into_any()
                    }}
                    {move || (!editing.get()).then(|| {
                        let r = role.get();
                        (!r.is_empty()).then(|| view! {
                            <span class="text-xs px-1.5 py-0.5 rounded-full bg-stone-100 dark:bg-stone-800 text-stone-600 dark:text-stone-300"
                                data-testid="member-role">{r}</span>
                        })
                    })}
                </div>
                {(!subtitle.is_empty()).then(|| view! { <div class="text-xs text-stone-500">{subtitle}</div> })}
                <div class="flex flex-wrap gap-x-3">
                    {emails.into_iter().map(|e| view! { <ContactAction kind=ContactKind::Email point=e /> }).collect_view()}
                    {phones.into_iter().map(|p| view! { <ContactAction kind=ContactKind::Phone point=p /> }).collect_view()}
                    {(more > 0).then(|| view! {
                        <button type="button" class="text-xs text-stone-500 hover:text-amber-700 hover:underline cursor-pointer"
                            data-testid="member-more-contacts"
                            on:click=move |_| navigate.get_value()(&format!("/people/{person_id}"), Default::default())>
                            {format!("+{more} more")}
                        </button>
                    })}
                </div>
                {move || (!editing.get()).then(|| {
                    let n = note.get();
                    (!n.is_empty()).then(|| view! { <p class="text-xs text-stone-500 whitespace-pre-line">{n}</p> })
                })}
                {move || editing.get().then(|| view! {
                    <form class="space-y-1.5 pt-1" on:submit=move |ev| { ev.prevent_default(); save(); }>
                        <MembershipFields draft=draft />
                        // ✓ / ✕, like the task editors' inline Save and Cancel.
                        <div class="flex gap-1">
                            <IconButton icon="check" label="Save" variant=IconButtonVariant::Save
                                on_click=Callback::new(move |()| save()) />
                            <IconButton icon="close" label="Cancel"
                                on_click=Callback::new(move |()| editing.set(false)) />
                        </div>
                    </form>
                })}
            </div>
            {(can_edit).then(|| view! {
                <div class="flex items-center gap-0.5 shrink-0">
                    <IconButton icon="edit" label="Edit role and note"
                        on_click=Callback::new(move |()| editing.update(|v| *v = !*v)) />
                    <IconButton icon="person_remove" label="Remove from this node"
                        variant=IconButtonVariant::Danger on_click=Callback::new(move |()| remove()) />
                </div>
            })}
        </li>
    }
}

/// Unique DOM ids per picker instance.
static NEXT_ID: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);

/// One row of the person picker.
#[derive(Clone, PartialEq)]
enum Pick {
    Existing(Person),
    Create(String),
}

/// Picker-first add: type-ahead over your registry (most people recur
/// across nodes), with "Create “…”" when there's no match. An APG combobox:
/// ↑/↓ move, Enter picks, Escape closes (stopped, so the page's Escape
/// doesn't also run).
#[component]
fn PersonPicker(
    node_id: NodeId,
    /// People already on the node (not offered again).
    taken: Vec<PersonId>,
    roles: Signal<Vec<String>>,
    #[prop(into)] on_added: Callback<()>,
    /// "Create “…”": open the person editor with this name.
    #[prop(into)]
    on_create: Callback<String>,
) -> impl IntoView {
    let base = format!(
        "person-picker-{}",
        NEXT_ID.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
    );
    let list_id = StoredValue::new(format!("{base}-list"));
    let opt_id = StoredValue::new(move |i: usize| format!("{base}-opt-{i}"));
    let query = RwSignal::new(String::new());
    let registry = LocalResource::new(|| crate::api::fetch_people(None));
    let taken = StoredValue::new(taken);
    let options = Memo::new(move |_| {
        let q = query.get();
        let people = registry.get().and_then(Result::ok).unwrap_or_default();
        let mut out: Vec<Pick> = people
            .into_iter()
            .filter(|p| !taken.with_value(|t| t.contains(&p.id)) && matches(p, &q))
            .take(8)
            .map(Pick::Existing)
            .collect();
        let name = q.trim().to_string();
        let exact = out
            .iter()
            .any(|o| matches!(o, Pick::Existing(p) if p.display_name.eq_ignore_ascii_case(&name)));
        if !name.is_empty() && !exact {
            out.push(Pick::Create(name));
        }
        out
    });
    let active = RwSignal::new(None::<usize>);
    Effect::new(move |_| {
        let empty = options.with(Vec::is_empty);
        active.set((!empty).then_some(0));
    });
    let busy = RwSignal::new(false);
    // An existing person, picked: the optional Role and Note come next,
    // before attaching (#162). Blank attaches exactly as before.
    let chosen = RwSignal::new(None::<Person>);
    let membership = MembershipDraft::new(roles);
    let input_ref: NodeRef<leptos::html::Input> = NodeRef::new();
    Effect::new(move |_| {
        if let Some(el) = input_ref.get() {
            let _ = el.focus();
        }
    });

    let pick = move |i: usize| {
        let Some(choice) = options.with_untracked(|o| o.get(i).cloned()) else {
            return;
        };
        match choice {
            // A new person goes through the full editor first.
            Pick::Create(name) => on_create.run(name),
            Pick::Existing(p) => {
                membership.clear();
                chosen.set(Some(p));
            }
        }
    };
    let attach = move || {
        let Some(person) = chosen.get_untracked() else {
            return;
        };
        if busy.get_untracked() {
            return;
        }
        busy.set(true);
        let input = membership.input(person.id);
        wasm_bindgen_futures::spawn_local(async move {
            let result = crate::api::add_node_person(node_id, &input).await;
            busy.try_set(false);
            match result {
                Ok(_) => on_added.run(()),
                Err(e) => push_toast(ToastLevel::Error, format!("Couldn't add: {e}")),
            }
        });
    };

    let on_keydown = move |ev: leptos::ev::KeyboardEvent| match ev.key().as_str() {
        "ArrowDown" | "ArrowUp" => {
            ev.prevent_default();
            let len = options.with_untracked(Vec::len);
            active.set(common::picker::step(
                active.get_untracked(),
                len,
                ev.key() == "ArrowDown",
            ));
        }
        "Enter" => {
            ev.prevent_default();
            ev.stop_propagation();
            if let Some(i) = active.get_untracked() {
                pick(i);
            }
        }
        "Escape" => {
            ev.prevent_default();
            ev.stop_propagation();
            on_added.run(());
        }
        _ => {}
    };

    // Back to the search: it remounts, with the query kept, and the focus
    // effect above puts the caret back in it.
    let back = move || chosen.set(None);
    // Escape steps back to the search; Ctrl/⌘+Enter attaches from the note.
    let on_confirm_key = move |ev: leptos::ev::KeyboardEvent| match ev.key().as_str() {
        "Escape" => {
            ev.prevent_default();
            ev.stop_propagation();
            back();
        }
        "Enter" if ev.ctrl_key() || ev.meta_key() => {
            ev.prevent_default();
            attach();
        }
        _ => {}
    };

    view! {
        <Show when=move || chosen.with(Option::is_some)>
            <form class="mb-2 space-y-1.5 rounded-lg border border-stone-200 dark:border-stone-800 p-2"
                data-testid="person-attach"
                on:submit=move |ev| { ev.prevent_default(); attach(); }
                on:keydown=on_confirm_key>
                <div class="flex items-center gap-2 text-xs text-stone-700 dark:text-stone-300">
                    <span class="material-symbols-outlined text-stone-400" style="font-size:14px;" aria-hidden="true">"person"</span>
                    <span class="font-medium truncate">{move || chosen.with(|c| c.as_ref().map(|p| p.display_name.clone()))}</span>
                    <span class="ml-auto text-stone-400">"On this node (optional)"</span>
                </div>
                <MembershipFields draft=membership autofocus=true />
                <div class="flex justify-end gap-2">
                    <button type="button"
                        class="px-2 py-1 rounded-lg text-xs text-stone-600 dark:text-stone-300 hover:bg-stone-100 dark:hover:bg-stone-800 cursor-pointer"
                        on:click=move |_| back()>
                        "Back"
                    </button>
                    // A form's commit button: icon + word.
                    <button type="submit"
                        class="px-2.5 py-1 rounded-lg text-xs font-medium bg-ember hover:bg-ember-strong text-white
                            disabled:opacity-50 disabled:cursor-not-allowed cursor-pointer flex items-center gap-1"
                        disabled=move || busy.get()>
                        <span class="material-symbols-outlined" style="font-size: 14px;" aria-hidden="true">"person_add"</span>
                        "Add"
                    </button>
                </div>
            </form>
        </Show>
        <Show when=move || chosen.with(Option::is_none)>
        <div class="relative mb-2" data-testid="person-picker">
            <input
                node_ref=input_ref
                type="text"
                role="combobox"
                aria-label="Find or create a person"
                // A person's name, not the user's: no autofill (#184).
                autocomplete="off"
                aria-autocomplete="list"
                aria-controls=list_id.get_value()
                aria-expanded=move || if options.with(Vec::is_empty) { "false" } else { "true" }
                aria-activedescendant=move || active.get().map(|i| opt_id.get_value()(i))
                placeholder="Find or create a person\u{2026}"
                class=INPUT_CLASS
                prop:value=move || query.get()
                on:input=move |ev| query.set(event_target_value(&ev))
                on:keydown=on_keydown
            />
            <div id=list_id.get_value() role="listbox" aria-label="People" class="mt-1 max-h-56 overflow-y-auto">
                {move || options.get().into_iter().enumerate().map(|(i, o)| {
                    let (label, detail, icon) = match &o {
                        Pick::Existing(p) => (
                            p.display_name.clone(),
                            p.company.clone().unwrap_or_default(),
                            "person",
                        ),
                        Pick::Create(name) => (format!("Create \u{201c}{name}\u{201d}"), String::new(), "person_add"),
                    };
                    view! {
                        <div
                            id=opt_id.get_value()(i)
                            role="option"
                            aria-selected=move || if active.get() == Some(i) { "true" } else { "false" }
                            class=move || format!(
                                "flex items-center gap-2 px-2 py-1.5 rounded-lg text-xs cursor-pointer \
                                 text-stone-700 dark:text-stone-300 {}",
                                if active.get() == Some(i) { "bg-amber-50 dark:bg-stone-800" } else { "" }
                            )
                            on:mouseenter=move |_| active.set(Some(i))
                            on:mousedown=move |ev: web_sys::MouseEvent| {
                                ev.prevent_default();
                                if common::picker::is_pointer_press(ev.detail()) { pick(i); }
                            }
                            // An accessibility press is a bare click (#174).
                            on:click=move |ev: web_sys::MouseEvent| {
                                if !common::picker::is_pointer_press(ev.detail()) { pick(i); }
                            }
                        >
                            <span class="material-symbols-outlined text-stone-400" style="font-size:14px;" aria-hidden="true">{icon}</span>
                            <span class="truncate">{label}</span>
                            <span class="ml-auto text-stone-400 truncate">{detail}</span>
                        </div>
                    }
                }).collect_view()}
            </div>
        </div>
        </Show>
    }
}
