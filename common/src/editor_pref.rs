use garde::Validate;
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;
use uuid::Uuid;

/// A saved editor UI preference for one entity (task or note) — currently the
/// resized editor height in pixels.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct EditorPref {
    /// `"task"` or `"note"`.
    pub entity_kind: String,
    pub entity_id: Uuid,
    pub height: i32,
}

/// Request to persist a resized editor height for one entity.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema, Validate)]
pub struct SetEditorPrefRequest {
    #[garde(length(min = 1, max = 8))]
    pub entity_kind: String,
    #[garde(skip)]
    pub entity_id: Uuid,
    /// Editor height in pixels; clamped to a sane range.
    #[garde(range(min = 60, max = 4000))]
    pub height: i32,
}
