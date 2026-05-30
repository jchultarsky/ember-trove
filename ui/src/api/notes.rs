use common::{
    id::{NodeId, NoteId},
    note::{CreateNoteRequest, FeedNote, Note, NoteSort, UpdateNoteRequest},
};

use super::{get_json, patch_json, post_json};
use crate::error::UiError;

pub async fn fetch_notes(node_id: NodeId) -> Result<Vec<Note>, UiError> {
    get_json(&format!("/nodes/{node_id}/notes")).await
}

pub async fn create_note(node_id: NodeId, req: &CreateNoteRequest) -> Result<Note, UiError> {
    post_json(&format!("/nodes/{node_id}/notes"), req).await
}

/// Create a note from the global Notes view. With `req.node_id == None` this is
/// a standalone (inbox / micro-blog) note; with `Some(id)` it attaches to that node.
pub async fn create_note_global(req: &CreateNoteRequest) -> Result<Note, UiError> {
    post_json("/notes", req).await
}

pub async fn update_note(note_id: NoteId, req: &UpdateNoteRequest) -> Result<Note, UiError> {
    patch_json(&format!("/notes/{note_id}"), req).await
}

/// Fetch the notes feed with optional filters + sort.
/// `node_id` and `uncategorized` are mutually exclusive (the UI sends at most one).
pub async fn fetch_notes_feed(
    node_id: Option<NodeId>,
    uncategorized: bool,
    from: Option<&str>,
    to: Option<&str>,
    q: Option<&str>,
    sort: NoteSort,
) -> Result<Vec<FeedNote>, UiError> {
    let mut parts: Vec<String> = Vec::new();
    if let Some(n) = node_id {
        parts.push(format!("node_id={}", n.0));
    }
    if uncategorized {
        parts.push("uncategorized=true".to_string());
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
    get_json(&format!("/notes/feed?{}", parts.join("&"))).await
}

#[allow(dead_code)]
pub fn _use_note_id(_: NoteId) {}
