use garde::Validate;
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;
use uuid::Uuid;

/// Largest batch a single "collapse all / expand all" request may touch.
pub const MAX_BULK_COLLAPSE: usize = 500;

/// Saved per-user UI preferences for one entity (task or note): the resized
/// editor height and, for notes, whether the note is collapsed in its list.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct EditorPref {
    /// `"task"` or `"note"`.
    pub entity_kind: String,
    pub entity_id: Uuid,
    /// Resized editor height in pixels; `None` when only other prefs were saved.
    #[serde(default)]
    pub height: Option<i32>,
    /// Note collapsed to its title line. Absent / `false` means expanded.
    #[serde(default)]
    pub collapsed: bool,
}

/// Request to persist UI prefs for one entity. Each field is optional and only
/// the fields sent are changed — saving a height never resets `collapsed` and
/// vice versa. At least one of them must be present.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema, Validate)]
pub struct SetEditorPrefRequest {
    #[garde(length(min = 1, max = 8))]
    pub entity_kind: String,
    #[garde(skip)]
    pub entity_id: Uuid,
    /// Editor height in pixels; clamped to a sane range.
    #[garde(inner(range(min = 60, max = 4000)))]
    #[serde(default)]
    pub height: Option<i32>,
    /// Collapse (`true`) or expand (`false`) a note. Notes only.
    #[garde(skip)]
    #[serde(default)]
    pub collapsed: Option<bool>,
}

/// Request to collapse or expand many notes at once (Collapse all / Expand all).
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema, Validate)]
pub struct SetCollapsedRequest {
    #[garde(length(min = 1, max = 8))]
    pub entity_kind: String,
    #[garde(length(min = 1, max = MAX_BULK_COLLAPSE))]
    pub entity_ids: Vec<Uuid>,
    #[garde(skip)]
    pub collapsed: bool,
}
