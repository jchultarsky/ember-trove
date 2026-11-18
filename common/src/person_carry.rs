//! Carrying what was typed for a new person over to the person who already
//! has the address (S8b, #357; ONTOLOGY §7.1).
//!
//! A save that would give a second person an email address is refused. The
//! person typing then has details in hand (a company, phones, notes) and a
//! record that already exists. [`carry_into`] adds what was typed to that
//! record **for review**: nothing here saves anything. The editor opens on
//! the result, marks what was added, says what was not used, and the person
//! presses Save or Discard.
//!
//! The rule in one line: **theirs is kept, blanks are filled, lists grow.**
//! It never removes or replaces a value the existing person has.

use std::collections::HashSet;

use serde::{Deserialize, Serialize};

use crate::id::PersonId;
use crate::person::{
    ContactKind, ContactPoint, DuplicateMatch, MAX_CONTACT_POINTS, MAX_NOTES, PersonInput,
    email_key,
};

/// What a dialer reads of a phone number: its digits and a `+`. Two
/// spellings of one number ("+1 (555) 010-0000", "+1 555 010 0000") have
/// the same key. With no digit in it, the text itself, so it still
/// compares.
#[must_use]
pub fn phone_key(value: &str) -> String {
    let dialled: String = value
        .chars()
        .filter(|c| c.is_ascii_digit() || *c == '+')
        .collect();
    if dialled.chars().any(|c| c.is_ascii_digit()) {
        dialled
    } else {
        value.trim().to_lowercase()
    }
}

/// When two entries of one contact list are the same entry: an email by
/// [`email_key`] (trimmed first, as `normalize` stores it), a phone by
/// [`phone_key`], a postal address with its white space collapsed and its
/// case ignored (a pasted address differs in line breaks).
#[must_use]
pub fn contact_key(kind: ContactKind, value: &str) -> String {
    match kind {
        ContactKind::Email => email_key(value.trim()),
        ContactKind::Phone => phone_key(value),
        ContactKind::Address => value
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" ")
            .to_lowercase(),
    }
}

/// One of the caller's people named by a refused save, with the addresses
/// the refusal named them for (their spelling).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TakenPerson {
    pub id: PersonId,
    pub display_name: String,
    pub emails: Vec<String>,
}

/// A 409's rows as people. The API answers one row per person and address,
/// so a person who has two of the typed addresses, or one address twice
/// from before the rule, is named more than once: here they are one entry,
/// in the order first named.
#[must_use]
pub fn people_named(matches: &[DuplicateMatch]) -> Vec<TakenPerson> {
    let mut people: Vec<TakenPerson> = Vec::new();
    for m in matches {
        match people.iter_mut().find(|p| p.id == m.id) {
            Some(p) => {
                if !p.emails.contains(&m.email) {
                    p.emails.push(m.email.clone());
                }
            }
            None => people.push(TakenPerson {
                id: m.id,
                display_name: m.display_name.clone(),
                emails: vec![m.email.clone()],
            }),
        }
    }
    people
}

/// A field of a person, for saying what a carry-over did to it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CarriedField {
    Name,
    Company,
    Position,
    Emails,
    Phones,
    Addresses,
    Notes,
}

impl CarriedField {
    /// The list a contact kind is.
    #[must_use]
    pub fn of(kind: ContactKind) -> Self {
        match kind {
            ContactKind::Email => Self::Emails,
            ContactKind::Phone => Self::Phones,
            ContactKind::Address => Self::Addresses,
        }
    }

    /// The field as a sentence names it.
    #[must_use]
    pub fn noun(self) -> &'static str {
        match self {
            Self::Name => "name",
            Self::Company => "company",
            Self::Position => "position",
            Self::Emails => "emails",
            Self::Phones => "phones",
            Self::Addresses => "addresses",
            Self::Notes => "notes",
        }
    }
}

/// A typed value that was not used because the existing person has
/// another: said in the review, never written anywhere. It is also how a
/// wrong match shows ("you typed Acme; theirs is Initech").
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NotUsed {
    pub field: CarriedField,
    pub typed: String,
}

/// What a carry-over did, for the review: what to mark, and what to say.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct CarryReport {
    /// The name typed for the new person.
    #[serde(default)]
    pub typed_name: String,
    /// Values added to each list.
    #[serde(default)]
    pub emails: Vec<String>,
    #[serde(default)]
    pub phones: Vec<String>,
    #[serde(default)]
    pub addresses: Vec<String>,
    /// Blank fields that were filled, and notes that were added.
    #[serde(default)]
    pub filled: Vec<CarriedField>,
    /// Typed values that differ from theirs: theirs is kept.
    #[serde(default)]
    pub not_used: Vec<NotUsed>,
    /// Typed addresses left out because they belong to someone else.
    #[serde(default)]
    pub someone_elses: Vec<String>,
    /// Fields now over their limit: the save will be refused until the
    /// person removes something. Added anyway: leaving it out would lose
    /// it without a word.
    #[serde(default)]
    pub over: Vec<CarriedField>,
}

impl CarryReport {
    /// Whether anything was added at all. If not, everything typed is
    /// already on them (or was not theirs to take).
    #[must_use]
    pub fn added_nothing(&self) -> bool {
        self.emails.is_empty()
            && self.phones.is_empty()
            && self.addresses.is_empty()
            && self.filled.is_empty()
    }

    /// The values added to the list of `kind`.
    #[must_use]
    pub fn added(&self, kind: ContactKind) -> &[String] {
        match kind {
            ContactKind::Email => &self.emails,
            ContactKind::Phone => &self.phones,
            ContactKind::Address => &self.addresses,
        }
    }

    /// Whether row `i` of the list of `kind` is one this carry-over added,
    /// judged by what the row holds now: change it and it is no longer
    /// "added".
    #[must_use]
    pub fn marks(&self, kind: ContactKind, rows: &[ContactPoint], i: usize) -> bool {
        let Some(key) = rows.get(i).map(|p| contact_key(kind, &p.value)) else {
            return false;
        };
        !key.is_empty() && self.added(kind).iter().any(|v| contact_key(kind, v) == key)
    }
}

/// The existing person with what was typed added, and what that did.
#[derive(Debug, Clone, PartialEq)]
pub struct CarryOver {
    pub person: PersonInput,
    pub report: CarryReport,
}

fn blank(value: Option<&str>) -> bool {
    value.is_none_or(|v| v.trim().is_empty())
}

fn same_text(a: &str, b: &str) -> bool {
    a.trim().to_lowercase() == b.trim().to_lowercase()
}

/// A scalar field: theirs is kept; a blank is filled; a typed value that
/// differs is reported.
fn carry_text(
    field: CarriedField,
    theirs: &mut Option<String>,
    typed: Option<&str>,
    report: &mut CarryReport,
) {
    let Some(typed) = typed.map(str::trim).filter(|t| !t.is_empty()) else {
        return;
    };
    match theirs.as_deref() {
        Some(have) if !have.trim().is_empty() => {
            if !same_text(have, typed) {
                report.not_used.push(NotUsed {
                    field,
                    typed: typed.to_string(),
                });
            }
        }
        _ => {
            *theirs = Some(typed.to_string());
            report.filled.push(field);
        }
    }
}

/// A contact list: theirs first and untouched, then the typed entries they
/// don't have. Returns the values added.
fn carry_list(
    kind: ContactKind,
    theirs: &mut Vec<ContactPoint>,
    typed: &[ContactPoint],
    someone_elses: &HashSet<String>,
    report: &mut CarryReport,
) -> Vec<String> {
    let had_none = theirs.is_empty();
    let mut keys: HashSet<String> = theirs.iter().map(|p| contact_key(kind, &p.value)).collect();
    let mut added = Vec::new();
    for entry in typed {
        let value = entry.value.trim();
        let key = contact_key(kind, value);
        // A row nobody typed in, or an entry they have already.
        if value.is_empty() || key.is_empty() || keys.contains(&key) {
            continue;
        }
        // Another person's address is not theirs to be given. (One they
        // list themselves was caught above: people who shared an address
        // before the rule keep it.)
        if kind == ContactKind::Email && someone_elses.contains(&key) {
            report.someone_elses.push(value.to_string());
            continue;
        }
        keys.insert(key);
        theirs.push(ContactPoint {
            label: entry.label.trim().to_string(),
            value: value.to_string(),
            // Their preferred entry stays preferred. With no entry of
            // their own, the typed choice stands (one at most).
            preferred: had_none && entry.preferred && !theirs.iter().any(|p| p.preferred),
        });
        added.push(value.to_string());
    }
    if theirs.len() > MAX_CONTACT_POINTS {
        report.over.push(CarriedField::of(kind));
    }
    added
}

/// The non-blank lines of a text, trimmed, for comparing notes.
fn lines_of(text: &str) -> Vec<&str> {
    text.lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .collect()
}

/// Notes: typed notes they already have, line for line, add nothing; if
/// the typed notes hold all of theirs, the typed notes are the result;
/// otherwise the typed notes follow theirs after a blank line. Compared by
/// whole lines: "ok" is not already there because the notes say "book".
fn carry_notes(theirs: &mut Option<String>, typed: Option<&str>, report: &mut CarryReport) {
    let Some(typed) = typed.map(str::trim).filter(|t| !t.is_empty()) else {
        return;
    };
    let result = match theirs.as_deref().map(str::trim).filter(|t| !t.is_empty()) {
        None => typed.to_string(),
        Some(have) => {
            let (have_lines, typed_lines) = (lines_of(have), lines_of(typed));
            if typed_lines.iter().all(|l| have_lines.contains(l)) {
                return;
            }
            if have_lines.iter().all(|l| typed_lines.contains(l)) {
                typed.to_string()
            } else {
                format!("{have}\n\n{typed}")
            }
        }
    };
    if result.chars().count() > MAX_NOTES {
        report.over.push(CarriedField::Notes);
    }
    *theirs = Some(result);
    report.filled.push(CarriedField::Notes);
}

/// `theirs` with what was `typed` for a new person added, for review.
///
/// - Name, company, position: theirs is kept. A blank company or position
///   is filled. A typed value that differs is reported, not used.
/// - Emails, phones, addresses: theirs, then the typed entries they don't
///   have ([`contact_key`]). An added entry is never preferred over one
///   of theirs.
/// - `someone_elses`: addresses the refusal named **another** person for.
///   They are left out and reported, unless this person lists them too.
/// - Notes: see [`carry_notes`].
/// - Over a limit, the value is added all the same and reported: the
///   API's own refusal then holds the save until something is removed.
///
/// Carrying the same input again adds nothing.
#[must_use]
pub fn carry_into(
    theirs: &PersonInput,
    typed: &PersonInput,
    someone_elses: &[String],
) -> CarryOver {
    let mut person = theirs.clone();
    let mut report = CarryReport {
        typed_name: typed.display_name.trim().to_string(),
        ..CarryReport::default()
    };
    if !report.typed_name.is_empty() && !same_text(&report.typed_name, &theirs.display_name) {
        report.not_used.push(NotUsed {
            field: CarriedField::Name,
            typed: report.typed_name.clone(),
        });
    }
    // A blank stored as `Some("")` counts as blank.
    if blank(person.company.as_deref()) {
        person.company = None;
    }
    if blank(person.position.as_deref()) {
        person.position = None;
    }
    carry_text(
        CarriedField::Company,
        &mut person.company,
        typed.company.as_deref(),
        &mut report,
    );
    carry_text(
        CarriedField::Position,
        &mut person.position,
        typed.position.as_deref(),
        &mut report,
    );
    let elses: HashSet<String> = someone_elses
        .iter()
        .map(|v| contact_key(ContactKind::Email, v))
        .collect();
    report.emails = carry_list(
        ContactKind::Email,
        &mut person.emails,
        &typed.emails,
        &elses,
        &mut report,
    );
    report.phones = carry_list(
        ContactKind::Phone,
        &mut person.phones,
        &typed.phones,
        &elses,
        &mut report,
    );
    report.addresses = carry_list(
        ContactKind::Address,
        &mut person.addresses,
        &typed.addresses,
        &elses,
        &mut report,
    );
    carry_notes(&mut person.notes, typed.notes.as_deref(), &mut report);
    CarryOver { person, report }
}

fn counted(n: usize, one: &str, many: &str) -> Option<String> {
    match n {
        0 => None,
        1 => Some(format!("1 {one}")),
        n => Some(format!("{n} {many}")),
    }
}

/// What the review says about a carry-over, a sentence a line: what was
/// added, what was typed and not used, what was left out, and what is now
/// over a limit. The typed values quoted are the reader's own.
#[must_use]
pub fn review_lines(report: &CarryReport) -> Vec<String> {
    let filled = |f: CarriedField| report.filled.contains(&f);
    let added: Vec<String> = [
        filled(CarriedField::Company).then(|| "company".to_string()),
        filled(CarriedField::Position).then(|| "position".to_string()),
        counted(report.emails.len(), "email", "emails"),
        counted(report.phones.len(), "phone", "phones"),
        counted(report.addresses.len(), "address", "addresses"),
        filled(CarriedField::Notes).then(|| "notes".to_string()),
    ]
    .into_iter()
    .flatten()
    .collect();
    let mut lines = Vec::new();
    lines.push(if added.is_empty() {
        "Nothing to add: they already have the rest of what you typed.".to_string()
    } else {
        format!("Added: {}.", added.join(", "))
    });
    for n in &report.not_used {
        lines.push(format!(
            "You typed the {} \u{201c}{}\u{201d}; theirs is kept.",
            n.field.noun(),
            n.typed
        ));
    }
    if !report.someone_elses.is_empty() {
        lines.push(format!(
            "Left out, because {} to someone else: {}.",
            if report.someone_elses.len() == 1 {
                "it belongs"
            } else {
                "they belong"
            },
            report.someone_elses.join(", ")
        ));
    }
    for field in &report.over {
        lines.push(match field {
            CarriedField::Notes => {
                "The notes are now too long. Shorten them before saving.".to_string()
            }
            list => format!(
                "There are now more than {MAX_CONTACT_POINTS} {}. Remove some before saving.",
                list.noun()
            ),
        });
    }
    lines
}

/// A fingerprint of what a person dialog holds, to tell later whether a
/// stored draft is still the one something was carried from. Not a
/// security boundary: an equality check, stable across builds.
#[must_use]
pub fn person_fingerprint(person: &PersonInput) -> u64 {
    // The JSON of a `PersonInput` has a fixed field order.
    let json = serde_json::to_string(person).unwrap_or_default();
    crate::save_watch::fingerprint(&json, "", "")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::person::normalize;

    fn cp(label: &str, value: &str, preferred: bool) -> ContactPoint {
        ContactPoint {
            label: label.into(),
            value: value.into(),
            preferred,
        }
    }

    fn ada() -> PersonInput {
        PersonInput {
            display_name: "Ada Lovelace".into(),
            company: Some("Analytical Engines".into()),
            position: None,
            emails: vec![cp("work", "ada@example.com", true)],
            phones: vec![cp("mobile", "+1 (555) 010-0000", true)],
            addresses: vec![cp("home", "1 Main St\nSpringfield", false)],
            notes: Some("Met at the fair.\nLikes tea.".into()),
        }
    }

    fn typed() -> PersonInput {
        PersonInput {
            display_name: "A. Lovelace".into(),
            company: Some("Engines Ltd".into()),
            position: Some("Analyst".into()),
            emails: vec![
                cp("", " ADA@example.com", true),
                cp("home", "ada@home.example", false),
            ],
            phones: vec![
                cp("cell", "+1 555 010 0000", true),
                cp("office", "+1 555 010 9999", false),
            ],
            addresses: vec![
                cp("", "1 main st   springfield", false),
                cp("office", "2 High St", true),
            ],
            notes: Some("Likes tea.\nPrefers email.".into()),
        }
    }

    #[test]
    fn theirs_is_kept_blanks_are_filled_and_lists_grow() {
        let CarryOver { person, report } = carry_into(&ada(), &typed(), &[]);
        // Theirs, untouched and first.
        assert_eq!(person.display_name, "Ada Lovelace");
        assert_eq!(person.company.as_deref(), Some("Analytical Engines"));
        assert_eq!(person.emails[0], cp("work", "ada@example.com", true));
        assert_eq!(person.phones[0], cp("mobile", "+1 (555) 010-0000", true));
        assert_eq!(person.addresses[0], ada().addresses[0]);
        // A blank filled.
        assert_eq!(person.position.as_deref(), Some("Analyst"));
        // Lists grow by what they don't have: the same address in another
        // case, the same number in another format and the same street on
        // one line are not new.
        assert_eq!(person.emails[1..], [cp("home", "ada@home.example", false)]);
        assert_eq!(person.phones[1..], [cp("office", "+1 555 010 9999", false)]);
        assert_eq!(person.addresses[1..], [cp("office", "2 High St", false)]);
        // Notes: theirs, then the typed ones, since neither holds the other.
        assert_eq!(
            person.notes.as_deref(),
            Some("Met at the fair.\nLikes tea.\n\nLikes tea.\nPrefers email.")
        );

        assert_eq!(report.typed_name, "A. Lovelace");
        assert_eq!(report.emails, ["ada@home.example"]);
        assert_eq!(report.phones, ["+1 555 010 9999"]);
        assert_eq!(report.addresses, ["2 High St"]);
        assert_eq!(report.filled, [CarriedField::Position, CarriedField::Notes]);
        // What was typed and differs is said, never used.
        assert_eq!(
            report.not_used,
            [
                NotUsed {
                    field: CarriedField::Name,
                    typed: "A. Lovelace".into()
                },
                NotUsed {
                    field: CarriedField::Company,
                    typed: "Engines Ltd".into()
                },
            ]
        );
        assert!(report.over.is_empty() && report.someone_elses.is_empty());
        assert!(!report.added_nothing());
        // What was built passes the API's own check.
        assert!(normalize(person).is_ok());
    }

    #[test]
    fn an_added_entry_is_never_preferred_over_one_of_theirs() {
        let CarryOver { person, .. } = carry_into(&ada(), &typed(), &[]);
        for list in [&person.emails, &person.phones, &person.addresses] {
            assert!(list[1..].iter().all(|p| !p.preferred), "{list:?}");
        }
        // Their address list had no preferred entry: it still has none.
        assert!(person.addresses.iter().all(|p| !p.preferred));
        // With no entry of their own, the typed choice stands, once.
        let bare = PersonInput {
            display_name: "Ada".into(),
            ..Default::default()
        };
        let two_preferred = PersonInput {
            phones: vec![
                cp("", "111", true),
                cp("", "222", true),
                cp("", "333", false),
            ],
            ..typed()
        };
        let CarryOver { person, .. } = carry_into(&bare, &two_preferred, &[]);
        assert_eq!(
            person
                .phones
                .iter()
                .map(|p| p.preferred)
                .collect::<Vec<_>>(),
            [true, false, false]
        );
        assert_eq!(
            person
                .addresses
                .iter()
                .map(|p| p.preferred)
                .collect::<Vec<_>>(),
            [false, true]
        );
    }

    #[test]
    fn carrying_the_same_input_again_adds_nothing() {
        let first = carry_into(&ada(), &typed(), &[]);
        let again = carry_into(&first.person, &typed(), &[]);
        assert_eq!(again.person, first.person);
        assert!(again.report.added_nothing(), "{:?}", again.report);
        // What differs is still said.
        assert_eq!(again.report.not_used.len(), 2);
    }

    #[test]
    fn nothing_typed_beyond_the_name_and_the_address_adds_nothing() {
        let only = PersonInput {
            display_name: "Ada Lovelace".into(),
            emails: vec![cp("", "Ada@Example.com", false), cp("", "  ", false)],
            ..Default::default()
        };
        let CarryOver { person, report } = carry_into(&ada(), &only, &[]);
        assert_eq!(person, ada());
        assert!(report.added_nothing());
        assert!(report.not_used.is_empty(), "the same name is no difference");
    }

    #[test]
    fn notes_are_compared_by_whole_lines() {
        let with_notes = |notes: &str| PersonInput {
            display_name: "x".into(),
            notes: Some(notes.into()),
            ..Default::default()
        };
        let theirs = with_notes("Read the book.\nCall back.");
        // "ok" is inside "book", and is not there.
        let got = carry_into(&theirs, &with_notes("ok"), &[]);
        assert_eq!(
            got.person.notes.as_deref(),
            Some("Read the book.\nCall back.\n\nok")
        );
        // Lines they have, in any order and spacing: nothing to add.
        let got = carry_into(&theirs, &with_notes("  Call back. \n\nRead the book."), &[]);
        assert_eq!(got.person.notes, theirs.notes);
        assert!(got.report.added_nothing());
        // The typed notes hold all of theirs: the typed notes it is.
        let longer = "Read the book.\nCall back.\nAsk about the garden.";
        let got = carry_into(&theirs, &with_notes(longer), &[]);
        assert_eq!(got.person.notes.as_deref(), Some(longer));
        assert_eq!(got.report.filled, [CarriedField::Notes]);
        // They had none.
        let got = carry_into(&with_notes("  "), &with_notes("First note."), &[]);
        assert_eq!(got.person.notes.as_deref(), Some("First note."));
    }

    #[test]
    fn another_persons_address_is_left_out_and_said() {
        let also = PersonInput {
            emails: vec![
                cp("", "ada@example.com", false),
                cp("", "Bea@example.com", false),
                cp("", "new@example.com", false),
            ],
            ..typed()
        };
        let CarryOver { person, report } = carry_into(&ada(), &also, &["bea@example.com".into()]);
        let values: Vec<&str> = person.emails.iter().map(|p| p.value.as_str()).collect();
        assert_eq!(values, ["ada@example.com", "new@example.com"]);
        assert_eq!(report.someone_elses, ["Bea@example.com"]);
        // An address shared from before the rule is theirs as well: it is
        // never called someone else's, whoever else the refusal named.
        let CarryOver { person, report } = carry_into(
            &ada(),
            &also,
            &["ada@example.com".into(), "bea@example.com".into()],
        );
        assert_eq!(person.emails[0].value, "ada@example.com");
        assert_eq!(report.someone_elses, ["Bea@example.com"]);
    }

    #[test]
    fn over_a_limit_it_is_added_and_said() {
        let full = PersonInput {
            emails: (0..MAX_CONTACT_POINTS)
                .map(|i| cp("", &format!("a{i}@example.com"), i == 0))
                .collect(),
            notes: Some("x".repeat(MAX_NOTES - 3)),
            ..ada()
        };
        let more = PersonInput {
            emails: vec![cp("", "eleventh@example.com", false)],
            notes: Some("four".into()),
            ..PersonInput::default()
        };
        let CarryOver { person, report } = carry_into(&full, &more, &[]);
        assert_eq!(person.emails.len(), MAX_CONTACT_POINTS + 1);
        assert_eq!(report.emails, ["eleventh@example.com"]);
        assert!(person.notes.as_deref().is_some_and(|n| n.ends_with("four")));
        assert_eq!(report.over, [CarriedField::Emails, CarriedField::Notes]);
        // The API refuses it, in words, until something is removed.
        assert!(normalize(person).is_err());
    }

    #[test]
    fn the_review_says_what_was_added_what_was_not_used_and_what_was_left_out() {
        let report = carry_into(&ada(), &typed(), &[]).report;
        assert_eq!(
            review_lines(&report),
            [
                "Added: position, 1 email, 1 phone, 1 address, notes.",
                "You typed the name \u{201c}A. Lovelace\u{201d}; theirs is kept.",
                "You typed the company \u{201c}Engines Ltd\u{201d}; theirs is kept.",
            ]
        );
        // Nothing added, two left out, two over.
        let report = CarryReport {
            someone_elses: vec!["bea@example.com".into(), "cy@example.com".into()],
            over: vec![CarriedField::Emails, CarriedField::Notes],
            ..CarryReport::default()
        };
        assert_eq!(
            review_lines(&report),
            [
                "Nothing to add: they already have the rest of what you typed.",
                "Left out, because they belong to someone else: bea@example.com, cy@example.com.",
                "There are now more than 10 emails. Remove some before saving.",
                "The notes are now too long. Shorten them before saving.",
            ]
        );
        let one = CarryReport {
            emails: vec!["a@example.com".into(), "b@example.com".into()],
            someone_elses: vec!["bea@example.com".into()],
            ..CarryReport::default()
        };
        assert_eq!(
            review_lines(&one),
            [
                "Added: 2 emails.",
                "Left out, because it belongs to someone else: bea@example.com.",
            ]
        );
    }

    #[test]
    fn a_phone_is_one_number_however_it_is_spelled() {
        assert_eq!(phone_key("+1 (555) 010-0000"), "+15550100000");
        assert_eq!(phone_key("+1 555 010 0000"), phone_key("+1.555.010.0000"));
        // Without the plus it may be another number: not the same.
        assert_ne!(phone_key("1 555 010 0000"), phone_key("+1 555 010 0000"));
        // No digit at all: the text, so it still compares.
        assert_eq!(phone_key(" Ask Reception "), "ask reception");
        assert_eq!(
            contact_key(ContactKind::Address, "1 Main St\r\n  Springfield"),
            "1 main st springfield"
        );
        assert_eq!(
            contact_key(ContactKind::Email, "Ada@Example.com\u{a0}"),
            "ada@example.com"
        );
    }

    #[test]
    fn a_person_named_twice_by_a_refusal_is_one_entry() {
        let (a, b) = (PersonId::new(), PersonId::new());
        let m = |id: PersonId, name: &str, email: &str| DuplicateMatch {
            id,
            display_name: name.into(),
            email: email.into(),
        };
        let people = people_named(&[
            m(a, "Ada", "ada@example.com"),
            m(b, "Bea", "bea@example.com"),
            m(a, "Ada", "ada@home.example"),
            m(a, "Ada", "ada@example.com"),
        ]);
        assert_eq!(people.len(), 2);
        assert_eq!(people[0].id, a);
        assert_eq!(people[0].emails, ["ada@example.com", "ada@home.example"]);
        assert_eq!(people[1].emails, ["bea@example.com"]);
        assert!(people_named(&[]).is_empty());
    }

    #[test]
    fn a_row_is_marked_added_only_while_it_holds_what_was_added() {
        let CarryOver { person, report } = carry_into(&ada(), &typed(), &[]);
        assert!(
            !report.marks(ContactKind::Email, &person.emails, 0),
            "theirs"
        );
        assert!(report.marks(ContactKind::Email, &person.emails, 1));
        assert!(report.marks(ContactKind::Phone, &person.phones, 1));
        assert!(
            !report.marks(ContactKind::Phone, &person.emails, 1),
            "by kind"
        );
        assert!(!report.marks(ContactKind::Email, &person.emails, 9));
        // Changed in review: no longer what was added.
        let mut edited = person.emails.clone();
        edited[1].value = "other@example.com".into();
        assert!(!report.marks(ContactKind::Email, &edited, 1));
        // The same value in another spelling still is.
        edited[1].value = " ADA@home.example ".into();
        assert!(report.marks(ContactKind::Email, &edited, 1));
        assert!(!CarryReport::default().marks(ContactKind::Email, &person.emails, 1));
    }

    #[test]
    fn the_fingerprint_tells_one_dialogs_input_from_another() {
        let a = typed();
        let mut b = typed();
        assert_eq!(person_fingerprint(&a), person_fingerprint(&b));
        b.notes = Some("something else".into());
        assert_ne!(person_fingerprint(&a), person_fingerprint(&b));
        assert_ne!(
            person_fingerprint(&a),
            person_fingerprint(&PersonInput::default())
        );
    }

    #[test]
    fn a_report_reads_back_from_storage_and_an_older_one_too() {
        let report = carry_into(&ada(), &typed(), &["x@example.com".into()]).report;
        let json = serde_json::to_string(&report).expect("serialize");
        assert_eq!(
            serde_json::from_str::<CarryReport>(&json).expect("parse"),
            report
        );
        // Fields a later version adds must not break a stored one.
        assert_eq!(
            serde_json::from_str::<CarryReport>("{}").expect("empty"),
            CarryReport::default()
        );
    }
}
