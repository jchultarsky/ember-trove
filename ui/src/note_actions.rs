//! Note actions shared by the Notes feed (`notes_view`) and a node's note
//! panel (`note_panel`), so both surfaces delete notes the same way.

use common::id::NoteId;
use leptos::prelude::*;

use crate::components::toast::{ToastLevel, ToastState, push_toast, push_undo_toast};

/// Soft-delete a note, then offer an Undo toast that restores it.
///
/// Instant delete + undo (the API tombstones the row and its `/restore` route
/// un-deletes it); both are owner-scoped server-side. `deleting` guards
/// against a double click firing two deletes. `on_change` runs after the
/// delete and again after a successful undo so the caller can refetch its
/// list. `toast_state` must be captured during component setup: the undo
/// closure runs after an `.await`, where there is no reactive owner to
/// `use_context` from (see `.claude/rules/leptos.md`).
pub fn delete_note_with_undo(
    note_id: NoteId,
    deleting: RwSignal<bool>,
    on_change: Callback<()>,
    toast_state: Option<ToastState>,
) {
    if deleting.get_untracked() {
        return;
    }
    deleting.set(true);
    wasm_bindgen_futures::spawn_local(async move {
        let result = crate::api::delete_note(note_id).await;
        deleting.set(false);
        match result {
            Ok(_) => {
                on_change.run(());
                push_undo_toast(
                    "Note deleted",
                    std::sync::Arc::new(move || {
                        wasm_bindgen_futures::spawn_local(async move {
                            match crate::api::restore_note(note_id).await {
                                Ok(_) => on_change.run(()),
                                Err(e) => {
                                    if let Some(ts) = toast_state {
                                        ts.push(ToastLevel::Error, format!("Undo failed: {e}"));
                                    }
                                }
                            }
                        });
                    }),
                );
            }
            Err(e) => push_toast(ToastLevel::Error, format!("Delete failed: {e}")),
        }
    });
}
