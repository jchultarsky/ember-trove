use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

use crate::id::{AttachmentId, NodeId};

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct Attachment {
    pub id: AttachmentId,
    pub node_id: NodeId,
    pub filename: String,
    pub content_type: String,
    pub size_bytes: i64,
    pub s3_key: String,
    pub created_at: DateTime<Utc>,
}
