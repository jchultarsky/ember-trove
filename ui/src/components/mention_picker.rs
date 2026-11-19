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
//!   is highlighted on a bare `@`, or when what is typed matches only an
//!   email address: an arrow key chooses first. So `@` then Enter is a
//!   new line. A mention made by accident in shared text would tell its
//!   readers a name.
//! - Escape closes the list (or the word that the registry could not be
//!   read) and nothing else: it never reaches the editor's own Escape,
//!   which would discard an edit.
//! - Ctrl/⌘+Enter is left to the editor (save), and closes the list.
//!
//! A pick is given to the browser as one edit of the text box
//! (`insertText`), so Ctrl+Z takes it back, and the box's own `input`
//! handler sets the editor's signal: drafts, autosave and the dirty
//! state see an ordinary edit.

use common::id::PersonId;
use common::mention::{
    MentionName, MentionQuery, PickerMatch, insert_mention, mention_edit, mention_query_at,
    picker_match, refresh_labels, stale_labels,
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
    /// "Keep as typed": the older names (a person and the label as
    /// written) the author chose to leave. A label that goes stale later,
    /// or another person's, is still offered.
    kept: RwSignal<Vec<(PersonId, String)>>,
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
            moved: RwSignal::new(None),
            dismissed: RwSignal::new(None),
            reads,
            people,
            known: RwSignal::new(Vec::new()),
            picked: RwSignal::new(Vec::new()),
            kept: RwSignal::new(Vec::new()),
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
    /// and only when a name matches what was typed. On a bare `@`, or
    /// when only an address matches, nobody: see the module doc.
    fn ready(options: &[(Person, PickerMatch)], query: &str) -> Option<usize> {
        (!query.trim().is_empty()
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
    ) -> Option<usize> {
        moved
            .and_then(|id| options.iter().position(|(p, _)| p.id == id))
            .or_else(|| Self::ready(options, query))
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

    /// Call from the text box's `input` (and `click`) handler, after the
    /// text signal is set: follows the `@` at the cursor, if any.
    pub fn after_input(&self) {
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
            self.after_input();
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
            // The editor's chord (save). The list must not stay up over
            // a box the editor is about to empty.
            self.close();
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
        let active = Self::active_in(self.moved.get_untracked(), &options, &typing.query);
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
        let by_browser = document()
            .dyn_into::<web_sys::HtmlDocument>()
            .ok()
            .and_then(|doc| {
                doc.exec_command_with_show_ui_and_value("insertText", false, insert)
                    .ok()
            })
            .unwrap_or(false);
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
    /// here, or have its name brought up to date in the raw text, which
    /// the one who planted it can read.
    pub fn names(&self) -> Vec<MentionName> {
        Self::names_of(self.known.get(), self.picked.get())
    }

    /// The author's own people among `names`, with their current names:
    /// the only labels the notice may offer to change.
    fn own(names: Vec<MentionName>) -> Vec<(PersonId, String)> {
        names
            .into_iter()
            .filter(|n| n.mine)
            .map(|n| (n.person_id, n.display_name))
            .collect()
    }

    fn own_untracked(&self) -> Vec<(PersonId, String)> {
        Self::own(Self::names_of(
            self.known.get_untracked(),
            self.picked.get_untracked(),
        ))
    }

    /// How many labels of the author's own people show an older name
    /// that the author has not chosen to keep.
    fn stale_count(&self) -> Option<usize> {
        refresh_labels(
            &self.value.get(),
            &Self::own(self.names()),
            &self.kept.get(),
        )
        .map(|(_, changed)| changed)
    }

    /// "Update": the older names brought up to date, as one edit of the
    /// box (it can be undone), worked out from the text as the box holds
    /// it now. Offered as an edit ([`Self::notice`]), never applied
    /// unasked.
    fn update_labels(&self) {
        let Some(el) = self.textarea.get_untracked() else {
            return;
        };
        let text = el.value();
        let Some((fresh, _)) =
            refresh_labels(&text, &self.own_untracked(), &self.kept.get_untracked())
        else {
            return;
        };
        let cursor = el.selection_start().ok().flatten().unwrap_or(0);
        let top = el.scroll_top();
        let whole = text.encode_utf16().count();
        self.edit(&el, 0, whole, &fresh, &fresh);
        // The edit left the cursor at the end, and the box scrolled there.
        let _ = el.set_selection_range(cursor, cursor);
        el.set_scroll_top(top);
    }

    /// "Keep as typed": the older names that are in the text now stay,
    /// and the notice goes. A name that goes stale later is offered.
    fn keep_labels(&self) {
        let stale = stale_labels(&self.text(), &self.own_untracked());
        self.kept.update(|kept| {
            for pair in stale {
                if !kept.contains(&pair) {
                    kept.push(pair);
                }
            }
        });
    }

    /// The notice that some mentions show an older name, with "Update"
    /// and "Keep as typed". In the page's flow, above the text box: over
    /// the box it hid the first line and could not be put away.
    pub fn notice(&self) -> impl IntoView + use<> {
        let picker = *self;
        move || {
            picker.stale_count().map(|changed| {
                view! {
                    <div class="flex flex-wrap items-center gap-x-3 gap-y-1 rounded-lg px-3 py-1.5 \
                            mb-1 text-xs bg-amber-50 dark:bg-amber-900/30 text-amber-900 \
                            dark:text-amber-200"
                        role="status" data-testid="stale-mentions">
                        <span>
                            {if changed == 1 {
                                "1 mention shows an older name.".to_string()
                            } else {
                                format!("{changed} mentions show an older name.")
                            }}
                        </span>
                        <button type="button" class="underline cursor-pointer"
                            on:click=move |_| picker.update_labels()>
                            "Update"
                        </button>
                        <button type="button" class="underline cursor-pointer"
                            on:click=move |_| picker.keep_labels()>
                            "Keep as typed"
                        </button>
                    </div>
                }
            })
        }
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
        Self::active_in(self.moved.get(), &options, &typing.query)
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
            let ready = Self::ready(&options, &query);
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
