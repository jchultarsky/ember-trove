use chrono::{DateTime, Utc};
use garde::Validate;
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

use crate::{id::TemplateId, node::NodeType};

/// A reusable body + type scaffold that users can apply when creating new nodes.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct NodeTemplate {
    pub id: TemplateId,
    pub name: String,
    pub description: Option<String>,
    pub node_type: NodeType,
    /// Markdown body pre-filled into the editor when the template is used.
    pub body: String,
    /// When `true` this template is automatically applied to new nodes of
    /// `node_type` created by the same user.  At most one template per owner
    /// per node type can be the default (enforced by a partial unique index).
    pub is_default: bool,
    /// Cognito sub of the creator.
    pub created_by: String,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema, Validate)]
pub struct CreateTemplateRequest {
    #[garde(length(min = 1, max = 200))]
    pub name: String,
    #[garde(skip)]
    pub description: Option<String>,
    #[garde(skip)]
    pub node_type: NodeType,
    #[garde(skip)]
    #[serde(default)]
    pub body: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema, Validate)]
pub struct UpdateTemplateRequest {
    #[garde(length(min = 1, max = 200))]
    pub name: String,
    #[garde(skip)]
    pub description: Option<String>,
    #[garde(skip)]
    pub node_type: NodeType,
    #[garde(skip)]
    pub body: String,
}

/// The owner's default template for `node_type`, if any (at most one per
/// owner per type — a partial unique index enforces it).
pub fn default_for_type<'a>(
    templates: &'a [NodeTemplate],
    node_type: &NodeType,
) -> Option<&'a NodeTemplate> {
    templates
        .iter()
        .find(|t| t.is_default && &t.node_type == node_type)
}

/// Templates for the create-mode picker: `node_type`'s own (default first,
/// then by name), then every other type's (by type, then name).
pub fn picker_order<'a>(
    templates: &'a [NodeTemplate],
    node_type: &NodeType,
) -> (Vec<&'a NodeTemplate>, Vec<&'a NodeTemplate>) {
    let (mut own, mut other): (Vec<_>, Vec<_>) =
        templates.iter().partition(|t| &t.node_type == node_type);
    own.sort_by(|a, b| b.is_default.cmp(&a.is_default).then(a.name.cmp(&b.name)));
    other.sort_by(|a, b| {
        a.node_type
            .as_str()
            .cmp(b.node_type.as_str())
            .then(a.name.cmp(&b.name))
    });
    (own, other)
}

/// Where a new node's initial body came from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CreateBodySource {
    /// An explicit template handoff (Templates "Use", fast capture).
    Prefill(TemplateId),
    /// Work rescued from an abandoned `/nodes/new`.
    Draft,
    /// The owner's default template for the type.
    Default(TemplateId),
    /// The built-in scaffold for the type.
    Scaffold,
}

/// The one rule every create path uses for a new node's body (#111).
/// Priority: explicit prefill > rescued draft > the type's default template >
/// built-in scaffold.
pub fn resolve_create_body(
    prefill: Option<(TemplateId, String)>,
    draft_body: Option<String>,
    default: Option<&NodeTemplate>,
    scaffold: &str,
) -> (String, CreateBodySource) {
    if let Some((id, body)) = prefill {
        return (body, CreateBodySource::Prefill(id));
    }
    if let Some(body) = draft_body {
        return (body, CreateBodySource::Draft);
    }
    if let Some(t) = default {
        return (t.body.clone(), CreateBodySource::Default(t.id));
    }
    (scaffold.to_string(), CreateBodySource::Scaffold)
}

/// May the system replace the body (a type change, or templates arriving
/// after mount)? Only while it is still exactly what the system last put
/// there — never over anything the user typed.
pub fn may_replace_body(current: &str, last_system_body: &str) -> bool {
    current == last_system_body
}

#[cfg(test)]
mod create_body_tests {
    use super::*;
    use chrono::Utc;

    fn tpl(name: &str, nt: NodeType, default: bool) -> NodeTemplate {
        NodeTemplate {
            id: TemplateId(uuid::Uuid::new_v4()),
            name: name.to_string(),
            description: None,
            node_type: nt,
            body: format!("## {name}\n"),
            is_default: default,
            created_by: "owner".to_string(),
            created_at: Utc::now(),
            updated_at: Utc::now(),
        }
    }

    #[test]
    fn default_for_type_picks_the_flagged_template_of_that_type() {
        let ts = vec![
            tpl("Plain project", NodeType::Project, false),
            tpl("Project kickoff", NodeType::Project, true),
            tpl("Article default", NodeType::Article, true),
        ];
        assert_eq!(
            default_for_type(&ts, &NodeType::Project).map(|t| t.name.as_str()),
            Some("Project kickoff")
        );
        assert!(default_for_type(&ts, &NodeType::Area).is_none());
    }

    #[test]
    fn resolve_follows_prefill_draft_default_scaffold() {
        let d = tpl("Kickoff", NodeType::Project, true);
        let pid = TemplateId(uuid::Uuid::new_v4());
        let (b, s) = resolve_create_body(Some((pid, "P".into())), Some("D".into()), Some(&d), "S");
        assert_eq!((b.as_str(), s), ("P", CreateBodySource::Prefill(pid)));
        // A rescued draft wins over the default (same rule as the modal).
        let (b, s) = resolve_create_body(None, Some("D".into()), Some(&d), "S");
        assert_eq!((b.as_str(), s), ("D", CreateBodySource::Draft));
        let (b, s) = resolve_create_body(None, None, Some(&d), "S");
        assert_eq!((b, s), (d.body.clone(), CreateBodySource::Default(d.id)));
        // No default (or it was deleted): built-in scaffold, as before.
        let (b, s) = resolve_create_body(None, None, None, "S");
        assert_eq!((b.as_str(), s), ("S", CreateBodySource::Scaffold));
    }

    #[test]
    fn body_is_replaced_only_while_pristine() {
        assert!(may_replace_body("## Kickoff\n", "## Kickoff\n"));
        assert!(!may_replace_body("## Kickoff\nmy notes", "## Kickoff\n"));
    }

    #[test]
    fn picker_lists_this_types_templates_first_default_leading() {
        let ts = vec![
            tpl("Zeta article", NodeType::Article, false),
            tpl("B project", NodeType::Project, false),
            tpl("A project", NodeType::Project, false),
            tpl("Kickoff", NodeType::Project, true),
            tpl("Area one", NodeType::Area, false),
        ];
        let (own, other) = picker_order(&ts, &NodeType::Project);
        let names = |v: &[&NodeTemplate]| v.iter().map(|t| t.name.clone()).collect::<Vec<_>>();
        assert_eq!(names(&own), ["Kickoff", "A project", "B project"]);
        assert_eq!(names(&other), ["Area one", "Zeta article"]);
    }
}
