use common::editor_pref::{EditorPref, SetEditorPrefRequest};
use uuid::Uuid;

use super::{get_json, put_empty};
use crate::error::UiError;

/// All of the caller's saved per-item editor heights.
pub async fn fetch_editor_prefs() -> Result<Vec<EditorPref>, UiError> {
    get_json("/editor-prefs").await
}

/// Persist a resized editor height for one entity (`"task"` or `"note"`).
pub async fn set_editor_pref(entity_kind: &str, entity_id: Uuid, height: i32) -> Result<(), UiError> {
    let req = SetEditorPrefRequest {
        entity_kind: entity_kind.to_string(),
        entity_id,
        height,
    };
    put_empty("/editor-prefs", &req).await
}
