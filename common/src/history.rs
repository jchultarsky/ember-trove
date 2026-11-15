//! A node's History (ONTOLOGY §10; slice S6d of #204): the record of what
//! actually occurred about it, in one chronology.
//!
//! Every document kind has one. Today it is the node's notes (§9.2) and
//! the changes its log holds that are something that *happened* to it
//! (`ActivityAction::is_history`: a status label or a state changed, a
//! cascade closed things, a Brief was replaced, it was published or
//! archived). Events join it in a later slice (S11a).
//!
//! This module is the rule for putting the two together and nothing
//! else. The notes and the changes are read by their own routes, each
//! with its own rights and redaction; [`merge`] interleaves what a reader
//! was given.

use chrono::{DateTime, Utc};

use crate::activity::ActivityEntry;
use crate::note::ReaderNote;

/// One line of a History.
#[derive(Debug, Clone)]
pub enum HistoryItem {
    /// A note, at the time it was written (never the time it was edited).
    Note(ReaderNote),
    /// A change, at the time it was made.
    Change(ActivityEntry),
}

impl HistoryItem {
    /// Its place in the chronology.
    #[must_use]
    pub fn at(&self) -> DateTime<Utc> {
        match self {
            Self::Note(note) => note.created_at,
            Self::Change(change) => change.created_at,
        }
    }

    /// A key that is the same for the same line across reloads, and
    /// differs between a note and a change (their ids are separate
    /// spaces): what a keyed list needs to keep a line in place.
    #[must_use]
    pub fn key(&self) -> String {
        match self {
            Self::Note(note) => format!("n:{}", note.id),
            Self::Change(change) => format!("c:{}", change.id.0),
        }
    }

    #[must_use]
    pub fn is_note(&self) -> bool {
        matches!(self, Self::Note(_))
    }

    /// The order within one instant: notes before changes, then the
    /// larger id first (as the two reads order their own rows), so the
    /// order is total and the same on every load.
    fn tie(&self) -> (u8, std::cmp::Reverse<uuid::Uuid>) {
        match self {
            Self::Note(note) => (0, std::cmp::Reverse(note.id.0)),
            Self::Change(change) => (1, std::cmp::Reverse(change.id.0)),
        }
    }
}

/// A node's History as one reader sees it.
#[derive(Debug, Clone, Default)]
pub struct Chronology {
    /// Newest first.
    pub items: Vec<HistoryItem>,
    /// Set when the changes that were read are not all of them (the
    /// server said there are more): changes made before this moment are
    /// not in `items`. The notes are all there. The page says so at that
    /// point rather than show a chronology that looks complete and is
    /// not.
    pub changes_cut_at: Option<DateTime<Utc>>,
}

/// Interleave a node's notes and its changes, newest first.
///
/// `more` is the server's word that the node has older entries than
/// `changes` holds (`activity::MORE_HEADER`). It is not worked out from
/// how many came back: the route leaves out an entry its reader may not
/// see after its limit, so a cut read can come back short.
///
/// `changes` may hold anything the log returned: an entry that is not
/// History (`is_history`) is dropped here too, so a caller that forgot to
/// ask for the History scope still shows no audit line as something that
/// occurred.
#[must_use]
pub fn merge(notes: Vec<ReaderNote>, changes: Vec<ActivityEntry>, more: bool) -> Chronology {
    // Taken over every row read, History or not: nothing older than the
    // oldest of them was read at all.
    let changes_cut_at = more
        .then(|| changes.iter().map(|change| change.created_at).min())
        .flatten();
    let mut items: Vec<HistoryItem> = notes
        .into_iter()
        .map(HistoryItem::Note)
        .chain(
            changes
                .into_iter()
                .filter(|change| change.action.is_history())
                .map(HistoryItem::Change),
        )
        .collect();
    items.sort_by(|a, b| b.at().cmp(&a.at()).then_with(|| a.tie().cmp(&b.tie())));
    Chronology {
        items,
        changes_cut_at,
    }
}

/// How many notes a History shows before "Show more".
pub const INITIAL_NOTES: usize = 5;
/// And how many lines at most, whatever they are.
pub const INITIAL_LINES: usize = 15;

/// How many of `items` (newest first) show before "Show more": through
/// the fifth note, and never more than fifteen lines. Counting notes, not
/// lines, keeps a burst of changes (a project parked and re-opened a few
/// times) from pushing every note out of sight; the cap keeps a node
/// with few notes and many changes from opening as a wall.
#[must_use]
pub fn initial_cut(items: &[HistoryItem]) -> usize {
    let mut notes = 0;
    for (index, item) in items.iter().enumerate() {
        if index >= INITIAL_LINES {
            return INITIAL_LINES;
        }
        if item.is_note() {
            notes += 1;
            if notes == INITIAL_NOTES {
                return index + 1;
            }
        }
    }
    items.len()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::activity::ActivityAction;
    use crate::id::{ActivityId, NodeId, NoteId};
    use crate::note::Note;
    use chrono::{Duration, TimeZone};
    use uuid::Uuid;

    fn at(minutes: i64) -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 5, 1, 9, 0, 0)
            .single()
            .expect("a date")
            + Duration::minutes(minutes)
    }

    fn note(id: u128, minutes: i64) -> ReaderNote {
        ReaderNote {
            note: Note {
                id: NoteId(Uuid::from_u128(id)),
                node_id: NodeId(Uuid::nil()),
                owner_id: "alice".into(),
                body: format!("note {id}"),
                color: "default".into(),
                created_at: at(minutes),
                updated_at: at(minutes),
                // Edited much later: its place does not move.
                edited_at: Some(at(minutes + 10_000)),
            },
            may_edit: false,
            may_remove: false,
        }
    }

    fn change(id: u128, minutes: i64, action: ActivityAction) -> ActivityEntry {
        ActivityEntry {
            id: ActivityId(Uuid::from_u128(id)),
            node_id: NodeId(Uuid::nil()),
            subject_id: "alice".into(),
            action,
            metadata: serde_json::json!({}),
            created_at: at(minutes),
        }
    }

    fn keys(chronology: &Chronology) -> Vec<String> {
        chronology
            .items
            .iter()
            .map(|item| match item {
                HistoryItem::Note(n) => format!("n{}", n.id.0.as_u128()),
                HistoryItem::Change(c) => format!("c{}", c.id.0.as_u128()),
            })
            .collect()
    }

    #[test]
    fn notes_and_changes_interleave_newest_first() {
        let merged = merge(
            vec![note(1, 10), note(2, 30)],
            vec![
                change(1, 20, ActivityAction::StatusChanged),
                change(2, 40, ActivityAction::ProjectStateChanged),
            ],
            false,
        );
        assert_eq!(keys(&merged), ["c2", "n2", "c1", "n1"]);
        assert_eq!(merged.changes_cut_at, None, "the read was whole");
        // A note sits where it was written, whenever it was edited.
        assert_eq!(merged.items[1].at(), at(30));
    }

    #[test]
    fn the_order_is_the_same_whatever_order_the_rows_came_in() {
        let notes = vec![note(2, 5), note(1, 5)];
        let changes = vec![
            change(9, 5, ActivityAction::CascadeClosed),
            change(3, 5, ActivityAction::StatusChanged),
        ];
        let one = merge(notes.clone(), changes.clone(), false);
        let mut reversed_notes = notes;
        reversed_notes.reverse();
        let mut reversed_changes = changes;
        reversed_changes.reverse();
        let two = merge(reversed_notes, reversed_changes, false);
        // Within one instant: notes first, then changes, each with the
        // larger id first, as the server's own lists order them.
        assert_eq!(keys(&one), ["n2", "n1", "c9", "c3"]);
        assert_eq!(keys(&one), keys(&two));
        // The keys a list uses are distinct even when the ids coincide.
        let same_id = merge(
            vec![note(7, 0)],
            vec![change(7, 0, ActivityAction::StatusChanged)],
            false,
        );
        assert_ne!(same_id.items[0].key(), same_id.items[1].key());
    }

    /// Activity's audit lines are not History, even if a caller asked the
    /// log for everything.
    #[test]
    fn an_audit_entry_is_never_shown_as_something_that_occurred() {
        let merged = merge(
            vec![note(1, 0)],
            vec![
                change(1, 1, ActivityAction::Edited),
                change(2, 2, ActivityAction::LifecycleChanged),
                change(3, 3, ActivityAction::Exported),
                change(4, 4, ActivityAction::BriefSuperseded),
            ],
            false,
        );
        assert_eq!(keys(&merged), ["c4", "c2", "n1"]);
    }

    #[test]
    fn a_cut_read_of_changes_says_where_it_stops() {
        let changes = vec![
            change(1, 50, ActivityAction::StatusChanged),
            change(2, 40, ActivityAction::StatusChanged),
        ];
        // The server said there are more: nothing older than minute 40
        // was read.
        let cut = merge(vec![note(1, 10)], changes.clone(), true);
        assert_eq!(cut.changes_cut_at, Some(at(40)));
        assert_eq!(keys(&cut), ["c1", "c2", "n1"], "every note is still there");
        // It said there are no more: that is all of them, however many.
        assert_eq!(merge(vec![], changes.clone(), false).changes_cut_at, None);
        // More, and nothing came back (every entry was the reader's to
        // not see): no moment to name.
        assert_eq!(merge(vec![], vec![], true).changes_cut_at, None);
        // The cut is where the read stopped, even when its oldest row was
        // an audit line that is not shown.
        let mut mixed = changes;
        mixed.push(change(3, 30, ActivityAction::Edited));
        let cut = merge(vec![], mixed, true);
        assert_eq!(cut.changes_cut_at, Some(at(30)));
        assert_eq!(keys(&cut), ["c1", "c2"]);
    }

    fn history(shape: &str) -> Vec<HistoryItem> {
        shape
            .chars()
            .enumerate()
            .map(|(i, c)| {
                let id = i as u128;
                if c == 'n' {
                    HistoryItem::Note(note(id, 0))
                } else {
                    HistoryItem::Change(change(id, 0, ActivityAction::StatusChanged))
                }
            })
            .collect()
    }

    #[test]
    fn the_first_view_is_five_notes_and_at_most_fifteen_lines() {
        // Few of anything: all of it.
        assert_eq!(initial_cut(&history("")), 0);
        assert_eq!(initial_cut(&history("ncn")), 3);
        assert_eq!(initial_cut(&history("nnnn")), 4);
        // Exactly five notes: all, and nothing is hidden.
        assert_eq!(initial_cut(&history("nnnnn")), 5);
        // Notes only: the fifth, as the Notes section did.
        assert_eq!(initial_cut(&history("nnnnnnnn")), 5);
        // A burst of changes on top does not hide the notes.
        assert_eq!(initial_cut(&history("ccccnnnnnnn")), 9);
        // Changes between notes ride along up to the fifth note.
        assert_eq!(initial_cut(&history("ncncncncncn")), 9);
        // The cap: many changes, few notes.
        assert_eq!(initial_cut(&history(&"c".repeat(40))), INITIAL_LINES);
        let late_notes = format!("{}nnnnn", "c".repeat(14));
        assert_eq!(initial_cut(&history(&late_notes)), INITIAL_LINES);
        // Fewer lines than the cap and fewer than five notes: all.
        assert_eq!(initial_cut(&history("cccccccccnn")), 11);
    }
}
