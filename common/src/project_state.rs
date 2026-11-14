//! A Project's closed state (ONTOLOGY §4, §4.1; slice S6a of #204).
//!
//! Beside its free-text status label (`crate::project_status`) a Project
//! carries one of four declared states: active, parked, complete or
//! cancelled. Complete and cancelled are terminal. Parked is not: the
//! project is set aside on purpose and should not ask for its person's
//! attention (owner decision, 2026-11-12).
//!
//! The state is **declared**, never derived: not from the project's tasks
//! or sub-projects, and not from the wording of its label. The one
//! exception is where no declaration can exist yet: the migration that
//! added the column (051) and the restore of an archive older than it
//! read the label once, through [`state_from_label`]. A label that only
//! suggests a state is never acted on; Data health lists it
//! ([`label_disagrees`]) and its person decides.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

use crate::node::NodeType;
use crate::ontology::{Capability, allows};

/// A Project's declared state. Only a Project has one (a node of another
/// kind has none: `Node::project_state` is `None`, and the database holds
/// the two together with a CHECK).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum ProjectState {
    Active,
    Parked,
    Complete,
    Cancelled,
}

wire_enum!(ProjectState {
    Active => "active",
    Parked => "parked",
    Complete => "complete",
    Cancelled => "cancelled",
});

impl ProjectState {
    /// The user-facing name.
    #[must_use]
    pub fn label(&self) -> &'static str {
        match self {
            ProjectState::Active => "Active",
            ProjectState::Parked => "Parked",
            ProjectState::Complete => "Complete",
            ProjectState::Cancelled => "Cancelled",
        }
    }

    /// One line on what the state means, shown at the point of choice.
    /// Worded from what the app does with it, not the wish.
    #[must_use]
    pub fn description(&self) -> &'static str {
        match self {
            ProjectState::Active => "In motion. It shows on the dashboard and can be in Focus.",
            ProjectState::Parked => {
                "Set aside on purpose. Out of Focus and the Area groups; its tasks leave Today and Overdue."
            }
            ProjectState::Complete => "The outcome is reached. It leaves the dashboard and Focus.",
            ProjectState::Cancelled => {
                "Stopped before its outcome. It leaves the dashboard and Focus."
            }
        }
    }

    /// Complete and cancelled: the project is closed (§4.1). Parked is
    /// not terminal.
    #[must_use]
    pub fn is_terminal(&self) -> bool {
        matches!(self, ProjectState::Complete | ProjectState::Cancelled)
    }

    /// Whether the project, and through it its tasks, may ask for its
    /// person's attention: Active only. The one rule behind Focus
    /// (`crate::project_focus::holds_focus`), the dashboard's Area groups
    /// and My Day's carry-over and Overdue (`crate::task`).
    #[must_use]
    pub fn asks_for_attention(&self) -> bool {
        matches!(self, ProjectState::Active)
    }
}

/// Whether a task's project lets it ask for attention. `None` is a task
/// on no node, or on a node that is not a Project (older data): nothing
/// holds it back.
#[must_use]
pub fn lets_tasks_ask(project: Option<ProjectState>) -> bool {
    project.is_none_or(|s| s.asks_for_attention())
}

/// What a status label reads as.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LabelReading {
    /// The whole label is one of the words the owner ruled on
    /// (2026-10-30): it is taken at its word, once, where no declaration
    /// exists yet.
    Says(ProjectState),
    /// The label mentions a word that suggests the state. Never acted on.
    Hints(ProjectState),
    /// Nothing in the label speaks about a closed or parked state.
    Silent,
}

/// Labels that **are** a terminal state, whole and alone. Migration 051
/// spells the same two lists in SQL; a Postgres test holds them together.
/// No label says "parked": parking is declared by a person, so a
/// migration never parks a project (it would leave its person's Area
/// groups and Today on the day of the deploy).
const SAYS_COMPLETE: [&str; 3] = ["done", "complete", "completed"];
const SAYS_CANCELLED: [&str; 3] = ["cancelled", "canceled", "abandoned"];

/// Words that suggest a state when they appear in a longer label, or on
/// their own without being one of the ruled words.
const HINTS_CANCELLED: [&str; 4] = ["cancelled", "canceled", "abandoned", "dropped"];
const HINTS_COMPLETE: [&str; 7] = [
    "done",
    "complete",
    "completed",
    "finished",
    "closed",
    "shipped",
    "delivered",
];
const HINTS_PARKED: [&str; 8] = [
    "parked",
    "paused",
    "hold",
    "shelved",
    "deferred",
    "someday",
    "dormant",
    "backburner",
];

/// Read a status label. ASCII case and surrounding spaces don't matter
/// (the database compares `lower(btrim(label))` under the C collation, so
/// neither side depends on a locale). A hint is a whole word: "Undone" or
/// "Holdings" hint at nothing.
#[must_use]
pub fn read_label(label: Option<&str>) -> LabelReading {
    let Some(label) = label else {
        return LabelReading::Silent;
    };
    let whole = label.trim_matches(' ').to_ascii_lowercase();
    if SAYS_COMPLETE.contains(&whole.as_str()) {
        return LabelReading::Says(ProjectState::Complete);
    }
    if SAYS_CANCELLED.contains(&whole.as_str()) {
        return LabelReading::Says(ProjectState::Cancelled);
    }
    let mentions = |words: &[&str]| {
        whole
            .split(|c: char| !c.is_ascii_alphanumeric())
            .any(|w| words.contains(&w))
    };
    // Cancelled before complete before parked: "Cancelled, was almost
    // done" reads as cancelled. The order only picks the word the report
    // shows; its person chooses the state.
    if mentions(&HINTS_CANCELLED) {
        LabelReading::Hints(ProjectState::Cancelled)
    } else if mentions(&HINTS_COMPLETE) {
        LabelReading::Hints(ProjectState::Complete)
    } else if mentions(&HINTS_PARKED) {
        LabelReading::Hints(ProjectState::Parked)
    } else {
        LabelReading::Silent
    }
}

/// The state a Project gets where none was ever declared: what its label
/// **says**, else active. Used once per project, by migration 051's twin
/// in SQL and by the restore of an archive that predates the state.
#[must_use]
pub fn state_from_label(label: Option<&str>) -> ProjectState {
    match read_label(label) {
        LabelReading::Says(state) => state,
        LabelReading::Hints(_) | LabelReading::Silent => ProjectState::Active,
    }
}

/// The state a label reads as when that is not the project's state:
/// what Data health reports for a project whose state nobody has declared
/// yet. `None` when the label is silent or agrees.
#[must_use]
pub fn label_disagrees(label: Option<&str>, state: ProjectState) -> Option<ProjectState> {
    match read_label(label) {
        LabelReading::Says(read) | LabelReading::Hints(read) if read != state => Some(read),
        _ => None,
    }
}

/// The state, and the date it was declared, that a node restored from a
/// backup gets. `archived` is what the archive holds for it and `label`
/// its status label **as restored** (the restore tidies the label first,
/// and may lift it out of an old body).
///
/// - A node that is not a Project has neither, whatever the archive says:
///   a hand-edited archive can't fail the database's CHECK.
/// - A Project keeps the state and date its archive carries (format 10 on).
/// - A Project from an older archive gets the state its label says, as
///   migration 051 gave the live rows, and no date: nobody declared it.
#[must_use]
pub fn restored(
    kind: &NodeType,
    archived: Option<ProjectState>,
    archived_at: Option<DateTime<Utc>>,
    label: Option<&str>,
) -> (Option<ProjectState>, Option<DateTime<Utc>>) {
    if !allows(kind, Capability::ProjectStatus) {
        return (None, None);
    }
    match archived {
        Some(state) => (Some(state), archived_at),
        None => (Some(state_from_label(label)), None),
    }
}

/// The body of `PUT /nodes/{id}/project-state`: the state its sender
/// declares. Always a named state, never "toggle": a request sent twice,
/// or from a page that is out of date, still says what its person meant.
///
/// `confirm` is the token of a cascade plan the sender was shown
/// (`crate::cascade::CascadePlan::token`). A change to a terminal state
/// that closes sub-Projects or cancels tasks needs it (S6b, §4.1:
/// "confirmation first"); with the state a project already has, it asks
/// for what is still open under it to be closed (the catch-up).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct DeclareProjectState {
    pub state: ProjectState,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub confirm: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use LabelReading::{Hints, Says, Silent};
    use ProjectState::{Active, Cancelled, Complete, Parked};

    #[test]
    fn complete_and_cancelled_are_terminal_and_parked_is_not() {
        for state in ProjectState::ALL {
            let expected = matches!(state, Complete | Cancelled);
            assert_eq!(state.is_terminal(), expected, "{state:?}");
        }
    }

    #[test]
    fn only_an_active_project_asks_for_attention() {
        for state in ProjectState::ALL {
            assert_eq!(state.asks_for_attention(), *state == Active, "{state:?}");
        }
    }

    #[test]
    fn a_task_on_no_project_is_never_held_back() {
        assert!(lets_tasks_ask(None));
        assert!(lets_tasks_ask(Some(Active)));
        for state in [Parked, Complete, Cancelled] {
            assert!(!lets_tasks_ask(Some(state)), "{state:?}");
        }
    }

    #[test]
    fn every_state_has_a_name_and_says_what_it_does() {
        for state in ProjectState::ALL {
            assert!(!state.label().is_empty());
            assert!(state.description().ends_with('.'), "{state:?}");
        }
    }

    #[test]
    fn the_ruled_words_say_a_state_whatever_their_case_or_spacing() {
        for label in ["Done", "done", "DONE", " complete ", "Completed"] {
            assert_eq!(read_label(Some(label)), Says(Complete), "{label:?}");
        }
        for label in ["Cancelled", "canceled", "ABANDONED", "  Abandoned"] {
            assert_eq!(read_label(Some(label)), Says(Cancelled), "{label:?}");
        }
    }

    #[test]
    fn no_label_says_parked() {
        // Parking is declared by a person; a label only hints at it.
        for label in ["Parked", "On hold", "on-hold", "Paused", "Someday"] {
            assert_eq!(read_label(Some(label)), Hints(Parked), "{label:?}");
            assert_eq!(state_from_label(Some(label)), Active, "{label:?}");
        }
    }

    #[test]
    fn a_longer_label_only_hints() {
        assert_eq!(read_label(Some("Done, awaiting invoice")), Hints(Complete));
        assert_eq!(read_label(Some("Mostly finished")), Hints(Complete));
        assert_eq!(read_label(Some("Closed")), Hints(Complete));
        assert_eq!(read_label(Some("Cancelled by customer")), Hints(Cancelled));
        assert_eq!(read_label(Some("Dropped")), Hints(Cancelled));
        assert_eq!(read_label(Some("On hold: budget")), Hints(Parked));
        // Cancelled wins over complete, and complete over parked.
        assert_eq!(
            read_label(Some("Cancelled, was almost done")),
            Hints(Cancelled)
        );
        assert_eq!(read_label(Some("Done but on hold")), Hints(Complete));
    }

    #[test]
    fn a_hint_is_a_whole_word() {
        for label in [
            "Undone",
            "Holdings",
            "Incomplete",
            "Parkedcar",
            "Active",
            "",
        ] {
            assert_eq!(read_label(Some(label)), Silent, "{label:?}");
        }
        assert_eq!(read_label(None), Silent);
    }

    #[test]
    fn only_ascii_case_is_folded() {
        // The database lower-cases under the C collation; so does this.
        assert_eq!(read_label(Some("DONE")), Says(Complete));
        assert_eq!(read_label(Some("DÖNE")), Silent);
    }

    #[test]
    fn a_state_is_derived_only_from_what_a_label_says() {
        assert_eq!(state_from_label(None), Active);
        assert_eq!(state_from_label(Some("Active")), Active);
        assert_eq!(state_from_label(Some("Done")), Complete);
        assert_eq!(state_from_label(Some("Abandoned")), Cancelled);
        assert_eq!(state_from_label(Some("Done, awaiting invoice")), Active);
    }

    #[test]
    fn a_label_disagrees_when_it_reads_as_another_state() {
        assert_eq!(label_disagrees(Some("On hold"), Active), Some(Parked));
        assert_eq!(label_disagrees(Some("On hold"), Parked), None);
        assert_eq!(label_disagrees(Some("Done"), Active), Some(Complete));
        assert_eq!(label_disagrees(Some("Done"), Complete), None);
        assert_eq!(label_disagrees(Some("Done"), Cancelled), Some(Complete));
        assert_eq!(label_disagrees(Some("Active"), Parked), None);
        assert_eq!(label_disagrees(None, Active), None);
    }

    #[test]
    fn a_restored_project_keeps_its_state_or_reads_it_from_the_label() {
        let when = DateTime::parse_from_rfc3339("2026-11-13T10:00:00Z")
            .expect("a time")
            .with_timezone(&Utc);
        let project = NodeType::Project;
        // Format 10 on: as archived, label or not.
        assert_eq!(
            restored(&project, Some(Parked), Some(when), Some("Done")),
            (Some(Parked), Some(when))
        );
        assert_eq!(
            restored(&project, Some(Complete), None, None),
            (Some(Complete), None)
        );
        // An older archive: what the label says, and no date, even if the
        // archive somehow carries one.
        assert_eq!(
            restored(&project, None, Some(when), Some("Done")),
            (Some(Complete), None)
        );
        assert_eq!(
            restored(&project, None, None, Some("On hold")),
            (Some(Active), None)
        );
        assert_eq!(restored(&project, None, None, None), (Some(Active), None));
    }

    #[test]
    fn nothing_but_a_project_is_restored_with_a_state() {
        let when = Utc::now();
        for kind in [NodeType::Article, NodeType::Area, NodeType::Reference] {
            assert_eq!(
                restored(&kind, Some(Complete), Some(when), Some("Done")),
                (None, None),
                "{kind:?}"
            );
        }
    }

    #[test]
    fn the_request_names_a_state() {
        let body: DeclareProjectState =
            serde_json::from_str(r#"{"state":"parked"}"#).expect("a state");
        assert_eq!(body.state, Parked);
        assert_eq!(body.confirm, None, "an older tab sends the state alone");
        let confirmed: DeclareProjectState =
            serde_json::from_str(r#"{"state":"complete","confirm":"abc"}"#).expect("confirmed");
        assert_eq!(confirmed.confirm.as_deref(), Some("abc"));
        // And nothing is sent that an older server would not expect.
        assert_eq!(
            serde_json::to_string(&body).expect("json"),
            r#"{"state":"parked"}"#
        );
        assert!(serde_json::from_str::<DeclareProjectState>(r#"{"state":"toggle"}"#).is_err());
        assert!(serde_json::from_str::<DeclareProjectState>("{}").is_err());
    }
}
