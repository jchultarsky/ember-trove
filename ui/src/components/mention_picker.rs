//! The `@` picker for a text box (S9, #360; ONTOLOGY §7.3): typing `@`
//! lists the author's own People, as name and email, and picking one
//! puts a mention of that person into the text.
//!
//! One hook for the node body's textarea and for `ResizableEditor`
//! (notes). The rules are `common::mention` (when an `@` starts a
//! mention, who is offered, what is inserted) and `common::picker::step`
//! (the arrow keys); this file is the wiring.
//!
//! The textarea owns its keys. It calls [`MentionPicker::on_keydown`]
//! first and stops if the picker used the key:
//! - Up and Down move the highlight.
//! - Enter and Tab pick, **only when an option is highlighted**. Nobody
//!   is highlighted on a bare `@`, when what is typed matches only an
//!   email address, or when the cursor only moved into an `@…` that was
//!   in the text already: an arrow key chooses first. So `@` then Enter
//!   is a new line, and so is Enter at the end of a line that happens
//!   to end in `@bob`. A mention made by accident in shared text would
//!   tell its readers a name.
//! - Escape closes the list (or the word that the registry could not be
//!   read) and nothing else: it never reaches the editor's own Escape,
//!   which would discard an edit.
//! - Ctrl/⌘+Enter is left to the editor (save), and closes the list.
//!   Any other key with a modifier is left alone.
//!
//! A pick is given to the browser as one edit of the text box
//! (`insertText`), so Ctrl+Z takes it back, and the box's own `input`
//! handler sets the editor's signal: drafts, autosave and the dirty
//! state see an ordinary edit.

use common::id::PersonId;
use common::mention::{
    MentionName, MentionQuery, PickerMatch, insert_mention, mention_edit, mention_query_at,
    picker_match, typed_by,
};
use common::person::{Person, primary};
use leptos::prelude::*;
use wasm_bindgen::JsCast as _;

use crate::error::UiError;

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
    /// Under the box (a note's editor). Not above: the Notes page's box
    /// is at the top of its scroll area, and a list above it was cut off.
    Below,
}

#[derive(Clone, Copy)]
pub struct MentionPicker {
    value: RwSignal<String>,
    textarea: NodeRef<leptos::html::Textarea>,
    /// The mention being typed, while the list follows it.
    typing: RwSignal<Option<MentionQuery>>,
    /// Whether the list follows something just typed. False when the
    /// cursor only moved into an `@…` that was in the text already (a
    /// click, an arrow key, End): nobody is ready then, and Enter is a
    /// new line, as it is anywhere else in the text.
    by_typing: RwSignal<bool>,
    /// Who an arrow key moved the highlight to. By person, not by place:
    /// the list can change under it (the registry is read again), and
    /// Enter must never pick someone the author did not look at.
    moved: RwSignal<Option<PersonId>>,
    /// The `@` (its byte offset) the list was closed on with Escape: it
    /// stays closed for that `@`.
    dismissed: RwSignal<Option<usize>>,
    /// How often the registry has been asked for. Nothing is read until
    /// the first `@`: not with every editor on the page.
    reads: RwSignal<u32>,
    people: LocalResource<Result<Vec<Person>, UiError>>,
    /// The people this text is known to mention, as the reader may know
    /// them: who the text came with from the API ([`Self::set_known`]).
    known: RwSignal<Vec<MentionName>>,
    /// People picked in this editor, for the preview before a save.
    picked: RwSignal<Vec<MentionName>>,
    /// While the picker itself is editing the box ([`Self::edit`]): the
    /// `input` event of that edit is not the person typing.
    editing: StoredValue<bool>,
    id: StoredValue<String>,
}

impl MentionPicker {
    /// A picker for the text in `value`, shown in `textarea`.
    pub fn new(value: RwSignal<String>, textarea: NodeRef<leptos::html::Textarea>) -> Self {
        let reads = RwSignal::new(0u32);
        let people = LocalResource::new(move || {
            let wanted = reads.get() > 0;
            async move {
                if wanted {
                    crate::api::fetch_people(None).await
                } else {
                    Ok(Vec::new())
                }
            }
        });
        Self {
            value,
            textarea,
            typing: RwSignal::new(None),
            by_typing: RwSignal::new(false),
            moved: RwSignal::new(None),
            dismissed: RwSignal::new(None),
            reads,
            people,
            known: RwSignal::new(Vec::new()),
            picked: RwSignal::new(Vec::new()),
            editing: StoredValue::new(false),
            id: StoredValue::new(format!(
                "mention-picker-{}",
                NEXT_ID.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
            )),
        }
    }

    /// The people the text came with (`ViewedNode.mentions`, a note's
    /// `mentions`): the real mentions in it, as this reader may know
    /// them.
    pub fn set_known(&self, names: Vec<MentionName>) {
        self.known.set(names);
    }

    fn cursor(&self) -> Option<usize> {
        self.textarea
            .get_untracked()
            .and_then(|el| el.selection_start().ok().flatten())
            .map(|c| c as usize)
    }

    /// The text as the box holds it: what the cursor's offsets count in.
    /// The editor's signal can differ until the first keystroke (a text
    /// saved with CRLF line ends keeps them there; the box does not).
    fn text(&self) -> String {
        self.textarea
            .get_untracked()
            .map(|el| el.value())
            .unwrap_or_else(|| self.value.get_untracked())
    }

    /// Who is offered for `query` among `people`: those whose name
    /// matches first, then those matched by an address.
    fn offered(people: Vec<Person>, query: &str) -> Vec<(Person, PickerMatch)> {
        let mut offered: Vec<(Person, PickerMatch)> = people
            .into_iter()
            .filter_map(|p| {
                let emails: Vec<&str> = p.emails.iter().map(|e| e.value.as_str()).collect();
                let how = picker_match(&p.display_name, &emails, query)?;
                Some((p, how))
            })
            .collect();
        // Stable: the registry's own order (by name) within each kind.
        offered.sort_by_key(|(_, how)| *how);
        offered.truncate(SHOWN);
        offered
    }

    /// The option Enter picks when no arrow key has chosen: the first,
    /// and only when a name matches what was **typed**. On a bare `@`,
    /// when only an address matches, or when the cursor only moved here
    /// (`typed` false), nobody: see the module doc.
    fn ready(options: &[(Person, PickerMatch)], query: &str, typed: bool) -> Option<usize> {
        (typed
            && !query.trim().is_empty()
            && options
                .first()
                .is_some_and(|(_, how)| *how == PickerMatch::Name))
        .then_some(0)
    }

    /// The highlighted option: whoever an arrow key moved to, while they
    /// are still offered; otherwise [`Self::ready`]. Worked out from the
    /// list as it is, so it is right when the registry arrives after the
    /// name was typed.
    fn active_in(
        moved: Option<PersonId>,
        options: &[(Person, PickerMatch)],
        query: &str,
        typed: bool,
    ) -> Option<usize> {
        moved
            .and_then(|id| options.iter().position(|(p, _)| p.id == id))
            .or_else(|| Self::ready(options, query, typed))
    }

    fn options(&self) -> Vec<(Person, PickerMatch)> {
        let Some(typing) = self.typing.get() else {
            return Vec::new();
        };
        let people = self.people.get().and_then(Result::ok).unwrap_or_default();
        Self::offered(people, &typing.query)
    }

    fn options_untracked(&self) -> Vec<(Person, PickerMatch)> {
        let Some(typing) = self.typing.get_untracked() else {
            return Vec::new();
        };
        let people = self
            .people
            .get_untracked()
            .and_then(Result::ok)
            .unwrap_or_default();
        Self::offered(people, &typing.query)
    }

    pub fn close(&self) {
        self.typing.set(None);
        self.moved.set(None);
    }

    /// Call from the text box's `input` handler, after the text signal
    /// is set: follows the `@` at the cursor, if any. `ev` says whether
    /// the person typed (`common::mention::typed_by`): after a paste, a
    /// drop or an undo the list shows with nobody ready, as it does when
    /// the cursor is moved.
    pub fn after_input(&self, ev: &leptos::ev::Event) {
        // An edit the picker makes (a pick) is an `insertText` too, and
        // nobody typed it.
        let typed = !self.editing.get_value()
            && ev
                .dyn_ref::<web_sys::InputEvent>()
                .is_some_and(|ev| typed_by(&ev.input_type()));
        self.follow(typed);
    }

    /// Call from the text box's `click` handler: the cursor may now be in
    /// an `@…` that is in the text already. The list opens for it, with
    /// nobody ready.
    pub fn after_click(&self) {
        self.follow(false);
    }

    fn follow(&self, typed: bool) {
        let text = self.text();
        let typing = self.cursor().and_then(|c| mention_query_at(&text, c));
        let Some(typing) = typing else {
            self.close();
            self.dismissed.set(None);
            return;
        };
        if self.dismissed.get_untracked() == Some(typing.at) {
            return;
        }
        // The registry is read when an `@` starts a mention: at the first
        // one, and again at each later one, so someone added since is
        // offered and a read that failed is tried again. Not at every
        // keystroke after it. (The list shows the people it had until the
        // new answer is in.)
        let new_at = self
            .typing
            .get_untracked()
            .is_none_or(|was| was.at != typing.at);
        if new_at {
            self.reads.update(|n| *n = n.saturating_add(1));
        }
        self.dismissed.set(None);
        self.moved.set(None);
        self.by_typing.set(typed);
        self.typing.set(Some(typing));
    }

    /// Call from the text box's `keyup`: a key that moves the cursor and
    /// leaves the text alone (an arrow, Home, End) can move it into or
    /// out of a mention being typed, and the list follows.
    pub fn on_keyup(&self, ev: &leptos::ev::KeyboardEvent) {
        if !matches!(
            ev.key().as_str(),
            "ArrowLeft"
                | "ArrowRight"
                | "ArrowUp"
                | "ArrowDown"
                | "Home"
                | "End"
                | "PageUp"
                | "PageDown"
        ) {
            return;
        }
        let text = self.text();
        let now = self.cursor().and_then(|c| mention_query_at(&text, c));
        // Only when the cursor is somewhere else: an arrow key the list
        // used did not move it, and must not reset the highlight.
        let left_a_dismissed = now.is_none() && self.dismissed.get_untracked().is_some();
        if now != self.typing.get_untracked() || left_a_dismissed {
            self.follow(false);
        }
    }

    /// Whether the registry could not be read.
    fn failed_untracked(&self) -> bool {
        matches!(self.people.get_untracked(), Some(Err(_)))
    }

    /// Call first from the text box's `keydown`. True when the picker
    /// used the key; the editor then does nothing with it.
    pub fn on_keydown(&self, ev: &leptos::ev::KeyboardEvent) -> bool {
        // An input method composing text owns its keys (Safari reports
        // the confirming Enter as key code 229 with no composing flag).
        if ev.is_composing() || ev.key_code() == 229 {
            return false;
        }
        if ev.ctrl_key() || ev.meta_key() || ev.alt_key() {
            // The editor's chord. Ctrl/⌘+Enter saves: the list must not
            // stay up over a box the editor is about to empty. Anything
            // else is left alone: on many keyboards AltGr or Option types
            // a letter (or the `@` itself), and closing the list for it
            // made the next keystroke a "new" `@`.
            if ev.key() == "Enter" {
                self.close();
            }
            return false;
        }
        let Some(typing) = self.typing.get_untracked() else {
            return false;
        };
        let options = self.options_untracked();
        // Something of the picker's is on screen: the list, or the word
        // that the registry could not be read.
        if options.is_empty() && !self.failed_untracked() {
            return false;
        }
        let active = Self::active_in(
            self.moved.get_untracked(),
            &options,
            &typing.query,
            self.by_typing.get_untracked(),
        );
        match ev.key().as_str() {
            // Escape closes what the picker shows, the failed-read alert
            // too, and never reaches the editor's own Escape.
            "Escape" => {
                ev.prevent_default();
                ev.stop_propagation();
                self.dismissed.set(Some(typing.at));
                self.close();
                true
            }
            _ if options.is_empty() => false,
            "ArrowDown" | "ArrowUp" => {
                ev.prevent_default();
                let next = common::picker::step(active, options.len(), ev.key() == "ArrowDown");
                self.moved
                    .set(next.and_then(|i| options.get(i).map(|(p, _)| p.id)));
                true
            }
            // Shift+Tab leaves the field, as everywhere.
            "Tab" if ev.shift_key() => {
                self.close();
                false
            }
            "Enter" | "Tab" => {
                let Some((person, _)) = active.and_then(|i| options.get(i).cloned()) else {
                    return false;
                };
                // The list must still be about where the cursor is. It
                // can have moved with no event to follow (⌘← on a Mac
                // sends no `keyup`): the key is then the editor's.
                let text = self.text();
                let here = self.cursor().and_then(|c| mention_query_at(&text, c));
                if here.as_ref() != Some(&typing) {
                    self.follow(false);
                    return false;
                }
                ev.prevent_default();
                ev.stop_propagation();
                self.pick(&person);
                true
            }
            _ => false,
        }
    }

    /// Replace the UTF-16 range `from..to` of the box with `insert`, as
    /// one edit by the browser: Ctrl+Z takes it back, and the box's own
    /// `input` handler sets the editor's signal. `whole` is the text that
    /// results. Where the browser won't (or did something else), the text
    /// is set outright: the same result, without the undo step. True when
    /// the browser made the edit (the cursor is then after `insert`).
    fn edit(
        &self,
        el: &web_sys::HtmlTextAreaElement,
        from: usize,
        to: usize,
        insert: &str,
        whole: &str,
    ) -> bool {
        let _ = el.focus();
        let _ = el.set_selection_range(
            u32::try_from(from).unwrap_or(u32::MAX),
            u32::try_from(to).unwrap_or(u32::MAX),
        );
        // The browser sends the box its `input` event before this
        // returns.
        self.editing.set_value(true);
        let by_browser = document()
            .dyn_into::<web_sys::HtmlDocument>()
            .ok()
            .and_then(|doc| {
                // Nothing to put in: the selection is taken out.
                if insert.is_empty() {
                    doc.exec_command("delete").ok()
                } else {
                    doc.exec_command_with_show_ui_and_value("insertText", false, insert)
                        .ok()
                }
            })
            .unwrap_or(false);
        self.editing.set_value(false);
        if by_browser && el.value() == whole {
            return true;
        }
        self.value.set(whole.to_string());
        el.set_value(whole);
        false
    }

    /// Put a mention of `person` where the `@…` being typed is. The
    /// place is worked out again from the text as it is now: an autosave
    /// or a paste may have run since the list opened.
    fn pick(&self, person: &Person) {
        let (Some(cursor), Some(el)) = (self.cursor(), self.textarea.get_untracked()) else {
            self.close();
            return;
        };
        let text = el.value();
        let Some(typing) = mention_query_at(&text, cursor) else {
            self.close();
            return;
        };
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
        let edit = mention_edit(&text, &typing, cursor, &person.display_name, person.id);
        let (new_text, after) =
            insert_mention(&text, &typing, cursor, &person.display_name, person.id);
        if !self.edit(&el, edit.from, edit.to, &edit.insert, &new_text) {
            let at = u32::try_from(after).unwrap_or(u32::MAX);
            let _ = el.set_selection_range(at, at);
        }
    }

    fn names_of(known: Vec<MentionName>, picked: Vec<MentionName>) -> Vec<MentionName> {
        let mut names = known;
        for p in picked {
            if !names.iter().any(|n| n.person_id == p.person_id) {
                names.push(p);
            }
        }
        names
    }

    /// Everyone the preview may name: who the text came with, and whoever
    /// was picked here.
    ///
    /// SECURITY: not "everyone in the author's registry". A token someone
    /// else put in the text with the id of the author's person is not a
    /// mention (the API makes no row for it), and must not look like one
    /// here.
    ///
    /// Nothing here writes a person's current name into the text, and
    /// nothing should (2026-11-19). An "Update" for labels that show an
    /// older name was built and taken out: it wrote the name into every
    /// token the page showed as a mention, and three reviews in a row
    /// found a text for which one of those was, to another reader or
    /// after the edit, the address of a link (Markdown reads a text
    /// differently by its length and by which wiki links lead somewhere).
    /// The page shows the current name without it.
    pub fn names(&self) -> Vec<MentionName> {
        Self::names_of(self.known.get(), self.picked.get())
    }

    /// The id of the list, for the text box's `aria-controls`, while it
    /// is open.
    pub fn controls(&self) -> Option<String> {
        (!self.options().is_empty()).then(|| self.id.get_value())
    }

    fn option_id(&self, person: PersonId) -> String {
        format!("{}-{}", self.id.get_value(), person.0.simple())
    }

    /// The id of the highlighted option, for `aria-activedescendant`.
    /// Keyed by the person, so it changes (and is read out again) when
    /// what is typed puts someone else first.
    pub fn active_descendant(&self) -> Option<String> {
        let options = self.options();
        let typing = self.typing.get()?;
        Self::active_in(
            self.moved.get(),
            &options,
            &typing.query,
            self.by_typing.get(),
        )
        .and_then(|i| options.get(i).map(|(p, _)| self.option_id(p.id)))
    }

    /// The list. Renders nothing while it is closed.
    pub fn view(&self, dock: Dock) -> impl IntoView + use<> {
        let picker = *self;
        let place = match dock {
            Dock::Inside => "absolute bottom-4 left-4",
            Dock::Below => "absolute top-full left-0 mt-1",
        };
        let frame = format!(
            "{place} z-50 w-72 max-w-full bg-white dark:bg-stone-900 border border-stone-200 \
             dark:border-stone-700 rounded-xl shadow-xl overflow-hidden"
        );
        move || {
            let typing = picker.typing.get()?;
            // The registry could not be read: said, not an empty list.
            if matches!(picker.people.get(), Some(Err(_))) {
                return Some(
                    view! {
                        <div class=frame.clone() data-testid="mention-picker">
                            <p role="alert" class="px-3 py-2 text-xs text-red-600 dark:text-red-400">
                                "Couldn\u{2019}t load your People. Type @ again to retry."
                            </p>
                        </div>
                    }
                    .into_any(),
                );
            }
            let options = picker.options();
            if options.is_empty() {
                return None;
            }
            let query = typing.query.clone();
            let list_id = picker.id.get_value();
            let count = options.len();
            let names: Vec<String> = options
                .iter()
                .map(|(p, _)| p.display_name.clone())
                .collect();
            let ids: Vec<PersonId> = options.iter().map(|(p, _)| p.id).collect();
            let ready = Self::ready(&options, &query, picker.by_typing.get());
            // As `active_in`: whoever an arrow key moved to, else the one
            // who is ready.
            let active = Signal::derive(move || {
                picker
                    .moved
                    .get()
                    .and_then(|id| ids.iter().position(|p| *p == id))
                    .or(ready)
            });
            // Said to a screen reader as the list opens, narrows and
            // moves: a text box can't say it is expanded, and the same
            // option id would not be read out twice.
            let heard = move || {
                let people = if count == 1 {
                    "1 person".to_string()
                } else {
                    format!("{count} people")
                };
                match active.get().and_then(|i| names.get(i).cloned()) {
                    Some(name) => format!("{people}. {name} highlighted."),
                    None => format!("{people}. Arrow down to choose."),
                }
            };
            Some(view! {
                <div class=frame.clone() data-testid="mention-picker">
                    <div class="px-3 py-1.5 text-xs text-stone-500 dark:text-stone-400 \
                            border-b border-stone-100 dark:border-stone-800">
                        {if query.is_empty() {
                            "Mention a person \u{2014} type a name, or \u{2193} to choose".to_string()
                        } else {
                            format!("Mention a person \u{2014} {query}")
                        }}
                    </div>
                    <p class="sr-only" aria-live="polite" data-testid="mention-heard">{heard}</p>
                    <ul id=list_id role="listbox" aria-label="People">
                        {options.into_iter().enumerate().map(|(i, (person, _))| {
                            let email = primary(&person.emails).map(|e| e.value.clone());
                            let name = person.display_name.clone();
                            let option_id = picker.option_id(person.id);
                            let chosen = person.clone();
                            let pressed = person.clone();
                            view! {
                                <li id=option_id role="option"
                                    aria-selected=move || {
                                        if active.get() == Some(i) { "true" } else { "false" }
                                    }
                                    class=move || {
                                        let base = "px-3 py-2 text-sm cursor-pointer \
                                            text-stone-800 dark:text-stone-200";
                                        if active.get() == Some(i) {
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
            }.into_any())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use common::person::ContactPoint;
    use uuid::Uuid;

    fn person(n: u128, name: &str, email: Option<&str>) -> Person {
        Person {
            id: PersonId(Uuid::from_u128(n)),
            owner_id: "me".to_string(),
            display_name: name.to_string(),
            company: Some("Lovelace Ltd".to_string()),
            position: None,
            emails: email
                .map(|value| ContactPoint {
                    label: String::new(),
                    value: value.to_string(),
                    preferred: true,
                })
                .into_iter()
                .collect(),
            phones: vec![ContactPoint {
                label: String::new(),
                value: "+1 555 010 4242".to_string(),
                preferred: false,
            }],
            addresses: Vec::new(),
            notes: None,
            created_at: chrono::DateTime::UNIX_EPOCH,
            updated_at: chrono::DateTime::UNIX_EPOCH,
        }
    }

    fn registry() -> Vec<Person> {
        vec![
            person(1, "Ada Lovelace", Some("countess@example.com")),
            person(2, "Bea Adams", Some("ada.fan@example.com")),
            person(3, "Cy Young", None),
        ]
    }

    fn ids(offered: &[(Person, PickerMatch)]) -> Vec<u128> {
        offered.iter().map(|(p, _)| p.id.0.as_u128()).collect()
    }

    #[test]
    fn people_are_offered_by_name_first_then_by_address() {
        // "ada": a word of Ada's name, of Bea's ("Adams"); nobody by a
        // company or a phone number.
        let offered = MentionPicker::offered(registry(), "ada");
        assert_eq!(ids(&offered), [1, 2]);
        assert!(offered.iter().all(|(_, how)| *how == PickerMatch::Name));
        // "countess": only an address. Listed, after any name match.
        let offered = MentionPicker::offered(registry(), "countess");
        assert_eq!(ids(&offered), [1]);
        assert_eq!(offered[0].1, PickerMatch::Email);
        assert!(MentionPicker::offered(registry(), "4242").is_empty());
        assert!(MentionPicker::offered(registry(), "lovelace ltd").is_empty());
        // A bare `@` offers everyone, up to what the list shows.
        assert_eq!(ids(&MentionPicker::offered(registry(), "")), [1, 2, 3]);
        let many: Vec<Person> = (0..20).map(|n| person(n, "Ann", None)).collect();
        assert_eq!(MentionPicker::offered(many, "an").len(), SHOWN);
    }

    /// SECURITY: Enter picks only someone a typed name matched. A mention
    /// made by accident in shared text tells its readers a name.
    #[test]
    fn somebody_is_ready_only_for_a_typed_name() {
        let ready = |query: &str, typed: bool| {
            MentionPicker::ready(&MentionPicker::offered(registry(), query), query, typed)
        };
        assert_eq!(ready("ada", true), Some(0));
        // A bare `@`, or white space only.
        assert_eq!(ready("", true), None);
        assert_eq!(ready("  ", true), None);
        // Only an address matches.
        assert_eq!(ready("countess", true), None);
        // Nobody matches.
        assert_eq!(ready("zed", true), None);
        // The cursor moved here, or the text was pasted: nobody.
        assert_eq!(ready("ada", false), None);
    }

    #[test]
    fn the_highlight_is_a_person_not_a_row() {
        let options = MentionPicker::offered(registry(), "ada");
        let bea = PersonId(Uuid::from_u128(2));
        let at = |moved, options: &[(Person, PickerMatch)], typed| {
            MentionPicker::active_in(moved, options, "ada", typed)
        };
        // An arrow key moved to Bea, the second row.
        assert_eq!(at(Some(bea), &options, true), Some(1));
        // The list changes under it: someone new sorts first. Still Bea.
        let mut longer = registry();
        longer.insert(0, person(9, "Aaron Ada", None));
        let longer = MentionPicker::offered(longer, "ada");
        assert_eq!(ids(&longer), [9, 1, 2]);
        assert_eq!(at(Some(bea), &longer, true), Some(2));
        // An arrow key's choice holds wherever the cursor came from.
        assert_eq!(at(Some(bea), &options, false), Some(1));
        // Bea is no longer offered: back to whoever is ready, if anyone.
        let without: Vec<_> = options
            .iter()
            .filter(|(p, _)| p.id != bea)
            .cloned()
            .collect();
        assert_eq!(at(Some(bea), &without, true), Some(0));
        assert_eq!(at(Some(bea), &without, false), None);
        assert_eq!(at(None, &options, true), Some(0));
        assert_eq!(at(None, &options, false), None);
    }

    /// SECURITY: the preview names who the text came with and who was
    /// picked here, never everyone in the registry.
    #[test]
    fn the_names_are_who_the_text_came_with_and_who_was_picked() {
        let name = |n: u128, name: &str, mine: bool| MentionName {
            person_id: PersonId(Uuid::from_u128(n)),
            display_name: name.to_string(),
            mine,
        };
        let names = MentionPicker::names_of(
            vec![name(1, "Ada King", true), name(7, "Theirs", false)],
            vec![name(1, "Ada (as picked)", true), name(2, "Bea", true)],
        );
        // Who the text came with wins over a pick of the same person.
        assert_eq!(
            names,
            [
                name(1, "Ada King", true),
                name(7, "Theirs", false),
                name(2, "Bea", true)
            ]
        );
    }
}
