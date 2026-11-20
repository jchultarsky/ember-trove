//! Merging two saved People into one (S8c, #363; ONTOLOGY §7.1).
//!
//! Email is the identity rule, and a merge is the fallback for what that
//! rule cannot reach: a person with no email, an email that changed, two
//! people who shared an address before the rule. One person is **kept**
//! and one **goes**. This module is the rule for what the kept person
//! holds afterwards; the API runs it under its locks and writes the
//! result (`api/src/repo/person.rs`), and the browser shows the same
//! plan before anything is written.
//!
//! - **Fields** follow [`crate::person_carry::carry_into`]: the kept
//!   person's values stand, blanks are filled, lists grow.
//! - **What the one who goes had and the kept person has otherwise** (a
//!   name, a company, a position) is kept as one line in the kept
//!   person's notes (owner decision, 2026-11-19): the record that held
//!   it is deleted, and a merge has no undo.
//! - **Both on one node:** the kept person's role stands (a blank is
//!   filled), the two notes are joined, and a role that differs is said.
//! - **Nothing is dropped without a word.** A list that would pass its
//!   limit, notes that would, or a joined membership note that would,
//!   refuse the merge: tidy one of the two first.
//!
//! Nothing here decides who may merge whom. Both people are the caller's
//! own, and the API checks that (owner only, no admin bypass).

use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

use crate::id::{MembershipId, NodeId, PersonId};
use crate::person::{
    MAX_MEMBERSHIP_NOTE, MAX_NOTES, Person, PersonInput, normalize, normalize_membership,
};
use crate::person_carry::{CarriedField, CarryReport, carry_into};

/// One of a person's memberships, as the merge reads it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MembershipIn {
    pub id: MembershipId,
    pub node_id: NodeId,
    pub node_title: String,
    pub role: Option<String>,
    pub note: Option<String>,
}

/// A node both people are on: what the kept person's membership there
/// holds after the merge.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct JoinedMembership {
    pub node_id: NodeId,
    pub node_title: String,
    /// The kept person's row, which stays; and the other's, which goes.
    pub kept_row: MembershipId,
    pub gone_row: MembershipId,
    pub role: Option<String>,
    pub note: Option<String>,
    /// The role the one who goes had there, when the kept person has
    /// another: said in the preview, not kept.
    pub role_not_kept: Option<String>,
}

/// Why a merge is refused before anything is written. Each names what to
/// tidy first; none is a matter of rights.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum MergeRefusal {
    /// Together the two have more entries in this list than one person
    /// may hold.
    TooMany { field: CarriedField },
    /// Together their notes are longer than a person's notes may be.
    NotesTooLong,
    /// On this node both have a note, and joined they are longer than a
    /// membership's note may be.
    MembershipNoteTooLong { node_id: NodeId, node_title: String },
    /// The result is not a person the registry accepts, for the reason
    /// given (the registry's own words).
    Invalid { reason: String },
}

impl MergeRefusal {
    /// The refusal as a sentence for the person merging.
    #[must_use]
    pub fn says(&self) -> String {
        match self {
            Self::TooMany { field } => format!(
                "Together they have more than {} {}. Take some off one of them first.",
                crate::person::MAX_CONTACT_POINTS,
                field.noun()
            ),
            Self::NotesTooLong => format!(
                "Together their notes are longer than {MAX_NOTES} characters. Shorten one first."
            ),
            Self::MembershipNoteTooLong { node_title, .. } => format!(
                "Both have a note on \u{201c}{node_title}\u{201d}, and together they are longer than {MAX_MEMBERSHIP_NOTE} characters. Shorten one first."
            ),
            Self::Invalid { reason } => format!("The merged person can't be saved: {reason}."),
        }
    }
}

/// What a merge will do, worked out from the two people and their
/// memberships. Pure: the API adds what only it can read
/// ([`MergePlan`]).
#[derive(Debug, Clone, PartialEq)]
pub struct Planned {
    /// The kept person after the merge, as it will be saved.
    pub result: PersonInput,
    /// What the carry rule added, filled and did not use.
    pub report: CarryReport,
    /// The line added to the kept person's notes for what the one who
    /// goes had otherwise, if anything.
    pub notes_line: Option<String>,
    /// Memberships of the one who goes that simply move to the kept
    /// person.
    pub moved: Vec<MembershipId>,
    /// Nodes both are on.
    pub joined: Vec<JoinedMembership>,
    pub refusals: Vec<MergeRefusal>,
}

/// The plan as the API answers it and the browser shows it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct MergePlan {
    pub kept: PersonId,
    pub gone: PersonId,
    pub kept_name: String,
    pub gone_name: String,
    /// The kept person after the merge.
    pub result: PersonInput,
    pub report: CarryReport,
    #[serde(default)]
    pub notes_line: Option<String>,
    /// How many memberships move to the kept person as they are.
    pub moved: u32,
    #[serde(default)]
    pub joined: Vec<JoinedMembership>,
    /// How many nodes the reader can open mention the one who goes. Their
    /// mentions become mentions of the kept person; no text is changed.
    pub mentioned_in: u32,
    #[serde(default)]
    pub refusals: Vec<MergeRefusal>,
    /// To send back with the merge: it says "this is the plan I was
    /// shown". Not a secret.
    pub token: String,
}

/// `POST /people/{kept}/merge`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct MergeRequest {
    /// The person who goes.
    pub from: PersonId,
    /// The token of the plan the caller was shown.
    pub confirm: String,
}

fn filled(value: Option<&str>) -> Option<&str> {
    value.map(str::trim).filter(|v| !v.is_empty())
}

fn same_text(a: &str, b: &str) -> bool {
    a.trim().to_lowercase() == b.trim().to_lowercase()
}

/// The line that keeps what the one who goes had and the kept person has
/// otherwise: their name, company and position, each only if it differs.
/// `None` when nothing does.
fn notes_line(report: &CarryReport) -> Option<String> {
    let parts: Vec<String> = [
        CarriedField::Name,
        CarriedField::Company,
        CarriedField::Position,
    ]
    .into_iter()
    .filter_map(|field| {
        report
            .not_used
            .iter()
            .find(|n| n.field == field)
            .map(|n| format!("{} \u{201c}{}\u{201d}", field.noun(), n.typed))
    })
    .collect();
    (!parts.is_empty()).then(|| format!("Merged in: {}.", parts.join("; ")))
}

/// `notes` with `line` after them, unless they hold that line already.
fn with_line(notes: Option<String>, line: &str) -> String {
    match notes.filter(|n| !n.trim().is_empty()) {
        None => line.to_string(),
        Some(have) if have.lines().any(|l| l.trim() == line) => have,
        Some(have) => format!("{}\n\n{line}", have.trim_end()),
    }
}

/// The membership the kept person has on a node both are on.
fn join_membership(
    kept: &MembershipIn,
    gone: &MembershipIn,
) -> Result<JoinedMembership, MergeRefusal> {
    let kept_role = filled(kept.role.as_deref());
    let gone_role = filled(gone.role.as_deref());
    let role = kept_role.or(gone_role).map(str::to_string);
    let role_not_kept = match (kept_role, gone_role) {
        (Some(k), Some(g)) if !same_text(k, g) => Some(g.to_string()),
        _ => None,
    };
    let note = match (filled(kept.note.as_deref()), filled(gone.note.as_deref())) {
        (Some(k), Some(g)) if !same_text(k, g) => Some(format!("{k}\n\n{g}")),
        (Some(k), _) => Some(k.to_string()),
        (None, g) => g.map(str::to_string),
    };
    let too_long = || MergeRefusal::MembershipNoteTooLong {
        node_id: kept.node_id,
        node_title: kept.node_title.clone(),
    };
    let (role, note) = normalize_membership(role, note).map_err(|_| too_long())?;
    Ok(JoinedMembership {
        node_id: kept.node_id,
        node_title: kept.node_title.clone(),
        kept_row: kept.id,
        gone_row: gone.id,
        role,
        note,
        role_not_kept,
    })
}

/// What merging `gone` into `kept` will do.
///
/// - `third_party`: addresses of the one who goes that **another** of the
///   account's people lists too (an old pair). They are left out and
///   said: taking them would make a new pair, which §7.1 forbids.
/// - `kept_on` / `gone_on`: each one's memberships.
#[must_use]
pub fn plan(
    kept: &Person,
    gone: &Person,
    third_party: &[String],
    kept_on: &[MembershipIn],
    gone_on: &[MembershipIn],
) -> Planned {
    let carried = carry_into(
        &PersonInput::from(kept),
        &PersonInput::from(gone),
        third_party,
    );
    let mut person = carried.person;
    let report = carried.report;
    let mut refusals: Vec<MergeRefusal> = report
        .over
        .iter()
        .map(|field| match field {
            CarriedField::Notes => MergeRefusal::NotesTooLong,
            field => MergeRefusal::TooMany { field: *field },
        })
        .collect();

    let notes_line = notes_line(&report);
    if let Some(line) = &notes_line {
        let notes = with_line(person.notes.take(), line);
        if notes.chars().count() > MAX_NOTES && !refusals.contains(&MergeRefusal::NotesTooLong) {
            refusals.push(MergeRefusal::NotesTooLong);
        }
        person.notes = Some(notes);
    }
    // What is saved is what the registry's own check makes of it. A
    // refusal already said is not said twice in other words.
    let result = match normalize(person.clone()) {
        Ok(tidy) => tidy,
        Err(e) => {
            if refusals.is_empty() {
                refusals.push(MergeRefusal::Invalid {
                    reason: e.to_string(),
                });
            }
            person
        }
    };

    let mut moved = Vec::new();
    let mut joined = Vec::new();
    for theirs in gone_on {
        match kept_on.iter().find(|k| k.node_id == theirs.node_id) {
            None => moved.push(theirs.id),
            Some(kept_row) => match join_membership(kept_row, theirs) {
                Ok(join) => joined.push(join),
                Err(refusal) => refusals.push(refusal),
            },
        }
    }
    Planned {
        result,
        report,
        notes_line,
        moved,
        joined,
        refusals,
    }
}

/// What a plan's token is made of: both people as they were read, and
/// every membership the merge touches. The API hashes it (`common` is
/// also the browser's code and carries no hashing crate).
///
/// Not in it: how many texts mention the one who goes. Other people's
/// saves change that, and it changes nothing the merge writes.
#[must_use]
pub fn fingerprint(
    kept: &Person,
    gone: &Person,
    third_party: &[String],
    kept_on: &[MembershipIn],
    gone_on: &[MembershipIn],
) -> String {
    let row = |m: &MembershipIn| format!("m:{}:{}:{:?}:{:?}", m.id, m.node_id, m.role, m.note);
    let mut rows: Vec<String> = kept_on.iter().chain(gone_on).map(row).collect();
    rows.sort();
    let mut left_out: Vec<String> = third_party.iter().map(|a| format!("x:{a}")).collect();
    left_out.sort();
    let mut lines = vec![
        format!("kept:{}:{}", kept.id, kept.updated_at.timestamp_micros()),
        format!("gone:{}:{}", gone.id, gone.updated_at.timestamp_micros()),
    ];
    lines.extend(rows);
    lines.extend(left_out);
    lines.join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::person::{ContactPoint, MAX_CONTACT_POINTS};
    use chrono::{DateTime, TimeZone, Utc};
    use uuid::Uuid;

    fn at(seconds: i64) -> DateTime<Utc> {
        Utc.timestamp_opt(seconds, 0).single().unwrap()
    }

    fn point(value: &str, label: &str, preferred: bool) -> ContactPoint {
        ContactPoint {
            label: label.to_string(),
            value: value.to_string(),
            preferred,
        }
    }

    fn person(n: u128, name: &str) -> Person {
        Person {
            id: PersonId(Uuid::from_u128(n)),
            owner_id: "me".to_string(),
            display_name: name.to_string(),
            company: None,
            position: None,
            emails: Vec::new(),
            phones: Vec::new(),
            addresses: Vec::new(),
            notes: None,
            created_at: at(0),
            updated_at: at(100),
        }
    }

    fn on(row: u128, node: u128, role: Option<&str>, note: Option<&str>) -> MembershipIn {
        MembershipIn {
            id: MembershipId(Uuid::from_u128(row)),
            node_id: NodeId(Uuid::from_u128(node)),
            node_title: format!("Project {node}"),
            role: role.map(str::to_string),
            note: note.map(str::to_string),
        }
    }

    #[test]
    fn the_kept_persons_values_stand_blanks_are_filled_and_lists_grow() {
        let mut kept = person(1, "Ada Lovelace");
        kept.emails = vec![point("ada@example.com", "work", true)];
        kept.notes = Some("Met at the fair.".to_string());
        let mut gone = person(2, "ada lovelace");
        gone.company = Some("Analytical Engines".to_string());
        gone.emails = vec![
            point("ADA@example.com", "home", true),
            point("countess@example.com", "", true),
        ];
        gone.phones = vec![point("+1 555 0100", "cell", true)];
        gone.notes = Some("Met at the fair.\nLikes punch cards.".to_string());

        let planned = plan(&kept, &gone, &[], &[], &[]);
        assert_eq!(planned.refusals, []);
        let result = &planned.result;
        // As the carry rule gives it, to the letter.
        let carried = carry_into(&PersonInput::from(&kept), &PersonInput::from(&gone), &[]);
        assert_eq!(*result, normalize(carried.person).unwrap());
        assert_eq!(result.display_name, "Ada Lovelace");
        assert_eq!(result.company.as_deref(), Some("Analytical Engines"));
        // Their own entry stays as it is; the new one is not preferred
        // over it. A list they had none of takes the other's choice.
        assert_eq!(
            result.emails,
            [
                point("ada@example.com", "work", true),
                point("countess@example.com", "", false)
            ]
        );
        assert_eq!(result.phones, [point("+1 555 0100", "cell", true)]);
        assert_eq!(
            result.notes.as_deref(),
            Some("Met at the fair.\n\nLikes punch cards.")
        );
        // The same name in another case is the same name: no line.
        assert_eq!(planned.notes_line, None);
    }

    /// Owner decision, 2026-11-19: the record that goes is deleted, so
    /// what it held and the kept person holds otherwise is kept in words.
    #[test]
    fn what_the_one_who_goes_had_otherwise_is_one_line_in_the_notes() {
        let mut kept = person(1, "Ada King");
        kept.company = Some("Lovelace Ltd".to_string());
        kept.notes = Some("First line.\n".to_string());
        let mut gone = person(2, "Ada Byron");
        gone.company = Some("Analytical Engines".to_string());
        gone.position = Some("Countess".to_string());

        let planned = plan(&kept, &gone, &[], &[], &[]);
        assert_eq!(planned.refusals, []);
        // The position was blank and is filled; the name and the company
        // differ and are kept in the line, in that order.
        assert_eq!(planned.result.position.as_deref(), Some("Countess"));
        let line = "Merged in: name \u{201c}Ada Byron\u{201d}; company \u{201c}Analytical Engines\u{201d}.";
        assert_eq!(planned.notes_line.as_deref(), Some(line));
        assert_eq!(
            planned.result.notes.as_deref(),
            Some(format!("First line.\n\n{line}").as_str())
        );
        // With no notes of their own, the line is the notes.
        kept.notes = None;
        let planned = plan(&kept, &gone, &[], &[], &[]);
        assert_eq!(planned.result.notes.as_deref(), Some(line));
        // Planned again over its own result (a merge refused late and
        // asked for again), the line is not added twice.
        kept.notes = Some(line.to_string());
        let planned = plan(&kept, &gone, &[], &[], &[]);
        assert_eq!(planned.result.notes.as_deref(), Some(line));
    }

    #[test]
    fn a_merge_that_would_pass_a_limit_is_refused_and_says_which() {
        let mut kept = person(1, "Ada");
        let mut gone = person(2, "Ada");
        for n in 0..MAX_CONTACT_POINTS {
            kept.phones.push(point(&format!("555 01{n:02}"), "", false));
        }
        gone.phones = vec![point("555 0999", "", false)];
        let planned = plan(&kept, &gone, &[], &[], &[]);
        assert_eq!(
            planned.refusals,
            [MergeRefusal::TooMany {
                field: CarriedField::Phones
            }]
        );
        assert!(planned.refusals[0].says().contains("more than 10 phones"));

        // Notes: theirs and the other's lines together.
        let mut kept = person(1, "Ada");
        let mut gone = person(2, "Ada");
        kept.notes = Some("a".repeat(MAX_NOTES - 5));
        gone.notes = Some("a line of their own".to_string());
        assert_eq!(
            plan(&kept, &gone, &[], &[], &[]).refusals,
            [MergeRefusal::NotesTooLong]
        );
        // Or theirs and the line for what differs.
        let mut gone = person(2, "Someone Else Entirely");
        gone.notes = None;
        kept.notes = Some("a".repeat(MAX_NOTES - 5));
        assert_eq!(
            plan(&kept, &gone, &[], &[], &[]).refusals,
            [MergeRefusal::NotesTooLong]
        );
    }

    /// §7.1: an address that a third person lists too is not taken. It
    /// would make a new pair of people with one address.
    #[test]
    fn an_address_a_third_person_lists_is_left_out_and_said() {
        let kept = person(1, "Ada");
        let mut gone = person(2, "Ada");
        gone.emails = vec![
            point("shared@example.com", "", false),
            point("own@example.com", "", false),
        ];
        let planned = plan(&kept, &gone, &["Shared@Example.com".to_string()], &[], &[]);
        assert_eq!(planned.refusals, []);
        assert_eq!(planned.result.emails, [point("own@example.com", "", false)]);
        assert_eq!(planned.report.someone_elses, ["shared@example.com"]);
    }

    #[test]
    fn memberships_move_and_where_both_are_on_a_node_they_are_joined() {
        let kept = person(1, "Ada");
        let gone = person(2, "Ada");
        let kept_on = [
            on(10, 100, Some("Lead"), Some("Since March.")),
            on(11, 101, None, None),
            on(12, 102, Some("Advisor"), Some("Same words.")),
        ];
        let gone_on = [
            on(20, 100, Some("Reviewer"), Some("Signs off.")),
            on(21, 101, Some("Sponsor"), Some("Pays.")),
            on(22, 102, Some("advisor"), Some("same words. ")),
            on(23, 103, Some("Guest"), None),
        ];
        let planned = plan(&kept, &gone, &[], &kept_on, &gone_on);
        assert_eq!(planned.refusals, []);
        // Only theirs on a node the kept person is not on moves as it is.
        assert_eq!(planned.moved, [MembershipId(Uuid::from_u128(23))]);
        let joined: Vec<_> = planned
            .joined
            .iter()
            .map(|j| {
                (
                    j.kept_row.0.as_u128(),
                    j.gone_row.0.as_u128(),
                    j.role.as_deref(),
                    j.note.as_deref(),
                    j.role_not_kept.as_deref(),
                )
            })
            .collect();
        assert_eq!(
            joined,
            [
                // The kept person's role stands; the other's is said. The
                // notes are joined, theirs first.
                (
                    10,
                    20,
                    Some("Lead"),
                    Some("Since March.\n\nSigns off."),
                    Some("Reviewer")
                ),
                // A blank role and a blank note are filled.
                (11, 21, Some("Sponsor"), Some("Pays."), None),
                // The same role and the same note, however typed, are one.
                (12, 22, Some("Advisor"), Some("Same words."), None),
            ]
        );
        assert_eq!(planned.joined[0].node_title, "Project 100");
    }

    #[test]
    fn two_notes_on_one_node_that_are_too_long_together_refuse_the_merge() {
        let kept = person(1, "Ada");
        let gone = person(2, "Ada");
        let long = "n".repeat(MAX_MEMBERSHIP_NOTE - 10);
        let kept_on = [on(10, 100, None, Some(&long))];
        let gone_on = [on(20, 100, None, Some("twenty characters!!!"))];
        let planned = plan(&kept, &gone, &[], &kept_on, &gone_on);
        assert_eq!(
            planned.refusals,
            [MergeRefusal::MembershipNoteTooLong {
                node_id: NodeId(Uuid::from_u128(100)),
                node_title: "Project 100".to_string()
            }]
        );
        assert!(planned.joined.is_empty());
        assert!(
            planned.refusals[0]
                .says()
                .contains("\u{201c}Project 100\u{201d}")
        );
    }

    /// The token says "this is the plan I was shown": it changes when
    /// either person or any membership the merge touches does, and not
    /// with how many texts mention the one who goes (that is not in it).
    #[test]
    fn the_fingerprint_changes_with_what_the_merge_reads() {
        let kept = person(1, "Ada");
        let gone = person(2, "Ada");
        let kept_on = [on(10, 100, Some("Lead"), None)];
        let gone_on = [on(20, 101, None, None)];
        let base = fingerprint(&kept, &gone, &[], &kept_on, &gone_on);
        assert_eq!(base, fingerprint(&kept, &gone, &[], &kept_on, &gone_on));

        let mut edited = kept.clone();
        edited.updated_at = at(101);
        assert_ne!(base, fingerprint(&edited, &gone, &[], &kept_on, &gone_on));
        let mut edited = gone.clone();
        edited.updated_at = at(101);
        assert_ne!(base, fingerprint(&kept, &edited, &[], &kept_on, &gone_on));
        // Which is kept and which goes.
        assert_ne!(base, fingerprint(&gone, &kept, &[], &gone_on, &kept_on));
        // A membership's role, a membership more, a membership fewer.
        let other = [on(10, 100, Some("Advisor"), None)];
        assert_ne!(base, fingerprint(&kept, &gone, &[], &other, &gone_on));
        let more = [on(20, 101, None, None), on(21, 102, None, None)];
        assert_ne!(base, fingerprint(&kept, &gone, &[], &kept_on, &more));
        assert_ne!(base, fingerprint(&kept, &gone, &[], &kept_on, &[]));
        // An address newly listed by a third person.
        assert_ne!(
            base,
            fingerprint(
                &kept,
                &gone,
                &["x@example.com".to_string()],
                &kept_on,
                &gone_on
            )
        );
        // In whatever order the rows were read.
        let two = [on(20, 101, None, None), on(21, 102, None, None)];
        let owt = [on(21, 102, None, None), on(20, 101, None, None)];
        assert_eq!(
            fingerprint(&kept, &gone, &[], &kept_on, &two),
            fingerprint(&kept, &gone, &[], &kept_on, &owt)
        );
    }

    #[test]
    fn a_refusal_reads_as_a_sentence_and_goes_over_the_wire() {
        for refusal in [
            MergeRefusal::TooMany {
                field: CarriedField::Emails,
            },
            MergeRefusal::NotesTooLong,
            MergeRefusal::MembershipNoteTooLong {
                node_id: NodeId(Uuid::from_u128(7)),
                node_title: "Plan".to_string(),
            },
            MergeRefusal::Invalid {
                reason: "a name is required".to_string(),
            },
        ] {
            let says = refusal.says();
            assert!(says.ends_with('.') && !says.contains("  "), "{says}");
            let wire = serde_json::to_string(&refusal).unwrap();
            assert_eq!(
                serde_json::from_str::<MergeRefusal>(&wire).unwrap(),
                refusal
            );
        }
    }
}
