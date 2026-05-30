use common::{
    id::{NodeId, NoteId},
    note::{CreateNoteRequest, FeedNote, Note, UpdateNoteRequest},
};

use super::{get_json, patch_json, post_json};
use crate::error::UiError;

pub async fn fetch_notes(node_id: NodeId) -> Result<Vec<Note>, UiError> {
    get_json(&format!("/nodes/{node_id}/notes")).await
}

pub async fn create_note(node_id: NodeId, req: &CreateNoteRequest) -> Result<Note, UiError> {
    post_json(&format!("/nodes/{node_id}/notes"), req).await
}

pub async fn update_note(note_id: NoteId, req: &UpdateNoteRequest) -> Result<Note, UiError> {
    patch_json(&format!("/notes/{note_id}"), req).await
}

pub async fn fetch_notes_feed() -> Result<Vec<FeedNote>, UiError> {
    get_json("/notes/feed").await
}

#[allow(dead_code)]
pub fn _use_note_id(_: NoteId) {}
