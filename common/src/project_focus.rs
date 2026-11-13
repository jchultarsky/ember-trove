//! Focus (#282, proposal B): the projects that hold a person's attention.
//!
//! Focus is to Projects what My Day is to Tasks: a declared, ordered set,
//! shown first on the dashboard. Each entry may carry a "why now" line.
//!
//! Focus is **private to the person whose Focus it is** (owner decision,
//! 2026-11-09). It is not part of the shared node: every row is keyed by
//! the account, the API matches the caller on every query, and an admin has
//! no bypass (like Captures and People). A sharee never learns that a
//! project is in someone's Focus, and never reads their why-line.
//!
//! It is persistent: nothing expires it, and the card shows how long the
//! project has been there (owner decision 3). A project stops showing when
//! it no longer [`holds_focus`], or when its reader can no longer see it.
//! That is a filter on the read: a row is deleted only by its own person
//! removing it, by its node going, or by the account being deleted.
//!
//! Named `project_focus` because "focus" already means a task's My Day
//! date (`focus_date`) and the browser's keyboard focus.
//!
//! Everything here is pure, so the rules are unit-tested once and shared by
//! the API and the UI.

use chrono::{DateTime, NaiveDate, Utc};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

use crate::id::NodeId;
use crate::node::{NodeStatus, NodeType};
use crate::ontology::{Capability, allows};
use crate::project_state::ProjectState;
use crate::task::ProjectDashboardEntry;

/// Longest why-line, in characters: one short sentence.
pub const MAX_WHY_NOW: usize = 200;
/// Past this many entries the dashboard says Focus works best small. A
/// hint only: the software sets no cap (ONTOLOGY §12).
pub const SOFT_LIMIT: usize = 5;
/// Most ids one reorder may carry. The dashboard lists at most this many
/// projects, so a longer list is not something the app sends.
pub const MAX_ORDER: usize = 500;

/// One project in the caller's Focus.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct FocusItem {
    pub node_id: NodeId,
    /// Why this project, now. One line; private to its writer.
    #[serde(default)]
    pub why_now: Option<String>,
    /// When the project entered Focus. A changed why-line or a new order
    /// leaves it alone.
    pub added_at: DateTime<Utc>,
}

/// A Focus entry with its dashboard card, as `GET /focus` returns it.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct FocusEntry {
    pub focus: FocusItem,
    pub project: ProjectDashboardEntry,
}

/// `PUT /focus/{node_id}` body: put the project in Focus. A project
/// already there keeps its place and its `added_at`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct FocusInput {
    /// Absent: leave the why-line as it is, so adding a project that is
    /// already in Focus (from a stale page, say) never wipes one. `null`:
    /// clear it. A string: set it.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "crate::task::deser_double_opt"
    )]
    pub why_now: Option<Option<String>>,
}

/// What a put does to the why-line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WhyChange {
    /// Leave it: none for a new entry, the stored one for an old.
    Keep,
    /// Replace it; `None` clears it.
    Set(Option<String>),
}

impl FocusInput {
    /// Put the project in Focus and leave any why-line alone.
    #[must_use]
    pub fn add() -> Self {
        Self { why_now: None }
    }

    /// Set the why-line (`None` clears it).
    #[must_use]
    pub fn why(text: Option<String>) -> Self {
        Self {
            why_now: Some(text),
        }
    }

    /// The validated change this body asks for.
    pub fn change(&self) -> Result<WhyChange, FocusError> {
        match &self.why_now {
            None => Ok(WhyChange::Keep),
            Some(why) => normalize_why(why.as_deref()).map(WhyChange::Set),
        }
    }
}

/// `PUT /focus/{node_id}/why` body: the why-line of a project that is in
/// the caller's Focus. `null`, absent or blank clears it. Unlike a put of
/// the entry, this never adds the project.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct FocusWhy {
    #[serde(default)]
    pub why_now: Option<String>,
}

/// `PUT /focus/order` body: the caller's Focus, in the order they want.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct FocusOrder {
    pub node_ids: Vec<NodeId>,
}

/// Why a Focus write was refused. The message is the API's 422 text; it
/// never echoes the user's content.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum FocusError {
    #[error("the why-line is longer than {MAX_WHY_NOW} characters")]
    WhyTooLong,
    #[error("the why-line must be a single line")]
    WhyMultiline,
    #[error("the why-line can't hold control characters")]
    WhyControl,
    #[error("an archived project can't be in Focus")]
    Archived,
    #[error("a parked project can't be in Focus")]
    Parked,
    #[error("a closed project can't be in Focus")]
    Closed,
    #[error("only Projects have a place in Focus")]
    NotAProject,
    #[error("a new order names at most {MAX_ORDER} projects")]
    OrderTooLong,
}

/// Validate and tidy a why-line: trimmed, and a blank one is no why-line.
pub fn normalize_why(raw: Option<&str>) -> Result<Option<String>, FocusError> {
    match raw.map(str::trim) {
        None | Some("") => Ok(None),
        Some(w) if w.contains(['\n', '\r', '\u{2028}', '\u{2029}']) => {
            Err(FocusError::WhyMultiline)
        }
        // A NUL can't be stored at all (Postgres refuses it, which would
        // answer as a 500), and no other control character belongs in one
        // line of text. A tab inside the line is kept.
        Some(w) if w.chars().any(|c| c.is_control() && c != '\t') => Err(FocusError::WhyControl),
        Some(w) if w.chars().count() > MAX_WHY_NOW => Err(FocusError::WhyTooLong),
        Some(w) => Ok(Some(w.to_string())),
    }
}

/// Why a node does not show in Focus, or `None` when it does: it is not
/// a Project (§3.2), it is archived (archived nodes are hidden from
/// lists, §3.3), or its declared state does not ask for attention (§10:
/// parked, complete and cancelled; `ProjectState::asks_for_attention`).
/// A Project with no state can't exist; it reads as closed.
///
/// The one rule for what falls out of Focus by its own state. The API's
/// list applies it in SQL (a Postgres test holds the two together) and its
/// add route refuses, with this reason, what the list would hide.
#[must_use]
pub fn focus_refusal(
    kind: &NodeType,
    status: &NodeStatus,
    state: Option<ProjectState>,
) -> Option<FocusError> {
    if !allows(kind, Capability::Focus) {
        return Some(FocusError::NotAProject);
    }
    if *status == NodeStatus::Archived {
        return Some(FocusError::Archived);
    }
    match state {
        Some(ProjectState::Active) => None,
        Some(ProjectState::Parked) => Some(FocusError::Parked),
        Some(ProjectState::Complete | ProjectState::Cancelled) | None => Some(FocusError::Closed),
    }
}

/// Whether a node shows in Focus: [`focus_refusal`] finds no reason
/// against it.
#[must_use]
pub fn holds_focus(kind: &NodeType, status: &NodeStatus, state: Option<ProjectState>) -> bool {
    focus_refusal(kind, status, state).is_none()
}

/// Whether to say that Focus works best small.
#[must_use]
pub fn over_soft_limit(entries: usize) -> bool {
    entries > SOFT_LIMIT
}

/// Days the project has been in Focus, by the calendar: `added` and
/// `today` are dates **in the reader's own time zone** (the UI reads them
/// from the browser). So a project added last night reads "1d" this
/// morning, not "today" until 24 hours have passed. Never negative: a
/// clock that runs behind the server's reads as today.
#[must_use]
pub fn days_in_focus(added: NaiveDate, today: NaiveDate) -> i64 {
    (today - added).num_days().max(0)
}

/// The card's age line: "In focus today", then "In focus 6d".
#[must_use]
pub fn in_focus_label(added: NaiveDate, today: NaiveDate) -> String {
    match days_in_focus(added, today) {
        0 => "In focus today".to_string(),
        days => format!("In focus {days}d"),
    }
}

/// A step in the Focus order.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Step {
    Up,
    Down,
}

/// `ids` with `id` moved one place. `None` when it can't move: it isn't in
/// the list, or it is already at that end.
#[must_use]
pub fn moved(ids: &[NodeId], id: NodeId, step: Step) -> Option<Vec<NodeId>> {
    let from = ids.iter().position(|n| *n == id)?;
    let to = match step {
        Step::Up => from.checked_sub(1)?,
        Step::Down => from + 1,
    };
    if to >= ids.len() {
        return None;
    }
    let mut next = ids.to_vec();
    next.swap(from, to);
    Some(next)
}

#[cfg(test)]
mod tests {
    use uuid::Uuid;

    use super::*;

    fn id(n: u128) -> NodeId {
        NodeId(Uuid::from_u128(n))
    }

    #[test]
    fn a_why_line_is_trimmed_and_a_blank_one_is_none() {
        assert_eq!(normalize_why(None), Ok(None));
        assert_eq!(normalize_why(Some("")), Ok(None));
        assert_eq!(normalize_why(Some("  \t ")), Ok(None));
        assert_eq!(
            normalize_why(Some("  Decision needed Friday ")),
            Ok(Some("Decision needed Friday".to_string()))
        );
    }

    #[test]
    fn a_why_line_is_one_line_of_at_most_the_limit() {
        let longest = "é".repeat(MAX_WHY_NOW);
        assert_eq!(normalize_why(Some(&longest)), Ok(Some(longest.clone())));
        assert_eq!(
            normalize_why(Some(&format!("{longest}x"))),
            Err(FocusError::WhyTooLong)
        );
        for breaker in ["a\nb", "a\rb", "a\u{2028}b", "a\u{2029}b"] {
            assert_eq!(
                normalize_why(Some(breaker)),
                Err(FocusError::WhyMultiline),
                "{breaker:?}"
            );
        }
        // Trimmed first: a trailing newline is not a second line.
        assert_eq!(normalize_why(Some("a\n")), Ok(Some("a".to_string())));
    }

    #[test]
    fn a_why_line_holds_no_control_characters() {
        // A NUL can't be stored: without this it reached Postgres and
        // answered as a 500.
        for bad in ["a\0b", "\0", "a\u{1b}[31mb", "a\u{7f}b"] {
            assert_eq!(
                normalize_why(Some(bad)),
                Err(FocusError::WhyControl),
                "{bad:?}"
            );
        }
        // A tab inside the line is text.
        assert_eq!(normalize_why(Some("a\tb")), Ok(Some("a\tb".to_string())));
        assert!(!FocusError::WhyControl.to_string().contains('\0'));
    }

    #[test]
    fn a_refusal_never_repeats_what_was_written() {
        let secret = "s3cret-reason ".repeat(40);
        let err = normalize_why(Some(&secret)).expect_err("too long");
        assert!(!err.to_string().contains("s3cret"), "{err}");
        let err = normalize_why(Some("s3cret\nreason")).expect_err("two lines");
        assert!(!err.to_string().contains("s3cret"), "{err}");
    }

    #[test]
    fn only_an_active_project_that_is_not_archived_holds_focus() {
        use NodeStatus::{Archived, Draft, Published};
        use NodeType::{Area, Article, Project, Reference};
        let states = [
            None,
            Some(ProjectState::Active),
            Some(ProjectState::Parked),
            Some(ProjectState::Complete),
            Some(ProjectState::Cancelled),
        ];
        for kind in [Article, Project, Area, Reference] {
            for status in [Draft, Published, Archived] {
                for state in states {
                    let expected = kind == Project
                        && status != Archived
                        && state == Some(ProjectState::Active);
                    assert_eq!(
                        holds_focus(&kind, &status, state),
                        expected,
                        "{kind:?} {status:?} {state:?}"
                    );
                }
            }
        }
    }

    #[test]
    fn a_refusal_names_its_reason_and_the_kind_comes_first() {
        use NodeStatus::{Archived, Published};
        use NodeType::{Area, Project};
        let active = Some(ProjectState::Active);
        assert_eq!(focus_refusal(&Project, &Published, active), None);
        assert_eq!(
            focus_refusal(&Area, &Archived, None),
            Some(FocusError::NotAProject)
        );
        // Archived is said before the state: un-archiving is the first step.
        assert_eq!(
            focus_refusal(&Project, &Archived, Some(ProjectState::Parked)),
            Some(FocusError::Archived)
        );
        assert_eq!(
            focus_refusal(&Project, &Published, Some(ProjectState::Parked)),
            Some(FocusError::Parked)
        );
        for closed in [ProjectState::Complete, ProjectState::Cancelled] {
            assert_eq!(
                focus_refusal(&Project, &Published, Some(closed)),
                Some(FocusError::Closed)
            );
        }
        // A Project always has a state; one without reads as closed, never
        // as something to put in front of its person.
        assert_eq!(
            focus_refusal(&Project, &Published, None),
            Some(FocusError::Closed)
        );
    }

    #[test]
    fn the_hint_starts_past_the_soft_limit() {
        assert!(!over_soft_limit(0));
        assert!(!over_soft_limit(SOFT_LIMIT));
        assert!(over_soft_limit(SOFT_LIMIT + 1));
    }

    #[test]
    fn the_age_counts_calendar_days_and_never_goes_negative() {
        let day = |d: u32| NaiveDate::from_ymd_opt(2026, 11, d).expect("a date");
        assert_eq!(days_in_focus(day(11), day(11)), 0);
        // Added last night: one day by the calendar, whatever the hours.
        assert_eq!(days_in_focus(day(11), day(12)), 1);
        assert_eq!(days_in_focus(day(5), day(11)), 6);
        assert_eq!(days_in_focus(day(12), day(11)), 0, "a clock running behind");
    }

    #[test]
    fn the_age_line_says_today_then_days() {
        let day = |d: u32| NaiveDate::from_ymd_opt(2026, 11, d).expect("a date");
        assert_eq!(in_focus_label(day(11), day(11)), "In focus today");
        assert_eq!(in_focus_label(day(10), day(11)), "In focus 1d");
        let long_ago = NaiveDate::from_ymd_opt(2026, 10, 2).expect("a date");
        assert_eq!(in_focus_label(long_ago, day(11)), "In focus 40d");
    }

    #[test]
    fn a_move_swaps_neighbours_and_stops_at_the_ends() {
        let ids = [id(1), id(2), id(3)];
        assert_eq!(
            moved(&ids, id(2), Step::Up),
            Some(vec![id(2), id(1), id(3)])
        );
        assert_eq!(
            moved(&ids, id(2), Step::Down),
            Some(vec![id(1), id(3), id(2)])
        );
        assert_eq!(moved(&ids, id(1), Step::Up), None);
        assert_eq!(moved(&ids, id(3), Step::Down), None);
        assert_eq!(moved(&ids, id(9), Step::Up), None);
        assert_eq!(moved(&[], id(1), Step::Down), None);
    }

    #[test]
    fn an_item_without_a_why_line_reads_from_json() {
        let item: FocusItem = serde_json::from_value(serde_json::json!({
            "node_id": Uuid::from_u128(7),
            "added_at": "2026-11-11T10:00:00Z",
        }))
        .expect("item");
        assert_eq!(item.why_now, None);
    }

    #[test]
    fn a_put_without_a_why_line_keeps_the_one_there() {
        // Absent, null and a value are three different requests.
        let read = |json: &str| serde_json::from_str::<FocusInput>(json).expect(json);
        assert_eq!(read("{}").change(), Ok(WhyChange::Keep));
        assert_eq!(
            read(r#"{"why_now": null}"#).change(),
            Ok(WhyChange::Set(None))
        );
        assert_eq!(
            read(r#"{"why_now": "  "}"#).change(),
            Ok(WhyChange::Set(None)),
            "a blank why-line clears it"
        );
        assert_eq!(
            read(r#"{"why_now": " Blocked on me "}"#).change(),
            Ok(WhyChange::Set(Some("Blocked on me".to_string())))
        );
        assert_eq!(
            read(r#"{"why_now": "a\nb"}"#).change(),
            Err(FocusError::WhyMultiline)
        );
        // And they are sent as they are read.
        let sent = |input: &FocusInput| serde_json::to_string(input).expect("json");
        assert_eq!(sent(&FocusInput::add()), "{}");
        assert_eq!(sent(&FocusInput::why(None)), r#"{"why_now":null}"#);
        assert_eq!(
            sent(&FocusInput::why(Some("x".to_string()))),
            r#"{"why_now":"x"}"#
        );
    }
}
