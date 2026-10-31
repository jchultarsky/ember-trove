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

    /// Whether the type has no direction (§6.2, #226): `related_to` only.
    /// It is stored once per pair, either end may delete it, and the UI
    /// shows it as ↔ from both ends.
    #[must_use]
    pub fn is_symmetric(&self) -> bool {
        *self == EdgeType::RelatedTo
    }

    /// Whether a loop of this type is a modelling error that writes refuse
    /// (#224): a home (§6.1), a prerequisite (§6.4) and an origin (§6.5)
    /// can't lead back to themselves. Links and mentions may loop.
    #[must_use]
    pub fn is_acyclic(&self) -> bool {
        matches!(
            self,
            EdgeType::Contains | EdgeType::DependsOn | EdgeType::DerivedFrom
        )
    }

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

/// Merge `related_to` edges that link the same two nodes (#226, §6.2):
/// the type is unordered, so A→B and B→A are one edge. The oldest of each
/// pair (by `created_at`, then id) is kept. A kept edge with no label takes
/// the oldest non-empty label among the rest. This is migration 041's rule,
/// and restore uses it for archives that still hold both directions.
///
/// Returns the edges to keep, in their original order, and the dropped ones.
#[must_use]
pub fn fold_related_pairs(edges: Vec<Edge>) -> (Vec<Edge>, Vec<Edge>) {
    use std::collections::HashMap;

    let pair = |e: &Edge| {
        let (a, b) = (e.source_id.0, e.target_id.0);
        if a <= b { (a, b) } else { (b, a) }
    };
    let older = |a: &Edge, b: &Edge| (a.created_at, a.id.0) < (b.created_at, b.id.0);
    // `btrim(label) = ''` in the migration: spaces only, like SQL's btrim.
    let blank = |l: &Option<String>| l.as_deref().is_none_or(|l| l.trim_matches(' ').is_empty());

    // Per pair: the index of the edge to keep.
    let mut keep: HashMap<_, usize> = HashMap::new();
    for (i, e) in edges.iter().enumerate() {
        if e.edge_type != EdgeType::RelatedTo {
            continue;
        }
        keep.entry(pair(e))
            .and_modify(|k| {
                if older(e, &edges[*k]) {
                    *k = i;
                }
            })
            .or_insert(i);
    }
    // Per kept edge without a label: the oldest non-empty label of the rest.
    let mut carried: HashMap<usize, &Edge> = HashMap::new();
    for e in edges.iter().filter(|e| e.edge_type == EdgeType::RelatedTo) {
        let Some(&k) = keep.get(&pair(e)) else {
            continue;
        };
        if std::ptr::eq(e, &edges[k]) || !blank(&edges[k].label) || blank(&e.label) {
            continue;
        }
        carried
            .entry(k)
            .and_modify(|c| {
                if older(e, c) {
                    *c = e;
                }
            })
            .or_insert(e);
    }
    let labels: HashMap<usize, Option<String>> = carried
        .into_iter()
        .map(|(k, e)| (k, e.label.clone()))
        .collect();

    let (mut kept, mut dropped) = (Vec::new(), Vec::new());
    for (i, mut e) in edges.into_iter().enumerate() {
        if e.edge_type == EdgeType::RelatedTo && keep.get(&pair(&e)) != Some(&i) {
            dropped.push(e);
            continue;
        }
        if let Some(label) = labels.get(&i) {
            e.label = label.clone();
        }
        kept.push(e);
    }
    (kept, dropped)
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
    fn only_related_to_is_symmetric() {
        let symmetric: Vec<_> = EdgeType::ALL.iter().filter(|t| t.is_symmetric()).collect();
        assert_eq!(symmetric, [&EdgeType::RelatedTo]);
    }

    #[test]
    fn only_homes_prerequisites_and_origins_are_acyclic() {
        let acyclic: Vec<_> = EdgeType::ALL.iter().filter(|t| t.is_acyclic()).collect();
        assert_eq!(
            acyclic,
            [
                &EdgeType::Contains,
                &EdgeType::DependsOn,
                &EdgeType::DerivedFrom
            ]
        );
    }

    #[test]
    fn option_labels_are_sentence_case() {
        assert_eq!(EdgeType::RelatedTo.option_label(), "Related to");
        assert_eq!(EdgeType::References.option_label(), "References");
        assert_eq!(EdgeType::DerivedFrom.option_label(), "Derived from");
    }
}

#[cfg(test)]
mod fold_tests {
    use super::*;
    use chrono::TimeZone;

    fn edge(n: u128, s: u128, t: u128, ty: EdgeType, at: i64, label: Option<&str>) -> Edge {
        Edge {
            id: EdgeId(uuid::Uuid::from_u128(n)),
            source_id: NodeId(uuid::Uuid::from_u128(s)),
            target_id: NodeId(uuid::Uuid::from_u128(t)),
            edge_type: ty,
            label: label.map(String::from),
            created_at: Utc.timestamp_opt(at, 0).unwrap(),
        }
    }
    fn ids(v: &[Edge]) -> Vec<u128> {
        v.iter().map(|e| e.id.0.as_u128()).collect()
    }

    #[test]
    fn a_pair_keeps_the_oldest_and_carries_a_label_only_into_a_blank() {
        use EdgeType::*;
        let edges = vec![
            edge(1, 10, 20, RelatedTo, 200, Some("newer")),
            edge(2, 20, 10, RelatedTo, 100, None), // older, blank: kept
            edge(3, 30, 40, RelatedTo, 100, Some("mine")),
            edge(4, 40, 30, RelatedTo, 200, Some("theirs")), // kept has a label
            edge(5, 10, 20, References, 50, None),           // other types stay
            edge(6, 20, 10, References, 60, None),
            edge(7, 50, 60, RelatedTo, 10, None), // a lone edge stays
        ];
        let (kept, dropped) = fold_related_pairs(edges.clone());
        assert_eq!(ids(&kept), vec![2, 3, 5, 6, 7]);
        assert_eq!(ids(&dropped), vec![1, 4]);
        assert_eq!(
            kept[0].label.as_deref(),
            Some("newer"),
            "carried into the blank"
        );
        assert_eq!(kept[1].label.as_deref(), Some("mine"), "an own label stays");

        // The same answer whatever the input order.
        let mut reversed = edges;
        reversed.reverse();
        let (mut kept2, _) = fold_related_pairs(reversed);
        kept2.sort_by_key(|e| e.id.0);
        assert_eq!(ids(&kept2), vec![2, 3, 5, 6, 7]);
        assert_eq!(kept2[0].label.as_deref(), Some("newer"));
    }

    #[test]
    fn the_oldest_label_wins_and_a_tie_on_time_goes_by_id() {
        use EdgeType::RelatedTo;
        let edges = vec![
            edge(9, 1, 2, RelatedTo, 100, Some(" ")), // blank (whitespace): kept, same time, lower id below
            edge(8, 2, 1, RelatedTo, 100, None),      // same time, lower id: kept
            edge(7, 1, 2, RelatedTo, 300, Some("late")),
            edge(6, 2, 1, RelatedTo, 200, Some("early")),
        ];
        let (kept, dropped) = fold_related_pairs(edges);
        assert_eq!(ids(&kept), vec![8]);
        assert_eq!(kept[0].label.as_deref(), Some("early"));
        assert_eq!(dropped.len(), 3);
    }
}
