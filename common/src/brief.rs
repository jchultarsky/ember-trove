//! The Project Brief (#282): where a project stands *now*.
//!
//! A Project carries at most one current Brief: an optional one-line
//! headline, a pitch (the current elevator speech, markdown) and a short
//! list of points (blockers, next moves, watch items). A Brief is part of
//! the shared node: viewers read it, editors write it.
//!
//! Saving does not version a Brief in place. The owner's rule (#282,
//! decision 2): a save by the **same author within the coalesce window**
//! updates the current Brief; any later save, or a save by someone else,
//! **supersedes** it, and the old Brief becomes a History entry
//! (`brief_superseded`, its text in the entry's metadata). History stays
//! the one chronology (ONTOLOGY §10); there is no second versioning store.
//!
//! Everything here is pure, so the rule and the limits are unit-tested
//! once and shared by the API and the UI.

use chrono::{DateTime, TimeDelta, Utc};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

use crate::{activity::EDIT_COALESCE_MINUTES, id::NodeId};

/// Longest headline, in characters: one short line for the dashboard.
pub const MAX_HEADLINE: usize = 200;
/// Longest pitch, in characters: a paragraph, not a document.
pub const MAX_PITCH: usize = 4_000;
/// Most points a Brief may hold.
pub const MAX_POINTS: usize = 20;
/// Longest single point, in characters.
pub const MAX_POINT: usize = 500;
/// A Brief older than this many days shows a stale marker by default
/// (display only: nothing is hidden or blocked, ONTOLOGY §12).
pub const STALE_AFTER_DAYS: i64 = 14;

/// A project's current Brief, as the API returns it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct ProjectBrief {
    pub node_id: NodeId,
    /// One line; the dashboard's one-glance summary.
    pub headline: Option<String>,
    /// The elevator speech, markdown. May be empty when points carry it.
    pub pitch: String,
    /// What matters now, in order. Each is one line of markdown.
    pub points: Vec<String>,
    /// Cognito sub of whoever last saved it.
    pub author_id: String,
    /// When this Brief became current (a supersede resets it). The
    /// coalesce window is measured from here, like the edit-activity rule.
    pub stated_at: DateTime<Utc>,
    /// The last save; the age the UI shows and the optimistic-concurrency
    /// version (`expected_updated_at`).
    pub updated_at: DateTime<Utc>,
}

/// `PUT /nodes/{id}/brief` body.
#[derive(Debug, Clone, Default, Serialize, Deserialize, ToSchema)]
pub struct BriefInput {
    #[serde(default)]
    pub headline: Option<String>,
    #[serde(default)]
    pub pitch: String,
    #[serde(default)]
    pub points: Vec<String>,
    /// The `updated_at` the editor started from. When set and the stored
    /// Brief has moved on (or is gone), the save is refused with 412
    /// instead of overwriting someone else's words. `None` with a Brief
    /// already present is also refused: a client must say which version it
    /// edited (a first Brief sends `None`).
    #[serde(default)]
    pub expected_updated_at: Option<DateTime<Utc>>,
}

/// A validated, tidied Brief's content.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct BriefFields {
    pub headline: Option<String>,
    pub pitch: String,
    pub points: Vec<String>,
}

/// Why a Brief was refused. The message is the API's 422 text; it never
/// echoes the user's content.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum BriefError {
    #[error("a brief needs a headline, a pitch or at least one point")]
    Empty,
    #[error("the headline is longer than {MAX_HEADLINE} characters")]
    HeadlineTooLong,
    #[error("the headline must be a single line")]
    HeadlineMultiline,
    #[error("the pitch is longer than {MAX_PITCH} characters")]
    PitchTooLong,
    #[error("a brief holds at most {MAX_POINTS} points")]
    TooManyPoints,
    #[error("a point is longer than {MAX_POINT} characters")]
    PointTooLong,
    #[error("each point must be a single line")]
    PointMultiline,
}

fn has_line_break(s: &str) -> bool {
    s.contains(['\n', '\r', '\u{2028}', '\u{2029}'])
}

/// Validate and tidy a Brief: trim every part, drop a blank headline and
/// blank points. A Brief with nothing left is [`BriefError::Empty`]
/// (removing a Brief is its own action, not an empty save).
pub fn normalize(input: &BriefInput) -> Result<BriefFields, BriefError> {
    let headline = match input.headline.as_deref().map(str::trim) {
        None | Some("") => None,
        Some(h) if has_line_break(h) => return Err(BriefError::HeadlineMultiline),
        Some(h) if h.chars().count() > MAX_HEADLINE => return Err(BriefError::HeadlineTooLong),
        Some(h) => Some(h.to_string()),
    };
    let pitch = input.pitch.trim();
    if pitch.chars().count() > MAX_PITCH {
        return Err(BriefError::PitchTooLong);
    }
    let mut points = Vec::new();
    for p in &input.points {
        let p = p.trim();
        if p.is_empty() {
            continue;
        }
        if has_line_break(p) {
            return Err(BriefError::PointMultiline);
        }
        if p.chars().count() > MAX_POINT {
            return Err(BriefError::PointTooLong);
        }
        points.push(p.to_string());
    }
    if points.len() > MAX_POINTS {
        return Err(BriefError::TooManyPoints);
    }
    if headline.is_none() && pitch.is_empty() && points.is_empty() {
        return Err(BriefError::Empty);
    }
    Ok(BriefFields {
        headline,
        pitch: pitch.to_string(),
        points,
    })
}

impl ProjectBrief {
    /// This Brief's content, for comparing with a save.
    #[must_use]
    pub fn fields(&self) -> BriefFields {
        BriefFields {
            headline: self.headline.clone(),
            pitch: self.pitch.clone(),
            points: self.points.clone(),
        }
    }

    /// Whether the Brief is older than `stale_after_days` at `now`
    /// (display only).
    #[must_use]
    pub fn is_stale(&self, now: DateTime<Utc>, stale_after_days: i64) -> bool {
        now - self.updated_at >= TimeDelta::days(stale_after_days)
    }
}

/// What a save does to the current Brief.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BriefSave {
    /// No Brief yet: this one becomes current.
    Create,
    /// Same author, inside the coalesce window: update in place.
    InPlace,
    /// The current Brief goes to History and this one replaces it.
    Supersede,
    /// Same content as the current Brief: nothing to write.
    Unchanged,
}

/// The owner's supersede rule (#282, decision 2). The window is measured
/// from when the current Brief was stated, not its last save, so a steady
/// trickle of edits can't keep one Brief open forever (the same shape as
/// the edit-activity rule in `api/src/routes/nodes.rs`).
#[must_use]
pub fn save_mode(
    current: Option<&ProjectBrief>,
    new: &BriefFields,
    subject: &str,
    now: DateTime<Utc>,
) -> BriefSave {
    let Some(cur) = current else {
        return BriefSave::Create;
    };
    if &cur.fields() == new {
        return BriefSave::Unchanged;
    }
    if cur.author_id == subject && now - cur.stated_at < TimeDelta::minutes(EDIT_COALESCE_MINUTES) {
        BriefSave::InPlace
    } else {
        BriefSave::Supersede
    }
}

/// The metadata of the `brief_superseded` History entry: the old Brief's
/// words and when it was stated. Its author is named by sub (an account
/// purge rewrites a sub found in metadata to the deleted-account marker);
/// no email or name is stored.
#[must_use]
pub fn superseded_metadata(old: &ProjectBrief) -> serde_json::Value {
    serde_json::json!({
        "headline": old.headline,
        "pitch": old.pitch,
        "points": old.points,
        "author_id": old.author_id,
        "stated_at": old.stated_at,
        "updated_at": old.updated_at,
    })
}

/// How History names a superseded Brief: its headline, else the start of
/// its pitch, else its first point, after "was:" (or "removed, was:"), so
/// the old words are not read as the new ones. `None` when the metadata
/// holds none.
#[must_use]
pub fn superseded_summary(metadata: &serde_json::Value) -> Option<String> {
    let removed = metadata.get("removed").and_then(serde_json::Value::as_bool) == Some(true);
    let lead = if removed { "removed, was: " } else { "was: " };
    superseded_words(metadata).map(|words| format!("{lead}{words}"))
}

/// The replaced Brief's own words, cut to one short line.
fn superseded_words(metadata: &serde_json::Value) -> Option<String> {
    const MAX: usize = 120;
    let text = |k: &str| {
        metadata
            .get(k)
            .and_then(|v| v.as_str())
            .map(str::trim)
            .filter(|s| !s.is_empty())
    };
    let first_point = metadata
        .get("points")
        .and_then(|v| v.as_array())
        .and_then(|a| a.first())
        .and_then(|v| v.as_str());
    let s = text("headline").or_else(|| text("pitch")).or(first_point)?;
    let line = s.lines().next().unwrap_or(s).trim();
    if line.chars().count() > MAX {
        Some(format!("{}\u{2026}", truncate(line, MAX - 1)))
    } else {
        Some(line.to_string())
    }
}

/// A Brief built from a body's `## Status` section (#282 §6, the one-click
/// "Move Status section into Brief"). `section` is the section's markdown
/// without its heading (`markdown::extract_section`). List items become
/// points and the remaining prose becomes the pitch; the headline is left
/// for the person to write. Over-long input is cut to the limits rather
/// than refused, since the person reviews the result before saving.
#[must_use]
pub fn from_status_section(section: &str) -> BriefFields {
    let (pitch, mut points) = split_status_section(section);
    points.truncate(MAX_POINTS);
    BriefFields {
        headline: None,
        pitch: truncate(&pitch, MAX_PITCH),
        points: points.iter().map(|p| truncate(p, MAX_POINT)).collect(),
    }
}

/// A status section's prose and its list items, uncut.
fn split_status_section(section: &str) -> (String, Vec<String>) {
    let mut pitch_lines: Vec<&str> = Vec::new();
    let mut points: Vec<String> = Vec::new();
    for line in section.lines() {
        let t = line.trim();
        if let Some(item) = list_item(t) {
            if !item.is_empty() {
                points.push(item.to_string());
            }
        } else {
            pitch_lines.push(line.trim_end());
        }
    }
    // Collapse runs of blank lines left where the list was.
    let mut pitch = String::new();
    let mut blank_run = false;
    for l in pitch_lines {
        if l.trim().is_empty() {
            blank_run = !pitch.is_empty();
            continue;
        }
        if blank_run {
            pitch.push_str("\n\n");
        } else if !pitch.is_empty() {
            pitch.push('\n');
        }
        blank_run = false;
        pitch.push_str(l);
    }
    (pitch.trim().to_string(), points)
}

/// A body's Status section taken out of it (#282 §6): the Brief it makes,
/// and the body without the section.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StatusMove {
    pub fields: BriefFields,
    pub body: String,
}

/// "Move Status section into Brief": the Brief a body's Status section
/// makes, and the body without that section (one fact, one home).
///
/// It rewrites what a person wrote, so it is conservative, like
/// `project_status::split_from_body`. `None`, and nothing is offered,
/// unless all of these hold:
/// - a heading's text is exactly "Status" or "Project Status" (any level,
///   any case; "Status Update" doesn't count);
/// - there is something under it;
/// - **all of it fits a Brief**. A section that [`from_status_section`]
///   would have to cut stays where it is: removing it would lose the rest.
///
/// The section runs to the next heading of the same or a higher level.
/// Everything outside it is kept byte for byte, line endings included.
#[must_use]
pub fn move_status_section(body: &str) -> Option<StatusMove> {
    use crate::markdown::atx_heading_level;
    let lines: Vec<&str> = body.split_inclusive('\n').collect();
    let (start, level) = lines.iter().enumerate().find_map(|(i, l)| {
        let t = l.trim();
        let level = atx_heading_level(t)?;
        let text = t[level..]
            .trim()
            .trim_end_matches('#')
            .trim()
            .to_lowercase();
        (text == "status" || text == "project status").then_some((i, level))
    })?;
    let end = (start + 1..lines.len())
        .find(|&i| atx_heading_level(lines[i].trim()).is_some_and(|l| l <= level))
        .unwrap_or(lines.len());

    let (pitch, points) = split_status_section(&lines[start + 1..end].concat());
    // What the section holds must be what the Brief will hold.
    let fields = normalize(&BriefInput {
        headline: None,
        pitch,
        points,
        expected_updated_at: None,
    })
    .ok()?;

    let mut out = lines[..start].concat();
    if end == lines.len() {
        // The section was the last thing: no blank lines are left dangling
        // where it stood.
        let kept = out.trim_end().len();
        let had_newline = out[kept..].contains('\n');
        out.truncate(kept);
        if had_newline && !out.is_empty() {
            out.push('\n');
        }
    } else {
        out.push_str(&lines[end..].concat());
    }
    Some(StatusMove { fields, body: out })
}

/// A Brief's points as the editor shows them: one per line.
#[must_use]
pub fn points_to_text(points: &[String]) -> String {
    points.join("\n")
}

/// The editor's text as points: one per line. A leading `- `, `* ` or `+ `
/// is dropped, since people type a list as a list. Blank lines go.
/// [`normalize`] still judges the result.
#[must_use]
pub fn points_from_text(text: &str) -> Vec<String> {
    text.lines()
        .map(|line| {
            let t = line.trim();
            ["- ", "* ", "+ "]
                .iter()
                .find_map(|m| t.strip_prefix(m))
                .unwrap_or(t)
                .trim()
        })
        .filter(|p| !p.is_empty())
        .map(str::to_string)
        .collect()
}

/// The text of a markdown list item (`- x`, `* x`, `+ x`, `1. x`, `1) x`,
/// with an optional task box), or `None` for any other line.
fn list_item(line: &str) -> Option<&str> {
    let rest = ["- ", "* ", "+ "]
        .iter()
        .find_map(|m| line.strip_prefix(m))
        .or_else(|| {
            let digits = line.chars().take_while(char::is_ascii_digit).count();
            (digits > 0)
                .then(|| &line[digits..])
                .and_then(|r| r.strip_prefix(". ").or_else(|| r.strip_prefix(") ")))
        })?;
    let rest = rest.trim();
    let rest = ["[ ] ", "[x] ", "[X] "]
        .iter()
        .find_map(|b| rest.strip_prefix(b))
        .unwrap_or(rest);
    Some(rest.trim())
}

fn truncate(s: &str, max: usize) -> String {
    s.chars().take(max).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use uuid::Uuid;

    fn input(headline: Option<&str>, pitch: &str, points: &[&str]) -> BriefInput {
        BriefInput {
            headline: headline.map(str::to_string),
            pitch: pitch.to_string(),
            points: points.iter().map(|p| (*p).to_string()).collect(),
            expected_updated_at: None,
        }
    }

    fn brief(author: &str, stated_mins_ago: i64, now: DateTime<Utc>) -> ProjectBrief {
        ProjectBrief {
            node_id: NodeId(Uuid::nil()),
            headline: Some("Waiting on vendor".into()),
            pitch: "We build the thing.".into(),
            points: vec!["Security review".into()],
            author_id: author.into(),
            stated_at: now - TimeDelta::minutes(stated_mins_ago),
            updated_at: now - TimeDelta::minutes(1),
        }
    }

    #[test]
    fn normalize_trims_and_drops_blanks() {
        let f = normalize(&input(Some("  "), "  pitch  ", &[" a ", "", "  "])).unwrap();
        assert_eq!(
            f,
            BriefFields {
                headline: None,
                pitch: "pitch".into(),
                points: vec!["a".into()],
            }
        );
    }

    #[test]
    fn an_empty_brief_is_refused() {
        assert_eq!(
            normalize(&input(Some(" "), "\n", &["", " "])),
            Err(BriefError::Empty)
        );
    }

    #[test]
    fn any_one_part_is_enough() {
        assert!(normalize(&input(Some("h"), "", &[])).is_ok());
        assert!(normalize(&input(None, "p", &[])).is_ok());
        assert!(normalize(&input(None, "", &["x"])).is_ok());
    }

    #[test]
    fn limits_are_enforced_in_characters() {
        let at = "é".repeat(MAX_HEADLINE);
        assert!(normalize(&input(Some(&at), "", &[])).is_ok());
        let over = "é".repeat(MAX_HEADLINE + 1);
        assert_eq!(
            normalize(&input(Some(&over), "", &[])),
            Err(BriefError::HeadlineTooLong)
        );
        assert_eq!(
            normalize(&input(None, &"p".repeat(MAX_PITCH + 1), &[])),
            Err(BriefError::PitchTooLong)
        );
        let long = "x".repeat(MAX_POINT + 1);
        assert_eq!(
            normalize(&input(None, "", &[&long])),
            Err(BriefError::PointTooLong)
        );
        let many = vec!["x"; MAX_POINTS + 1];
        assert_eq!(
            normalize(&input(None, "", &many)),
            Err(BriefError::TooManyPoints)
        );
        // Blank points don't count toward the limit.
        let mut padded = vec!["x"; MAX_POINTS];
        padded.extend(["", " "]);
        assert!(normalize(&input(None, "", &padded)).is_ok());
    }

    #[test]
    fn headline_and_points_are_single_lines() {
        assert_eq!(
            normalize(&input(Some("a\nb"), "", &[])),
            Err(BriefError::HeadlineMultiline)
        );
        assert_eq!(
            normalize(&input(None, "", &["a\r\nb"])),
            Err(BriefError::PointMultiline)
        );
        // The pitch is a paragraph and may wrap.
        assert!(normalize(&input(None, "a\nb", &[])).is_ok());
    }

    #[test]
    fn the_error_text_never_echoes_content() {
        let secret = "sekrit-content";
        let e = normalize(&input(Some(&format!("{secret}\nmore")), "", &[])).unwrap_err();
        assert!(!e.to_string().contains(secret));
    }

    #[test]
    fn a_first_save_creates() {
        let now = Utc::now();
        let f = normalize(&input(None, "p", &[])).unwrap();
        assert_eq!(save_mode(None, &f, "a", now), BriefSave::Create);
    }

    #[test]
    fn same_author_inside_the_window_updates_in_place() {
        let now = Utc::now();
        let cur = brief("a", EDIT_COALESCE_MINUTES - 1, now);
        let f = normalize(&input(None, "typo fixed", &[])).unwrap();
        assert_eq!(save_mode(Some(&cur), &f, "a", now), BriefSave::InPlace);
    }

    #[test]
    fn the_window_is_measured_from_when_the_brief_was_stated() {
        let now = Utc::now();
        // Saved a minute ago, but stated a whole window ago: supersede.
        let cur = brief("a", EDIT_COALESCE_MINUTES, now);
        let f = normalize(&input(None, "new story", &[])).unwrap();
        assert_eq!(save_mode(Some(&cur), &f, "a", now), BriefSave::Supersede);
    }

    #[test]
    fn another_author_always_supersedes() {
        let now = Utc::now();
        let cur = brief("a", 0, now);
        let f = normalize(&input(None, "their take", &[])).unwrap();
        assert_eq!(save_mode(Some(&cur), &f, "b", now), BriefSave::Supersede);
    }

    #[test]
    fn an_identical_save_writes_nothing() {
        let now = Utc::now();
        let cur = brief("a", 120, now);
        assert_eq!(
            save_mode(Some(&cur), &cur.fields(), "b", now),
            BriefSave::Unchanged
        );
    }

    #[test]
    fn staleness_is_by_last_save() {
        let now = Utc::now();
        let mut b = brief("a", 0, now);
        assert!(!b.is_stale(now, STALE_AFTER_DAYS));
        b.updated_at = now - TimeDelta::days(STALE_AFTER_DAYS);
        assert!(b.is_stale(now, STALE_AFTER_DAYS));
    }

    #[test]
    fn superseded_metadata_carries_the_words_and_no_contact_details() {
        let now = Utc::now();
        let b = brief("sub-a", 0, now);
        let m = superseded_metadata(&b);
        assert_eq!(m["pitch"], "We build the thing.");
        assert_eq!(m["points"][0], "Security review");
        assert_eq!(m["author_id"], "sub-a");
        assert!(m.get("actor_email").is_none());
    }

    #[test]
    fn history_names_a_superseded_brief_by_headline_then_pitch() {
        let now = Utc::now();
        let mut b = brief("a", 0, now);
        assert_eq!(
            superseded_summary(&superseded_metadata(&b)).as_deref(),
            Some("was: Waiting on vendor")
        );
        // A removed Brief says so: the line must not read as a new Brief.
        let mut removed = superseded_metadata(&b);
        removed["removed"] = serde_json::Value::Bool(true);
        assert_eq!(
            superseded_summary(&removed).as_deref(),
            Some("removed, was: Waiting on vendor")
        );
        b.headline = None;
        b.pitch = format!("{}\nsecond line", "p".repeat(200));
        let s = superseded_summary(&superseded_metadata(&b)).unwrap();
        let words = s.strip_prefix("was: ").unwrap();
        assert_eq!(words.chars().count(), 120);
        assert!(words.ends_with('\u{2026}'));
        b.pitch = String::new();
        assert_eq!(
            superseded_summary(&superseded_metadata(&b)).as_deref(),
            Some("was: Security review")
        );
        assert_eq!(superseded_summary(&serde_json::json!({})), None);
    }

    #[test]
    fn a_status_section_splits_into_pitch_and_points() {
        let section = "We are building the importer.\nIt matters for Q4.\n\n\
                       - Vendor review pending\n* [ ] Draft the plan\n1. Ship beta\n\n\
                       Next check-in Friday.";
        let f = from_status_section(section);
        assert_eq!(f.headline, None);
        assert_eq!(
            f.pitch,
            "We are building the importer.\nIt matters for Q4.\n\nNext check-in Friday."
        );
        assert_eq!(
            f.points,
            vec!["Vendor review pending", "Draft the plan", "Ship beta"]
        );
        // The result is a valid Brief as it stands.
        let back = normalize(&BriefInput {
            headline: f.headline.clone(),
            pitch: f.pitch.clone(),
            points: f.points.clone(),
            expected_updated_at: None,
        });
        assert_eq!(back, Ok(f));
    }

    #[test]
    fn a_status_section_is_cut_to_the_limits() {
        let mut s = String::new();
        for i in 0..MAX_POINTS + 5 {
            s.push_str(&format!("- point {i}\n"));
        }
        s.push_str(&format!("- {}\n", "x".repeat(MAX_POINT + 10)));
        let f = from_status_section(&s);
        assert_eq!(f.points.len(), MAX_POINTS);
        assert!(f.points.iter().all(|p| p.chars().count() <= MAX_POINT));
        let f = from_status_section(&"y".repeat(MAX_PITCH + 10));
        assert_eq!(f.pitch.chars().count(), MAX_PITCH);
    }

    #[test]
    fn plain_dashes_and_numbers_in_prose_are_not_points() {
        let f = from_status_section("-not a list\n2024 was slow\n3.5 people");
        assert!(f.points.is_empty());
        assert_eq!(f.pitch, "-not a list\n2024 was slow\n3.5 people");
    }

    // ── Moving the body's Status section into the Brief (#282 §6) ───────

    #[test]
    fn a_status_section_moves_out_of_the_body_whole() {
        let body = "# Plan\n\nGoals stay.\n\n## Status\n\nWaiting on the vendor.\n\n\
                    - Review booked\n- [ ] Send the scope\n\n### Risks\nNone yet.\n\n\
                    ## Background\n\nKept.\n";
        let moved = move_status_section(body).expect("a Status section");
        assert_eq!(moved.fields.headline, None);
        assert_eq!(
            moved.fields.pitch,
            "Waiting on the vendor.\n\n### Risks\nNone yet."
        );
        assert_eq!(moved.fields.points, ["Review booked", "Send the scope"]);
        assert_eq!(
            moved.body,
            "# Plan\n\nGoals stay.\n\n## Background\n\nKept.\n"
        );
    }

    #[test]
    fn a_status_section_at_the_end_leaves_no_dangling_blank_lines() {
        let moved = move_status_section("Intro.\n\n## Project Status\n- one\n").expect("moves");
        assert_eq!(moved.fields.points, ["one"]);
        assert_eq!(moved.body, "Intro.\n");
        // The whole body was the section.
        let moved = move_status_section("## status\nAll of it.").expect("moves");
        assert_eq!(moved.fields.pitch, "All of it.");
        assert_eq!(moved.body, "");
        // CRLF outside the section is kept as written.
        let moved =
            move_status_section("A\r\n\r\n## Status\r\n- x\r\n\r\n## B\r\nc\r\n").expect("moves");
        assert_eq!(moved.body, "A\r\n\r\n## B\r\nc\r\n");
    }

    #[test]
    fn a_status_section_is_left_alone_when_moving_it_would_lose_words() {
        // No such heading, a near miss, or nothing under it.
        assert_eq!(move_status_section("# Plan\n\nText."), None);
        assert_eq!(move_status_section("## Status Update\n\nText."), None);
        assert_eq!(move_status_section("## Status\n\n## Next\n\nText."), None);
        // More than a Brief holds: it would be cut, so it stays in the body.
        let many: String = (0..=MAX_POINTS).map(|i| format!("- point {i}\n")).collect();
        assert_eq!(move_status_section(&format!("## Status\n{many}")), None);
        let long_point = format!("## Status\n- {}\n", "x".repeat(MAX_POINT + 1));
        assert_eq!(move_status_section(&long_point), None);
        let long_pitch = format!("## Status\n{}\n", "x".repeat(MAX_PITCH + 1));
        assert_eq!(move_status_section(&long_pitch), None);
        // The lenient parser still cuts, for a person to review.
        assert_eq!(
            from_status_section(&many).points.len(),
            MAX_POINTS,
            "from_status_section keeps cutting"
        );
    }

    #[test]
    fn the_editors_points_are_one_per_line() {
        let points =
            points_from_text("- first\n\n  * second  \n+ third\nfourth\n   \n2. kept as typed");
        assert_eq!(
            points,
            ["first", "second", "third", "fourth", "2. kept as typed"]
        );
        assert_eq!(
            points_to_text(&points),
            "first\nsecond\nthird\nfourth\n2. kept as typed"
        );
        assert_eq!(points_from_text(""), Vec::<String>::new());
    }
}
