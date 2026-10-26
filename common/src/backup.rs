use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;
use uuid::Uuid;

use crate::{
    attachment::Attachment, edge::Edge, favorite::Favorite, graph::NodePosition, node::Node,
    node_link::NodeLink, node_version::NodeVersion, note::Note, permission::Permission, tag::Tag,
    task::Task,
};

/// Entity count summary embedded in the manifest and returned for previews.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct EntityCounts {
    pub nodes: u32,
    pub edges: u32,
    pub tags: u32,
    pub notes: u32,
    pub tasks: u32,
    pub attachments: u32,
}

/// Top-level manifest written as `manifest.json` inside the archive.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct BackupManifest {
    /// Monotonically increasing format version. Current: 1.
    pub schema_version: u32,
    pub created_at: DateTime<Utc>,
    pub created_by: String,
    pub entity_counts: EntityCounts,
}

/// A recorded backup job stored in `backup_jobs`.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct BackupJob {
    pub id: Uuid,
    pub created_by: String,
    pub created_at: DateTime<Utc>,
    pub size_bytes: i64,
    pub s3_key: String,
    pub node_count: i32,
    pub edge_count: i32,
    pub tag_count: i32,
    pub note_count: i32,
    pub task_count: i32,
    pub attachment_count: i32,
    /// Optional user-provided comment describing the purpose of this backup.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub comment: Option<String>,
}

/// Full data payload written as `data.json` inside the archive.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BackupData {
    pub nodes: Vec<Node>,
    pub edges: Vec<Edge>,
    pub tags: Vec<Tag>,
    pub notes: Vec<Note>,
    pub tasks: Vec<Task>,
    /// Attachment metadata only; raw bytes are stored as separate entries in the archive.
    pub attachments: Vec<Attachment>,
    // ── Added in schema_version 2 ─────────────────────────────────────────
    /// External links attached to nodes.
    #[serde(default)]
    pub node_links: Vec<NodeLink>,
    /// Sidebar favorites (both node-pinned and URL bookmarks).
    #[serde(default)]
    pub favorites: Vec<Favorite>,
    /// Sharing permissions.
    #[serde(default)]
    pub permissions: Vec<Permission>,
    // `share_tokens` was here until public share links were removed (#147).
    // Older archives still carry it; serde ignores the unknown field, so they
    // restore without their (now meaningless) tokens.
    /// Node body version history.
    #[serde(default)]
    pub node_versions: Vec<NodeVersion>,
    /// Graph canvas positions.
    #[serde(default)]
    pub node_positions: Vec<NodePosition>,
}

/// Returned by the preview endpoint before a restore is confirmed.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct BackupPreview {
    pub job_id: Uuid,
    pub created_at: DateTime<Utc>,
    pub entity_counts: EntityCounts,
    /// Human-readable warnings the user should acknowledge before restoring.
    pub warnings: Vec<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_archive_with_share_tokens_still_restores() {
        // Archives made before #147 carry `share_tokens`; the field is gone,
        // and restoring must not fail on it.
        let json = r#"{
            "nodes": [], "edges": [], "tags": [], "notes": [], "tasks": [],
            "attachments": [],
            "share_tokens": [{
                "id": "00000000-0000-0000-0000-000000000001",
                "node_id": "00000000-0000-0000-0000-000000000002",
                "token": "00000000-0000-0000-0000-000000000003",
                "created_by": "someone",
                "created_at": "2026-01-01T00:00:00Z",
                "expires_at": null
            }]
        }"#;
        let data: BackupData = serde_json::from_str(json).expect("old archive must parse");
        assert!(data.nodes.is_empty() && data.permissions.is_empty());
    }
}
