//! People (#147): the registry (`/people`), a person's card
//! (`/people/:id`), and the pieces the node section reuses (the contact
//! actions and the person form).
//!
//! These are third parties' contact details. The API only ever returns the
//! caller's own people, so nothing here has to filter. Contact values render
//! as text (never HTML) and as mailto:/tel: links.

use common::id::PersonId;
use common::person::{ContactPoint, DuplicateMatch, Person, PersonInput, PersonUsage, primary};
use leptos::prelude::*;
use leptos_router::hooks::{use_navigate, use_params_map};

use crate::api::SavePerson;
use crate::components::empty_state::EmptyState;
use crate::components::format_helpers::copy_to_clipboard;
use crate::components::page_header::PageHeader;
use crate::components::toast::{ToastLevel, push_toast};

const INPUT_CLASS: &str = "w-full text-sm bg-stone-100 dark:bg-stone-800 text-stone-800 \
     dark:text-stone-200 rounded-lg px-2.5 py-1.5 focus:outline-none focus:ring-1 \
     focus:ring-amber-500";
const LABEL_CLASS: &str = "block text-xs font-medium text-stone-500 dark:text-stone-400 mb-1";

/// `mailto:` / `tel:` href for a contact value. Phone links keep only what
/// a dialer reads.
fn contact_href(kind: ContactKind, value: &str) -> String {
    match kind {
        ContactKind::Email => format!("mailto:{value}"),
        ContactKind::Phone => format!(
            "tel:{}",
            value
                .chars()
                .filter(|c| c.is_ascii_digit() || *c == '+')
                .collect::<String>()
        ),
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum ContactKind {
    Email,
    Phone,
}

/// One email or phone as actions: the value is a mailto:/tel: link, and a
/// button copies it.
#[component]
pub fn ContactAction(kind: ContactKind, point: ContactPoint) -> impl IntoView {
    let href = contact_href(kind, &point.value);
    let value = point.value.clone();
    let icon = match kind {
        ContactKind::Email => "mail",
        ContactKind::Phone => "call",
    };
    let copy_label = match kind {
        ContactKind::Email => "Copy email",
        ContactKind::Phone => "Copy phone",
    };
    view! {
        <span class="inline-flex items-center gap-1 min-w-0 text-sm">
            <span class="material-symbols-outlined text-stone-400" style="font-size:14px;" aria-hidden="true">
                {icon}
            </span>
            <a href=href class="text-amber-700 dark:text-amber-400 hover:underline truncate"
                on:click=move |ev: web_sys::MouseEvent| ev.stop_propagation()>
                {point.value.clone()}
            </a>
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

/// The editable contact list (emails or phones). Typing writes through
/// untracked, so inputs never re-render (and lose focus) mid-word. Only
/// adding or removing a row redraws the list.
#[component]
fn ContactListEditor(kind: ContactKind, points: RwSignal<Vec<ContactPoint>>) -> impl IntoView {
    let redraw = RwSignal::new(0u32);
    let (noun, placeholder, input_type) = match kind {
        ContactKind::Email => ("email", "name@example.com", "email"),
        ContactKind::Phone => ("phone", "+1 555 010 0000", "tel"),
    };
    let label_placeholder = match kind {
        ContactKind::Email => "work",
        ContactKind::Phone => "cell",
    };
    view! {
        <div class="space-y-1.5">
            {move || {
                redraw.track();
                points.get_untracked().into_iter().enumerate().map(|(i, p)| view! {
                    <div class="flex items-center gap-1.5" data-testid=format!("{noun}-row")>
                        <input
                            type="text"
                            class=format!("{INPUT_CLASS} !w-24")
                            placeholder=label_placeholder
                            aria-label=format!("{noun} label")
                            prop:value=p.label.clone()
                            on:input=move |ev| {
                                let v = event_target_value(&ev);
                                points.update_untracked(|ps| if let Some(x) = ps.get_mut(i) { x.label = v; });
                            }
                        />
                        <input
                            type=input_type
                            class=INPUT_CLASS
                            placeholder=placeholder
                            aria-label=noun
                            prop:value=p.value.clone()
                            on:input=move |ev| {
                                let v = event_target_value(&ev);
                                points.update_untracked(|ps| if let Some(x) = ps.get_mut(i) { x.value = v; });
                            }
                        />
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
                        <button
                            type="button"
                            class="p-1 rounded text-stone-400 hover:text-red-500 cursor-pointer"
                            title=format!("Remove {noun}")
                            aria-label=format!("Remove {noun}")
                            on:click=move |_| {
                                points.update_untracked(|ps| { if i < ps.len() { ps.remove(i); } });
                                redraw.update(|n| *n += 1);
                            }
                        >
                            <span class="material-symbols-outlined" style="font-size:16px;" aria-hidden="true">"close"</span>
                        </button>
                    </div>
                }).collect_view()
            }}
            <button
                type="button"
                class="text-xs text-amber-700 dark:text-amber-400 hover:underline cursor-pointer"
                on:click=move |_| {
                    points.update_untracked(|ps| ps.push(ContactPoint {
                        label: String::new(),
                        value: String::new(),
                        preferred: ps.is_empty(),
                    }));
                    redraw.update(|n| *n += 1);
                }
            >
                {format!("+ Add {noun}")}
            </button>
        </div>
    }
}

/// Edit every field of a person. The server validates (`common::person::
/// normalize`); its message is shown as-is.
#[component]
pub fn PersonForm(
    initial: PersonInput,
    #[prop(into)] on_save: Callback<PersonInput>,
    #[prop(into)] on_cancel: Callback<()>,
    #[prop(into)] saving: Signal<bool>,
) -> impl IntoView {
    let name = RwSignal::new(initial.display_name);
    let company = RwSignal::new(initial.company.unwrap_or_default());
    let position = RwSignal::new(initial.position.unwrap_or_default());
    let emails = RwSignal::new(initial.emails);
    let phones = RwSignal::new(initial.phones);
    let address_label = RwSignal::new(initial.address_label.unwrap_or_default());
    let address = RwSignal::new(initial.address.unwrap_or_default());
    let notes = RwSignal::new(initial.notes.unwrap_or_default());
    let opt = |s: String| (!s.trim().is_empty()).then_some(s);
    let submit = move || {
        on_save.run(PersonInput {
            display_name: name.get_untracked(),
            company: opt(company.get_untracked()),
            position: opt(position.get_untracked()),
            emails: emails.get_untracked(),
            phones: phones.get_untracked(),
            address_label: opt(address_label.get_untracked()),
            address: opt(address.get_untracked()),
            notes: opt(notes.get_untracked()),
        });
    };
    let field = |label: &'static str, sig: RwSignal<String>| {
        view! {
            <label class="block">
                <span class=LABEL_CLASS>{label}</span>
                <input type="text" class=INPUT_CLASS prop:value=move || sig.get()
                    on:input=move |ev| sig.set(event_target_value(&ev)) />
            </label>
        }
    };
    view! {
        <form
            class="space-y-4"
            data-testid="person-form"
            on:submit=move |ev| { ev.prevent_default(); submit(); }
        >
            {field("Name", name)}
            <div class="grid grid-cols-1 sm:grid-cols-2 gap-3">
                {field("Company", company)}
                {field("Position", position)}
            </div>
            <div>
                <span class=LABEL_CLASS>"Emails"</span>
                <ContactListEditor kind=ContactKind::Email points=emails />
            </div>
            <div>
                <span class=LABEL_CLASS>"Phones"</span>
                <ContactListEditor kind=ContactKind::Phone points=phones />
            </div>
            <div class="grid grid-cols-1 sm:grid-cols-[8rem_1fr] gap-3">
                {field("Address label", address_label)}
                <label class="block">
                    <span class=LABEL_CLASS>"Address"</span>
                    <textarea rows="2" class=INPUT_CLASS prop:value=move || address.get()
                        on:input=move |ev| address.set(event_target_value(&ev)) />
                </label>
            </div>
            <label class="block">
                <span class=LABEL_CLASS>"Notes"</span>
                <textarea rows="3" class=INPUT_CLASS prop:value=move || notes.get()
                    on:input=move |ev| notes.set(event_target_value(&ev)) />
            </label>
            <div class="flex items-center gap-2">
                <button type="submit"
                    class="px-3 py-1.5 rounded-lg text-sm font-medium bg-ember text-white disabled:opacity-50 cursor-pointer"
                    disabled=move || saving.get()>
                    "Save"
                </button>
                <button type="button"
                    class="px-3 py-1.5 rounded-lg text-sm text-stone-600 dark:text-stone-300 hover:bg-stone-100 dark:hover:bg-stone-800 cursor-pointer"
                    on:click=move |_| on_cancel.run(())>
                    "Cancel"
                </button>
            </div>
        </form>
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

#[component]
pub fn PeopleView() -> impl IntoView {
    let navigate = StoredValue::new(use_navigate());
    let query = RwSignal::new(String::new());
    let people = LocalResource::new(move || {
        let q = query.get();
        async move { crate::api::fetch_people(Some(&q)).await }
    });
    let creating = RwSignal::new(false);
    let new_name = RwSignal::new(String::new());
    let duplicate = RwSignal::new(None::<Vec<DuplicateMatch>>);
    let busy = RwSignal::new(false);

    let create = move |confirm: bool| {
        let name = new_name.get_untracked();
        if name.trim().is_empty() || busy.get_untracked() {
            return;
        }
        busy.set(true);
        wasm_bindgen_futures::spawn_local(async move {
            let input = PersonInput {
                display_name: name,
                ..Default::default()
            };
            match crate::api::create_person(&input, confirm).await {
                Ok(SavePerson::Saved(p)) => {
                    navigate.get_value()(&format!("/people/{}", p.id), Default::default());
                }
                Ok(SavePerson::Duplicate(w)) => duplicate.set(Some(w.matches)),
                Err(e) => push_toast(ToastLevel::Error, format!("Couldn't create: {e}")),
            }
            busy.set(false);
        });
    };

    view! {
        <div class="flex flex-col h-full">
            <PageHeader icon="group" title="People">
                <button
                    type="button"
                    class="flex items-center gap-1 px-3 py-1.5 rounded-lg text-sm font-medium bg-ember text-white cursor-pointer"
                    on:click=move |_| { creating.set(true); duplicate.set(None); }
                >
                    <span class="material-symbols-outlined" style="font-size:16px;" aria-hidden="true">"person_add"</span>
                    "New person"
                </button>
            </PageHeader>
            <div class="flex-1 overflow-auto p-4 md:p-6 space-y-4 max-w-3xl">
                {move || creating.get().then(|| view! {
                    <form class="flex items-center gap-2" data-testid="new-person-form"
                        on:submit=move |ev| { ev.prevent_default(); create(false); }>
                        <input type="text" class=INPUT_CLASS placeholder="Name\u{2026}" aria-label="New person's name"
                            autofocus=true
                            prop:value=move || new_name.get()
                            on:input=move |ev| new_name.set(event_target_value(&ev)) />
                        <button type="submit" class="px-3 py-1.5 rounded-lg text-sm font-medium bg-ember text-white cursor-pointer disabled:opacity-50"
                            disabled=move || busy.get() || new_name.get().trim().is_empty()>
                            "Create"
                        </button>
                    </form>
                })}
                {move || duplicate.get().map(|matches| view! {
                    <DuplicateNotice matches=matches proceed_label="Create a new person anyway"
                        on_proceed=Callback::new(move |()| create(true)) />
                })}
                <input
                    type="search"
                    class=INPUT_CLASS
                    placeholder="Search people\u{2026}"
                    aria-label="Search people"
                    prop:value=move || query.get()
                    on:input=move |ev| query.set(event_target_value(&ev))
                />
                <Suspense fallback=|| view! { <p class="text-sm text-stone-400">"Loading\u{2026}"</p> }>
                    {move || people.get().map(|r| match r {
                        Ok(list) if list.is_empty() => view! {
                            <EmptyState icon="group" message="No people yet."
                                hint="People you add here can be placed on any node." />
                        }.into_any(),
                        Ok(list) => view! {
                            <ul class="divide-y divide-stone-100 dark:divide-stone-800 rounded-lg border border-stone-200 dark:border-stone-800">
                                {list.into_iter().map(|p| view! { <PersonRow person=p /> }).collect_view()}
                            </ul>
                        }.into_any(),
                        Err(e) => view! { <p class="text-sm text-red-500">{format!("Error: {e}")}</p> }.into_any(),
                    })}
                </Suspense>
            </div>
        </div>
    }
}

#[component]
fn PersonRow(person: Person) -> impl IntoView {
    let navigate = use_navigate();
    let id = person.id;
    let subtitle = [person.position.clone(), person.company.clone()]
        .into_iter()
        .flatten()
        .collect::<Vec<_>>()
        .join(" \u{b7} ");
    let email = primary(&person.emails).cloned();
    view! {
        <li class="px-4 py-3 hover:bg-stone-50 dark:hover:bg-stone-900/60 cursor-pointer"
            data-testid="person-row"
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
    let editing = RwSignal::new(false);
    let saving = RwSignal::new(false);
    // The last save that hit the duplicate warning, kept to "Save anyway".
    let pending = RwSignal::new(None::<(PersonInput, Vec<DuplicateMatch>)>);

    let save_with = move |input: PersonInput, confirm: bool| {
        saving.set(true);
        wasm_bindgen_futures::spawn_local(async move {
            match crate::api::update_person(id, &input, confirm).await {
                Ok(SavePerson::Saved(_)) => {
                    pending.set(None);
                    editing.set(false);
                    version.update(|v| *v += 1);
                    push_toast(ToastLevel::Success, "Saved.");
                }
                Ok(SavePerson::Duplicate(w)) => pending.set(Some((input, w.matches))),
                Err(e) => push_toast(ToastLevel::Error, format!("Couldn't save: {e}")),
            }
            saving.set(false);
        });
    };
    let save = move |input: PersonInput| save_with(input, false);
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
                {move || pending.get().map(|(input, matches)| view! {
                    <DuplicateNotice matches=matches proceed_label="Save anyway"
                        on_proceed=Callback::new(move |()| save_with(input.clone(), true)) />
                })}
                <Suspense fallback=|| view! { <p class="text-sm text-stone-400">"Loading\u{2026}"</p> }>
                    {move || person.get().map(|r| match r {
                        Err(e) => view! { <p class="text-sm text-red-500">{format!("Error: {e}")}</p> }.into_any(),
                        Ok(p) if editing.get() => {
                            let initial = PersonInput {
                                display_name: p.display_name,
                                company: p.company,
                                position: p.position,
                                emails: p.emails,
                                phones: p.phones,
                                address_label: p.address_label,
                                address: p.address,
                                notes: p.notes,
                            };
                            view! {
                                <PersonForm initial=initial on_save=Callback::new(save)
                                    on_cancel=Callback::new(move |()| editing.set(false)) saving=saving />
                            }.into_any()
                        }
                        Ok(p) => view! {
                            <PersonCard person=p
                                on_edit=Callback::new(move |()| editing.set(true))
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
        && person.address.is_none()
        && person.notes.is_none();
    view! {
        <div class="space-y-4" data-testid="person-card">
            <div class="flex items-start justify-between gap-3">
                <div>
                    <h1 class="font-display text-2xl text-stone-900 dark:text-stone-100">{person.display_name.clone()}</h1>
                    {(!subtitle.is_empty()).then(|| view! { <p class="text-sm text-stone-500">{subtitle}</p> })}
                </div>
                <div class="flex items-center gap-1">
                    <button type="button" class="px-3 py-1.5 rounded-lg text-sm border border-stone-200 dark:border-stone-700 hover:bg-stone-100 dark:hover:bg-stone-800 cursor-pointer"
                        on:click=move |_| on_edit.run(())>"Edit"</button>
                    <button type="button" class="p-1.5 rounded-lg text-stone-400 hover:text-red-500 cursor-pointer"
                        title="Delete person" aria-label="Delete person" on:click=move |_| on_delete.run(())>
                        <span class="material-symbols-outlined" aria-hidden="true">"delete"</span>
                    </button>
                </div>
            </div>
            <div class="space-y-1.5">
                {person.emails.into_iter().map(|e| view! { <div><ContactAction kind=ContactKind::Email point=e /></div> }).collect_view()}
                {person.phones.into_iter().map(|p| view! { <div><ContactAction kind=ContactKind::Phone point=p /></div> }).collect_view()}
            </div>
            {person.address.map(|a| view! {
                <div class="text-sm">
                    <div class=LABEL_CLASS>{person.address_label.unwrap_or_else(|| "Address".into())}</div>
                    <p class="whitespace-pre-line text-stone-700 dark:text-stone-300">{a}</p>
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
