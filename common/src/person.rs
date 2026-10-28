//! People (#147): a registry of humans, and memberships that place a person
//! in a node's context ("sponsor" on a project).
//!
//! A person is an entity, not a node: nodes are documents, and a person is
//! someone documents refer to. Fields are a vCard subset (FN, ORG, TITLE,
//! EMAIL, TEL, ADR as freeform text, NOTE), so later import/export is a
//! mapping rather than a redesign. All three channels are lists of labelled
//! values with at most one preferred, like vCard's TYPE and PREF (#155).
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
/// At most this many emails, phones, and addresses (each) per person.
pub const MAX_CONTACT_POINTS: usize = 10;

/// One email address, phone number or postal address with an open label
/// ("work", "cell", "home"). An address value may span lines.
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
///
/// Deserializes through [`PersonWire`] so backups written before #155 (one
/// `address_label` + `address`) restore their address instead of losing it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, ToSchema)]
#[serde(from = "PersonWire")]
pub struct Person {
    pub id: PersonId,
    pub owner_id: String,
    pub display_name: String,
    pub company: Option<String>,
    pub position: Option<String>,
    pub emails: Vec<ContactPoint>,
    pub phones: Vec<ContactPoint>,
    pub addresses: Vec<ContactPoint>,
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
    pub addresses: Vec<ContactPoint>,
    #[serde(default)]
    pub notes: Option<String>,
}

/// The single address of the pre-#155 shape as a list entry, if it has a
/// value. It's preferred: it was the person's only address.
fn legacy_address(label: Option<String>, value: Option<String>) -> Option<ContactPoint> {
    let value = value.filter(|v| !v.trim().is_empty())?;
    Some(ContactPoint {
        label: label.unwrap_or_default(),
        value,
        preferred: true,
    })
}

/// [`Person`] as it may arrive: the current shape, or a pre-#155 backup's.
#[derive(Deserialize)]
struct PersonWire {
    id: PersonId,
    owner_id: String,
    display_name: String,
    company: Option<String>,
    position: Option<String>,
    emails: Vec<ContactPoint>,
    phones: Vec<ContactPoint>,
    #[serde(default)]
    addresses: Vec<ContactPoint>,
    #[serde(default)]
    address_label: Option<String>,
    #[serde(default)]
    address: Option<String>,
    notes: Option<String>,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
}

impl From<PersonWire> for Person {
    fn from(w: PersonWire) -> Self {
        let mut addresses = w.addresses;
        if addresses.is_empty() {
            addresses.extend(legacy_address(w.address_label, w.address));
        }
        Person {
            id: w.id,
            owner_id: w.owner_id,
            display_name: w.display_name,
            company: w.company,
            position: w.position,
            emails: w.emails,
            phones: w.phones,
            addresses,
            notes: w.notes,
            created_at: w.created_at,
            updated_at: w.updated_at,
        }
    }
}

/// The body `POST /people` and `PUT /people/{id}` accept: a [`PersonInput`],
/// or the pre-#155 shape a browser tab opened before the upgrade still sends
/// (`address_label` + `address`, no `addresses`).
///
/// Such a client never saw the address list, so a PUT from it must not wipe
/// the list: when `addresses` is absent, [`PersonBody::resolve`] keeps the
/// stored entries and adds the old-style address if it's a new one.
#[derive(Debug, Clone, Default, PartialEq, Deserialize, ToSchema)]
pub struct PersonBody {
    pub display_name: String,
    #[serde(default)]
    pub company: Option<String>,
    #[serde(default)]
    pub position: Option<String>,
    #[serde(default)]
    pub emails: Vec<ContactPoint>,
    #[serde(default)]
    pub phones: Vec<ContactPoint>,
    /// Absent only from pre-#155 clients.
    #[serde(default)]
    pub addresses: Option<Vec<ContactPoint>>,
    /// Pre-#155 clients only.
    #[serde(default)]
    pub address_label: Option<String>,
    /// Pre-#155 clients only.
    #[serde(default)]
    pub address: Option<String>,
    #[serde(default)]
    pub notes: Option<String>,
}

impl PersonBody {
    /// The input to save. `stored` is the person's current address list
    /// (empty on create); it only matters for a pre-#155 body.
    pub fn resolve(self, stored: &[ContactPoint]) -> PersonInput {
        let addresses = match self.addresses {
            Some(list) => list,
            None => {
                let mut list = stored.to_vec();
                if let Some(old) = legacy_address(self.address_label, self.address)
                    && !list.iter().any(|a| a.value.trim() == old.value.trim())
                {
                    let preferred = list.is_empty();
                    list.push(ContactPoint { preferred, ..old });
                }
                list
            }
        };
        PersonInput {
            display_name: self.display_name,
            company: self.company,
            position: self.position,
            emails: self.emails,
            phones: self.phones,
            addresses,
            notes: self.notes,
        }
    }

    /// Whether this body came from a pre-#155 client, so the caller needs
    /// the stored addresses to [`resolve`](Self::resolve) it.
    pub fn is_legacy(&self) -> bool {
        self.addresses.is_none()
    }
}

/// A saved person as the editor's starting values (edit opens on these).
impl From<&Person> for PersonInput {
    fn from(p: &Person) -> Self {
        PersonInput {
            display_name: p.display_name.clone(),
            company: p.company.clone(),
            position: p.position.clone(),
            emails: p.emails.clone(),
            phones: p.phones.clone(),
            addresses: p.addresses.clone(),
            notes: p.notes.clone(),
        }
    }
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
        addresses: contact_points("addresses", input.addresses, MAX_ADDRESS, |_| true)?,
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

/// A multi-line field (notes): line breaks are fine.
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
        if field == "addresses" {
            // An address spans lines; it's freeform text, never parsed.
            too_long(field, &value, max_value)?;
        } else {
            check_line(field, &value, max_value)?;
        }
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

impl MembershipInput {
    /// A membership from what was typed in an attach flow (#162): blank
    /// fields mean none, so leaving the Role and Note empty attaches
    /// exactly as before. The API still validates (`normalize_membership`).
    pub fn from_typed(person_id: PersonId, role: &str, note: &str) -> Self {
        let filled = |s: &str| {
            let t = s.trim();
            (!t.is_empty()).then(|| t.to_string())
        };
        Self {
            person_id,
            role: filled(role),
            note: filled(note),
        }
    }
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
    /// Owner only: company, position, emails, phones, addresses, notes.
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

/// The contact point to show first: the preferred one, else the first.
pub fn primary(points: &[ContactPoint]) -> Option<&ContactPoint> {
    points
        .iter()
        .find(|p| p.preferred)
        .or_else(|| points.first())
}

/// How many entries of one kind a member card shows before "+n more" (#172).
pub const CARD_CONTACTS: usize = 3;

/// The entries a member card shows, preferred first then in stored order,
/// capped at `cap`, and how many were left out. The card says "+n more"
/// for those, so a list is never silently cut short (#172).
pub fn card_contacts(points: &[ContactPoint], cap: usize) -> (Vec<ContactPoint>, usize) {
    let ordered = points
        .iter()
        .filter(|p| p.preferred)
        .chain(points.iter().filter(|p| !p.preferred));
    let shown: Vec<ContactPoint> = ordered.take(cap).cloned().collect();
    let hidden = points.len().saturating_sub(shown.len());
    (shown, hidden)
}

/// Whether `p` matches the picker's text: name, company, any email or phone,
/// case-insensitively. Blank text matches everyone.
pub fn matches(p: &Person, q: &str) -> bool {
    let q = q.trim().to_lowercase();
    if q.is_empty() {
        return true;
    }
    let hit = |s: &str| s.to_lowercase().contains(&q);
    hit(&p.display_name)
        || p.company.as_deref().is_some_and(hit)
        || p.emails.iter().chain(&p.phones).any(|c| hit(&c.value))
}

/// Which contact list a [`ContactPoint`] belongs to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ContactKind {
    Email,
    Phone,
    Address,
}

/// Starter labels offered for a contact entry (#183), most common first.
///
/// Suggestions only: the label stays free text and is saved as typed, so
/// a custom label ("Ana's line") or another spelling is never rewritten.
pub fn label_suggestions(kind: ContactKind) -> &'static [&'static str] {
    match kind {
        ContactKind::Email => &["work", "personal"],
        ContactKind::Phone => &["mobile", "office", "home"],
        ContactKind::Address => &["office", "home"],
    }
}

/// A person-form field the browser could take for the user's own details:
/// one its address profile (name, company, email, phone, street) can fill.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PersonField {
    Name,
    Company,
    Contact(ContactKind),
}

/// The `autocomplete` value for a person-form field (#184).
///
/// These fields describe someone else, so the owner's own details must
/// never be offered. Chrome ignores `autocomplete="off"` on fields it
/// takes for identity fields: the email row leaked on v2.43.1, the name on
/// v2.44.0. Chrome does skip a token it doesn't recognise, as the email
/// row proved on v2.44.0, so each such field gets its own. A token must
/// stay one word that is not a standard autofill name. Fields with no
/// profile type (position, notes) keep `off`.
pub fn autofill_token(field: PersonField) -> &'static str {
    match field {
        PersonField::Name => "ember-person-name",
        PersonField::Company => "ember-person-company",
        PersonField::Contact(ContactKind::Email) => "ember-contact-email",
        PersonField::Contact(ContactKind::Phone) => "ember-contact-phone",
        PersonField::Contact(ContactKind::Address) => "ember-contact-address",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const ALL_FIELDS: [PersonField; 5] = [
        PersonField::Name,
        PersonField::Company,
        PersonField::Contact(ContactKind::Email),
        PersonField::Contact(ContactKind::Phone),
        PersonField::Contact(ContactKind::Address),
    ];

    /// The HTML autofill field names and modifiers (WHATWG "autofill
    /// detail tokens"). A token from this list would turn autofill ON.
    const STANDARD_AUTOFILL: &[&str] = &[
        "on",
        "off",
        "name",
        "honorific-prefix",
        "given-name",
        "additional-name",
        "family-name",
        "honorific-suffix",
        "nickname",
        "username",
        "new-password",
        "current-password",
        "one-time-code",
        "organization-title",
        "organization",
        "street-address",
        "address-line1",
        "address-line2",
        "address-line3",
        "address-level4",
        "address-level3",
        "address-level2",
        "address-level1",
        "country",
        "country-name",
        "postal-code",
        "cc-name",
        "cc-given-name",
        "cc-additional-name",
        "cc-family-name",
        "cc-number",
        "cc-exp",
        "cc-exp-month",
        "cc-exp-year",
        "cc-csc",
        "cc-type",
        "transaction-currency",
        "transaction-amount",
        "language",
        "bday",
        "bday-day",
        "bday-month",
        "bday-year",
        "sex",
        "url",
        "photo",
        "tel",
        "tel-country-code",
        "tel-national",
        "tel-area-code",
        "tel-local",
        "tel-local-prefix",
        "tel-local-suffix",
        "tel-extension",
        "email",
        "impp",
        "webauthn",
        "shipping",
        "billing",
        "home",
        "work",
        "mobile",
        "fax",
        "pager",
    ];

    #[test]
    fn autofill_tokens_are_ones_the_browser_does_not_recognise() {
        for field in ALL_FIELDS {
            let t = autofill_token(field);
            assert!(
                !t.is_empty() && !t.contains(char::is_whitespace),
                "{field:?}: one word, or the browser parses it as several tokens"
            );
            assert!(
                !STANDARD_AUTOFILL.contains(&t) && !t.starts_with("section-"),
                "{field:?}: {t:?} is a standard autofill name and would turn autofill on"
            );
        }
    }

    #[test]
    fn autofill_tokens_are_distinct_per_field() {
        let mut tokens: Vec<_> = ALL_FIELDS.iter().map(|f| autofill_token(*f)).collect();
        tokens.sort_unstable();
        tokens.dedup();
        assert_eq!(tokens.len(), ALL_FIELDS.len());
    }

    #[test]
    fn email_keeps_the_token_v2_44_0_shipped() {
        // The real-Chrome re-test passed with this exact value (#184).
        assert_eq!(
            autofill_token(PersonField::Contact(ContactKind::Email)),
            "ember-contact-email"
        );
    }

    #[test]
    fn label_suggestions_per_kind() {
        assert_eq!(label_suggestions(ContactKind::Email), ["work", "personal"]);
        assert_eq!(
            label_suggestions(ContactKind::Phone),
            ["mobile", "office", "home"]
        );
        assert_eq!(label_suggestions(ContactKind::Address), ["office", "home"]);
    }

    #[test]
    fn label_suggestions_fit_the_label_limit() {
        for kind in [ContactKind::Email, ContactKind::Phone, ContactKind::Address] {
            assert!(
                label_suggestions(kind)
                    .iter()
                    .all(|l| !l.is_empty() && l.chars().count() <= MAX_LABEL)
            );
        }
    }

    #[test]
    fn label_suggestions_filter_as_typed_and_never_echo_the_typed_text() {
        let phone = label_suggestions(ContactKind::Phone);
        // Typing narrows the list; the typed text itself is not offered back.
        assert_eq!(
            crate::picker::free_text_suggestions("o", phone, 8),
            ["office", "mobile", "home"]
        );
        assert!(crate::picker::free_text_suggestions("home", phone, 8).is_empty());
        // A custom label is kept verbatim: nothing is committed unless chosen.
        let options = crate::picker::free_text_suggestions("Off", phone, 8);
        assert_eq!(options, ["office"]);
        assert_eq!(
            crate::picker::free_text_commit("Off", None, &options),
            "Off"
        );
    }

    #[test]
    fn membership_from_typed_blank_is_todays_attach() {
        let id = PersonId::new();
        let m = MembershipInput::from_typed(id, "  ", "\n\t");
        assert_eq!(
            m,
            MembershipInput {
                person_id: id,
                role: None,
                note: None,
            }
        );
    }

    #[test]
    fn membership_from_typed_trims_role_and_note() {
        let id = PersonId::new();
        let m = MembershipInput::from_typed(id, " Sponsor ", " met at kickoff\n");
        assert_eq!(m.role.as_deref(), Some("Sponsor"));
        assert_eq!(m.note.as_deref(), Some("met at kickoff"));
    }

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
            notes: Some("Prefers email.\nMet at the conference.".into()),
            ..named("Ada")
        })
        .unwrap();
        assert_eq!(
            p.notes.as_deref(),
            Some("Prefers email.\nMet at the conference.")
        );
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
                addresses: vec![cp("home", "1 Main St\nSpringfield", true)],
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

    #[test]
    fn the_primary_contact_is_the_preferred_one_else_the_first() {
        let a = cp("work", "a@x.test", false);
        let b = cp("home", "b@x.test", true);
        assert_eq!(primary(&[a.clone(), b.clone()]), Some(&b));
        assert_eq!(primary(std::slice::from_ref(&a)), Some(&a));
        assert_eq!(primary(&[]), None);
    }

    #[test]
    fn a_card_shows_every_entry_preferred_first() {
        let office = cp("Office", "+1 555 0100", false);
        let cell = cp("Cell", "+1 555 0101", true);
        let (shown, hidden) = card_contacts(&[office.clone(), cell.clone()], CARD_CONTACTS);
        assert_eq!(shown, vec![cell, office]);
        assert_eq!(hidden, 0);
    }

    #[test]
    fn a_card_caps_the_list_and_counts_the_rest() {
        let points: Vec<_> = (0..5)
            .map(|i| cp("", &format!("p{i}@x.test"), i == 4))
            .collect();
        let (shown, hidden) = card_contacts(&points, 3);
        let values: Vec<_> = shown.iter().map(|p| p.value.as_str()).collect();
        assert_eq!(values, ["p4@x.test", "p0@x.test", "p1@x.test"]);
        assert_eq!(hidden, 2);
        assert_eq!(card_contacts(&[], 3), (Vec::new(), 0));
        assert_eq!(card_contacts(&points, 0), (Vec::new(), 5));
    }

    #[test]
    fn the_picker_matches_name_company_email_and_phone() {
        let p = membership("o").person;
        for q in ["ada", "ANALYTICAL", "example.com", ""] {
            assert!(matches(&p, q), "{q}");
        }
        assert!(!matches(&p, "babbage"));
    }

    #[test]
    fn addresses_are_a_list_like_emails_and_phones() {
        let p = normalize(PersonInput {
            addresses: vec![
                cp(" home ", " 1 Main St\nSpringfield ", true),
                cp("", "", false), // an untouched "add" row
                cp("office", "2 Elm St", false),
                cp("office", "3 Oak Ave\r\nSuite 4", false),
            ],
            ..named("Ada")
        })
        .unwrap();
        assert_eq!(
            p.addresses,
            vec![
                cp("home", "1 Main St\nSpringfield", true),
                cp("office", "2 Elm St", false),
                cp("office", "3 Oak Ave\r\nSuite 4", false),
            ]
        );
        // Zero addresses is fine.
        assert_eq!(normalize(named("Ada")).unwrap().addresses, vec![]);
    }

    #[test]
    fn address_entries_follow_the_list_rules() {
        let with = |addresses| {
            normalize(PersonInput {
                addresses,
                ..named("Ada")
            })
        };
        assert_eq!(
            with(vec![cp("a", "1", true), cp("b", "2", true)]),
            Err(PersonError::MultiplePreferred { field: "addresses" })
        );
        assert_eq!(
            with(vec![cp("home", " \n ", false)]),
            Err(PersonError::EmptyValue { field: "addresses" })
        );
        assert_eq!(
            with(vec![cp("home\nwork", "1 Main St", false)]),
            Err(PersonError::ControlChar { field: "addresses" })
        );
        assert_eq!(
            with(vec![cp("", &"x".repeat(MAX_ADDRESS + 1), false)]),
            Err(PersonError::TooLong {
                field: "addresses",
                max: MAX_ADDRESS
            })
        );
        assert!(with(vec![cp("", &"é".repeat(MAX_ADDRESS), false)]).is_ok());
        let many = (0..=MAX_CONTACT_POINTS)
            .map(|i| cp("", &format!("{i} Main St"), false))
            .collect();
        assert_eq!(with(many), Err(PersonError::TooMany { field: "addresses" }));
    }

    #[test]
    fn a_pre_155_backup_restores_its_single_address() {
        let mut v = serde_json::to_value(membership("o").person).unwrap();
        let obj = v.as_object_mut().unwrap();
        obj.remove("addresses");
        obj.insert("address_label".into(), "office".into());
        obj.insert("address".into(), "2 Elm St".into());
        let p: Person = serde_json::from_value(v.clone()).unwrap();
        assert_eq!(p.addresses, vec![cp("office", "2 Elm St", true)]);

        // No address at all: an empty list.
        let obj = v.as_object_mut().unwrap();
        obj.insert("address_label".into(), serde_json::Value::Null);
        obj.insert("address".into(), serde_json::Value::Null);
        let p: Person = serde_json::from_value(v).unwrap();
        assert_eq!(p.addresses, vec![]);
    }

    #[test]
    fn a_person_round_trips_through_json() {
        let p = membership("o").person;
        let json = serde_json::to_string(&p).unwrap();
        assert!(!json.contains("address_label"), "{json}");
        assert_eq!(serde_json::from_str::<Person>(&json).unwrap(), p);
    }

    fn body(json: serde_json::Value) -> PersonBody {
        serde_json::from_value(json).unwrap()
    }

    #[test]
    fn a_current_body_replaces_the_address_list() {
        let b = body(serde_json::json!({
            "display_name": "Ada",
            "addresses": [{"label": "home", "value": "1 Main St", "preferred": true}],
        }));
        assert!(!b.is_legacy());
        let stored = [cp("office", "2 Elm St", true)];
        assert_eq!(
            b.resolve(&stored).addresses,
            vec![cp("home", "1 Main St", true)]
        );
        // An explicit empty list clears them.
        let b = body(serde_json::json!({"display_name": "Ada", "addresses": []}));
        assert_eq!(b.resolve(&stored).addresses, vec![]);
    }

    #[test]
    fn a_pre_155_body_never_wipes_the_stored_addresses() {
        let stored = [
            cp("home", "1 Main St", true),
            cp("office", "2 Elm St", false),
        ];
        // A stale tab that never saw the list sends no address at all.
        let b = body(serde_json::json!({"display_name": "Ada", "address": null}));
        assert!(b.is_legacy());
        assert_eq!(b.resolve(&stored).addresses, stored.to_vec());
        // Its old single address is added when it's new...
        let b = body(serde_json::json!({
            "display_name": "Ada", "address_label": "lab", "address": "3 Oak Ave",
        }));
        let mut want = stored.to_vec();
        want.push(cp("lab", "3 Oak Ave", false));
        assert_eq!(b.resolve(&stored).addresses, want);
        // ...and not duplicated when it's already there.
        let b = body(serde_json::json!({"display_name": "Ada", "address": " 2 Elm St "}));
        assert_eq!(b.resolve(&stored).addresses, stored.to_vec());
        // On create (nothing stored) it becomes the preferred address.
        let b = body(serde_json::json!({"display_name": "Ada", "address": "3 Oak Ave"}));
        assert_eq!(b.resolve(&[]).addresses, vec![cp("", "3 Oak Ave", true)]);
    }

    #[test]
    fn a_person_round_trips_into_the_editors_input() {
        let p = membership("o").person;
        let input = PersonInput::from(&p);
        assert_eq!(input.display_name, p.display_name);
        assert_eq!(
            (input.company, input.notes),
            (p.company.clone(), p.notes.clone())
        );
        assert_eq!(input.emails, p.emails);
        // And it's already normal: saving it unchanged changes nothing.
        assert_eq!(normalize(PersonInput::from(&p)), Ok(PersonInput::from(&p)));
    }
}
