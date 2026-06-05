use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;
use uuid::Uuid;

use crate::{
    attachment::Attachment, edge::Edge, favorite::Favorite, graph::NodePosition, node::Node,
    node_link::NodeLink, node_version::NodeVersion, note::Note, permission::Permission,
    share_token::ShareToken, tag::Tag, task::Task,
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
    /// Public share tokens.
    #[serde(default)]
    pub share_tokens: Vec<ShareToken>,
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
