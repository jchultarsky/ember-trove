//! People (#147): a registry of humans, and memberships that place a person
//! in a node's context ("sponsor" on a project).
//!
//! A person is an entity, not a node: nodes are documents, and a person is
//! someone documents refer to. Fields are a vCard subset (FN, ORG, TITLE,
//! EMAIL, TEL, ADR as freeform text, NOTE), so later import/export is a
//! mapping rather than a redesign.
//!
//! **Privacy:** these are third parties' personal details, and they never
//! signed up. A person is visible only to its owner, including the admin
//! (there's no admin bypass). A node sharee sees a membership's name and
//! role only ([`MembershipView::redacted`]). Public share links were removed
//! in #147, before this existed.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

use crate::id::{MembershipId, NodeId, PersonId};
use crate::node::NodeType;

/// Field limits, in characters. Mirrored by the CHECKs in migration 037.
pub const MAX_NAME: usize = 200;
pub const MAX_COMPANY: usize = 200;
pub const MAX_POSITION: usize = 200;
pub const MAX_LABEL: usize = 30;
pub const MAX_EMAIL: usize = 254;
pub const MAX_PHONE: usize = 50;
pub const MAX_ADDRESS: usize = 1000;
pub const MAX_NOTES: usize = 10_000;
pub const MAX_ROLE: usize = 100;
pub const MAX_MEMBERSHIP_NOTE: usize = 2000;
/// At most this many emails, and this many phones, per person.
pub const MAX_CONTACT_POINTS: usize = 10;

/// One email address or phone number with an open label ("work", "cell").
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct ContactPoint {
    #[serde(default)]
    pub label: String,
    pub value: String,
    /// At most one per list.
    #[serde(default)]
    pub preferred: bool,
}

/// A person in the registry. Only its owner ever receives this.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct Person {
    pub id: PersonId,
    pub owner_id: String,
    pub display_name: String,
    pub company: Option<String>,
    pub position: Option<String>,
    pub emails: Vec<ContactPoint>,
    pub phones: Vec<ContactPoint>,
    pub address_label: Option<String>,
    pub address: Option<String>,
    pub notes: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// Create a person, or replace all of its fields (PUT). Only the name is
/// required: cards start blank and get filled in as needed.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct PersonInput {
    pub display_name: String,
    #[serde(default)]
    pub company: Option<String>,
    #[serde(default)]
    pub position: Option<String>,
    #[serde(default)]
    pub emails: Vec<ContactPoint>,
    #[serde(default)]
    pub phones: Vec<ContactPoint>,
    #[serde(default)]
    pub address_label: Option<String>,
    #[serde(default)]
    pub address: Option<String>,
    #[serde(default)]
    pub notes: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PersonError {
    /// The name is blank.
    NameRequired,
    /// `field` is longer than `max` characters.
    TooLong { field: &'static str, max: usize },
    /// More than [`MAX_CONTACT_POINTS`] entries in `field`.
    TooMany { field: &'static str },
    /// More than one preferred entry in `field`.
    MultiplePreferred { field: &'static str },
    /// An entry in `field` has a blank value.
    EmptyValue { field: &'static str },
    /// Not shaped like an email address.
    InvalidEmail,
    /// Contains characters a phone number doesn't.
    InvalidPhone,
    /// A one-line field contains a line break or control character.
    ControlChar { field: &'static str },
}

impl std::fmt::Display for PersonError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            PersonError::NameRequired => write!(f, "a name is required"),
            PersonError::TooLong { field, max } => {
                write!(f, "{field} must be at most {max} characters")
            }
            PersonError::TooMany { field } => {
                write!(f, "at most {MAX_CONTACT_POINTS} {field} per person")
            }
            PersonError::MultiplePreferred { field } => {
                write!(f, "only one of the {field} can be preferred")
            }
            PersonError::EmptyValue { field } => write!(f, "an entry in {field} is blank"),
            PersonError::InvalidEmail => write!(f, "that doesn't look like an email address"),
            PersonError::InvalidPhone => write!(f, "that doesn't look like a phone number"),
            PersonError::ControlChar { field } => write!(f, "{field} must be a single line"),
        }
    }
}

/// Validate and tidy a person's fields: trim everything, blank optional
/// fields become `None`, and contact entries with neither label nor value
/// are dropped (an untouched "add email" row).
pub fn normalize(input: PersonInput) -> Result<PersonInput, PersonError> {
    let display_name = input.display_name.trim().to_string();
    if display_name.is_empty() {
        return Err(PersonError::NameRequired);
    }
    check_line("name", &display_name, MAX_NAME)?;
    Ok(PersonInput {
        display_name,
        company: optional_line("company", input.company, MAX_COMPANY)?,
        position: optional_line("position", input.position, MAX_POSITION)?,
        emails: contact_points("emails", input.emails, MAX_EMAIL, valid_email)?,
        phones: contact_points("phones", input.phones, MAX_PHONE, valid_phone)?,
        address_label: optional_line("address label", input.address_label, MAX_LABEL)?,
        address: optional_text("address", input.address, MAX_ADDRESS)?,
        notes: optional_text("notes", input.notes, MAX_NOTES)?,
    })
}

fn too_long(field: &'static str, value: &str, max: usize) -> Result<(), PersonError> {
    if value.chars().count() > max {
        return Err(PersonError::TooLong { field, max });
    }
    Ok(())
}

/// A one-line field: no line breaks or other control characters.
fn check_line(field: &'static str, value: &str, max: usize) -> Result<(), PersonError> {
    if value.chars().any(char::is_control) {
        return Err(PersonError::ControlChar { field });
    }
    too_long(field, value, max)
}

fn optional_line(
    field: &'static str,
    value: Option<String>,
    max: usize,
) -> Result<Option<String>, PersonError> {
    match value
        .map(|v| v.trim().to_string())
        .filter(|v| !v.is_empty())
    {
        Some(v) => check_line(field, &v, max).map(|()| Some(v)),
        None => Ok(None),
    }
}

/// A multi-line field (address, notes): line breaks are fine.
fn optional_text(
    field: &'static str,
    value: Option<String>,
    max: usize,
) -> Result<Option<String>, PersonError> {
    match value
        .map(|v| v.trim().to_string())
        .filter(|v| !v.is_empty())
    {
        Some(v) => too_long(field, &v, max).map(|()| Some(v)),
        None => Ok(None),
    }
}

fn contact_points(
    field: &'static str,
    points: Vec<ContactPoint>,
    max_value: usize,
    valid: fn(&str) -> bool,
) -> Result<Vec<ContactPoint>, PersonError> {
    let mut out = Vec::new();
    for p in points {
        let label = p.label.trim().to_string();
        let value = p.value.trim().to_string();
        if label.is_empty() && value.is_empty() {
            continue; // an untouched "add" row
        }
        if value.is_empty() {
            return Err(PersonError::EmptyValue { field });
        }
        check_line(field, &label, MAX_LABEL)?;
        check_line(field, &value, max_value)?;
        if !valid(&value) {
            return Err(if field == "emails" {
                PersonError::InvalidEmail
            } else {
                PersonError::InvalidPhone
            });
        }
        out.push(ContactPoint {
            label,
            value,
            preferred: p.preferred,
        });
    }
    if out.len() > MAX_CONTACT_POINTS {
        return Err(PersonError::TooMany { field });
    }
    if out.iter().filter(|p| p.preferred).count() > 1 {
        return Err(PersonError::MultiplePreferred { field });
    }
    Ok(out)
}

/// A deliberately loose shape check: one `@`, something on both sides, a
/// dot in the domain, no whitespace. Real validation is the mail server's.
fn valid_email(v: &str) -> bool {
    let mut parts = v.split('@');
    match (parts.next(), parts.next(), parts.next()) {
        (Some(local), Some(domain), None) => {
            !local.is_empty()
                && domain.contains('.')
                && !domain.starts_with('.')
                && !domain.ends_with('.')
                && !v.chars().any(char::is_whitespace)
        }
        _ => false,
    }
}

/// Digits plus the punctuation phone numbers use, and an extension marker
/// ("ext.", "x"). It needs at least one digit.
fn valid_phone(v: &str) -> bool {
    let lower = v.to_lowercase();
    let rest = lower.replace("ext.", "").replace("ext", "");
    rest.chars().any(|c| c.is_ascii_digit())
        && rest
            .chars()
            .all(|c| c.is_ascii_digit() || " +-().x/".contains(c))
}

/// A membership: a person in a node's context.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct MembershipInput {
    pub person_id: PersonId,
    #[serde(default)]
    pub role: Option<String>,
    #[serde(default)]
    pub note: Option<String>,
}

/// Change a membership's role or note (the person stays the same).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct MembershipUpdate {
    #[serde(default)]
    pub role: Option<String>,
    #[serde(default)]
    pub note: Option<String>,
}

/// Validate a membership's role and note; blank means none.
pub fn normalize_membership(
    role: Option<String>,
    note: Option<String>,
) -> Result<(Option<String>, Option<String>), PersonError> {
    Ok((
        optional_line("role", role, MAX_ROLE)?,
        optional_text("note", note, MAX_MEMBERSHIP_NOTE)?,
    ))
}

/// A membership as stored (repo layer).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct Membership {
    pub id: MembershipId,
    pub node_id: NodeId,
    pub person: Person,
    pub role: Option<String>,
    pub note: Option<String>,
    pub created_at: DateTime<Utc>,
}

/// A membership row as backups store it (people are a separate list).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct MembershipRecord {
    pub id: MembershipId,
    pub node_id: NodeId,
    pub person_id: PersonId,
    pub role: Option<String>,
    pub note: Option<String>,
    pub created_at: DateTime<Utc>,
}

/// A membership as the node page receives it. For the person's owner it
/// carries the contact card and the note. Anyone else who can see the node
/// (a sharee, or the admin) gets only the name and the role.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct MembershipView {
    pub id: MembershipId,
    pub person_id: PersonId,
    pub display_name: String,
    pub role: Option<String>,
    /// Owner only.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
    /// Owner only: company, position, emails, phones, address, notes.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub person: Option<Person>,
}

impl MembershipView {
    /// What `viewer` may see of `m`: everything if they own the person,
    /// otherwise the name and the role.
    pub fn for_viewer(m: Membership, viewer: &str) -> Self {
        if m.person.owner_id != viewer {
            return Self::redacted(m);
        }
        Self {
            id: m.id,
            person_id: m.person.id,
            display_name: m.person.display_name.clone(),
            role: m.role,
            note: m.note,
            person: Some(m.person),
        }
    }

    /// Name and role only.
    pub fn redacted(m: Membership) -> Self {
        Self {
            id: m.id,
            person_id: m.person.id,
            display_name: m.person.display_name,
            role: m.role,
            note: None,
            person: None,
        }
    }
}

/// "Used in": one node a person appears in, with their role there.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct PersonUsage {
    pub membership_id: MembershipId,
    pub node_id: NodeId,
    pub node_title: String,
    pub node_type: NodeType,
    pub role: Option<String>,
}

/// A role the owner already uses, with how many memberships use it (the
/// role field's suggestions; the same discipline as project statuses).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct RoleCount {
    pub role: String,
    pub count: u32,
}

/// `409` body when a new person's email matches someone already in the
/// registry: a duplicate warning, never an identity rule. Retry with
/// `?confirm=true` to create anyway.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct DuplicateWarning {
    pub matches: Vec<DuplicateMatch>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct DuplicateMatch {
    pub id: PersonId,
    pub display_name: String,
    pub email: String,
}

/// The lowercased email values of `input`, for the duplicate check.
pub fn email_keys(input: &PersonInput) -> Vec<String> {
    input
        .emails
        .iter()
        .map(|e| e.value.to_lowercase())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cp(label: &str, value: &str, preferred: bool) -> ContactPoint {
        ContactPoint {
            label: label.into(),
            value: value.into(),
            preferred,
        }
    }

    fn named(name: &str) -> PersonInput {
        PersonInput {
            display_name: name.into(),
            ..Default::default()
        }
    }

    #[test]
    fn a_name_alone_is_a_valid_person_and_everything_is_trimmed() {
        let p = normalize(PersonInput {
            display_name: "  Ada Lovelace ".into(),
            company: Some("  ".into()),
            position: Some(" Analyst ".into()),
            notes: Some("\n".into()),
            ..Default::default()
        })
        .unwrap();
        assert_eq!(p.display_name, "Ada Lovelace");
        assert_eq!(p.company, None);
        assert_eq!(p.position.as_deref(), Some("Analyst"));
        assert_eq!(p.notes, None);
    }

    #[test]
    fn the_name_is_required_and_bounded() {
        assert_eq!(normalize(named("   ")), Err(PersonError::NameRequired));
        assert_eq!(
            normalize(named(&"x".repeat(MAX_NAME + 1))),
            Err(PersonError::TooLong {
                field: "name",
                max: MAX_NAME
            })
        );
        assert!(normalize(named(&"é".repeat(MAX_NAME))).is_ok());
        assert_eq!(
            normalize(named("Ada\nLovelace")),
            Err(PersonError::ControlChar { field: "name" })
        );
    }

    #[test]
    fn untouched_contact_rows_are_dropped_and_values_trimmed() {
        let p = normalize(PersonInput {
            emails: vec![cp("", "", false), cp(" work ", " ada@example.com ", true)],
            phones: vec![
                cp("cell", " +1 (555) 010-0000 ", false),
                cp("  ", " ", false),
            ],
            ..named("Ada")
        })
        .unwrap();
        assert_eq!(p.emails, vec![cp("work", "ada@example.com", true)]);
        assert_eq!(p.phones, vec![cp("cell", "+1 (555) 010-0000", false)]);
    }

    #[test]
    fn a_labelled_entry_without_a_value_is_an_error() {
        let p = normalize(PersonInput {
            emails: vec![cp("work", "  ", false)],
            ..named("Ada")
        });
        assert_eq!(p, Err(PersonError::EmptyValue { field: "emails" }));
    }

    #[test]
    fn only_one_preferred_per_list() {
        let p = normalize(PersonInput {
            phones: vec![cp("a", "1", true), cp("b", "2", true)],
            ..named("Ada")
        });
        assert_eq!(p, Err(PersonError::MultiplePreferred { field: "phones" }));
    }

    #[test]
    fn contact_lists_are_capped() {
        let many: Vec<_> = (0..=MAX_CONTACT_POINTS)
            .map(|i| cp("", &format!("a{i}@example.com"), false))
            .collect();
        let p = normalize(PersonInput {
            emails: many,
            ..named("Ada")
        });
        assert_eq!(p, Err(PersonError::TooMany { field: "emails" }));
    }

    #[test]
    fn emails_and_phones_are_shape_checked() {
        for bad in [
            "ada",
            "ada@",
            "@example.com",
            "a da@example.com",
            "ada@@example.com",
        ] {
            let p = normalize(PersonInput {
                emails: vec![cp("", bad, false)],
                ..named("Ada")
            });
            assert_eq!(p, Err(PersonError::InvalidEmail), "{bad}");
        }
        for bad in ["call me", "555-0100; drop table", "<script>"] {
            let p = normalize(PersonInput {
                phones: vec![cp("", bad, false)],
                ..named("Ada")
            });
            assert_eq!(p, Err(PersonError::InvalidPhone), "{bad}");
        }
        assert!(
            normalize(PersonInput {
                phones: vec![cp("", "+44 20 7946 0958 ext. 12", false)],
                ..named("Ada")
            })
            .is_ok()
        );
    }

    #[test]
    fn multi_line_fields_keep_their_lines_but_are_bounded() {
        let p = normalize(PersonInput {
            address: Some("1 Main St\nSpringfield".into()),
            notes: Some("Prefers email.\nMet at the conference.".into()),
            ..named("Ada")
        })
        .unwrap();
        assert_eq!(p.address.as_deref(), Some("1 Main St\nSpringfield"));
        let long = normalize(PersonInput {
            notes: Some("x".repeat(MAX_NOTES + 1)),
            ..named("Ada")
        });
        assert_eq!(
            long,
            Err(PersonError::TooLong {
                field: "notes",
                max: MAX_NOTES
            })
        );
    }

    #[test]
    fn a_membership_role_and_note_are_trimmed_and_bounded() {
        assert_eq!(
            normalize_membership(Some(" Sponsor ".into()), Some("  ".into())),
            Ok((Some("Sponsor".into()), None))
        );
        assert_eq!(
            normalize_membership(Some("x".repeat(MAX_ROLE + 1)), None),
            Err(PersonError::TooLong {
                field: "role",
                max: MAX_ROLE
            })
        );
        assert_eq!(
            normalize_membership(Some("a\nb".into()), None),
            Err(PersonError::ControlChar { field: "role" })
        );
    }

    #[test]
    fn email_keys_are_lowercased_for_the_duplicate_check() {
        let p = PersonInput {
            emails: vec![cp("", "Ada@Example.COM", false)],
            ..named("Ada")
        };
        assert_eq!(email_keys(&p), vec!["ada@example.com".to_string()]);
    }

    fn membership(owner: &str) -> Membership {
        let now = Utc::now();
        Membership {
            id: MembershipId::new(),
            node_id: NodeId::new(),
            person: Person {
                id: PersonId::new(),
                owner_id: owner.into(),
                display_name: "Ada".into(),
                company: Some("Analytical Engines".into()),
                position: None,
                emails: vec![cp("work", "ada@example.com", true)],
                phones: vec![],
                address_label: None,
                address: None,
                notes: Some("private".into()),
                created_at: now,
                updated_at: now,
            },
            role: Some("Sponsor".into()),
            note: Some("met at kickoff".into()),
            created_at: now,
        }
    }

    #[test]
    fn the_persons_owner_sees_the_card_and_the_note() {
        let v = MembershipView::for_viewer(membership("owner"), "owner");
        assert_eq!(v.note.as_deref(), Some("met at kickoff"));
        assert_eq!(v.person.map(|p| p.emails.len()), Some(1));
    }

    #[test]
    fn anyone_else_sees_only_name_and_role() {
        let v = MembershipView::for_viewer(membership("owner"), "a-sharee");
        assert_eq!(
            (v.display_name.as_str(), v.role.as_deref()),
            ("Ada", Some("Sponsor"))
        );
        assert_eq!((v.note, v.person), (None, None));
        // And the serialized form carries no contact fields at all.
        let json = serde_json::to_string(&MembershipView::redacted(membership("o"))).unwrap();
        assert!(
            !json.contains("example.com") && !json.contains("private"),
            "{json}"
        );
    }
}
