use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

use crate::id::{EdgeId, NodeId};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum EdgeType {
    References,
    Contains,
    RelatedTo,
    DependsOn,
    DerivedFrom,
    /// Automatically created from `[[node title]]` wiki-link syntax in node bodies.
    WikiLink,
}

wire_enum!(EdgeType {
    References => "references",
    Contains => "contains",
    RelatedTo => "related_to",
    DependsOn => "depends_on",
    DerivedFrom => "derived_from",
    WikiLink => "wiki_link",
});

impl EdgeType {
    /// Human label, e.g. "related to". The UI used to derive this from
    /// `format!("{:?}")`, which yields "relatedto" (Debug has no underscores).
    pub fn label(&self) -> &'static str {
        match self {
            EdgeType::References => "references",
            EdgeType::Contains => "contains",
            EdgeType::RelatedTo => "related to",
            EdgeType::DependsOn => "depends on",
            EdgeType::DerivedFrom => "derived from",
            EdgeType::WikiLink => "wiki link",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct Edge {
    pub id: EdgeId,
    pub source_id: NodeId,
    pub target_id: NodeId,
    pub edge_type: EdgeType,
    pub label: Option<String>,
    pub created_at: DateTime<Utc>,
}

/// Edge enriched with source and target node titles — returned by the node-scoped edge list.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct EdgeWithTitles {
    pub id: EdgeId,
    pub source_id: NodeId,
    pub source_title: String,
    pub target_id: NodeId,
    pub target_title: String,
    pub edge_type: EdgeType,
    pub label: Option<String>,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema, garde::Validate)]
pub struct CreateEdgeRequest {
    #[garde(skip)]
    pub source_id: NodeId,
    #[garde(skip)]
    pub target_id: NodeId,
    #[garde(skip)]
    pub edge_type: EdgeType,
    #[garde(inner(length(max = 200)))]
    pub label: Option<String>,
}

#[cfg(test)]
mod edge_type_tests {
    use super::*;

    #[test]
    fn as_str_matches_serde_and_labels_are_spaced() {
        for t in EdgeType::ALL {
            assert_eq!(t.label(), t.as_str().replace('_', " "));
        }
        assert_eq!(EdgeType::RelatedTo.label(), "related to");
    }
}
