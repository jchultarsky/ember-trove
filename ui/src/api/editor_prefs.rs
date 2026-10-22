use common::editor_pref::{EditorPref, SetCollapsedRequest, SetEditorPrefRequest};
use uuid::Uuid;

use super::{get_json, put_empty};
use crate::error::UiError;

/// All of the caller's saved per-item editor prefs (heights, note collapse).
pub async fn fetch_editor_prefs() -> Result<Vec<EditorPref>, UiError> {
    get_json("/editor-prefs").await
}

/// Persist a resized editor height for one entity (`"task"` or `"note"`).
pub async fn set_editor_pref(
    entity_kind: &str,
    entity_id: Uuid,
    height: i32,
) -> Result<(), UiError> {
    let req = SetEditorPrefRequest {
        entity_kind: entity_kind.to_string(),
        entity_id,
        height: Some(height),
        collapsed: None,
    };
    put_empty("/editor-prefs", &req).await
}

/// Persist whether one note is collapsed to its title line.
pub async fn set_note_collapsed(note_id: Uuid, collapsed: bool) -> Result<(), UiError> {
    let req = SetEditorPrefRequest {
        entity_kind: "note".to_string(),
        entity_id: note_id,
        height: None,
        collapsed: Some(collapsed),
    };
    put_empty("/editor-prefs", &req).await
}

/// Collapse or expand many notes in one request (Collapse all / Expand all).
pub async fn set_notes_collapsed(note_ids: Vec<Uuid>, collapsed: bool) -> Result<(), UiError> {
    let req = SetCollapsedRequest {
        entity_kind: "note".to_string(),
        entity_ids: note_ids,
        collapsed,
    };
    put_empty("/editor-prefs/collapsed", &req).await
}
