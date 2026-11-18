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

    /// A link the machine keeps, never a person (§6.6): a `wiki_link`
    /// mirrors `[[Title]]` in the source's text. It can't be created,
    /// deleted or converted by hand: sync would only put it back on the
    /// next save. `POST /edges`, `DELETE /edges/{id}`, the Data health
    /// buttons and the node page's remove button all use this one rule.
    #[must_use]
    pub fn is_machine_owned(&self) -> bool {
        *self == EdgeType::WikiLink
    }

    /// Whether an editor of the target alone may delete a link of this
    /// type (§14). True for a home, so one home never traps a child (#224),
    /// and for `related_to`, which has no direction (#226). Every other
    /// type belongs to its source's editors. `DELETE /edges/{id}`, the
    /// convert fix and the Data health buttons all use this one rule.
    #[must_use]
    pub fn deletable_from_target(&self) -> bool {
        *self == EdgeType::Contains || self.is_symmetric()
    }

    /// Whether someone who edits the link's source, its target, both or
    /// neither may remove it: the one rule behind `DELETE /edges/{id}`,
    /// the Data health buttons ([`crate::data_health::LinkRef::removable`])
    /// and the node page's remove control (`EdgeWithTitles::may_remove`).
    /// A machine-owned link is never removed by hand (§6.6); a home and a
    /// `related_to` go from either end ([`Self::deletable_from_target`]);
    /// every other link belongs to its source's editors.
    #[must_use]
    pub fn removable_by(&self, edits_source: bool, edits_target: bool) -> bool {
        !self.is_machine_owned() && (edits_source || (self.deletable_from_target() && edits_target))
    }

    /// Whether a link of this type, touching a node and with its other end
    /// out of sight, is **that node's editors' to clear**: told to them as
    /// a count, never named, and removed together (ONTOLOGY §14; the rule
    /// a task's prerequisites not shown have, §9.3). `outgoing` is whether
    /// the node is the link's source.
    ///
    /// It is the links an editor of the node alone could remove if they
    /// knew of them ([`Self::removable_by`]): every link made by hand
    /// from the node, and a `related_to` either way (it has no
    /// direction: which end it is stored from is an accident). Left out:
    /// - a home the node sits in (an incoming `contains`): the Home row
    ///   already tells the node's editors it exists and frees it (§6.1);
    /// - any other link *into* the node: it is its source's;
    /// - a `wiki_link`, either way: it mirrors `[[Title]]` in the text and
    ///   is never removed by hand. Counting it would also let someone
    ///   write `[[a guess]]` and watch the count: a way to probe titles.
    #[must_use]
    pub fn is_its_nodes_to_clear(&self, outgoing: bool) -> bool {
        if outgoing {
            self.removable_by(true, false)
        } else {
            self.removable_by(false, true) && *self != EdgeType::Contains
        }
    }

    /// The types [`Self::is_its_nodes_to_clear`] accepts for a direction,
    /// in the database's words: the SQL is built from this list
    /// (`repo::edge`), so it can't name a different set than the rule.
    #[must_use]
    pub fn a_nodes_to_clear(outgoing: bool) -> Vec<&'static str> {
        EdgeType::ALL
            .iter()
            .filter(|t| t.is_its_nodes_to_clear(outgoing))
            .map(EdgeType::as_str)
            .collect()
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

/// `DELETE /nodes/{id}/edges/not-shown`: how many links went, and how many
/// are still not shown (0, unless one appeared meanwhile). Nothing about
/// which, to what, or by whom.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct LinksCleared {
    pub removed: u64,
    pub links_not_shown: u32,
}

/// What a link's list shows of one of its ends beside the title (S7a):
/// its kind, and whether it is still in use. Nothing here is more than
/// its reader could open: the list holds only links whose two ends they
/// can view.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct EdgeEnd {
    pub node_type: crate::node::NodeType,
    pub status: crate::node::NodeStatus,
    /// A Project's declared state; `None` for every other kind.
    #[serde(default)]
    pub project_state: Option<crate::project_state::ProjectState>,
}

impl EdgeEnd {
    /// [`crate::node::is_live`] for this end.
    #[must_use]
    pub fn is_live(&self) -> bool {
        crate::node::is_live(&self.status, self.project_state)
    }
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
    /// The source's kind and standing (S7a). `None` only in an answer
    /// cached from before the field: then nothing is said about it.
    #[serde(default)]
    pub source_node: Option<EdgeEnd>,
    /// The target's, likewise.
    #[serde(default)]
    pub target_node: Option<EdgeEnd>,
    /// Whether this reader may remove the link: the rule of
    /// `DELETE /edges/{id}` ([`EdgeType::removable_by`]) read for them, so
    /// the page offers the control only where the call would pass.
    #[serde(default)]
    pub may_remove: bool,
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

/// `GET`/`PUT /nodes/{id}/home` (#228): a node's one home (§3.3, §6.1).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct NodeHome {
    /// `None`: the node stands alone (or its home is hidden from a viewer).
    pub home: Option<Home>,
}

/// The `contains` link that is a node's home.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct Home {
    pub edge_id: EdgeId,
    /// The parent, or `None` for a home the caller can't see. Only the
    /// child's editors are told such a home exists, with nothing about it
    /// (owner decision, #228): they may remove it (§6.1), and a new home
    /// would be refused anyway ("already has a home"). Viewers never are.
    pub node: Option<HomeNode>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct HomeNode {
    pub id: NodeId,
    pub title: String,
    pub node_type: crate::node::NodeType,
}

/// `PUT /nodes/{id}/home`: the new home, or `None` to stand alone.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct SetHomeRequest {
    pub parent_id: Option<NodeId>,
}

/// Turns a link into another type in one step (#228): a Data health fix.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct ConvertEdgeRequest {
    pub edge_type: EdgeType,
    /// Turn the link round as well: the new link runs from the old
    /// target to the old source (S7a). Made for a "derived from an Area"
    /// link on a Reference, which becomes "the Area references it"; it
    /// works for any link the caller could delete and then make the
    /// other way round, and needs exactly those rights (`convert_ends`).
    #[serde(default)]
    pub reverse: bool,
}

/// What the caller may do to one end of a link. `edit` implies `view`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct EndRights {
    pub edit: bool,
    pub view: bool,
}

/// Why a link can't be converted.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConvertRefusal {
    /// The caller may not delete the old link or create the new one (403).
    Forbidden,
    /// Not a conversion the fixes offer (422).
    NotConvertible,
}

impl ConvertRefusal {
    #[must_use]
    pub fn message(self) -> &'static str {
        match self {
            Self::Forbidden => "you can't change this link",
            Self::NotConvertible => {
                "a link can only become a references or a related-to link, \
                 and a wiki link never changes"
            }
        }
    }
}

/// Whether a link of type `from` may become `to`, and if so whether its
/// ends swap (#228). The checks run in this order:
/// 1. **Delete the old link.** The caller must be allowed to delete it
///    ([`EdgeType::deletable_from_target`]); this comes first, so nothing
///    about the link is judged for a caller who has no say over it.
/// 2. **Allowed conversions only.** `to` is `references` or `related_to`:
///    neither has a structural rule (one home, loops) to re-check. It must
///    differ from `from`, and a machine-owned wiki link never converts.
/// 3. **Create the new one.** The caller needs edit rights on its source
///    and view rights on its target. A `related_to` has no direction, so
///    it may run from whichever end the caller edits: `Ok(true)` means the
///    new link's source is the old target.
///
/// `reverse` asks for the new link to run the other way (S7a): its source
/// is the old target, so step 3 needs edit rights on the old target and
/// view rights on the old source, and the answer is always `Ok(true)`. A
/// `related_to` has no direction to turn, so that is not a conversion.
pub fn convert_ends(
    from: &EdgeType,
    to: &EdgeType,
    reverse: bool,
    source: EndRights,
    target: EndRights,
) -> Result<bool, ConvertRefusal> {
    if !(source.edit || (from.deletable_from_target() && target.edit)) {
        return Err(ConvertRefusal::Forbidden);
    }
    if from == to
        || from.is_machine_owned()
        || !matches!(to, EdgeType::References | EdgeType::RelatedTo)
        || (reverse && to.is_symmetric())
    {
        return Err(ConvertRefusal::NotConvertible);
    }
    if reverse {
        return if target.edit && source.view {
            Ok(true)
        } else {
            Err(ConvertRefusal::Forbidden)
        };
    }
    if source.edit && target.view {
        Ok(false)
    } else if to.is_symmetric() && target.edit && source.view {
        Ok(true)
    } else {
        Err(ConvertRefusal::Forbidden)
    }
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

/// Keep one home per node (#228, §6.1): the oldest `contains` edge into
/// each child stays, the rest are returned to be archived. Labels aren't
/// carried: each belongs to its own parent. Migration 042 applies the same
/// rule (`created_at`, then `id`), and restore uses this, so an archive
/// from before the one-home index still restores; the 042 pg test checks
/// that the two agree. Other types pass through untouched, in order.
pub fn fold_homes(edges: Vec<Edge>) -> (Vec<Edge>, Vec<Edge>) {
    use std::collections::HashMap;

    let mut keep: HashMap<uuid::Uuid, (DateTime<Utc>, uuid::Uuid)> = HashMap::new();
    for e in edges.iter().filter(|e| e.edge_type == EdgeType::Contains) {
        let age = (e.created_at, e.id.0);
        keep.entry(e.target_id.0)
            .and_modify(|k| *k = (*k).min(age))
            .or_insert(age);
    }
    edges.into_iter().partition(|e| {
        e.edge_type != EdgeType::Contains
            || keep.get(&e.target_id.0) == Some(&(e.created_at, e.id.0))
    })
}

#[cfg(test)]
mod home_fold_tests {
    use super::*;
    use chrono::TimeZone;

    fn edge(id: u128, s: u128, t: u128, edge_type: EdgeType, day: u32) -> Edge {
        Edge {
            id: EdgeId(uuid::Uuid::from_u128(id)),
            source_id: NodeId(uuid::Uuid::from_u128(s)),
            target_id: NodeId(uuid::Uuid::from_u128(t)),
            edge_type,
            label: Some(format!("l{id}")),
            created_at: Utc
                .with_ymd_and_hms(2026, 1, day, 0, 0, 0)
                .single()
                .expect("date"),
        }
    }

    #[test]
    fn each_node_keeps_its_oldest_home() {
        let edges = vec![
            edge(1, 10, 99, EdgeType::Contains, 3),
            edge(2, 11, 99, EdgeType::Contains, 1), // oldest: kept
            edge(3, 12, 99, EdgeType::Contains, 2),
            edge(4, 10, 98, EdgeType::Contains, 5), // the only home: kept
            edge(5, 11, 99, EdgeType::References, 9),
        ];
        let (kept, dropped) = fold_homes(edges);
        let ids = |v: &[Edge]| v.iter().map(|e| e.id.0.as_u128()).collect::<Vec<_>>();
        assert_eq!(ids(&kept), vec![2, 4, 5], "other types untouched, in order");
        assert_eq!(ids(&dropped), vec![1, 3]);
        assert_eq!(
            kept[0].label.as_deref(),
            Some("l2"),
            "labels stay with their link"
        );
    }

    #[test]
    fn a_tie_on_age_goes_to_the_smaller_id() {
        let (kept, dropped) = fold_homes(vec![
            edge(7, 10, 99, EdgeType::Contains, 1),
            edge(6, 11, 99, EdgeType::Contains, 1),
        ]);
        assert_eq!(kept[0].id.0.as_u128(), 6);
        assert_eq!(dropped[0].id.0.as_u128(), 7);
    }
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
mod convert_tests {
    use super::*;
    use EdgeType::*;

    const NONE: EndRights = EndRights {
        edit: false,
        view: false,
    };
    const VIEW: EndRights = EndRights {
        edit: false,
        view: true,
    };
    const EDIT: EndRights = EndRights {
        edit: true,
        view: true,
    };

    /// Hidden links (#349): what a node's editors are told of by count
    /// and may clear is what they could remove if they knew of it, less
    /// the hidden home, and never a wiki link.
    #[test]
    fn a_nodes_editors_clear_only_links_they_could_remove_themselves() {
        for t in EdgeType::ALL {
            // From the node: exactly what its editor may remove.
            assert_eq!(
                t.is_its_nodes_to_clear(true),
                t.removable_by(true, false),
                "{t:?} outgoing"
            );
            // Into the node: what its editor may remove, but for the
            // home, which the Home row tells and frees.
            assert_eq!(
                t.is_its_nodes_to_clear(false),
                t.removable_by(false, true) && *t != Contains,
                "{t:?} incoming"
            );
            // Never something its editor could not remove anyway.
            for outgoing in [true, false] {
                if t.is_its_nodes_to_clear(outgoing) {
                    assert!(t.removable_by(outgoing, !outgoing), "{t:?}");
                }
            }
        }
        // By name.
        let sorted = |outgoing| {
            let mut words = EdgeType::a_nodes_to_clear(outgoing);
            words.sort_unstable();
            words
        };
        assert_eq!(
            sorted(true),
            [
                "contains",
                "depends_on",
                "derived_from",
                "references",
                "related_to"
            ]
        );
        assert_eq!(sorted(false), ["related_to"]);
        // A wiki link, either way: never (it would be a way to probe
        // titles). A dependency or an origin pointing *at* the node is
        // the other node's.
        assert!(!WikiLink.is_its_nodes_to_clear(true));
        assert!(!WikiLink.is_its_nodes_to_clear(false));
        assert!(!Contains.is_its_nodes_to_clear(false), "the hidden home");
        assert!(!DependsOn.is_its_nodes_to_clear(false));
        assert!(!DerivedFrom.is_its_nodes_to_clear(false));
        assert!(!References.is_its_nodes_to_clear(false));
        // `related_to` reads the same from both ends: its direction is
        // how it happens to be stored.
        assert_eq!(
            RelatedTo.is_its_nodes_to_clear(true),
            RelatedTo.is_its_nodes_to_clear(false)
        );
    }

    #[test]
    fn only_a_home_and_a_related_link_may_be_deleted_from_the_target() {
        for t in [
            References,
            Contains,
            RelatedTo,
            DependsOn,
            DerivedFrom,
            WikiLink,
        ] {
            assert_eq!(
                t.deletable_from_target(),
                matches!(t, Contains | RelatedTo),
                "{t:?}"
            );
            // Only the wiki link is kept by the machine (§6.6).
            assert_eq!(t.is_machine_owned(), t == WikiLink, "{t:?}");
        }
    }

    /// One rule for the delete route, the Data health buttons and the
    /// node page's remove control (S7a).
    #[test]
    fn who_may_remove_a_link_is_one_rule_for_every_type() {
        for t in EdgeType::ALL {
            // Nobody with no say over either end, and never a wiki link.
            assert!(!t.removable_by(false, false), "{t:?}");
            let by_hand = *t != WikiLink;
            // The source's editor: every link made by hand.
            assert_eq!(t.removable_by(true, false), by_hand, "{t:?}");
            assert_eq!(t.removable_by(true, true), by_hand, "{t:?}");
            // The target's editor alone: a home and a related link only.
            assert_eq!(
                t.removable_by(false, true),
                matches!(t, Contains | RelatedTo),
                "{t:?}"
            );
        }
    }

    /// S7a: a link can be turned round as it is converted. The new link
    /// runs from the old target, so that is the end that must be edited.
    #[test]
    fn a_link_is_turned_round_only_by_someone_who_edits_its_new_source() {
        // "This Reference was derived from that Area" becomes "that Area
        // references this Reference": the caller edits both.
        assert_eq!(
            convert_ends(&DerivedFrom, &References, true, EDIT, EDIT),
            Ok(true)
        );
        // They may delete the old link (they edit its source), but may
        // not write on the Area: refused.
        assert_eq!(
            convert_ends(&DerivedFrom, &References, true, EDIT, VIEW),
            Err(ConvertRefusal::Forbidden)
        );
        // Editing the Area alone gives no say over the old link.
        assert_eq!(
            convert_ends(&DerivedFrom, &References, true, VIEW, EDIT),
            Err(ConvertRefusal::Forbidden)
        );
        // A related link has no direction to turn.
        assert_eq!(
            convert_ends(&DerivedFrom, &RelatedTo, true, EDIT, EDIT),
            Err(ConvertRefusal::NotConvertible)
        );
        // Nor does turning round open any other conversion.
        assert_eq!(
            convert_ends(&DerivedFrom, &Contains, true, EDIT, EDIT),
            Err(ConvertRefusal::NotConvertible)
        );
        assert_eq!(
            convert_ends(&WikiLink, &References, true, EDIT, EDIT),
            Err(ConvertRefusal::NotConvertible)
        );
        // `reverse` is general, not only for a link into an Area. A
        // child's editor may leave its home (a home never traps a
        // child), and may write a link on their own node: "the child
        // references what was its home" is that delete and that create.
        assert_eq!(
            convert_ends(&Contains, &References, true, VIEW, EDIT),
            Ok(true)
        );
        // Without `reverse` the new link would be written on the home.
        assert_eq!(
            convert_ends(&Contains, &References, false, VIEW, EDIT),
            Err(ConvertRefusal::Forbidden)
        );
        // Someone who can't see the old source can't name it in a link.
        assert_eq!(
            convert_ends(&Contains, &References, true, EndRights::default(), EDIT),
            Err(ConvertRefusal::Forbidden)
        );
        // An older client sends no flag: nothing is turned.
        let req: ConvertEdgeRequest =
            serde_json::from_str(r#"{"edge_type":"references"}"#).expect("parses");
        assert!(!req.reverse);
    }

    #[test]
    fn a_contained_reference_becomes_a_reference_link_from_its_parent() {
        assert_eq!(
            convert_ends(&Contains, &References, false, EDIT, VIEW),
            Ok(false)
        );
        // The Reference's editor may free it, but can't speak for the parent.
        assert_eq!(
            convert_ends(&Contains, &References, false, VIEW, EDIT),
            Err(ConvertRefusal::Forbidden)
        );
    }

    #[test]
    fn a_related_link_runs_from_whichever_end_the_caller_edits() {
        assert_eq!(
            convert_ends(&Contains, &RelatedTo, false, EDIT, VIEW),
            Ok(false)
        );
        assert_eq!(
            convert_ends(&Contains, &RelatedTo, false, VIEW, EDIT),
            Ok(true)
        );
        // A depends_on is its source's: the target's editor can't touch it.
        assert_eq!(
            convert_ends(&DependsOn, &RelatedTo, false, VIEW, EDIT),
            Err(ConvertRefusal::Forbidden)
        );
        assert_eq!(
            convert_ends(&DependsOn, &RelatedTo, false, EDIT, VIEW),
            Ok(false)
        );
    }

    #[test]
    fn rights_are_judged_before_the_conversion() {
        // A stranger learns nothing about what the link could become.
        assert_eq!(
            convert_ends(&WikiLink, &Contains, false, NONE, NONE),
            Err(ConvertRefusal::Forbidden)
        );
        assert_eq!(
            convert_ends(&Contains, &RelatedTo, false, VIEW, VIEW),
            Err(ConvertRefusal::Forbidden)
        );
    }

    #[test]
    fn only_unstructured_targets_and_never_a_wiki_link() {
        for to in [Contains, DependsOn, DerivedFrom, WikiLink] {
            assert_eq!(
                convert_ends(&References, &to, false, EDIT, EDIT),
                Err(ConvertRefusal::NotConvertible),
                "{to:?}"
            );
        }
        assert_eq!(
            convert_ends(&References, &References, false, EDIT, EDIT),
            Err(ConvertRefusal::NotConvertible)
        );
        assert_eq!(
            convert_ends(&WikiLink, &References, false, EDIT, EDIT),
            Err(ConvertRefusal::NotConvertible)
        );
    }

    #[test]
    fn the_new_link_needs_a_visible_target() {
        assert_eq!(
            convert_ends(&DependsOn, &References, false, EDIT, NONE),
            Err(ConvertRefusal::Forbidden)
        );
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
            edge(9, 1, 2, RelatedTo, 100, Some(" ")), // same time, higher id: dropped; its label is blank
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
