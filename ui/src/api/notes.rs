use common::{
    id::{NodeId, NoteId},
    note::{CreateFeedNoteRequest, CreateNoteRequest, FeedNote, Note, NoteSort, UpdateNoteRequest},
};

use super::{delete_empty, get_json, patch_json, post_json};
use crate::error::UiError;

pub async fn fetch_notes(node_id: NodeId) -> Result<Vec<Note>, UiError> {
    get_json(&format!("/nodes/{node_id}/notes")).await
}

pub async fn create_note(node_id: NodeId, req: &CreateNoteRequest) -> Result<Note, UiError> {
    post_json(&format!("/nodes/{node_id}/notes"), req).await
}

/// Create a note from the Notes page, on the node it names (every note has
/// one, §9.2; quick thoughts are captures, #234).
pub async fn create_note_global(req: &CreateFeedNoteRequest) -> Result<Note, UiError> {
    post_json("/notes", req).await
}

pub async fn update_note(note_id: NoteId, req: &UpdateNoteRequest) -> Result<Note, UiError> {
    patch_json(&format!("/notes/{note_id}"), req).await
}

pub async fn delete_note(note_id: NoteId) -> Result<(), UiError> {
    delete_empty(&format!("/notes/{note_id}")).await
}

/// `POST /api/notes/:id/restore` — un-delete a soft-deleted note (undo toast).
pub async fn restore_note(note_id: NoteId) -> Result<Note, UiError> {
    post_json(&format!("/notes/{note_id}/restore"), &serde_json::json!({})).await
}

/// Rows fetched per "Load more" page of the notes feed. A page that comes back
/// full (== this many rows) means there may be more — the feed view uses that
/// to decide whether to keep the "Load more" control visible.
pub const FEED_PAGE_SIZE: u32 = 50;

/// Fetch one page of the notes feed with optional filters + sort.
/// `page` is 1-based; each page returns up to [`FEED_PAGE_SIZE`] rows.
pub async fn fetch_notes_feed(
    node_id: Option<NodeId>,
    from: Option<&str>,
    to: Option<&str>,
    q: Option<&str>,
    sort: NoteSort,
    page: u32,
) -> Result<Vec<FeedNote>, UiError> {
    let mut parts: Vec<String> = Vec::new();
    if let Some(n) = node_id {
        parts.push(format!("node_id={}", n.0));
    }
    if let Some(f) = from.filter(|s| !s.is_empty()) {
        parts.push(format!("from={f}"));
    }
    if let Some(t) = to.filter(|s| !s.is_empty()) {
        parts.push(format!("to={t}"));
    }
    if let Some(text) = q.filter(|s| !s.trim().is_empty()) {
        let enc: String = js_sys::encode_uri_component(text).into();
        parts.push(format!("q={enc}"));
    }
    let sort_str = match sort {
        NoteSort::Newest => "newest",
        NoteSort::Oldest => "oldest",
        NoteSort::Updated => "updated",
    };
    parts.push(format!("sort={sort_str}"));
    parts.push(format!("per_page={FEED_PAGE_SIZE}"));
    parts.push(format!("page={}", page.max(1)));
    get_json(&format!("/notes/feed?{}", parts.join("&"))).await
}

#[allow(dead_code)]
pub fn _use_note_id(_: NoteId) {}
