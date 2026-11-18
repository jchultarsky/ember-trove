//! Mentions of people in rendered text (S9, #360; ONTOLOGY §7.3, §14).
//!
//! `common::mention` reads the `@[label](person:id)` syntax; the API
//! says who each mention may be named as (`MentionName`, which rides on
//! the text's own answer). This module turns the two into HTML, before
//! Markdown is parsed, in the same pass as wiki links.

use std::collections::HashMap;
use std::ops::Range;

use common::id::NodeId;
use common::mention::{Mention, MentionName, html_inert};

/// A mention as HTML.
///
/// - The reader's own person: a link to their page, showing their
///   current name.
/// - Someone else's person the reader may be told of: the current name
///   as plain text. No link, and no id in the markup: there is no way
///   into another account's People (§14).
/// - Anyone else (no such person, not really mentioned here, or their
///   owner can no longer open the node): the label as it was typed,
///   marked unresolved.
///
/// SECURITY: a name is written with every character that is not a
/// letter or a digit as a numeric entity (`html_inert`), so a name such
/// as `[x](https://e)` or `<b>` is neither Markdown nor markup.
fn mention_html(mention: &Mention<'_>, people: &[MentionName]) -> String {
    match people.iter().find(|p| p.person_id == mention.person_id) {
        Some(person) if person.mine => format!(
            r#"<a class="mention" href="/people/{}">{}</a>"#,
            person.person_id.0.hyphenated(),
            html_inert(&format!("@{}", person.display_name)),
        ),
        Some(person) => format!(
            r#"<span class="mention">{}</span>"#,
            html_inert(&format!("@{}", person.display_name)),
        ),
        None => format!(
            r#"<span class="mention-unresolved">{}</span>"#,
            html_inert(&format!("@{}", mention.label)),
        ),
    }
}

/// `content` with its wiki links (when `titles` is given) and its
/// mentions turned into HTML, ready for the Markdown parser.
///
/// One pass over the original text: each scanner reads what was
/// written, and neither reads the other's output. (Two passes would
/// have the second scan the first's HTML.) The mention scanner already
/// leaves wiki links and code alone, so the two sets of ranges never
/// overlap.
pub fn preprocess_links(
    content: &str,
    titles: Option<&HashMap<String, NodeId>>,
    people: &[MentionName],
) -> String {
    let mut pieces: Vec<(Range<usize>, String)> = common::mention::mentions(content)
        .iter()
        .map(|m| (m.range.clone(), mention_html(m, people)))
        .collect();
    if let Some(titles) = titles {
        pieces.extend(common::wikilink::wikilinks(content).iter().map(|link| {
            (
                link.range.clone(),
                crate::wikilink::wikilink_html(link, titles),
            )
        }));
    }
    pieces.sort_by_key(|(range, _)| range.start);
    let mut out = String::with_capacity(content.len());
    let mut at = 0;
    for (range, html) in pieces {
        // Overlap can't happen (see above); if it ever did, the later
        // piece is dropped rather than text being repeated.
        if range.start < at {
            continue;
        }
        out.push_str(content.get(at..range.start).unwrap_or_default());
        out.push_str(&html);
        at = range.end;
    }
    out.push_str(content.get(at..).unwrap_or_default());
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use common::id::PersonId;
    use common::mention::mention_token;
    use uuid::Uuid;

    fn ada() -> PersonId {
        PersonId(Uuid::from_u128(0xada))
    }

    fn named(name: &str, mine: bool) -> Vec<MentionName> {
        vec![MentionName {
            person_id: ada(),
            display_name: name.to_string(),
            mine,
        }]
    }

    #[test]
    fn a_mention_is_a_link_for_its_owner_and_a_name_for_anyone_else() {
        let text = format!("Met {}.", mention_token("Ada", ada()));
        // The current name, not the label.
        let own = preprocess_links(&text, None, &named("Ada King", true));
        assert_eq!(
            own,
            format!(
                r#"Met <a class="mention" href="/people/{}">&#64;Ada&#32;King</a>."#,
                ada().0.hyphenated()
            )
        );
        let others = preprocess_links(&text, None, &named("Ada King", false));
        assert_eq!(
            others,
            r#"Met <span class="mention">&#64;Ada&#32;King</span>."#
        );
        // SECURITY: nothing of the person but the name for another reader.
        assert!(!others.contains("href") && !others.contains(&ada().0.to_string()));
        // Nobody to name: the label as typed, unresolved.
        let nobody = preprocess_links(&text, None, &[]);
        assert_eq!(
            nobody,
            r#"Met <span class="mention-unresolved">&#64;Ada</span>."#
        );
    }

    #[test]
    fn a_name_is_never_markdown_or_markup() {
        let text = mention_token("x", ada());
        let hostile = "[click](https://evil.example) *b* <img src=x onerror=1> `c`";
        for people in [named(hostile, true), named(hostile, false)] {
            let html = crate::markdown::render_note(&text, &people);
            assert!(!html.contains("evil.example\""), "{html}");
            assert!(!html.contains("<img") && !html.contains("<em>"), "{html}");
            assert!(!html.contains("<code>"), "{html}");
            assert!(html.contains("evil"), "the name is still shown: {html}");
        }
        // A label too (it is the writer's own text, and still inert).
        let token = format!("@[<b>*x*</b>](person:{})", ada().0.hyphenated());
        let html = crate::markdown::render_note(&token, &[]);
        assert!(!html.contains("<b>") && !html.contains("<em>"), "{html}");
    }

    #[test]
    fn wiki_links_and_mentions_are_spliced_in_one_pass() {
        let node = NodeId(Uuid::from_u128(7));
        let titles: HashMap<String, NodeId> = [("plan".to_string(), node)].into();
        let text = format!(
            "[[Plan]] with {} and `{}` and [[Nowhere]]",
            mention_token("Ada", ada()),
            mention_token("Ada", ada())
        );
        let html = preprocess_links(&text, Some(&titles), &named("Ada", true));
        assert!(
            html.starts_with(r#"<a class="wikilink" data-node-id="#),
            "{html}"
        );
        assert_eq!(html.matches(r#"class="mention""#).count(), 1, "{html}");
        // Inside code it is left as written.
        assert!(html.contains(&format!("`{}`", mention_token("Ada", ada()))));
        assert!(html.contains(r#"<span class="wikilink-unresolved">Nowhere</span>"#));
        // Without titles, wiki links are left as text (notes).
        let html = preprocess_links(&text, None, &named("Ada", true));
        assert!(
            html.starts_with("[[Plan]] with <a class=\"mention\""),
            "{html}"
        );
    }

    #[test]
    fn the_rendered_mention_keeps_its_class_and_its_link() {
        let text = format!("Met {}.", mention_token("Ada", ada()));
        let own = crate::markdown::render_note(&text, &named("Ada", true));
        assert!(own.contains(r#"class="mention""#), "{own}");
        assert!(
            own.contains(&format!(r#"href="/people/{}""#, ada().0.hyphenated())),
            "{own}"
        );
        assert!(own.contains("@Ada"), "{own}");
        let other = crate::markdown::render_note(&text, &named("Ada", false));
        assert!(
            other.contains(r#"<span class="mention">@Ada</span>"#),
            "{other}"
        );
        let nobody = crate::markdown::render_note(&text, &[]);
        assert!(
            nobody.contains(r#"<span class="mention-unresolved">@Ada</span>"#),
            "{nobody}"
        );
        // A writer can't choose these classes for anything else, and no
        // other class comes through.
        let forged = crate::markdown::render_note(
            r#"<a class="mention fixed inset-0" href="/people/x">x</a> <span class="etl-root mention">y</span>"#,
            &[],
        );
        assert!(
            !forged.contains("fixed") && !forged.contains("etl-root"),
            "{forged}"
        );
    }

    /// Where no names are at hand (a Brief, a template, a task title),
    /// a token is a Markdown link with a scheme the sanitizer drops: it
    /// reads "@Ada", linked to nothing.
    #[test]
    fn a_token_that_is_not_resolved_reads_as_at_name_with_no_link() {
        let text = format!("Met {}.", mention_token("Ada", ada()));
        let html = crate::markdown::render_markdown_plain(&text);
        assert!(html.contains("@<a") && html.contains(">Ada</a>"), "{html}");
        assert!(
            !html.contains("person:") && !html.contains("href"),
            "{html}"
        );
    }
}
