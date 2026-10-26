//! The People section on a node page (#147): who is involved here, and in
//! what role.
//!
//! What you see depends on whose person it is (the API redacts): the
//! person's owner gets the card (company, position, preferred email/phone
//! as actions) and the context note; a sharee gets name and role only.
//! Only the node's own owner can add, edit or remove, and the picker offers
//! only their own registry.

use common::id::NodeId;
use common::person::{
    MembershipInput, MembershipUpdate, MembershipView, Person, PersonInput, matches, primary,
};
use leptos::prelude::*;
use leptos_router::hooks::use_navigate;

use crate::components::icon_button::{IconButton, IconButtonVariant};
use crate::components::node_section::{NodeSection, SectionShell};
use crate::components::people_view::{ContactAction, ContactKind, EditorTarget, PersonEditor};
use crate::components::toast::{ToastLevel, push_toast};

const INPUT_CLASS: &str = "w-full text-xs bg-stone-100 dark:bg-stone-800 text-stone-800 \
     dark:text-stone-200 rounded-lg px-2 py-1.5 focus:outline-none focus:ring-1 focus:ring-amber-500";

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
    // Copy, so the section's child closures can all use it.
    let roles_list_id = StoredValue::new(format!("people-roles-{node_id}"));

    let count = Signal::derive(move || members.with(|r| r.as_ref()?.as_ref().ok().map(Vec::len)));
    let actions: ChildrenFn = std::sync::Arc::new(move || {
        view! {
            {move || (open.get() && can_edit).then(|| view! {
                <button
                    type="button"
                    class="p-1.5 rounded-lg text-stone-400 hover:text-stone-600 dark:hover:text-stone-300
                        hover:bg-stone-100 dark:hover:bg-stone-800 transition-colors cursor-pointer"
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
    let on_create = Callback::new(move |name: String| {
        prefill.set(name);
        editor_open.set(true);
    });
    let on_created = Callback::new(move |p: Person| {
        wasm_bindgen_futures::spawn_local(async move {
            let input = MembershipInput {
                person_id: p.id,
                role: None,
                note: None,
            };
            match crate::api::add_node_person(node_id, &input).await {
                Ok(_) => on_added.run(()),
                Err(e) => push_toast(ToastLevel::Error, format!("Couldn't add: {e}")),
            }
        });
    });

    view! {
        <SectionShell section=NodeSection::People open=open count=count actions=actions>
            <PersonEditor open=editor_open target=EditorTarget::New
                initial=Signal::derive(move || PersonInput { display_name: prefill.get(), ..Default::default() })
                on_saved=on_created />
            <datalist id=roles_list_id.get_value()>
                {move || roles.get().unwrap_or_default().into_iter().map(|r| view! {
                    <option value=r.role />
                }).collect_view()}
            </datalist>
            {move || adding.get().then(|| {
                let taken: Vec<_> = members
                    .get()
                    .and_then(Result::ok)
                    .unwrap_or_default()
                    .into_iter()
                    .map(|m| m.person_id)
                    .collect();
                view! { <PersonPicker node_id=node_id taken=taken on_added=on_added on_create=on_create /> }
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
                                        roles_list_id=roles_list_id.get_value()
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
    roles_list_id: String,
    #[prop(into)] on_change: Callback<()>,
) -> impl IntoView {
    let navigate = StoredValue::new(use_navigate());
    let id = member.id;
    let editing = RwSignal::new(false);
    let role = RwSignal::new(member.role.clone().unwrap_or_default());
    let note = RwSignal::new(member.note.clone().unwrap_or_default());
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
    let email = card.as_ref().and_then(|p| primary(&p.emails).cloned());
    let phone = card.as_ref().and_then(|p| primary(&p.phones).cloned());
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
                    {email.map(|e| view! { <ContactAction kind=ContactKind::Email point=e /> })}
                    {phone.map(|p| view! { <ContactAction kind=ContactKind::Phone point=p /> })}
                </div>
                {move || (!editing.get()).then(|| {
                    let n = note.get();
                    (!n.is_empty()).then(|| view! { <p class="text-xs text-stone-500 whitespace-pre-line">{n}</p> })
                })}
                {move || editing.get().then(|| view! {
                    <form class="space-y-1.5 pt-1" on:submit=move |ev| { ev.prevent_default(); save(); }>
                        <input type="text" class=INPUT_CLASS placeholder="Role here (sponsor, approver\u{2026})"
                            aria-label="Role" list=roles_list_id.clone()
                            prop:value=move || role.get() on:input=move |ev| role.set(event_target_value(&ev)) />
                        <textarea rows="2" class=INPUT_CLASS placeholder="Note about them on this node\u{2026}"
                            aria-label="Note"
                            prop:value=move || note.get() on:input=move |ev| note.set(event_target_value(&ev)) />
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
                    <button type="button" class="p-1 rounded text-stone-400 hover:text-stone-600 cursor-pointer"
                        title="Edit role and note" aria-label="Edit role and note"
                        on:click=move |_| editing.update(|v| *v = !*v)>
                        <span class="material-symbols-outlined" style="font-size:16px;" aria-hidden="true">"edit"</span>
                    </button>
                    <button type="button" class="p-1 rounded text-stone-400 hover:text-red-500 cursor-pointer"
                        title="Remove from this node" aria-label="Remove from this node"
                        on:click=move |_| remove()>
                        <span class="material-symbols-outlined" style="font-size:16px;" aria-hidden="true">"person_remove"</span>
                    </button>
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
    taken: Vec<common::id::PersonId>,
    #[prop(into)] on_added: Callback<()>,
    /// "Create “…”": open the person editor with this name.
    #[prop(into)]
    on_create: Callback<String>,
) -> impl IntoView {
    let base = format!(
        "person-picker-{}",
        NEXT_ID.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
    );
    let list_id = format!("{base}-list");
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
        let person_id = match choice {
            // A new person goes through the full editor first.
            Pick::Create(name) => {
                on_create.run(name);
                return;
            }
            Pick::Existing(p) => p.id,
        };
        if busy.get_untracked() {
            return;
        }
        busy.set(true);
        wasm_bindgen_futures::spawn_local(async move {
            let result = crate::api::add_node_person(
                node_id,
                &MembershipInput {
                    person_id,
                    role: None,
                    note: None,
                },
            )
            .await;
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

    let list_id_attr = list_id.clone();
    view! {
        <div class="relative mb-2" data-testid="person-picker">
            <input
                node_ref=input_ref
                type="text"
                role="combobox"
                aria-label="Find or create a person"
                aria-autocomplete="list"
                aria-controls=list_id_attr
                aria-expanded="true"
                aria-activedescendant=move || active.get().map(|i| opt_id.get_value()(i))
                placeholder="Find or create a person\u{2026}"
                class=INPUT_CLASS
                prop:value=move || query.get()
                on:input=move |ev| query.set(event_target_value(&ev))
                on:keydown=on_keydown
            />
            <div id=list_id role="listbox" aria-label="People" class="mt-1 max-h-56 overflow-y-auto">
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
                            on:mousedown=move |ev: web_sys::MouseEvent| { ev.prevent_default(); pick(i); }
                        >
                            <span class="material-symbols-outlined text-stone-400" style="font-size:14px;" aria-hidden="true">{icon}</span>
                            <span class="truncate">{label}</span>
                            <span class="ml-auto text-stone-400 truncate">{detail}</span>
                        </div>
                    }
                }).collect_view()}
            </div>
        </div>
    }
}
