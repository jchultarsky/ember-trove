use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;
use uuid::Uuid;

use crate::{
    activity::ActivityRecord,
    attachment::Attachment,
    edge::Edge,
    favorite::Favorite,
    graph::NodePosition,
    id::{AttachmentId, NodeId, TaskId, WebhookId},
    node::Node,
    node_link::NodeLink,
    node_version::NodeVersion,
    note::Note,
    permission::Permission,
    person::{MembershipRecord, Person},
    search::SearchPreset,
    tag::Tag,
    task::Task,
    template::NodeTemplate,
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
    /// People in the registry (#147). 0 for archives made before them.
    #[serde(default)]
    pub people: u32,
    /// Activity-log rows (#208). 0 for archives made before backups
    /// carried the log; restoring one of those leaves Activity empty.
    #[serde(default)]
    pub activity: u32,
    /// Inbox captures (#234). 0 for archives made before them.
    #[serde(default)]
    pub captures: u32,
}

/// The archive format this release writes.
/// 3 (#222): node kinds lost `resource` (merged into `reference`), so a
/// format-2 archive can name a kind that no longer exists. Older archives
/// are refused with a clear message rather than half-read (owner decision,
/// 2026-10-31); a rollback goes through the server snapshot instead.
/// 4 (#234): captures have their own list, and every note has a node.
/// 5 (#281): an attachment's bytes are stored under its id
/// ([`attachment_entry_path`]), no longer under its node and file name,
/// which two attachments can share. The archive also carries templates,
/// saved searches, editor prefs, webhooks (without their secrets) and
/// recurrence links. An older release would restore a format-5 archive
/// without any attachment bytes, so the number moves and it refuses.
/// 6 (#282): the archive carries Project Briefs. An older release would
/// restore it without them and say nothing, so the number moves
/// (`.claude/rules/api.md`, "What a backup holds").
/// 7 (owner decision, 2026-11-10): a webhook's address is no longer in the
/// archive ([`BackupWebhook`]). An older release needs it to restore a
/// webhook at all, so the number moves and it refuses.
/// 8 (#282): the archive carries each account's Focus ([`BackupFocus`]).
/// An older release would restore it without them and say nothing, so the
/// number moves.
/// 9 (S5a, #314): a task carries its completion date, and may be
/// `waiting`. An older release would drop the dates and can't read the new
/// state, so the number moves. Archives of 3 to 8 still restore: their
/// tasks have no completion date, and `in_progress` reads as open.
/// 10 (S6a, #320): a Project carries its declared state and the date it
/// was declared. An older release would drop both and restore every
/// closed or parked project as if nothing had been declared, so the
/// number moves. Archives of 3 to 9 still restore: a project's state is
/// read from its label, once (`project_state::restored`).
pub const SCHEMA_VERSION: u32 = 10;

/// The oldest format this release restores. Format 3 differs from 4 only
/// in holding captures as notes without a node, which restore converts
/// losslessly (`capture::from_nodeless_note`), so the backups taken just
/// before 3.5.0 stay restorable. Formats 3 and 4 keep their attachment
/// bytes under the old paths, which [`match_attachment_entries`] still reads.
pub const OLDEST_RESTORABLE: u32 = 3;

/// Where an attachment's bytes sit in a format-5 archive: under its id,
/// which is unique. The path holds no file name.
pub fn attachment_entry_path(id: AttachmentId) -> String {
    format!("attachments/{}", id.0)
}

/// Where formats 3 and 4 put an attachment's bytes. Two attachments on one
/// node with the same file name share this path (#281).
fn legacy_attachment_entry_path(a: &Attachment) -> String {
    format!("attachments/{}/{}", a.node_id.0, a.filename)
}

/// For each file entry of an archive, given as `(path, byte length)` in
/// archive order, the index of the attachment whose bytes it holds, or
/// `None` when that can't be told. Restore uploads only what this names,
/// so one attachment's bytes never land under another's key (#281).
///
/// - A format-5 path names the attachment by id.
/// - An old path can be shared by several attachments. The backup wrote
///   their entries in the order of `attachments`, so when every one of them
///   has an entry they pair up in order. When some are missing (the backup
///   skips a file it couldn't fetch), an entry is matched only if its length
///   fits exactly one attachment still unmatched.
pub fn match_attachment_entries(
    attachments: &[Attachment],
    entries: &[(&str, u64)],
) -> Vec<Option<usize>> {
    use std::collections::HashMap;

    let by_id: HashMap<String, usize> = attachments
        .iter()
        .enumerate()
        .map(|(i, a)| (attachment_entry_path(a.id), i))
        .collect();
    let mut sharing_path: HashMap<String, Vec<usize>> = HashMap::new();
    for (i, a) in attachments.iter().enumerate() {
        sharing_path
            .entry(legacy_attachment_entry_path(a))
            .or_default()
            .push(i);
    }

    let mut matched: Vec<Option<usize>> = vec![None; entries.len()];
    let mut old_entries: Vec<(&str, Vec<usize>)> = Vec::new();
    for (e, (path, _)) in entries.iter().enumerate() {
        if let Some(&a) = by_id.get(*path) {
            matched[e] = Some(a);
        } else if let Some(group) = old_entries.iter_mut().find(|(p, _)| p == path) {
            group.1.push(e);
        } else {
            old_entries.push((path, vec![e]));
        }
    }

    for (path, group) in old_entries {
        let Some(candidates) = sharing_path.get(path) else {
            continue;
        };
        if group.len() == candidates.len() {
            for (&e, &a) in group.iter().zip(candidates) {
                matched[e] = Some(a);
            }
            continue;
        }
        let mut free: Vec<usize> = candidates.clone();
        for e in group {
            let fits: Vec<usize> = free
                .iter()
                .copied()
                .filter(|&a| u64::try_from(attachments[a].size_bytes).ok() == Some(entries[e].1))
                .collect();
            if let [only] = fits[..] {
                matched[e] = Some(only);
                free.retain(|&a| a != only);
            }
        }
    }
    matched
}

/// One owner's saved editor state for a task or a note, as an archive holds
/// it: `EditorPref` plus the owner, which the API never sends.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BackupEditorPref {
    pub owner_id: String,
    /// `"task"` or `"note"`.
    pub entity_kind: String,
    pub entity_id: Uuid,
    #[serde(default)]
    pub height: Option<i32>,
    #[serde(default)]
    pub collapsed: bool,
}

/// One project in one account's Focus (#282), as an archive holds it:
/// `project_focus::FocusItem` plus the account and the stored position,
/// which the API never sends.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BackupFocus {
    pub subject_id: String,
    pub node_id: NodeId,
    pub position: i32,
    #[serde(default)]
    pub why_now: Option<String>,
    pub added_at: DateTime<Utc>,
}

/// A webhook as an archive holds it. SECURITY: there is no `secret` field,
/// on purpose (owner decision, 2026-11-09): an admin can download an
/// archive, and a signing secret is a credential, not content. The archive
/// records only that one was set.
///
/// Nor is its address written (owner decision, 2026-11-10): many webhook
/// addresses are credentials in themselves (a Slack-style URL posts for
/// whoever holds it, and so does a random host name), and there is no
/// telling which. `url` is read from a format-5 or -6 archive, which holds
/// it, and is never serialised.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BackupWebhook {
    pub id: WebhookId,
    pub owner_id: String,
    /// `None` from this release's archives; the address from an older one.
    #[serde(default, skip_serializing)]
    pub url: Option<String>,
    pub events: Vec<String>,
    pub is_active: bool,
    /// The webhook signed its payloads. Its secret is not in the archive.
    #[serde(default)]
    pub had_secret: bool,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// The address a webhook is restored with when the archive holds none and
/// no live webhook has its id. `.invalid` never resolves (RFC 2606), and
/// the webhook comes back switched off: its owner enters the real address.
pub const WEBHOOK_URL_NOT_IN_BACKUP: &str = "https://address-not-in-backup.invalid/";

impl BackupWebhook {
    /// Whether the webhook delivers when it is restored **from the archive
    /// alone**. It comes back switched off when the archive lacks what it
    /// needs: its address, or the secret it signed with (it would send
    /// unsigned payloads to a receiver that expects signed ones). Its owner
    /// enters them again and switches it on.
    pub fn restored_active(&self) -> bool {
        self.is_active && !self.had_secret && self.url.is_some()
    }

    /// Whether it delivers when a live webhook with its id is still there
    /// to take the address and secret from (`live_has_secret`): then only a
    /// secret that has since been removed keeps it off.
    pub fn restored_active_over_live(&self, live_has_secret: bool) -> bool {
        self.is_active && (!self.had_secret || live_has_secret)
    }
}

/// A recurring task's occurrence and the task whose completion scheduled it
/// (`node_tasks.recurrence_parent_id`). Without it, reopening a restored
/// completed task can't retract the occurrence it scheduled.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct RecurrenceLink {
    pub task_id: TaskId,
    pub parent_id: TaskId,
}

/// A note as an archive holds it. Before format 4 a note could have no
/// node; those are captures now (#234). Everything a `Note` has, with an
/// optional node.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BackupNote {
    pub id: crate::id::NoteId,
    #[serde(default)]
    pub node_id: Option<crate::id::NodeId>,
    pub owner_id: String,
    pub body: String,
    #[serde(default = "default_color")]
    pub color: String,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

fn default_color() -> String {
    "default".to_string()
}

impl From<Note> for BackupNote {
    fn from(n: Note) -> Self {
        Self {
            id: n.id,
            node_id: Some(n.node_id),
            owner_id: n.owner_id,
            body: n.body,
            color: n.color,
            created_at: n.created_at,
            updated_at: n.updated_at,
        }
    }
}

/// Top-level manifest written as `manifest.json` inside the archive.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct BackupManifest {
    /// Monotonically increasing format version; see [`SCHEMA_VERSION`].
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

/// `created_by` of a backup the server makes on its own: the
/// `ember-trove-api backup create` command, which the release workflow runs
/// after each deploy. A Cognito subject is a UUID, so it can't collide.
pub const AUTOMATIC_ACTOR: &str = "automatic";

/// How many automatic backups are kept. Each new one deletes the older ones
/// beyond this. Backups an admin makes are never deleted this way: each
/// holds a full copy, deleted accounts' data included, until someone
/// removes it, so the automatic ones must not pile up.
pub const AUTOMATIC_KEPT: usize = 10;

impl BackupJob {
    /// Made by the server, not by an admin.
    pub fn is_automatic(&self) -> bool {
        self.created_by == AUTOMATIC_ACTOR
    }
}

/// The automatic backups past the newest `keep`, oldest last. Backups an
/// admin made are never listed, whatever their age.
pub fn automatic_to_prune(jobs: &[BackupJob], keep: usize) -> Vec<Uuid> {
    let mut automatic: Vec<&BackupJob> = jobs.iter().filter(|j| j.is_automatic()).collect();
    automatic.sort_by(|a, b| b.created_at.cmp(&a.created_at).then(b.id.cmp(&a.id)));
    automatic.into_iter().skip(keep).map(|j| j.id).collect()
}

/// Full data payload written as `data.json` inside the archive.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct BackupData {
    pub nodes: Vec<Node>,
    pub edges: Vec<Edge>,
    pub tags: Vec<Tag>,
    pub notes: Vec<BackupNote>,
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
    // ── People (#147) ─────────────────────────────────────────────────────
    /// The registry: third parties' contact details, so archives need the
    /// same care as node bodies.
    #[serde(default)]
    pub people: Vec<Person>,
    /// Memberships: a person in a node's context.
    #[serde(default)]
    pub node_people: Vec<MembershipRecord>,
    // ── Activity (S1, #208) ───────────────────────────────────────────────
    /// The activity log: History (ONTOLOGY §10) can't be rebuilt, so a
    /// restore must bring it back. Archives made before this have none and
    /// restore with an empty log, as they always did.
    #[serde(default)]
    pub activity: Vec<ActivityRecord>,
    // ── Captures (format 4, #234) ─────────────────────────────────────────
    /// The Inbox's unprocessed input (ONTOLOGY §9.1). A format-3 archive
    /// has none here: its captures are its notes without a node.
    #[serde(default)]
    pub captures: Vec<crate::capture::Capture>,
    // ── Format 5 (#281) ───────────────────────────────────────────────────
    // `None` means the archive is older and never carried the list. Restore
    // then leaves the live rows alone, as it always did; `Some`, even when
    // empty, replaces them.
    /// Node templates, all owners.
    #[serde(default)]
    pub node_templates: Option<Vec<NodeTemplate>>,
    /// Saved searches, all owners.
    #[serde(default)]
    pub search_presets: Option<Vec<SearchPreset>>,
    /// Editor heights and collapsed notes, all owners.
    #[serde(default)]
    pub editor_prefs: Option<Vec<BackupEditorPref>>,
    /// Webhooks, all owners, without their signing secrets.
    #[serde(default)]
    pub webhooks: Option<Vec<BackupWebhook>>,
    /// Which task scheduled which occurrence. Empty in older archives.
    #[serde(default)]
    pub recurrence_links: Vec<RecurrenceLink>,
    // ── Format 6: Project Briefs (#282) ───────────────────────────────────
    /// Each project's current Brief. Replaced Briefs are History entries
    /// and travel in `activity`. A Brief belongs to its node, so restoring
    /// an older archive (which has none) leaves no Brief behind: the nodes
    /// it hung on are replaced.
    #[serde(default)]
    pub briefs: Vec<crate::brief::ProjectBrief>,
    // ── Format 8: Focus (#282) ────────────────────────────────────────────
    /// Every account's Focus. `None` in an older archive: restore then
    /// keeps the live entries whose project the archive brings back.
    #[serde(default)]
    pub focus: Option<Vec<BackupFocus>>,
}

impl BackupData {
    /// How many webhooks come back switched off when nothing live can fill
    /// in what the archive lacks ([`BackupWebhook::restored_active`]).
    pub fn webhooks_switched_off(&self) -> usize {
        self.webhooks
            .iter()
            .flatten()
            .filter(|w| w.is_active && !w.restored_active())
            .count()
    }
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
        assert!(data.activity.is_empty(), "no activity in an old archive");
        // What format 5 added is absent, not empty: restore must leave the
        // live templates, presets, prefs and webhooks alone.
        assert!(data.node_templates.is_none() && data.search_presets.is_none());
        assert!(data.editor_prefs.is_none() && data.webhooks.is_none());
        assert!(data.recurrence_links.is_empty());
        // Format 8's Focus too: absent, so restore keeps the live entries.
        assert!(data.focus.is_none());
    }

    #[test]
    fn an_archives_focus_is_absent_or_a_list_never_both() {
        let base = r#""nodes": [], "edges": [], "tags": [], "notes": [], "tasks": [],
            "attachments": []"#;
        let empty: BackupData =
            serde_json::from_str(&format!(r#"{{ {base}, "focus": [] }}"#)).expect("empty list");
        assert_eq!(empty.focus, Some(Vec::new()), "a list, even an empty one");
        let one: BackupData = serde_json::from_str(&format!(
            r#"{{ {base}, "focus": [{{
                "subject_id": "alice",
                "node_id": "00000000-0000-0000-0000-000000000002",
                "position": 3,
                "added_at": "2026-11-11T00:00:00Z"
            }}] }}"#
        ))
        .expect("one entry");
        let entry = &one.focus.as_deref().expect("a list")[0];
        assert_eq!((entry.subject_id.as_str(), entry.position), ("alice", 3));
        assert_eq!(entry.why_now, None, "the why-line is optional");
    }

    fn attachment(node: Uuid, name: &str, size: i64) -> Attachment {
        Attachment {
            id: AttachmentId(Uuid::new_v4()),
            node_id: NodeId(node),
            filename: name.into(),
            content_type: "image/png".into(),
            size_bytes: size,
            s3_key: format!("{node}/{}/{name}", Uuid::new_v4()),
            created_at: Utc::now(),
        }
    }

    #[test]
    fn two_attachments_with_one_name_have_their_own_entries() {
        // #281: two pasted `image.png` on one node.
        let node = Uuid::new_v4();
        let atts = [
            attachment(node, "image.png", 10),
            attachment(node, "image.png", 20),
        ];
        let (p0, p1) = (
            attachment_entry_path(atts[0].id),
            attachment_entry_path(atts[1].id),
        );
        assert_ne!(p0, p1);
        assert!(!p0.contains("image.png"), "no file name in the path");
        // Archive order doesn't matter for format-5 paths.
        let entries = [(p1.as_str(), 20), (p0.as_str(), 10)];
        assert_eq!(
            match_attachment_entries(&atts, &entries),
            vec![Some(1), Some(0)]
        );
    }

    #[test]
    fn an_old_archives_shared_path_pairs_up_in_order() {
        // Formats 3 and 4: both entries have the same path, in the order of
        // `attachments`. Restore used to send both to the first one's key.
        let node = Uuid::new_v4();
        let other = Uuid::new_v4();
        let atts = [
            attachment(node, "image.png", 10),
            attachment(other, "notes.pdf", 5),
            attachment(node, "image.png", 10),
        ];
        let shared = format!("attachments/{node}/image.png");
        let alone = format!("attachments/{other}/notes.pdf");
        let entries = [
            (shared.as_str(), 10),
            (alone.as_str(), 5),
            (shared.as_str(), 10),
        ];
        assert_eq!(
            match_attachment_entries(&atts, &entries),
            vec![Some(0), Some(1), Some(2)]
        );
    }

    #[test]
    fn a_missing_old_entry_is_matched_by_length_or_not_at_all() {
        let node = Uuid::new_v4();
        let shared = format!("attachments/{node}/image.png");
        // The backup skipped one of two files it couldn't fetch. The lengths
        // differ, so the one entry is the second attachment's.
        let atts = [
            attachment(node, "image.png", 10),
            attachment(node, "image.png", 20),
        ];
        assert_eq!(
            match_attachment_entries(&atts, &[(shared.as_str(), 20)]),
            vec![Some(1)]
        );
        // Same length: it could be either, so it is uploaded to neither.
        let same = [
            attachment(node, "image.png", 10),
            attachment(node, "image.png", 10),
        ];
        assert_eq!(
            match_attachment_entries(&same, &[(shared.as_str(), 10)]),
            vec![None]
        );
        // A length that fits nothing, and a path no attachment has.
        assert_eq!(
            match_attachment_entries(&atts, &[(shared.as_str(), 99), ("attachments/x/y", 1)]),
            vec![None, None]
        );
    }

    /// A webhook as a format-5 or -6 archive holds it: with its address.
    fn webhook(is_active: bool, had_secret: bool) -> BackupWebhook {
        BackupWebhook {
            id: WebhookId(Uuid::new_v4()),
            owner_id: "someone".into(),
            url: Some("https://example.com/hook".into()),
            events: vec!["node.created".into()],
            is_active,
            had_secret,
            created_at: Utc::now(),
            updated_at: Utc::now(),
        }
    }

    #[test]
    fn a_webhook_that_signed_comes_back_switched_off() {
        assert!(
            webhook(true, false).restored_active(),
            "unsigned: as it was"
        );
        assert!(
            !webhook(true, true).restored_active(),
            "signed: off until re-keyed"
        );
        assert!(!webhook(false, false).restored_active());
        assert!(!webhook(false, true).restored_active());
        let data = BackupData {
            webhooks: Some(vec![
                webhook(true, true),
                webhook(true, false),
                webhook(false, true),
            ]),
            ..BackupData::default()
        };
        assert_eq!(
            data.webhooks_switched_off(),
            1,
            "only the active signed one changes"
        );
        assert_eq!(BackupData::default().webhooks_switched_off(), 0);
    }

    /// SECURITY (owner decision, 2026-11-10): a webhook's address can be a
    /// credential, so it is never written, and a webhook without one comes
    /// back switched off unless a live one still has it.
    #[test]
    fn an_archived_webhook_has_no_address_and_comes_back_switched_off() {
        let hook = webhook(true, false);
        let json = serde_json::to_string(&hook).expect("serialises");
        assert!(!json.contains("example.com"), "{json}");
        assert!(!json.contains("url"), "{json}");

        let read: BackupWebhook = serde_json::from_str(&json).expect("parses");
        assert_eq!(read.url, None);
        assert!(!read.restored_active(), "no address: off");
        assert!(read.restored_active_over_live(false), "the live one has it");
        let data = BackupData {
            webhooks: Some(vec![read]),
            ..BackupData::default()
        };
        assert_eq!(data.webhooks_switched_off(), 1);

        // A signed one needs the live secret too; one that was off stays off.
        let signed = BackupWebhook {
            url: None,
            ..webhook(true, true)
        };
        assert!(signed.restored_active_over_live(true));
        assert!(!signed.restored_active_over_live(false));
        assert!(!webhook(false, false).restored_active_over_live(true));

        // An older archive holds the address, and it is still read.
        let older = r#"{"id":"00000000-0000-0000-0000-000000000001","owner_id":"o",
            "url":"https://example.com/hook","events":[],"is_active":true,
            "created_at":"2026-01-01T00:00:00Z","updated_at":"2026-01-01T00:00:00Z"}"#;
        let read: BackupWebhook = serde_json::from_str(older).expect("parses");
        assert_eq!(read.url.as_deref(), Some("https://example.com/hook"));
        assert!(read.restored_active());
        assert!(WEBHOOK_URL_NOT_IN_BACKUP.ends_with(".invalid/"));
    }

    #[test]
    fn an_archived_webhook_has_no_secret_field() {
        // The type is the guard: nothing can put a secret into data.json.
        let json = serde_json::to_value(webhook(true, true)).expect("serialises");
        let keys: Vec<&str> = json
            .as_object()
            .expect("object")
            .keys()
            .map(String::as_str)
            .collect();
        assert!(
            !keys
                .iter()
                .any(|k| k.contains("secret") && *k != "had_secret"),
            "{keys:?}"
        );
        // An archive that somehow carries one is read without it.
        let with_secret = r#"{"id":"00000000-0000-0000-0000-000000000001","owner_id":"o",
            "url":"https://example.com","events":[],"is_active":true,"had_secret":true,
            "secret":"s3cr3t","created_at":"2026-01-01T00:00:00Z",
            "updated_at":"2026-01-01T00:00:00Z"}"#;
        let read: BackupWebhook = serde_json::from_str(with_secret).expect("parses");
        assert!(!format!("{read:?}").contains("s3cr3t"));
    }

    #[test]
    fn an_activity_action_this_build_does_not_know_still_parses() {
        // A newer release may have written an action this build has never
        // heard of; the archive must stay readable and keep it verbatim.
        let json = r#"{
            "nodes": [], "edges": [], "tags": [], "notes": [], "tasks": [],
            "attachments": [],
            "activity": [{
                "id": "00000000-0000-0000-0000-000000000001",
                "node_id": "00000000-0000-0000-0000-000000000002",
                "subject_id": "someone",
                "action": "an_action_from_the_future",
                "metadata": {"k": "v"},
                "created_at": "2026-01-01T00:00:00Z"
            }]
        }"#;
        let data: BackupData = serde_json::from_str(json).expect("archive must parse");
        assert_eq!(data.activity.len(), 1);
        assert_eq!(data.activity[0].action, "an_action_from_the_future");
    }

    fn job(created_by: &str, minutes_ago: i64) -> BackupJob {
        BackupJob {
            id: Uuid::new_v4(),
            created_by: created_by.into(),
            created_at: Utc::now() - chrono::Duration::minutes(minutes_ago),
            size_bytes: 1,
            s3_key: "backups/x.tar.gz".into(),
            node_count: 0,
            edge_count: 0,
            tag_count: 0,
            note_count: 0,
            task_count: 0,
            attachment_count: 0,
            comment: None,
        }
    }

    #[test]
    fn only_automatic_backups_past_the_newest_are_pruned() {
        // 12 automatic (0 = newest … 11 = oldest) and 2 by an admin, both
        // older than every automatic one.
        let automatic: Vec<BackupJob> = (0..12).map(|m| job(AUTOMATIC_ACTOR, m)).collect();
        let mut jobs = vec![job("f1eb2590-admin", 100), job("f1eb2590-admin", 200)];
        jobs.extend(automatic.iter().rev().cloned()); // order must not matter
        let pruned = automatic_to_prune(&jobs, 10);
        assert_eq!(pruned, vec![automatic[10].id, automatic[11].id]);
    }

    #[test]
    fn nothing_is_pruned_at_or_under_the_limit() {
        let jobs: Vec<BackupJob> = (0..10).map(|m| job(AUTOMATIC_ACTOR, m)).collect();
        assert!(automatic_to_prune(&jobs, AUTOMATIC_KEPT).is_empty());
        assert!(automatic_to_prune(&[], AUTOMATIC_KEPT).is_empty());
    }

    #[test]
    fn an_admin_backup_is_not_automatic() {
        assert!(job(AUTOMATIC_ACTOR, 0).is_automatic());
        assert!(!job("f1eb2590-0091-70e4-d9b3-24e4a23d24d1", 0).is_automatic());
    }
}
