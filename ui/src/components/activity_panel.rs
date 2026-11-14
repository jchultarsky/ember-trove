//! Activity / audit-log panel — shows who did what on a node.
use common::activity::{
    ActivityAction, ActivityEntry, project_state_change_summary, status_change_summary,
};
use common::id::NodeId;
use leptos::prelude::*;

use crate::api;
use crate::components::format_helpers::format_timestamp;

/// An account id as shown: a deleted account's placeholder reads as its
/// name (#239); any other id is shown as it is.
fn shown_subject(subject: &str) -> &str {
    if subject == common::admin::DELETED_ACCOUNT {
        common::admin::DELETED_ACCOUNT_NAME
    } else {
        subject
    }
}

/// Returns actor name from metadata, falling back to `subject_id`.
fn actor_display(entry: &ActivityEntry) -> String {
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
fn action_detail(entry: &ActivityEntry) -> Option<String> {
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
        // The replaced Brief's headline, else the start of its pitch (#282).
        // Shown as plain text: Leptos escapes it, no markdown is rendered.
        ActivityAction::BriefSuperseded => common::brief::superseded_summary(&entry.metadata),
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
fn action_colour(action: &ActivityAction) -> &'static str {
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
        | ActivityAction::CascadeClosed => {
            "bg-sky-100 text-sky-700 dark:bg-sky-900 dark:text-sky-300"
        }
    }
}

#[component]
pub fn ActivityPanel(
    node_id: NodeId,
    /// Bumped by the page when something it did wrote History (a change
    /// of state, a cascade), so an open panel shows it.
    #[prop(optional, into)]
    refresh: Option<Signal<u32>>,
) -> impl IntoView {
    let open = RwSignal::new(false);

    let entries = LocalResource::new(move || {
        let is_open = open.get();
        if let Some(refresh) = refresh {
            refresh.track();
        }
        async move {
            if !is_open {
                return Ok(vec![]);
            }
            api::fetch_activity(node_id, Some(50)).await
        }
    });

    view! {
        <crate::components::node_section::SectionShell section=crate::components::node_section::NodeSection::Activity open=open>

                    <Suspense fallback=|| view! {
                        <p class="text-xs text-stone-400 dark:text-stone-500 animate-pulse py-2">"Loading activity…"</p>
                    }>
                        {move || entries.get().map(|result| match result {
                            Err(e) => view! {
                                <p class="text-xs text-red-500">{format!("Error: {e}")}</p>
                            }.into_any(),
                            Ok(list) if list.is_empty() => view! {
                                <p class="text-xs text-stone-400 dark:text-stone-500 py-2">"No activity recorded yet."</p>
                            }.into_any(),
                            Ok(list) => view! {
                                <ol class="relative border-l border-stone-200 dark:border-stone-700 ml-2 space-y-4 py-1">
                                    {list.into_iter().map(|entry| {
                                        let icon = entry.action.icon();
                                        let label = entry.action.label();
                                        let colour = action_colour(&entry.action);
                                        let actor = actor_display(&entry);
                                        let detail = action_detail(&entry);
                                        let ts = format_timestamp(&entry.created_at);
                                        view! {
                                            <li class="ml-4">
                                                // Icon bubble on the timeline line
                                                <span class=format!(
                                                    "absolute -left-3 flex h-6 w-6 items-center justify-center rounded-full text-xs {colour}"
                                                )>
                                                    <span class="material-symbols-outlined" style="font-size:14px">{icon}</span>
                                                </span>
                                                <div class="text-xs leading-relaxed">
                                                    <span class="font-medium text-stone-800 dark:text-stone-200">{actor}</span>
                                                    <span class="text-stone-500 dark:text-stone-400">{format!(" {label}")}</span>
                                                    {detail.map(|d| view! {
                                                        <span class="text-stone-400 dark:text-stone-500">{format!(" · {d}")}</span>
                                                    })}
                                                    <br />
                                                    <span class="text-stone-400 dark:text-stone-500 text-[11px]">{ts}</span>
                                                </div>
                                            </li>
                                        }
                                    }).collect::<Vec<_>>()}
                                </ol>
                            }.into_any(),
                        })}
                    </Suspense>
        </crate::components::node_section::SectionShell>
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
}
