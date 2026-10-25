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
    /// The types a person may choose when connecting nodes: everything but
    /// `WikiLink`, which only the `[[link]]` sync creates. One list for the
    /// Connections form and the graph's New Edge dialog (#133 — they had
    /// hand-written copies that already disagreed: "Related to" vs
    /// "Related To").
    pub const USER_SELECTABLE: &'static [EdgeType] = &[
        EdgeType::References,
        EdgeType::Contains,
        EdgeType::RelatedTo,
        EdgeType::DependsOn,
        EdgeType::DerivedFrom,
    ];

    /// The label as an option shows it: "Related to".
    pub fn option_label(&self) -> String {
        let label = self.label();
        let mut chars = label.chars();
        chars
            .next()
            .map(|c| c.to_uppercase().chain(chars).collect())
            .unwrap_or_default()
    }

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

#[cfg(test)]
mod option_tests {
    use super::EdgeType;

    #[test]
    fn user_selectable_is_every_type_but_wiki_link() {
        let mut expected: Vec<_> = EdgeType::ALL
            .iter()
            .filter(|t| **t != EdgeType::WikiLink)
            .collect();
        let mut got: Vec<_> = EdgeType::USER_SELECTABLE.iter().collect();
        expected.sort_by_key(|t| t.as_str());
        got.sort_by_key(|t| t.as_str());
        assert_eq!(got, expected);
    }

    #[test]
    fn option_labels_are_sentence_case() {
        assert_eq!(EdgeType::RelatedTo.option_label(), "Related to");
        assert_eq!(EdgeType::References.option_label(), "References");
        assert_eq!(EdgeType::DerivedFrom.option_label(), "Derived from");
    }
}
