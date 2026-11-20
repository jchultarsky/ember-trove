//! A node's History (S6d, #329; ONTOLOGY §9.2, §10): what occurred about
//! it, in one chronology, on every kind.
//!
//! The section that was "Notes". It still holds the node's notes, with the
//! add-note box on top, and between them the changes that happened to the
//! node (its status label or state changed, a cascade closed things, a
//! Brief was replaced, it was published or archived), each as a slim line
//! at the moment it happened, newest first.
//!
//! - The two are read by their own routes (`/notes`, and
//!   `/activity?scope=history`) and put together by
//!   `common::history::merge`. When the changes fail to load, the notes
//!   still show, and a line says so.
//! - A note sits where it was written. An edit never moves it; it gains
//!   an "edited" mark, from `edited_at` (set only when its words change).
//! - Edit and Delete are offered per note from what the API says this
//!   reader may do (`may_edit`, `may_remove`), not from who owns the node.
//! - The list is keyed and its frame does not redraw when a read brings
//!   notes again, so a reload (another note added, a state change
//!   elsewhere on the page) leaves an open note edit as it is. A note
//!   that was itself written again gets a new card.

use std::collections::{HashMap, HashSet};

use common::{
    activity::ActivityEntry,
    history::{Chronology, HistoryItem, initial_cut, merge},
    id::NodeId,
    note::{CreateNoteRequest, ReaderNote, UpdateNoteRequest},
};
use leptos::prelude::*;
use uuid::Uuid;

use crate::components::activity_lines::{
    action_colour, action_detail, action_label, actor_display, change_details,
};
use crate::components::format_helpers::format_timestamp;
use crate::components::icon_button::{IconButton, IconButtonVariant};
use crate::components::mention_picker::MentionPicker;
use crate::components::node_section::{NodeSection, SectionShell};
use crate::components::resizable_editor::ResizableEditor;
use crate::components::toast::{ToastLevel, ToastState, push_toast, push_undo_toast};
use crate::markdown::render_note;

/// Post-it palette — 5 warm colours + neutral default.
/// Each entry: (key, swatch inline-style, full Tailwind card classes).
/// All card class strings are written out in full so Tailwind's scanner picks them up.
const PALETTE: &[(&str, &str, &str)] = &[
    (
        "default",
        "background:#e7e5e4",
        "bg-stone-50 dark:bg-stone-900/50 border-stone-200 dark:border-stone-700",
    ),
    (
        "amber",
        "background:#fef3c7",
        "bg-amber-100 dark:bg-amber-950/60 border-amber-300 dark:border-amber-800",
    ),
    (
        "rose",
        "background:#ffe4e6",
        "bg-rose-100 dark:bg-rose-950/60 border-rose-300 dark:border-rose-800",
    ),
    (
        "lime",
        "background:#dcfce7",
        "bg-lime-100 dark:bg-lime-950/60 border-lime-300 dark:border-lime-800",
    ),
    (
        "sky",
        "background:#e0f2fe",
        "bg-sky-100 dark:bg-sky-950/60 border-sky-300 dark:border-sky-800",
    ),
    (
        "violet",
        "background:#ede9fe",
        "bg-violet-100 dark:bg-violet-950/60 border-violet-300 dark:border-violet-800",
    ),
];

fn palette_card_class(color: &str) -> &'static str {
    PALETTE
        .iter()
        .find(|(k, _, _)| *k == color)
        .map(|(_, _, cls)| *cls)
        .unwrap_or(PALETTE[0].2)
}

// ── ColorPicker ───────────────────────────────────────────────────────────────

#[component]
fn ColorPicker(selected: RwSignal<String>) -> impl IntoView {
    view! {
        <div class="flex items-center gap-1.5">
            {PALETTE.iter().map(|(key, swatch_style, _)| {
                let key_str  = key.to_string();
                let key_cmp  = key.to_string();
                let swatch   = swatch_style.to_string();
                view! {
                    <button
                        type="button"
                        title=key_str.clone()
                        style=format!("{swatch}; width:18px; height:18px; border-radius:50%; \
                            flex-shrink:0; transition:transform 0.1s;")
                        class=move || {
                            if selected.get() == key_cmp {
                                "ring-2 ring-offset-1 ring-amber-500 scale-110"
                            } else {
                                "ring-1 ring-stone-300 dark:ring-stone-600 hover:scale-110"
                            }
                        }
                        on:click={
                            let k = key_str.clone();
                            move |_| selected.set(k.clone())
                        }
                    />
                }
            }).collect_view()}
        </div>
    }
}

/// Pre-defined note structures: (label, Markdown body). The icon of each
/// is [`template_icon`].
const NOTE_TEMPLATES: &[(&str, &str)] = &[
    (
        "meeting",
        "## Meeting Notes\n\n**Date:** \n**Attendees:** \n\n## Agenda\n\n1. \n\n## Decisions\n\n- \n\n## Action Items\n\n- [ ] ",
    ),
    (
        "decision",
        "## Decision Record\n\n**Status:** Proposed\n**Date:** \n\n## Context\n\n\n\n## Decision\n\n\n\n## Consequences\n\n- ",
    ),
    (
        "status",
        "## Status Update\n\n**Period:** \n\n## Progress\n\n- \n\n## Blockers\n\n- \n\n## Next Steps\n\n- ",
    ),
    ("checklist", "## Checklist\n\n- [ ] \n- [ ] \n- [ ] "),
    (
        "idea",
        "## Idea\n\n**Problem:** \n\n**Proposed Solution:** \n\n**Open Questions:** \n\n- ",
    ),
];

/// The Material Symbols glyph of a note template. A function with `icon`
/// in its name, so `scripts/refresh-icon-font.sh` finds the names: held
/// in the table above they were never in the font, and the buttons showed
/// the words "groups", "gavel", "trending_up" and "lightbulb".
fn template_icon(label: &str) -> &'static str {
    match label {
        "meeting" => "groups",
        "decision" => "gavel",
        "status" => "trending_up",
        "checklist" => "checklist",
        _ => "lightbulb",
    }
}

// ── Notes ─────────────────────────────────────────────────────────────────────

/// What every note card of one History shares.
#[derive(Clone, Copy)]
struct NoteCtx {
    /// Bumped to read the notes again.
    refresh: RwSignal<u32>,
    /// The notes as last read.
    notes: RwSignal<Notes>,
    /// Notes collapsed to their title line: per account, in editor prefs.
    collapsed: RwSignal<HashSet<Uuid>>,
    /// A note deep-linked from the Notes feed shows expanded even if it
    /// is saved as collapsed, without changing the saved state.
    force_open: RwSignal<Option<Uuid>>,
    /// Saved editor heights, by note.
    heights: RwSignal<HashMap<Uuid, i32>>,
    deleting: RwSignal<bool>,
    toast: Option<ToastState>,
}

impl NoteCtx {
    fn is_collapsed(self, id: Uuid) -> bool {
        self.collapsed.with(|s| s.contains(&id)) && self.force_open.get() != Some(id)
    }

    /// Toggle one note: update the screen at once, then persist; on
    /// failure put it back and say so.
    fn toggle(self, id: Uuid) {
        let want_collapsed = !(self.collapsed.with_untracked(|s| s.contains(&id))
            && self.force_open.get_untracked() != Some(id));
        if self.force_open.get_untracked() == Some(id) {
            self.force_open.set(None);
        }
        let saved = self.collapsed.with_untracked(|s| s.contains(&id));
        if saved == want_collapsed {
            // Clearing the deep-link override already shows the wanted state.
            return;
        }
        let collapsed = self.collapsed;
        let apply = move |collapse: bool| {
            collapsed.try_update(|s| {
                if collapse {
                    s.insert(id);
                } else {
                    s.remove(&id);
                }
            });
        };
        apply(want_collapsed);
        wasm_bindgen_futures::spawn_local(async move {
            if let Err(e) = crate::api::set_note_collapsed(id, want_collapsed).await {
                apply(!want_collapsed);
                push_toast(ToastLevel::Error, format!("Couldn't save note state: {e}"));
            }
        });
    }
}

impl NoteCtx {
    /// Put a note the server just answered with in the list, in place of
    /// the one it replaces. The card is then drawn again from what was
    /// saved, at once: waiting for a reload left the old words on screen,
    /// and an Edit opened in that moment started from them.
    fn saved(self, saved: common::note::SavedNote) {
        self.notes.try_update(|notes| {
            if let Some(Ok(list)) = notes
                && let Some(slot) = list.iter_mut().find(|n| n.id == saved.note.id)
            {
                slot.note = saved.note;
                // The people it mentions now, as the writer may know them.
                slot.mentions = saved.mentions;
            }
        });
    }
}

/// A note's date line: when it was written, and "edited" when its words
/// were changed since (with when, on hover). The date never moves.
fn note_dates(note: &ReaderNote) -> (String, Option<String>) {
    (
        format_timestamp(&note.created_at),
        note.edited_at
            .map(|at| format!("Edited {}", format_timestamp(&at))),
    )
}

#[component]
fn NoteDate(written: String, edited: Option<String>) -> impl IntoView {
    view! {
        <span class="text-xs text-stone-400 dark:text-stone-500">
            {written}
            {edited.map(|when| view! {
                " \u{b7} "
                <span title=when.clone() aria-label=when data-testid="note-edited">"edited"</span>
            })}
        </span>
    }
}

/// One note of a History: its card, and its editor while it is being
/// edited. It lives as long as its key does, so a reload of the list
/// around it leaves an edit in progress alone.
#[component]
fn NoteCard(note: ReaderNote, ctx: NoteCtx) -> impl IntoView {
    let note_id = note.id;
    let (may_edit, may_remove) = (note.may_edit, note.may_remove);
    let (written, edited) = note_dates(&note);

    let editing = RwSignal::new(false);
    let edit_body = RwSignal::new(note.body.clone());
    let edit_color = RwSignal::new(note.color.clone());
    let orig_body = StoredValue::new(note.body.clone());
    let orig_color = StoredValue::new(note.color.clone());
    let save_error = RwSignal::new(Option::<String>::None);
    let saving = RwSignal::new(false);
    // S9: the `@` picker of this note's editor.
    let edit_ref: NodeRef<leptos::html::Textarea> = NodeRef::new();
    let mentions = MentionPicker::new(edit_body, edit_ref);
    mentions.set_known(note.mentions.clone());

    // Shared by Ctrl+Enter (editor on_submit) and the Save button; Escape
    // and Cancel share do_cancel.
    let do_save = Callback::new(move |()| {
        let new_body = edit_body.get_untracked().trim().to_string();
        if new_body.is_empty() || saving.get_untracked() {
            return;
        }
        let new_color = edit_color.get_untracked();
        // Nothing changed: nothing to send. (The text is trimmed before it
        // is sent, so a save of words that only differ by that would
        // otherwise count as an edit.)
        if new_body == orig_body.get_value().trim() && new_color == orig_color.get_value() {
            editing.set(false);
            save_error.set(None);
            return;
        }
        let req = UpdateNoteRequest {
            body: new_body,
            color: new_color,
        };
        saving.set(true);
        wasm_bindgen_futures::spawn_local(async move {
            let saved = crate::api::update_note(note_id, &req).await;
            saving.try_set(false);
            match saved {
                // The card's key carries the note's last write, so the
                // saved note replaces this card with one drawn from it.
                Ok(note) => {
                    editing.try_set(false);
                    save_error.try_set(None);
                    ctx.saved(note);
                }
                Err(e) => {
                    save_error.try_set(Some(format!("Couldn\u{2019}t save the note: {e}")));
                }
            }
        });
    });
    let do_cancel = Callback::new(move |()| {
        editing.set(false);
        edit_body.set(orig_body.get_value());
        edit_color.set(orig_color.get_value());
    });
    // Escape: cancel, but offer Undo if anything was changed. A stray
    // Escape used to discard a note edit silently (#123).
    let do_escape = Callback::new(move |()| {
        let typed = (edit_body.get_untracked(), edit_color.get_untracked());
        let dirty = typed.0 != orig_body.get_value() || typed.1 != orig_color.get_value();
        do_cancel.run(());
        if dirty {
            push_undo_toast(
                "Edit discarded",
                std::sync::Arc::new(move || {
                    edit_body.try_set(typed.0.clone());
                    edit_color.try_set(typed.1.clone());
                    editing.try_set(true);
                }),
            );
        }
    });

    // Markdown rendered once per card; a changed note gets a new card.
    // Its mentions are named as this reader may know them (S9): the
    // names came with the note.
    let body_html = render_note(&note.body, &note.mentions);
    let title = common::note::note_title(&note.body);
    let card_class = palette_card_class(&note.color).to_string();

    view! {
        <div
            class=format!("rounded-lg border px-3 py-2.5 {card_class}")
            data-note-id=note_id.0.to_string()
            data-testid="history-note"
        >
            // ── Display mode ──────────────────────────────────────────
            // The chevron toggles collapse; collapsed shows the title line
            // and the date (click to expand).
            {move || (!editing.get()).then({
                let body_html = body_html.clone();
                let title = title.clone();
                let (written, edited) = (written.clone(), edited.clone());
                move || {
                    let folded = ctx.is_collapsed(note_id.0);
                    let toggle_label = if folded { "Expand note" } else { "Collapse note" };
                    view! {
                        <div class="flex items-start gap-1.5">
                            <button
                                type="button"
                                class="mt-0.5 shrink-0 rounded
                                    text-stone-400 dark:text-stone-500
                                    hover:text-amber-500 dark:hover:text-amber-400
                                    transition-colors cursor-pointer"
                                title=toggle_label
                                aria-label=toggle_label
                                aria-expanded=(!folded).to_string()
                                on:click=move |_| ctx.toggle(note_id.0)
                            >
                                <span class="material-symbols-outlined" style="font-size: 16px;" aria-hidden="true">
                                    {if folded { "chevron_right" } else { "expand_more" }}
                                </span>
                            </button>
                            <div class="flex-1 min-w-0">
                                {if folded {
                                    view! {
                                        <button
                                            type="button"
                                            class="w-full flex items-baseline gap-2 text-left cursor-pointer"
                                            title="Expand note"
                                            data-testid="note-collapsed-title"
                                            on:click=move |_| ctx.toggle(note_id.0)
                                        >
                                            <span class="flex-1 min-w-0 truncate text-sm font-medium
                                                text-stone-800 dark:text-stone-200">
                                                {title.clone()}
                                            </span>
                                            <span class="shrink-0">
                                                <NoteDate written=written.clone() edited=edited.clone() />
                                            </span>
                                        </button>
                                    }.into_any()
                                } else {
                                    view! {
                                        <div
                                            class="prose prose-sm max-w-none dark:prose-invert
                                                prose-p:my-0.5 prose-ul:my-0.5 prose-ol:my-0.5
                                                prose-li:my-0 prose-headings:mt-1 prose-headings:mb-0.5"
                                            inner_html=body_html.clone()
                                        />
                                        <div class="flex items-center justify-between mt-1.5">
                                            <p><NoteDate written=written.clone() edited=edited.clone() /></p>
                                            // Offered only where it works: the API says, per
                                            // note, what this reader may do.
                                            {(may_edit || may_remove).then(|| view! {
                                                <div class="flex items-center gap-0.5">
                                                    {may_edit.then(|| view! {
                                                        <button
                                                            type="button"
                                                            class="p-0.5 rounded
                                                                text-stone-300 dark:text-stone-600
                                                                hover:text-amber-500 dark:hover:text-amber-400
                                                                transition-colors"
                                                            title="Edit note"
                                                            aria-label="Edit note"
                                                            on:click=move |_| editing.set(true)
                                                        >
                                                            <span aria-hidden="true" class="material-symbols-outlined"
                                                                style="font-size: 14px;">"edit"</span>
                                                        </button>
                                                    })}
                                                    // Delete: at once, with an Undo toast (soft delete).
                                                    {may_remove.then(|| view! {
                                                        <IconButton
                                                            icon="delete"
                                                            label="Delete note"
                                                            variant=IconButtonVariant::Danger
                                                            on_click=Callback::new(move |()| {
                                                                crate::note_actions::delete_note_with_undo(
                                                                    note_id,
                                                                    ctx.deleting,
                                                                    ctx.refresh,
                                                                    ctx.toast,
                                                                )
                                                            })
                                                        />
                                                    })}
                                                </div>
                                            })}
                                        </div>
                                    }.into_any()
                                }}
                            </div>
                        </div>
                    }
                }
            })}

            // ── Edit mode ─────────────────────────────────────────────
            {move || editing.get().then(move || view! {
                <ResizableEditor
                    value=edit_body
                    node_ref=edit_ref
                    mentions=mentions
                    placeholder="Note…"
                    initial_height=ctx.heights.get_untracked().get(&note_id.0).copied()
                    on_resize=Callback::new(move |h: i32| {
                        ctx.heights.update(|m| { m.insert(note_id.0, h); });
                        wasm_bindgen_futures::spawn_local(async move {
                            // Best-effort: losing a height pref is cosmetic.
                            let _ = crate::api::set_editor_pref("note", note_id.0, h).await;
                        });
                    })
                    on_submit=do_save
                    on_escape=do_escape
                    class="w-full bg-transparent text-sm text-stone-800 dark:text-stone-200 \
                        resize-y min-h-[80px] focus:outline-none".to_string()
                />
                {move || save_error.get().map(|msg| view! {
                    <p role="alert" class="text-xs text-red-500 mt-1">{msg}</p>
                })}
                <div class="flex items-center justify-between mt-2">
                    <ColorPicker selected=edit_color />
                    <div class="flex items-center gap-1">
                        <IconButton
                            icon="close"
                            label="Cancel"
                            on_click=do_cancel
                        />
                        <IconButton
                            icon="check"
                            label="Save"
                            variant=IconButtonVariant::Save
                            disabled=Signal::derive(move || edit_body.get().trim().is_empty() || saving.get())
                            on_click=do_save
                        />
                    </div>
                </div>
            })}
        </div>
    }
}

// ── Changes ───────────────────────────────────────────────────────────────────

/// One change in a History: a slim line (who, what, when), which opens to
/// show exactly what it holds when it holds more than the line says (a
/// cascade's list, a replaced Brief's words). All of it is text.
#[component]
fn ChangeLine(entry: ActivityEntry) -> impl IntoView {
    let icon = entry.action.icon();
    let label = action_label(&entry);
    let colour = action_colour(&entry.action);
    let actor = actor_display(&entry);
    let detail = action_detail(&entry);
    let when = format_timestamp(&entry.created_at);
    let blocks = StoredValue::new(change_details(&entry));
    let has_more = !blocks.with_value(Vec::is_empty);
    let open = RwSignal::new(false);

    view! {
        <div class="flex items-start gap-2 px-1 text-xs leading-relaxed"
            data-testid="history-change"
            data-action=entry.action.as_str()>
            <span class=format!(
                "mt-0.5 flex h-5 w-5 shrink-0 items-center justify-center rounded-full {colour}"
            )>
                <span class="material-symbols-outlined" style="font-size: 13px;" aria-hidden="true">{icon}</span>
            </span>
            <div class="min-w-0 flex-1 break-words">
                <span class="font-medium text-stone-800 dark:text-stone-200">{actor}</span>
                <span class="text-stone-500 dark:text-stone-400">{format!(" {label}")}</span>
                {detail.map(|d| view! {
                    <span class="text-stone-500 dark:text-stone-400">{format!(" \u{b7} {d}")}</span>
                })}
                <span class="text-stone-400 dark:text-stone-500">{format!(" \u{b7} {when}")}</span>
                {has_more.then(|| view! {
                    " "
                    <button
                        type="button"
                        class="inline-flex items-center align-middle rounded text-stone-400
                            hover:text-amber-600 dark:hover:text-amber-400 cursor-pointer"
                        title=move || if open.get() { "Hide what it holds" } else { "Show what it holds" }
                        aria-label=move || if open.get() { "Hide what it holds" } else { "Show what it holds" }
                        aria-expanded=move || if open.get() { "true" } else { "false" }
                        data-testid="history-change-toggle"
                        on:click=move |_| open.update(|v| *v = !*v)
                    >
                        <span class="material-symbols-outlined" style="font-size: 16px;" aria-hidden="true">
                            {move || if open.get() { "expand_less" } else { "expand_more" }}
                        </span>
                    </button>
                })}
                {move || open.get().then(|| view! {
                    <div class="mt-1 flex flex-col gap-1.5 text-stone-600 dark:text-stone-400"
                        data-testid="history-change-details">
                        {blocks.get_value().into_iter().map(|block| view! {
                            <div>
                                <p class="font-medium text-stone-700 dark:text-stone-300">{block.heading}</p>
                                {block.text.into_iter().map(|words| view! { <p>{words}</p> }).collect_view()}
                                {(!block.lines.is_empty()).then(|| view! {
                                    <ul class="pl-4 list-disc">
                                        {block.lines.into_iter().map(|line| view! { <li>{line}</li> }).collect_view()}
                                        {(block.more > 0).then(|| view! {
                                            <li class="list-none text-stone-400">{format!("+{} more", block.more)}</li>
                                        })}
                                    </ul>
                                })}
                            </div>
                        }).collect_view()}
                    </div>
                })}
            </div>
        </div>
    }
}

// ── HistoryPanel ──────────────────────────────────────────────────────────────

/// The notes as last read (`None` until the first answer). A failed
/// first read is `Err`; a failed later one keeps the notes it had.
type Notes = Option<Result<Vec<ReaderNote>, ()>>;

/// The changes as last read: the entries and whether the node has older
/// ones, or that the read failed.
type Changes = Option<Result<(Vec<ActivityEntry>, bool), ()>>;

#[component]
pub fn HistoryPanel(
    node_id: NodeId,
    /// Whether to offer the add-note box (the node's owner, or an admin).
    is_owner: bool,
    /// Bumped by the page when something on it changed the node's History
    /// (its state, its status label, its Brief), so the changes read again.
    #[prop(into)]
    changes_refresh: Signal<u32>,
) -> impl IntoView {
    let refresh = RwSignal::new(0u32);
    let changes_retry = RwSignal::new(0u32);
    let show_form = RwSignal::new(false);
    let body = RwSignal::new(String::new());
    let new_color = RwSignal::new("default".to_string());
    let adding = RwSignal::new(false);
    let add_error = RwSignal::new(None::<String>);
    let show_all = RwSignal::new(false);
    // Section open state (#131: SectionShell owns the toggle).
    let open = RwSignal::new(true);

    // The two reads. Each is numbered and only its latest answer is kept:
    // one sent before a change must not land over the one sent after.
    // `None` until the first answer; a later reload keeps what is shown
    // until the new answer arrives, so nothing flickers.
    let notes: RwSignal<Notes> = RwSignal::new(None);
    let changes: RwSignal<Changes> = RwSignal::new(None);
    // A later read of the notes failed: the ones on screen are kept, and
    // a line says they may be out of date.
    let notes_stale = RwSignal::new(false);

    let ctx = NoteCtx {
        refresh,
        notes,
        collapsed: RwSignal::new(HashSet::new()),
        force_open: RwSignal::new(
            crate::focus_note::pending_focus_note().and_then(|s| Uuid::parse_str(&s).ok()),
        ),
        heights: RwSignal::new(HashMap::new()),
        deleting: RwSignal::new(false),
        toast: use_context::<ToastState>(),
    };

    // Arrived via a `/nodes/:id?note=<id>` deep link from the Notes feed:
    // open the section and show everything, so the note is in the page for
    // `focus_note` to scroll to (it may be past the first few).
    if crate::focus_note::pending_focus_note().is_some() {
        show_all.set(true);
        open.set(true);
    }

    // The page's shared prefs (one fetch for the task and note panels).
    crate::components::task_row::EditorPrefs::with(move |prefs| {
        let (mut heights, mut folded) = (HashMap::new(), HashSet::new());
        for p in prefs.iter().filter(|p| p.entity_kind == "note") {
            if let Some(h) = p.height {
                heights.insert(p.entity_id, h);
            }
            if p.collapsed {
                folded.insert(p.entity_id);
            }
        }
        ctx.heights.try_set(heights);
        ctx.collapsed.try_set(folded);
    });

    let notes_read = StoredValue::new(0u32);
    let changes_read = StoredValue::new(0u32);
    Effect::new(move |_| {
        refresh.track();
        notes_read.update_value(|n| *n += 1);
        let this = notes_read.get_value();
        wasm_bindgen_futures::spawn_local(async move {
            let read = crate::api::fetch_notes(node_id).await.map_err(|_| ());
            if notes_read.try_get_value() != Some(this) {
                return;
            }
            // A failed re-read does not take away the notes already on
            // screen (and an edit open among them): it is said instead.
            let had_notes = notes
                .try_with_untracked(|n| matches!(n, Some(Ok(_))))
                .unwrap_or(false);
            if read.is_err() && had_notes {
                notes_stale.try_set(true);
            } else {
                notes_stale.try_set(false);
                notes.try_set(Some(read));
            }
        });
    });
    Effect::new(move |_| {
        changes_refresh.track();
        changes_retry.track();
        changes_read.update_value(|n| *n += 1);
        let this = changes_read.get_value();
        wasm_bindgen_futures::spawn_local(async move {
            let read = crate::api::fetch_history_changes(node_id)
                .await
                .map_err(|_| ());
            if changes_read.try_get_value() == Some(this) {
                changes.try_set(Some(read));
            }
        });
    });

    // The chronology: the notes, with the changes between them once they
    // are read. Without the notes there is none (their failure is the
    // section's); without the changes it is the notes alone.
    let chronology = RwSignal::new(None::<Chronology>);
    Effect::new(move |_| {
        let merged = notes.get().and_then(Result::ok).map(|notes| {
            let (entries, more) = changes.get().and_then(Result::ok).unwrap_or_default();
            merge(notes, entries, more)
        });
        chronology.set(merged);
    });
    // What the section's frame depends on: whether the notes are loading,
    // failed, or here. Memoised, so a reload that brings notes again does
    // not redraw the frame: the keyed list below then keeps every card
    // that did not change, an open editor included.
    let phase = Memo::new(move |_| notes.with(|n| n.as_ref().map(Result::is_ok)));
    // The changes have answered, well or not. Until then "nothing yet"
    // would be a guess, and after a failure it would be untrue.
    let changes_known = move || matches!(changes.get(), Some(Ok(_)));
    let total = move || chronology.with(|c| c.as_ref().map_or(0, |c| c.items.len()));
    let cut = move || chronology.with(|c| c.as_ref().map_or(0, |c| initial_cut(&c.items)));
    let visible = move || {
        let take = if show_all.get() { usize::MAX } else { cut() };
        chronology.with(|c| {
            c.as_ref()
                .map(|c| c.items.iter().take(take).cloned().collect::<Vec<_>>())
                .unwrap_or_default()
        })
    };

    // Ids of this node's notes, for Collapse all / Expand all.
    let note_ids = move || -> Vec<Uuid> {
        notes
            .get()
            .and_then(Result::ok)
            .map(|notes| notes.iter().map(|n| n.id.0).collect())
            .unwrap_or_default()
    };
    let all_collapsed = move || {
        let ids = note_ids();
        !ids.is_empty() && ids.iter().all(|id| ctx.is_collapsed(*id))
    };
    // Collapse or expand every note on the node in one request per
    // MAX_BULK_COLLAPSE ids; on failure restore the previous state.
    let set_all = move |collapse: bool| {
        let ids = note_ids();
        if ids.is_empty() {
            return;
        }
        let previous = ctx.collapsed.get_untracked();
        ctx.force_open.set(None);
        ctx.collapsed.update(|s| {
            for id in &ids {
                if collapse {
                    s.insert(*id);
                } else {
                    s.remove(id);
                }
            }
        });
        wasm_bindgen_futures::spawn_local(async move {
            for chunk in ids.chunks(common::editor_pref::MAX_BULK_COLLAPSE) {
                if let Err(e) = crate::api::set_notes_collapsed(chunk.to_vec(), collapse).await {
                    ctx.collapsed.try_set(previous);
                    push_toast(ToastLevel::Error, format!("Couldn't save note state: {e}"));
                    return;
                }
            }
        });
    };

    // S9: the `@` picker of the add box.
    let add_ref: NodeRef<leptos::html::Textarea> = NodeRef::new();
    let add_mentions = MentionPicker::new(body, add_ref);
    let do_add = move || {
        let trimmed = body.get_untracked().trim().to_string();
        if trimmed.is_empty() || adding.get_untracked() {
            return;
        }
        let req = CreateNoteRequest {
            body: trimmed,
            color: new_color.get_untracked(),
        };
        adding.set(true);
        add_error.set(None);
        wasm_bindgen_futures::spawn_local(async move {
            let made = crate::api::create_note(node_id, &req).await;
            adding.try_set(false);
            match made {
                Ok(_) => {
                    body.try_set(String::new());
                    new_color.try_set("default".to_string());
                    show_form.try_set(false);
                    refresh.try_update(|n| *n += 1);
                }
                // Said, with the text left in the box: it used to fail
                // without a word.
                Err(e) => {
                    add_error.try_set(Some(format!("Couldn\u{2019}t save the note: {e}")));
                }
            }
        });
    };

    // Collapsing the section closes the add form.
    Effect::new(move |_| {
        if !open.get() {
            show_form.set(false);
        }
    });
    // Every line, once both reads have answered: counted earlier it would
    // show the notes alone and then jump.
    let count = Signal::derive(move || {
        changes
            .with(Option::is_some)
            .then(|| chronology.with(|c| c.as_ref().map(|c| c.items.len())))
            .flatten()
    });
    let header_actions: ChildrenFn = std::sync::Arc::new(move || {
        view! {
            <div class="flex items-center gap-1">
                // Collapse all / Expand all: any viewer (the state is per
                // account). It is about the notes; a change is one line.
                {move || (open.get() && !note_ids().is_empty()).then(|| view! {
                    <button
                        type="button"
                        class=crate::components::icon_button::icon_button_class(
                            IconButtonVariant::Neutral,
                        )
                        title=move || if all_collapsed() { "Expand all notes" } else { "Collapse all notes" }
                        aria-label=move || if all_collapsed() { "Expand all notes" } else { "Collapse all notes" }
                        on:click=move |_| set_all(!all_collapsed())
                    >
                        <span class="material-symbols-outlined" style="font-size: 18px;" aria-hidden="true">
                            {move || if all_collapsed() { "unfold_more" } else { "unfold_less" }}
                        </span>
                    </button>
                })}
                {move || (is_owner && open.get()).then(|| view! {
                    <button
                        type="button"
                        class="p-1.5 rounded-lg text-stone-400 hover:text-stone-600 dark:hover:text-stone-300
                            hover:bg-stone-100 dark:hover:bg-stone-800 transition-colors cursor-pointer"
                        title=move || if show_form.get() { "Cancel" } else { "Add note" }
                        aria-label=move || if show_form.get() { "Cancel" } else { "Add note" }
                        on:click=move |_| show_form.update(|v| *v = !*v)
                    >
                        <span class="material-symbols-outlined" style="font-size: 18px;" aria-hidden="true">
                            {move || if show_form.get() { "close" } else { "add" }}
                        </span>
                    </button>
                })}
            </div>
        }
        .into_any()
    });

    view! {
        <SectionShell section=NodeSection::History open=open count=count actions=header_actions>
            // Add note form (owner only, toggled)
            {move || (is_owner && show_form.get()).then(|| view! {
                <div class="mb-4 rounded-lg bg-stone-50 dark:bg-stone-900/50
                    border border-stone-200 dark:border-stone-700 p-3">
                    <ResizableEditor
                        value=body
                        node_ref=add_ref
                        mentions=add_mentions
                        placeholder="Write a note… (Ctrl+Enter to save)"
                        on_submit=Callback::new(move |()| do_add())
                        class="w-full bg-transparent text-sm text-stone-800 dark:text-stone-200 \
                            placeholder:text-stone-400 dark:placeholder:text-stone-600 \
                            resize-y min-h-[64px] focus:outline-none".to_string()
                    />
                    // ── Template picker ───────────────────
                    <div class="flex flex-wrap items-center gap-1.5 mt-2 mb-2">
                        <span class="text-[10px] uppercase tracking-wider text-stone-400
                            dark:text-stone-500 mr-0.5">"Template"</span>
                        {NOTE_TEMPLATES.iter().map(|(label, tmpl)| {
                            let (label, tmpl) = (*label, *tmpl);
                            let icon = template_icon(label);
                            view! {
                                <button
                                    type="button"
                                    title=label
                                    class="inline-flex items-center gap-1 px-2 py-0.5 rounded text-[11px]
                                        text-stone-500 dark:text-stone-400 bg-stone-100 dark:bg-stone-800
                                        hover:bg-amber-100 dark:hover:bg-amber-900/40
                                        hover:text-amber-700 dark:hover:text-amber-300
                                        transition-colors cursor-pointer"
                                    on:click=move |_| body.set(tmpl.to_string())
                                >
                                    <span class="material-symbols-outlined" style="font-size: 14px;" aria-hidden="true">{icon}</span>
                                    {label}
                                </button>
                            }
                        }).collect_view()}
                    </div>
                    {move || add_error.get().map(|msg| view! {
                        <p role="alert" class="text-xs text-red-500 mb-2">{msg}</p>
                    })}
                    <div class="flex items-center justify-between">
                        <ColorPicker selected=new_color />
                        <button
                            type="button"
                            class="p-1.5 rounded-lg text-stone-400 hover:text-green-600 dark:hover:text-green-400
                                hover:bg-green-50 dark:hover:bg-green-900/30 transition-colors cursor-pointer
                                disabled:opacity-40 disabled:cursor-not-allowed"
                            title="Save note"
                            aria-label="Save note"
                            disabled=move || body.get().trim().is_empty() || adding.get()
                            on:click=move |_| do_add()
                        >
                            <span aria-hidden="true" class="material-symbols-outlined">"check"</span>
                        </button>
                    </div>
                </div>
            })}

            // The chronology
            {move || match phase.get() {
                None => view! {
                    <p class="text-xs text-stone-400">"Loading\u{2026}"</p>
                }.into_any(),
                // A failed load is not "nothing yet" (#281, 1.7).
                Some(false) => view! {
                    <crate::components::load_error::LoadError
                        what="this node\u{2019}s history"
                        on_retry=Callback::new(move |()| refresh.update(|n| *n += 1))
                    />
                }.into_any(),
                Some(true) => view! {
                    // (d) Empty only when both reads say so (#281, 1.7).
                    {move || (total() == 0 && changes_known()).then(|| view! {
                        <p class="text-xs text-stone-400 dark:text-stone-600 italic py-2"
                            data-testid="history-empty">
                            "Nothing yet. Notes, and changes to this node, show here in order."
                        </p>
                    })}
                    <div class=move || {
                        if show_all.get() && total() > cut() {
                            "max-h-[480px] overflow-y-auto pr-1"
                        } else { "" }
                    }>
                        // Keyed: a reload moves nothing that did not change,
                        // and a note being edited stays as it is. A note's
                        // key carries its last write, so a changed note is
                        // drawn again.
                        <div class="space-y-3" data-testid="history-list">
                            <For
                                each=visible
                                key=|item| match item {
                                    HistoryItem::Note(note) => {
                                        format!("{}@{}", item.key(), note.updated_at.timestamp_micros())
                                    }
                                    HistoryItem::Change(_) => item.key(),
                                }
                                children=move |item| match item {
                                    HistoryItem::Note(note) => view! { <NoteCard note=note ctx=ctx /> }.into_any(),
                                    HistoryItem::Change(entry) => view! { <ChangeLine entry=entry /> }.into_any(),
                                }
                            />
                        </div>
                    </div>

                    // "Show N more" / "Show less"
                    {move || {
                        let hidden = total().saturating_sub(cut());
                        (hidden > 0).then(|| view! {
                            <button
                                type="button"
                                class="mt-3 text-xs text-stone-400 dark:text-stone-500
                                    hover:text-amber-600 dark:hover:text-amber-400
                                    transition-colors cursor-pointer"
                                data-testid="history-show-more"
                                on:click=move |_| show_all.update(|v| *v = !*v)
                            >
                                {move || if show_all.get() {
                                    "Show less".to_string()
                                } else {
                                    format!("Show {hidden} more\u{2026}")
                                }}
                            </button>
                        })
                    }}

                    // The changes are a second read. When it stops short or
                    // fails, the notes above are still all there: say which.
                    {move || chronology.with(|c| c.as_ref().and_then(|c| c.changes_cut_at)).map(|at| view! {
                        <p class="mt-3 text-xs text-stone-400 dark:text-stone-500" data-testid="history-cut">
                            {format!(
                                "Changes before {} aren\u{2019}t shown here. The notes are all here.",
                                format_timestamp(&at)
                            )}
                        </p>
                    })}
                    {move || notes_stale.get().then(|| view! {
                        <p class="mt-3 text-xs text-stone-500 dark:text-stone-400" role="alert"
                            data-testid="history-notes-stale">
                            "Couldn\u{2019}t read the notes again, so these may be out of date. "
                            <button type="button"
                                class="underline underline-offset-2 hover:text-amber-700 dark:hover:text-amber-400 cursor-pointer"
                                on:click=move |_| refresh.update(|n| *n += 1)>
                                "Try again"
                            </button>
                        </p>
                    })}
                    {move || matches!(changes.get(), Some(Err(()))).then(|| view! {
                        <p class="mt-3 text-xs text-stone-500 dark:text-stone-400" role="alert"
                            data-testid="history-changes-error">
                            "Couldn\u{2019}t load the changes to this node. The notes are all here. "
                            <button type="button"
                                class="underline underline-offset-2 hover:text-amber-700 dark:hover:text-amber-400 cursor-pointer"
                                on:click=move |_| changes_retry.update(|n| *n += 1)>
                                "Try again"
                            </button>
                        </p>
                    })}
                }.into_any(),
            }}
        </SectionShell>
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_template_has_a_glyph_in_the_font() {
        // A name the subset lacks renders as its raw text ("gavel").
        let names = include_str!("../../icon-names.txt");
        for (label, _) in NOTE_TEMPLATES {
            let icon = template_icon(label);
            assert!(
                names.lines().any(|n| n == icon),
                "{icon} is not in ui/icon-names.txt: run scripts/refresh-icon-font.sh"
            );
        }
    }

    #[test]
    fn an_unknown_colour_falls_back_to_the_plain_card() {
        assert_eq!(palette_card_class("neon"), palette_card_class("default"));
        assert_ne!(palette_card_class("amber"), palette_card_class("default"));
    }
}
