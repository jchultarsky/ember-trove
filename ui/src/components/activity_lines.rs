//! How one entry of a node's log reads on screen: who, what, and the
//! detail beside it. Shared by the History section (what occurred) and
//! the Activity section (the audit trail), so a change is worded the same
//! wherever it shows.
//!
//! Everything here is plain text for a text node. A status label and a
//! Brief's words are free text: none of it is ever rendered as HTML.

use common::activity::{
    ActivityAction, ActivityEntry, project_state_change_summary, status_change_summary,
};

/// An account id as shown: a deleted account's placeholder reads as its
/// name (#239); any other id is shown as it is.
pub fn shown_subject(subject: &str) -> &str {
    if subject == common::admin::DELETED_ACCOUNT {
        common::admin::DELETED_ACCOUNT_NAME
    } else {
        subject
    }
}

/// Returns actor name from metadata, falling back to `subject_id`.
pub fn actor_display(entry: &ActivityEntry) -> String {
    entry
        .metadata
        .get("actor_name")
        .and_then(|v| v.as_str())
        .filter(|s| !s.is_empty())
        .or_else(|| {
            entry
                .metadata
                .get("actor_email")
                .and_then(|v| v.as_str())
                .filter(|s| !s.is_empty())
        })
        .unwrap_or_else(|| shown_subject(&entry.subject_id))
        .to_string()
}

/// Returns a short detail string for extra context (tag id, filename, role, etc.)
pub fn action_detail(entry: &ActivityEntry) -> Option<String> {
    match entry.action {
        ActivityAction::TagAdded | ActivityAction::TagRemoved => entry
            .metadata
            .get("tag_id")
            .and_then(|v| v.as_str())
            .map(|s| format!("tag {}", &s[..8.min(s.len())])),
        ActivityAction::AttachmentUploaded => entry
            .metadata
            .get("filename")
            .and_then(|v| v.as_str())
            .map(str::to_string),
        ActivityAction::PermissionGranted => {
            let role = entry
                .metadata
                .get("role")
                .and_then(|v| v.as_str())
                .unwrap_or("?");
            let who = entry
                .metadata
                .get("invited_email")
                .or_else(|| entry.metadata.get("subject_id"))
                .and_then(|v| v.as_str())
                .map_or("?", shown_subject);
            Some(format!("{who} as {role}"))
        }
        ActivityAction::Exported => entry
            .metadata
            .get("format")
            .and_then(|v| v.as_str())
            .map(str::to_string),
        ActivityAction::StatusChanged => {
            let label = |k: &str| entry.metadata.get(k).and_then(|v| v.as_str());
            Some(status_change_summary(label("from"), label("to")))
        }
        // A change of the declared state (S6a), in the states' own words.
        ActivityAction::ProjectStateChanged => {
            let state = |k: &str| entry.metadata.get(k).and_then(|v| v.as_str());
            Some(project_state_change_summary(state("from"), state("to")))
        }
        // A move between draft, published and archived (S6d).
        ActivityAction::LifecycleChanged => {
            let stage = |k: &str| entry.metadata.get(k).and_then(|v| v.as_str());
            Some(common::activity::lifecycle_change_summary(
                stage("from"),
                stage("to"),
            ))
        }
        // The replaced Brief's headline, else the start of its pitch (#282).
        // Shown as plain text: Leptos escapes it, no markdown is rendered.
        // The verb says "removed" for a removed one (`action_label`), so
        // the detail is the old words either way.
        ActivityAction::BriefSuperseded => {
            common::brief::superseded_summary(&entry.metadata).map(|said| {
                match said.strip_prefix("removed, ") {
                    Some(rest) => rest.to_string(),
                    None => said,
                }
            })
        }
        // How much a cascade closed here (S6b), and on a sub-Project's
        // entry the master it came from. The API has already left out what
        // this reader may not see, so this says the rest.
        ActivityAction::CascadeClosed => {
            let from = entry
                .metadata
                .get("root")
                .and_then(|root| root.get("title"))
                .and_then(|title| title.as_str())
                .map(|title| format!("with \u{201c}{title}\u{201d}"));
            let parts: Vec<String> = [from, common::cascade::summary(&entry.metadata)]
                .into_iter()
                .flatten()
                .collect();
            (!parts.is_empty()).then(|| parts.join(" \u{b7} "))
        }
        _ => None,
    }
}

/// Tailwind colour class for the action icon bubble.
pub fn action_colour(action: &ActivityAction) -> &'static str {
    match action {
        ActivityAction::Created => {
            "bg-emerald-100 text-emerald-700 dark:bg-emerald-900 dark:text-emerald-300"
        }
        ActivityAction::Edited => {
            "bg-amber-100 text-amber-700 dark:bg-amber-900 dark:text-amber-300"
        }
        ActivityAction::TagAdded | ActivityAction::TagRemoved => {
            "bg-violet-100 text-violet-700 dark:bg-violet-900 dark:text-violet-300"
        }
        ActivityAction::AttachmentUploaded => {
            "bg-sky-100 text-sky-700 dark:bg-sky-900 dark:text-sky-300"
        }
        ActivityAction::PermissionGranted | ActivityAction::PermissionRevoked => {
            "bg-orange-100 text-orange-700 dark:bg-orange-900 dark:text-orange-300"
        }
        ActivityAction::Shared => "bg-teal-100 text-teal-700 dark:bg-teal-900 dark:text-teal-300",
        ActivityAction::Exported => {
            "bg-stone-100 text-stone-700 dark:bg-stone-800 dark:text-stone-300"
        }
        ActivityAction::CreatedFromTemplate => {
            "bg-emerald-100 text-emerald-700 dark:bg-emerald-900 dark:text-emerald-300"
        }
        ActivityAction::StatusChanged
        | ActivityAction::BriefSuperseded
        | ActivityAction::ProjectStateChanged
        | ActivityAction::CascadeClosed
        | ActivityAction::LifecycleChanged => {
            "bg-sky-100 text-sky-700 dark:bg-sky-900 dark:text-sky-300"
        }
    }
}

/// The verb of an entry's line. The action's own label, except where the
/// entry says more: a Brief that was removed was not "updated".
#[must_use]
pub fn action_label(entry: &ActivityEntry) -> &'static str {
    if entry.action == ActivityAction::BriefSuperseded && brief_removed(entry) {
        "removed the brief"
    } else {
        entry.action.label()
    }
}

fn brief_removed(entry: &ActivityEntry) -> bool {
    entry.metadata.get("removed").and_then(|v| v.as_bool()) == Some(true)
}

/// One block of what a change shows when it is opened.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DetailBlock {
    pub heading: &'static str,
    /// Plain paragraphs (a Brief's headline and pitch).
    pub text: Vec<String>,
    /// A list (sub-projects, tasks, a Brief's points).
    pub lines: Vec<String>,
    /// How many more there are than `lines` holds (a long task list is
    /// cut by the API; it says how many there were).
    pub more: usize,
}

/// What a change holds beyond its one line (§4.1: a cascade's record is
/// "exactly which" sub-Projects and tasks it closed): the sub-Projects a
/// cascade closed and the tasks it cancelled, or the words of a Brief
/// that was replaced or removed. Empty for a change whose line says it
/// all.
///
/// A cascade's entry arrives already cut to what this reader may see
/// (`cascade::presented`), so this lists only that.
#[must_use]
pub fn change_details(entry: &ActivityEntry) -> Vec<DetailBlock> {
    let meta = &entry.metadata;
    let texts = |key: &str, field: Option<&str>| -> Vec<String> {
        meta.get(key)
            .and_then(|v| v.as_array())
            .into_iter()
            .flatten()
            .filter_map(|item| match field {
                Some(field) => item.get(field).and_then(|v| v.as_str()),
                None => item.as_str(),
            })
            .map(str::to_string)
            .collect()
    };
    let mut blocks = Vec::new();
    match entry.action {
        ActivityAction::CascadeClosed => {
            let projects = texts("sub_projects", Some("title"));
            if !projects.is_empty() {
                blocks.push(DetailBlock {
                    heading: "Sub-projects closed",
                    text: Vec::new(),
                    lines: projects,
                    more: 0,
                });
            }
            let tasks = texts("tasks", Some("title"));
            if !tasks.is_empty() {
                let total = meta
                    .get("tasks_total")
                    .and_then(serde_json::Value::as_u64)
                    .and_then(|n| usize::try_from(n).ok())
                    .unwrap_or(tasks.len());
                blocks.push(DetailBlock {
                    heading: "Tasks cancelled",
                    text: Vec::new(),
                    more: total.saturating_sub(tasks.len()),
                    lines: tasks,
                });
            }
        }
        ActivityAction::BriefSuperseded => {
            let text = |key: &str| {
                meta.get(key)
                    .and_then(|v| v.as_str())
                    .map(str::trim)
                    .filter(|s| !s.is_empty())
                    .map(str::to_string)
            };
            let words: Vec<String> = [text("headline"), text("pitch")]
                .into_iter()
                .flatten()
                .collect();
            let points = texts("points", None);
            if !words.is_empty() || !points.is_empty() {
                blocks.push(DetailBlock {
                    heading: if brief_removed(entry) {
                        "The brief that was removed"
                    } else {
                        "The brief before this"
                    },
                    text: words,
                    lines: points,
                    more: 0,
                });
            }
        }
        _ => {}
    }
    blocks
}

#[cfg(test)]
mod tests {
    use super::*;
    use common::id::NodeId;

    fn entry(action: ActivityAction, metadata: serde_json::Value) -> ActivityEntry {
        ActivityEntry {
            id: common::id::ActivityId(uuid::Uuid::nil()),
            node_id: NodeId(uuid::Uuid::nil()),
            subject_id: "sub".to_string(),
            action,
            metadata,
            created_at: chrono::Utc::now(),
        }
    }

    #[test]
    fn a_lifecycle_change_shows_from_and_to() {
        let e = entry(
            ActivityAction::LifecycleChanged,
            serde_json::json!({ "from": "draft", "to": "published" }),
        );
        assert_eq!(
            action_detail(&e).as_deref(),
            Some("Draft \u{2192} Published")
        );
    }

    #[test]
    fn a_cascade_says_how_much_it_closed() {
        let e = entry(
            ActivityAction::CascadeClosed,
            serde_json::json!({
                "state": "complete",
                "sub_projects": [{ "id": "a", "title": "A" }, { "id": "b", "title": "B" }],
                "tasks": [{ "id": "t", "title": "T" }],
                "tasks_total": 1,
            }),
        );
        assert_eq!(action_detail(&e).as_deref(), Some("2 sub-projects, 1 task"));
        // On a sub-Project: the master it was closed with, when the
        // reader was given it.
        let on_child = entry(
            ActivityAction::CascadeClosed,
            serde_json::json!({
                "state": "complete",
                "root": { "id": "m", "title": "Kitchen" },
                "tasks": [{ "id": "t", "title": "T" }],
                "tasks_total": 1,
            }),
        );
        assert_eq!(
            action_detail(&on_child).as_deref(),
            Some("with \u{201c}Kitchen\u{201d} \u{b7} 1 task")
        );
        // Nothing this reader may see closed here: the line stands alone.
        let bare = entry(
            ActivityAction::CascadeClosed,
            serde_json::json!({ "state": "complete", "tasks": [] }),
        );
        assert_eq!(action_detail(&bare), None);
    }

    #[test]
    fn a_status_change_shows_from_and_to() {
        let e = entry(
            ActivityAction::StatusChanged,
            serde_json::json!({ "from": "Active", "to": "Blocked" }),
        );
        assert_eq!(
            action_detail(&e).as_deref(),
            Some("Active \u{2192} Blocked")
        );
        let set = entry(
            ActivityAction::StatusChanged,
            serde_json::json!({ "from": null, "to": "Active" }),
        );
        assert_eq!(action_detail(&set).as_deref(), Some("set to Active"));
        let cleared = entry(
            ActivityAction::StatusChanged,
            serde_json::json!({ "from": "Done", "to": null }),
        );
        assert_eq!(action_detail(&cleared).as_deref(), Some("Done cleared"));
    }

    #[test]
    fn every_action_has_a_glyph_in_the_font() {
        // The names come from `common`, which the font script used to
        // skip: "flag" rendered as the word FLAG (2026-11-13).
        let names = include_str!("../../icon-names.txt");
        for action in &ActivityAction::ALL {
            let icon = action.icon();
            assert!(
                names.lines().any(|n| n == icon),
                "{icon} is not in ui/icon-names.txt: run scripts/refresh-icon-font.sh"
            );
        }
    }

    #[test]
    fn a_state_change_shows_from_and_to_in_words() {
        let e = entry(
            ActivityAction::ProjectStateChanged,
            serde_json::json!({ "from": "active", "to": "parked" }),
        );
        assert_eq!(action_detail(&e).as_deref(), Some("Active \u{2192} Parked"));
    }

    /// #239: what stays on others' nodes after an account is deleted reads
    /// as "Deleted account", never the raw placeholder.
    #[test]
    fn a_deleted_account_reads_as_deleted_account() {
        let mut by = entry(ActivityAction::Edited, serde_json::json!({}));
        by.subject_id = common::admin::DELETED_ACCOUNT.to_string();
        assert_eq!(actor_display(&by), "Deleted account");

        let grant = entry(
            ActivityAction::PermissionGranted,
            serde_json::json!({ "subject_id": common::admin::DELETED_ACCOUNT, "role": "editor" }),
        );
        assert_eq!(
            action_detail(&grant).as_deref(),
            Some("Deleted account as editor")
        );

        let other = entry(
            ActivityAction::PermissionGranted,
            serde_json::json!({ "subject_id": "someone", "role": "viewer" }),
        );
        assert_eq!(action_detail(&other).as_deref(), Some("someone as viewer"));
    }

    #[test]
    fn a_cascade_opens_to_exactly_what_it_closed() {
        let e = entry(
            ActivityAction::CascadeClosed,
            serde_json::json!({
                "state": "complete",
                "sub_projects": [{ "id": "a", "title": "Cabinets" }],
                "tasks": [{ "id": "t", "title": "Order tiles" }, { "id": "u", "title": "Call" }],
                "tasks_total": 60,
            }),
        );
        assert_eq!(
            change_details(&e),
            vec![
                DetailBlock {
                    heading: "Sub-projects closed",
                    text: vec![],
                    lines: vec!["Cabinets".to_string()],
                    more: 0,
                },
                DetailBlock {
                    heading: "Tasks cancelled",
                    text: vec![],
                    lines: vec!["Order tiles".to_string(), "Call".to_string()],
                    more: 58,
                },
            ]
        );
        // Nothing beyond the line: nothing to open.
        let bare = entry(
            ActivityAction::CascadeClosed,
            serde_json::json!({ "state": "complete", "root": { "id": "m", "title": "M" }, "tasks": [] }),
        );
        assert!(change_details(&bare).is_empty());
        let status = entry(
            ActivityAction::StatusChanged,
            serde_json::json!({ "from": "Active", "to": "Blocked" }),
        );
        assert!(change_details(&status).is_empty());
    }

    #[test]
    fn a_replaced_brief_opens_to_its_old_words() {
        let e = entry(
            ActivityAction::BriefSuperseded,
            serde_json::json!({
                "headline": "On track",
                "pitch": "  The kitchen is half done.  ",
                "points": ["Tiles ordered", "Electrician Friday"],
                "author_id": "someone",
            }),
        );
        let blocks = change_details(&e);
        assert_eq!(blocks.len(), 1);
        assert_eq!(blocks[0].heading, "The brief before this");
        // The words as paragraphs, the points as the list: one bullet
        // each, drawn by the list.
        assert_eq!(blocks[0].text, ["On track", "The kitchen is half done."]);
        assert_eq!(blocks[0].lines, ["Tiles ordered", "Electrician Friday"]);
        // The author's id is not among the words.
        let all: Vec<&String> = blocks[0].text.iter().chain(&blocks[0].lines).collect();
        assert!(!all.iter().any(|l| l.contains("someone")));
        assert_eq!(action_label(&e), "updated the brief");
        assert_eq!(action_detail(&e).as_deref(), Some("was: On track"));

        // Removed, not replaced: the line says so once.
        let removed = entry(
            ActivityAction::BriefSuperseded,
            serde_json::json!({ "pitch": "The kitchen is half done.", "removed": true }),
        );
        assert_eq!(action_label(&removed), "removed the brief");
        assert_eq!(
            action_detail(&removed).as_deref(),
            Some("was: The kitchen is half done.")
        );
        assert_eq!(
            change_details(&removed)[0].heading,
            "The brief that was removed"
        );
        let empty = entry(ActivityAction::BriefSuperseded, serde_json::json!({}));
        assert!(change_details(&empty).is_empty());
    }
}
