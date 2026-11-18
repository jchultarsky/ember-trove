//! The `@` picker for a text box (S9, #360; ONTOLOGY §7.3): typing `@`
//! lists the author's own People, as name and email, and picking one
//! puts a mention of that person into the text.
//!
//! One hook for the node body's textarea and for `ResizableEditor`
//! (notes). The rules are `common::mention` (when an `@` starts a
//! mention, what is inserted) and `common::picker::step` (the arrow
//! keys); this file is the wiring.
//!
//! The textarea owns its keys. It calls [`MentionPicker::on_keydown`]
//! first and stops if the picker used the key:
//! - Up and Down move the highlight.
//! - Enter and Tab pick, **only when an option is highlighted**. On a
//!   bare `@` nothing is highlighted until an arrow key is pressed, so
//!   `@` then Enter is a new line. A mention made by accident in shared
//!   text would tell its readers a name.
//! - Escape closes the list and nothing else: it never reaches the
//!   editor's own Escape, which would discard an edit.
//! - Ctrl/⌘+Enter is left to the editor (save).

use common::id::PersonId;
use common::mention::{
    MentionName, MentionQuery, insert_mention, mention_query_at, mentions, refresh_labels,
};
use common::person::{Person, matches, primary};
use leptos::prelude::*;

/// How many people the list shows at once.
const SHOWN: usize = 8;

/// Unique DOM ids per picker instance.
static NEXT_ID: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);

/// Where the list sits relative to its text box.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Dock {
    /// Inside the box's pane, at its lower left (the node editor, where
    /// the `[[` list sits too).
    Inside,
    /// Above the box (a note's editor).
    Above,
}

#[derive(Clone, Copy)]
pub struct MentionPicker {
    value: RwSignal<String>,
    textarea: NodeRef<leptos::html::Textarea>,
    /// The mention being typed, while the list follows it.
    typing: RwSignal<Option<MentionQuery>>,
    active: RwSignal<Option<usize>>,
    /// The `@` (its byte offset) the list was closed on with Escape: it
    /// stays closed for that `@`.
    dismissed: RwSignal<Option<usize>>,
    /// Set by the first `@`, or by text that already holds a mention:
    /// the registry is read then, not with every editor.
    wanted: RwSignal<bool>,
    people: LocalResource<Vec<Person>>,
    /// People picked in this editor, for the preview before a save.
    picked: RwSignal<Vec<MentionName>>,
    id: StoredValue<String>,
}

impl MentionPicker {
    /// A picker for the text in `value`, shown in `textarea`.
    pub fn new(value: RwSignal<String>, textarea: NodeRef<leptos::html::Textarea>) -> Self {
        let wanted = RwSignal::new(false);
        let people = LocalResource::new(move || {
            let wanted = wanted.get();
            async move {
                if wanted {
                    // A failed read lists nobody: the text is typed as
                    // before, and the next `@` asks again.
                    crate::api::fetch_people(None).await.unwrap_or_default()
                } else {
                    Vec::new()
                }
            }
        });
        let picker = Self {
            value,
            textarea,
            typing: RwSignal::new(None),
            active: RwSignal::new(None),
            dismissed: RwSignal::new(None),
            wanted,
            people,
            picked: RwSignal::new(Vec::new()),
            id: StoredValue::new(format!(
                "mention-picker-{}",
                NEXT_ID.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
            )),
        };
        // Text that holds a mention already: the names are needed to say
        // whether a label is out of date.
        Effect::new(move |_| {
            if !wanted.get() && value.with(|text| !mentions(text).is_empty()) {
                wanted.set(true);
            }
        });
        picker
    }

    fn cursor(&self) -> Option<usize> {
        self.textarea
            .get_untracked()
            .and_then(|el| el.selection_start().ok().flatten())
            .map(|c| c as usize)
    }

    /// The people the list offers for what is typed.
    fn options(&self) -> Vec<Person> {
        let Some(typing) = self.typing.get() else {
            return Vec::new();
        };
        self.people
            .get()
            .unwrap_or_default()
            .into_iter()
            .filter(|p| matches(p, &typing.query))
            .take(SHOWN)
            .collect()
    }

    fn options_untracked(&self) -> Vec<Person> {
        let Some(typing) = self.typing.get_untracked() else {
            return Vec::new();
        };
        self.people
            .get_untracked()
            .unwrap_or_default()
            .into_iter()
            .filter(|p| matches(p, &typing.query))
            .take(SHOWN)
            .collect()
    }

    pub fn close(&self) {
        self.typing.set(None);
        self.active.set(None);
    }

    /// Call from the text box's `input` (and `click`) handler, after the
    /// text signal is set: follows the `@` at the cursor, if any.
    pub fn after_input(&self) {
        let text = self.value.get_untracked();
        let typing = self.cursor().and_then(|c| mention_query_at(&text, c));
        let Some(typing) = typing else {
            self.close();
            self.dismissed.set(None);
            return;
        };
        if self.dismissed.get_untracked() == Some(typing.at) {
            return;
        }
        self.wanted.set(true);
        // Something typed after the `@` highlights the first match; a
        // bare `@` highlights nothing (see the module doc).
        self.active.set((!typing.query.is_empty()).then_some(0));
        self.dismissed.set(None);
        self.typing.set(Some(typing));
    }

    /// Call first from the text box's `keydown`. True when the picker
    /// used the key; the editor then does nothing with it.
    pub fn on_keydown(&self, ev: &leptos::ev::KeyboardEvent) -> bool {
        if ev.is_composing() || ev.ctrl_key() || ev.meta_key() || ev.alt_key() {
            return false;
        }
        let options = self.options_untracked();
        if options.is_empty() {
            return false;
        }
        match ev.key().as_str() {
            "ArrowDown" | "ArrowUp" => {
                ev.prevent_default();
                self.active.set(common::picker::step(
                    self.active.get_untracked(),
                    options.len(),
                    ev.key() == "ArrowDown",
                ));
                true
            }
            "Enter" | "Tab" => {
                let Some(person) = self
                    .active
                    .get_untracked()
                    .and_then(|i| options.get(i).cloned())
                else {
                    return false;
                };
                ev.prevent_default();
                ev.stop_propagation();
                self.pick(&person);
                true
            }
            "Escape" => {
                ev.prevent_default();
                ev.stop_propagation();
                self.dismissed
                    .set(self.typing.get_untracked().map(|t| t.at));
                self.close();
                true
            }
            _ => false,
        }
    }

    /// Put a mention of `person` where the `@…` being typed is. The
    /// place is worked out again from the text as it is now: an autosave
    /// or a paste may have run since the list opened.
    fn pick(&self, person: &Person) {
        let text = self.value.get_untracked();
        let Some(cursor) = self.cursor() else {
            self.close();
            return;
        };
        let Some(typing) = mention_query_at(&text, cursor) else {
            self.close();
            return;
        };
        let (new_text, after) =
            insert_mention(&text, &typing, cursor, &person.display_name, person.id);
        self.picked.update(|picked| {
            if !picked.iter().any(|p| p.person_id == person.id) {
                picked.push(MentionName {
                    person_id: person.id,
                    display_name: person.display_name.clone(),
                    mine: true,
                });
            }
        });
        self.close();
        // Through the editor's own signal, so its draft, autosave and
        // dirty state see an ordinary edit.
        self.value.set(new_text.clone());
        if let Some(el) = self.textarea.get_untracked() {
            el.set_value(&new_text);
            let at = u32::try_from(after).unwrap_or(u32::MAX);
            let _ = el.set_selection_start(Some(at));
            let _ = el.set_selection_end(Some(at));
            let _ = el.focus();
        }
    }

    /// The author's own people as the preview may name them before a
    /// save: everyone in their registry (once it is read), and whoever
    /// was picked here.
    pub fn own_names(&self) -> Vec<MentionName> {
        let mut names: Vec<MentionName> = self
            .people
            .get()
            .unwrap_or_default()
            .into_iter()
            .map(|p| MentionName {
                person_id: p.id,
                display_name: p.display_name,
                mine: true,
            })
            .collect();
        for p in self.picked.get() {
            if !names.iter().any(|n| n.person_id == p.person_id) {
                names.push(p);
            }
        }
        names
    }

    /// `text` with the labels of the author's own people brought up to
    /// their current names, and how many changed; `None` when all are
    /// current (or the registry is not read yet). Offered as an edit,
    /// never applied behind the author's back
    /// (`common::mention::refresh_labels`).
    pub fn stale_labels(&self, text: &str) -> Option<(String, usize)> {
        let own: Vec<(PersonId, String)> = self
            .people
            .get()
            .unwrap_or_default()
            .into_iter()
            .map(|p| (p.id, p.display_name))
            .collect();
        refresh_labels(text, &own)
    }

    /// As [`Self::stale_labels`], outside a reactive closure (a save).
    pub fn refreshed(&self, text: &str) -> String {
        let own: Vec<(PersonId, String)> = self
            .people
            .get_untracked()
            .unwrap_or_default()
            .into_iter()
            .map(|p| (p.id, p.display_name))
            .collect();
        refresh_labels(text, &own).map_or_else(|| text.to_string(), |(fresh, _)| fresh)
    }

    /// The id of the list, for the text box's `aria-controls`, while it
    /// is open.
    pub fn controls(&self) -> Option<String> {
        (!self.options().is_empty()).then(|| self.id.get_value())
    }

    /// The id of the highlighted option, for `aria-activedescendant`.
    pub fn active_descendant(&self) -> Option<String> {
        let len = self.options().len();
        self.active
            .get()
            .filter(|i| *i < len)
            .map(|i| format!("{}-opt-{i}", self.id.get_value()))
    }

    /// The list. Renders nothing while it is closed.
    pub fn view(&self, dock: Dock) -> impl IntoView + use<> {
        let picker = *self;
        let place = match dock {
            Dock::Inside => "absolute bottom-4 left-4",
            Dock::Above => "absolute bottom-full left-0 mb-1",
        };
        move || {
            let options = picker.options();
            if options.is_empty() {
                return None;
            }
            let query = picker.typing.get().map(|t| t.query).unwrap_or_default();
            let list_id = picker.id.get_value();
            Some(view! {
                <div class=format!("{place} z-50 w-72 max-w-full bg-white dark:bg-stone-900 \
                        border border-stone-200 dark:border-stone-700 rounded-xl shadow-xl \
                        overflow-hidden")
                    data-testid="mention-picker">
                    <div class="px-3 py-1.5 text-xs text-stone-500 dark:text-stone-400 \
                            border-b border-stone-100 dark:border-stone-800">
                        {if query.is_empty() {
                            "Mention a person \u{2014} type a name, or \u{2193} to choose".to_string()
                        } else {
                            format!("Mention a person \u{2014} {query}")
                        }}
                    </div>
                    <ul id=list_id.clone() role="listbox" aria-label="People">
                        {options.into_iter().enumerate().map(|(i, person)| {
                            let email = primary(&person.emails).map(|e| e.value.clone());
                            let name = person.display_name.clone();
                            let chosen = person.clone();
                            let pressed = person.clone();
                            view! {
                                <li id=format!("{list_id}-opt-{i}") role="option"
                                    aria-selected=move || {
                                        if picker.active.get() == Some(i) { "true" } else { "false" }
                                    }
                                    class=move || {
                                        let base = "px-3 py-2 text-sm cursor-pointer \
                                            text-stone-800 dark:text-stone-200";
                                        if picker.active.get() == Some(i) {
                                            format!("{base} bg-amber-50 dark:bg-amber-900/30")
                                        } else {
                                            format!("{base} hover:bg-stone-50 dark:hover:bg-stone-800")
                                        }
                                    }
                                    // A pointer press picks on mousedown, with the
                                    // default stopped so the text box keeps the
                                    // keyboard; an accessibility press arrives as
                                    // a click with no pointer (#174).
                                    on:mousedown=move |ev| {
                                        if common::picker::is_pointer_press(ev.detail()) {
                                            ev.prevent_default();
                                            picker.pick(&pressed);
                                        }
                                    }
                                    on:click=move |ev| {
                                        if !common::picker::is_pointer_press(ev.detail()) {
                                            picker.pick(&chosen);
                                        }
                                    }>
                                    <span class="block truncate">{name}</span>
                                    {email.map(|e| view! {
                                        <span class="block truncate text-xs text-stone-500 dark:text-stone-400">{e}</span>
                                    })}
                                </li>
                            }
                        }).collect_view()}
                    </ul>
                </div>
            })
        }
    }
}
